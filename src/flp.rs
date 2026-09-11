//! Minimal FL Studio project (.flp) event-stream parser.
//!
//! An FLP file is a sequence of chunks/events:
//! - `"FLhd"` + u32 LE length + header payload
//! - `"FLdt"` + u32 LE length + event stream
//!
//! Each event in the stream starts with a one-byte event id whose value
//! determines the framing of the remainder:
//! - `0..64`   : byte event, 1 byte of data
//! - `64..128` : word event, 2 bytes LE
//! - `128..192`: dword event, 4 bytes LE
//! - `>=192`   : variable-length event: varint length (7 bits per byte,
//!   little-endian groups, high bit = continuation) then payload.

pub const EV_NEW_CHANNEL: u8 = 64;
pub const EV_TEXT_CHANNEL_NAME: u8 = 203; // 0xCB
pub const EV_TEXT_FX_TRACK_NAME: u8 = 204; // 0xCC
pub const EV_PLUGIN_PARAMS: u8 = 213; // 0xD5

/// A single parsed FLP event.
#[derive(Debug)]
pub struct Event<'a> {
    pub id: u8,
    pub data: &'a [u8],
}

#[derive(Debug)]
pub struct FlpError(pub String);

impl std::fmt::Display for FlpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn err<T>(msg: impl Into<String>) -> Result<T, FlpError> {
    Err(FlpError(msg.into()))
}

/// Parse the event stream out of an (uncompressed) FLP file.
pub fn parse_events(buf: &[u8]) -> Result<Vec<Event<'_>>, FlpError> {
    if buf.len() < 8 {
        return err("file is too small to be an FLP");
    }
    if &buf[0..2] == b"PK" {
        return err(
            "this FLP is a ZIP archive (zipped loop package); extract the .flp member first",
        );
    }
    if &buf[0..4] != b"FLhd" {
        return err("missing FLhd header (not an FLP file?)");
    }
    let hdrlen = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    let mut pos = 8 + hdrlen;
    if buf.len() < pos + 8 || &buf[pos..pos + 4] != b"FLdt" {
        return err("missing FLdt chunk");
    }
    let dtlen = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into().unwrap()) as usize;
    pos += 8;
    let end = (pos + dtlen).min(buf.len());

    let mut events = Vec::new();
    while pos < end {
        let ev_offset = pos;
        let id = buf[pos];
        pos += 1;
        let dlen: usize = match id {
            0..=63 => 1,
            64..=127 => 2,
            128..=191 => 4,
            _ => {
                let Some(v) = read_varint(buf, &mut pos) else {
                    return err(format!("truncated varint at offset {ev_offset:#x}"));
                };
                v as usize
            }
        };
        if pos + dlen > end {
            return err(format!(
                "event {id} at offset {ev_offset:#x} overruns the FLdt chunk"
            ));
        }
        events.push(Event {
            id,
            data: &buf[pos..pos + dlen],
        });
        pos += dlen;
    }
    Ok(events)
}

/// Read an FLP varint (little-endian 7-bit groups, MSB = continuation).
pub fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u32> {
    let mut value: u32 = 0;
    let mut shift = 0u32;
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        value |= ((b & 0x7f) as u32).checked_shl(shift)?;
        if b & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift > 28 {
            return None;
        }
    }
}

/// The chunks found inside a `PluginParams` (event 213) payload.
#[derive(Debug, Default)]
pub struct PluginParams<'a> {
    pub name: &'a [u8],
    pub filename: &'a [u8],
    pub vendor: &'a [u8],
    pub state: &'a [u8],
}

const PLUGIN_CHUNK_STATE: u32 = 53;
const PLUGIN_CHUNK_NAME: u32 = 54;
const PLUGIN_CHUNK_FILENAME: u32 = 55;
const PLUGIN_CHUNK_VENDOR: u32 = 56;

/// Parse the chunk sequence of a `PluginParams` event payload.
///
/// Layout: `u32 LE format version`, then a sequence of
/// `[u32 LE chunk id][u32 LE size lo][u32 LE size hi][data]` records.
/// Chunk ids 53..56 carry the plugin state / name / filename / vendor.
pub fn parse_plugin_params(data: &[u8]) -> Result<PluginParams<'_>, FlpError> {
    if data.len() < 4 {
        return err("PluginParams payload too small");
    }
    let version = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let mut out = PluginParams::default();
    if version < 5 {
        // Very old FL versions store a bare state blob.
        out.state = &data[4..];
        return Ok(out);
    }
    let mut pos = 4usize;
    while pos + 12 <= data.len() {
        let cid = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
        let sz_lo = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as u64;
        let sz_hi = u32::from_le_bytes(data[pos + 8..pos + 12].try_into().unwrap()) as u64;
        let sz = sz_lo | (sz_hi << 32);
        pos += 12;
        let Some(sz) = usize::try_from(sz).ok() else {
            return err("plugin chunk size overflows address space");
        };
        if pos + sz > data.len() {
            return err(format!(
                "plugin chunk {cid} (size {sz}) overruns PluginParams payload"
            ));
        }
        let payload = &data[pos..pos + sz];
        match cid {
            PLUGIN_CHUNK_STATE => out.state = payload,
            PLUGIN_CHUNK_NAME => out.name = payload,
            PLUGIN_CHUNK_FILENAME => out.filename = payload,
            PLUGIN_CHUNK_VENDOR => out.vendor = payload,
            _ => {}
        }
        pos += sz;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_flp(events: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut dt = Vec::new();
        for (id, data) in events {
            dt.push(*id);
            if *id >= 192 {
                // varint length
                let mut len = data.len() as u32;
                loop {
                    let b = (len & 0x7f) as u8;
                    len >>= 7;
                    if len == 0 {
                        dt.push(b);
                        break;
                    }
                    dt.push(b | 0x80);
                }
            } else {
                assert_eq!(
                    data.len(),
                    match id {
                        0..=63 => 1,
                        64..=127 => 2,
                        _ => 4,
                    }
                );
            }
            dt.extend_from_slice(data);
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"FLhd");
        out.extend_from_slice(&6u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0x46, 0, 0x60, 0]);
        out.extend_from_slice(b"FLdt");
        out.extend_from_slice(&(dt.len() as u32).to_le_bytes());
        out.extend_from_slice(&dt);
        out
    }

    #[test]
    fn parses_event_framing() {
        let flp = build_flp(&[
            (9, vec![1]),                        // byte event
            (64, vec![3, 0]),                    // word event
            (128, vec![0xAA, 0xBB, 0xCC, 0xDD]), // dword event
            (203, b"Channel 1\0".to_vec()),      // text event
        ]);
        let evs = parse_events(&flp).unwrap();
        assert_eq!(evs.len(), 4);
        assert_eq!(evs[0].id, 9);
        assert_eq!(evs[0].data, &[1]);
        assert_eq!(evs[1].data, &[3, 0]);
        assert_eq!(evs[2].data, &[0xAA, 0xBB, 0xCC, 0xDD]);
        assert_eq!(evs[3].id, 203);
        assert_eq!(evs[3].data, b"Channel 1\0");
    }

    #[test]
    fn parses_long_varint() {
        let mut flp_body = Vec::new();
        flp_body.push(199);
        flp_body.extend_from_slice(&[0xFF, 0xFF, 0x0F]); // varint 0x3FFFF
        flp_body.extend_from_slice(&vec![0u8; 0x3FFFF]);
        let mut flp = Vec::new();
        flp.extend_from_slice(b"FLhd");
        flp.extend_from_slice(&6u32.to_le_bytes());
        flp.extend_from_slice(&[0, 0, 0x46, 0, 0x60, 0]);
        flp.extend_from_slice(b"FLdt");
        flp.extend_from_slice(&(flp_body.len() as u32).to_le_bytes());
        flp.extend_from_slice(&flp_body);
        let evs = parse_events(&flp).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data.len(), 0x3FFFF);
    }

    #[test]
    fn rejects_zip() {
        assert!(parse_events(b"PK\x03\x04xxxxxxxx").is_err());
    }

    #[test]
    fn parses_plugin_params_chunks() {
        let mut data = Vec::new();
        data.extend_from_slice(&12u32.to_le_bytes()); // version
        let add = |d: &mut Vec<u8>, cid: u32, payload: &[u8]| {
            d.extend_from_slice(&cid.to_le_bytes());
            d.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            d.extend_from_slice(&0u32.to_le_bytes());
            d.extend_from_slice(payload);
        };
        add(&mut data, 54, b"Serum");
        add(&mut data, 55, b"/Library/Audio/Plug-Ins/VST3/Serum.vst3");
        add(&mut data, 53, &[0x78, 0x01, 0x00, 0x00, 0x00, 0x02]);
        let pp = parse_plugin_params(&data).unwrap();
        assert_eq!(pp.name, b"Serum");
        assert_eq!(pp.filename, b"/Library/Audio/Plug-Ins/VST3/Serum.vst3");
        assert_eq!(pp.state, &[0x78, 0x01, 0x00, 0x00, 0x00, 0x02]);
    }
}

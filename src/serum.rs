//! Serum plugin state handling: detecting Serum 1 instances and recovering
//! the Serum 1 preset chunk (the exact `chunk` region of a Serum `.fxp`).

use flate2::read::ZlibDecoder;

/// Size of the decompressed Serum 1 preset state for contemporary presets.
pub const SERUM1_STATE_SIZE: usize = 172_736;
/// Offset of the 32-byte preset name inside the decompressed state.
pub const OFF_PRESET_NAME: usize = 0x4972;
/// Offset of the f32 preset-format version inside the decompressed state.
pub const OFF_VERSION_F32: usize = 0x4994;
/// Offset of the 48-byte author string inside the decompressed state.
pub const OFF_AUTHOR: usize = 0x49A0;
/// Offset of the 48-byte category string inside the decompressed state.
pub const OFF_CATEGORY: usize = 0x49D0;

const MAX_STREAM_OUT: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct SerumError(pub String);

impl std::fmt::Display for SerumError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn err<T>(msg: impl Into<String>) -> Result<T, SerumError> {
    Err(SerumError(msg.into()))
}

/// Where the Serum 1 chunk was recovered from (for diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// FL Studio VST3 wrapper: `[prologue][chunks..., state = cid 3]`, the
    /// cid-3 payload being the zlib-stream preset chunk.
    FlVst3Wrapper,
    /// A bare zlib-stream chunk stored directly in the state event.
    RawZlib,
    /// A complete `CcnK`/`FPCh` preset embedded in the state.
    CcnKFxPreset,
    /// An `FPCh` preset inside FL's `VstW` VST2 wrapper.
    VstWFxPreset,
}

/// Metadata read from the decompressed 172,736-byte preset state.
#[derive(Debug, Clone, Default)]
pub struct PresetMeta {
    pub preset_name: String,
    pub author: String,
    pub category: String,
    pub version_f32: f32,
}

/// The recovered Serum 1 chunk plus parsed metadata.
#[derive(Debug)]
pub struct Serum1Chunk {
    /// Exact bytes of the fxp `chunk` region: one or more concatenated zlib
    /// streams followed by a `u32 LE` trailer with the compressed size of the
    /// first stream.
    pub chunk: Vec<u8>,
    pub source: SourceKind,
    pub meta: PresetMeta,
    /// Decompressed sizes of every zlib stream (stream 0 = the preset state,
    /// later streams = embedded wavetable / noise data).
    pub stream_sizes: Vec<usize>,
}

/// True when the plugin described by `name`/`filename` is Serum 1
/// (the synth or "Serum FX"), i.e. not Serum 2 and not something unrelated.
pub fn is_serum1(name: &[u8], filename: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name).trim().to_ascii_lowercase();
    let filename = String::from_utf8_lossy(filename)
        .trim()
        .to_ascii_lowercase();
    let basename = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim_end_matches(".vst3")
        .trim_end_matches(".vst")
        .trim_end_matches(".dll")
        .trim_end_matches(".vstpreset")
        .to_ascii_lowercase();

    if name == "serum2" || name.starts_with("serum 2") || basename == "serum2" {
        return false;
    }
    name == "serum"
        || name == "serum fx"
        || name == "serum_x64"
        || basename == "serum"
        || basename == "serum fx"
        || basename == "serum_x64"
}

/// True when the plugin is Serum 2 (reported separately, never extracted).
pub fn is_serum2(name: &[u8], filename: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name).trim().to_ascii_lowercase();
    let filename = String::from_utf8_lossy(filename)
        .trim()
        .to_ascii_lowercase();
    let basename = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    name == "serum2" || name.starts_with("serum 2") || basename.starts_with("serum2")
}

/// Inflate one zlib stream starting at `data[0]`.
/// Returns the decompressed bytes and the number of input bytes consumed.
fn inflate_stream(data: &[u8]) -> Result<(Vec<u8>, usize), SerumError> {
    use std::io::Read;
    if data.is_empty() || data[0] != 0x78 {
        return err("not a zlib stream (expected 0x78 header byte)");
    }
    let mut dec = ZlibDecoder::new(data);
    let mut out = Vec::with_capacity(4096);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match dec.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&chunk[..n]);
                if out.len() > MAX_STREAM_OUT {
                    return err("decompressed stream exceeds sanity limit");
                }
            }
            Err(e) => return err(format!("zlib error: {e}")),
        }
    }
    Ok((out, dec.total_in() as usize))
}

/// Split a Serum 1 chunk into its zlib streams and validate the trailer.
///
/// Layout: `[zlib stream 0][zlib stream 1]...[u32 LE trailer]` where the
/// trailer equals the compressed size of stream 0 (the preset state; the
/// remaining streams carry embedded wavetable / noise data). A minimal zlib
/// stream is 8 bytes, so a 4-byte tail can only be the trailer.
fn analyze_chunk(chunk: &[u8]) -> Result<(Vec<Vec<u8>>, bool), SerumError> {
    if chunk.len() < 8 {
        return err("chunk too small to contain a zlib stream");
    }
    let candidate_trailer =
        u32::from_le_bytes(chunk[chunk.len() - 4..].try_into().unwrap()) as usize;
    let mut pos = 0usize;
    let mut streams = Vec::new();
    while chunk.len() - pos > 4 {
        if chunk[pos] != 0x78 {
            if pos == 0 {
                return err("chunk does not start with a zlib stream");
            }
            break;
        }
        let (out, consumed) = match inflate_stream(&chunk[pos..]) {
            Ok(v) => v,
            Err(e) => {
                if pos == 0 {
                    return Err(e);
                }
                break;
            }
        };
        streams.push(out);
        pos += consumed;
    }
    if streams.is_empty() {
        return err("chunk contains no zlib streams");
    }
    // The consumed prefix must cover every byte except the trailer.
    let clean_tail = chunk.len() - pos == 4;
    let (_, s0_len) = inflate_stream(chunk)?;
    let has_trailer = clean_tail && candidate_trailer == s0_len;
    Ok((streams, has_trailer))
}

fn read_cstr(buf: &[u8], off: usize, len: usize) -> String {
    let end = (off + len).min(buf.len());
    if off >= buf.len() {
        return String::new();
    }
    let s = &buf[off..end];
    let nul = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..nul]).trim().to_string()
}

fn parse_meta(state: &[u8]) -> PresetMeta {
    PresetMeta {
        preset_name: read_cstr(state, OFF_PRESET_NAME, 32),
        author: read_cstr(state, OFF_AUTHOR, 48),
        category: read_cstr(state, OFF_CATEGORY, 48),
        version_f32: if state.len() >= OFF_VERSION_F32 + 4 {
            f32::from_le_bytes(
                state[OFF_VERSION_F32..OFF_VERSION_F32 + 4]
                    .try_into()
                    .unwrap(),
            )
        } else {
            0.0
        },
    }
}

/// Walk FL Studio's VST3 wrapper state and return the cid-3 payload
/// (the plugin's own saved state), if the layout matches.
fn fl_vst3_wrapper_cid3(state: &[u8]) -> Option<&[u8]> {
    for start in 0..=8usize {
        let mut pos = start;
        let mut found: Option<&[u8]> = None;
        let mut nchunks = 0usize;
        let mut has_cid1 = false;
        while pos + 12 <= state.len() {
            let cid = u32::from_le_bytes(state[pos..pos + 4].try_into().unwrap());
            let sz = u64::from_le_bytes(state[pos + 4..pos + 12].try_into().unwrap());
            let Some(sz) = usize::try_from(sz).ok() else {
                break;
            };
            if pos + 12 + sz > state.len() {
                break;
            }
            if cid == 1 && sz == 64 {
                has_cid1 = true;
            }
            if cid == 3 && found.is_none() {
                found = Some(&state[pos + 12..pos + 12 + sz]);
            }
            nchunks += 1;
            pos += 12 + sz;
        }
        if pos == state.len() && nchunks >= 2 && has_cid1 {
            return found;
        }
    }
    None
}

/// Extract the `CcnK` preset chunk out of a complete VST2 fxp/fxb blob.
fn chunk_from_ccnk(blob: &[u8]) -> Result<&[u8], SerumError> {
    if blob.len() < 0x3C || &blob[0..4] != b"CcnK" {
        return err("bad CcnK blob");
    }
    let magic = &blob[8..12];
    if magic == b"FPCh" {
        let cs = u32::from_be_bytes(blob[0x38..0x3C].try_into().unwrap()) as usize;
        if 0x3C + cs > blob.len() {
            return err("FPCh chunk overruns blob");
        }
        Ok(&blob[0x3C..0x3C + cs])
    } else if magic == b"FBCh" {
        // Chunk bank: CcnK(4) size(4) FBCh(4) ver(4) uid(4) fxVer(4)
        // numPrograms(4) chunkSize(4) chunk...
        let cs = u32::from_be_bytes(blob[28..32].try_into().unwrap()) as usize;
        if 32 + cs > blob.len() {
            return err("FBCh chunk overruns blob");
        }
        Ok(&blob[32..32 + cs])
    } else {
        err(format!("unsupported fxMagic {:?}", magic))
    }
}

/// Recover the Serum 1 preset chunk from a `PluginParams` state payload.
pub fn serum1_chunk_from_state(state: &[u8]) -> Result<Serum1Chunk, SerumError> {
    if state.is_empty() {
        return err("plugin state is empty");
    }

    let (raw, source): (&[u8], SourceKind) = if let Some(cid3) = fl_vst3_wrapper_cid3(state) {
        if cid3.starts_with(b"XferJson") {
            return err("cid-3 state is Serum 2 XferJson (not Serum 1)");
        }
        (cid3, SourceKind::FlVst3Wrapper)
    } else if state.starts_with(b"VstW") {
        let off = state[..64.min(state.len())]
            .windows(4)
            .position(|w| w == b"CcnK")
            .ok_or_else(|| SerumError("VstW wrapper without CcnK blob".into()))?;
        (chunk_from_ccnk(&state[off..])?, SourceKind::VstWFxPreset)
    } else if state.starts_with(b"CcnK") {
        (chunk_from_ccnk(state)?, SourceKind::CcnKFxPreset)
    } else if state[0] == 0x78 {
        (state, SourceKind::RawZlib)
    } else {
        return err("unrecognized plugin state layout");
    };

    let (streams, has_trailer) = analyze_chunk(raw)?;
    let stream0 = &streams[0];
    if stream0.len() > SERUM1_STATE_SIZE {
        return err(format!(
            "preset state is {} bytes (expected at most {SERUM1_STATE_SIZE})",
            stream0.len()
        ));
    }
    let meta = parse_meta(stream0);

    let mut chunk = raw.to_vec();
    if !has_trailer {
        // Serum 2's importer silently rejects chunks without the trailer
        // word; append it (compressed size of stream 0) at the end.
        let (_, s0_len) = inflate_stream(raw)?;
        chunk.extend_from_slice(&(s0_len as u32).to_le_bytes());
    }

    Ok(Serum1Chunk {
        chunk,
        source,
        meta,
        stream_sizes: streams.iter().map(|s| s.len()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zlib_stream(data: &[u8]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;
        let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn synthetic_state() -> Vec<u8> {
        let mut s0 = vec![0u8; SERUM1_STATE_SIZE];
        s0[OFF_PRESET_NAME..OFF_PRESET_NAME + 5].copy_from_slice(b"Test\0");
        s0[OFF_VERSION_F32..OFF_VERSION_F32 + 4].copy_from_slice(&0.1631f32.to_le_bytes());
        let z0 = zlib_stream(&s0);
        let z1 = zlib_stream(&vec![0u8; 2048 * 4]);
        let mut out = Vec::new();
        out.extend_from_slice(&z0);
        out.extend_from_slice(&z1);
        out.extend_from_slice(&(z0.len() as u32).to_le_bytes());
        out
    }

    #[test]
    fn extracts_from_raw_zlib() {
        let state = synthetic_state();
        let c = serum1_chunk_from_state(&state).unwrap();
        assert_eq!(c.source, SourceKind::RawZlib);
        assert_eq!(c.stream_sizes, vec![SERUM1_STATE_SIZE, 8192]);
        assert_eq!(c.meta.preset_name, "Test");
        assert!((c.meta.version_f32 - 0.1631).abs() < 1e-6);
    }

    #[test]
    fn extracts_from_fl_vst3_wrapper() {
        let cid3 = synthetic_state();
        let mut state = Vec::new();
        state.extend_from_slice(&[1, 0, 0, 0]); // prologue
        // cid 1, 64 bytes
        state.extend_from_slice(&1u32.to_le_bytes());
        state.extend_from_slice(&64u64.to_le_bytes());
        state.extend_from_slice(&[0u8; 64]);
        // cid 3
        state.extend_from_slice(&3u32.to_le_bytes());
        state.extend_from_slice(&(cid3.len() as u64).to_le_bytes());
        state.extend_from_slice(&cid3);
        // cid 4
        state.extend_from_slice(&4u32.to_le_bytes());
        state.extend_from_slice(&8u64.to_le_bytes());
        state.extend_from_slice(&[0u8; 8]);
        let c = serum1_chunk_from_state(&state).unwrap();
        assert_eq!(c.source, SourceKind::FlVst3Wrapper);
        assert_eq!(c.chunk, cid3);
        assert_eq!(c.meta.preset_name, "Test");
    }

    #[test]
    fn detects_serum2_xferjson() {
        let mut cid3 = b"XferJson\0".to_vec();
        cid3.extend_from_slice(&[0u8; 32]);
        let mut state = Vec::new();
        state.extend_from_slice(&[1, 0, 0, 0]);
        state.extend_from_slice(&1u32.to_le_bytes());
        state.extend_from_slice(&64u64.to_le_bytes());
        state.extend_from_slice(&[0u8; 64]);
        state.extend_from_slice(&3u32.to_le_bytes());
        state.extend_from_slice(&(cid3.len() as u64).to_le_bytes());
        state.extend_from_slice(&cid3);
        assert!(serum1_chunk_from_state(&state).is_err());
    }

    #[test]
    fn repairs_missing_trailer() {
        let state = synthetic_state();
        let mut truncated = state.clone();
        truncated.truncate(state.len() - 4);
        let c = serum1_chunk_from_state(&truncated).unwrap();
        assert_eq!(c.chunk, state);
    }

    #[test]
    fn plugin_detection() {
        assert!(is_serum1(
            b"Serum",
            b"/Library/Audio/Plug-Ins/VST3/Serum.vst3"
        ));
        assert!(is_serum1(b"Serum", b"C:\\VST\\Serum_x64.dll"));
        assert!(is_serum1(
            b"Serum FX",
            b"/Library/Audio/Plug-Ins/VST3/Serum FX.vst3"
        ));
        assert!(!is_serum1(
            b"Serum2",
            b"/Library/Audio/Plug-Ins/VST3/Serum2.vst3"
        ));
        assert!(!is_serum1(b"Serum 2", b""));
        assert!(!is_serum1(b"Vital", b"/Vital.vst3"));
        assert!(is_serum2(b"Serum2", b"/Serum2.vst3"));
        assert!(!is_serum2(b"Serum", b"/Serum.vst3"));
    }
}

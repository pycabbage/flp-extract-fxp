//! FLP rewrite machinery: turning Serum 1 `PluginParams` (event 213) payloads
//! into Serum 2 ones.
//!
//! Every byte rule implemented here comes from
//! `docs/flp-serum2-conversion.md` §6 ("Rewrite recipe"). This module only
//! performs byte surgery on the event stream; building the actual Serum 2
//! XferJson records (processor / controller / parameter list) is the
//! importer's job, injected through the [`BundleSource`] seam.

use std::collections::HashMap;
use std::ops::Range;

use crate::core::text;
use crate::flp;
use crate::s1state;
use crate::serum;
use crate::serum2state;

/// 16-byte Serum 2 plugin UID for top-level cid 52 (doc §2.6):
/// ASCII `XESVsfsPerum 2` + 2 NULs.
const SERUM2_UID: [u8; 16] = [
    0x58, 0x45, 0x53, 0x56, 0x73, 0x66, 0x73, 0x50, 0x65, 0x72, 0x75, 0x6D, 0x20, 0x32, 0x00, 0x00,
];

/// Everything needed to turn one Serum 1 instance into a Serum 2 instance.
/// Built by the orchestrator (the importer runs elsewhere); flpconv only does
/// byte surgery.
#[derive(Debug, Clone)]
pub struct Serum2Bundle {
    /// Full XferJson processor record (the FL wrapper's inner cid-3 payload).
    pub processor_record: Vec<u8>,
    /// Full XferJson controller record (inner cid-2), or None to omit it.
    pub controller_record: Option<Vec<u8>>,
    /// FL's saved parameter-id list (inner cid-4 payload bytes, lifted from a
    /// genuine Serum 2 instance — 10,496 B for 2.0.22).
    pub param_list: Vec<u8>,
    /// Plugin name string for cid 54 ("Serum2").
    pub name: String,
    /// Plugin filename string for cid 55 (e.g. "/Library/Audio/Plug-Ins/VST3/Serum2.vst3").
    pub filename: String,
    /// Vendor string for cid 56 ("Xfer Records").
    pub vendor: String,
}

/// One convertible Serum 1 synth instance, located in the event stream.
#[derive(Debug, Clone)]
pub struct InstancePlan {
    /// Index into `parse_events()` output.
    pub event_index: usize,
    /// Byte offset of the event id in the FLdt stream (diagnostics only).
    pub event_offset: usize,
    pub channel: Option<u16>,
    pub channel_name: String,
    pub plugin_name: String,
    /// The ORIGINAL event-213 payload (verbatim).
    pub payload: Vec<u8>,
    /// The original plugin filename (cid 55), decoded lossily.
    pub plugin_filename: String,
}

/// One successfully rewritten instance, for the human report.
#[derive(Debug, Clone)]
pub struct ConvertedInstance {
    pub channel: Option<u16>,
    pub channel_name: String,
    /// Preset name recovered from the original Serum 1 state (may be empty).
    pub preset_name: String,
    pub new_payload_len: usize,
}

/// Result of [`apply`]: what was rewritten plus non-fatal notes.
#[derive(Debug, Default)]
pub struct FlpConversionReport {
    pub converted: Vec<ConvertedInstance>,
    pub warnings: Vec<String>,
}

/// Orchestrator seam: the importer module (written next) implements this.
pub trait BundleSource {
    fn bundle_for(
        &mut self,
        plan: &InstancePlan,
        s1_chunk: &[u8],
    ) -> Result<Option<Serum2Bundle>, String>;
}

/// Placeholder source: always errors (kept for tests of the trait seam).
pub struct UnimplementedSource;

impl BundleSource for UnimplementedSource {
    fn bundle_for(
        &mut self,
        _plan: &InstancePlan,
        _s1_chunk: &[u8],
    ) -> Result<Option<Serum2Bundle>, String> {
        Err("importer not wired yet".into())
    }
}

/// Produces Serum 2 bundles from Serum 1 instances by running the importer.
pub struct RealSource {
    /// FL's saved parameter-id list template (inner cid-4 payload of a real
    /// Serum 2 instance, 2.0.22-era).
    pub param_list: Vec<u8>,
    /// Controller record template (inner cid-2 payload of a real Serum 2
    /// instance; the JSON header gets patched per preset).
    pub controller_template: Option<Vec<u8>>,
    /// Failure reasons collected for instances that produced `Ok(None)`
    /// (drained by the orchestrator to warn or abort).
    pub warnings: Vec<String>,
}

impl RealSource {
    /// Loads the embedded calibration defaults (real Serum 2 2.0.22 instance
    /// payloads lifted from `serina1.flp`, see docs/flp-serum2-conversion.md
    /// §7).
    pub fn embedded() -> Self {
        RealSource {
            param_list: include_bytes!("../docs/data/serum2_cid4.bin").to_vec(),
            controller_template: Some(
                include_bytes!("../docs/data/serum2_controller_record.bin").to_vec(),
            ),
            warnings: Vec::new(),
        }
    }
}

impl BundleSource for RealSource {
    fn bundle_for(
        &mut self,
        plan: &InstancePlan,
        s1_chunk: &[u8],
    ) -> Result<Option<Serum2Bundle>, String> {
        let preset = match s1state::parse_preset(s1_chunk) {
            Ok(p) => p,
            Err(e) => {
                self.warnings.push(format!(
                    "instance on channel '{}': {e}",
                    display_name(&plan.channel_name)
                ));
                return Ok(None);
            }
        };
        let converted = crate::importer::convert_s1_to_s2(&preset, 0)?;
        let processor_record = serum2state::build_processor_record(&converted.body);
        let controller_record = match &self.controller_template {
            Some(t) => Some(wrap_controller_record(t, &preset)?),
            None => None,
        };
        Ok(Some(Serum2Bundle {
            processor_record,
            controller_record,
            param_list: self.param_list.clone(),
            name: "Serum2".into(),
            filename: serum2_filename(&plan.plugin_filename),
            vendor: "Xfer Records".into(),
        }))
    }
}

/// Derive the Serum 2 plugin filename from the ORIGINAL instance's filename:
/// keep the directory prefix, replace the basename with `Serum2.vst3`
/// (doc §6 #4). e.g. `/Library/.../Serum.vst3` -> `/Library/.../Serum2.vst3`,
/// `C:\VST\Serum_x64.dll` -> `C:\VST\Serum2.vst3`.
fn serum2_filename(orig: &str) -> String {
    let s = orig.trim();
    match s.rfind(['/', '\\']) {
        Some(i) => format!("{}Serum2.vst3", &s[..=i]),
        None => "Serum2.vst3".into(),
    }
}

/// Build the per-preset controller record from the template: keep the
/// template's zstd frame, uncompressed size and format verbatim; patch the
/// JSON header with the frame's md5 and the S1 preset's metadata while
/// keeping the template's own `productVersion`/`version` values.
fn wrap_controller_record(template: &[u8], preset: &s1state::S1Preset) -> Result<Vec<u8>, String> {
    let (json, uncomp, format, frame_start) = serum2state::parse_xfer_json(template)?;
    let frame = &template[frame_start..];
    let product_version =
        json_string_field(&json, "productVersion").unwrap_or_else(|| "2.0.22".into());
    let version = json_raw_field(&json, "version")
        .unwrap_or("8.0")
        .to_string();
    let header = serum2state::controller_json_header(
        &md5_hex(frame),
        &preset.meta.preset_name,
        &preset.meta.author,
        &preset.meta.category,
        &product_version,
        &version,
    );
    Ok(serum2state::wrap_controller_frame(
        header, frame, uncomp, format,
    ))
}

fn md5_hex(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(data);
    let digest = h.finalize();
    let mut out = String::with_capacity(32);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Value of a `"key":"string"` field in a flat sorted-key JSON header.
fn json_string_field(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let start = json.find(&pat)? + pat.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Raw (unquoted) value of a `"key":<token>` field, up to `,` or `}`.
fn json_raw_field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let start = json.find(&pat)? + pat.len();
    let rest = &json[start..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    Some(rest[..end].trim())
}

/// Walk the FLP events and locate every Serum 1 SYNTH instance (plugin name
/// exactly "Serum" or basename Serum.vst3/Serum_x64.dll — NOT "Serum FX",
/// which stays untouched). Instances are returned in file order, matching
/// `scan_serum_instances`' channel/name bookkeeping.
pub fn scan_convertible(buf: &[u8]) -> Result<Vec<InstancePlan>, String> {
    Ok(scan_convertible_detailed(buf)?.0)
}

/// Like [`scan_convertible`] but also returns warnings (one per Serum FX
/// instance deliberately left untouched).
pub fn scan_convertible_detailed(buf: &[u8]) -> Result<(Vec<InstancePlan>, Vec<String>), String> {
    let events = flp::parse_events(buf).map_err(|e| e.to_string())?;
    let spans = walk_spans(buf)?;
    if spans.len() != events.len() || events.iter().zip(&spans).any(|(ev, sp)| ev.id != sp.id) {
        return Err("internal error: event framing disagreement".into());
    }

    let mut channels: HashMap<u16, String> = HashMap::new();
    let mut cur_channel: Option<u16> = None;
    let mut cur_fx_name = String::new();
    let mut plans = Vec::new();
    let mut warnings = Vec::new();

    for (i, ev) in events.iter().enumerate() {
        match ev.id {
            flp::EV_NEW_CHANNEL => {
                if ev.data.len() >= 2 {
                    cur_channel = Some(u16::from_le_bytes([ev.data[0], ev.data[1]]));
                }
            }
            flp::EV_TEXT_CHANNEL_NAME => {
                if let Some(ch) = cur_channel {
                    channels.insert(ch, text(ev.data));
                }
            }
            flp::EV_TEXT_FX_TRACK_NAME => {
                cur_fx_name = text(ev.data);
            }
            flp::EV_PLUGIN_PARAMS => {
                let Ok(pp) = flp::parse_plugin_params(ev.data) else {
                    continue;
                };
                if serum::is_serum2(pp.name, pp.filename) {
                    continue;
                }
                let where_ = cur_channel
                    .and_then(|c| channels.get(&c).cloned())
                    .unwrap_or_else(|| cur_fx_name.clone());
                if is_serum_fx(pp.name, pp.filename) {
                    warnings.push(format!(
                        "Serum FX instance on channel '{where_}' left untouched (only the Serum synth is converted)"
                    ));
                    continue;
                }
                if !is_serum1_synth(pp.name, pp.filename) || pp.state.is_empty() {
                    continue;
                }
                plans.push(InstancePlan {
                    event_index: i,
                    event_offset: spans[i].start - locate_chunks(buf)?.2,
                    channel: cur_channel,
                    channel_name: where_,
                    plugin_name: text(pp.name),
                    payload: ev.data.to_vec(),
                    plugin_filename: String::from_utf8_lossy(pp.filename).into_owned(),
                });
            }
            _ => {}
        }
    }
    Ok((plans, warnings))
}

/// Byte-surgery application: for every (InstancePlan, Serum2Bundle) pair with
/// `Some(bundle)`, replace that event's payload with the rebuilt Serum 2
/// payload and re-frame everything (varint length, FLdt u32 length). Every
/// other event stays byte-identical and everything before the FLdt payload is
/// preserved; the output never has trailing bytes.
///
/// `plans` order must match `bundles` order (1:1; None = skip that instance).
pub fn apply(
    buf: &[u8],
    plans: &[InstancePlan],
    bundles: &[Option<Serum2Bundle>],
) -> Result<(Vec<u8>, FlpConversionReport), String> {
    if plans.len() != bundles.len() {
        return Err(format!(
            "plans ({}) and bundles ({}) lengths differ",
            plans.len(),
            bundles.len()
        ));
    }
    // 1. Verify the plans still describe this exact buffer.
    let events = flp::parse_events(buf).map_err(|e| e.to_string())?;
    for (i, (plan, _)) in plans.iter().zip(bundles).enumerate() {
        let Some(ev) = events.get(plan.event_index) else {
            return Err(format!(
                "plan {i} references event {} but the file has {} events",
                plan.event_index,
                events.len()
            ));
        };
        if ev.id != flp::EV_PLUGIN_PARAMS {
            return Err(format!(
                "plan {i} references event {} (id {}), not a PluginParams event",
                plan.event_index, ev.id
            ));
        }
        if ev.data != plan.payload.as_slice() {
            return Err(format!(
                "plan {i} payload does not match the buffer (stale InstancePlan?)"
            ));
        }
    }

    let (_, dt_len_pos, dt_start, _) = locate_chunks(buf)?;
    let spans = walk_spans(buf)?;
    if spans.len() != events.len() {
        return Err("internal error: event framing disagreement".into());
    }

    // 2. Build the replacement payloads.
    let mut replacements: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut report = FlpConversionReport::default();
    for (i, (plan, bundle)) in plans.iter().zip(bundles).enumerate() {
        let Some(b) = bundle else {
            report.warnings.push(format!(
                "instance on channel '{}' skipped (no Serum 2 bundle supplied)",
                display_name(&plan.channel_name)
            ));
            continue;
        };
        if replacements
            .insert(plan.event_index, build_serum2_payload(&plan.payload, b)?)
            .is_some()
        {
            return Err(format!("plan {i} duplicates event {}", plan.event_index));
        }
        report.converted.push(ConvertedInstance {
            channel: plan.channel,
            channel_name: plan.channel_name.clone(),
            preset_name: original_preset_name(&plan.payload),
            new_payload_len: replacements[&plan.event_index].len(),
        });
    }

    // 3. Splice: everything before the FLdt payload verbatim, then re-emit
    //    each event (replaced ones with fresh framing).
    let mut out: Vec<u8> = Vec::with_capacity(buf.len() + 4096);
    out.extend_from_slice(&buf[..dt_start]);
    for (i, span) in spans.iter().enumerate() {
        match replacements.get(&i) {
            Some(new_payload) => {
                out.push(flp::EV_PLUGIN_PARAMS);
                push_varint(&mut out, new_payload.len());
                out.extend_from_slice(new_payload);
            }
            None => out.extend_from_slice(&buf[span.start..span.payload.end]),
        }
    }

    // 4. Fix the FLdt u32 LE chunk length.
    let new_dtlen = out.len() - dt_start;
    out[dt_len_pos..dt_len_pos + 4].copy_from_slice(&(new_dtlen as u32).to_le_bytes());
    Ok((out, report))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// A `[u32 cid][u64 size][data]` record; `data` indexes into the parsed buffer.
struct TopRecord {
    cid: u32,
    data: Range<usize>,
}

/// Locate the FLhd/FLdt chunk boundaries.
/// Returns `(hdrlen, dtlen_field_pos, dt_payload_start, dtlen)`.
fn locate_chunks(buf: &[u8]) -> Result<(usize, usize, usize, usize), String> {
    if buf.len() < 8 || &buf[0..4] != b"FLhd" {
        return Err("missing FLhd header (not an FLP file?)".into());
    }
    let hdrlen = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    let dt_pos = 8 + hdrlen;
    if buf.len() < dt_pos + 8 || &buf[dt_pos..dt_pos + 4] != b"FLdt" {
        return Err("missing FLdt chunk".into());
    }
    let dtlen = u32::from_le_bytes(buf[dt_pos + 4..dt_pos + 8].try_into().unwrap()) as usize;
    Ok((hdrlen, dt_pos + 4, dt_pos + 8, dtlen))
}

/// Byte offset + id + payload range of every event in the FLdt stream
/// (framing mirrors `flp::parse_events`).
struct Span {
    start: usize,
    id: u8,
    payload: Range<usize>,
}

fn walk_spans(buf: &[u8]) -> Result<Vec<Span>, String> {
    let (_, _, dt_start, dtlen) = locate_chunks(buf)?;
    let end = (dt_start + dtlen).min(buf.len());
    let mut pos = dt_start;
    let mut spans = Vec::new();
    while pos < end {
        let start = pos;
        let id = buf[pos];
        pos += 1;
        let dlen: usize = match id {
            0..=63 => 1,
            64..=127 => 2,
            128..=191 => 4,
            _ => {
                let Some(v) = flp::read_varint(buf, &mut pos) else {
                    return Err(format!("truncated varint at offset {start:#x}"));
                };
                v as usize
            }
        };
        if pos + dlen > end {
            return Err(format!(
                "event {id} at offset {start:#x} overruns the FLdt chunk"
            ));
        }
        spans.push(Span {
            start,
            id,
            payload: pos..pos + dlen,
        });
        pos += dlen;
    }
    Ok(spans)
}

/// Parse a `[u32 cid][u64 size][data]` record sequence starting at `pos`.
fn parse_record_seq(buf: &[u8], mut pos: usize) -> Result<Vec<TopRecord>, String> {
    let mut recs = Vec::new();
    while pos < buf.len() {
        if pos + 12 > buf.len() {
            return Err(format!("truncated record header at offset {pos:#x}"));
        }
        let cid = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
        let sz = u64::from_le_bytes(buf[pos + 4..pos + 12].try_into().unwrap());
        let Some(sz) = usize::try_from(sz).ok() else {
            return Err(format!("record {cid} size overflows address space"));
        };
        pos += 12;
        if pos + sz > buf.len() {
            return Err(format!("record {cid} (size {sz}) overruns payload"));
        }
        recs.push(TopRecord {
            cid,
            data: pos..pos + sz,
        });
        pos += sz;
    }
    Ok(recs)
}

fn record_data<'a>(payload: &'a [u8], recs: &'a [TopRecord], cid: u32) -> Option<&'a [u8]> {
    recs.iter()
        .find(|r| r.cid == cid)
        .map(|r| &payload[r.data.clone()])
}

fn push_rec(out: &mut Vec<u8>, cid: u32, data: &[u8]) {
    out.extend_from_slice(&cid.to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(data);
}

fn push_varint(out: &mut Vec<u8>, mut len: usize) {
    loop {
        let b = (len & 0x7f) as u8;
        len >>= 7;
        if len == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
}

fn display_name(name: &str) -> &str {
    if name.is_empty() { "-" } else { name }
}

fn lower(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_ascii_lowercase()
}

/// Basename of a plugin path with known extensions stripped
/// (mirrors `serum::is_serum1`).
fn plugin_basename(filename: &[u8]) -> String {
    String::from_utf8_lossy(filename)
        .trim()
        .to_ascii_lowercase()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim_end_matches(".vst3")
        .trim_end_matches(".vst")
        .trim_end_matches(".dll")
        .trim_end_matches(".vstpreset")
        .to_string()
}

/// Serum 1 *synth* only — excludes "Serum FX" and Serum 2.
fn is_serum1_synth(name: &[u8], filename: &[u8]) -> bool {
    let name = lower(name);
    let base = plugin_basename(filename);
    if name == "serum2"
        || name.starts_with("serum 2")
        || base == "serum2"
        || base.starts_with("serum2")
    {
        return false;
    }
    name == "serum" || name == "serum_x64" || base == "serum" || base == "serum_x64"
}

fn is_serum_fx(name: &[u8], filename: &[u8]) -> bool {
    lower(name) == "serum fx" || plugin_basename(filename) == "serum fx"
}

/// Build the new Serum 2 event-213 payload from the original Serum 1 payload
/// plus a bundle (doc §6). The original version u32 and records
/// cid 2/30/32/50 are kept verbatim; cid 1 gets its u32@8 patched 12 -> 1;
/// cid 52/54/55 are replaced; cid 56 uses the bundle vendor (falling back to
/// the original bytes when the bundle omits it); cid 53 is rebuilt LAST.
fn build_serum2_payload(orig: &[u8], b: &Serum2Bundle) -> Result<Vec<u8>, String> {
    if orig.len() < 4 {
        return Err("PluginParams payload too small".into());
    }
    let version = u32::from_le_bytes(orig[0..4].try_into().unwrap());
    if version < 5 {
        return Err(format!(
            "PluginParams version {version} predates the chunked layout"
        ));
    }
    let recs = parse_record_seq(orig, 4)?;
    let Some(cid1) = record_data(orig, &recs, 1) else {
        return Err("missing cid 1 record".into());
    };
    if cid1.len() != 20 {
        return Err(format!(
            "cid 1 record is {} bytes (expected 20)",
            cid1.len()
        ));
    }
    let Some(cid2) = record_data(orig, &recs, 2) else {
        return Err("missing cid 2 record".into());
    };
    let Some(cid30) = record_data(orig, &recs, 30) else {
        return Err("missing cid 30 record".into());
    };
    let Some(cid32) = record_data(orig, &recs, 32) else {
        return Err("missing cid 32 record".into());
    };
    let Some(cid50) = record_data(orig, &recs, 50) else {
        return Err("missing cid 50 record".into());
    };
    if b.processor_record.is_empty() {
        return Err("Serum2Bundle has an empty processor record".into());
    }
    if b.param_list.is_empty() {
        return Err("Serum2Bundle has an empty parameter list".into());
    }

    let mut p = Vec::with_capacity(orig.len() + b.processor_record.len() + 4096);
    p.extend_from_slice(&version.to_le_bytes());

    // cid 1: original bytes with u32@8 = 1 (doc §2.3 / §6 #1).
    let mut c1 = cid1.to_vec();
    c1[8..12].copy_from_slice(&1u32.to_le_bytes());
    push_rec(&mut p, 1, &c1);

    // Kept verbatim (doc §6 "Keep").
    push_rec(&mut p, 2, cid2);
    push_rec(&mut p, 30, cid30);
    push_rec(&mut p, 32, cid32);
    push_rec(&mut p, 50, cid50);

    // Swapped plugin identity (doc §6 #2-4).
    push_rec(&mut p, 52, &SERUM2_UID);
    push_rec(&mut p, 54, b.name.as_bytes());
    push_rec(&mut p, 55, b.filename.as_bytes());
    let vendor: &[u8] = if b.vendor.is_empty() {
        record_data(orig, &recs, 56).unwrap_or(&[])
    } else {
        b.vendor.as_bytes()
    };
    push_rec(&mut p, 56, vendor);

    // cid 53 LAST: rebuilt FL VST3 wrapper (doc §3.1 / §6 #6-9):
    // [prol=1][cid1 64B][cid3 processor][cid2 controller?][cid4 param list].
    let mut w = Vec::new();
    w.extend_from_slice(&1u32.to_le_bytes());
    let mut inner1 = vec![0u8; 64];
    inner1[0..4].copy_from_slice(&1u32.to_le_bytes());
    push_rec(&mut w, 1, &inner1);
    push_rec(&mut w, 3, &b.processor_record);
    if let Some(c) = &b.controller_record {
        push_rec(&mut w, 2, c);
    }
    push_rec(&mut w, 4, &b.param_list);
    push_rec(&mut p, 53, &w);
    Ok(p)
}

/// Recover the Serum 1 preset name from the original payload for the report
/// (best effort; empty on any failure).
fn original_preset_name(payload: &[u8]) -> String {
    let Ok(pp) = flp::parse_plugin_params(payload) else {
        return String::new();
    };
    let Ok(chunk) = serum::serum1_chunk_from_state(pp.state) else {
        return String::new();
    };
    chunk.meta.preset_name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flp::{EV_NEW_CHANNEL, EV_TEXT_CHANNEL_NAME};

    const CID1_S1: [u8; 20] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    const CID2_S1: [u8; 25] = [
        0x00, 0xA0, 0x00, 0x00, 0x00, 0x19, 0x00, 0x00, 0x00, 0x8D, 0x7D, 0x20, 0xA4, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    const C30: [u8; 16] = [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const C32: [u8; 12] = [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
    const C50: [u8; 16] = [0x08, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const S1_UID: [u8; 16] = [
        0x58, 0x54, 0x53, 0x56, 0x73, 0x66, 0x73, 0x58, 0x65, 0x72, 0x75, 0x6D, 0x00, 0x00, 0x00,
        0x00,
    ];
    const S2_UID: [u8; 16] = [
        0x58, 0x45, 0x53, 0x56, 0x73, 0x66, 0x73, 0x50, 0x65, 0x72, 0x75, 0x6D, 0x20, 0x32, 0x00,
        0x00,
    ];

    fn zlib_stream(data: &[u8]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;
        let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    /// Real-shaped Serum 1 inner cid 3: zlib preset state (preset name at
    /// 0x4972) + a second wavetable stream + u32 LE trailer (doc §3.3).
    fn s1_cid3() -> Vec<u8> {
        let mut s0 = vec![0u8; serum::SERUM1_STATE_SIZE];
        s0[serum::OFF_PRESET_NAME..serum::OFF_PRESET_NAME + 11].copy_from_slice(b"TestPreset\0");
        s0[serum::OFF_VERSION_F32..serum::OFF_VERSION_F32 + 4]
            .copy_from_slice(&0.1631f32.to_le_bytes());
        let z0 = zlib_stream(&s0);
        let z1 = zlib_stream(&[0u8; 8192]);
        let mut v = z0;
        v.extend_from_slice(&z1);
        let z0_len = v.len() - z1.len() - 4;
        v.extend_from_slice(&(z0_len as u32).to_le_bytes());
        v
    }

    fn top_rec(cid: u32, data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        push_rec(&mut v, cid, data);
        v
    }

    /// A realistic Serum 1 event-213 payload (doc §2.1 / §3).
    fn serum1_payload(name: &str, filename: &str) -> Vec<u8> {
        let cid3 = s1_cid3();
        let mut cid4 = Vec::new();
        cid4.extend_from_slice(&3u32.to_le_bytes());
        cid4.extend_from_slice(&0u32.to_le_bytes());
        cid4.extend_from_slice(&1u32.to_le_bytes());
        cid4.extend_from_slice(&2u32.to_le_bytes());

        let mut wrapper = Vec::new();
        wrapper.extend_from_slice(&1u32.to_le_bytes());
        let mut inner1 = vec![0u8; 64];
        inner1[0..4].copy_from_slice(&1u32.to_le_bytes());
        wrapper.extend_from_slice(&top_rec(1, &inner1));
        wrapper.extend_from_slice(&top_rec(3, &cid3));
        wrapper.extend_from_slice(&top_rec(4, &cid4));

        let mut p = Vec::new();
        p.extend_from_slice(&12u32.to_le_bytes());
        p.extend_from_slice(&top_rec(1, &CID1_S1));
        p.extend_from_slice(&top_rec(2, &CID2_S1));
        p.extend_from_slice(&top_rec(30, &C30));
        p.extend_from_slice(&top_rec(32, &C32));
        p.extend_from_slice(&top_rec(50, &C50));
        p.extend_from_slice(&top_rec(52, &S1_UID));
        p.extend_from_slice(&top_rec(54, name.as_bytes()));
        p.extend_from_slice(&top_rec(55, filename.as_bytes()));
        p.extend_from_slice(&top_rec(56, b"Xfer Records"));
        p.extend_from_slice(&top_rec(53, &wrapper));
        p
    }

    fn build_flp(events: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut dt = Vec::new();
        for (id, data) in events {
            dt.push(*id);
            if *id >= 192 {
                push_varint(&mut dt, data.len());
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

    fn utf16_name(s: &str) -> Vec<u8> {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn sample_flp() -> Vec<u8> {
        build_flp(&[
            (EV_NEW_CHANNEL, vec![0, 0]),
            (EV_TEXT_CHANNEL_NAME, utf16_name("Serum")),
            (
                flp::EV_PLUGIN_PARAMS,
                serum1_payload("Serum", "/Library/Audio/Plug-Ins/VST3/Serum.vst3"),
            ),
            (128, vec![1, 2, 3, 4]),
        ])
    }

    fn test_bundle(proc_extra: usize) -> Serum2Bundle {
        let mut processor_record = b"XferJson\0".to_vec();
        processor_record.extend(std::iter::repeat_n(0xABu8, proc_extra));
        Serum2Bundle {
            processor_record,
            controller_record: Some(b"XferJson\0ctrl".to_vec()),
            param_list: vec![0x11, 0x22, 0x33, 0x44],
            name: "Serum2".into(),
            filename: "/Library/Audio/Plug-Ins/VST3/Serum2.vst3".into(),
            vendor: "Xfer Records".into(),
        }
    }

    /// Test-local `[u32 cid][u64 size][data]` record parser (panics on error).
    fn records_of(buf: &[u8], pos: usize) -> Vec<(u32, Vec<u8>)> {
        let mut out = Vec::new();
        let mut pos = pos;
        while pos < buf.len() {
            let cid = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
            let sz = u64::from_le_bytes(buf[pos + 4..pos + 12].try_into().unwrap()) as usize;
            pos += 12;
            out.push((cid, buf[pos..pos + sz].to_vec()));
            pos += sz;
        }
        assert_eq!(
            pos,
            buf.len(),
            "records must end exactly at the payload end"
        );
        out
    }

    fn fldt_len(buf: &[u8]) -> usize {
        u32::from_le_bytes(buf[18..22].try_into().unwrap()) as usize
    }

    #[test]
    fn scan_finds_serum1_synth() {
        let buf = sample_flp();
        let plans = scan_convertible(&buf).unwrap();
        assert_eq!(plans.len(), 1);
        let p = &plans[0];
        assert_eq!(p.event_index, 2);
        assert_eq!(p.event_offset, walk_spans(&buf).unwrap()[2].start - 22);
        assert_eq!(p.channel, Some(0));
        assert_eq!(p.channel_name, "Serum");
        assert_eq!(p.plugin_name, "Serum");
        assert_eq!(
            p.payload,
            serum1_payload("Serum", "/Library/Audio/Plug-Ins/VST3/Serum.vst3")
        );
    }

    #[test]
    fn apply_rewrites_payload_byte_exactly() {
        let buf = sample_flp();
        let plans = scan_convertible(&buf).unwrap();
        let bundle = test_bundle(0);
        let (out, report) = apply(&buf, &plans, &[Some(bundle.clone())]).unwrap();

        // Re-parses; event count unchanged; non-plugin events byte-identical.
        let evs = flp::parse_events(&out).unwrap();
        assert_eq!(evs.len(), 4);
        assert_eq!(evs[0].id, EV_NEW_CHANNEL);
        assert_eq!(evs[0].data, &[0, 0]);
        assert_eq!(evs[1].id, EV_TEXT_CHANNEL_NAME);
        assert_eq!(evs[1].data, utf16_name("Serum"));
        assert_eq!(evs[3].id, 128);
        assert_eq!(evs[3].data, &[1, 2, 3, 4]);

        // FLdt length correct, no trailing bytes.
        assert_eq!(fldt_len(&out), out.len() - 22);

        // Header before the FLdt length field untouched (the length itself
        // is legitimately rewritten).
        assert_eq!(&out[..18], &buf[..18]);

        // Report.
        assert_eq!(report.converted.len(), 1);
        assert_eq!(report.converted[0].channel, Some(0));
        assert_eq!(report.converted[0].channel_name, "Serum");
        assert_eq!(report.converted[0].preset_name, "TestPreset");
        assert_eq!(report.converted[0].new_payload_len, evs[2].data.len());

        // Byte-exact expected new payload.
        let new_payload = evs[2].data;
        let mut expect = Vec::new();
        expect.extend_from_slice(&12u32.to_le_bytes());
        let mut c1 = CID1_S1.to_vec();
        c1[8..12].copy_from_slice(&1u32.to_le_bytes());
        expect.extend_from_slice(&top_rec(1, &c1));
        expect.extend_from_slice(&top_rec(2, &CID2_S1));
        expect.extend_from_slice(&top_rec(30, &C30));
        expect.extend_from_slice(&top_rec(32, &C32));
        expect.extend_from_slice(&top_rec(50, &C50));
        expect.extend_from_slice(&top_rec(52, &S2_UID));
        expect.extend_from_slice(&top_rec(54, b"Serum2"));
        expect.extend_from_slice(&top_rec(55, b"/Library/Audio/Plug-Ins/VST3/Serum2.vst3"));
        expect.extend_from_slice(&top_rec(56, b"Xfer Records"));
        let mut w = Vec::new();
        w.extend_from_slice(&1u32.to_le_bytes());
        let mut inner1 = vec![0u8; 64];
        inner1[0..4].copy_from_slice(&1u32.to_le_bytes());
        w.extend_from_slice(&top_rec(1, &inner1));
        w.extend_from_slice(&top_rec(3, &bundle.processor_record));
        w.extend_from_slice(&top_rec(2, bundle.controller_record.as_ref().unwrap()));
        w.extend_from_slice(&top_rec(4, &bundle.param_list));
        expect.extend_from_slice(&top_rec(53, &w));
        assert_eq!(new_payload, expect);
    }

    #[test]
    fn new_payload_structure_is_valid_serum2() {
        let buf = sample_flp();
        let plans = scan_convertible(&buf).unwrap();
        let (out, _) = apply(&buf, &plans, &[Some(test_bundle(4096))]).unwrap();
        let evs = flp::parse_events(&out).unwrap();
        let p = evs[2].data;

        // Version + cid order + sizes.
        assert_eq!(u32::from_le_bytes(p[0..4].try_into().unwrap()), 12);
        let recs = records_of(p, 4);
        let cids: Vec<u32> = recs.iter().map(|r| r.0).collect();
        assert_eq!(cids, vec![1, 2, 30, 32, 50, 52, 54, 55, 56, 53]);
        assert_eq!(recs[0].1.len(), 20);
        assert_eq!(recs[5].1, S2_UID);
        assert_eq!(recs[6].1, b"Serum2");
        assert_eq!(recs[7].1, b"/Library/Audio/Plug-Ins/VST3/Serum2.vst3");
        assert_eq!(recs[8].1, b"Xfer Records");

        // cid1: u32@8 patched to 1, rest identical to S1.
        assert_eq!(u32::from_le_bytes(recs[0].1[8..12].try_into().unwrap()), 1);
        let mut c1 = CID1_S1.to_vec();
        c1[8..12].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(recs[0].1, c1);

        // Wrapper layout (cid 53): prologue + [1, 3, 2, 4].
        let w = &recs[9].1;
        assert_eq!(u32::from_le_bytes(w[0..4].try_into().unwrap()), 1);
        let inner = records_of(w, 4);
        assert_eq!(
            inner.iter().map(|r| r.0).collect::<Vec<_>>(),
            vec![1, 3, 2, 4]
        );
        assert_eq!(inner[0].1.len(), 64);
        assert_eq!(inner[0].1[0], 1);
        assert!(inner[0].1[4..].iter().all(|&b| b == 0));
        assert_eq!(inner[1].1, test_bundle(4096).processor_record);
        assert_eq!(inner[2].1, test_bundle(4096).controller_record.unwrap());
        assert_eq!(inner[3].1, test_bundle(4096).param_list);
    }

    #[test]
    fn varint_reframing_two_byte() {
        // Payload ~128+ bytes -> 2-byte varint; also grow the stream so the
        // FLdt length must be re-fixed after the splice.
        let buf = sample_flp();
        let plans = scan_convertible(&buf).unwrap();
        let (out, rep) = apply(&buf, &plans, &[Some(test_bundle(140))]).unwrap();
        let evs = flp::parse_events(&out).unwrap();
        assert!(evs[2].data.len() >= 128);
        assert!(evs[2].data.len() < 16384);
        assert_eq!(evs[2].data.len(), rep.converted[0].new_payload_len);
        // Varint byte check: 0xD5 then two bytes with the continuation bit.
        let off = walk_spans(&out).unwrap()[2].start;
        assert_eq!(out[off], 0xD5);
        assert_eq!(out[off + 1] & 0x80, 0x80);
        assert_eq!(out[off + 2] & 0x80, 0x00);
        assert_eq!(fldt_len(&out), out.len() - 22);
    }

    #[test]
    fn varint_reframing_three_byte() {
        // 16384+ payload bytes -> 3-byte varint (doc §1).
        let buf = sample_flp();
        let plans = scan_convertible(&buf).unwrap();
        let (out, _) = apply(&buf, &plans, &[Some(test_bundle(20_000))]).unwrap();
        let evs = flp::parse_events(&out).unwrap();
        assert!(evs[2].data.len() >= 16384);
        let off = walk_spans(&out).unwrap()[2].start;
        assert_eq!(out[off], 0xD5);
        assert_eq!(out[off + 1] & 0x80, 0x80);
        assert_eq!(out[off + 2] & 0x80, 0x80);
        assert_eq!(out[off + 3] & 0x80, 0x00);
        assert_eq!(fldt_len(&out), out.len() - 22);
    }

    #[test]
    fn serum_fx_not_planned() {
        let buf = build_flp(&[
            (EV_NEW_CHANNEL, vec![3, 0]),
            (
                flp::EV_PLUGIN_PARAMS,
                serum1_payload("Serum FX", "/Library/Audio/Plug-Ins/VST3/Serum FX.vst3"),
            ),
        ]);
        let (plans, warnings) = scan_convertible_detailed(&buf).unwrap();
        assert!(plans.is_empty());
        assert_eq!(warnings.len(), 1);
        // Also visible via scan_serum_instances' bookkeeping: it IS Serum 1
        // per serum::is_serum1, but our planner must exclude the FX.
        let (instances, _) = crate::core::scan_serum_instances(&buf).unwrap();
        assert_eq!(instances.len(), 1);
        // Nothing to convert -> apply is a no-op.
        let (out, rep) = apply(&buf, &plans, &[]).unwrap();
        assert_eq!(out, buf);
        assert!(rep.converted.is_empty());
    }

    #[test]
    fn rejects_zip_buffer() {
        let mut buf = b"PK\x03\x04".to_vec();
        buf.extend_from_slice(&[0u8; 64]);
        assert!(scan_convertible(&buf).is_err());
        assert!(apply(&buf, &[], &[]).is_err());
    }

    #[test]
    fn rejects_malformed_fldt() {
        // Missing FLdt entirely.
        let mut buf = b"FLhd".to_vec();
        buf.extend_from_slice(&6u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 6]);
        assert!(scan_convertible(&buf).is_err());
        // Event overruns the declared FLdt chunk (id 0xD5 + varint claiming
        // far more payload bytes than the declared chunk holds).
        let mut buf3 = build_flp(&[]);
        buf3[18..22].copy_from_slice(&32u32.to_le_bytes());
        buf3.extend_from_slice(&[0xD5, 0xE4, 0x07, 1, 2, 3]); // varint = 100
        assert!(scan_convertible(&buf3).is_err());
        assert!(apply(&buf3, &[], &[]).is_err());
    }

    #[test]
    fn apply_rejects_stale_plans() {
        let buf = sample_flp();
        let plans = scan_convertible(&buf).unwrap();
        // Mismatched buffer: same shape, different payload bytes.
        let other = build_flp(&[
            (EV_NEW_CHANNEL, vec![0, 0]),
            (EV_TEXT_CHANNEL_NAME, utf16_name("Serum")),
            (
                flp::EV_PLUGIN_PARAMS,
                serum1_payload("Serum", "/Library/Audio/Plug-Ins/VST3/Serum.vst3"),
            ),
            (128, vec![9, 9, 9, 9]),
        ]);
        // The plugin event payload is identical, so cross-check by mutating:
        let mut tampered = other.clone();
        tampered.truncate(tampered.len() - 1);
        assert!(apply(&tampered, &plans, &[None]).is_err());
        // Length mismatch between plans and bundles.
        assert!(apply(&buf, &plans, &[]).is_err());
    }

    #[test]
    fn apply_none_bundles_is_identity() {
        let buf = sample_flp();
        let plans = scan_convertible(&buf).unwrap();
        let (out, rep) = apply(&buf, &plans, &[None]).unwrap();
        assert_eq!(out, buf);
        assert!(rep.converted.is_empty());
        assert_eq!(rep.warnings.len(), 1);
        // And with no plans at all.
        let (out2, rep2) = apply(&buf, &[], &[]).unwrap();
        assert_eq!(out2, buf);
        assert!(rep2.converted.is_empty());
    }

    #[test]
    fn controller_none_omits_inner_cid2() {
        let buf = sample_flp();
        let plans = scan_convertible(&buf).unwrap();
        let mut b = test_bundle(10);
        b.controller_record = None;
        let (out, _) = apply(&buf, &plans, &[Some(b)]).unwrap();
        let evs = flp::parse_events(&out).unwrap();
        let recs = records_of(evs[2].data, 4);
        let w = &recs[9].1;
        let inner = records_of(w, 4);
        assert_eq!(inner.iter().map(|r| r.0).collect::<Vec<_>>(), vec![1, 3, 4]);
    }

    #[test]
    fn unimplemented_source_errors() {
        let buf = sample_flp();
        let plans = scan_convertible(&buf).unwrap();
        let mut src = UnimplementedSource;
        let err = src.bundle_for(&plans[0], &[]).expect_err("must error");
        assert_eq!(err, "importer not wired yet");
    }

    #[test]
    fn real_source_embedded_loads_calibration_data() {
        let src = RealSource::embedded();
        assert_eq!(src.param_list.len(), 10_496);
        let t = src.controller_template.expect("controller template");
        assert_eq!(t.len(), 3_340);
        assert_eq!(&t[..9], b"XferJson\0");
        let (json, _, format, _) = serum2state::parse_xfer_json(&t).unwrap();
        assert_eq!(format, 2);
        assert!(json.contains("\"productVersion\":\"2.0.22\""), "{json}");
    }

    #[test]
    fn serum2_filename_derivation() {
        assert_eq!(
            serum2_filename("/Library/Audio/Plug-Ins/VST3/Serum.vst3"),
            "/Library/Audio/Plug-Ins/VST3/Serum2.vst3"
        );
        assert_eq!(
            serum2_filename("C:\\VST\\Serum_x64.dll"),
            "C:\\VST\\Serum2.vst3"
        );
        assert_eq!(serum2_filename("Serum.vst3"), "Serum2.vst3");
    }

    #[test]
    fn wrap_controller_patches_preset_metadata() {
        let src = RealSource::embedded();
        let template = src.controller_template.unwrap();
        let preset = s1state::parse_preset(&s1_cid3()).unwrap();
        let rec = wrap_controller_record(&template, &preset).unwrap();
        let (json, uncomp, format, foff) = serum2state::parse_xfer_json(&rec).unwrap();
        assert!(json.contains("TestPreset"), "{json}");
        assert_eq!(format, 2);
        assert!(uncomp > 0);
        // The zstd frame is kept verbatim from the template (frame offsets
        // differ because the patched JSON header has a different length).
        let (_, _, _, t_foff) = serum2state::parse_xfer_json(&template).unwrap();
        assert_eq!(&rec[foff..], &template[t_foff..]);
        // The hash field is the md5 of that frame.
        assert!(json.contains(&format!("\"hash\":\"{}\"", md5_hex(&template[t_foff..]))));
        // Template version fields preserved.
        assert!(json.contains("\"productVersion\":\"2.0.22\""), "{json}");
        assert!(json.contains("\"version\":8.0"), "{json}");
    }

    #[test]
    fn real_source_bundle_for_end_to_end() {
        let plan = InstancePlan {
            event_index: 0,
            event_offset: 0,
            channel: Some(0),
            channel_name: "Serum".into(),
            plugin_name: "Serum".into(),
            payload: Vec::new(),
            plugin_filename: "/Library/Audio/Plug-Ins/VST3/Serum.vst3".into(),
        };
        let mut src = RealSource::embedded();
        let bundle = src
            .bundle_for(&plan, &s1_cid3())
            .unwrap()
            .expect("bundle for a modern preset");
        assert_eq!(bundle.name, "Serum2");
        assert_eq!(bundle.vendor, "Xfer Records");
        assert_eq!(bundle.filename, "/Library/Audio/Plug-Ins/VST3/Serum2.vst3");
        assert_eq!(bundle.param_list.len(), 10_496);
        assert_eq!(&bundle.processor_record[..9], b"XferJson\0");
        let (json, _, _, _) =
            serum2state::parse_xfer_json(bundle.controller_record.as_ref().unwrap()).unwrap();
        assert!(json.contains("TestPreset"), "{json}");
        assert!(src.warnings.is_empty());
    }

    #[test]
    fn real_source_remembers_old_format_failures() {
        let plan = InstancePlan {
            event_index: 0,
            event_offset: 0,
            channel: Some(3),
            channel_name: "Old".into(),
            plugin_name: "Serum".into(),
            payload: Vec::new(),
            plugin_filename: "Serum_x64.dll".into(),
        };
        let mut src = RealSource::embedded();
        // Not a zlib stream at all -> parse_preset fails -> Ok(None) + warning.
        let res = src.bundle_for(&plan, b"garbage").unwrap();
        assert!(res.is_none());
        assert_eq!(src.warnings.len(), 1);
        assert!(
            src.warnings[0].contains("channel 'Old'"),
            "{}",
            src.warnings[0]
        );
    }
}

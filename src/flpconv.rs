//! FLP rewrite machinery: turning Serum `PluginParams` (event 213) payloads
//! into Serum2 ones.
//!
//! Every byte rule implemented here comes from
//! `docs/flp-serum2-conversion.md` §6 ("Rewrite recipe"). This module only
//! performs byte surgery on the event stream; building the actual Serum2
//! XferJson records (processor / controller / parameter list) is the
//! importer's job, injected through the [`BundleSource`] seam.

use std::collections::HashMap;
use std::ops::Range;

use crate::core::{display_name, text};
use crate::flp;
use crate::fxp;
use crate::s1state;
use crate::s2tree;
use crate::serum;
use crate::serum2preset;
use crate::serum2state;

/// 16-byte Serum2 plugin UID for top-level cid 52 (doc §2.6):
/// ASCII `XESVsfsPerum 2` + 2 NULs.
const SERUM2_UID: [u8; 16] = [
    0x58, 0x45, 0x53, 0x56, 0x73, 0x66, 0x73, 0x50, 0x65, 0x72, 0x75, 0x6D, 0x20, 0x32, 0x00, 0x00,
];

/// Everything needed to turn one Serum instance into a Serum2 instance.
/// Built by the orchestrator (the importer runs elsewhere); flpconv only does
/// byte surgery.
#[derive(Debug, Clone)]
pub struct Serum2Bundle {
    /// Full XferJson processor record (the FL wrapper's inner cid-3 payload).
    pub processor_record: Vec<u8>,
    /// Full XferJson controller record (inner cid-2), or None to omit it.
    pub controller_record: Option<Vec<u8>>,
    /// FL's saved parameter-id list (inner cid-4 payload bytes, lifted from a
    /// genuine Serum2 instance — 10,496 B for 2.0.22).
    pub param_list: Vec<u8>,
    /// Plugin name string for cid 54 ("Serum2").
    pub name: String,
    /// Plugin filename string for cid 55 (e.g. "/Library/Audio/Plug-Ins/VST3/Serum2.vst3").
    pub filename: String,
    /// Vendor string for cid 56 ("Xfer Records").
    pub vendor: String,
}

/// One convertible Serum synth instance, located in the event stream.
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
    /// Preset name recovered while planning (empty when the state could not
    /// be recovered/parsed).
    pub preset_name: String,
    /// The parsed Serum preset, captured during the same scan walk
    /// (`None` when parsing failed; the failure is reported as a warning).
    pub s1: Option<s1state::S1Preset>,
}

/// One successfully rewritten instance, for the human report.
#[derive(Debug, Clone)]
pub struct ConvertedInstance {
    pub channel: Option<u16>,
    pub channel_name: String,
    /// Preset name recovered from the original Serum state (may be empty).
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

/// Produces Serum2 bundles from Serum instances by running the importer.
pub struct RealSource {
    /// FL's saved parameter-id list template (inner cid-4 payload of a real
    /// Serum2 instance, 2.0.22-era).
    pub param_list: Vec<u8>,
    /// Controller record template (inner cid-2 payload of a real Serum2
    /// instance; the JSON header gets patched per preset).
    pub controller_template: Option<Vec<u8>>,
    /// Failure reasons collected for instances that produced `Ok(None)`
    /// (drained by the orchestrator to warn or abort).
    pub warnings: Vec<String>,
}

impl RealSource {
    /// Loads the embedded calibration defaults (real Serum2 2.0.22 instance
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
        // Prefer the preset captured during the planning walk; fall back to
        // parsing `s1_chunk` for plans built without it.
        let fallback;
        let preset = match &plan.s1 {
            Some(p) => p,
            None => {
                fallback = match s1state::parse_preset(s1_chunk) {
                    Ok(p) => p,
                    Err(e) => {
                        self.warnings.push(format!(
                            "instance on channel '{}': {e}",
                            display_name(&plan.channel_name)
                        ));
                        return Ok(None);
                    }
                };
                &fallback
            }
        };
        let converted = crate::importer::convert_s1_to_s2(preset, 0)?;
        let processor_record = serum2state::build_processor_record(&converted.body);
        let controller_record = match &self.controller_template {
            Some(t) => Some(wrap_controller_record(t, preset)?),
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

/// Derive the Serum2 plugin filename from the ORIGINAL instance's filename:
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
        &serum2state::md5_hex(frame),
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

/// Value of a `"key":"string"` field in a flat sorted-key JSON header.
fn json_string_field(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let start = json.find(&pat)? + pat.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

// ---------------------------------------------------------------------------
// Standalone .fxp -> Serum2 preset conversion
// ---------------------------------------------------------------------------

/// One successfully converted standalone Serum `.fxp` (see
/// [`convert_fxp_bytes`]).
#[derive(Debug, Clone)]
pub struct ConvertedFxp {
    pub preset_name: String,
    pub author: String,
    pub category: String,
    pub version_f32: f32,
    /// Decompressed Serum preset-state size on the input side.
    pub state_size: usize,
    /// Number of zlib streams in the chunk (1 = state, +1 per embedded
    /// wavetable/noise stream).
    pub stream_count: usize,
    /// Full XferJson processor record — byte-identical to the inner cid-3
    /// payload the FLP flow embeds for the same preset.
    pub processor_record: Vec<u8>,
    /// `.SerumPreset` container (EXPERIMENTAL output, see
    /// `serum2preset`): the processor-state CBOR body wrapped in the
    /// preset-style container, not the authored preset format.
    pub serum_preset: Vec<u8>,
    /// Uncompressed CBOR body length (declared by both containers).
    pub body_cbor_len: usize,
    /// Non-fatal notes: fxp container warnings + importer conversion notes.
    pub notes: Vec<String>,
}

/// Convert one standalone Serum `.fxp` preset into a Serum2 preset.
///
/// Reuses the exact pieces of the FLP conversion pipeline: chunk extraction
/// (`serum::serum1_chunk_from_state`, i.e. chunkSize BE32@0x38 +
/// `file[0x3C..0x3C+cs]`), `s1state::parse_preset`,
/// `importer::convert_s1_to_s2(preset, 0)` (the same call the FLP flow's
/// `RealSource` makes) and `serum2state::build_processor_record`. The
/// produced processor record is therefore byte-identical to the one the FLP
/// flow embeds for the same preset.
///
/// The `.SerumPreset` container wraps the SAME zstd frame (the converted
/// processor-state CBOR body, as-is) with a preset-style JSON header. This is
/// the processor-state variant of the container, not Serum2's authored preset
/// format — see `serum2preset` and docs/flp-conversion.md ("convert-fxp").
pub fn convert_fxp_bytes(fxp: &[u8]) -> Result<ConvertedFxp, String> {
    // 1. Container validation — the same rules `validate` reports and the
    //    Serum2 importer enforces; fatals abort, warnings become notes.
    let report = fxp::validate_fxp(fxp);
    let mut notes: Vec<String> = report.warnings().map(str::to_string).collect();
    if !report.is_ok() {
        let mut msg = String::from("fxp failed Serum2 import validation");
        for f in report.fatals() {
            msg.push_str(&format!("\n  {f}"));
        }
        return Err(msg);
    }

    // 2. Chunk extraction + metadata (shared with the FLP flow).
    let chunk = serum::serum1_chunk_from_state(fxp)?;
    let preset = s1state::parse_preset(&chunk.chunk)?;
    let meta = preset.meta.clone();

    // 3. Conversion (flag 0 = synth import, same as the FLP flow).
    let converted = crate::importer::convert_s1_to_s2(&preset, 0)?;
    notes.extend(converted.report.notes.iter().cloned());

    // 4. Processor record + self-check (md5/size sanity).
    let processor_record = serum2state::build_processor_record(&converted.body);
    let (json, uncomp, format, frame_start) = serum2state::parse_xfer_json(&processor_record)?;
    let frame = &processor_record[frame_start..];
    if format != 2 {
        return Err(format!("processor record format is {format}, expected 2"));
    }
    let frame_body = s2tree::zstd_frame_body_len(frame).unwrap_or(0);
    if frame_body != uncomp as usize {
        return Err(format!(
            "processor record body length mismatch (frame {frame_body} B, declared {uncomp} B)"
        ));
    }
    if !json.contains(&format!("\"hash\":\"{}\"", serum2state::md5_hex(frame))) {
        return Err("processor record hash does not match its frame".into());
    }

    // 5. `.SerumPreset` container: same frame, preset-style header.
    //    Voicing tag from Global0's mono toggle (badge is always "Wavetable"
    //    for Serum presets; the factory corpus uses [badge, voicing, ...]).
    let mono = converted
        .body
        .get("Global0")
        .and_then(|g| g.get("plainParams"))
        .and_then(|p| p.get("kParamMonoToggle"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        != 0.0;
    let tags: Vec<String> = vec![
        "Wavetable".into(),
        if mono { "Mono".into() } else { "Poly".into() },
    ];
    let serum_preset = serum2preset::build_preset_container(
        frame,
        uncomp,
        &meta.preset_name,
        &meta.author,
        &meta.category,
        &tags,
    );

    Ok(ConvertedFxp {
        preset_name: meta.preset_name,
        author: meta.author,
        category: meta.category,
        version_f32: meta.version_f32,
        state_size: preset.blob.len(),
        stream_count: chunk.stream_sizes.len(),
        processor_record,
        serum_preset,
        body_cbor_len: uncomp as usize,
        notes,
    })
}

/// Raw (unquoted) value of a `"key":<token>` field, up to `,` or `}`.
fn json_raw_field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let start = json.find(&pat)? + pat.len();
    let rest = &json[start..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    Some(rest[..end].trim())
}

/// Walk the FLP events and locate every Serum SYNTH instance, also
/// returning warnings (one per Serum FX instance deliberately left
/// untouched). Instances are returned in file order, matching
/// `scan_serum_instances`' channel/name bookkeeping.
pub fn scan_convertible_detailed(buf: &[u8]) -> Result<(Vec<InstancePlan>, Vec<String>), String> {
    let spans = flp::parse_event_spans(buf)?;
    let dt_start = locate_chunks(buf)?.2;

    let mut channels: HashMap<u16, String> = HashMap::new();
    let mut cur_channel: Option<u16> = None;
    let mut cur_fx_name = String::new();
    let mut plans = Vec::new();
    let mut warnings = Vec::new();

    for (i, (ev_off, ev)) in spans.iter().enumerate() {
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
                if serum::is_serum_fx(pp.name, pp.filename) {
                    warnings.push(format!(
                        "Serum FX instance on channel '{where_}' left untouched (only the Serum synth is converted)"
                    ));
                    continue;
                }
                if !serum::is_serum1_synth(pp.name, pp.filename) || pp.state.is_empty() {
                    continue;
                }
                // Single-walk: recover the cid-3 chunk and parse the preset
                // here so bundling never has to re-inflate the state. A
                // parse failure yields None + a warning (never aborts).
                let (preset_name, s1) = match serum::serum1_chunk_from_state(pp.state) {
                    Ok(chunk) => match s1state::parse_preset(&chunk.chunk) {
                        Ok(preset) => (chunk.meta.preset_name, Some(preset)),
                        Err(e) => {
                            warnings.push(format!(
                                "instance on channel '{}': {e}",
                                display_name(&where_)
                            ));
                            (chunk.meta.preset_name, None)
                        }
                    },
                    Err(e) => {
                        warnings.push(format!(
                            "instance on channel '{}': {e}",
                            display_name(&where_)
                        ));
                        (String::new(), None)
                    }
                };
                plans.push(InstancePlan {
                    event_index: i,
                    event_offset: ev_off - dt_start,
                    channel: cur_channel,
                    channel_name: where_,
                    plugin_name: text(pp.name),
                    payload: ev.data.to_vec(),
                    plugin_filename: String::from_utf8_lossy(pp.filename).into_owned(),
                    preset_name,
                    s1,
                });
            }
            _ => {}
        }
    }
    Ok((plans, warnings))
}

/// Byte-surgery application: for every (InstancePlan, Serum2Bundle) pair with
/// `Some(bundle)`, replace that event's payload with the rebuilt Serum2
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
    let spans = flp::parse_event_spans(buf)?;
    for (i, (plan, _)) in plans.iter().zip(bundles).enumerate() {
        let Some((_, ev)) = spans.get(plan.event_index) else {
            return Err(format!(
                "plan {i} references event {} but the file has {} events",
                plan.event_index,
                spans.len()
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

    // 2. Build the replacement payloads.
    let mut replacements: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut report = FlpConversionReport::default();
    for (i, (plan, bundle)) in plans.iter().zip(bundles).enumerate() {
        let Some(b) = bundle else {
            report.warnings.push(format!(
                "instance on channel '{}' skipped (no Serum2 bundle supplied)",
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
            preset_name: plan.preset_name.clone(),
            new_payload_len: replacements[&plan.event_index].len(),
        });
    }

    // 3. Splice: everything before the FLdt payload verbatim, then re-emit
    //    each event (replaced ones with fresh framing).
    let mut out: Vec<u8> = Vec::with_capacity(buf.len() + 4096);
    out.extend_from_slice(&buf[..dt_start]);
    for (i, (off, ev)) in spans.iter().enumerate() {
        match replacements.get(&i) {
            Some(new_payload) => {
                out.push(flp::EV_PLUGIN_PARAMS);
                push_varint(&mut out, new_payload.len());
                out.extend_from_slice(new_payload);
            }
            None => {
                let pstart = ev.data.as_ptr() as usize - buf.as_ptr() as usize;
                out.extend_from_slice(&buf[*off..pstart + ev.data.len()]);
            }
        }
    }

    // 4. Fix the FLdt u32 LE chunk length.
    let new_dtlen = out.len() - dt_start;
    out[dt_len_pos..dt_len_pos + 4].copy_from_slice(&(new_dtlen as u32).to_le_bytes());
    Ok((out, report))
}

// ---------------------------------------------------------------------------
// Metadata patching (Serum instances inside an FLP)
// ---------------------------------------------------------------------------

/// One patched Serum instance, for the human report.
#[derive(Debug, Clone)]
pub struct PatchedInstance {
    pub channel: Option<u16>,
    pub channel_name: String,
    /// Preset name before the patch (may be empty when unreadable).
    pub old_preset_name: String,
    /// Preset name after the patch (equals the old one when no Name patch).
    pub new_preset_name: String,
}

/// Result of [`patch_serum_metadata`]: what was rewritten plus non-fatal notes.
#[derive(Debug, Default)]
pub struct FlpPatchReport {
    pub patched: Vec<PatchedInstance>,
    pub warnings: Vec<String>,
}

/// Patch metadata in every Serum synth instance of an FLP, in memory.
///
/// For each Serum instance the inner cid-3 preset chunk is inflated, the
/// fields are rewritten ([`fxp::patch_chunk_fields`]) and the chunk is
/// spliced back with fresh record framing and a fixed FLdt u32 length.
/// Non-plugin events and everything before the FLdt payload stay
/// byte-identical. Instances whose state cannot be patched are skipped with
/// a warning (never aborts).
pub fn patch_serum_metadata(
    buf: &[u8],
    patches: &[fxp::PatchField],
) -> Result<(Vec<u8>, FlpPatchReport), String> {
    if patches.is_empty() {
        return Err("nothing to patch (empty patch list)".into());
    }
    let spans = flp::parse_event_spans(buf)?;
    let (_, dt_len_pos, dt_start, _) = locate_chunks(buf)?;

    // Channel bookkeeping identical to scan_convertible_detailed.
    let mut channels: HashMap<u16, String> = HashMap::new();
    let mut cur_channel: Option<u16> = None;
    let mut cur_fx_name = String::new();
    let mut replacements: HashMap<usize, Vec<u8>> = HashMap::new();
    let mut report = FlpPatchReport::default();

    for (i, (_off, ev)) in spans.iter().enumerate() {
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
                if serum::is_serum_fx(pp.name, pp.filename) {
                    report.warnings.push(format!(
                        "Serum FX instance on channel '{where_}' left untouched"
                    ));
                    continue;
                }
                if !serum::is_serum1_synth(pp.name, pp.filename) || pp.state.is_empty() {
                    continue;
                }
                let old_name = serum::serum1_chunk_from_state(pp.state)
                    .map(|c| c.meta.preset_name)
                    .unwrap_or_default();
                let new_payload = match patch_event_payload(ev.data, patches) {
                    Ok(p) => p,
                    Err(e) => {
                        report.warnings.push(format!(
                            "instance on channel '{}': {e}",
                            display_name(&where_)
                        ));
                        continue;
                    }
                };
                replacements.insert(i, new_payload);
                let new_name = match patches.iter().rev().find_map(|p| match p {
                    fxp::PatchField::Name(s) => Some(s.clone()),
                    _ => None,
                }) {
                    Some(s) => {
                        let mut b = s.as_bytes().to_vec();
                        b.truncate(31);
                        while std::str::from_utf8(&b).is_err() {
                            b.pop();
                        }
                        String::from_utf8_lossy(&b).into_owned()
                    }
                    None => old_name.clone(),
                };
                report.patched.push(PatchedInstance {
                    channel: cur_channel,
                    channel_name: where_,
                    old_preset_name: old_name,
                    new_preset_name: new_name,
                });
            }
            _ => {}
        }
    }

    // Splice: everything before the FLdt payload verbatim, re-emit each
    // event (replaced ones with fresh framing), then fix the FLdt length.
    let mut out: Vec<u8> = Vec::with_capacity(buf.len() + 4096);
    out.extend_from_slice(&buf[..dt_start]);
    for (i, (_off, ev)) in spans.iter().enumerate() {
        match replacements.get(&i) {
            Some(new_payload) => {
                out.push(flp::EV_PLUGIN_PARAMS);
                push_varint(&mut out, new_payload.len());
                out.extend_from_slice(new_payload);
            }
            None => {
                let pstart = ev.data.as_ptr() as usize - buf.as_ptr() as usize;
                out.extend_from_slice(&buf[*_off..pstart + ev.data.len()]);
            }
        }
    }
    let new_dtlen = out.len() - dt_start;
    out[dt_len_pos..dt_len_pos + 4].copy_from_slice(&(new_dtlen as u32).to_le_bytes());
    Ok((out, report))
}

/// Replace the inner cid-3 chunk of a PluginParams payload with its patched
/// version; every other record and the record order stay verbatim.
fn patch_event_payload(orig: &[u8], patches: &[fxp::PatchField]) -> Result<Vec<u8>, String> {
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
    let Some(cid53) = record_data(orig, &recs, 53) else {
        return Err("missing cid 53 record".into());
    };
    // The FL VST3 wrapper is [u32 prologue][records...] — find the record
    // start the same way serum::fl_vst3_wrapper_cid3 does.
    let mut wstart = None;
    for s in 0..=8usize {
        if cid53.len() >= s && parse_record_seq(cid53, s).is_ok() {
            wstart = Some(s);
            break;
        }
    }
    let Some(wstart) = wstart else {
        return Err("cid 53 wrapper records do not parse".into());
    };
    let wrecs = parse_record_seq(cid53, wstart)?;
    let Some(inner3) = record_data(cid53, &wrecs, 3) else {
        return Err("missing inner cid 3 record".into());
    };
    let new_cid3 = fxp::patch_chunk_fields(inner3, patches)?;
    let mut w = Vec::with_capacity(wstart + cid53.len() + new_cid3.len());
    w.extend_from_slice(&cid53[..wstart]);
    for r in &wrecs {
        let data = if r.cid == 3 {
            &new_cid3
        } else {
            &cid53[r.data.clone()]
        };
        push_rec(&mut w, r.cid, data);
    }
    let mut p = Vec::with_capacity(orig.len() + new_cid3.len());
    p.extend_from_slice(&version.to_le_bytes());
    for r in &recs {
        let data = if r.cid == 53 {
            &w
        } else {
            &orig[r.data.clone()]
        };
        push_rec(&mut p, r.cid, data);
    }
    Ok(p)
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

/// Parse a `[u32 cid][u64 size][data]` record sequence starting at `start`.
fn parse_record_seq(buf: &[u8], start: usize) -> Result<Vec<TopRecord>, String> {
    let mut recs = Vec::new();
    let mut it = flp::Records::new(&buf[start..]);
    while let Some((cid, data)) = it.next() {
        let dstart = it.pos() - data.len();
        recs.push(TopRecord {
            cid,
            data: start + dstart..start + it.pos(),
        });
    }
    if it.overran() {
        return Err(format!(
            "record payload overruns the record sequence at offset {:#x}",
            start + it.pos()
        ));
    }
    if it.pos() != buf.len() - start {
        return Err(format!(
            "truncated record header at offset {:#x}",
            start + it.pos()
        ));
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

/// Build the new Serum2 event-213 payload from the original Serum payload
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flp::{EV_NEW_CHANNEL, EV_TEXT_CHANNEL_NAME};
    use crate::testutil::{build_flp, zlib_stream};

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

    /// Real-shaped Serum inner cid 3: zlib preset state (preset name at
    /// 0x4972) + a second wavetable stream + u32 LE trailer (doc §3.3).
    fn s1_cid3() -> Vec<u8> {
        let mut s0 = vec![0u8; serum::SERUM1_STATE_SIZE];
        s0[serum::OFF_PRESET_NAME..serum::OFF_PRESET_NAME + 11].copy_from_slice(b"TestPreset\0");
        s0[serum::OFF_VERSION_F32..serum::OFF_VERSION_F32 + 4]
            .copy_from_slice(&0.1631f32.to_le_bytes());
        let z0 = zlib_stream(&s0);
        let z1 = zlib_stream(&[0u8; 8192]);
        let mut v = z0.clone();
        v.extend_from_slice(&z1);
        v.extend_from_slice(&(z0.len() as u32).to_le_bytes());
        v
    }

    fn top_rec(cid: u32, data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        push_rec(&mut v, cid, data);
        v
    }

    /// A realistic Serum event-213 payload (doc §2.1 / §3).
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
        let plans = scan_convertible_detailed(&buf).unwrap().0;
        assert_eq!(plans.len(), 1);
        let p = &plans[0];
        assert_eq!(p.event_index, 2);
        assert_eq!(
            p.event_offset,
            flp::parse_event_spans(&buf).unwrap()[2].0 - 22
        );
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
        let plans = scan_convertible_detailed(&buf).unwrap().0;
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
        let plans = scan_convertible_detailed(&buf).unwrap().0;
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
        let plans = scan_convertible_detailed(&buf).unwrap().0;
        let (out, rep) = apply(&buf, &plans, &[Some(test_bundle(140))]).unwrap();
        let evs = flp::parse_events(&out).unwrap();
        assert!(evs[2].data.len() >= 128);
        assert!(evs[2].data.len() < 16384);
        assert_eq!(evs[2].data.len(), rep.converted[0].new_payload_len);
        // Varint byte check: 0xD5 then two bytes with the continuation bit.
        let off = flp::parse_event_spans(&out).unwrap()[2].0;
        assert_eq!(out[off], 0xD5);
        assert_eq!(out[off + 1] & 0x80, 0x80);
        assert_eq!(out[off + 2] & 0x80, 0x00);
        assert_eq!(fldt_len(&out), out.len() - 22);
    }

    #[test]
    fn varint_reframing_three_byte() {
        // 16384+ payload bytes -> 3-byte varint (doc §1).
        let buf = sample_flp();
        let plans = scan_convertible_detailed(&buf).unwrap().0;
        let (out, _) = apply(&buf, &plans, &[Some(test_bundle(20_000))]).unwrap();
        let evs = flp::parse_events(&out).unwrap();
        assert!(evs[2].data.len() >= 16384);
        let off = flp::parse_event_spans(&out).unwrap()[2].0;
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
        // Also visible via scan_serum_instances' bookkeeping: it IS Serum
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
        assert!(scan_convertible_detailed(&buf).is_err());
        assert!(apply(&buf, &[], &[]).is_err());
    }

    #[test]
    fn rejects_malformed_fldt() {
        // Missing FLdt entirely.
        let mut buf = b"FLhd".to_vec();
        buf.extend_from_slice(&6u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 6]);
        assert!(scan_convertible_detailed(&buf).is_err());
        // Event overruns the declared FLdt chunk (id 0xD5 + varint claiming
        // far more payload bytes than the declared chunk holds).
        let mut buf3 = build_flp(&[]);
        buf3[18..22].copy_from_slice(&32u32.to_le_bytes());
        buf3.extend_from_slice(&[0xD5, 0xE4, 0x07, 1, 2, 3]); // varint = 100
        assert!(scan_convertible_detailed(&buf3).is_err());
        assert!(apply(&buf3, &[], &[]).is_err());
    }

    #[test]
    fn apply_rejects_stale_plans() {
        let buf = sample_flp();
        let plans = scan_convertible_detailed(&buf).unwrap().0;
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
        let plans = scan_convertible_detailed(&buf).unwrap().0;
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
        let plans = scan_convertible_detailed(&buf).unwrap().0;
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
        assert!(json.contains(&format!(
            "\"hash\":\"{}\"",
            serum2state::md5_hex(&template[t_foff..])
        )));
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
            preset_name: String::new(),
            s1: None,
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
            preset_name: String::new(),
            s1: None,
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

    // -----------------------------------------------------------------------
    // convert_fxp_bytes (standalone .fxp -> Serum2 preset)
    // -----------------------------------------------------------------------

    fn fixture_base() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn read_fixture(nn: u8, what: &str) -> Option<Vec<u8>> {
        match std::fs::read(fixture_base().join(what.replace("{nn}", &format!("0{nn}")))) {
            Ok(b) => Some(b),
            Err(_) => {
                eprintln!(
                    "skipping preset {nn}: untracked fixtures absent ({what}; docs/flp-conversion.md)"
                );
                None
            }
        }
    }

    #[test]
    fn convert_fxp_matches_flp_flow_processor_record() {
        let Some(flp) = read_fixture(0, "serina1.flp") else {
            return;
        };
        let (plans, _) = scan_convertible_detailed(&flp).unwrap();
        assert_eq!(plans.len(), 5);
        let mut source = RealSource::embedded();
        let mut flow: HashMap<String, Vec<u8>> = HashMap::new();
        for plan in &plans {
            let bundle = source
                .bundle_for(plan, &[])
                .unwrap()
                .expect("bundle for a modern preset");
            flow.insert(plan.preset_name.clone(), bundle.processor_record);
        }
        for nn in 1..=5u8 {
            let Some(bytes) = read_fixture(nn, "serina1/{nn}.fxp") else {
                continue;
            };
            let c = convert_fxp_bytes(&bytes).unwrap_or_else(|e| panic!("preset {nn}: {e}"));
            let rec = flow
                .get(&c.preset_name)
                .unwrap_or_else(|| panic!("preset '{}' missing from the FLP flow", c.preset_name));
            assert_eq!(
                &c.processor_record, rec,
                "preset {nn} ({}) processor record differs from the FLP flow",
                c.preset_name
            );
        }
    }

    #[test]
    fn convert_fxp_container_parse_back_and_golden_body() {
        for nn in 1..=5u8 {
            let Some(bytes) = read_fixture(nn, "serina1/{nn}.fxp") else {
                continue;
            };
            let c = convert_fxp_bytes(&bytes).unwrap_or_else(|e| panic!("preset {nn}: {e}"));
            assert!(c.notes.is_empty(), "preset {nn}: {:?}", c.notes);

            // Container parse-back: header fields + hash == md5(frame).
            let (json, uncomp, format, foff) =
                serum2state::parse_xfer_json(&c.serum_preset).unwrap();
            assert_eq!(format, 2);
            assert_eq!(uncomp as usize, c.body_cbor_len);
            assert!(json.contains("\"fileType\":\"SerumPreset\""), "{json}");
            assert!(json.contains("\"product\":\"Serum2\""), "{json}");
            assert!(json.contains("\"version\":9.0"), "{json}");
            assert!(
                json.contains(&format!("\"presetName\":\"{}\"", c.preset_name)),
                "{json}"
            );
            let frame = &c.serum_preset[foff..];
            assert_eq!(
                json.contains(&format!("\"hash\":\"{}\"", serum2state::md5_hex(frame))),
                true,
                "{json}"
            );

            // The preset body IS the processor record's frame, as-is.
            let (_, _, _, proc_foff) = serum2state::parse_xfer_json(&c.processor_record).unwrap();
            assert_eq!(frame, &c.processor_record[proc_foff..]);

            // And its decoded CBOR equals the REAL importer's golden body.
            if let Some(golden) = read_fixture(nn, "golden_s2/{nn}_processor_state.bin") {
                let (_, _, _, gfoff) = serum2state::parse_xfer_json(&golden).unwrap();
                assert_eq!(
                    crate::testutil::decode_zstd_frame(frame),
                    crate::testutil::decode_zstd_frame(&golden[gfoff..]),
                    "preset {nn} body differs from the real importer golden"
                );
    /// Pull the inner cid-3 chunk out of a PluginParams payload (test-local).
    fn inner_cid3(payload: &[u8]) -> Vec<u8> {
        let recs = records_of(payload, 4);
        let w = &recs.iter().find(|r| r.0 == 53).unwrap().1;
        records_of(w, 4).into_iter().find(|r| r.0 == 3).unwrap().1
    }

    #[test]
    fn patch_flp_updates_state_and_report() {
        let buf = sample_flp();
        let (out, rep) = patch_serum_metadata(
            &buf,
            &[
                fxp::PatchField::Name("Patched Name".into()),
                fxp::PatchField::Author("Patched Author".into()),
            ],
        )
        .unwrap();

        assert_eq!(rep.patched.len(), 1);
        assert_eq!(rep.patched[0].channel_name, "Serum");
        assert_eq!(rep.patched[0].old_preset_name, "TestPreset");
        assert_eq!(rep.patched[0].new_preset_name, "Patched Name");
        assert!(rep.warnings.is_empty());

        // FLdt length correct, header prefix untouched.
        assert_eq!(fldt_len(&out), out.len() - 22);
        assert_eq!(&out[..18], &buf[..18]);

        // Non-plugin events byte-identical.
        let evs_old = flp::parse_events(&buf).unwrap();
        let evs_new = flp::parse_events(&out).unwrap();
        assert_eq!(evs_old.len(), evs_new.len());
        for (o, n) in evs_old.iter().zip(&evs_new) {
            if o.id == flp::EV_PLUGIN_PARAMS {
                continue;
            }
            assert_eq!((o.id, &o.data), (n.id, &n.data));
        }

        // The patched state carries the new metadata.
        let state = crate::zlibio::inflate(&inner_cid3(evs_new[2].data), 1 << 20)
            .unwrap()
            .0;
        let field = |off: usize, len: usize| {
            String::from_utf8_lossy(&state[off..off + len])
                .trim_end_matches('\0')
                .to_string()
        };
        assert_eq!(field(serum::OFF_PRESET_NAME, 32), "Patched Name");
        assert_eq!(field(serum::OFF_AUTHOR, 48), "Patched Author");
        assert_eq!(field(serum::OFF_CATEGORY, 48), "");
    }

    #[test]
    fn patch_flp_keeps_other_records_byte_identical() {
        let buf = sample_flp();
        let (out, _) =
            patch_serum_metadata(&buf, &[fxp::PatchField::Category("Cat".into())]).unwrap();
        let old = flp::parse_events(&buf).unwrap();
        let new = flp::parse_events(&out).unwrap();
        let po = records_of(old[2].data, 4);
        let pn = records_of(new[2].data, 4);
        // Same record sequence; only cid 53 differs (its inner cid-3 chunk
        // was rebuilt).
        assert_eq!(
            po.iter().map(|r| r.0).collect::<Vec<_>>(),
            pn.iter().map(|r| r.0).collect::<Vec<_>>()
        );
        for (o, n) in po.iter().zip(&pn) {
            if o.0 == 53 {
                // Inside the wrapper, only the inner cid-3 chunk differs.
                let wo = records_of(&o.1, 4);
                let wn = records_of(&n.1, 4);
                assert_eq!(
                    wo.iter().map(|r| r.0).collect::<Vec<_>>(),
                    wn.iter().map(|r| r.0).collect::<Vec<_>>()
                );
                for (a, b) in wo.iter().zip(&wn) {
                    if a.0 == 3 {
                        continue;
                    }
                    assert_eq!(a.1, b.1, "wrapper cid {} changed", a.0);
                }
            } else {
                assert_eq!(o.1, n.1, "record cid {} changed", o.0);

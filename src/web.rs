//! WebAssembly bindings for the browser ([`wasm-bindgen`]).
//!
//! This module is only compiled for `wasm32-unknown-unknown` (same cfg gate
//! as in `lib.rs`), so native CLI builds never reference it. It mirrors the
//! CLI operations:
//!
//! - [`scan_flp_report`]: scan an FLP for Serum instances (one-shot).
//! - [`scan_doc`]: the session variant — keeps every preset's raw chunk in
//!   an [`FlpDoc`] so the browser can assemble the `.fxp` bytes for any
//!   preset via [`FlpDoc::build_fxp`] without re-scanning the document.
//! - [`convert_flp`]: rewrite every Serum synth instance in place as a
//!   Serum2 instance and return the converted FLP bytes plus a report.
//! - [`convert_flp_selected`]: same, but only for the instances selected
//!   by preset-table row index; unselected instances stay byte-identical.
//!
//! Per-instance conversion failures never abort a scan; they are reported
//! as a JSON string array via [`ScanReport::failed_json`]. Duplicates are
//! still returned (marked with `duplicate`) and can be deduped client-side
//! with the deterministic `content_hash` string.
//!
//! Field access from JavaScript uses the generated camelCase getters
//! (`presetName`, `versionF32`, `contentHash`, `warningsJson`, ...).

use std::collections::HashSet;

use wasm_bindgen::prelude::*;

use crate::core::{self, Instance};
use crate::flpconv::{self, BundleSource};
use crate::fxp;
use crate::serum;

/// Escape a string for embedding inside a JSON document.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Serialize strings as a JSON array.
fn json_string_array<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let parts: Vec<String> = items
        .into_iter()
        .map(|s| format!("\"{}\"", json_escape(s)))
        .collect();
    format!("[{}]", parts.join(","))
}

/// One Serum instance found by a scan, in file order.
///
/// Every field is readable from JavaScript through a generated camelCase
/// getter; `warnings_json` / `errors_json` are already-serialized JSON
/// string arrays so the UI never has to recompute them.
#[wasm_bindgen]
#[derive(Clone)]
pub struct WasmPreset {
    index: u32,
    channel: String,
    channel_name: String,
    plugin_name: String,
    preset_name: String,
    author: String,
    category: String,
    version_f32: f32,
    state_bytes: u32,
    chunk_bytes: u32,
    source: String,
    duplicate: bool,
    content_hash: String,
    has_warnings: bool,
    valid: bool,
    warnings_json: String,
    errors_json: String,
}

#[wasm_bindgen]
impl WasmPreset {
    /// Zero-based position in the scan order; pass this to
    /// [`FlpDoc::build_fxp`].
    #[wasm_bindgen(getter)]
    pub fn index(&self) -> u32 {
        self.index
    }

    /// FL Studio channel number as a decimal string (`""` when unknown).
    #[wasm_bindgen(getter)]
    pub fn channel(&self) -> String {
        self.channel.clone()
    }

    /// Channel name (falls back to the FX track name).
    #[wasm_bindgen(getter)]
    pub fn channel_name(&self) -> String {
        self.channel_name.clone()
    }

    /// Plugin display name as stored in the FLP.
    #[wasm_bindgen(getter)]
    pub fn plugin_name(&self) -> String {
        self.plugin_name.clone()
    }

    /// Preset name embedded in the Serum state (may be empty).
    #[wasm_bindgen(getter)]
    pub fn preset_name(&self) -> String {
        self.preset_name.clone()
    }

    /// Author string embedded in the preset state.
    #[wasm_bindgen(getter)]
    pub fn author(&self) -> String {
        self.author.clone()
    }

    /// Category string embedded in the preset state.
    #[wasm_bindgen(getter)]
    pub fn category(&self) -> String {
        self.category.clone()
    }

    /// Preset-format version float (e.g. `0.1631`).
    #[wasm_bindgen(getter)]
    pub fn version_f32(&self) -> f32 {
        self.version_f32
    }

    /// Decompressed size of the first (preset state) zlib stream, in bytes.
    #[wasm_bindgen(getter)]
    pub fn state_bytes(&self) -> u32 {
        self.state_bytes
    }

    /// Total size of the raw chunk payload embedded in the `.fxp`.
    #[wasm_bindgen(getter)]
    pub fn chunk_bytes(&self) -> u32 {
        self.chunk_bytes
    }

    /// How the chunk was recovered (`FlVst3Wrapper`, `RawZlib`,
    /// `CcnKFxPreset`, `VstWFxPreset`).
    #[wasm_bindgen(getter)]
    pub fn source(&self) -> String {
        self.source.clone()
    }

    /// True when an earlier instance had an identical chunk (dedupe
    /// client-side with `content_hash`).
    #[wasm_bindgen(getter)]
    pub fn duplicate(&self) -> bool {
        self.duplicate
    }

    /// Deterministic content hash of the chunk as a decimal string.
    #[wasm_bindgen(getter)]
    pub fn content_hash(&self) -> String {
        self.content_hash.clone()
    }

    /// True when the preset produced Serum2 warnings (imports, but note).
    #[wasm_bindgen(getter)]
    pub fn has_warnings(&self) -> bool {
        self.has_warnings
    }

    /// True when all Serum2 import checks pass (no fatal issues).
    #[wasm_bindgen(getter)]
    pub fn valid(&self) -> bool {
        self.valid
    }

    /// Warnings as an already-serialized JSON string array.
    #[wasm_bindgen(getter)]
    pub fn warnings_json(&self) -> String {
        self.warnings_json.clone()
    }

    /// Fatal issues as an already-serialized JSON string array.
    #[wasm_bindgen(getter)]
    pub fn errors_json(&self) -> String {
        self.errors_json.clone()
    }
}

impl WasmPreset {
    /// Build the JS-facing view of one instance (running the Serum2
    /// validation once, here, so both `valid` and the JSON arrays share it).
    fn from_instance(index: usize, inst: &Instance, duplicate: bool) -> WasmPreset {
        let report = fxp::validate_chunk_report(&inst.chunk.chunk);
        let warnings: Vec<String> = report.warnings().map(|w| w.to_string()).collect();
        let errors: Vec<String> = report.fatals().map(|e| e.to_string()).collect();
        WasmPreset {
            index: index as u32,
            channel: inst.channel.map(|c| c.to_string()).unwrap_or_default(),
            channel_name: inst.channel_name.clone(),
            plugin_name: inst.plugin_name.clone(),
            preset_name: inst.chunk.meta.preset_name.clone(),
            author: inst.chunk.meta.author.clone(),
            category: inst.chunk.meta.category.clone(),
            version_f32: inst.chunk.meta.version_f32,
            state_bytes: inst.chunk.stream_sizes.first().copied().unwrap_or(0) as u32,
            chunk_bytes: inst.chunk.chunk.len() as u32,
            source: format!("{:?}", inst.chunk.source),
            duplicate,
            content_hash: core::hash_bytes(&inst.chunk.chunk).to_string(),
            has_warnings: !warnings.is_empty(),
            valid: report.is_ok(),
            warnings_json: json_string_array(warnings.iter().map(|s| s.as_str())),
            errors_json: json_string_array(errors.iter().map(|s| s.as_str())),
        }
    }
}

/// Full scan result: the presets plus per-scan diagnostics that have no
/// per-preset home (failed conversions, skipped Serum2 instances).
#[wasm_bindgen]
pub struct ScanReport {
    presets: Vec<WasmPreset>,
    failed_json: String,
    serum2_skipped: u32,
}

#[wasm_bindgen]
impl ScanReport {
    /// All instances as a JS array (clone of the internal list).
    pub fn presets(&self) -> Vec<WasmPreset> {
        self.presets.clone()
    }

    /// Failed per-instance conversions as an already-serialized JSON string
    /// array of human-readable messages (empty array when none).
    #[wasm_bindgen(getter)]
    pub fn failed_json(&self) -> String {
        self.failed_json.clone()
    }

    /// Number of Serum2 instances skipped during the scan.
    #[wasm_bindgen(getter)]
    pub fn serum2_skipped(&self) -> u32 {
        self.serum2_skipped
    }
}

/// Shared scan internals behind [`scan_flp_report`] and [`scan_doc`]: the
/// JS-facing preset views in file order, each preset's raw Serum chunk
/// (same order), and the scan-wide diagnostics.
struct ScanPieces {
    presets: Vec<WasmPreset>,
    chunks: Vec<Vec<u8>>,
    failed_json: String,
    serum2_skipped: u32,
}

/// Shared scan implementation: one walk over the FLP, shared by
/// [`scan_flp_report`] and [`scan_doc`] so both produce identical
/// [`WasmPreset`] lists.
fn scan_impl(data: &[u8]) -> Result<ScanPieces, JsValue> {
    let (instances, stats) = core::scan_serum_instances(data).map_err(|e| JsValue::from_str(&e))?;
    let mut seen: HashSet<u64> = HashSet::new();
    let mut presets = Vec::with_capacity(instances.len());
    let mut chunks = Vec::with_capacity(instances.len());
    for (i, inst) in instances.iter().enumerate() {
        let duplicate = !seen.insert(core::hash_bytes(&inst.chunk.chunk));
        presets.push(WasmPreset::from_instance(i, inst, duplicate));
        chunks.push(inst.chunk.chunk.clone());
    }
    Ok(ScanPieces {
        presets,
        chunks,
        failed_json: json_string_array(stats.failed.iter().map(|s| s.as_str())),
        serum2_skipped: stats.serum2_count as u32,
    })
}

/// Scan an FLP file for Serum plugin instances and return a
/// [`ScanReport`].
///
/// `data` are the raw `.flp` bytes. Duplicates of an earlier identical
/// chunk are still returned but flagged with `duplicate == true` (dedupe
/// client-side by `content_hash`). Unparseable input (zip archive, missing
/// `FLhd`, ...) rejects with a human-readable message; per-instance
/// conversion failures are not fatal (see [`ScanReport::failed_json`]).
#[wasm_bindgen]
pub fn scan_flp_report(data: &[u8]) -> Result<ScanReport, JsValue> {
    let scan = scan_impl(data)?;
    Ok(ScanReport {
        presets: scan.presets,
        failed_json: scan.failed_json,
        serum2_skipped: scan.serum2_skipped,
    })
}

/// A scanned FLP document: the Serum presets plus their raw chunks, kept
/// for the session so the browser can assemble `.fxp` bytes for any preset
/// in O(1) instead of re-scanning the whole file per download.
///
/// [`FlpDoc::presets`] returns the same views as [`ScanReport::presets`],
/// and [`FlpDoc::build_fxp`] indexes both lists identically.
#[wasm_bindgen]
pub struct FlpDoc {
    presets: Vec<WasmPreset>,
    chunks: Vec<Vec<u8>>,
    failed_json: String,
    serum2_skipped: u32,
}

#[wasm_bindgen]
impl FlpDoc {
    /// All instances as a JS array (clone of the internal list).
    pub fn presets(&self) -> Vec<WasmPreset> {
        self.presets.clone()
    }

    /// Assemble the Serum2-loadable `.fxp` bytes for the instance at
    /// `index` (same order as [`FlpDoc::presets`]). The `prgName` inside
    /// the file is the instance's embedded preset name, matching the CLI
    /// output.
    pub fn build_fxp(&self, index: usize) -> Result<Vec<u8>, JsValue> {
        let preset = self.presets.get(index).ok_or_else(|| {
            JsValue::from_str(&format!(
                "preset index {index} out of range ({} Serum instance(s) found)",
                self.presets.len()
            ))
        })?;
        let chunk = self.chunks.get(index).ok_or_else(|| {
            JsValue::from_str("internal error: preset chunk list out of sync with presets")
        })?;
        Ok(fxp::build_fxp(chunk, &preset.preset_name))
    }

    /// Failed per-instance conversions as an already-serialized JSON string
    /// array of human-readable messages (empty array when none).
    #[wasm_bindgen(getter)]
    pub fn failed_json(&self) -> String {
        self.failed_json.clone()
    }

    /// Number of Serum2 instances skipped during the scan.
    #[wasm_bindgen(getter)]
    pub fn serum2_skipped(&self) -> u32 {
        self.serum2_skipped
    }
}

/// Scan an FLP file into a reusable [`FlpDoc`] (same behavior as
/// [`scan_flp_report`]; the document additionally retains the chunks).
#[wasm_bindgen]
pub fn scan_doc(data: &[u8]) -> Result<FlpDoc, JsValue> {
    let scan = scan_impl(data)?;
    Ok(FlpDoc {
        presets: scan.presets,
        chunks: scan.chunks,
        failed_json: scan.failed_json,
        serum2_skipped: scan.serum2_skipped,
    })
}

/// Convert every Serum synth instance in the FLP to a Serum2 instance.
///
/// The orchestration mirrors the CLI's `convert` command: plan via
/// [`flpconv::scan_convertible_detailed`], build one Serum2 bundle per
/// plan through [`flpconv::RealSource::embedded`] (importer + embedded
/// templates), then splice everything back with [`flpconv::apply`]. Serum
/// FX instances are deliberately left untouched (reported as warnings).
///
/// Per-instance conversion failures are never fatal: the instance stays
/// unconverted and the reason is added to `warnings_json`.
#[wasm_bindgen]
pub struct ConvertReport {
    converted_count: u32,
    flp: Vec<u8>,
    warnings_json: String,
    details_json: String,
}

#[wasm_bindgen]
impl ConvertReport {
    /// Number of Serum instances rewritten as Serum2.
    #[wasm_bindgen(getter)]
    pub fn converted_count(&self) -> u32 {
        self.converted_count
    }

    /// The converted FLP bytes (ready to save as a new `.flp`).
    pub fn flp(&self) -> Vec<u8> {
        self.flp.clone()
    }

    /// Warnings as an already-serialized JSON string array (per-instance
    /// failures, Serum FX instances left untouched).
    #[wasm_bindgen(getter)]
    pub fn warnings_json(&self) -> String {
        self.warnings_json.clone()
    }

    /// Per-instance details as an already-serialized JSON array of
    /// `{channel, channelName, presetName, payloadLen, notes:[...]}`.
    #[wasm_bindgen(getter)]
    pub fn details_json(&self) -> String {
        self.details_json.clone()
    }
}

/// Convert an FLP to Serum2 instances (see [`ConvertReport`]).
///
/// `data` are the raw `.flp` bytes. Unparseable input (zip archive, missing
/// `FLhd`, bookkeeping mismatch, ...) rejects with a human-readable
/// message; instances that fail to convert individually only produce a
/// warning and are left as Serum.
#[wasm_bindgen]
pub fn convert_flp(data: &[u8]) -> Result<ConvertReport, JsValue> {
    convert_impl(data, None)
}

/// Convert only the selected Serum instances of an FLP (see
/// [`ConvertReport`]).
///
/// `indices` are scan-order instance indices — the same numbers the web UI
/// shows as the preset table's row (`WasmPreset::index`), not conversion
/// plan positions (Serum FX rows exist in the table but are never planned;
/// see [`flpconv::InstancePlan::instance_index`]). Every instance that is
/// not selected stays byte-identical and is reported in `warnings_json`,
/// as is a selected index with no convertible Serum synth behind it (a
/// Serum FX row or an out-of-range index). An empty selection converts
/// every instance, matching [`convert_flp`].
#[wasm_bindgen]
pub fn convert_flp_selected(data: &[u8], indices: &[u32]) -> Result<ConvertReport, JsValue> {
    if indices.is_empty() {
        return convert_flp(data);
    }
    convert_impl(data, Some(indices))
}

/// Shared orchestration for [`convert_flp`] and [`convert_flp_selected`];
/// `rows` narrows the conversion to the selected preset-table rows
/// (`None` = convert every plan).
fn convert_impl(data: &[u8], rows: Option<&[u32]>) -> Result<ConvertReport, JsValue> {
    let (all_plans, mut warnings) =
        flpconv::scan_convertible_detailed(data).map_err(|e| JsValue::from_str(&e))?;

    // Resolve the selection to the plans to rewrite. `apply` only touches
    // plans in this list, so unselected instances stay byte-identical.
    let plans: Vec<flpconv::InstancePlan> = match rows {
        None => all_plans,
        Some(indices) => {
            let (positions, mut skipped) = flpconv::filter_plans_by_rows(&all_plans, indices);
            warnings.append(&mut skipped);
            let mut kept = Vec::with_capacity(positions.len());
            let mut next = 0;
            for (i, plan) in all_plans.into_iter().enumerate() {
                if next < positions.len() && positions[next] == i {
                    kept.push(plan);
                    next += 1;
                }
            }
            kept
        }
    };

    let mut source = flpconv::RealSource::embedded();
    let mut bundles: Vec<Option<flpconv::Serum2Bundle>> = Vec::with_capacity(plans.len());
    for (i, plan) in plans.iter().enumerate() {
        // `bundle_for` prefers the preset captured during the planning walk
        // (`plan.s1`); the raw chunk is re-derived from the payload only for
        // plans whose parse failed (so the fallback can report the reason).
        let fallback;
        let s1_chunk: &[u8] = match &plan.s1 {
            Some(_) => &[],
            None => {
                fallback = serum::serum1_chunk_from_state(&plan.payload)
                    .map(|c| c.chunk)
                    .unwrap_or_default();
                &fallback
            }
        };
        match source.bundle_for(plan, s1_chunk) {
            Ok(Some(bundle)) => bundles.push(Some(bundle)),
            Ok(None) => {
                let reason = source
                    .warnings
                    .pop()
                    .unwrap_or_else(|| "no Serum2 bundle produced".into());
                warnings.push(format!(
                    "instance {} on channel '{}': {reason}",
                    i + 1,
                    core::display_name(&plan.channel_name)
                ));
                bundles.push(None);
            }
            Err(e) => {
                warnings.push(format!(
                    "instance {} on channel '{}': {e}",
                    i + 1,
                    core::display_name(&plan.channel_name)
                ));
                bundles.push(None);
            }
        }
    }

    let (out, report) =
        flpconv::apply(data, &plans, &bundles).map_err(|e| JsValue::from_str(&e))?;

    let details: Vec<String> = report
        .converted
        .iter()
        .map(|c| {
            format!(
                "{{\"channel\":\"{}\",\"channelName\":\"{}\",\
                 \"presetName\":\"{}\",\"payloadLen\":{},\"notes\":[]}}",
                json_escape(&c.channel.map(|v| v.to_string()).unwrap_or_default()),
                json_escape(&c.channel_name),
                json_escape(&c.preset_name),
                c.new_payload_len
            )
        })
        .collect();
    Ok(ConvertReport {
        converted_count: report.converted.len() as u32,
        flp: out,
        warnings_json: json_string_array(warnings.iter().map(|s| s.as_str())),
        details_json: format!("[{}]", details.join(",")),
    })
}

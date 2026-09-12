//! Structured, machine-readable reports for the CLI subcommands.
//!
//! Every command collects its results into one of the `*Report` types here
//! ([`ListReport`], [`ExtractReport`], [`ValidateReport`],
//! [`ConvertReport`]) while it runs; `--json` then serializes the report
//! to stdout instead of the human-readable lines. Key names use camelCase
//! and mirror the wasm report fields ([`crate::web`]: `presets`, `failed`,
//! `serum2Skipped`, `convertedCount`, `warnings`, `details`, ...) so CLI
//! and web consumers can share parsing logic.
//!
//! All types serialize identically on native and wasm targets; `serde` /
//! `serde_json` are wasm32-safe.

use serde::Serialize;

/// Shared behavior of the top-level per-command reports: a command that
/// ran to completion but must still exit non-zero ("validation failed",
/// "no presets extracted", ...) reports that through [`CommandReport::error`]
/// while still serializing its full results. Aborting errors are not
/// represented here — those surface as `{"error": "..."}` instead of a
/// report.
pub trait CommandReport: Serialize {
    /// Terminal error message, when the process must exit non-zero.
    fn error(&self) -> Option<&str>;
}

/// One Serum instance found by a scan (mirrors the wasm `WasmPreset`
/// fields; `channel` is `null` when unknown instead of an empty string).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetEntry {
    /// Zero-based position in the scan order.
    pub index: u32,
    /// FL Studio channel number (`null` when unknown).
    pub channel: Option<u16>,
    /// Channel name (falls back to the FX track name).
    pub channel_name: String,
    /// Plugin display name as stored in the FLP.
    pub plugin_name: String,
    /// Preset name embedded in the Serum state (may be empty).
    pub preset_name: String,
    /// Author string embedded in the preset state.
    pub author: String,
    /// Category string embedded in the preset state.
    pub category: String,
    /// Preset-format version float (e.g. `0.1631`).
    pub version_f32: f32,
    /// Decompressed size of the first (preset state) zlib stream, in bytes.
    pub state_bytes: usize,
    /// Total size of the raw chunk payload embedded in the `.fxp`, in bytes.
    pub chunk_bytes: usize,
    /// How the chunk was recovered (`FlVst3Wrapper`, `RawZlib`,
    /// `CcnKFxPreset`, `VstWFxPreset`).
    pub source: String,
    /// True when an earlier instance had an identical chunk.
    pub duplicate: bool,
    /// Deterministic content hash of the chunk as a decimal string.
    pub content_hash: String,
    /// True when all Serum2 import checks pass (no fatal issues).
    pub valid: bool,
    /// Serum2 validation warnings (imports, but note).
    pub warnings: Vec<String>,
    /// Fatal Serum2 validation issues.
    pub errors: Vec<String>,
}

/// Result of `patch`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchReport {
    /// Input path as given on the command line.
    pub input: String,
    /// Output path written (or planned under `--dry-run`).
    pub output: String,
    /// True under `--dry-run` (no write performed).
    pub dry_run: bool,
    /// Patched Serum instances (1 for a standalone .fxp; the FLP path reports
    /// every rewritten instance).
    pub patched: u32,
    /// Warnings collected while re-validating the patched output.
    pub warnings: Vec<String>,
}

impl CommandReport for PatchReport {
    fn error(&self) -> Option<&str> {
        None
    }
}
/// Per-input result of `list`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListInputReport {
    /// Input FLP path as given on the command line.
    pub input: String,
    /// Discovered Serum instances, in file order.
    pub presets: Vec<PresetEntry>,
    /// Per-instance scan failures (never fatal).
    pub failed: Vec<String>,
    /// Number of Serum2 instances skipped during the scan.
    pub serum2_skipped: u32,
}

/// `list` output: per-input results plus a total.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListReport {
    /// One report per input file, in command-line order.
    pub inputs: Vec<ListInputReport>,
    /// Total number of Serum presets found across all inputs.
    pub preset_count: u32,
}

impl CommandReport for ListReport {
    fn error(&self) -> Option<&str> {
        None
    }
}

/// What happened to one preset during `extract`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtractStatus {
    /// The `.fxp` was written.
    Written,
    /// Identical to an earlier preset in the same input, skipped.
    Duplicate,
    /// Target file exists and `--overwrite` was not given, skipped.
    Exists,
    /// Failed Serum2 validation and `--keep-invalid` was not given.
    Invalid,
}

/// Outcome of one extracted (or skipped) preset in `extract`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractEntry {
    /// Zero-based position in the scan order.
    pub index: u32,
    /// FL Studio channel number (`null` when unknown).
    pub channel: Option<u16>,
    /// Channel name (falls back to the FX track name).
    pub channel_name: String,
    /// Preset name embedded in the Serum state (may be empty).
    pub preset_name: String,
    /// Author string embedded in the preset state.
    pub author: String,
    /// Category string embedded in the preset state.
    pub category: String,
    /// Preset-format version float (e.g. `0.1631`).
    pub version_f32: f32,
    /// Decompressed size of the first (preset state) zlib stream, in bytes.
    pub state_bytes: usize,
    /// Total size of the raw chunk payload embedded in the `.fxp`, in bytes.
    pub chunk_bytes: usize,
    /// How the chunk was recovered (`FlVst3Wrapper`, `RawZlib`, ...).
    pub source: String,
    /// What happened to this preset (see [`ExtractStatus`]).
    pub status: ExtractStatus,
    /// Written `.fxp` path (set only for [`ExtractStatus::Written`]).
    pub path: Option<String>,
    /// True when all Serum2 import checks pass (no fatal issues).
    pub valid: bool,
    /// Serum2 validation warnings (imports, but note).
    pub warnings: Vec<String>,
    /// Fatal Serum2 validation issues (non-empty when `valid == false`).
    pub errors: Vec<String>,
}

/// Per-input result of `extract`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractInputReport {
    /// Input FLP path as given on the command line.
    pub input: String,
    /// Output directory the presets are written to.
    pub out_dir: String,
    /// Written / skipped presets, in scan order.
    pub entries: Vec<ExtractEntry>,
    /// Number of presets actually written.
    pub extracted_count: u32,
    /// Per-instance scan failures (never fatal).
    pub failed: Vec<String>,
    /// Number of Serum2 instances skipped during the scan.
    pub serum2_skipped: u32,
}

/// `extract` output: per-input results plus totals.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractReport {
    /// One report per input file, in command-line order.
    pub inputs: Vec<ExtractInputReport>,
    /// Total number of presets written across all inputs.
    pub extracted_count: u32,
    /// Total number of presets rejected by Serum2 validation.
    pub invalid_count: u32,
    /// Terminal error, present when the command must exit non-zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CommandReport for ExtractReport {
    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// Per-file result of `validate` (mirrors the wasm `WasmPreset` issue
/// fields: `valid`, `warnings`, `errors`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateInputReport {
    /// Input `.fxp` path as given on the command line.
    pub input: String,
    /// True when all Serum2 import checks pass (no fatal issues).
    pub valid: bool,
    /// Fatal issues (Serum2 refuses to import the file).
    pub errors: Vec<String>,
    /// Warnings (the file loads, but deviates from what Serum writes).
    pub warnings: Vec<String>,
    /// Embedded preset name, when the state could be decompressed.
    pub preset_name: Option<String>,
}

/// `validate` output: per-input results.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateReport {
    /// One report per input file, in command-line order.
    pub inputs: Vec<ValidateInputReport>,
    /// Terminal error, present when the command must exit non-zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CommandReport for ValidateReport {
    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// One converted instance in `convert` (mirrors the wasm `details` entries:
/// `channel`, `channelName`, `presetName`, `payloadLen`, `notes`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertDetail {
    /// FL Studio channel number (`null` when unknown).
    pub channel: Option<u16>,
    /// Channel name (falls back to the FX track name).
    pub channel_name: String,
    /// Preset name recovered from the original Serum state (may be empty).
    pub preset_name: String,
    /// Size of the rewritten event-213 payload, in bytes.
    pub payload_len: usize,
    /// Human-readable notes (currently always empty, kept for wasm parity).
    pub notes: Vec<String>,
}

/// Per-input result of `convert`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertInputReport {
    /// Input FLP path as given on the command line.
    pub input: String,
    /// True when the plan was printed without writing anything.
    pub dry_run: bool,
    /// Written output FLP path (`null` on `--dry-run`).
    pub output: Option<String>,
    /// Size of the written FLP, in bytes (`null` on `--dry-run`).
    pub output_bytes: Option<usize>,
    /// Number of Serum instances rewritten as Serum2.
    pub converted_count: u32,
    /// Per-instance details, in scan order.
    pub details: Vec<ConvertDetail>,
    /// Non-fatal warnings (scan failures, Serum FX instances left
    /// untouched, skipped instances).
    pub warnings: Vec<String>,
}

/// `convert` output: per-input results plus a total.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertReport {
    /// One report per input file, in command-line order.
    pub inputs: Vec<ConvertInputReport>,
    /// Total number of instances converted across all inputs.
    pub converted_count: u32,
    /// Terminal error, present when the command must exit non-zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CommandReport for ConvertReport {
    fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_camel_case_keys() {
        let report = ValidateReport {
            inputs: vec![ValidateInputReport {
                input: "a.fxp".into(),
                valid: true,
                errors: vec![],
                warnings: vec!["w".into()],
                preset_name: Some("P".into()),
            }],
            error: None,
        };
        let v: serde_json::Value = serde_json::to_value(&report).unwrap();
        assert_eq!(v["inputs"][0]["input"], "a.fxp");
        assert_eq!(v["inputs"][0]["valid"], true);
        assert_eq!(v["inputs"][0]["presetName"], "P");
        assert_eq!(v["inputs"][0]["warnings"][0], "w");
        assert!(v.get("error").is_none());
    }

    #[test]
    fn terminal_error_is_present_only_when_set() {
        let mut report = ConvertReport {
            inputs: vec![],
            converted_count: 0,
            error: None,
        };
        assert!(
            serde_json::to_string(&report)
                .unwrap()
                .find("\"error\"")
                .is_none()
        );
        report.error = Some("no instances converted".into());
        assert_eq!(report.error(), Some("no instances converted"));
        let v: serde_json::Value = serde_json::to_value(&report).unwrap();
        assert_eq!(v["error"], "no instances converted");
    }

    #[test]
    fn list_report_never_carries_an_error() {
        let report = ListReport {
            inputs: vec![],
            preset_count: 0,
        };
        assert_eq!(report.error(), None);
        assert!(
            serde_json::to_string(&report)
                .unwrap()
                .find("\"error\"")
                .is_none()
        );
    }
}

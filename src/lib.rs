//! # flp-extract-fxp
//!
//! Library core for extracting Serum presets (`.fxp`) embedded in FL
//! Studio project (`.flp`) files. The same logic backs both the
//! `flp-extract-fxp` CLI binary (`src/main.rs`) and the WebAssembly
//! bindings for browsers (`src/web.rs`).
//!
//! Module map:
//! - [`flp`]: minimal FLP event-stream parser (`parse_events`,
//!   `parse_event_spans`, `[u32 cid][u64 size][data]` record walking).
//! - [`serum`]: Serum / Serum2 detection and preset-chunk recovery.
//! - [`fxp`]: `.fxp` assembly and Serum2 import-rule validation.
//! - [`core`]: reusable "scan an FLP for Serum instances" logic plus
//!   small text / filename / hashing helpers shared by the CLI and wasm.
//! - [`report`]: structured `--json` CLI report types (serde; camelCase
//!   keys aligned with the wasm report fields).
//! - [`zlibio`]: shared zlib inflate + Serum chunk stream splitting.
//! - [`flpconv`]: FLP byte surgery rewriting Serum event-213 payloads
//!   into Serum2 ones (plan / bundle / apply pipeline).
//! - [`importer`]: Serum → Serum2 conversion (a faithful port of
//!   Serum2.vst3's `s1state_load`; split into `params`, `modmatrix`,
//!   `fxrack`, `lfo`, `meta` submodules).
//! - [`s1state`]: Serum preset-state parsing (typed view of the
//!   172,736-byte state blob).
//! - [`s2tables`]: GENERATED runtime-dumped Serum2 descriptor tables
//!   (never hand-edit; provenance in docs/s2-runtime-tables.md).
//! - [`s2tree`]: deterministic CBOR value tree + raw zstd frames.
//! - [`serum2state`]: Serum2 `XferJson` container assembly and parsing.
//! - `web`: `wasm-bindgen` bindings; only compiled when targeting
//!   `wasm32-unknown-unknown` (cfg-gated, so native builds never link it).
//! - `testutil`: `#[cfg(test)]` helpers shared by crate-internal tests.

pub mod core;
pub mod flp;
pub mod flpconv;
pub mod fxp;
pub mod report;
pub mod serum;
pub mod zlibio;

pub mod importer;
pub mod s1state;
pub mod s2tables;
pub mod s2tree;
pub mod serum2state;

#[cfg(test)]
pub(crate) mod testutil;

#[cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))]
pub mod web;

/// Convenience re-exports of the most-used scanning API.
pub use core::{Instance, ScanStats, scan_serum_instances};

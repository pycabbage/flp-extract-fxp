//! # flp-extract-fxp
//!
//! Library core for extracting Serum 1 presets (`.fxp`) embedded in FL
//! Studio project (`.flp`) files. The same logic backs both the
//! `flp-extract-fxp` CLI binary (`src/main.rs`) and the WebAssembly
//! bindings for browsers (`src/web.rs`).
//!
//! Module map:
//! - [`flp`]: minimal FLP event-stream parser.
//! - [`serum`]: Serum 1 / Serum 2 detection and preset-chunk recovery.
//! - [`fxp`]: `.fxp` assembly and Serum 2 import-rule validation.
//! - [`core`]: reusable "scan an FLP for Serum 1 instances" logic plus
//!   small text / filename / hashing helpers shared by the CLI and wasm.
//! - `web`: `wasm-bindgen` bindings; only compiled when targeting
//!   `wasm32-unknown-unknown` (cfg-gated, so native builds never link it).

pub mod core;
pub mod flp;
pub mod flpconv;
pub mod fxp;
pub mod serum;

pub mod importer;
pub mod s1state;
pub mod s2tables;
pub mod s2tree;
pub mod serum2state;

#[cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))]
pub mod web;

/// Convenience re-exports of the most-used scanning API.
pub use core::{Instance, ScanStats, scan_serum_instances};

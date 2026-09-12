//! End-to-end tests: build a synthetic FLP, extract it with the real CLI
//! binary, and validate the resulting .fxp through the CLI as well.
//!
//! The `convert_*` tests exercise the Serum -> Serum2 FLP converter on a
//! real FL Studio project fixture (5 Serum instances and 1 genuine Serum2
//! instance). That fixture is not tracked in the repo (third-party project
//! content; see docs/flp-conversion.md) — the tests skip when it is absent.

use flate2::Compression;
use flate2::write::ZlibEncoder;
use flp_extract_fxp::{core::scan_serum_instances, flpconv::scan_convertible_detailed};
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_flp-extract-fxp");

fn zlib_stream(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// A synthetic Serum chunk: zlib(preset state) + zlib(table data) + trailer.
fn synthetic_serum1_chunk() -> Vec<u8> {
    let mut s0 = vec![0u8; 172_736];
    let name = b"SynthTest";
    s0[0x4972..0x4972 + name.len()].copy_from_slice(name);
    s0[0x4994..0x4998].copy_from_slice(&0.1631f32.to_le_bytes());
    let z0 = zlib_stream(&s0);
    let z1 = zlib_stream(&vec![0u8; 8192]);
    let mut chunk = z0.clone();
    chunk.extend_from_slice(&z1);
    chunk.extend_from_slice(&(z0.len() as u32).to_le_bytes());
    chunk
}

/// FL Studio's VST3 wrapper state: prologue + cid1(64B) + cid3(plugin state)
/// + cid4(8B).
fn synthetic_vst3_wrapper_state() -> Vec<u8> {
    let cid3 = synthetic_serum1_chunk();
    let mut state = Vec::new();
    state.extend_from_slice(&[1, 0, 0, 0]);
    for (cid, payload) in [(1u32, vec![0u8; 64]), (3u32, cid3), (4u32, vec![0u8; 8])] {
        state.extend_from_slice(&cid.to_le_bytes());
        state.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        state.extend_from_slice(&payload);
    }
    state
}

/// PluginParams (event 213) payload: version + chunk id/size records.
fn synthetic_plugin_params() -> Vec<u8> {
    let state = synthetic_vst3_wrapper_state();
    let mut data = Vec::new();
    data.extend_from_slice(&12u32.to_le_bytes());
    for (cid, payload) in [
        (54u32, b"Serum".to_vec()),
        (55u32, b"Serum.vst3".to_vec()),
        (53u32, state),
    ] {
        data.extend_from_slice(&cid.to_le_bytes());
        data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&payload);
    }
    data
}

/// Encode a length as an FLP varint (LE 7-bit groups, high bit = continue).
fn flp_varint(mut len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (len & 0x7f) as u8;
        len >>= 7;
        if len == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
    out
}

/// Minimal FLP: FLhd + FLdt containing NewChan + channel name + PluginParams.
fn synthetic_flp() -> Vec<u8> {
    let events: Vec<(u8, Vec<u8>)> = vec![
        (64, vec![0, 0]),        // NewChan: channel 0
        (203, b"Bass".to_vec()), // channel name
        (213, synthetic_plugin_params()),
    ];

    let mut dt = Vec::new();
    for (id, data) in events {
        dt.push(id);
        if id >= 192 {
            dt.extend_from_slice(&flp_varint(data.len()));
        }
        dt.extend_from_slice(&data);
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

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("flp_extract_it_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn extract_from_synthetic_flp_then_validate() {
    let dir = temp_dir("synthetic");
    let flp_path = dir.join("test_project.flp");
    std::fs::write(&flp_path, synthetic_flp()).unwrap();

    let out_dir = dir.join("out");
    let status = Command::new(BIN)
        .args(["extract", "-o"])
        .arg(&out_dir)
        .arg(&flp_path)
        .status()
        .unwrap();
    assert!(status.success(), "extract failed");

    let fxp_path = out_dir.join("01_SynthTest.fxp");
    assert!(fxp_path.exists(), "expected {}", fxp_path.display());
    let fxp = std::fs::read(&fxp_path).unwrap();
    assert_eq!(&fxp[0..4], b"CcnK");
    assert_eq!(&fxp[0x10..0x14], b"XfsX");

    let status = Command::new(BIN)
        .args(["validate"])
        .arg(&fxp_path)
        .status()
        .unwrap();
    assert!(status.success(), "validate failed");
}

#[test]
fn validates_real_fixture() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extracted_serum1.fxp");
    if !fixture.exists() {
        eprintln!("skipping: untracked fixture absent (docs/flp-conversion.md)");
        return;
    }
    let status = Command::new(BIN)
        .args(["validate"])
        .arg(&fixture)
        .status()
        .unwrap();
    assert!(status.success(), "fixture failed validation");
}

fn serina1_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/serina1.flp")
}

/// The real-project fixture is untracked; skip the test when absent.
fn have_serina1() -> bool {
    let f = serina1_fixture();
    if f.exists() {
        true
    } else {
        eprintln!("skipping: untracked fixture absent (docs/flp-conversion.md)");
        false
    }
}

#[test]
fn convert_writes_output() {
    if !have_serina1() {
        return;
    }
    let dir = temp_dir("convert");
    let out = dir.join("conv.flp");
    let output = Command::new(BIN)
        .args(["convert", "--out"])
        .arg(&out)
        .arg(serina1_fixture())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("converting 5 Serum instance(s)"),
        "{stdout}"
    );
    assert_eq!(stdout.matches("-> converted (cid3").count(), 5, "{stdout}");
    assert!(out.exists(), "expected {}", out.display());
}

#[test]
fn converted_flp_scans_clean() {
    if !have_serina1() {
        return;
    }
    let dir = temp_dir("scan");
    let out = dir.join("conv.flp");
    let status = Command::new(BIN)
        .args(["convert", "--out"])
        .arg(&out)
        .arg(serina1_fixture())
        .status()
        .unwrap();
    assert!(status.success(), "convert failed");
    let output = Command::new(BIN).args(["list"]).arg(&out).output().unwrap();
    assert!(output.status.success(), "list failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // 5 converted + 1 pre-existing Serum2 instance.
    assert!(
        stdout.contains("0 Serum preset(s), 6 Serum2 instance(s)"),
        "{stdout}"
    );
}

#[test]
fn converted_flp_diff_is_localized() {
    if !have_serina1() {
        return;
    }
    let dir = temp_dir("diff");
    let out = dir.join("conv.flp");
    let status = Command::new(BIN)
        .args(["convert", "--out"])
        .arg(&out)
        .arg(serina1_fixture())
        .status()
        .unwrap();
    assert!(status.success(), "convert failed");

    let orig = std::fs::read(serina1_fixture()).unwrap();
    let conv = std::fs::read(&out).unwrap();
    // FLhd prefix unchanged (first 16 bytes).
    assert_eq!(&conv[..16], &orig[..16]);
    // FLdt chunk magic at offset 8+hdrlen (=14) unchanged; only the u32
    // length field after it is legitimately rewritten.
    assert_eq!(&conv[14..18], &orig[14..18]);
    // The conversion grew the file (Serum2 payloads are larger).
    assert!(conv.len() > orig.len());

    // The converted file holds exactly 6 Serum2 instances (5 converted +
    // 1 original) and no Serum instances at all.
    let (instances, stats) = scan_serum_instances(&conv).unwrap();
    assert!(
        instances.is_empty(),
        "Serum instances left: {}",
        instances.len()
    );
    assert_eq!(stats.serum2_count, 6);

    // Structural alignment: on the original sample the converter's plans and
    // the core scan's Serum instances must be 1:1 in the same order.
    let plans = scan_convertible_detailed(&orig).unwrap().0;
    let (instances, _) = scan_serum_instances(&orig).unwrap();
    assert_eq!(plans.len(), instances.len());
    for (p, i) in plans.iter().zip(&instances) {
        assert_eq!(p.channel, i.channel);
        assert_eq!(p.channel_name, i.channel_name);
        assert_eq!(p.plugin_name, i.plugin_name);
    }
}

// ---------------------------------------------------------------------------
// --json output: every command emits one machine-readable JSON document on
// stdout (progress on stderr). These tests parse the documents and make
// structural assertions (no snapshot files), including the camelCase key
// names shared with the wasm report JSONs (src/web.rs).
// ---------------------------------------------------------------------------

/// Run the CLI and return (exit code, stdout, stderr).
fn run_cli(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(BIN).args(args).output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Top-level record of an event-213 payload: `[u32 cid][u64 size][data]`.
fn top_rec(cid: u32, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(12 + data.len());
    v.extend_from_slice(&cid.to_le_bytes());
    v.extend_from_slice(&(data.len() as u64).to_le_bytes());
    v.extend_from_slice(data);
    v
}

/// Inner cid-3 chunk (synthetic Serum state + wavetable stream + trailer),
/// same as [`synthetic_serum1_chunk`].
fn convertible_cid3() -> Vec<u8> {
    synthetic_serum1_chunk()
}

/// A realistic Serum event-213 payload (mirrors the `flpconv` unit-test
/// fixture): identity records + FL VST3 wrapper around a synthetic chunk.
fn synthetic_convertible_plugin_params() -> Vec<u8> {
    let cid1: [u8; 20] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let cid2: [u8; 25] = [
        0x00, 0xA0, 0x00, 0x00, 0x00, 0x19, 0x00, 0x00, 0x00, 0x8D, 0x7D, 0x20, 0xA4, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let c30: [u8; 16] = [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let c32: [u8; 12] = [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
    let c50: [u8; 16] = [0x08, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let s1_uid: [u8; 16] = [
        b'X', b'T', b'S', b'V', b's', b'f', b's', b'X', b's', b'e', b'r', b'u', b'm', 0, 0, 0,
    ];

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
    wrapper.extend_from_slice(&top_rec(3, &convertible_cid3()));
    wrapper.extend_from_slice(&top_rec(4, &cid4));

    let mut p = Vec::new();
    p.extend_from_slice(&12u32.to_le_bytes());
    p.extend_from_slice(&top_rec(1, &cid1));
    p.extend_from_slice(&top_rec(2, &cid2));
    p.extend_from_slice(&top_rec(30, &c30));
    p.extend_from_slice(&top_rec(32, &c32));
    p.extend_from_slice(&top_rec(50, &c50));
    p.extend_from_slice(&top_rec(52, &s1_uid));
    p.extend_from_slice(&top_rec(54, b"Serum"));
    p.extend_from_slice(&top_rec(55, b"/Library/Audio/Plug-Ins/VST3/Serum.vst3"));
    p.extend_from_slice(&top_rec(56, b"Xfer Records"));
    p.extend_from_slice(&top_rec(53, &wrapper));
    p
}

/// Minimal FLP holding one fully-shaped convertible Serum instance (the
/// real importer runs on it, so `convert` succeeds without any fixture).
fn synthetic_convertible_flp() -> Vec<u8> {
    let events: Vec<(u8, Vec<u8>)> = vec![
        (64, vec![0, 0]), // NewChan: channel 0
        (
            203,
            "Bass".encode_utf16().flat_map(u16::to_le_bytes).collect(),
        ), // channel name (UTF-16LE)
        (213, synthetic_convertible_plugin_params()),
    ];
    let mut dt = Vec::new();
    for (id, data) in events {
        dt.push(id);
        if id >= 192 {
            dt.extend_from_slice(&flp_varint(data.len()));
        }
        dt.extend_from_slice(&data);
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
fn list_json_has_wasm_aligned_structure() {
    let dir = temp_dir("json_list");
    let flp_path = dir.join("test_project.flp");
    std::fs::write(&flp_path, synthetic_flp()).unwrap();
    let flp_arg = flp_path.display().to_string();

    let (code, stdout, stderr) = run_cli(&["list", "--json", &flp_arg]);
    assert_eq!(code, 0);
    let report: Value =
        serde_json::from_str(&stdout).expect("stdout must be a single JSON document");
    assert!(stderr.contains("1 Serum preset(s)"), "{stderr}");
    assert!(stderr.contains(&flp_arg), "{stderr}");

    let inputs = report["inputs"].as_array().expect("inputs array");
    assert_eq!(inputs.len(), 1);
    assert_eq!(report["presetCount"], 1);
    let input = &inputs[0];
    assert!(
        input["input"]
            .as_str()
            .unwrap()
            .ends_with("test_project.flp"),
        "{}",
        input["input"]
    );
    assert_eq!(input["serum2Skipped"], 0);
    assert_eq!(input["failed"].as_array().unwrap().len(), 0);

    let presets = input["presets"].as_array().unwrap();
    assert_eq!(presets.len(), 1);
    let p = &presets[0];
    // Key names mirror the wasm WasmPreset fields (camelCase).
    assert_eq!(p["index"], 0);
    assert_eq!(p["channel"], 0);
    assert_eq!(p["channelName"], "Bass");
    assert_eq!(p["pluginName"], "Serum");
    assert_eq!(p["presetName"], "SynthTest");
    assert_eq!(p["author"], "");
    assert_eq!(p["category"], "");
    assert!((p["versionF32"].as_f64().unwrap() - 0.1631).abs() < 1e-6);
    assert_eq!(p["stateBytes"], 172_736);
    assert!(p["chunkBytes"].as_u64().unwrap() > 0);
    assert_eq!(p["source"], "FlVst3Wrapper");
    assert_eq!(p["duplicate"], false);
    assert!(
        p["contentHash"]
            .as_str()
            .unwrap()
            .bytes()
            .all(|b| b.is_ascii_digit()),
        "contentHash must be a decimal string: {}",
        p["contentHash"]
    );
    assert_eq!(p["valid"], true);
    assert_eq!(p["warnings"].as_array().unwrap().len(), 0);
    assert_eq!(p["errors"].as_array().unwrap().len(), 0);
}

#[test]
fn extract_json_reports_written_files() {
    let dir = temp_dir("json_extract");
    let flp_path = dir.join("test_project.flp");
    std::fs::write(&flp_path, synthetic_flp()).unwrap();

    let out_dir = dir.join("out");
    let out_arg = out_dir.display().to_string();
    let flp_arg = flp_path.display().to_string();
    let (code, stdout, stderr) = run_cli(&["extract", "--json", "-o", &out_arg, &flp_arg]);
    assert_eq!(code, 0, "{stderr}");
    let report: Value = serde_json::from_str(&stdout).expect("stdout must be valid JSON");
    assert!(stderr.contains("1 Serum preset(s)"), "{stderr}");

    assert_eq!(report["extractedCount"], 1);
    assert_eq!(report["invalidCount"], 0);
    assert!(
        report.get("error").is_none(),
        "no terminal error on success"
    );

    let input = &report["inputs"][0];
    assert_eq!(input["extractedCount"], 1);
    assert!(input["outDir"].as_str().unwrap().ends_with("out"));
    assert_eq!(input["serum2Skipped"], 0);

    let entries = input["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["index"], 0);
    assert_eq!(entry["status"], "written");
    assert_eq!(entry["presetName"], "SynthTest");
    assert_eq!(entry["valid"], true);
    let path = entry["path"]
        .as_str()
        .expect("written entry carries a path");
    assert!(path.ends_with("01_SynthTest.fxp"), "{path}");
    assert!(PathBuf::from(path).exists(), "reported file must exist");
}

#[test]
fn validate_json_reports_pass_fail_and_terminal_error() {
    let dir = temp_dir("json_validate");
    let flp_path = dir.join("test_project.flp");
    std::fs::write(&flp_path, synthetic_flp()).unwrap();
    let out_dir = dir.join("out");
    let fxp_path = out_dir.join("01_SynthTest.fxp");

    // Produce a known-good fxp via the normal (non-json) path.
    let status = Command::new(BIN)
        .args(["extract", "-o"])
        .arg(&out_dir)
        .arg(&flp_path)
        .status()
        .unwrap();
    assert!(status.success(), "extract failed");

    // PASS case: valid == true, embedded preset name surfaced, no error.
    let fxp_arg = fxp_path.display().to_string();
    let (code, stdout, _stderr) = run_cli(&["validate", "--json", &fxp_arg]);
    assert_eq!(code, 0);
    let report: Value = serde_json::from_str(&stdout).unwrap();
    assert!(report.get("error").is_none());
    let file = &report["inputs"][0];
    assert!(
        file["input"]
            .as_str()
            .unwrap()
            .ends_with("01_SynthTest.fxp")
    );
    assert_eq!(file["valid"], true);
    assert_eq!(file["errors"].as_array().unwrap().len(), 0);
    assert_eq!(file["presetName"], "SynthTest");

    // FAIL case: garbage input -> valid == false with fatal errors, exit 1,
    // and a terminal "error" alongside the full per-file results.
    let bad = dir.join("bad.fxp");
    std::fs::write(&bad, b"not an fxp at all").unwrap();
    let bad_arg = bad.display().to_string();
    let (code, stdout, stderr) = run_cli(&["validate", "--json", &bad_arg]);
    assert_eq!(code, 1);
    assert!(stdout.contains("\"error\""), "{stdout}");
    let report: Value = serde_json::from_str(&stdout).expect("stdout must be valid JSON");
    assert_eq!(report["error"], "validation failed");
    let file = &report["inputs"][0];
    assert_eq!(file["valid"], false);
    assert!(!file["errors"].as_array().unwrap().is_empty());
    // Progress stayed on stderr in --json mode.
    assert!(!stdout.contains("FAIL\n"), "per-file lines go to stderr");
    let _ = stderr;
}

#[test]
fn convert_json_reports_dry_run_and_written_output() {
    let dir = temp_dir("json_convert");
    let flp_path = dir.join("conv_project.flp");
    std::fs::write(&flp_path, synthetic_convertible_flp()).unwrap();
    let flp_arg = flp_path.display().to_string();

    // Dry run: plan reported, nothing written.
    let (code, stdout, stderr) = run_cli(&["convert", "--json", "--dry-run", &flp_arg]);
    assert_eq!(code, 0, "{stderr}");
    let report: Value = serde_json::from_str(&stdout).expect("stdout must be valid JSON");
    assert!(
        stderr.contains("converting 1 Serum instance(s)"),
        "{stderr}"
    );
    assert_eq!(report["convertedCount"], 1);
    assert!(report.get("error").is_none());
    let input = &report["inputs"][0];
    assert_eq!(input["dryRun"], true);
    assert!(input["output"].is_null());
    assert!(input["outputBytes"].is_null());
    assert_eq!(input["convertedCount"], 1);
    let details = input["details"].as_array().unwrap();
    assert_eq!(details.len(), 1);
    // Detail keys mirror the wasm `details` entries.
    assert_eq!(details[0]["channel"], 0);
    assert_eq!(details[0]["channelName"], "Bass");
    assert_eq!(details[0]["presetName"], "SynthTest");
    assert!(details[0]["payloadLen"].as_u64().unwrap() > 0);
    assert_eq!(details[0]["notes"].as_array().unwrap().len(), 0);

    // Real run: output path + size reported and matches the file on disk.
    let out_path = dir.join("converted.flp");
    let out_arg = out_path.display().to_string();
    let (code, stdout, stderr) = run_cli(&["convert", "--json", "--out", &out_arg, &flp_arg]);
    assert_eq!(code, 0, "{stderr}");
    let report: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["convertedCount"], 1);
    let input = &report["inputs"][0];
    assert_eq!(input["dryRun"], false);
    assert_eq!(
        input["output"].as_str().unwrap(),
        out_path.display().to_string()
    );
    let bytes = input["outputBytes"].as_u64().unwrap();
    assert_eq!(
        bytes as usize,
        std::fs::metadata(&out_path).unwrap().len() as usize
    );

    // The converted output scans as Serum2-only through the JSON report too.
    let (code, stdout, _stderr) = run_cli(&["list", "--json", &out_arg]);
    assert_eq!(code, 0);
    let report: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["presetCount"], 0);
    assert_eq!(report["inputs"][0]["serum2Skipped"], 1);
}

#[test]
fn json_errors_are_machine_readable() {
    // An aborting error (unreadable input) prints {"error": ...} on stdout
    // and exits non-zero; nothing else pollutes stdout.
    let missing = "/nonexistent/flp_extract_it/missing.flp";
    let (code, stdout, _stderr) = run_cli(&["list", "--json", missing]);
    assert_eq!(code, 1);
    let report: Value = serde_json::from_str(&stdout).expect("stdout must be valid JSON");
    let error = report["error"].as_str().expect("error must be a string");
    assert!(error.contains(missing), "{error}");
}

#[test]
fn without_json_output_is_unchanged() {
    // Regression guard: without --json the human-readable lines still go to
    // stdout and no JSON appears anywhere.
    let dir = temp_dir("plain_list");
    let flp_path = dir.join("test_project.flp");
    std::fs::write(&flp_path, synthetic_flp()).unwrap();
    let flp_arg = flp_path.display().to_string();

    let (code, stdout, stderr) = run_cli(&["list", &flp_arg]);
    assert_eq!(code, 0);
    assert!(stdout.contains("1 Serum preset(s)"), "{stdout}");
    assert!(stdout.contains("[01] channel 0 'Bass'"), "{stdout}");
    assert!(stderr.is_empty(), "no logs on stderr: {stderr}");

    let (code, stdout, stderr) = run_cli(&["list", "--json", &flp_arg]);
    assert_eq!(code, 0);
    assert!(stdout.trim_start().starts_with('{'), "{stdout}");
    assert!(
        !stdout.contains("[01]"),
        "per-instance lines move to stderr"
    );
    assert!(stderr.contains("[01]"), "{stderr}");
}

//! End-to-end tests: build a synthetic FLP, extract it with the real CLI
//! binary, and validate the resulting .fxp through the CLI as well.
//!
//! The `convert_*` tests exercise the Serum -> Serum2 FLP converter on a
//! real FL Studio project fixture (5 Serum instances and 1 genuine Serum2
//! instance). That fixture is not tracked in the repo (third-party project
//! content; see docs/flp-conversion.md) — the tests skip when it is absent.

use flate2::Compression;
use flate2::write::ZlibEncoder;
use flp_extract_fxp::core::scan_serum_instances;
use flp_extract_fxp::flp;
use flp_extract_fxp::flpconv::{
    BundleSource, InstancePlan, RealSource, filter_plans_by_rows, scan_convertible_detailed,
};
use flp_extract_fxp::{s2tree, serum2state};
use md5::{Digest, Md5};
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_flp-extract-fxp");

fn zlib_stream(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// A synthetic Serum chunk: zlib(preset state) + zlib(table data) + trailer.
/// `table_filler` varies the embedded table data (but not the preset state),
/// producing a distinct chunk hash while keeping the same preset name -
/// i.e. a duplicate-name case that survives the duplicate-chunk skip.
fn synthetic_serum1_chunk_with(table_filler: u8) -> Vec<u8> {
    let mut s0 = vec![0u8; 172_736];
    let name = b"SynthTest";
    s0[0x4972..0x4972 + name.len()].copy_from_slice(name);
    s0[0x4994..0x4998].copy_from_slice(&0.1631f32.to_le_bytes());
    let z0 = zlib_stream(&s0);
    let z1 = zlib_stream(&vec![table_filler; 8192]);
    let mut chunk = z0.clone();
    chunk.extend_from_slice(&z1);
    chunk.extend_from_slice(&(z0.len() as u32).to_le_bytes());
    chunk
}

/// The plain zero-filled variant used by main's synthetic helpers.
fn synthetic_serum1_chunk() -> Vec<u8> {
    synthetic_serum1_chunk_with(0)
}

/// FL Studio's VST3 wrapper state: prologue + cid1(64B) + cid3(plugin state)
/// + cid4(8B).
fn synthetic_vst3_wrapper_state_with(table_filler: u8) -> Vec<u8> {
    let cid3 = synthetic_serum1_chunk_with(table_filler);
    let mut state = Vec::new();
    state.extend_from_slice(&[1, 0, 0, 0]);
    for (cid, payload) in [(1u32, vec![0u8; 64]), (3u32, cid3), (4u32, vec![0u8; 8])] {
        state.extend_from_slice(&cid.to_le_bytes());
        state.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        state.extend_from_slice(&payload);
    }
    state
}

/// The plain zero-filled variant used by main's synthetic tests.
fn synthetic_vst3_wrapper_state() -> Vec<u8> {
    synthetic_vst3_wrapper_state_with(0)
}

/// PluginParams (event 213) payload: version + chunk id/size records,
/// including every cid `apply` needs to rebuild a Serum2 wrapper.
fn synthetic_plugin_params() -> Vec<u8> {
    synthetic_plugin_params_with(0)
}

fn synthetic_plugin_params_with(table_filler: u8) -> Vec<u8> {
    let state = synthetic_vst3_wrapper_state_with(table_filler);
    let cid1: [u8; 20] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x0C, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let cid2: [u8; 25] = [
        0, 0xA0, 0, 0, 0, 0x19, 0, 0, 0, 0x8D, 0x7D, 0x20, 0xA4, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
    ];
    let serum_uid: [u8; 16] = [
        0x58, 0x54, 0x53, 0x56, 0x73, 0x66, 0x73, 0x58, 0x65, 0x72, 0x75, 0x6D, 0, 0, 0, 0,
    ];
    let mut data = Vec::new();
    data.extend_from_slice(&12u32.to_le_bytes());
    let records: Vec<(u32, Vec<u8>)> = vec![
        (1, cid1.to_vec()),
        (2, cid2.to_vec()),
        (30, vec![0; 16]),
        (32, vec![0; 12]),
        (50, {
            let mut v = vec![0x08u8, 0, 0, 0];
            v.extend_from_slice(&[0; 12]);
            v
        }),
        (52, serum_uid.to_vec()),
        (54, b"Serum".to_vec()),
        (55, b"Serum.vst3".to_vec()),
        (56, b"Xfer Records".to_vec()),
        (53, state),
    ];
    for (cid, payload) in records {
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

/// Minimal FLP: FLhd + FLdt containing the given `(id, data)` events.
fn encode_flp(events: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut dt = Vec::new();
    for (id, data) in events {
        dt.push(*id);
        if *id >= 192 {
            dt.extend_from_slice(&flp_varint(data.len()));
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

fn flp_from_events(events: Vec<(u8, Vec<u8>)>) -> Vec<u8> {
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

/// Minimal FLP: FLhd + FLdt containing NewChan + channel name + PluginParams.
fn synthetic_flp() -> Vec<u8> {
    synthetic_flp_channel("Bass")
}

fn synthetic_flp_channel(channel: &str) -> Vec<u8> {
    flp_from_events(vec![
        (64, vec![0, 0]),                   // NewChan: channel 0
        (203, channel.as_bytes().to_vec()), // channel name
        (213, synthetic_plugin_params()),
    ])
}

/// Two channels ("Bass", "Lead"), each holding a Serum instance with the
/// same preset name "SynthTest" but distinct chunk content, so both survive
/// the duplicate-chunk skip and reach naming as duplicate preset names.
fn synthetic_flp_duplicate_preset_names() -> Vec<u8> {
    flp_from_events(vec![
        (64, vec![0, 0]),        // NewChan: channel 0
        (203, b"Bass".to_vec()), // channel name
        (213, synthetic_plugin_params_with(0)),
        (64, vec![1, 0]),        // NewChan: channel 1
        (203, b"Lead".to_vec()), // channel name
        (213, synthetic_plugin_params_with(1)),
    ])
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
fn extract_honors_name_template() {
    let dir = temp_dir("template");
    let flp_path = dir.join("test_project.flp");
    std::fs::write(&flp_path, synthetic_flp()).unwrap();

    let out_dir = dir.join("out");
    let status = Command::new(BIN)
        .args(["extract", "-o"])
        .arg(&out_dir)
        .args(["--name-template", "{channel}_{index}"])
        .arg(&flp_path)
        .status()
        .unwrap();
    assert!(status.success(), "extract failed");

    // Synthetic FLP: channel "Bass", preset "SynthTest" -> template wins.
    let fxp_path = out_dir.join("Bass_01.fxp");
    assert!(fxp_path.exists(), "expected {}", fxp_path.display());
    assert!(!out_dir.join("01_SynthTest.fxp").exists());
}

#[test]
fn extract_default_template_duplicates_match_legacy_names() {
    let dir = temp_dir("dup_default");
    let flp_path = dir.join("test_project.flp");
    std::fs::write(&flp_path, synthetic_flp_duplicate_preset_names()).unwrap();

    let out_dir = dir.join("out");
    let status = Command::new(BIN)
        .args(["extract", "-o"])
        .arg(&out_dir)
        .arg(&flp_path)
        .status()
        .unwrap();
    assert!(status.success(), "extract failed");

    // Pre-template behavior: dedup keys on the index-free preset name, so
    // the second "SynthTest" gets its suffix between the index and the base.
    let first = out_dir.join("01_SynthTest.fxp");
    let second = out_dir.join("02_SynthTest_2.fxp");
    assert!(first.exists(), "expected {}", first.display());
    assert!(second.exists(), "expected {}", second.display());
    // The regression (dedup keyed on the templated name) produced a bare
    // 02_SynthTest.fxp instead.
    assert!(!out_dir.join("02_SynthTest.fxp").exists());
}

#[test]
fn extract_custom_template_duplicates_get_suffix() {
    let dir = temp_dir("dup_custom");
    let flp_path = dir.join("test_project.flp");
    std::fs::write(&flp_path, synthetic_flp_duplicate_preset_names()).unwrap();

    let out_dir = dir.join("out");
    let status = Command::new(BIN)
        .args(["extract", "-o"])
        .arg(&out_dir)
        .args(["--name-template", "{preset}"])
        .arg(&flp_path)
        .status()
        .unwrap();
    assert!(status.success(), "extract failed");

    // Custom templates dedup on the whole rendered name.
    let first = out_dir.join("SynthTest.fxp");
    let second = out_dir.join("SynthTest_2.fxp");
    assert!(first.exists(), "expected {}", first.display());
    assert!(second.exists(), "expected {}", second.display());
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

#[test]
fn extract_skips_cross_file_duplicates() {
    let dir = temp_dir("dup");
    let a = dir.join("a_project.flp");
    let b = dir.join("b_project.flp");
    std::fs::write(&a, synthetic_flp_channel("Bass")).unwrap();
    std::fs::write(&b, synthetic_flp_channel("Lead")).unwrap();

    let out_dir = dir.join("out");
    let output = Command::new(BIN)
        .args(["extract", "-o"])
        .arg(&out_dir)
        .arg(&a)
        .arg(&b)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = format!("duplicate of {}:{:02}", a.display(), 1);
    assert!(stdout.contains(&expected), "{stdout}");

    let written = std::fs::read_dir(&out_dir).unwrap().count();
    assert_eq!(written, 1, "duplicate must not be written");
}

#[test]
fn extract_keep_duplicates_writes_both() {
    let dir = temp_dir("keepdup");
    let a = dir.join("a_project.flp");
    let b = dir.join("b_project.flp");
    std::fs::write(&a, synthetic_flp_channel("Bass")).unwrap();
    std::fs::write(&b, synthetic_flp_channel("Lead")).unwrap();

    let output = Command::new(BIN)
        .args(["extract", "--keep-duplicates"])
        .arg(&a)
        .arg(&b)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let a_fxp = dir.join("a_project_serum_fxp/01_SynthTest.fxp");
    let b_fxp = dir.join("b_project_serum_fxp/01_SynthTest.fxp");
    assert!(a_fxp.exists(), "expected {}", a_fxp.display());
    assert!(b_fxp.exists(), "expected {}", b_fxp.display());
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
// patch subcommand
// ---------------------------------------------------------------------------

fn extracted_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extracted_serum1.fxp")
}

#[test]
fn patch_fixture_then_validate() {
    let fixture = extracted_fixture();
    if !fixture.exists() {
        eprintln!("skipping: untracked fixture absent (docs/flp-conversion.md)");
        return;
    }
    let dir = temp_dir("patch");
    let fxp = dir.join("patched.fxp");
    std::fs::copy(&fixture, &fxp).unwrap();

    let output = Command::new(BIN)
        .args(["patch", "--name", "Renamed Patch Test", "--out"])
        .arg(&fxp)
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "patch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("patching 1 field(s)"), "{stdout}");
    assert!(stdout.contains("'Renamed Patch Test'"), "{stdout}");
    assert!(stdout.contains("validate: PASS"), "{stdout}");

    // The header prgName carries the new name; the CLI validator accepts it.
    let data = std::fs::read(&fxp).unwrap();
    assert_eq!(&data[0x1C..0x1C + 18], b"Renamed Patch Test");
    assert_eq!(&data[0x1C + 18..0x38], &[0u8; 10]);
    let status = Command::new(BIN)
        .args(["validate"])
        .arg(&fxp)
        .status()
        .unwrap();
    assert!(status.success(), "validate failed after patch");
}

#[test]
fn patch_dry_run_makes_no_changes() {
    let fixture = extracted_fixture();
    if !fixture.exists() {
        eprintln!("skipping: untracked fixture absent (docs/flp-conversion.md)");
        return;
    }
    let before = std::fs::read(&fixture).unwrap();
    let output = Command::new(BIN)
        .args([
            "patch",
            "--name",
            "Should Not Persist",
            "--author",
            "Nobody",
            "--dry-run",
        ])
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "dry-run patch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dry run: no files written"), "{stdout}");
    let after = std::fs::read(&fixture).unwrap();
    assert_eq!(before, after, "dry-run must not touch the input file");
}

#[test]
fn patch_rejects_missing_flags_and_bad_files() {
    let dir = temp_dir("patch_bad");
    let fxp = dir.join("x.fxp");
    std::fs::write(&fxp, b"not an fxp at all, but long enough to parse.......").unwrap();

    // No patch flags at all.
    let output = Command::new(BIN).arg("patch").arg(&fxp).output().unwrap();
    assert!(!output.status.success(), "no-flag patch must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("nothing to patch"),
        "expected a 'nothing to patch' error"
    );

    // A flag, but garbage input.
    let output = Command::new(BIN)
        .args(["patch", "--name", "X"])
        .arg(&fxp)
        .output()
        .unwrap();
    assert!(!output.status.success(), "garbage input must fail");
}

#[test]
fn patch_flp_fixture_updates_and_reextracts() {
    if !have_serina1() {
        return;
    }
    const NEW_NAME: &str = "Renamed Patch Test";
    let dir = temp_dir("patch_flp");
    let out = dir.join("patched.flp");
    let output = Command::new(BIN)
        .args(["patch", "--name", NEW_NAME, "--out"])
        .arg(&out)
        .arg(serina1_fixture())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "flp patch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("patching 5 Serum instance(s)"), "{stdout}");
    assert_eq!(stdout.matches(NEW_NAME).count(), 5, "{stdout}");

    // Non-target events byte-identical: same event count, every non-213
    // event exactly as before.
    let orig = std::fs::read(serina1_fixture()).unwrap();
    let patched = std::fs::read(&out).unwrap();
    let evs_old = flp_extract_fxp::flp::parse_events(&orig).unwrap();
    let evs_new = flp_extract_fxp::flp::parse_events(&patched).unwrap();
    assert_eq!(evs_old.len(), evs_new.len());
    for (o, n) in evs_old.iter().zip(&evs_new) {
        if o.id == 213 {
            continue;
        }
        assert_eq!((o.id, &o.data), (n.id, &n.data));
    }

    // The patched state is visible via re-extraction.
    let out_dir = dir.join("re");
    let status = Command::new(BIN)
        .args(["extract", "--overwrite", "-o"])
        .arg(&out_dir)
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success(), "re-extract failed");
    let renamed: Vec<_> = std::fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(NEW_NAME))
        .collect();
    assert_eq!(renamed.len(), 5, "expected 5 renamed presets");
}

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

// ---------------------------------------------------------------------------
// Zipped loop packages (PK-prefixed ZIP exports): every `*.flp` member is
// processed as its own document (src/zip.rs + core::flp_inputs).
// ---------------------------------------------------------------------------

/// Raw-deflate one buffer (ZIP method 8).
fn deflate_raw(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), Compression::new(6));
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// Assemble a minimal ZIP archive around `(name, data)` members (CRC fields
/// left zero — the reader ignores them).
fn zip_archive(members: &[(&str, Vec<u8>)], deflate: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in members {
        let name = name.as_bytes();
        let (method, payload) = if deflate {
            (8u16, deflate_raw(data))
        } else {
            (0u16, data.clone())
        };
        let local_offset = out.len() as u32;
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // mod time
        out.extend_from_slice(&0u16.to_le_bytes()); // mod date
        out.extend_from_slice(&0u32.to_le_bytes()); // crc32
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name);
        out.extend_from_slice(&payload);
        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0u16.to_le_bytes()); // flags
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // mod time
        central.extend_from_slice(&0u16.to_le_bytes()); // mod date
        central.extend_from_slice(&0u32.to_le_bytes()); // crc32
        central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra len
        central.extend_from_slice(&0u16.to_le_bytes()); // comment len
        central.extend_from_slice(&0u16.to_le_bytes()); // disk number
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&local_offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let count = members.len() as u16;
    let cd_size = central.len() as u32;
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // disk number
    out.extend_from_slice(&0u16.to_le_bytes()); // CD disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len
    out
}

/// PluginParams payload with the additional FL bookkeeping records the FLP
/// converter keeps verbatim (cid 1/2/30/32/50), so the synthetic project is
/// convertible end-to-end.
fn convertible_plugin_params() -> Vec<u8> {
    let state = synthetic_vst3_wrapper_state();
    let mut data = Vec::new();
    data.extend_from_slice(&12u32.to_le_bytes());
    for (cid, payload) in [
        (1u32, vec![7u8; 20]),
        (2u32, vec![7u8; 16]),
        (30u32, vec![7u8; 8]),
        (32u32, vec![7u8; 8]),
        (50u32, vec![7u8; 12]),
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

/// Like [`synthetic_flp`], but with a convertible PluginParams payload.
fn convertible_flp() -> Vec<u8> {
    let events: Vec<(u8, Vec<u8>)> = vec![
        (64, vec![0, 0]),        // NewChan: channel 0
        (203, b"Bass".to_vec()), // channel name
        (213, convertible_plugin_params()),
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

/// Patch the declared uncompressed size of the first central-directory entry
/// (to build archives that lie about their decompressed size).
fn patch_cd_uncompressed_size(archive: &mut [u8], new_size: u32) {
    let eocd = archive
        .windows(4)
        .rposition(|w| w == b"PK\x05\x06")
        .expect("EOCD signature");
    let cd_offset = u32::from_le_bytes(archive[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    archive[cd_offset + 24..cd_offset + 28].copy_from_slice(&new_size.to_le_bytes());
}

#[test]
fn list_and_extract_from_zipped_loop_package() {
    for deflate in [false, true] {
        let tag = if deflate { "deflate" } else { "store" };
        let dir = temp_dir(&format!("zip_{tag}"));
        let zip_path = dir.join("pack.zip");
        std::fs::write(
            &zip_path,
            zip_archive(
                &[
                    ("song.flp", synthetic_flp()),
                    ("docs/readme.txt", b"loop package".to_vec()),
                ],
                deflate,
            ),
        )
        .unwrap();

        // list: the preset is reported under the member name.
        let output = Command::new(BIN)
            .args(["list"])
            .arg(&zip_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "list ({tag}) failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("pack.zip#song.flp: 1 Serum preset(s)"),
            "{tag}: {stdout}"
        );

        // extract: the .fxp lands in the archive-named output directory.
        let out_dir = dir.join("out");
        let status = Command::new(BIN)
            .args(["extract", "-o"])
            .arg(&out_dir)
            .arg(&zip_path)
            .status()
            .unwrap();
        assert!(status.success(), "extract ({tag}) failed");
        let fxp_path = out_dir.join("01_SynthTest.fxp");
        assert!(fxp_path.exists(), "{tag}: expected {}", fxp_path.display());
    }
}

#[test]
fn zipped_loop_package_with_wrong_extensions_only() {
    let dir = temp_dir("zip_noext");
    let zip_path = dir.join("pack.zip");
    std::fs::write(
        &zip_path,
        zip_archive(&[("readme.txt", b"no project here".to_vec())], false),
    )
    .unwrap();
    let output = Command::new(BIN)
        .args(["list"])
        .arg(&zip_path)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "list should fail without any .flp member"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("without any .flp member"), "{stderr}");
}

#[test]
fn zipped_loop_package_dedupes_across_members() {
    let dir = temp_dir("zip_dedupe");
    let zip_path = dir.join("pack.zip");
    std::fs::write(
        &zip_path,
        zip_archive(
            &[
                ("song.flp", synthetic_flp()),
                ("copy/SONG.FLP", synthetic_flp()),
            ],
            true,
        ),
    )
    .unwrap();
    let output = Command::new(BIN)
        .args(["list"])
        .arg(&zip_path)
        .output()
        .unwrap();
    assert!(output.status.success(), "list failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("pack.zip#song.flp: 1 Serum preset(s)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("pack.zip#copy/SONG.FLP: 1 Serum preset(s)"),
        "{stdout}"
    );

    let out_dir = dir.join("out");
    let output = Command::new(BIN)
        .args(["extract", "-o"])
        .arg(&out_dir)
        .arg(&zip_path)
        .output()
        .unwrap();
    assert!(output.status.success(), "extract failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("duplicate of ") && stdout.contains(":01, skipped"),
        "{stdout}"
    );
    // Only one .fxp is written for the identical pair.
    let written: Vec<_> = std::fs::read_dir(&out_dir).unwrap().collect();
    assert_eq!(written.len(), 1, "{:?}", written.len());
}

#[test]
fn oversized_zipped_loop_package_is_rejected() {
    let dir = temp_dir("zip_oversize");
    let zip_path = dir.join("bomb.zip");
    let mut archive = zip_archive(&[("song.flp", synthetic_flp())], true);
    // Declare a 300 MiB decompressed size (the sanity cap is 256 MiB).
    patch_cd_uncompressed_size(&mut archive, 300 * 1024 * 1024);
    std::fs::write(&zip_path, archive).unwrap();
    let output = Command::new(BIN)
        .args(["list"])
        .arg(&zip_path)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "oversized archive should be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("sanity limit"), "{stderr}");
}

#[test]
fn corrupt_zip_reports_a_zip_error() {
    let dir = temp_dir("zip_corrupt");
    let zip_path = dir.join("broken.zip");
    let mut archive = zip_archive(&[("song.flp", synthetic_flp())], false);
    archive.truncate(archive.len() - 12);
    std::fs::write(&zip_path, archive).unwrap();
    let output = Command::new(BIN)
        .args(["list"])
        .arg(&zip_path)
        .output()
        .unwrap();
    assert!(!output.status.success(), "truncated archive should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ZIP"), "{stderr}");
}

#[test]
fn convert_zipped_loop_package() {
    let dir = temp_dir("zip_convert");
    let zip_path = dir.join("synth.zip");
    std::fs::write(
        &zip_path,
        zip_archive(&[("song.flp", convertible_flp())], true),
    )
    .unwrap();
    let output = Command::new(BIN)
        .args(["convert"])
        .arg(&zip_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "convert failed: {stdout}{stderr}");
    assert!(
        stdout.contains("synth.zip#song.flp: converting 1 Serum instance(s)"),
        "{stdout}"
    );
    let out_path = dir.join("synth_song_serum2.flp");
    assert!(out_path.exists(), "expected {}", out_path.display());
    let converted = std::fs::read(&out_path).unwrap();
    let (instances, stats) = scan_serum_instances(&converted).unwrap();
    assert!(instances.is_empty(), "Serum instances left after convert");
    assert_eq!(stats.serum2_count, 1);
}

#[test]
fn convert_rejects_out_for_multi_member_zip() {
    let dir = temp_dir("zip_convert_out");
    let zip_path = dir.join("pack.zip");
    std::fs::write(
        &zip_path,
        zip_archive(
            &[("a.flp", synthetic_flp()), ("b.flp", synthetic_flp())],
            false,
        ),
    )
    .unwrap();
    let output = Command::new(BIN)
        .args(["convert", "--out"])
        .arg(dir.join("one.flp"))
        .arg(&zip_path)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "--out with a multi-member archive should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--out cannot be used"), "{stderr}");
}

/// Directory tree for input-resolution tests:
///
/// ```text
/// root/
///   alpha.flp  beta.FLP  c.flp  notes.txt
///   sub/gamma.flp
///   sub/deep/delta.flp
/// ```
fn resolve_tree(tag: &str) -> PathBuf {
    let root = temp_dir(tag);
    for rel in [
        "alpha.flp",
        "beta.FLP",
        "c.flp",
        "notes.txt",
        "sub/gamma.flp",
        "sub/deep/delta.flp",
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        if rel.to_ascii_lowercase().ends_with(".flp") {
            std::fs::write(&path, synthetic_flp()).unwrap();
        } else {
            std::fs::write(&path, b"definitely not a flp").unwrap();
        }
    }
    root
}

fn resolved_count(stdout: &str) -> usize {
    stdout.matches(": 1 Serum preset(s)").count()
}

#[test]
fn list_resolves_directory_recursively() {
    let root = resolve_tree("resolve_dir");
    let output = Command::new(BIN).arg("list").arg(&root).output().unwrap();
    assert!(
        output.status.success(),
        "list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Expected paths are joined component-wise so the comparison is
    // platform-neutral: the binary prints paths with the platform separator
    // (`\` on Windows, `/` elsewhere), and `PathBuf::join("sub/gamma.flp")`
    // would embed a literal `/` in the expected string on Windows.
    let expected: [&[&str]; 4] = [
        &["alpha.flp"],
        &["beta.FLP"],
        &["sub", "gamma.flp"],
        &["sub", "deep", "delta.flp"],
    ];
    for rel in expected {
        let mut p = root.clone();
        for part in rel {
            p.push(part);
        }
        assert!(
            stdout.contains(p.to_str().unwrap()),
            "missing {rel:?}:\n{stdout}"
        );
    }
    // Non-.flp files are never collected (or read).
    assert!(!stdout.contains("notes.txt"), "{stdout}");
    // Deterministic (sorted) order.
    let a = stdout.find("alpha.flp").unwrap();
    let b = stdout.find("beta.FLP").unwrap();
    let d = stdout.find("delta.flp").unwrap();
    let g = stdout.find("gamma.flp").unwrap();
    assert!(a < b && b < d && d < g, "{stdout}");
}

#[test]
fn list_resolves_recursive_glob() {
    let root = resolve_tree("resolve_glob2");
    let pattern = root.join("**").join("*.flp");
    let output = Command::new(BIN)
        .arg("list")
        .arg(&pattern)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // `**` matches zero or more levels: 3 top-level + sub/ + sub/deep/.
    assert_eq!(resolved_count(&String::from_utf8_lossy(&output.stdout)), 5);
}

#[test]
fn list_resolves_single_star_glob() {
    let root = resolve_tree("resolve_glob1");
    let pattern = root.join("*.flp");
    let output = Command::new(BIN)
        .arg("list")
        .arg(&pattern)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Top level only: alpha.flp, beta.FLP, c.flp.
    assert_eq!(resolved_count(&stdout), 3, "{stdout}");
    assert!(!stdout.contains("gamma.flp"), "{stdout}");
    assert!(!stdout.contains("delta.flp"), "{stdout}");
}

#[test]
fn list_resolves_question_mark_glob() {
    let root = resolve_tree("resolve_glob_q");
    let pattern = root.join("?.flp");
    let output = Command::new(BIN)
        .arg("list")
        .arg(&pattern)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // `?` is exactly one character, so only c.flp matches (alpha.flp is too
    // long and beta.FLP has the wrong extension length as well).
    assert_eq!(resolved_count(&stdout), 1, "{stdout}");
    assert!(stdout.contains("c.flp"), "{stdout}");
}

#[test]
fn list_empty_directory_errors() {
    let dir = temp_dir("resolve_empty");
    let output = Command::new(BIN).arg("list").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("no .flp files found in {}", dir.display())),
        "{stderr}"
    );
}

#[test]
fn list_unmatched_glob_errors() {
    let root = resolve_tree("resolve_glob_miss");
    let pattern = root.join("**").join("*.zzz");
    let output = Command::new(BIN)
        .arg("list")
        .arg(&pattern)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("no .flp files found in {}", pattern.display())),
        "{stderr}"
    );
}

#[test]
fn extract_resolves_directory() {
    let root = resolve_tree("resolve_extract");
    let out_dir = root.join("out");
    let status = Command::new(BIN)
        .arg("extract")
        .arg("-o")
        .arg(&out_dir)
        .arg(&root)
        .status()
        .unwrap();
    assert!(status.success(), "extract failed");
    assert!(out_dir.join("01_SynthTest.fxp").exists());
}

#[test]
fn convert_resolves_before_single_input_check() {
    let root = resolve_tree("resolve_convert");
    let out = root.join("conv.flp");
    // The tree resolves to 5 inputs, so --out must be rejected. Proves the
    // convert inputs flow through resolve_inputs before the arity check.
    let output = Command::new(BIN)
        .arg("convert")
        .arg("--out")
        .arg(&out)
        .arg(&root)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--out can only be used with a single input file"),
        "{stderr}"
    );
}

/// Apply the same plan-narrowing pipeline the wasm `convert_flp_selected`
/// uses: rows -> `filter_plans_by_rows` -> bundles -> `apply`.
fn convert_selected(
    orig: &[u8],
    rows: &[u32],
) -> (Vec<u8>, flp_extract_fxp::flpconv::FlpConversionReport) {
    let (plans, _) = scan_convertible_detailed(orig).unwrap();
    let (positions, _) = filter_plans_by_rows(&plans, rows);
    let kept: Vec<InstancePlan> = positions.iter().map(|&i| plans[i].clone()).collect();
    let mut source = RealSource::embedded();
    let bundles: Vec<Option<flp_extract_fxp::flpconv::Serum2Bundle>> = kept
        .iter()
        .map(|p| source.bundle_for(p, &[]).unwrap())
        .collect();
    flp_extract_fxp::flpconv::apply(orig, &kept, &bundles).unwrap()
}

#[test]
fn subset_convert_leaves_unselected_byte_identical() {
    // Two synthetic Serum instances on separate channels; convert only the
    // second one (row 1). Runs without any fixture.
    let params = synthetic_plugin_params();
    let orig = encode_flp(&[
        (64, vec![0, 0]),
        (203, b"Bass".to_vec()),
        (213, params.clone()),
        (64, vec![1, 0]),
        (203, b"Lead".to_vec()),
        (213, params),
    ]);
    let (plans, _) = scan_convertible_detailed(&orig).unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].instance_index, Some(0));
    assert_eq!(plans[1].instance_index, Some(1));

    let (out, report) = convert_selected(&orig, &[1]);
    assert_eq!(report.converted.len(), 1);
    assert_eq!(report.converted[0].channel_name, "Lead");

    let orig_events = flp::parse_events(&orig).unwrap();
    let out_events = flp::parse_events(&out).unwrap();
    assert_eq!(orig_events.len(), out_events.len());
    for (i, (a, b)) in orig_events.iter().zip(&out_events).enumerate() {
        if i == 5 {
            assert_ne!(a.data, b.data, "the selected instance must be rewritten");
        } else {
            assert_eq!(a.data, b.data, "event {i} must stay byte-identical");
        }
    }
}

#[test]
fn subset_convert_real_fixture_leaves_unselected_identical() {
    if !have_serina1() {
        return;
    }
    let orig = std::fs::read(serina1_fixture()).unwrap();
    let (plans, _) = scan_convertible_detailed(&orig).unwrap();
    assert_eq!(plans.len(), 5);

    // Convert rows 1 and 3 only; rows 0, 2, 4 must stay byte-identical.
    let (positions, warnings) = filter_plans_by_rows(&plans, &[1, 3]);
    assert_eq!(positions, vec![1, 3]);
    assert_eq!(warnings.len(), 3);

    let (out, report) = convert_selected(&orig, &[1, 3]);
    assert_eq!(report.converted.len(), 2);

    let orig_events = flp::parse_events(&orig).unwrap();
    let out_events = flp::parse_events(&out).unwrap();
    assert_eq!(orig_events.len(), out_events.len());
    let rewritten: HashSet<usize> = positions.iter().map(|&p| plans[p].event_index).collect();
    for (i, (a, b)) in orig_events.iter().zip(&out_events).enumerate() {
        if rewritten.contains(&i) {
            assert_ne!(a.data, b.data, "event {i} must be rewritten");
        } else {
            assert_eq!(a.data, b.data, "event {i} must stay byte-identical");
        }
    }

    // The converted file scans as 3 Serum (untouched) + 3 Serum2
    // (2 converted + the 1 pre-existing instance).
    let (instances, stats) = scan_serum_instances(&out).unwrap();
    assert_eq!(instances.len(), 3);
    assert_eq!(stats.serum2_count, 3);
}

fn serum_preset_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("serumpreset"))
        })
        .collect();
    out.sort();
    out
}

/// `extract --serum2` on the real-project fixture writes exactly one
/// parseable .SerumPreset (the fixture's single genuine Serum2 instance,
/// controller presetName "Release Cut Piano"). Untracked fixture; skips when
/// absent.
#[test]
fn extract_serum2_writes_preset() {
    if !have_serina1() {
        return;
    }
    let dir = temp_dir("serum2");
    let out_dir = dir.join("out");
    let status = Command::new(BIN)
        .args(["extract", "--serum2", "-o"])
        .arg(&out_dir)
        .arg(serina1_fixture())
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "extract --serum2 failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    let presets = serum_preset_files(&out_dir);
    assert_eq!(presets.len(), 1, "{presets:?}");
    let data = std::fs::read(&presets[0]).unwrap();
    let (json, uncomp, format, foff) = serum2state::parse_xfer_json(&data).unwrap();
    assert_eq!(format, 2);
    assert!(json.contains("\"fileType\":\"SerumPreset\""), "{json}");
    assert!(
        json.contains("\"presetName\":\"Release Cut Piano\""),
        "{json}"
    );
    assert!(json.contains("\"product\":\"Serum2\""), "{json}");

    // hash == md5 of the zstd frame (container rule).
    let frame = &data[foff..];
    let mut h = Md5::new();
    h.update(frame);
    let expect_hash = format!("{:x}", h.finalize());
    assert!(
        json.contains(&format!("\"hash\":\"{expect_hash}\"")),
        "hash mismatch: {json}"
    );

    // Body: authored format — 175 top-level keys, engine-type UI keys
    // present, state-only `component` gone, preset name carried.
    let body = serum2state::decode_zstd(frame).unwrap();
    assert_eq!(body.len() as u32, uncomp);
    let val = s2tree::decode_cbor(&body).unwrap();
    let keys = val.as_map().expect("top-level map");
    assert_eq!(keys.len(), 175);
    assert!(val.get("component").is_none(), "component must be dropped");
    for k in [
        "WTOsc",
        "Osc",
        "MultiSampleOsc",
        "SpectralOsc",
        "GranularOsc",
        "Filter",
        "ClipPlayer",
        "SerumGUI",
        "fileType",
        "presetName",
        "presetAuthor",
        "presetDescription",
        "arpBankDisplayName",
        "clipBankDisplayName",
        "Oscillator0",
        "Oscillator4",
        "ModSlot63",
        "Global0",
    ] {
        assert!(val.get(k).is_some(), "missing authored key {k}");
    }
    assert_eq!(
        val.get("presetName").and_then(s2tree::Val::as_str),
        Some("Release Cut Piano")
    );

    // Without the flag, no .SerumPreset files are written.
    let out_plain = dir.join("out_plain");
    let status = Command::new(BIN)
        .args(["extract", "-o"])
        .arg(&out_plain)
        .arg(serina1_fixture())
        .status()
        .unwrap();
    assert!(status.success(), "plain extract failed");
    assert!(serum_preset_files(&out_plain).is_empty());
}

/// Structural round-trip over 5 real factory presets: parse → authored Val →
/// re-encode container → parse again == same Val. Gated behind
/// `FLPX_S2_CORPUS_DIR` (read-only third-party content; never read in CI).
#[test]
fn corpus_preset_round_trip() {
    let Some(corpus) = std::env::var_os("FLPX_S2_CORPUS_DIR").map(PathBuf::from) else {
        eprintln!("skipping: FLPX_S2_CORPUS_DIR not set");
        return;
    };
    if !corpus.is_dir() {
        eprintln!("skipping: corpus dir absent");
        return;
    }
    let mut files: Vec<PathBuf> = globwalk_serum_presets(&corpus);
    assert!(
        !files.is_empty(),
        "no .SerumPreset files under {}",
        corpus.display()
    );
    files.sort();
    let n = files.len();
    let picks: Vec<usize> = [0, n / 4, n / 2, 3 * n / 4, n - 1].to_vec();
    for idx in picks {
        let path = &files[idx];
        let data = std::fs::read(path).unwrap();
        let (json, _, _, foff) = serum2state::parse_xfer_json(&data)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let meta = serum2state::controller_meta_from_json(&json);
        let original = s2tree::decode_cbor(&serum2state::decode_zstd(&data[foff..]).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let n_keys = original.as_map().unwrap().len();
        assert!(
            (174..=178).contains(&n_keys),
            "{}: unexpected top-level key count {n_keys}",
            path.display()
        );

        let rebuilt = serum2state::build_preset_file(
            &original,
            serum2state::PresetHeader {
                preset_name: meta.preset_name,
                preset_author: meta.preset_author,
                preset_description: meta.preset_description,
            },
        );
        let (json2, _, _, foff2) = serum2state::parse_xfer_json(&rebuilt).unwrap();
        let reparsed = s2tree::decode_cbor(&serum2state::decode_zstd(&rebuilt[foff2..]).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(
            s2tree::encode_cbor(&original),
            s2tree::encode_cbor(&reparsed),
            "{}: round-trip body mismatch",
            path.display()
        );
        // The rebuilt container's hash matches its own frame.
        let mut h = Md5::new();
        h.update(&rebuilt[foff2..]);
        assert!(json2.contains(&format!("\"hash\":\"{:x}\"", h.finalize())));
    }
}

/// Collect `**/*.SerumPreset` below `dir` without external crates.
fn globwalk_serum_presets(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("serumpreset"))
            {
                out.push(p);
            }
        }
    }
    out
}

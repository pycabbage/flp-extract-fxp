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
    for rel in [
        "alpha.flp",
        "beta.FLP",
        "sub/gamma.flp",
        "sub/deep/delta.flp",
    ] {
        let p = root.join(rel);
        assert!(
            stdout.contains(p.to_str().unwrap()),
            "missing {rel}:\n{stdout}"
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

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
use flp_extract_fxp::{flpconv::scan_convertible_detailed, s2tree, serum2state};
use md5::{Digest, Md5};
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

//! End-to-end tests: build a synthetic FLP, extract it with the real CLI
//! binary, and validate the resulting .fxp through the CLI as well.
//!
//! The `convert_*` tests exercise the Serum -> Serum2 FLP converter on a
//! real FL Studio project fixture (5 Serum instances and 1 genuine Serum2
//! instance). That fixture is not tracked in the repo (third-party project
//! content; see docs/flp-conversion.md) — the tests skip when it is absent.

use flate2::Compression;
use flate2::write::ZlibEncoder;
use flp_extract_fxp::flp;
use flp_extract_fxp::flpconv::{
    BundleSource, InstancePlan, RealSource, filter_plans_by_rows, scan_convertible_detailed,
};
use flp_extract_fxp::scan_serum_instances;
use std::collections::HashSet;
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

/// PluginParams (event 213) payload: version + chunk id/size records, in
/// the real Serum layout (identity cids 1/2/30/32/50 + plugin cids
/// 52/54/55/56 + the wrapper in cid 53; docs/flp-serum2-conversion.md §2).
fn synthetic_plugin_params() -> Vec<u8> {
    let state = synthetic_vst3_wrapper_state();
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

fn synthetic_flp() -> Vec<u8> {
    encode_flp(&[
        (64, vec![0, 0]),        // NewChan: channel 0
        (203, b"Bass".to_vec()), // channel name
        (213, synthetic_plugin_params()),
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

//! End-to-end tests: build a synthetic FLP, extract it with the real CLI
//! binary, and validate the resulting .fxp through the CLI as well.

use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_flp-extract-fxp");

fn zlib_stream(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// A synthetic Serum 1 chunk: zlib(preset state) + zlib(table data) + trailer.
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
    let status = Command::new(BIN)
        .args(["validate"])
        .arg(&fixture)
        .status()
        .unwrap();
    assert!(status.success(), "fixture failed validation");
}

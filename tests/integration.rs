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
        stdout.contains("duplicate of an earlier preset"),
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

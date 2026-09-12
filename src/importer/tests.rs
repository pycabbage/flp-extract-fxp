use super::*;

fn fxp_chunk(bytes: &[u8]) -> Vec<u8> {
    assert_eq!(&bytes[..4], b"CcnK");
    let cs = u32::from_be_bytes(bytes[0x38..0x3C].try_into().unwrap()) as usize;
    bytes[0x3C..0x3C + cs].to_vec()
}

/// The real-preset fixtures are intentionally NOT tracked in the repo
/// (third-party preset content; see docs/flp-conversion.md). Tests that
/// need them skip silently when they are absent so CI stays green.
fn fixture(nn: u8) -> Option<(S1Preset, Vec<u8>)> {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let bytes = std::fs::read(base.join(format!("serina1/0{nn}.fxp"))).ok()?;
    let golden = std::fs::read(base.join(format!("golden_s2/0{nn}_processor_state.bin"))).ok()?;
    let preset = s1state::parse_preset(&fxp_chunk(&bytes)).ok()?;
    Some((preset, golden))
}

fn skip_untracked(nn: u8) -> Option<(S1Preset, Vec<u8>)> {
    match fixture(nn) {
        Some(v) => Some(v),
        None => {
            eprintln!("skipping preset {nn}: untracked fixtures absent (docs/flp-conversion.md)");
            None
        }
    }
}

fn diff_leaves(a: &Val, b: &Val, path: String, out: &mut Vec<(String, String, String)>) {
    // Leaves compare byte-equal canonical encodings, so Val-variant
    // differences that encode identically (f32-exact f64 vs f32, Int vs
    // UInt) do not create noise.
    let same = |x: &Val, y: &Val| {
        !matches!(
            (x, y),
            (Val::Map(_), Val::Map(_)) | (Val::Array(_), Val::Array(_))
        ) && crate::s2tree::encode_cbor(x) == crate::s2tree::encode_cbor(y)
    };
    match (a, b) {
        (Val::Map(ma), Val::Map(mb)) => {
            for (k, va) in ma {
                match mb.iter().find(|(k2, _)| k2 == k) {
                    Some((_, vb)) => diff_leaves(va, vb, format!("{path}.{k}"), out),
                    None => out.push((format!("{path}.{k}"), "present".into(), "absent".into())),
                }
            }
            for (k, _) in mb {
                if !ma.iter().any(|(k2, _)| k2 == k) {
                    out.push((format!("{path}.{k}"), "absent".into(), "present".into()));
                }
            }
        }
        (Val::Array(aa), Val::Array(ab)) => {
            if aa.len() != ab.len() {
                out.push((
                    format!("{path}[len]"),
                    format!("{}", aa.len()),
                    format!("{}", ab.len()),
                ));
            }
            for (i, (va, vb)) in aa.iter().zip(ab.iter()).enumerate() {
                diff_leaves(va, vb, format!("{path}[{i}]"), out);
            }
        }
        _ => {
            if !same(a, b) {
                out.push((path, format!("{a:?}"), format!("{b:?}")));
            }
        }
    }
}

#[test]
fn golden_byte_identical_01() {
    golden_one(1);
}
#[test]
fn golden_byte_identical_02() {
    golden_one(2);
}
#[test]
fn golden_byte_identical_03() {
    golden_one(3);
}
#[test]
fn golden_byte_identical_04() {
    golden_one(4);
}
#[test]
fn golden_byte_identical_05() {
    golden_one(5);
}

fn golden_one(nn: u8) {
    let Some((preset, want)) = skip_untracked(nn) else {
        return;
    };
    let conv = convert_s1_to_s2(&preset, 0).expect("convert");
    let record = crate::serum2state::build_processor_record(&conv.body);
    // Container header must match except the frame md5 (`hash` field):
    // the golden frame is a real zstd stream, build_processor_record emits
    // a raw-block frame, so the md5 necessarily differs.
    let (m_json, m_uncomp, m_fmt, foff) = crate::serum2state::parse_xfer_json(&record).unwrap();
    let (w_json, w_uncomp, w_fmt, woff) = crate::serum2state::parse_xfer_json(&want).unwrap();
    let strip = |j: &str| -> String {
        let a = j.find("\"hash\":\"").unwrap() + 8;
        let b = j[a..].find('"').unwrap() + a;
        format!("{}{}", &j[..a], &j[b..])
    };
    assert_eq!(strip(&m_json), strip(&w_json), "container header {nn}");
    // The decisive check: the decoded CBOR body is byte-identical to the golden's.
    let a = decode_frame(&record[foff..]);
    let b = decode_frame(&want[woff..]);
    let my_cbor = crate::s2tree::encode_cbor(&a);
    let want_cbor = crate::s2tree::encode_cbor(&b);
    assert_eq!(my_cbor, want_cbor, "cbor body {nn}");
    assert_eq!(m_uncomp, w_uncomp, "uncompressed size {nn}");
    assert_eq!(m_fmt, w_fmt, "format {nn}");
    // Tree-level diff diagnostics on failure.
    let mut diffs = Vec::new();
    diff_leaves(&a, &b, String::new(), &mut diffs);
    assert!(
        diffs.is_empty(),
        "leaf diffs on {nn}: {:?}",
        &diffs[..8.min(diffs.len())]
    );
}

fn decode_frame(frame: &[u8]) -> Val {
    let out = crate::testutil::decode_zstd_frame(frame);
    crate::s2tree::decode_cbor(&out).expect("cbor")
}

// ---------------------------------------------------------------------------
// Legacy (2015-era) presets: upgraded through parse_preset's zero-padding and
// verified against trees produced by the REAL Serum2 importer, called at
// runtime on the raw fxp chunk (regeneration: docs/flp-conversion.md).
// ---------------------------------------------------------------------------

/// Loads a legacy fxp + the real importer's tree for it. Both files live in
/// the untracked `tests/fixtures/legacy/` directory; the tests skip when they
/// are absent.
fn legacy_fixture(name: &str) -> Option<(S1Preset, Val)> {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy");
    let fxp = std::fs::read(base.join(format!("{name}.fxp"))).ok()?;
    let tree = std::fs::read(base.join(format!("{name}_importer_tree.cbor"))).ok()?;
    let cs = u32::from_be_bytes(fxp[0x38..0x3C].try_into().unwrap()) as usize;
    let preset = s1state::parse_preset(&fxp[0x3C..0x3C + cs]).ok()?;
    let tree = crate::s2tree::decode_cbor(&tree).expect("importer tree cbor");
    Some((preset, tree))
}

/// The real importer's tree merged over the init-body skeleton exactly like
/// `convert_s1_to_s2` merges its own tree (top-level overlay + mpeEnabled
/// normalization) — the expected CBOR body for a legacy fixture.
fn legacy_expected_body(tree: &Val) -> Result<Val, String> {
    let mut body = crate::s2tree::decode_cbor(crate::s2tables::INIT_BODY)?;
    if let (Val::Map(m), Val::Map(t)) = (&mut body, tree) {
        for (k, v) in t.clone() {
            m.retain(|(ek, _)| ek != &k);
            m.push((k, v));
        }
        for (k, v) in m.iter_mut() {
            if k == "mpeEnabled" {
                *v = Val::Bool(false);
            }
        }
    }
    Ok(body)
}

fn legacy_golden_one(name: &str) {
    let Some((preset, tree)) = legacy_fixture(name) else {
        eprintln!("skipping legacy preset {name}: untracked fixtures absent");
        return;
    };
    let conv = convert_s1_to_s2(&preset, 0).expect("convert");
    let want = legacy_expected_body(&tree).expect("skeleton");
    let mine = crate::s2tree::encode_cbor(&conv.body);
    let want_cbor = crate::s2tree::encode_cbor(&want);
    if mine != want_cbor {
        let mut diffs = Vec::new();
        diff_leaves(&conv.body, &want, String::new(), &mut diffs);
        if std::env::var("LEGACY_DIFF_DUMP").is_ok() {
            let path = std::env::temp_dir().join(format!("legacy_diff_{name}.txt"));
            let mut txt = String::new();
            for (p, a, b) in &diffs {
                txt.push_str(&format!("{p}\t{a}\t{b}\n"));
            }
            let _ = std::fs::write(&path, txt);
            eprintln!("full diff dump: {}", path.display());
        }
        panic!(
            "legacy body mismatch on {name}: {} leaf diffs, first: {:?}",
            diffs.len(),
            &diffs[..8.min(diffs.len())]
        );
    }
}

#[test]
fn legacy_golden_fl_bass_adventure() {
    legacy_golden_one("FL_BASS_Adventure");
}
#[test]
fn legacy_golden_fl_beautybeast() {
    legacy_golden_one("FL_BeautyBeast");
}
#[test]
fn legacy_golden_fl_cryptic() {
    legacy_golden_one("FL_Cryptic");
}
#[test]
fn legacy_golden_fl_downpour() {
    legacy_golden_one("FL_Downpour");
}
#[test]
fn legacy_golden_fl_fmitup() {
    legacy_golden_one("FL_FMItUp");
}
#[test]
fn legacy_golden_fl_heavenly() {
    legacy_golden_one("FL_Heavenly");
}

#[test]
fn legacy_parse_reports_legacy_metadata() {
    let Some((preset, _)) = legacy_fixture("FL_Downpour") else {
        eprintln!("skipping: untracked fixtures absent");
        return;
    };
    assert_eq!(preset.blob.len(), s1state::S1_BLOB_SIZE);
    assert!((preset.meta.version_f32 - 0.1531).abs() < 1e-4);
    assert_eq!(preset.meta.preset_name, "ARP - Downpour");
}

/// Deterministic upgrade check without any fixture files: a synthetic
/// legacy-sized (28,232-byte) state chunk parses (zero-padded to the modern
/// size) and converts like any other preset.
#[test]
fn legacy_sized_synthetic_blob_converts() {
    let mut blob = vec![0u8; 28_232];
    blob[s1state::OFF_PRESET_NAME..s1state::OFF_PRESET_NAME + 4].copy_from_slice(b"Old\0");
    blob[s1state::OFF_VERSION_F32..s1state::OFF_VERSION_F32 + 4]
        .copy_from_slice(&0.147f32.to_le_bytes());
    // one live mod slot (marker 80 <slot> FF at +0x21) with a dead-format
    // dest code, restamped by the < 0.148 migration
    blob[0x04..0x08].copy_from_slice(&0.25f32.to_le_bytes());
    blob[0x14..0x16].copy_from_slice(&5u16.to_le_bytes());
    blob[0x1A..0x1C].copy_from_slice(&1u16.to_le_bytes());
    blob[0x20..0x24].copy_from_slice(&[0x80, 0x80, 0x00, 0xFF]);
    let z0 = crate::testutil::zlib_stream(&blob);
    let mut chunk = z0.clone();
    chunk.extend_from_slice(&(z0.len() as u32).to_le_bytes());
    let preset = s1state::parse_preset(&chunk).expect("legacy chunk parses");
    assert_eq!(preset.blob.len(), s1state::S1_BLOB_SIZE);
    assert_eq!(preset.mod_slots.len(), 1);
    let conv = convert_s1_to_s2(&preset, 0).expect("legacy converts");
    assert_eq!(
        conv.body.get("fileType"),
        Some(&Val::Text("SerumPreset".into()))
    );
    let slot0 = conv.body.get("ModSlot0").expect("ModSlot0");
    assert_eq!(
        slot0.get("destModuleTypeString").and_then(|v| v.as_str()),
        Some("Oscillator")
    );
}

#[test]
fn report_sanity_05() {
    let Some((preset, _)) = skip_untracked(5) else {
        return;
    };
    let conv = convert_s1_to_s2(&preset, 0).expect("convert");
    eprintln!("notes: {:?}", conv.report.notes);
    assert!(conv.report.notes.is_empty(), "unexpected notes");
}

#[test]
fn flag_semantics_fx_build() {
    let Some((preset, _)) = skip_untracked(1) else {
        return;
    };
    let conv = convert_s1_to_s2(&preset, 1).expect("convert");
    let osc0 = conv.body.get("Oscillator0").unwrap();
    assert!(osc0.get("WTOsc0").is_none(), "FX build drops WTOsc0");
}

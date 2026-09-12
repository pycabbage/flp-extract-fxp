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
            if format!("{a:?}") != format!("{b:?}") {
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
    // compressed frames are not guaranteed byte-identical across libzstd
    // builds, so the md5 is not guaranteed to match (it happens to match
    // with the goldens' level-3 libzstd).
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

//! Serum2 `XferJson` state container assembly and parsing
//! (`docs/serum2-state-format.md` §1, `docs/flp-serum2-conversion.md` §4)
//! plus the state → authored (`.SerumPreset`) body conversion
//! (`docs/s2-param-corpus.md` §3, §9).
//!
//! Record layout (identical shape for processor, controller and preset
//! records):
//!
//! ```text
//! "XferJson\0" (9 bytes)
//! u64 LE  json header length
//! <json_len bytes>  JSON header, keys sorted, no NUL
//! u32 LE  uncompressed body size
//! u32 LE  format (2)
//! <one zstd frame>  (hash field in the JSON header = md5 of this frame)
//! ```

use std::io::Read;

use md5::{Digest, Md5};

use crate::s2tree::{self, Val};

const MAGIC: &[u8; 9] = b"XferJson\0";

pub(crate) fn md5_hex(data: &[u8]) -> String {
    let mut h = Md5::new();
    h.update(data);
    let digest = h.finalize();
    let mut out = String::with_capacity(32);
    for b in digest.iter() {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Minimal JSON string escaping (quotes, backslash, control chars).
pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Processor JSON header, keys sorted, byte-exact against the canonical
/// Python container builder (`version` is the JSON number 9.0).
pub fn processor_json_header(md5_hex_str: &str) -> String {
    format!(
        "{{\"component\":\"processor\",\"hash\":\"{}\",\"product\":\"Serum2\",\
         \"productVersion\":\"2.0.23\",\"url\":\"https://xferrecords.com/\",\
         \"vendor\":\"Xfer Records\",\"version\":9.0}}",
        json_escape(md5_hex_str)
    )
}

/// Controller JSON header, keys sorted. `product_version` and `version` are
/// interpolated verbatim (the calibrated real-world header uses
/// `productVersion "2.0.22"` / `version 8.0` as a JSON number, so callers
/// pass `"2.0.22"` and `"8.0"`). Free-text fields are JSON-escaped.
pub fn controller_json_header(
    md5_hex_str: &str,
    preset_name: &str,
    preset_author: &str,
    preset_description: &str,
    product_version: &str,
    version: &str,
) -> String {
    format!(
        "{{\"component\":\"controller\",\"hash\":\"{}\",\
         \"presetAuthor\":\"{}\",\"presetDescription\":\"{}\",\"presetName\":\"{}\",\
         \"product\":\"Serum2\",\"productVersion\":\"{}\",\
         \"url\":\"https://xferrecords.com/\",\"vendor\":\"Xfer Records\",\"version\":{}}}",
        json_escape(md5_hex_str),
        json_escape(preset_author),
        json_escape(preset_description),
        json_escape(preset_name),
        json_escape(product_version),
        version
    )
}

fn assemble(header: &str, frame: &[u8], body_len: u32, format: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(MAGIC.len() + 8 + header.len() + 8 + frame.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(header.len() as u64).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&body_len.to_le_bytes());
    out.extend_from_slice(&format.to_le_bytes());
    out.extend_from_slice(frame);
    out
}

/// Build a complete processor `XferJson` record from a CBOR body tree:
/// encode canonically, wrap in a zstd frame, md5 the frame, assemble.
/// `bodyLen` = uncompressed CBOR byte length, `format` = 2,
/// `productVersion` = "2.0.23", `version` = 9.0.
pub fn build_processor_record(body: &Val) -> Vec<u8> {
    let cbor = s2tree::encode_cbor(body);
    let frame = s2tree::zstd_frame(&cbor);
    let header = processor_json_header(&md5_hex(&frame));
    assemble(&header, &frame, cbor.len() as u32, 2)
}

/// Wrap a GIVEN (unmodified) zstd frame — e.g. lifted from a real Serum2
/// controller instance — into a complete `XferJson` record. The caller is
/// responsible for the header text (including its `hash` field) and the
/// declared uncompressed size.
pub fn wrap_controller_frame(
    json_header: String,
    frame: &[u8],
    body_len: u32,
    format: u32,
) -> Vec<u8> {
    assemble(&json_header, frame, body_len, format)
}

/// Decompress one zstd frame with the pure-Rust `ruzstd` decoder (works on
/// native and wasm32 targets alike).
pub fn decode_zstd(frame: &[u8]) -> Result<Vec<u8>, String> {
    let mut dec = ruzstd::decoding::StreamingDecoder::new(std::io::Cursor::new(frame))
        .map_err(|e| format!("zstd: {e}"))?;
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| format!("zstd: {e}"))?;
    Ok(out)
}

/// Extract a JSON string field (`"key":"value"`) from a record header,
/// resolving the standard JSON escapes. Missing fields return `None`.
pub fn json_string_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let pos = json.find(&needle)? + needle.len();
    let bytes = json.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = pos;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return String::from_utf8(out).ok(),
            b'\\' => {
                i += 1;
                match *bytes.get(i)? {
                    b'"' => out.push(b'"'),
                    b'\\' => out.push(b'\\'),
                    b'/' => out.push(b'/'),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0C),
                    b'u' => {
                        let hex = std::str::from_utf8(bytes.get(i + 1..i + 5)?).ok()?;
                        let cp = u32::from_str_radix(hex, 16).ok()?;
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(char::from_u32(cp)?.encode_utf8(&mut buf).as_bytes());
                        i += 4;
                    }
                    _ => return None,
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    None
}

/// Preset identity carried by a Serum2 controller record's JSON header.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ControllerMeta {
    pub preset_name: String,
    pub preset_author: String,
    pub preset_description: String,
}

/// Parse `presetName` / `presetAuthor` / `presetDescription` out of a
/// controller record's JSON header (missing fields stay empty).
pub fn controller_meta_from_json(json: &str) -> ControllerMeta {
    ControllerMeta {
        preset_name: json_string_field(json, "presetName").unwrap_or_default(),
        preset_author: json_string_field(json, "presetAuthor").unwrap_or_default(),
        preset_description: json_string_field(json, "presetDescription").unwrap_or_default(),
    }
}

/// Preset identity handed to [`state_body_to_authored`] (fills the authored
/// body's `presetName` / `presetAuthor` / `presetDescription` metadata keys).
#[derive(Debug, Clone, Default)]
pub struct PresetFileMeta {
    pub preset_name: String,
    pub preset_author: String,
    pub preset_description: String,
}

/// Preset identity for the `.SerumPreset` JSON header built by
/// [`build_preset_file`]. The remaining header fields (`tags`, `product`,
/// `productVersion`, `version`) are derived from the authored body itself.
#[derive(Debug, Clone, Default)]
pub struct PresetHeader {
    pub preset_name: String,
    pub preset_author: String,
    pub preset_description: String,
}

/// Convert an instantiated processor state body (162-key CBOR map,
/// `docs/serum2-state-format.md` §2.2) into the authored `.SerumPreset`
/// body format (`docs/s2-param-corpus.md` §3):
///
/// - the state-only `component` key is dropped;
/// - every per-instance section (`Oscillator0..4`, `Env0..3`, `LFO0..9`,
///   `ModSlot0..63`, `FXRack0..2`, `Global0`, `Macro0..7`, ...) and the meta
///   keys (`product`, `productVersion`, `version`, `tags`, `scalars`, `mpe*`,
///   `lock*`, `url`, `vendor`) are copied 1:1 — verified identical between
///   the formats against the 626-file factory corpus;
/// - the 8 engine-type UI keys (`WTOsc`, `Osc`, `MultiSampleOsc`,
///   `SpectralOsc`, `GranularOsc`, `Filter`, `ClipPlayer`, `SerumGUI`) are
///   synthesized with the corpus-consensus default values (§9; the only
///   non-zero consensus value is `kUIParamShowKeyboard` = 1.0, present in
///   626/626 factory presets);
/// - the metadata keys (`fileType`, `presetName`, `presetAuthor`,
///   `presetDescription`, `arpBankDisplayName`, `clipBankDisplayName`) are
///   added; the bank display names are empty (state carries no bank info).
///
/// 162 state keys − `component` + 14 authored keys = the standard 175.
pub fn state_body_to_authored(state: &Val, meta: PresetFileMeta) -> Val {
    let mut out = Val::obj();
    if let Val::Map(m) = state {
        for (k, v) in m {
            if k != "component" {
                out.set(k, v.clone());
            }
        }
    }

    // Authored-only metadata keys (docs/s2-param-corpus.md §3, §9).
    out.set("fileType", Val::Text("SerumPreset".into()));
    out.set("presetName", Val::Text(meta.preset_name));
    out.set("presetAuthor", Val::Text(meta.preset_author));
    out.set("presetDescription", Val::Text(meta.preset_description));
    out.set("arpBankDisplayName", Val::Text(String::new()));
    out.set("clipBankDisplayName", Val::Text(String::new()));

    // Engine-type UI keys: pure UI state arrays/maps indexed by slot; they
    // carry no engine parameters (those live inside `Oscillator<N>`).
    let ui1 = |key: &str, v: Val| {
        let mut m = Val::obj();
        m.set(key, v);
        m
    };
    let ui2 = |k1: &str, k2: &str| {
        let mut m = Val::obj();
        m.set(k1, Val::F32(0.0));
        m.set(k2, Val::F32(0.0));
        m
    };
    out.set("Filter", ui1("kUIParamMixOrGain", Val::F32(0.0)));
    let mut clip = Val::obj();
    clip.set("kUIParamPreviewClip", Val::F64(1.0 / 12.0));
    clip.set("kUIParamSelectedClip", Val::F32(0.0));
    out.set("ClipPlayer", clip);
    out.set("SerumGUI", ui1("kUIParamShowKeyboard", Val::F32(1.0)));
    out.set(
        "WTOsc",
        Val::Array(vec![ui1("kUIParamWTOverviewMouseTag", Val::F32(0.0)); 3]),
    );
    out.set(
        "Osc",
        Val::Array(
            (0..5)
                .map(|_| {
                    let mut m = Val::obj();
                    m.set("kUIParamAutoSyncSlicing", Val::F32(0.0));
                    m.set("kUIParamShowMarkerAnimations", Val::F32(0.0));
                    m.set("kUIParamZoomToStartEnd", Val::F32(0.0));
                    m
                })
                .collect(),
        ),
    );
    out.set(
        "MultiSampleOsc",
        Val::Array(vec![
            ui1(
                "kUIParamMultiSampleOverviewMouseTag",
                Val::F32(0.0)
            );
            3
        ]),
    );
    out.set(
        "SpectralOsc",
        Val::Array(vec![
            ui2(
                "kUIParamDisplayXYInput",
                "kUIParamShowWaveformDisplay"
            );
            3
        ]),
    );
    out.set(
        "GranularOsc",
        Val::Array(vec![ui1("kUIParamDisplayXYInput", Val::F32(0.0)); 3]),
    );
    out
}

/// Serum2 preset-file JSON header, keys sorted (alphabetical), matching the
/// real factory presets (`docs/s2-param-corpus.md` §2). `version` is
/// interpolated verbatim as a JSON number ("9.0"); free text is escaped.
#[allow(clippy::too_many_arguments)]
pub fn preset_json_header(
    md5_hex_str: &str,
    preset_name: &str,
    preset_author: &str,
    preset_description: &str,
    product: &str,
    product_version: &str,
    tags: &[String],
    version: &str,
) -> String {
    let mut tags_json = String::from("[");
    for (i, t) in tags.iter().enumerate() {
        if i > 0 {
            tags_json.push(',');
        }
        tags_json.push('"');
        tags_json.push_str(&json_escape(t));
        tags_json.push('"');
    }
    tags_json.push(']');
    format!(
        "{{\"fileType\":\"SerumPreset\",\"hash\":\"{}\",\
         \"presetAuthor\":\"{}\",\"presetDescription\":\"{}\",\"presetName\":\"{}\",\
         \"product\":\"{}\",\"productVersion\":\"{}\",\"tags\":{tags_json},\
         \"url\":\"https://xferrecords.com/\",\"vendor\":\"Xfer Records\",\"version\":{version}}}",
        json_escape(md5_hex_str),
        json_escape(preset_author),
        json_escape(preset_description),
        json_escape(preset_name),
        json_escape(product),
        json_escape(product_version),
    )
}

/// Format a JSON-header `version` number the way the real headers do: one
/// decimal for integral values (`9.0`), shortest form otherwise.
fn format_version(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// Build a complete `.SerumPreset` file from an authored body: encode
/// canonically, wrap in a raw zstd frame, md5 the frame, assemble with the
/// preset JSON header. `tags`, `product`, `productVersion` and `version`
/// are taken from the body itself (they were copied 1:1 from the state);
/// missing values fall back to the current-build constants
/// (`"Serum2"` / `"2.0.23"` / `9.0`).
pub fn build_preset_file(authored: &Val, header: PresetHeader) -> Vec<u8> {
    let cbor = s2tree::encode_cbor(authored);
    let frame = s2tree::zstd_frame(&cbor);
    let product = authored
        .get("product")
        .and_then(Val::as_str)
        .unwrap_or("Serum2");
    let product_version = authored
        .get("productVersion")
        .and_then(Val::as_str)
        .unwrap_or("2.0.23");
    let version = match authored.get("version").and_then(Val::as_f64) {
        Some(v) => format_version(v),
        None => "9.0".to_string(),
    };
    let tags: Vec<String> = match authored.get("tags") {
        Some(Val::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    let json = preset_json_header(
        &md5_hex(&frame),
        &header.preset_name,
        &header.preset_author,
        &header.preset_description,
        product,
        product_version,
        &tags,
        &version,
    );
    assemble(&json, &frame, cbor.len() as u32, 2)
}

/// One-shot `.SerumPreset` assembly from a Serum2 processor record (the
/// inner cid-3 `XferJson` payload of a plugin instance) plus the controller
/// metadata (preset name/author/description from the inner cid-2 JSON
/// header). The processor state body is decompressed, converted to the
/// authored format ([`state_body_to_authored`]) and re-encoded into a fresh
/// container ([`build_preset_file`], recomputed md5).
pub fn preset_file_from_processor(
    processor: &[u8],
    meta: ControllerMeta,
) -> Result<Vec<u8>, String> {
    let (_, _, _, foff) = parse_xfer_json(processor)?;
    let body = decode_zstd(&processor[foff..])?;
    let state = s2tree::decode_cbor(&body)?;
    let authored = state_body_to_authored(
        &state,
        PresetFileMeta {
            preset_name: meta.preset_name.clone(),
            preset_author: meta.preset_author.clone(),
            preset_description: meta.preset_description.clone(),
        },
    );
    Ok(build_preset_file(
        &authored,
        PresetHeader {
            preset_name: meta.preset_name,
            preset_author: meta.preset_author,
            preset_description: meta.preset_description,
        },
    ))
}

/// Parse an `XferJson` record: returns
/// `(json_text, declared_uncompressed_size, format, frame_start_offset)`.
pub fn parse_xfer_json(record: &[u8]) -> Result<(String, u32, u32, usize), String> {
    if record.len() < 17 || &record[..9] != MAGIC {
        return Err(format!(
            "XferJson: bad magic at byte 0 ({} bytes total), expected \"XferJson\\0\"",
            record.len()
        ));
    }
    let jlen = u64::from_le_bytes(record[9..17].try_into().expect("16 bytes")) as usize;
    let json_end = 17usize
        .checked_add(jlen)
        .ok_or_else(|| "XferJson: json length overflow".to_string())?;
    if record.len() < json_end + 8 {
        return Err(format!(
            "XferJson: truncated record at byte {} (need {} for JSON + u32 pair)",
            record.len(),
            json_end + 8
        ));
    }
    let json = String::from_utf8(record[17..json_end].to_vec())
        .map_err(|_| "XferJson: JSON header is not valid UTF-8".to_string())?;
    let uncomp = u32::from_le_bytes(record[json_end..json_end + 4].try_into().expect("4 bytes"));
    let format = u32::from_le_bytes(
        record[json_end + 4..json_end + 8]
            .try_into()
            .expect("4 bytes"),
    );
    Ok((json, uncomp, format, json_end + 8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::decode_zstd_frame;

    #[test]
    fn processor_header_exact_text() {
        let hdr = processor_json_header("56a4a7cd2d58b933d65be68878ca06ec");
        let expected = "{\"component\":\"processor\",\
\"hash\":\"56a4a7cd2d58b933d65be68878ca06ec\",\
\"product\":\"Serum2\",\
\"productVersion\":\"2.0.23\",\
\"url\":\"https://xferrecords.com/\",\
\"vendor\":\"Xfer Records\",\"version\":9.0}";
        assert_eq!(hdr, expected);
        assert_eq!(hdr.len(), 183);
    }

    #[test]
    fn controller_header_exact_text() {
        let hdr = controller_json_header(
            "06efd53571617f74445b1037e0ce055a",
            "Release Cut Piano",
            "Kagi",
            "kagimusic.com",
            "2.0.22",
            "8.0",
        );
        let expected = "{\"component\":\"controller\",\
\"hash\":\"06efd53571617f74445b1037e0ce055a\",\
\"presetAuthor\":\"Kagi\",\
\"presetDescription\":\"kagimusic.com\",\
\"presetName\":\"Release Cut Piano\",\
\"product\":\"Serum2\",\
\"productVersion\":\"2.0.22\",\
\"url\":\"https://xferrecords.com/\",\
\"vendor\":\"Xfer Records\",\"version\":8.0}";
        assert_eq!(hdr, expected);
    }

    #[test]
    fn controller_header_escapes_strings() {
        let hdr = controller_json_header(
            "a",
            "na\"me\\x",
            "au\tthor",
            "de\nscription",
            "2.0.22",
            "8.0",
        );
        assert!(hdr.contains("\"presetName\":\"na\\\"me\\\\x\""), "{hdr}");
        assert!(hdr.contains("\"presetAuthor\":\"au\\tthor\""), "{hdr}");
        assert!(
            hdr.contains("\"presetDescription\":\"de\\nscription\""),
            "{hdr}"
        );
    }

    #[test]
    fn processor_record_round_trip() {
        let mut body = Val::obj();
        body.set("version", Val::F32(9.0));
        let mut env0 = Val::obj();
        let mut params = Val::obj();
        params.set("kParamCurve1", Val::F32(50.0));
        env0.set("plainParams", params);
        body.set("Env0", env0);

        let rec = build_processor_record(&body);

        // Magic and u64 json_len at offset 9.
        assert_eq!(&rec[..9], b"XferJson\0");
        let jlen = u64::from_le_bytes(rec[9..17].try_into().unwrap());
        assert_eq!(jlen as usize, processor_json_header(&"0".repeat(32)).len());

        let (json, uncomp, format, foff) = parse_xfer_json(&rec).expect("parse");
        assert_eq!(format, 2);
        let frame = &rec[foff..];
        assert_eq!(uncomp as usize, s2tree::encode_cbor(&body).len());
        assert_eq!(s2tree::zstd_frame_body_len(frame), Some(uncomp as usize));

        // hash == md5(frame), header text exact.
        let expect_json = processor_json_header(&md5_hex(frame));
        assert_eq!(json, expect_json);
        assert!(json.contains(&format!("\"hash\":\"{}\"", md5_hex(frame))));

        // Frame decompresses (ruzstd) back to the canonical CBOR body.
        let body_bytes = decode_zstd_frame(frame);
        assert_eq!(body_bytes, s2tree::encode_cbor(&body));
        assert_eq!(body_bytes.len(), uncomp as usize);

        // Body decodes back to a tree that re-encodes identically (map entry
        // order differs after decode, but canonical encoding sorts keys).
        let tree = s2tree::decode_cbor(&body_bytes).expect("cbor");
        assert_eq!(s2tree::encode_cbor(&tree), s2tree::encode_cbor(&body));
    }

    #[test]
    fn controller_record_wrap_and_parse() {
        let payload = b"controller body bytes".to_vec();
        let frame = s2tree::zstd_frame(&payload);
        let hdr = controller_json_header(
            &md5_hex(&frame),
            "Release Cut Piano",
            "Kagi",
            "kagimusic.com",
            "2.0.22",
            "8.0",
        );
        let rec = wrap_controller_frame(hdr.clone(), &frame, payload.len() as u32, 2);

        assert_eq!(&rec[..9], b"XferJson\0");
        let jlen = u64::from_le_bytes(rec[9..17].try_into().unwrap());
        assert_eq!(jlen as usize, hdr.len());
        let (json, uncomp, format, foff) = parse_xfer_json(&rec).expect("parse");
        assert_eq!(json, hdr);
        assert_eq!(uncomp, payload.len() as u32);
        assert_eq!(format, 2);
        assert_eq!(&rec[foff..], &frame[..]);
        assert_eq!(decode_zstd_frame(&rec[foff..]), payload);
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert!(parse_xfer_json(b"").is_err());
        assert!(parse_xfer_json(b"XferJson").is_err());
        assert!(parse_xfer_json(b"NotXferJson000000").is_err());
        // Truncated after the json length field.
        let mut rec = build_processor_record(&Val::obj());
        rec.truncate(20);
        assert!(parse_xfer_json(&rec).is_err());
    }

    #[test]
    fn json_string_field_handles_escapes() {
        let json = r#"{"a":"plain","b":"na\"me\\x","c":"tab\there","d":"音楽"}"#;
        assert_eq!(json_string_field(json, "a").as_deref(), Some("plain"));
        assert_eq!(json_string_field(json, "b").as_deref(), Some("na\"me\\x"));
        assert_eq!(json_string_field(json, "c").as_deref(), Some("tab\there"));
        assert_eq!(json_string_field(json, "d").as_deref(), Some("音楽"));
        assert_eq!(json_string_field(json, "missing"), None);
        assert_eq!(json_string_field(r#"{"a":42}"#, "a"), None);
        assert_eq!(controller_meta_from_json(json).preset_name, "");
        let ctrl = controller_meta_from_json(
            r#"{"component":"controller","presetAuthor":"Kagi","presetName":"Release Cut Piano"}"#,
        );
        assert_eq!(ctrl.preset_name, "Release Cut Piano");
        assert_eq!(ctrl.preset_author, "Kagi");
        assert_eq!(ctrl.preset_description, "");
    }

    /// The generated init body is a genuine 162-key instantiated state.
    fn init_state() -> Val {
        s2tree::decode_cbor(crate::s2tables::INIT_BODY).expect("init body decodes")
    }

    #[test]
    fn state_to_authored_key_set() {
        let state = init_state();
        let n_state = state.as_map().unwrap().len();
        assert_eq!(n_state, 162, "init state must have the 162 documented keys");
        let authored = state_body_to_authored(
            &state,
            PresetFileMeta {
                preset_name: "My Preset".into(),
                preset_author: "Me".into(),
                preset_description: String::new(),
            },
        );
        let keys: Vec<&str> = authored
            .as_map()
            .unwrap()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(keys.len(), 175, "162 - component + 14 authored keys");
        assert!(!keys.contains(&"component"));
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
        ] {
            assert!(keys.contains(&k), "missing authored key {k}");
        }
        // Meta and per-instance sections carried 1:1.
        assert_eq!(authored.get("version"), Some(&Val::F32(9.0)));
        assert_eq!(
            authored.get("productVersion").and_then(Val::as_str),
            Some("2.0.23")
        );
        assert_eq!(
            authored.get("presetName").and_then(Val::as_str),
            Some("My Preset")
        );
        assert!(authored.get("Oscillator0").is_some());
        assert!(authored.get("ModSlot63").is_some());
        assert_eq!(
            authored.get("tags"),
            state.get("tags"),
            "tags copied 1:1 (2 entries in the state format)"
        );
    }

    #[test]
    fn state_to_authored_engine_ui_shapes() {
        let authored = state_body_to_authored(&init_state(), PresetFileMeta::default());
        // WTOsc: array[3] of {kUIParamWTOverviewMouseTag: f32 0.0}
        let wt = authored.get("WTOsc").unwrap();
        let Val::Array(wt) = wt else { panic!("array") };
        assert_eq!(wt.len(), 3);
        assert_eq!(
            wt[0].get("kUIParamWTOverviewMouseTag"),
            Some(&Val::F32(0.0))
        );
        // Osc: array[5] of the 3-key UI map.
        let Val::Array(osc) = authored.get("Osc").unwrap() else {
            panic!("array")
        };
        assert_eq!(osc.len(), 5);
        assert_eq!(osc[4].get("kUIParamAutoSyncSlicing"), Some(&Val::F32(0.0)));
        // SpectralOsc / GranularOsc array[3].
        let Val::Array(spec) = authored.get("SpectralOsc").unwrap() else {
            panic!("array")
        };
        assert_eq!(spec.len(), 3);
        assert_eq!(
            spec[2].get("kUIParamShowWaveformDisplay"),
            Some(&Val::F32(0.0))
        );
        let Val::Array(gran) = authored.get("GranularOsc").unwrap() else {
            panic!("array")
        };
        assert_eq!(gran.len(), 3);
        // Filter / ClipPlayer / SerumGUI maps with corpus-consensus defaults.
        assert_eq!(
            authored.get("Filter").unwrap().get("kUIParamMixOrGain"),
            Some(&Val::F32(0.0))
        );
        let clip = authored.get("ClipPlayer").unwrap();
        assert_eq!(clip.get("kUIParamPreviewClip"), Some(&Val::F64(1.0 / 12.0)));
        assert_eq!(clip.get("kUIParamSelectedClip"), Some(&Val::F32(0.0)));
        assert_eq!(
            authored
                .get("SerumGUI")
                .unwrap()
                .get("kUIParamShowKeyboard"),
            Some(&Val::F32(1.0))
        );
    }

    #[test]
    fn preset_header_exact_text() {
        let hdr = preset_json_header(
            "d5a9c4fb4af5ec301d34f711fca17477",
            "ARP - Aardvark",
            "Audiotent",
            "www.audiotent.com",
            "Serum2",
            "2.0.13",
            &[
                "Wavetable".to_string(),
                "Mono".to_string(),
                "Arp".to_string(),
                "Preview".to_string(),
            ],
            "5.0",
        );
        let expected = "{\"fileType\":\"SerumPreset\",\
\"hash\":\"d5a9c4fb4af5ec301d34f711fca17477\",\
\"presetAuthor\":\"Audiotent\",\
\"presetDescription\":\"www.audiotent.com\",\
\"presetName\":\"ARP - Aardvark\",\
\"product\":\"Serum2\",\"productVersion\":\"2.0.13\",\
\"tags\":[\"Wavetable\",\"Mono\",\"Arp\",\"Preview\"],\
\"url\":\"https://xferrecords.com/\",\"vendor\":\"Xfer Records\",\"version\":5.0}";
        assert_eq!(hdr, expected);
    }

    #[test]
    fn preset_file_round_trip() {
        let authored = state_body_to_authored(
            &init_state(),
            PresetFileMeta {
                preset_name: "Round Trip".into(),
                preset_author: "Tester".into(),
                preset_description: "desc".into(),
            },
        );
        let file = build_preset_file(
            &authored,
            PresetHeader {
                preset_name: "Round Trip".into(),
                preset_author: "Tester".into(),
                preset_description: "desc".into(),
            },
        );

        let (json, uncomp, format, foff) = parse_xfer_json(&file).expect("parse");
        assert_eq!(format, 2);
        let frame = &file[foff..];
        assert_eq!(uncomp as usize, s2tree::encode_cbor(&authored).len());
        // hash == md5(frame); header fields derived from the body.
        assert_eq!(
            json,
            preset_json_header(
                &md5_hex(frame),
                "Round Trip",
                "Tester",
                "desc",
                "Serum2",
                "2.0.23",
                &["Wavetable".into(), "Poly".into()],
                "9.0"
            )
        );
        let body = decode_zstd(frame).expect("zstd");
        assert_eq!(body.len(), uncomp as usize);
        let back = s2tree::decode_cbor(&body).expect("cbor");
        assert_eq!(s2tree::encode_cbor(&back), s2tree::encode_cbor(&authored));
        assert_eq!(
            back.get("fileType").and_then(Val::as_str),
            Some("SerumPreset")
        );
    }

    #[test]
    fn preset_header_version_fallbacks() {
        // Body without version/productVersion -> current-build constants.
        let bare = Val::obj();
        let file = build_preset_file(&bare, PresetHeader::default());
        let (json, _, _, _) = parse_xfer_json(&file).unwrap();
        assert!(json.contains("\"product\":\"Serum2\""), "{json}");
        assert!(json.contains("\"productVersion\":\"2.0.23\""), "{json}");
        assert!(json.contains("\"version\":9.0"), "{json}");
        // f32 5.0 in the body renders as 5.0 (one decimal), like real files.
        let mut body = Val::obj();
        body.set("version", Val::F32(5.0));
        let (json, _, _, _) =
            parse_xfer_json(&build_preset_file(&body, PresetHeader::default())).unwrap();
        assert!(json.contains("\"version\":5.0"), "{json}");
    }

    #[test]
    fn preset_from_processor_end_to_end() {
        let state = init_state();
        let processor = build_processor_record(&state);
        let meta = ControllerMeta {
            preset_name: "Release Cut Piano".into(),
            preset_author: "Kagi".into(),
            preset_description: "kagimusic.com".into(),
        };
        let file = preset_file_from_processor(&processor, meta.clone()).expect("build");
        let (json, _, _, _) = parse_xfer_json(&file).unwrap();
        assert_eq!(controller_meta_from_json(&json), meta);
        assert!(json.contains("\"fileType\":\"SerumPreset\""));
        let (_, _, _, foff) = parse_xfer_json(&file).unwrap();
        let val = s2tree::decode_cbor(&decode_zstd(&file[foff..]).unwrap()).unwrap();
        assert_eq!(val.as_map().unwrap().len(), 175);
        assert!(val.get("component").is_none());
    }
}

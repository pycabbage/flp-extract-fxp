//! Serum 2 `XferJson` state container assembly and parsing
//! (`docs/serum2-state-format.md` §1, `docs/flp-serum2-conversion.md` §4).
//!
//! Record layout (identical shape for processor and controller records):
//!
//! ```text
//! "XferJson\0" (9 bytes)
//! u64 LE  json header length
//! <json_len bytes>  JSON header, keys sorted, no NUL
//! u32 LE  uncompressed body size
//! u32 LE  format (2)
//! <one zstd frame>  (hash field in the JSON header = md5 of this frame)
//! ```

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
/// encode canonically, wrap in a raw zstd frame, md5 the frame, assemble.
/// `bodyLen` = uncompressed CBOR byte length, `format` = 2,
/// `productVersion` = "2.0.23", `version` = 9.0.
pub fn build_processor_record(body: &Val) -> Vec<u8> {
    let cbor = s2tree::encode_cbor(body);
    let frame = s2tree::zstd_raw_frame(&cbor);
    let header = processor_json_header(&md5_hex(&frame));
    assemble(&header, &frame, cbor.len() as u32, 2)
}

/// Wrap a GIVEN (unmodified) zstd frame — e.g. lifted from a real Serum 2
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
        let frame = s2tree::zstd_raw_frame(&payload);
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
}

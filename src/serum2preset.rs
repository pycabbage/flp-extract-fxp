//! `.SerumPreset` output container assembly (the native Serum2 preset file).
//!
//! EXPERIMENTAL: the body wrapped here is the **processor-state variant**
//! (the same CBOR body `serum2state::build_processor_record` encodes for the
//! VST3 instance state), not Serum2's *authored* preset format — the authored
//! body carries extra UI-state top-level keys and omits the state-only
//! `component` key (docs/s2-param-corpus.md §3). A follow-up will convert the
//! body to the authored format; until then the output is honestly labeled as
//! experimental and final UI-load confirmation is an owner-side step.
//!
//! Container layout — byte-compatible with the 626 factory presets
//! (docs/s2-param-corpus.md §2):
//!
//! ```text
//! "XferJson\0" (9 bytes)
//! u64 LE  json header length
//! <json_len bytes>  JSON header, keys sorted, no NUL
//! u32 LE  uncompressed body size
//! u32 LE  format (2)
//! <one zstd frame>  (header `hash` = md5 of this frame)
//! ```
//!
//! Header keys (alphabetical, all factory presets):
//! `fileType` ("SerumPreset"), `hash`, `presetAuthor`, `presetDescription`,
//! `presetName`, `product` ("Serum2"), `productVersion`, `tags` (array),
//! `url`, `vendor` ("Xfer Records"), `version`.

use crate::serum2state::{json_escape, md5_hex};

const MAGIC: &[u8; 9] = b"XferJson\0";

/// Serum2 preset file `version` field and `productVersion` written by the
/// current converter (matches the processor records this crate emits).
const PRESET_PRODUCT_VERSION: &str = "2.0.23";
const PRESET_VERSION: &str = "9.0";

/// Preset-style JSON header, keys sorted (matches the factory-preset corpus).
/// `tags` entries are JSON-escaped; free-text fields too; `version` is the
/// JSON number `9.0`.
pub fn preset_json_header(
    md5_hex_str: &str,
    preset_name: &str,
    preset_author: &str,
    preset_description: &str,
    tags: &[String],
) -> String {
    let tags_json: Vec<String> = tags
        .iter()
        .map(|t| format!("\"{}\"", json_escape(t)))
        .collect();
    format!(
        "{{\"fileType\":\"SerumPreset\",\"hash\":\"{}\",\
         \"presetAuthor\":\"{}\",\"presetDescription\":\"{}\",\"presetName\":\"{}\",\
         \"product\":\"Serum2\",\"productVersion\":\"{PRESET_PRODUCT_VERSION}\",\
         \"tags\":[{}],\"url\":\"https://xferrecords.com/\",\
         \"vendor\":\"Xfer Records\",\"version\":{PRESET_VERSION}}}",
        json_escape(md5_hex_str),
        json_escape(preset_author),
        json_escape(preset_description),
        json_escape(preset_name),
        tags_json.join(","),
    )
}

/// Wrap a GIVEN (unmodified) zstd frame into a complete `.SerumPreset`
/// container. The caller supplies the frame, its uncompressed size and the
/// preset metadata; the header `hash` is computed here as md5 of the frame.
///
/// The frame is normally lifted unchanged from the converted processor record
/// so that both containers embed the identical CBOR body (and therefore share
/// the same `hash`).
pub fn build_preset_container(
    frame: &[u8],
    body_len: u32,
    preset_name: &str,
    preset_author: &str,
    preset_description: &str,
    tags: &[String],
) -> Vec<u8> {
    let header = preset_json_header(
        &md5_hex(frame),
        preset_name,
        preset_author,
        preset_description,
        tags,
    );
    let mut out = Vec::with_capacity(MAGIC.len() + 8 + header.len() + 8 + frame.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(header.len() as u64).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(&body_len.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(frame);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s2tree::{self, Val};
    use crate::serum2state::build_processor_record;
    use crate::testutil::decode_zstd_frame;

    #[test]
    fn header_exact_text() {
        let hdr = preset_json_header(
            "d5a9c4fb4af5ec301d34f711fca17477",
            "ARP - Aardvark",
            "Audiotent",
            "www.audiotent.com",
            &[
                "Wavetable".into(),
                "Mono".into(),
                "Arp".into(),
                "Preview".into(),
            ],
        );
        let expected = "{\"fileType\":\"SerumPreset\",\
\"hash\":\"d5a9c4fb4af5ec301d34f711fca17477\",\
\"presetAuthor\":\"Audiotent\",\
\"presetDescription\":\"www.audiotent.com\",\
\"presetName\":\"ARP - Aardvark\",\
\"product\":\"Serum2\",\
\"productVersion\":\"2.0.23\",\
\"tags\":[\"Wavetable\",\"Mono\",\"Arp\",\"Preview\"],\
\"url\":\"https://xferrecords.com/\",\
\"vendor\":\"Xfer Records\",\"version\":9.0}";
        assert_eq!(hdr, expected);
    }

    #[test]
    fn header_escapes_strings_and_empty_tags() {
        let hdr = preset_json_header("a", "na\"me\\x", "au\tthor", "de\nscription", &[]);
        assert!(hdr.contains("\"presetName\":\"na\\\"me\\\\x\""), "{hdr}");
        assert!(hdr.contains("\"presetAuthor\":\"au\\tthor\""), "{hdr}");
        assert!(
            hdr.contains("\"presetDescription\":\"de\\nscription\""),
            "{hdr}"
        );
        assert!(hdr.contains("\"tags\":[]"), "{hdr}");
    }

    #[test]
    fn container_parses_back_and_shares_body_with_processor_record() {
        let mut body = Val::obj();
        body.set("presetName", Val::Text("X".into()));
        let proc_rec = build_processor_record(&body);
        let (json, uncomp, format, foff) = crate::serum2state::parse_xfer_json(&proc_rec).unwrap();
        assert_eq!(format, 2);

        let preset = build_preset_container(
            &proc_rec[foff..],
            uncomp,
            "X",
            "auth",
            "desc",
            &["Wavetable".into(), "Poly".into()],
        );
        assert_eq!(&preset[..9], b"XferJson\0");
        let (pj, puncomp, pformat, pfoff) = crate::serum2state::parse_xfer_json(&preset).unwrap();
        assert!(pj.contains("\"fileType\":\"SerumPreset\""), "{pj}");
        assert!(pj.contains("\"presetName\":\"X\""), "{pj}");
        assert!(pj.contains("\"tags\":[\"Wavetable\",\"Poly\"]"), "{pj}");
        assert!(pj.contains("\"version\":9.0"), "{pj}");
        assert_eq!(pformat, 2);
        assert_eq!(puncomp, uncomp);
        // Identical frame => identical hash field.
        assert_eq!(&preset[pfoff..], &proc_rec[foff..]);
        assert!(pj.contains(&format!("\"hash\":\"{}\"", md5_hex(&preset[pfoff..]))));
        // And the same decompressed CBOR body as the processor record.
        let (_, _, _, rec_foff) = crate::serum2state::parse_xfer_json(&proc_rec).unwrap();
        let body_bytes = decode_zstd_frame(&preset[pfoff..]);
        assert_eq!(body_bytes, decode_zstd_frame(&proc_rec[rec_foff..]));
        assert_eq!(body_bytes.len(), puncomp as usize);
        assert_eq!(
            s2tree::encode_cbor(&s2tree::decode_cbor(&body_bytes).unwrap()),
            s2tree::encode_cbor(&body)
        );
    }
}

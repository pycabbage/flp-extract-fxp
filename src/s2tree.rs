//! Serum2 state body tree: a deterministic CBOR (RFC 8949 subset) value tree
//! with encode/decode plus zstd frame compression, matching the canonical
//! Python reference encoder byte-for-byte (validated against 460+ KB of
//! plugin-produced bodies; see `docs/serum2-state-format.md` and
//! `docs/flp-conversion.md`).
//!
//! Canonical rules reproduced here:
//! - map keys emitted in sorted byte-lexicographic order; duplicate keys
//!   collapse to the LAST inserted value at encode time;
//! - integers use minimal-length CBOR headers (major 0 for `UInt` and
//!   non-negative `Int`, major 1 for negative `Int` as `-1 - n`);
//! - `F32` always encodes as `0xFA` + big-endian f32; `F64` encodes as `0xFA`
//!   when the value round-trips through f32 exactly (and is not NaN),
//!   otherwise `0xFB` + big-endian f64.

/// A decoded CBOR value tree. Map keys are kept as `String`s; encode order is
/// decided at encode time, not insertion time.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    UInt(u64),
    Int(i64),
    F32(f32),
    F64(f64),
    Text(String),
    Bytes(Vec<u8>),
    Bool(bool),
    Null,
    Array(Vec<Val>),
    Map(Vec<(String, Val)>),
}

impl Val {
    /// New empty map.
    pub fn obj() -> Val {
        Val::Map(Vec::new())
    }

    /// New empty array.
    pub fn arr() -> Val {
        Val::Array(Vec::new())
    }

    /// First value stored under `key` (map only).
    pub fn get(&self, key: &str) -> Option<&Val> {
        match self {
            Val::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Mutable first value stored under `key` (map only).
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Val> {
        match self {
            Val::Map(m) => m.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Replace the value under `key`, or append a new entry. No-op on
    /// non-map values.
    pub fn set(&mut self, key: &str, val: Val) {
        if let Val::Map(m) = self {
            match m.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = val,
                None => m.push((key.to_string(), val)),
            }
        }
    }

    /// Get or insert an empty map under `key` and return it. If `self` is not
    /// a map it is replaced by an empty map first (deterministic fallback).
    pub fn obj_at(&mut self, key: &str) -> &mut Val {
        if !matches!(self, Val::Map(_)) {
            *self = Val::Map(Vec::new());
        }
        let m = match self {
            Val::Map(m) => m,
            _ => unreachable!(),
        };
        let idx = match m.iter().position(|(k, _)| k == key) {
            Some(i) => i,
            None => {
                m.push((key.to_string(), Val::Map(Vec::new())));
                m.len() - 1
            }
        };
        &mut m[idx].1
    }

    /// Get or insert an empty array under `key` and return it. If `self` is
    /// not a map it is replaced by an empty map first (deterministic fallback).
    pub fn arr_at(&mut self, key: &str) -> &mut Val {
        if !matches!(self, Val::Map(_)) {
            *self = Val::Map(Vec::new());
        }
        let m = match self {
            Val::Map(m) => m,
            _ => unreachable!(),
        };
        let idx = match m.iter().position(|(k, _)| k == key) {
            Some(i) => i,
            None => {
                m.push((key.to_string(), Val::Array(Vec::new())));
                m.len() - 1
            }
        };
        &mut m[idx].1
    }

    /// Append to an array; silently ignored on non-array values (panic-safe).
    pub fn push(&mut self, v: Val) {
        if let Val::Array(a) = self {
            a.push(v);
        }
    }

    /// Numeric view: `F32`/`F64`/`UInt`/`Int`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Val::F32(f) => Some(*f as f64),
            Val::F64(f) => Some(*f),
            Val::UInt(u) => Some(*u as f64),
            Val::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Integer view: `Int`/`UInt` (saturating) and truncated `F32`/`F64`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Val::Int(i) => Some(*i),
            Val::UInt(u) => Some(*u as i64),
            Val::F32(f) => Some(*f as i64),
            Val::F64(f) => Some(*f as i64),
            _ => None,
        }
    }

    /// Text view.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Map view.
    pub fn as_map(&self) -> Option<&Vec<(String, Val)>> {
        match self {
            Val::Map(m) => Some(m),
            _ => None,
        }
    }
}

fn write_head(out: &mut Vec<u8>, n: u64, major: u8) {
    let maj = major << 5;
    if n < 24 {
        out.push(maj | n as u8);
    } else if n <= 0xFF {
        out.push(maj | 24);
        out.push(n as u8);
    } else if n <= 0xFFFF {
        out.push(maj | 25);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else if n <= 0xFFFF_FFFF {
        out.push(maj | 26);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    } else {
        out.push(maj | 27);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

fn encode_f64_like(out: &mut Vec<u8>, v: f64) {
    let f = v as f32;
    if !v.is_nan() && f as f64 == v {
        out.push(0xFA);
        out.extend_from_slice(&f.to_be_bytes());
    } else {
        out.push(0xFB);
        out.extend_from_slice(&v.to_be_bytes());
    }
}

fn encode_val(v: &Val, out: &mut Vec<u8>) {
    match v {
        Val::Null => out.push(0xF6),
        Val::Bool(b) => out.push(if *b { 0xF5 } else { 0xF4 }),
        Val::Text(s) => {
            let raw = s.as_bytes();
            write_head(out, raw.len() as u64, 3);
            out.extend_from_slice(raw);
        }
        Val::Bytes(b) => {
            write_head(out, b.len() as u64, 2);
            out.extend_from_slice(b);
        }
        Val::UInt(u) => write_head(out, *u, 0),
        // -1 - i == !i in two's complement; overflow-free for all i64.
        Val::Int(i) => {
            if *i >= 0 {
                write_head(out, *i as u64, 0);
            } else {
                write_head(out, !(*i as u64), 1);
            }
        }
        Val::F32(f) => {
            out.push(0xFA);
            out.extend_from_slice(&f.to_be_bytes());
        }
        Val::F64(f) => encode_f64_like(out, *f),
        Val::Array(items) => {
            write_head(out, items.len() as u64, 4);
            for item in items {
                encode_val(item, out);
            }
        }
        Val::Map(m) => {
            // Stable-sort entry indices by key bytes, then keep the LAST
            // entry of every equal-key run (duplicate keys: last wins).
            let mut order: Vec<usize> = (0..m.len()).collect();
            order.sort_by(|&a, &b| m[a].0.as_bytes().cmp(m[b].0.as_bytes()));
            let mut sel: Vec<usize> = Vec::with_capacity(order.len());
            for (pos, &i) in order.iter().enumerate() {
                let is_last =
                    pos + 1 == order.len() || m[order[pos + 1]].0.as_bytes() != m[i].0.as_bytes();
                if is_last {
                    sel.push(i);
                }
            }
            write_head(out, sel.len() as u64, 5);
            for &i in &sel {
                encode_val(&Val::Text(m[i].0.clone()), out);
                encode_val(&m[i].1, out);
            }
        }
    }
}

/// Canonical CBOR encode (see module docs for the rules).
pub fn encode_cbor(v: &Val) -> Vec<u8> {
    let mut out = Vec::new();
    encode_val(v, &mut out);
    out
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8, String> {
        if self.p >= self.d.len() {
            return Err(format!("cbor: unexpected end of input at byte {}", self.p));
        }
        let b = self.d[self.p];
        self.p += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], String> {
        if self.d.len() - self.p < n {
            return Err(format!(
                "cbor: truncated {what} at byte {} (need {n} more bytes)",
                self.p
            ));
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }

    fn be_u(&mut self, n: usize, what: &str) -> Result<u64, String> {
        let s = self.take(n, what)?;
        let mut v = 0u64;
        for &b in s {
            v = (v << 8) | b as u64;
        }
        Ok(v)
    }

    fn item(&mut self) -> Result<Val, String> {
        let start = self.p;
        let b = self.u8()?;
        let major = b >> 5;
        let info = b & 0x1F;
        let arg = match info {
            0..=23 => info as u64,
            24 => self.be_u(1, "argument")?,
            25 => self.be_u(2, "argument")?,
            26 => self.be_u(4, "argument")?,
            27 => self.be_u(8, "argument")?,
            31 => {
                return Err(format!(
                    "cbor: indefinite length item at byte {start} not supported"
                ));
            }
            _ => {
                return Err(format!(
                    "cbor: reserved additional info {info} at byte {start}"
                ));
            }
        };
        match major {
            0 => Ok(Val::UInt(arg)),
            1 => {
                if arg <= i64::MAX as u64 {
                    Ok(Val::Int(-1 - arg as i64))
                } else {
                    Err(format!(
                        "cbor: negative integer -1-({arg}) at byte {start} out of i64 range"
                    ))
                }
            }
            2 => {
                let n = arg as usize;
                Ok(Val::Bytes(self.take(n, "byte string")?.to_vec()))
            }
            3 => {
                let n = arg as usize;
                let raw = self.take(n, "text string")?;
                match std::str::from_utf8(raw) {
                    Ok(s) => Ok(Val::Text(s.to_string())),
                    Err(_) => Err(format!("cbor: invalid UTF-8 text at byte {start}")),
                }
            }
            4 => {
                let n = arg as usize;
                let mut out = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    out.push(self.item()?);
                }
                Ok(Val::Array(out))
            }
            5 => {
                let n = arg as usize;
                let mut out = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    let key = match self.item()? {
                        Val::Text(k) => k,
                        other => {
                            return Err(format!(
                                "cbor: non-text map key {other:?} at byte {start}"
                            ));
                        }
                    };
                    out.push((key, self.item()?));
                }
                Ok(Val::Map(out))
            }
            6 => {
                // Tag: consume and decode the tagged item (tags are unused in
                // Serum2 bodies but tolerated for strict forward reads).
                self.item()
            }
            _ => match info {
                20 => Ok(Val::Bool(false)),
                21 => Ok(Val::Bool(true)),
                22 | 23 => Ok(Val::Null),
                25 => {
                    let h = arg as u16;
                    Ok(Val::F64(decode_half(h)))
                }
                26 => Ok(Val::F32(f32::from_bits(arg as u32))),
                27 => Ok(Val::F64(f64::from_bits(arg))),
                _ => Err(format!(
                    "cbor: unsupported simple value (info {info}) at byte {start}"
                )),
            },
        }
    }
}

fn decode_half(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (h >> 10) & 0x1F;
    let mant = (h & 0x3FF) as f64;
    match exp {
        0 => sign * mant * 2f64.powi(-24),
        31 => {
            if mant == 0.0 {
                sign * f64::INFINITY
            } else {
                f64::NAN
            }
        }
        e => sign * (mant + 1024.0) * 2f64.powi(e as i32 - 25),
    }
}

/// Strict CBOR decode: rejects indefinite lengths, trailing garbage and
/// out-of-i64-range negatives; overlong-but-valid headers are accepted.
/// Errors carry byte offsets.
pub fn decode_cbor(data: &[u8]) -> Result<Val, String> {
    let mut r = Reader { d: data, p: 0 };
    let v = r.item()?;
    if r.p != data.len() {
        return Err(format!(
            "cbor: {} trailing bytes after document at byte {}",
            data.len() - r.p,
            r.p
        ));
    }
    Ok(v)
}

/// Compress `data` into a single standard zstd frame (libzstd level 3 via
/// `zstd::bulk::compress`). The frame declares its content size in the
/// header and decompresses with any standard zstd decoder, including
/// Serum2's importer.
pub fn zstd_frame(data: &[u8]) -> Vec<u8> {
    zstd::bulk::compress(data, 3).expect("zstd compression of in-memory data cannot fail")
}

/// Parse a zstd frame header and return the declared frame content size.
/// Returns `None` when the magic is wrong, the header is truncated or the
/// content size is not declared.
pub fn zstd_frame_body_len(frame: &[u8]) -> Option<usize> {
    if frame.len() < 6 || frame[0..4] != [0x28, 0xB5, 0x2F, 0xFD] {
        return None;
    }
    let fhd = frame[4];
    let single_segment = fhd & 0x20 != 0;
    let fcs_flag = fhd >> 6;
    let mut p = 5;
    if !single_segment {
        p += 1; // window descriptor
    }
    let dict_bytes = match fhd & 0x03 {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 4,
    };
    if frame.len() < p + dict_bytes {
        return None;
    }
    p += dict_bytes;
    match fcs_flag {
        // Flag 0 + single segment: raw 1-byte size (libzstd uses this for
        // content <= 255 B; no offset — see ZSTD_getFrameHeader, fcsId 0).
        0 => {
            if single_segment && frame.len() > p {
                Some(frame[p] as usize)
            } else {
                None
            }
        }
        // Flag 1: 2-byte field, +256 offset.
        1 => {
            if frame.len() < p + 2 {
                return None;
            }
            Some(u16::from_le_bytes(frame[p..p + 2].try_into().ok()?) as usize + 256)
        }
        2 => {
            if frame.len() < p + 4 {
                return None;
            }
            Some(u32::from_le_bytes(frame[p..p + 4].try_into().ok()?) as usize)
        }
        _ => {
            if frame.len() < p + 8 {
                return None;
            }
            Some(u64::from_le_bytes(frame[p..p + 8].try_into().ok()?) as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::decode_zstd_frame;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn round_trip_all_variants() {
        let mut root = Val::obj();
        root.set("a_u40", Val::UInt(1u64 << 40));
        root.set("b_neg", Val::Int(-1));
        root.set("c_f32", Val::F32(-0.5));
        root.set("d_f64", Val::F64(49.99999999999999));
        root.set("e_f64exact", Val::F64(50.0));
        root.set("f_utf8", Val::Text("音楽".to_string()));
        root.set("g_bytes", Val::Bytes(vec![0x00, 0x01, 0x02, 0xFF]));
        root.set("h_true", Val::Bool(true));
        root.set("h_false", Val::Bool(false));
        root.set("i_null", Val::Null);
        let mut arr = Val::arr();
        arr.push(Val::UInt(1));
        arr.push(Val::Text("x".to_string()));
        arr.push(Val::Null);
        root.set("j_arr", arr);
        let mut big = Val::obj();
        for i in 0..30u64 {
            big.set(&format!("k{i:02}"), Val::UInt(i));
        }
        root.set("big30", big);

        let enc = encode_cbor(&root);
        let dec = decode_cbor(&enc).expect("decode");
        assert_eq!(enc, encode_cbor(&dec), "re-encode must be byte-identical");

        // Spot checks on the first encoding.
        assert!(
            contains(
                &enc,
                &[0x1B, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]
            ),
            "2^40 as 0x1B+u64"
        );
        assert!(contains(&enc, &[0x20]), "-1 as 0x20");
        assert!(
            contains(&enc, &[0xFA, 0x42, 0x48, 0x00, 0x00]),
            "f64 50.0 -> 0xFA f32 50.0"
        );
        assert!(
            contains(
                &enc,
                &[0xFB, 0x40, 0x48, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
            ),
            "49.99999999999999 -> 0xFB"
        );
        // "音楽" is 6 UTF-8 bytes -> 0x66.
        assert!(
            contains(&enc, &[0x66, 0xE9, 0x9F, 0xB3, 0xE6, 0xA5, 0xBD]),
            "multibyte text"
        );
        // 30-entry map uses the 0xB8 header.
        assert!(contains(&enc, &[0xB8, 30]), "0xB8 count path");
        // Decoded views.
        assert_eq!(
            dec.get("a_u40").unwrap().as_f64(),
            Some((1u64 << 40) as f64)
        );
        assert_eq!(dec.get("b_neg").unwrap().as_i64(), Some(-1));
        assert_eq!(dec.get("e_f64exact").unwrap().as_f64(), Some(50.0));
        assert_eq!(dec.get("d_f64").unwrap().as_f64(), Some(49.99999999999999));
        assert_eq!(dec.get("f_utf8").unwrap().as_str(), Some("音楽"));
        assert!(dec.get("i_null").is_some());
        assert_eq!(dec.get("big30").unwrap().as_map().unwrap().len(), 30);
        // 2-element map path (0xA2) exercised by every inner map; check a
        // minimal one explicitly.
        let mut two = Val::obj();
        two.set("x", Val::UInt(1));
        two.set("y", Val::UInt(2));
        assert_eq!(hex(&encode_cbor(&two)), "a2617801617902");
    }

    #[test]
    fn map_key_order_is_sorted_and_last_wins() {
        let mut m = Val::obj();
        m.set("b", Val::UInt(1));
        m.set("a", Val::UInt(2));
        m.set("c", Val::UInt(3));
        m.set("a", Val::UInt(9)); // duplicate: last wins
        let enc = encode_cbor(&m);
        // map[3]: "a"->9, "b"->1, "c"->3
        let expect = "a3"          // map(3)
            .to_string()
            + "61" + "61" + "09"  // "a": 9
            + "61" + "62" + "01"  // "b": 1
            + "61" + "63" + "03"; // "c": 3
        assert_eq!(hex(&enc), expect);
    }

    #[test]
    fn decode_init_body() {
        let body = decode_cbor(crate::s2tables::INIT_BODY).expect("init body must decode");
        let map = body.as_map().expect("top level must be a map");
        assert!(map.len() >= 160, "init top map has {} keys", map.len());
        match body.get("version") {
            Some(Val::F32(f)) => assert_eq!(*f, 9.0),
            other => panic!("version key not f32 9.0: {other:?}"),
        }
        assert!(body.get("Arp0").is_some());
        assert!(body.get("Oscillator0").is_some());
    }

    #[test]
    fn zstd_frames_round_trip_via_ruzstd() {
        // Real init body (6460 B) -> one standard frame with a declared FCS.
        let frame = zstd_frame(crate::s2tables::INIT_BODY);
        assert!(frame.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]), "magic");
        assert!(frame.len() < crate::s2tables::INIT_BODY.len(), "compressed");
        assert_eq!(
            zstd_frame_body_len(&frame),
            Some(crate::s2tables::INIT_BODY.len())
        );
        assert_eq!(decode_zstd_frame(&frame), crate::s2tables::INIT_BODY);

        // Synthetic 300,000-byte body -> one frame, multiple blocks.
        let big: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let frame = zstd_frame(&big);
        assert_eq!(decode_zstd_frame(&frame), big);
        assert_eq!(zstd_frame_body_len(&frame), Some(300_000));

        // 0-byte and 1-byte inputs.
        let frame = zstd_frame(&[]);
        assert_eq!(decode_zstd_frame(&frame), Vec::<u8>::new());
        assert_eq!(zstd_frame_body_len(&frame), Some(0));
        let frame = zstd_frame(b"x");
        assert_eq!(decode_zstd_frame(&frame), b"x".to_vec());
        assert_eq!(zstd_frame_body_len(&frame), Some(1));
    }

    #[test]
    fn canonical_vector_env0_curve1() {
        // Tree equivalent of the documented init record
        // `Env0.plainParams.kParamCurve1 = f32 50.0`, hand-derived from
        // canonical_cbor_encoder.py: sorted keys, minimal int/text headers,
        // f32-exact values as 0xFA + BE f32.
        let mut params = Val::obj();
        params.set("kParamCurve1", Val::F32(50.0));
        let mut plain = Val::obj();
        plain.set("plainParams", params);
        let mut root = Val::obj();
        root.set("Env0", plain);

        let expected_hex =
            "a164456e7630a16b706c61696e506172616d73a16c6b506172616d437572766531fa42480000";
        assert_eq!(hex(&encode_cbor(&root)), expected_hex);
        // And it decodes back to the same tree.
        let dec = decode_cbor(&encode_cbor(&root)).unwrap();
        assert_eq!(encode_cbor(&dec), encode_cbor(&root));
    }

    #[test]
    fn decode_strictness() {
        // Trailing garbage.
        assert!(decode_cbor(&[0xA0, 0x00]).is_err());
        // Indefinite length rejected.
        assert!(decode_cbor(&[0x9F, 0xFF]).is_err());
        // Truncated.
        assert!(decode_cbor(&[0x63, 0x61]).is_err());
        // Overlong-but-valid encodings accepted: uint 0 in 0x18+u8 form.
        assert_eq!(decode_cbor(&[0x18, 0x00]).unwrap(), Val::UInt(0));
        assert_eq!(decode_cbor(&[0xF4]).unwrap(), Val::Bool(false));
        assert_eq!(decode_cbor(&[0xF6]).unwrap(), Val::Null);
        // Offset in the error message: the truncated text payload starts at
        // byte 1 (after the 0x63 header).
        let err = decode_cbor(&[0x63, 0x61]).unwrap_err();
        assert!(err.contains("byte 1"), "{err}");
    }
}

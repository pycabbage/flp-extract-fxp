//! Test-only helpers shared by the crate-internal test modules
//! (`tests/integration.rs` is a separate crate and keeps its own copies).

use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::{Read, Write};

/// Minimal one-stream zlib encoding (level 1).
pub(crate) fn zlib_stream(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// Assemble a minimal FLP file around `(event id, payload)` pairs
/// (fixed-size framing asserted, varint framing for ids >= 192).
pub(crate) fn build_flp(events: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut dt = Vec::new();
    for (id, data) in events {
        dt.push(*id);
        if *id >= 192 {
            // varint length
            let mut len = data.len() as u32;
            loop {
                let b = (len & 0x7f) as u8;
                len >>= 7;
                if len == 0 {
                    dt.push(b);
                    break;
                }
                dt.push(b | 0x80);
            }
        } else {
            assert_eq!(
                data.len(),
                match id {
                    0..=63 => 1,
                    64..=127 => 2,
                    _ => 4,
                }
            );
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

/// Decompress a zstd frame with the test-only `ruzstd` decoder (panics on
/// malformed frames).
pub(crate) fn decode_zstd_frame(frame: &[u8]) -> Vec<u8> {
    let mut dec =
        ruzstd::decoding::StreamingDecoder::new(std::io::Cursor::new(frame)).expect("zstd init");
    let mut out = Vec::new();
    dec.read_to_end(&mut out).expect("zstd read");
    out
}

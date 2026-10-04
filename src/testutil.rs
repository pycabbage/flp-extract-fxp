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

/// Raw-deflate one buffer (for ZIP method-8 test entries).
pub(crate) fn deflate_raw(data: &[u8]) -> Vec<u8> {
    use flate2::write::DeflateEncoder;
    let mut e = DeflateEncoder::new(Vec::new(), Compression::new(6));
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// Assemble a minimal ZIP archive around `(name, data)` members (CRC fields
/// left zero — the reader ignores them). `deflate` compresses every member
/// with method 8; otherwise all members are stored (method 0).
pub(crate) fn zip_archive(members: &[(&str, Vec<u8>)], deflate: bool) -> Vec<u8> {
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
        out.extend_from_slice(&0x04034b50u32.to_le_bytes()); // local header sig
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
        central.extend_from_slice(&0x02014b50u32.to_le_bytes()); // CD sig
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
    out.extend_from_slice(&0x06054b50u32.to_le_bytes()); // EOCD sig
    out.extend_from_slice(&0u16.to_le_bytes()); // disk number
    out.extend_from_slice(&0u16.to_le_bytes()); // CD disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len
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

// ---------------------------------------------------------------------------
// Event-213 payload builders (plain plugins)
// ---------------------------------------------------------------------------

/// Decode a lowercase/uppercase hex string (panics on malformed input).
pub(crate) fn hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex digit"))
        .collect()
}

/// One `[u32 cid][u64 size][data]` record.
pub(crate) fn rec(cid: u32, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(12 + data.len());
    v.extend_from_slice(&cid.to_le_bytes());
    v.extend_from_slice(&(data.len() as u64).to_le_bytes());
    v.extend_from_slice(data);
    v
}

/// UTF-16LE without terminator (the encoding of Patcher node strings).
pub(crate) fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// UTF-16LE with a NUL terminator (the encoding of FLP text events).
pub(crate) fn utf16_name(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

pub(crate) const CID1_S1: [u8; 20] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
];
pub(crate) const CID2_S1: [u8; 25] = [
    0x00, 0xA0, 0x00, 0x00, 0x00, 0x19, 0x00, 0x00, 0x00, 0x8D, 0x7D, 0x20, 0xA4, 0x00, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
pub(crate) const C30: [u8; 16] = [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
pub(crate) const C32: [u8; 12] = [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
pub(crate) const C50: [u8; 16] = [0x08, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
pub(crate) const S1_UID: [u8; 16] = [
    0x58, 0x54, 0x53, 0x56, 0x73, 0x66, 0x73, 0x58, 0x65, 0x72, 0x75, 0x6D, 0x00, 0x00, 0x00, 0x00,
];
pub(crate) const S2_UID: [u8; 16] = [
    0x58, 0x45, 0x53, 0x56, 0x73, 0x66, 0x73, 0x50, 0x65, 0x72, 0x75, 0x6D, 0x20, 0x32, 0x00, 0x00,
];

/// Real-shaped Serum inner cid 3: zlib preset state (preset name
/// `TestPreset` at 0x4972) + a second wavetable stream + u32 LE trailer.
pub(crate) fn s1_cid3() -> Vec<u8> {
    s1_cid3_named("TestPreset")
}

/// Same as [`s1_cid3`] with a caller-chosen preset name.
pub(crate) fn s1_cid3_named(preset: &str) -> Vec<u8> {
    use crate::serum;
    let mut s0 = vec![0u8; serum::SERUM1_STATE_SIZE];
    let name = preset.as_bytes();
    s0[serum::OFF_PRESET_NAME..serum::OFF_PRESET_NAME + name.len()].copy_from_slice(name);
    s0[serum::OFF_VERSION_F32..serum::OFF_VERSION_F32 + 4]
        .copy_from_slice(&0.1631f32.to_le_bytes());
    let z0 = zlib_stream(&s0);
    let z1 = zlib_stream(&[0u8; 8192]);
    let mut v = z0.clone();
    v.extend_from_slice(&z1);
    v.extend_from_slice(&(z0.len() as u32).to_le_bytes());
    v
}

/// A realistic Serum event-213 payload (docs/flp-serum2-conversion.md §2/§3).
pub(crate) fn serum1_payload(name: &str, filename: &str) -> Vec<u8> {
    serum1_payload_with(name, filename, s1_cid3())
}

/// Same, with a caller-supplied inner cid 3 (to fake broken chunks).
pub(crate) fn serum1_payload_with(name: &str, filename: &str, cid3: Vec<u8>) -> Vec<u8> {
    let mut cid4 = Vec::new();
    cid4.extend_from_slice(&3u32.to_le_bytes());
    cid4.extend_from_slice(&0u32.to_le_bytes());
    cid4.extend_from_slice(&1u32.to_le_bytes());
    cid4.extend_from_slice(&2u32.to_le_bytes());

    let mut wrapper = Vec::new();
    wrapper.extend_from_slice(&1u32.to_le_bytes());
    let mut inner1 = vec![0u8; 64];
    inner1[0..4].copy_from_slice(&1u32.to_le_bytes());
    wrapper.extend_from_slice(&rec(1, &inner1));
    wrapper.extend_from_slice(&rec(3, &cid3));
    wrapper.extend_from_slice(&rec(4, &cid4));

    let mut p = Vec::new();
    p.extend_from_slice(&12u32.to_le_bytes());
    p.extend_from_slice(&rec(1, &CID1_S1));
    p.extend_from_slice(&rec(2, &CID2_S1));
    p.extend_from_slice(&rec(30, &C30));
    p.extend_from_slice(&rec(32, &C32));
    p.extend_from_slice(&rec(50, &C50));
    p.extend_from_slice(&rec(52, &S1_UID));
    p.extend_from_slice(&rec(54, name.as_bytes()));
    p.extend_from_slice(&rec(55, filename.as_bytes()));
    p.extend_from_slice(&rec(56, b"Xfer Records"));
    p.extend_from_slice(&rec(53, &wrapper));
    p
}

/// A Serum2-shaped event-213 payload: wrapper `[1][cid1 64 B][cid3 XferJson
/// processor][cid2 XferJson controller][cid4]`.
pub(crate) fn serum2_payload(name: &str, filename: &str) -> Vec<u8> {
    let mut wrapper = Vec::new();
    wrapper.extend_from_slice(&1u32.to_le_bytes());
    let mut inner1 = vec![0u8; 64];
    inner1[0..4].copy_from_slice(&1u32.to_le_bytes());
    wrapper.extend_from_slice(&rec(1, &inner1));
    wrapper.extend_from_slice(&rec(3, b"XferJson\0processor"));
    wrapper.extend_from_slice(&rec(2, b"XferJson\0controller"));
    wrapper.extend_from_slice(&rec(4, &[1, 2, 3, 4]));

    let mut c1 = CID1_S1.to_vec();
    c1[8..12].copy_from_slice(&1u32.to_le_bytes());
    let mut p = Vec::new();
    p.extend_from_slice(&12u32.to_le_bytes());
    p.extend_from_slice(&rec(1, &c1));
    p.extend_from_slice(&rec(2, &CID2_S1));
    p.extend_from_slice(&rec(30, &C30));
    p.extend_from_slice(&rec(32, &C32));
    p.extend_from_slice(&rec(50, &C50));
    p.extend_from_slice(&rec(52, &S2_UID));
    p.extend_from_slice(&rec(54, name.as_bytes()));
    p.extend_from_slice(&rec(55, filename.as_bytes()));
    p.extend_from_slice(&rec(56, b"Xfer Records"));
    p.extend_from_slice(&rec(53, &wrapper));
    p
}

/// A minimal unrelated plugin payload (e.g. Vital, kHs Limiter): name,
/// filename and an opaque state blob.
pub(crate) fn other_plugin_payload(name: &str, state: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&12u32.to_le_bytes());
    p.extend_from_slice(&rec(54, name.as_bytes()));
    p.extend_from_slice(&rec(55, format!("/VST3/{name}.vst3").as_bytes()));
    p.extend_from_slice(&rec(53, state));
    p
}

// ---------------------------------------------------------------------------
// FL Studio Patcher event-213 payload builders (docs/flp-patcher.md)
// ---------------------------------------------------------------------------
//
// The constants below are records COPIED from the real Patcher event of a
// FL Studio 26.1.6.5639 project (hex of the record data). The 1004 record is
// 324 B in the sample; only its first 72 B are non-zero, so it is stored
// without its zero tail and padded back in `patcher_payload`.

/// Patcher record 1000 (64 B).
const P_1000: &str = "fe030000000000000100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
/// Patcher record 1002 (44 B).
const P_1002: &str =
    "00000000000000000000000000000000000f000000b76d2b42b76d2f43dbb60944b76d2f4300000000000000";
/// First 72 B of Patcher record 1004 (324 B, rest zero).
const P_1004_HEAD: &str = "01000000af07723f1815fe3f00000000000000009701000085010000b00000004000000002000000030000000100000004000000000000000000000000000000";
/// Control Surface node (inner records of the outer cid-0 container, 394 B).
const P_CONTROL_SURFACE: &str = "88130000280000000000000002000000ea03000000000000090000000000000001000000000000000000000000000000000000008c1300001e0000000000000043006f006e00740072006f006c0020005300750072006600610063006500901300001e0000000000000043006f006e00740072006f006c0020005300750072006600610063006500921300000e0000000000000053007500720066006100630065008d130000740000000000000001000000d0070000400000000000000001000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d207000008000000000000001c020000c6010000d30700000400000000000000020000008f13000028000000000000000000803fffffffff0000000000000000ffffff7f3c00000000000000000000000000000000000000911300001c000000000000000100000000000000eb03000000000000020000000000000000000000891300000000000000000000";
/// `Notes` pseudo node (inner records, 74 B).
const P_NOTES: &str = "780500001c00000000000000e903000000000000010000000700000000ffffffff000000000000007a0500000a000000000000004e006f00740065007300790500000000000000000000";
/// `Audio output` pseudo node (inner records, 88 B).
const P_AUDIO_OUT: &str = "780500001c00000000000000e80300000000000000000000060000000200000000000000000000007a050000180000000000000041007500640069006f0020006f0075007400700075007400790500000000000000000000";
/// Wire record (inner records, 56 B; inner 1300 + empty 1301).
const P_WIRE: &str = "140500002000000000000000f303000000000000e9030000000000000000803f020000000100000000000000150500000000000000000000";
/// Top-level record 1500 (40 B).
const P_1500: &str =
    "0000803f00000000000000001a040000360300000000000000000000000000000000000000000000";
/// Node record 5007 (40 B, identical in every node of the sample).
const P_5007: &str =
    "0000803fffffffff0000000000000000ffffff7f3c00000000000000000000000000000000000000";
/// Per-plugin-node record (inner 1600, 24 B) of the first node.
const P_1600: &str = "ea03000000000000254992426edb06420000000000000000";

/// One plugin node to put into a synthetic Patcher.
pub(crate) struct NodeSpec {
    pub plugin_name: String,
    pub label: String,
    /// Hosted plugin state (the node's 5005 record data).
    pub state: Vec<u8>,
}

impl NodeSpec {
    pub(crate) fn new(plugin_name: &str, label: &str, state: Vec<u8>) -> NodeSpec {
        NodeSpec {
            plugin_name: plugin_name.to_string(),
            label: label.to_string(),
            state,
        }
    }
}

/// A plugin node container record (outer cid 0) shaped like the sample's
/// Serum 2 / Vital nodes: 5000 header, 5004 `Fruity Wrapper`, 5008 plugin
/// name, 5010 label, 5005 state, 5007, eight 5009 ports, 5001 end marker.
pub(crate) fn patcher_node(id: u32, spec: &NodeSpec) -> Vec<u8> {
    let mut header = Vec::new();
    for v in [3u32, id, 0, 8, 2, 6, 1199, 382, 0, 0] {
        header.extend_from_slice(&v.to_le_bytes());
    }
    let mut inner = Vec::new();
    inner.extend_from_slice(&rec(5000, &header));
    inner.extend_from_slice(&rec(5004, &utf16("Fruity Wrapper")));
    inner.extend_from_slice(&rec(5008, &utf16(&spec.plugin_name)));
    inner.extend_from_slice(&rec(5010, &utf16(&spec.label)));
    inner.extend_from_slice(&rec(5005, &spec.state));
    inner.extend_from_slice(&rec(5007, &hex(P_5007)));
    // (a, b, id offset, c) of the eight ports seen in every plugin node.
    for (a, b, off, c) in [
        (0u32, 0u32, 1u32, 3u32),
        (1, 0, 7, 3),
        (0, 0, 2, 2),
        (0, 1, 3, 0),
        (0, 2, 4, 0),
        (0, 3, 5, 0),
        (0, 4, 6, 0),
        (1, 0, 8, 2),
    ] {
        let mut port = Vec::new();
        for v in [a, b, id + off, 0, c, 0, 0] {
            port.extend_from_slice(&v.to_le_bytes());
        }
        inner.extend_from_slice(&rec(5009, &port));
    }
    inner.extend_from_slice(&rec(5001, &[]));
    rec(0, &inner)
}

/// A Patcher event-213 payload around the given plugin nodes, in the record
/// order observed in the sample: 1000, 1002, 1004, Control Surface, plugin
/// nodes, Notes, Audio output, one wire per node, 1500, two empty cid-0
/// records, one 1600 record per node, 1001.
pub(crate) fn patcher_payload(nodes: &[NodeSpec]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&4u32.to_le_bytes());
    p.extend_from_slice(&rec(1000, &hex(P_1000)));
    p.extend_from_slice(&rec(1002, &hex(P_1002)));
    let mut r1004 = hex(P_1004_HEAD);
    r1004.resize(324, 0);
    p.extend_from_slice(&rec(1004, &r1004));
    p.extend_from_slice(&rec(0, &hex(P_CONTROL_SURFACE)));
    for (i, n) in nodes.iter().enumerate() {
        p.extend_from_slice(&patcher_node(0x3ec + 9 * i as u32, n));
    }
    p.extend_from_slice(&rec(0, &hex(P_NOTES)));
    p.extend_from_slice(&rec(0, &hex(P_AUDIO_OUT)));
    for _ in nodes {
        p.extend_from_slice(&rec(0, &hex(P_WIRE)));
    }
    p.extend_from_slice(&rec(1500, &hex(P_1500)));
    p.extend_from_slice(&rec(0, &[]));
    p.extend_from_slice(&rec(0, &[]));
    for i in 0..nodes.len() {
        let mut r = hex(P_1600);
        r[0..4].copy_from_slice(&(0x3ec + 9 * i as u32).to_le_bytes());
        p.extend_from_slice(&rec(0, &rec(1600, &r)));
    }
    p.extend_from_slice(&rec(1001, &[]));
    p
}

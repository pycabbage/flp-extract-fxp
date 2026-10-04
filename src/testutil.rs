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

/// Decode a hex string (whitespace ignored); panics on malformed input.
pub(crate) fn hex(s: &str) -> Vec<u8> {
    let digits: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    assert!(digits.len().is_multiple_of(2), "odd hex length");
    digits
        .chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
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

/// UTF-16LE without a terminator (the Patcher node string fields).
pub(crate) fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

// ---------------------------------------------------------------------------
// Plugin event-213 payload builders (real-shaped, see
// docs/flp-serum2-conversion.md §2/§3)
// ---------------------------------------------------------------------------

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

/// Real-shaped Serum inner cid 3: zlib preset state (preset name at
/// 0x4972) + a second wavetable stream + u32 LE trailer (doc §3.3).
pub(crate) fn s1_cid3() -> Vec<u8> {
    use crate::serum;
    let mut s0 = vec![0u8; serum::SERUM1_STATE_SIZE];
    s0[serum::OFF_PRESET_NAME..serum::OFF_PRESET_NAME + 11].copy_from_slice(b"TestPreset\0");
    s0[serum::OFF_VERSION_F32..serum::OFF_VERSION_F32 + 4]
        .copy_from_slice(&0.1631f32.to_le_bytes());
    let z0 = zlib_stream(&s0);
    let z1 = zlib_stream(&[0u8; 8192]);
    let mut v = z0.clone();
    v.extend_from_slice(&z1);
    v.extend_from_slice(&(z0.len() as u32).to_le_bytes());
    v
}

/// FL VST3 wrapper (cid 53 payload): `[prologue 1][records...]`.
fn vst3_wrapper(inner: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut w = 1u32.to_le_bytes().to_vec();
    for (cid, data) in inner {
        w.extend_from_slice(&rec(*cid, data));
    }
    w
}

fn plugin_payload(name: &str, filename: &str, uid: &[u8], wrapper: &[u8]) -> Vec<u8> {
    let mut p = 12u32.to_le_bytes().to_vec();
    p.extend_from_slice(&rec(1, &CID1_S1));
    p.extend_from_slice(&rec(2, &CID2_S1));
    p.extend_from_slice(&rec(30, &C30));
    p.extend_from_slice(&rec(32, &C32));
    p.extend_from_slice(&rec(50, &C50));
    p.extend_from_slice(&rec(52, uid));
    p.extend_from_slice(&rec(54, name.as_bytes()));
    p.extend_from_slice(&rec(55, filename.as_bytes()));
    p.extend_from_slice(&rec(56, b"Xfer Records"));
    p.extend_from_slice(&rec(53, wrapper));
    p
}

/// A realistic Serum event-213 payload (doc §2.1 / §3).
pub(crate) fn serum1_payload(name: &str, filename: &str) -> Vec<u8> {
    serum1_payload_with(name, filename, s1_cid3())
}

/// Same, with a caller-supplied inner cid 3 (to fake broken chunks).
pub(crate) fn serum1_payload_with(name: &str, filename: &str, cid3: Vec<u8>) -> Vec<u8> {
    let mut cid4 = Vec::new();
    for v in [3u32, 0, 1, 2] {
        cid4.extend_from_slice(&v.to_le_bytes());
    }
    let mut inner1 = vec![0u8; 64];
    inner1[0..4].copy_from_slice(&1u32.to_le_bytes());
    let wrapper = vst3_wrapper(&[(1, inner1), (3, cid3), (4, cid4)]);
    plugin_payload(name, filename, &S1_UID, &wrapper)
}

/// A Serum2-shaped event-213 payload: inner cid 3 / cid 2 carry the
/// `XferJson` magic (content is not a valid container; recognisers only
/// check the magic).
pub(crate) fn serum2_payload(name: &str, filename: &str) -> Vec<u8> {
    let mut inner1 = vec![0u8; 64];
    inner1[0..4].copy_from_slice(&1u32.to_le_bytes());
    let wrapper = vst3_wrapper(&[
        (1, inner1),
        (3, b"XferJson\0proc".to_vec()),
        (2, b"XferJson\0ctrl".to_vec()),
        (4, vec![0x11, 0x22, 0x33, 0x44]),
    ]);
    plugin_payload(name, filename, &S2_UID, &wrapper)
}

/// A non-Serum VST3 plugin payload (e.g. Vital): wrapper without XferJson.
pub(crate) fn other_plugin_payload(name: &str, filename: &str) -> Vec<u8> {
    let mut inner1 = vec![0u8; 64];
    inner1[0..4].copy_from_slice(&1u32.to_le_bytes());
    let wrapper = vst3_wrapper(&[(1, inner1), (3, b"VitalState".to_vec())]);
    plugin_payload(name, filename, b"OTHERUID16bytes!", &wrapper)
}

// ---------------------------------------------------------------------------
// FL Studio Patcher event-213 payload builders. Small records are copied
// from a real FL Studio 26.1.6.5639 project; what 1002 / 1004 / 1500 / 1600 /
// 5007 mean is NOT known (docs/flp-patcher.md), 1004 is abbreviated.
// ---------------------------------------------------------------------------

/// Patcher header record 1000 (64 B).
const PATCHER_1000: &str = "fe030000000000000100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
/// 1002 (44 B).
const PATCHER_1002: &str =
    "00000000000000000000000000000000000f000000b76d2b42b76d2f43dbb60944b76d2f4300000000000000";
/// First 48 bytes of the 324-byte 1004 record (abbreviated).
const PATCHER_1004: &str = "01000000af07723f1815fe3f00000000000000009701000085010000b000000040000000020000000300000001000000";
/// Node header 5000 template (40 B) of a plugin node; u32@4 is the node id.
const NODE_5000_PLUGIN: &str =
    "03000000ec03000000000000080000000200000006000000af0400007e0100000000000000000000";
/// Node header 5000 of the Control Surface node (kind 2, id 0x3ea).
const NODE_5000_CONTROL: &str =
    "02000000ea0300000000000009000000000000000100000000000000000000000000000000000000";
/// 5007 (40 B), identical in every node.
const NODE_5007: &str =
    "0000803fffffffff0000000000000000ffffff7f3c00000000000000000000000000000000000000";
/// Control Surface 5005: a 116-byte version-1 blob (not a plugin payload).
const CONTROL_SURFACE_5005: &str = "01000000d0070000400000000000000001000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d207000008000000000000001c020000c6010000d3070000040000000000000002000000";
/// 5009 port records of a plugin node: (a, b, id offset, c).
const PLUGIN_PORTS: [(u32, u32, u64, u32); 8] = [
    (0, 0, 1, 3),
    (1, 0, 7, 3),
    (0, 0, 2, 2),
    (0, 1, 3, 0),
    (0, 2, 4, 0),
    (0, 3, 5, 0),
    (0, 4, 6, 0),
    (1, 0, 8, 2),
];

/// Node id of the Control Surface node.
pub(crate) const CONTROL_NODE_ID: u32 = 0x3ea;

/// A `cid 0` Patcher node record carrying the real record order
/// 5000, 5004, 5008, 5010, 5005, 5007, 5009 x8 (ports), 5001.
pub(crate) fn patcher_plugin_node(
    node_id: u32,
    display: &str,
    label: &str,
    state: &[u8],
) -> Vec<u8> {
    let mut h = hex(NODE_5000_PLUGIN);
    h[4..8].copy_from_slice(&node_id.to_le_bytes());
    let mut n = rec(5000, &h);
    n.extend_from_slice(&rec(5004, &utf16le("Fruity Wrapper")));
    n.extend_from_slice(&rec(5008, &utf16le(display)));
    n.extend_from_slice(&rec(5010, &utf16le(label)));
    n.extend_from_slice(&rec(5005, state));
    n.extend_from_slice(&rec(5007, &hex(NODE_5007)));
    for (a, b, off, c) in PLUGIN_PORTS {
        let mut p = Vec::new();
        p.extend_from_slice(&a.to_le_bytes());
        p.extend_from_slice(&b.to_le_bytes());
        p.extend_from_slice(&(node_id as u64 + off).to_le_bytes());
        p.extend_from_slice(&c.to_le_bytes());
        p.extend_from_slice(&[0u8; 8]);
        n.extend_from_slice(&rec(5009, &p));
    }
    n.extend_from_slice(&rec(5001, &[]));
    rec(0, &n)
}

/// The Control Surface node (kind 2; its 5005 is a version-1 blob).
pub(crate) fn patcher_control_surface_node() -> Vec<u8> {
    let mut n = rec(5000, &hex(NODE_5000_CONTROL));
    n.extend_from_slice(&rec(5004, &utf16le("Control Surface")));
    n.extend_from_slice(&rec(5008, &utf16le("Control Surface")));
    n.extend_from_slice(&rec(5010, &utf16le("Surface")));
    n.extend_from_slice(&rec(5005, &hex(CONTROL_SURFACE_5005)));
    n.extend_from_slice(&rec(5007, &hex(NODE_5007)));
    let mut port = Vec::new();
    port.extend_from_slice(&1u32.to_le_bytes());
    port.extend_from_slice(&[0u8; 4]);
    port.extend_from_slice(&(CONTROL_NODE_ID as u64 + 1).to_le_bytes());
    port.extend_from_slice(&2u32.to_le_bytes());
    port.extend_from_slice(&[0u8; 4]);
    n.extend_from_slice(&rec(5009, &port));
    n.extend_from_slice(&rec(5001, &[]));
    rec(0, &n)
}

/// A `cid 0` node holding the given inner `(cid, data)` records.
pub(crate) fn patcher_container(inner: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut n = Vec::new();
    for (cid, data) in inner {
        n.extend_from_slice(&rec(*cid, data));
    }
    rec(0, &n)
}

/// A whole Patcher event-213 payload around `plugin_nodes` (each from
/// [`patcher_plugin_node`]) with the real top-level record order: 1000,
/// 1002, 1004, Control Surface, plugin nodes, Notes, Audio output, wire
/// records, 1500, two empty containers, one 1600 container per node (Control
/// Surface included), 1001. `ids` are the plugin nodes' ids (for the wire and
/// 1600 records).
pub(crate) fn patcher_event(plugin_nodes: &[Vec<u8>], ids: &[u32]) -> Vec<u8> {
    assert_eq!(plugin_nodes.len(), ids.len());
    let mut p = 4u32.to_le_bytes().to_vec();
    p.extend_from_slice(&rec(1000, &hex(PATCHER_1000)));
    p.extend_from_slice(&rec(1002, &hex(PATCHER_1002)));
    p.extend_from_slice(&rec(1004, &hex(PATCHER_1004)));
    p.extend_from_slice(&patcher_control_surface_node());
    for n in plugin_nodes {
        p.extend_from_slice(n);
    }
    p.extend_from_slice(&patcher_container(&[
        (
            1400,
            hex("e903000000000000010000000700000000ffffffff00000000000000"),
        ),
        (1402, utf16le("Notes")),
        (1401, vec![]),
    ]));
    p.extend_from_slice(&patcher_container(&[
        (
            1400,
            hex("e8030000000000000000000006000000020000000000000000000000"),
        ),
        (1402, utf16le("Audio output")),
        (1401, vec![]),
    ]));
    for id in ids {
        let mut w = Vec::new();
        w.extend_from_slice(&(*id as u64 + 7).to_le_bytes());
        w.extend_from_slice(&(CONTROL_NODE_ID as u64 - 1).to_le_bytes());
        w.extend_from_slice(&1.0f32.to_le_bytes());
        w.extend_from_slice(&2u32.to_le_bytes());
        w.extend_from_slice(&1u32.to_le_bytes());
        w.extend_from_slice(&[0u8; 4]);
        p.extend_from_slice(&patcher_container(&[(1300, w), (1301, vec![])]));
    }
    p.extend_from_slice(&rec(
        1500,
        &hex("0000803f00000000000000001a040000360300000000000000000000000000000000000000000000"),
    ));
    p.extend_from_slice(&rec(0, &[]));
    p.extend_from_slice(&rec(0, &[]));
    // One 1600 container per node, Control Surface first.
    for id in std::iter::once(&CONTROL_NODE_ID).chain(ids) {
        let mut t = Vec::new();
        t.extend_from_slice(&(*id as u64).to_le_bytes());
        t.extend_from_slice(&hex("254992426edb0642"));
        t.extend_from_slice(&[0u8; 8]);
        p.extend_from_slice(&patcher_container(&[(1600, t)]));
    }
    p.extend_from_slice(&rec(1001, &[]));
    p
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

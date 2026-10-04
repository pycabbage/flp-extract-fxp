//! FL Studio **Patcher** support: plugins hosted inside a Patcher's nodes.
//!
//! A Patcher instance is a normal `PluginParams` (event 213) event whose
//! payload is NOT a plugin chunk list but the Patcher's own graph:
//! `u32 LE version = 4` followed by `[u32 cid][u64 size][data]` records
//! (the same framing as plugin chunks; first record cid 1000). Every hosted
//! plugin lives in an outer `cid = 0` container record whose data is again
//! a record sequence; the inner 5005 record carries the hosted plugin's saved
//! state, byte-for-byte the layout of a plain event-213 payload (see
//! `docs/flp-patcher.md`). `flp::parse_plugin_params` reads a Patcher as a
//! bare legacy blob, which is why Patcher contents were invisible to every
//! scanner before this module existed.
//!
//! This module only does structural record surgery:
//! [`plugin_sites`] enumerates the plugin payloads of an event (a plain
//! plugin event is its own single site), [`site_payload`] resolves a site
//! again later (stale-plan checks) and [`rewrite`] replaces chosen nodes'
//! state while copying every other byte verbatim.
//!
//! Only observed against FL Studio 26.1.6.5639 projects; nested Patchers
//! (a Patcher hosted inside a Patcher) are followed recursively but have
//! never been seen in a real file.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::flp;

/// The only Patcher payload version ever observed.
const PATCHER_VERSION: u32 = 4;
/// First record of every Patcher payload.
const CID_PATCHER_HEADER: u32 = 1000;
/// Outer record id of a graph node container.
const CID_CONTAINER: u32 = 0;
/// Node record: the hosted plugin's saved state.
const CID_NODE_STATE: u32 = 5005;
/// Node record: plugin display name (UTF-16LE, no terminator).
const CID_NODE_PLUGIN_NAME: u32 = 5008;
/// Node record: user-visible node label (UTF-16LE, no terminator).
const CID_NODE_LABEL: u32 = 5010;
/// Oldest `PluginParams` version that uses the chunked plugin layout.
const MIN_CHUNKED_VERSION: u32 = 5;
/// Deepest Patcher-in-Patcher nesting that is followed (the top-level
/// Patcher is level 1). Deeper content is ignored.
pub const MAX_NESTING: usize = 4;
/// Separator between channel / node labels in reported locations.
const LOCATION_SEP: &str = " > ";

/// One plugin state found in an event-213 payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSite<'a> {
    /// Empty when the plugin owns its event; otherwise the record index of
    /// the hosting node inside each (nested) Patcher payload.
    pub path: Vec<usize>,
    /// Node label (5010); nested labels are joined with ` > `. Empty for a
    /// plugin that owns its event.
    pub label: String,
    /// Node plugin display name (5008); empty for a plain event.
    pub plugin_name: String,
    /// The plugin's event-213-style payload (the whole event payload for a
    /// plain plugin, the node's 5005 data inside a Patcher).
    pub payload: &'a [u8],
}

impl PluginSite<'_> {
    /// Reported location: the channel / FX slot name followed by the node
    /// label, e.g. `Intro Chord Patch > Serum 2 #4`.
    pub fn location(&self, base: &str) -> String {
        match (base.is_empty(), self.label.is_empty()) {
            (_, true) => base.to_string(),
            (true, false) => self.label.clone(),
            (false, false) => format!("{base}{LOCATION_SEP}{}", self.label),
        }
    }
}

/// Replacement state for one hosted plugin node, see [`rewrite`].
#[derive(Debug, Clone)]
pub struct NodeEdit {
    /// Site path as reported by [`plugin_sites`] (never empty).
    pub path: Vec<usize>,
    /// New 5005 data (an event-213-style plugin payload).
    pub payload: Vec<u8>,
    /// New 5008 plugin display name; `None` keeps the node's name.
    pub plugin_name: Option<String>,
}

/// A `[u32 cid][u64 size][data]` record; `data` indexes the walked buffer
/// (the 12-byte header sits right before it).
struct Rec {
    cid: u32,
    data: Range<usize>,
}

/// Walk the record sequence of `buf[start..]`; `None` unless the records
/// consume it exactly.
fn walk(buf: &[u8], start: usize) -> Option<Vec<Rec>> {
    let body = buf.get(start..)?;
    let mut it = flp::Records::new(body);
    let mut out = Vec::new();
    while let Some((cid, data)) = it.next() {
        let end = start + it.pos();
        out.push(Rec {
            cid,
            data: end - data.len()..end,
        });
    }
    (!it.overran() && it.pos() == body.len()).then_some(out)
}

fn push_rec(out: &mut Vec<u8>, cid: u32, data: &[u8]) {
    out.extend_from_slice(&cid.to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(data);
}

fn le_u32(b: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(0..4)?.try_into().ok()?))
}

fn utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_string()
}

fn encode_utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Structural Patcher detection: version 4, first record cid 1000 and a
/// record walk that consumes the payload exactly.
pub fn is_patcher(payload: &[u8]) -> bool {
    payload.len() >= 4 + 12
        && le_u32(payload) == Some(PATCHER_VERSION)
        && le_u32(&payload[4..]) == Some(CID_PATCHER_HEADER)
        && walk(payload, 4).is_some()
}

/// True for a payload in the chunked plugin layout (`version >= 5`). The
/// Control Surface node's 5005 (a 116-byte version-1 blob) is not one.
fn is_plugin_payload(state: &[u8]) -> bool {
    le_u32(state).is_some_and(|v| v >= MIN_CHUNKED_VERSION)
}

/// Fields of a node container record.
struct Node<'a> {
    state: &'a [u8],
    plugin_name: String,
    label: String,
}

/// Read a node (outer cid 0 container with a 5005 record) out of `rec`.
fn node_of<'a>(payload: &'a [u8], rec: &Rec) -> Option<Node<'a>> {
    if rec.cid != CID_CONTAINER {
        return None;
    }
    let data = &payload[rec.data.clone()];
    let inner = walk(data, 0)?;
    let field = |cid: u32| {
        inner
            .iter()
            .find(|r| r.cid == cid)
            .map(|r| &data[r.data.clone()])
    };
    Some(Node {
        state: field(CID_NODE_STATE)?,
        plugin_name: field(CID_NODE_PLUGIN_NAME).map(utf16le).unwrap_or_default(),
        label: field(CID_NODE_LABEL).map(utf16le).unwrap_or_default(),
    })
}

fn collect<'a>(
    payload: &'a [u8],
    path: &mut Vec<usize>,
    chain: &str,
    level: usize,
    out: &mut Vec<PluginSite<'a>>,
) {
    let Some(recs) = walk(payload, 4) else {
        return;
    };
    for (i, rec) in recs.iter().enumerate() {
        let Some(node) = node_of(payload, rec) else {
            continue;
        };
        let label = if chain.is_empty() {
            node.label.clone()
        } else {
            format!("{chain}{LOCATION_SEP}{}", node.label)
        };
        path.push(i);
        if is_patcher(node.state) {
            if level < MAX_NESTING {
                collect(node.state, path, &label, level + 1, out);
            }
        } else if is_plugin_payload(node.state) {
            out.push(PluginSite {
                path: path.clone(),
                label,
                plugin_name: node.plugin_name,
                payload: node.state,
            });
        }
        path.pop();
    }
}

/// Every plugin state hosted by an event-213 payload.
///
/// A plain plugin event yields itself (empty path, empty label); a Patcher
/// yields one site per hosted plugin node, in record (= file) order. Nodes
/// without a chunked plugin state (the Control Surface) yield nothing.
pub fn plugin_sites(event_payload: &[u8]) -> Vec<PluginSite<'_>> {
    if !is_patcher(event_payload) {
        return vec![PluginSite {
            path: Vec::new(),
            label: String::new(),
            plugin_name: String::new(),
            payload: event_payload,
        }];
    }
    let mut out = Vec::new();
    collect(event_payload, &mut Vec::new(), "", 1, &mut out);
    out
}

/// Resolve a site path (see [`PluginSite::path`]) back to its payload; an
/// empty path is the event payload itself. Used to check that a plan still
/// describes the buffer.
pub fn site_payload<'a>(event_payload: &'a [u8], path: &[usize]) -> Result<&'a [u8], String> {
    if path.len() > MAX_NESTING {
        return Err(format!("site path is deeper than {MAX_NESTING} levels"));
    }
    let mut cur = event_payload;
    for (level, &idx) in path.iter().enumerate() {
        if !is_patcher(cur) {
            return Err(format!("no Patcher payload at nesting level {}", level + 1));
        }
        let recs = walk(cur, 4).ok_or("malformed Patcher payload")?;
        let rec = recs
            .get(idx)
            .ok_or_else(|| format!("Patcher has no record #{idx}"))?;
        let node =
            node_of(cur, rec).ok_or_else(|| format!("Patcher record #{idx} is not a node"))?;
        cur = node.state;
    }
    Ok(cur)
}

type PendingEdit<'a> = (&'a [usize], &'a NodeEdit);

/// Replace the state (and optionally the plugin name) of hosted plugin
/// nodes. Every other byte of the payload is copied verbatim, record sizes
/// are recomputed, so an empty edit list — or edits that restate the
/// existing bytes — reproduce the input byte for byte.
///
/// Errors: `event_payload` is not a Patcher, an edit has an empty path,
/// addresses something that is not a hosted plugin node (bad index,
/// Control Surface, ...), or two edits conflict (same node twice, or a
/// node and one of its nested nodes).
pub fn rewrite(event_payload: &[u8], edits: &[NodeEdit]) -> Result<Vec<u8>, String> {
    if !is_patcher(event_payload) {
        return Err("not a Patcher payload".into());
    }
    let pending: Vec<PendingEdit> = edits.iter().map(|e| (e.path.as_slice(), e)).collect();
    rewrite_level(event_payload, &pending, 1)
}

fn rewrite_level(payload: &[u8], edits: &[PendingEdit], level: usize) -> Result<Vec<u8>, String> {
    let recs = walk(payload, 4).ok_or("malformed Patcher payload")?;
    let mut by_record: BTreeMap<usize, Vec<PendingEdit>> = BTreeMap::new();
    for &(path, edit) in edits {
        let Some((&first, rest)) = path.split_first() else {
            return Err("node edit has an empty path".into());
        };
        if first >= recs.len() {
            return Err(format!("Patcher has no record #{first}"));
        }
        by_record.entry(first).or_default().push((rest, edit));
    }

    let mut out = Vec::with_capacity(payload.len());
    out.extend_from_slice(&payload[..4]);
    for (i, rec) in recs.iter().enumerate() {
        match by_record.get(&i) {
            None => out.extend_from_slice(&payload[rec.data.start - 12..rec.data.end]),
            Some(group) => {
                let node = rewrite_node(&payload[rec.data.clone()], rec.cid, group, level)
                    .map_err(|e| format!("record #{i}: {e}"))?;
                push_rec(&mut out, CID_CONTAINER, &node);
            }
        }
    }
    Ok(out)
}

/// Rebuild one node container's inner records with its edits applied.
fn rewrite_node(
    data: &[u8],
    outer_cid: u32,
    group: &[PendingEdit],
    level: usize,
) -> Result<Vec<u8>, String> {
    let not_plugin = || String::from("not a hosted plugin node");
    if outer_cid != CID_CONTAINER {
        return Err(not_plugin());
    }
    let inner = walk(data, 0).ok_or_else(not_plugin)?;
    let state_rec = inner
        .iter()
        .position(|r| r.cid == CID_NODE_STATE)
        .ok_or_else(not_plugin)?;
    let old_state = &data[inner[state_rec].data.clone()];

    let direct: Vec<&NodeEdit> = group
        .iter()
        .filter(|(rest, _)| rest.is_empty())
        .map(|(_, e)| *e)
        .collect();
    let (new_state, new_name): (Vec<u8>, Option<&str>) = match (direct.as_slice(), group.len()) {
        ([e], 1) => {
            if !is_plugin_payload(old_state) && !is_patcher(old_state) {
                return Err(not_plugin());
            }
            (e.payload.clone(), e.plugin_name.as_deref())
        }
        ([], _) => {
            if level >= MAX_NESTING || !is_patcher(old_state) {
                return Err("edit path descends into a node that is not a Patcher".into());
            }
            let nested: Vec<PendingEdit> = group.to_vec();
            (rewrite_level(old_state, &nested, level + 1)?, None)
        }
        _ => return Err("conflicting edits for the same node".into()),
    };

    let mut out = Vec::with_capacity(data.len() + new_state.len().saturating_sub(old_state.len()));
    let mut name_replaced = new_name.is_none();
    for (i, r) in inner.iter().enumerate() {
        if i == state_rec {
            push_rec(&mut out, r.cid, &new_state);
        } else if let (Some(name), CID_NODE_PLUGIN_NAME, false) = (new_name, r.cid, name_replaced) {
            push_rec(&mut out, r.cid, &encode_utf16le(name));
            name_replaced = true;
        } else {
            out.extend_from_slice(&data[r.data.start - 12..r.data.end]);
        }
    }
    if !name_replaced {
        return Err("node has no plugin-name record (5008) to rename".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{
        NodeSpec, hex, other_plugin_payload, patcher_node, patcher_payload, rec, serum1_payload,
        utf16,
    };

    const SERUM: &str = "/Library/Audio/Plug-Ins/VST3/Serum.vst3";

    /// Top-level record data slices of a payload.
    fn records(payload: &[u8]) -> Vec<(u32, Vec<u8>)> {
        walk(payload, 4)
            .expect("payload walks exactly")
            .into_iter()
            .map(|r| (r.cid, payload[r.data].to_vec()))
            .collect()
    }

    fn two_node_patcher() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let serum = serum1_payload("Serum", SERUM);
        let vital = other_plugin_payload("Vital", &[7; 40]);
        let p = patcher_payload(&[
            NodeSpec::new("Serum", "Lead", serum.clone()),
            NodeSpec::new("Vital", "Vital", vital.clone()),
        ]);
        (p, serum, vital)
    }

    /// Wrap `inner` as the only node of another Patcher.
    fn wrap_patcher(label: &str, inner: &[u8]) -> Vec<u8> {
        patcher_payload(&[NodeSpec::new("Patcher", label, inner.to_vec())])
    }

    #[test]
    fn detects_patchers_structurally() {
        let (p, serum, _) = two_node_patcher();
        assert!(is_patcher(&p));
        assert!(!is_patcher(&serum));
        assert!(!is_patcher(&[]));
        assert!(!is_patcher(&4u32.to_le_bytes()));
        // Walk must consume the payload exactly.
        assert!(!is_patcher(&p[..p.len() - 1]));
        let mut trailing = p.clone();
        trailing.push(0);
        assert!(!is_patcher(&trailing));
        // Version and first record id are both required.
        let mut other_version = p.clone();
        other_version[0] = 12;
        assert!(!is_patcher(&other_version));
        let mut other_cid = p.clone();
        other_cid[4] = 0xE9;
        assert!(!is_patcher(&other_cid));
    }

    #[test]
    fn fixture_records_match_the_sample_sizes() {
        let p = patcher_payload(&[NodeSpec::new(
            "Vital",
            "Vital",
            other_plugin_payload("V", &[]),
        )]);
        let recs = records(&p);
        let sizes: Vec<(u32, usize)> = recs.iter().map(|(c, d)| (*c, d.len())).collect();
        assert_eq!(
            &sizes[..4],
            &[(1000, 64), (1002, 44), (1004, 324), (0, 394)]
        );
        assert!(sizes.contains(&(1500, 40)));
        assert_eq!(*sizes.last().unwrap(), (1001, 0));
        assert_eq!(hex("00ff"), vec![0, 255]);
    }

    #[test]
    fn plain_event_is_its_own_site() {
        let serum = serum1_payload("Serum", SERUM);
        let sites = plugin_sites(&serum);
        assert_eq!(sites.len(), 1);
        assert!(sites[0].path.is_empty());
        assert!(sites[0].label.is_empty());
        assert_eq!(sites[0].payload, serum.as_slice());
        assert_eq!(sites[0].location("Bass"), "Bass");
        assert_eq!(site_payload(&serum, &[]).unwrap(), serum.as_slice());
    }

    #[test]
    fn patcher_sites_skip_control_surface_and_non_nodes() {
        let (p, serum, vital) = two_node_patcher();
        let sites = plugin_sites(&p);
        assert_eq!(sites.len(), 2, "{sites:?}");
        // Record order: 1000, 1002, 1004, Control Surface (#3), nodes #4, #5.
        assert_eq!(sites[0].path, vec![4]);
        assert_eq!(sites[0].label, "Lead");
        assert_eq!(sites[0].plugin_name, "Serum");
        assert_eq!(sites[0].payload, serum.as_slice());
        assert_eq!(sites[1].path, vec![5]);
        assert_eq!(sites[1].plugin_name, "Vital");
        assert_eq!(sites[1].payload, vital.as_slice());
        assert_eq!(sites[0].location("Chords"), "Chords > Lead");
        assert_eq!(sites[0].location(""), "Lead");
        // The Control Surface record itself is a node with a 5005 blob.
        let cs = node_of(&p, &walk(&p, 4).unwrap()[3]).unwrap();
        assert_eq!(cs.plugin_name, "Control Surface");
        assert_eq!(cs.label, "Surface");
        assert_eq!(cs.state.len(), 116);
        // Every site resolves again through its path.
        for s in &sites {
            assert_eq!(site_payload(&p, &s.path).unwrap(), s.payload);
        }
    }

    #[test]
    fn patcher_without_plugin_nodes_has_no_sites() {
        let p = patcher_payload(&[]);
        assert!(is_patcher(&p));
        assert!(plugin_sites(&p).is_empty());
    }

    #[test]
    fn nested_patchers_are_followed() {
        let serum = serum1_payload("Serum", SERUM);
        let inner = patcher_payload(&[NodeSpec::new("Serum", "Inner", serum.clone())]);
        let outer = wrap_patcher("Outer", &inner);
        let sites = plugin_sites(&outer);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, vec![4, 4]);
        assert_eq!(sites[0].label, "Outer > Inner");
        assert_eq!(sites[0].payload, serum.as_slice());
        assert_eq!(site_payload(&outer, &[4, 4]).unwrap(), serum.as_slice());
        // The inner Patcher state itself is not a plugin site.
        assert_eq!(site_payload(&outer, &[4]).unwrap(), inner.as_slice());
    }

    #[test]
    fn nesting_is_capped() {
        let mut p = patcher_payload(&[NodeSpec::new(
            "Serum",
            "leaf",
            serum1_payload("Serum", SERUM),
        )]);
        for level in 1..MAX_NESTING {
            p = wrap_patcher(&format!("L{level}"), &p);
        }
        // MAX_NESTING levels in total: still reachable.
        assert_eq!(plugin_sites(&p).len(), 1);
        // One level deeper is ignored.
        let too_deep = wrap_patcher("deep", &p);
        assert!(plugin_sites(&too_deep).is_empty());
        let path = vec![4; MAX_NESTING + 1];
        assert!(site_payload(&too_deep, &path).is_err());
    }

    #[test]
    fn site_payload_rejects_bad_paths() {
        let (p, ..) = two_node_patcher();
        assert!(
            site_payload(&p, &[99])
                .unwrap_err()
                .contains("no record #99")
        );
        // Record #0 is the 1000 header, not a node.
        assert!(site_payload(&p, &[0]).unwrap_err().contains("not a node"));
        // Descending into a plugin state that is not a Patcher.
        assert!(
            site_payload(&p, &[4, 0])
                .unwrap_err()
                .contains("no Patcher")
        );
        // Plain events have no children.
        let serum = serum1_payload("Serum", SERUM);
        assert!(site_payload(&serum, &[0]).is_err());
    }

    fn identity_edit(p: &[u8], path: &[usize]) -> NodeEdit {
        NodeEdit {
            path: path.to_vec(),
            payload: site_payload(p, path).unwrap().to_vec(),
            plugin_name: None,
        }
    }

    #[test]
    fn empty_and_identity_rewrites_are_byte_identical() {
        let (p, ..) = two_node_patcher();
        assert_eq!(rewrite(&p, &[]).unwrap(), p);
        assert_eq!(rewrite(&p, &[identity_edit(&p, &[4])]).unwrap(), p);
        assert_eq!(
            rewrite(&p, &[identity_edit(&p, &[4]), identity_edit(&p, &[5])]).unwrap(),
            p
        );
        // Restating the existing plugin name is identical as well.
        let mut e = identity_edit(&p, &[5]);
        e.plugin_name = Some("Vital".into());
        assert_eq!(rewrite(&p, &[e]).unwrap(), p);
    }

    #[test]
    fn rewrite_grows_a_node_and_fixes_sizes() {
        let (p, serum, vital) = two_node_patcher();
        let bigger = {
            let mut v = serum.clone();
            v.extend_from_slice(&[0xAB; 1000]);
            v
        };
        let out = rewrite(
            &p,
            &[NodeEdit {
                path: vec![4],
                payload: bigger.clone(),
                plugin_name: None,
            }],
        )
        .unwrap();
        assert_eq!(out.len(), p.len() + 1000);
        // The size fix-ups keep the walk exact at every level.
        assert!(is_patcher(&out));
        let sites = plugin_sites(&out);
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].payload, bigger.as_slice());
        assert_eq!(sites[0].label, "Lead");
        assert_eq!(sites[1].payload, vital.as_slice());
        // Every top-level record except the edited node is untouched.
        let (a, b) = (records(&p), records(&out));
        assert_eq!(a.len(), b.len());
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            if i == 4 {
                assert_eq!(y.1.len(), x.1.len() + 1000);
            } else {
                assert_eq!(x, y, "record #{i} changed");
            }
        }
        // Inside the node only the 5005 record changed.
        let (ia, ib) = (walk(&a[4].1, 0).unwrap(), walk(&b[4].1, 0).unwrap());
        assert_eq!(ia.len(), ib.len());
        for (x, y) in ia.iter().zip(&ib) {
            if x.cid == CID_NODE_STATE {
                assert_eq!(&b[4].1[y.data.clone()], bigger.as_slice());
            } else {
                assert_eq!(
                    a[4].1[x.data.clone()],
                    b[4].1[y.data.clone()],
                    "cid {}",
                    x.cid
                );
            }
        }
    }

    #[test]
    fn rewrite_can_shrink_and_rename() {
        let (p, ..) = two_node_patcher();
        let small = other_plugin_payload("S", &[]);
        let out = rewrite(
            &p,
            &[NodeEdit {
                path: vec![4],
                payload: small.clone(),
                plugin_name: Some("Serum 2".into()),
            }],
        )
        .unwrap();
        assert!(out.len() < p.len());
        let sites = plugin_sites(&out);
        assert_eq!(sites[0].plugin_name, "Serum 2");
        assert_eq!(sites[0].label, "Lead", "label is kept");
        assert_eq!(sites[0].payload, small.as_slice());
        // 5008 is stored as UTF-16LE without a terminator.
        let node = &records(&out)[4].1;
        let inner = walk(node, 0).unwrap();
        let name = inner
            .iter()
            .find(|r| r.cid == CID_NODE_PLUGIN_NAME)
            .unwrap();
        assert_eq!(&node[name.data.clone()], utf16("Serum 2").as_slice());
    }

    #[test]
    fn rewrite_edits_several_nodes_in_one_patcher() {
        let (p, serum, vital) = two_node_patcher();
        let mut new_b = vital.clone();
        new_b.extend_from_slice(&[1, 2, 3]);
        let mut new_a2 = serum.clone();
        new_a2.extend_from_slice(&[9; 20]);
        let out = rewrite(
            &p,
            &[
                // Out of order on purpose.
                NodeEdit {
                    path: vec![5],
                    payload: new_b.clone(),
                    plugin_name: None,
                },
                NodeEdit {
                    path: vec![4],
                    payload: new_a2.clone(),
                    plugin_name: Some("Serum 2".into()),
                },
            ],
        )
        .unwrap();
        let sites = plugin_sites(&out);
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].payload, new_a2.as_slice());
        assert_eq!(sites[0].plugin_name, "Serum 2");
        assert_eq!(sites[1].payload, new_b.as_slice());
        assert_eq!(sites[1].plugin_name, "Vital");
        // +20 and +3 state bytes, +4 for "Serum" -> "Serum 2" in UTF-16LE.
        assert_eq!(out.len(), p.len() + 20 + 3 + 4);
    }

    #[test]
    fn rewrite_edits_a_nested_patcher_node() {
        let serum = serum1_payload("Serum", SERUM);
        let inner = patcher_payload(&[
            NodeSpec::new("Serum", "Inner", serum.clone()),
            NodeSpec::new("Vital", "Other", other_plugin_payload("Vital", &[1])),
        ]);
        let outer = wrap_patcher("Outer", &inner);
        assert_eq!(
            rewrite(&outer, &[identity_edit(&outer, &[4, 4])]).unwrap(),
            outer
        );
        let new_state = other_plugin_payload("New", &[5; 10]);
        let out = rewrite(
            &outer,
            &[NodeEdit {
                path: vec![4, 4],
                payload: new_state.clone(),
                plugin_name: Some("Serum 2".into()),
            }],
        )
        .unwrap();
        let sites = plugin_sites(&out);
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].path, vec![4, 4]);
        assert_eq!(sites[0].payload, new_state.as_slice());
        assert_eq!(sites[0].plugin_name, "Serum 2");
        assert_eq!(sites[0].label, "Outer > Inner");
        assert_eq!(sites[1].label, "Outer > Other");
        // The hosting node's own name and label are untouched.
        let host = &records(&out)[4].1;
        let host_inner = walk(host, 0).unwrap();
        let name = host_inner
            .iter()
            .find(|r| r.cid == CID_NODE_PLUGIN_NAME)
            .unwrap();
        assert_eq!(&host[name.data.clone()], utf16("Patcher").as_slice());
    }

    #[test]
    fn rewrite_rejects_invalid_edits() {
        let (p, serum, _) = two_node_patcher();
        let edit = |path: &[usize]| NodeEdit {
            path: path.to_vec(),
            payload: serum.clone(),
            plugin_name: None,
        };
        // Not a Patcher.
        assert!(rewrite(&serum, &[]).unwrap_err().contains("not a Patcher"));
        // Empty path.
        assert!(
            rewrite(&p, &[edit(&[])])
                .unwrap_err()
                .contains("empty path")
        );
        // Bad record index.
        assert!(
            rewrite(&p, &[edit(&[99])])
                .unwrap_err()
                .contains("no record #99")
        );
        // A non-node record and the Control Surface (not a plugin node).
        assert!(
            rewrite(&p, &[edit(&[0])])
                .unwrap_err()
                .contains("not a hosted plugin node")
        );
        assert!(
            rewrite(&p, &[edit(&[3])])
                .unwrap_err()
                .contains("not a hosted plugin node")
        );
        // Same node twice.
        assert!(
            rewrite(&p, &[edit(&[4]), edit(&[4])])
                .unwrap_err()
                .contains("conflicting")
        );
        // A node and a path below it.
        assert!(
            rewrite(&p, &[edit(&[4]), edit(&[4, 4])])
                .unwrap_err()
                .contains("conflicting")
        );
        // Descending into a node that hosts a plain plugin.
        assert!(
            rewrite(&p, &[edit(&[4, 4])])
                .unwrap_err()
                .contains("not a Patcher")
        );
        // Renaming needs a 5008 record: build a node without one.
        let mut inner = Vec::new();
        inner.extend_from_slice(&rec(5004, &utf16("Fruity Wrapper")));
        inner.extend_from_slice(&rec(5005, &serum));
        let mut nameless = p.clone();
        nameless.truncate(nameless.len() - 12); // drop the 1001 trailer
        nameless.extend_from_slice(&rec(0, &inner));
        nameless.extend_from_slice(&rec(1001, &[]));
        let idx = records(&nameless).len() - 2;
        let mut e = edit(&[idx]);
        e.plugin_name = Some("X".into());
        assert!(rewrite(&nameless, &[e]).unwrap_err().contains("5008"));
        // ... but a state-only edit of that node works.
        assert!(rewrite(&nameless, &[edit(&[idx])]).is_ok());
    }

    #[test]
    fn nodes_are_built_from_the_documented_layout() {
        let node = patcher_node(
            0x3ec,
            &NodeSpec::new("Serum 2", "Serum 2 #3", vec![0, 0, 0, 0]),
        );
        let recs = walk(&node, 0).unwrap();
        assert_eq!(recs.len(), 1);
        let inner = walk(&node[recs[0].data.clone()], 0).unwrap();
        let cids: Vec<u32> = inner.iter().map(|r| r.cid).collect();
        assert_eq!(
            cids,
            vec![
                5000, 5004, 5008, 5010, 5005, 5007, 5009, 5009, 5009, 5009, 5009, 5009, 5009, 5009,
                5001
            ]
        );
    }
}

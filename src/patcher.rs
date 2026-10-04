//! FL Studio Patcher support: plugin states hosted inside a Patcher
//! (`PluginParams`, event 213) payload.
//!
//! A Patcher is itself a plugin whose event-213 payload is
//! `u32 LE version = 4` followed by `[u32 cid][u64 size][data]` records (the
//! same framing as plugin chunks). Records with `cid = 0` are *nodes*:
//! containers holding another `[u32 cid][u64 size][data]` sequence, and a
//! node that hosts a plugin carries that plugin's saved state in its inner
//! record 5005 — byte-for-byte the layout of a plain plugin event-213
//! payload, so every plain-event recogniser and rewrite works on it
//! unchanged. See `docs/flp-patcher.md` for the layout tables, the evidence
//! and what is NOT known.
//!
//! This module only locates and replaces those states; every other byte of
//! the Patcher is copied verbatim (an empty or no-op edit list reproduces the
//! input byte for byte).

use std::collections::BTreeMap;
use std::ops::Range;

use crate::flp;

/// Patcher payload version (`u32 LE` at offset 0).
const PATCHER_VERSION: u32 = 4;
/// First top-level record of every Patcher payload (64 B header).
const CID_HEADER: u32 = 1000;
/// Outer cid of node containers.
const CID_NODE: u32 = 0;
/// Node record holding the hosted plugin's saved state.
const NODE_STATE: u32 = 5005;
/// Node record holding the plugin display name (UTF-16LE, no terminator).
const NODE_PLUGIN_NAME: u32 = 5008;
/// Node record holding the node label (UTF-16LE, no terminator).
const NODE_LABEL: u32 = 5010;
/// First plugin-state format version using the chunked layout.
const MIN_PLUGIN_STATE_VERSION: u32 = 5;
/// Nesting cap for Patcher-in-Patcher. Nesting has never been observed in a
/// real project; the cap only keeps malformed input from recursing forever.
const MAX_DEPTH: usize = 8;

/// A plugin state found inside an event-213 payload.
#[derive(Debug, Clone)]
pub struct PluginSite<'a> {
    /// Top-level record indices leading to the hosting node: empty when the
    /// plugin owns its event, one entry per Patcher level otherwise.
    pub path: Vec<usize>,
    /// Node labels (record 5010) along `path`, outermost first.
    pub labels: Vec<String>,
    /// Plugin display name of the node (record 5008); empty for a plain
    /// plugin event.
    pub plugin_name: String,
    /// The plugin state: the event payload itself (plain) or the node's
    /// 5005 record data. Same layout as a plain event-213 payload.
    pub payload: &'a [u8],
}

impl PluginSite<'_> {
    /// Location string for reports: `base` (channel or FX slot name)
    /// followed by the node labels, joined with ` > `
    /// (e.g. `Intro Chord Patch > Serum 2 #4`). Empty parts are skipped; a
    /// plain plugin yields `base` unchanged.
    pub fn location(&self, base: &str) -> String {
        let mut out = String::from(base);
        for label in &self.labels {
            if label.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push_str(" > ");
            }
            out.push_str(label);
        }
        out
    }
}

/// Replacement of one hosted plugin's state inside a Patcher.
#[derive(Debug, Clone)]
pub struct NodeEdit {
    /// Path of the node (see [`PluginSite::path`]); never empty.
    pub path: Vec<usize>,
    /// New plugin state (record 5005 data); must itself be a plugin state.
    pub payload: Vec<u8>,
    /// New display name (record 5008) or `None` to keep it.
    pub plugin_name: Option<String>,
}

/// Encode a node string field (UTF-16LE, no terminator).
fn encode_utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Decode a node string field (UTF-16LE, no terminator; lossy).
fn decode_utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .iter()
        .step_by(2)
        .zip(bytes.iter().skip(1).step_by(2))
        .map(|(&lo, &hi)| u16::from_le_bytes([lo, hi]))
        .collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_string()
}

fn u32_at(buf: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(pos..pos + 4)?.try_into().ok()?))
}

type RecordRanges = Vec<(u32, Range<usize>)>;

/// Walk `buf[start..]` as `[u32 cid][u64 size][data]` records; `Some` only
/// when the walk consumes the buffer exactly. Ranges index into `buf` and
/// cover the record data (the 12-byte header sits right before each).
fn walk(buf: &[u8], start: usize) -> Option<RecordRanges> {
    let mut it = flp::Records::new(buf.get(start..)?);
    let mut out = Vec::new();
    while let Some((cid, data)) = it.next() {
        let end = start + it.pos();
        out.push((cid, end - data.len()..end));
    }
    if it.overran() || start + it.pos() != buf.len() {
        return None;
    }
    Some(out)
}

/// Top-level records of a Patcher payload; `None` when `payload` is not one.
fn patcher_records(payload: &[u8]) -> Option<RecordRanges> {
    if u32_at(payload, 0)? != PATCHER_VERSION || u32_at(payload, 4)? != CID_HEADER {
        return None;
    }
    walk(payload, 4)
}

/// True when `payload` is a Patcher event-213 payload: version 4, first
/// record cid 1000, and the record walk consumes the payload exactly.
pub fn is_patcher(payload: &[u8]) -> bool {
    patcher_records(payload).is_some()
}

/// True when `state` has the chunked plugin-state layout (version >= 5 and
/// an exact record walk). The Control Surface node's 5005 is a version-1 blob
/// and does not qualify.
fn is_plugin_state(state: &[u8]) -> bool {
    u32_at(state, 0).is_some_and(|v| v >= MIN_PLUGIN_STATE_VERSION) && walk(state, 4).is_some()
}

fn find<'a>(buf: &'a [u8], recs: &RecordRanges, cid: u32) -> Option<&'a [u8]> {
    recs.iter()
        .find(|(c, _)| *c == cid)
        .map(|(_, r)| &buf[r.clone()])
}

fn string_field(buf: &[u8], recs: &RecordRanges, cid: u32) -> String {
    find(buf, recs, cid).map(decode_utf16le).unwrap_or_default()
}

/// Enumerate the plugin states inside an event-213 payload.
///
/// A plain plugin event yields itself (empty path); a Patcher yields one
/// site per node whose record 5005 is a plugin state, in file order (the
/// Control Surface node and non-node records are skipped). A node whose
/// 5005 is itself a Patcher is followed recursively up to a depth cap
/// (UNTESTED: never observed in a real project).
pub fn plugin_sites(event: &[u8]) -> Vec<PluginSite<'_>> {
    let mut out = Vec::new();
    if is_patcher(event) {
        collect(event, &mut Vec::new(), &mut Vec::new(), 0, &mut out);
    } else {
        out.push(PluginSite {
            path: Vec::new(),
            labels: Vec::new(),
            plugin_name: String::new(),
            payload: event,
        });
    }
    out
}

fn collect<'a>(
    patcher: &'a [u8],
    path: &mut Vec<usize>,
    labels: &mut Vec<String>,
    depth: usize,
    out: &mut Vec<PluginSite<'a>>,
) {
    let Some(recs) = patcher_records(patcher) else {
        return;
    };
    for (idx, (cid, range)) in recs.iter().enumerate() {
        if *cid != CID_NODE {
            continue;
        }
        let node = &patcher[range.clone()];
        let Some(inner) = walk(node, 0) else {
            continue;
        };
        let Some(state) = find(node, &inner, NODE_STATE) else {
            continue;
        };
        path.push(idx);
        labels.push(string_field(node, &inner, NODE_LABEL));
        if is_patcher(state) {
            if depth < MAX_DEPTH {
                collect(state, path, labels, depth + 1, out);
            }
        } else if is_plugin_state(state) {
            out.push(PluginSite {
                path: path.clone(),
                labels: labels.clone(),
                plugin_name: string_field(node, &inner, NODE_PLUGIN_NAME),
                payload: state,
            });
        }
        path.pop();
        labels.pop();
    }
}

/// The plugin state at `path` inside `event` (the event payload itself for
/// an empty path). Used to detect stale plans.
pub fn site_payload<'a>(event: &'a [u8], path: &[usize]) -> Result<&'a [u8], String> {
    let mut cur = event;
    for &idx in path {
        let recs =
            patcher_records(cur).ok_or("path leads through a payload that is not a Patcher")?;
        let (cid, range) = recs
            .get(idx)
            .ok_or_else(|| format!("Patcher has no record {idx}"))?;
        if *cid != CID_NODE {
            return Err(format!("Patcher record {idx} is cid {cid}, not a node"));
        }
        let node = &cur[range.clone()];
        let inner = walk(node, 0).ok_or_else(|| format!("Patcher record {idx} is not a node"))?;
        cur = find(node, &inner, NODE_STATE)
            .ok_or_else(|| format!("Patcher node {idx} has no plugin state"))?;
    }
    Ok(cur)
}

struct Pending<'a> {
    rest: &'a [usize],
    edit: &'a NodeEdit,
}

/// Replace the targeted nodes' plugin states (and optionally their display
/// names), recomputing the node record sizes; every other record is copied
/// verbatim. With no edits, or edits that change nothing, the output equals
/// the input byte for byte.
///
/// Errors: `event` is not a Patcher, a path is empty / out of range / not a
/// plugin node, a replacement is not a plugin state, or edits conflict
/// (duplicate paths, or a node edited together with nodes below it).
pub fn rewrite(event: &[u8], edits: &[NodeEdit]) -> Result<Vec<u8>, String> {
    if !is_patcher(event) {
        return Err("payload is not a Patcher".into());
    }
    let pending: Vec<Pending> = edits
        .iter()
        .map(|edit| Pending {
            rest: &edit.path,
            edit,
        })
        .collect();
    rewrite_level(event, &pending, 0)
}

fn rewrite_level(patcher: &[u8], pending: &[Pending], depth: usize) -> Result<Vec<u8>, String> {
    if depth > MAX_DEPTH {
        return Err("Patcher nesting is too deep".into());
    }
    let recs = patcher_records(patcher).ok_or("payload is not a Patcher")?;
    let mut groups: BTreeMap<usize, Vec<Pending>> = BTreeMap::new();
    for p in pending {
        let Some((&idx, rest)) = p.rest.split_first() else {
            return Err("empty node path".into());
        };
        match recs.get(idx) {
            None => return Err(format!("Patcher has no record {idx}")),
            Some((cid, _)) if *cid != CID_NODE => {
                return Err(format!("Patcher record {idx} is cid {cid}, not a node"));
            }
            Some(_) => {}
        }
        groups
            .entry(idx)
            .or_default()
            .push(Pending { rest, edit: p.edit });
    }

    let mut out = Vec::with_capacity(patcher.len());
    out.extend_from_slice(&patcher[..4]);
    for (idx, (cid, range)) in recs.iter().enumerate() {
        match groups.get(&idx) {
            Some(group) => {
                let node = rewrite_node(&patcher[range.clone()], group, depth, idx)?;
                push_rec(&mut out, *cid, &node);
            }
            None => out.extend_from_slice(&patcher[range.start - 12..range.end]),
        }
    }
    Ok(out)
}

fn rewrite_node(
    node: &[u8],
    group: &[Pending],
    depth: usize,
    idx: usize,
) -> Result<Vec<u8>, String> {
    let inner = walk(node, 0).ok_or_else(|| format!("Patcher record {idx} is not a node"))?;
    let state_pos = inner
        .iter()
        .position(|(cid, _)| *cid == NODE_STATE)
        .ok_or_else(|| format!("Patcher node {idx} has no plugin state"))?;
    let old_state = &node[inner[state_pos].1.clone()];

    let direct: Vec<&Pending> = group.iter().filter(|p| p.rest.is_empty()).collect();
    if direct.len() > 1 {
        return Err(format!("node {idx} is edited more than once"));
    }
    if direct.len() == 1 && group.len() > 1 {
        return Err(format!(
            "node {idx} is edited together with nodes nested below it"
        ));
    }
    let (new_state, new_name) = match direct.first() {
        Some(d) => {
            if !is_plugin_state(old_state) {
                return Err(format!("node {idx} does not host a plugin state"));
            }
            if !is_plugin_state(&d.edit.payload) {
                return Err(format!(
                    "replacement for node {idx} is not a plugin state payload"
                ));
            }
            (d.edit.payload.clone(), d.edit.plugin_name.as_deref())
        }
        None => (rewrite_level(old_state, group, depth + 1)?, None),
    };

    let mut out = Vec::with_capacity(node.len() + new_state.len());
    let mut name_done = false;
    for (i, (cid, range)) in inner.iter().enumerate() {
        if i == state_pos {
            push_rec(&mut out, NODE_STATE, &new_state);
        } else if let (NODE_PLUGIN_NAME, Some(name), false) = (*cid, new_name, name_done) {
            push_rec(&mut out, NODE_PLUGIN_NAME, &encode_utf16le(name));
            name_done = true;
        } else {
            out.extend_from_slice(&node[range.start - 12..range.end]);
        }
    }
    if new_name.is_some() && !name_done {
        return Err(format!("node {idx} has no plugin name record"));
    }
    Ok(out)
}

fn push_rec(out: &mut Vec<u8>, cid: u32, data: &[u8]) {
    out.extend_from_slice(&cid.to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(data);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{
        other_plugin_payload, patcher_container, patcher_control_surface_node, patcher_event,
        patcher_plugin_node, rec, serum1_payload, serum2_payload, utf16le,
    };

    const S1: &str = "/VST/Serum.vst3";
    const S2: &str = "/VST/Serum2.vst3";

    fn two_node_patcher() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let s2 = serum2_payload("Serum2", S2);
        let vital = other_plugin_payload("Vital", "/VST/Vital.vst3");
        let ev = patcher_event(
            &[
                patcher_plugin_node(0x3ec, "Serum 2", "Serum 2 #3", &s2),
                patcher_plugin_node(0x3f5, "Vital", "Vital", &vital),
            ],
            &[0x3ec, 0x3f5],
        );
        (ev, s2, vital)
    }

    fn identity_edits(ev: &[u8]) -> Vec<NodeEdit> {
        plugin_sites(ev)
            .into_iter()
            .map(|s| NodeEdit {
                path: s.path.clone(),
                payload: s.payload.to_vec(),
                plugin_name: None,
            })
            .collect()
    }

    #[test]
    fn detects_patcher_not_plain_events() {
        let (ev, s2, _) = two_node_patcher();
        assert!(is_patcher(&ev));
        assert!(!is_patcher(&s2));
        assert!(!is_patcher(&serum1_payload("Serum", S1)));
        assert!(!is_patcher(&[]));
        assert!(!is_patcher(&[4, 0, 0, 0]));
        // Version 4 + cid 1000 but a record walk that does not end exactly.
        let mut trailing = ev.clone();
        trailing.push(0);
        assert!(!is_patcher(&trailing));
        let truncated = &ev[..ev.len() - 1];
        assert!(!is_patcher(truncated));
        // Right version, wrong first record.
        let mut wrong_first = ev.clone();
        wrong_first[4..8].copy_from_slice(&1002u32.to_le_bytes());
        assert!(!is_patcher(&wrong_first));
        // A bare old-format state blob that happens to be version 4.
        let mut bare = 4u32.to_le_bytes().to_vec();
        bare.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(!is_patcher(&bare));
    }

    #[test]
    fn plain_event_yields_itself() {
        let p = serum1_payload("Serum", S1);
        let sites = plugin_sites(&p);
        assert_eq!(sites.len(), 1);
        assert!(sites[0].path.is_empty());
        assert!(sites[0].labels.is_empty());
        assert_eq!(sites[0].payload, p.as_slice());
        assert_eq!(sites[0].location("Lead"), "Lead");
        assert_eq!(sites[0].location(""), "");
        assert_eq!(site_payload(&p, &[]).unwrap(), p.as_slice());
    }

    #[test]
    fn enumerates_plugin_nodes_only() {
        let (ev, s2, vital) = two_node_patcher();
        let sites = plugin_sites(&ev);
        // Control Surface (record 3) and the Notes / Audio output / wire /
        // trailer records are skipped; the plugin nodes sit at 4 and 5.
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].path, vec![4]);
        assert_eq!(sites[0].labels, vec!["Serum 2 #3".to_string()]);
        assert_eq!(sites[0].plugin_name, "Serum 2");
        assert_eq!(sites[0].payload, s2.as_slice());
        assert_eq!(sites[1].path, vec![5]);
        assert_eq!(sites[1].plugin_name, "Vital");
        assert_eq!(sites[1].payload, vital.as_slice());
        assert_eq!(
            sites[0].location("Intro Chord Patch"),
            "Intro Chord Patch > Serum 2 #3"
        );
        assert_eq!(sites[0].location(""), "Serum 2 #3");
        assert_eq!(site_payload(&ev, &[4]).unwrap(), s2.as_slice());
        assert_eq!(site_payload(&ev, &[5]).unwrap(), vital.as_slice());
    }

    #[test]
    fn patcher_without_plugins_has_no_sites() {
        let ev = patcher_event(&[], &[]);
        assert!(is_patcher(&ev));
        assert!(plugin_sites(&ev).is_empty());
    }

    #[test]
    fn site_payload_rejects_bad_paths() {
        let (ev, _, _) = two_node_patcher();
        assert!(site_payload(&ev, &[99]).is_err());
        // Record 0 is the cid-1000 header, not a node.
        assert!(site_payload(&ev, &[0]).is_err());
        assert!(site_payload(&ev, &[4, 0]).is_err());
        assert!(site_payload(&serum1_payload("Serum", S1), &[0]).is_err());
    }

    #[test]
    fn nested_patcher_is_followed() {
        let s1 = serum1_payload("Serum", S1);
        let inner = patcher_event(
            &[patcher_plugin_node(0x3ec, "Serum", "Inner Serum", &s1)],
            &[0x3ec],
        );
        let outer = patcher_event(
            &[patcher_plugin_node(
                0x3f5,
                "Patcher",
                "Inner Patcher",
                &inner,
            )],
            &[0x3f5],
        );
        let sites = plugin_sites(&outer);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, vec![4, 4]);
        assert_eq!(
            sites[0].labels,
            vec!["Inner Patcher".to_string(), "Inner Serum".to_string()]
        );
        assert_eq!(
            sites[0].location("Chan"),
            "Chan > Inner Patcher > Inner Serum"
        );
        assert_eq!(sites[0].payload, s1.as_slice());
        assert_eq!(site_payload(&outer, &[4, 4]).unwrap(), s1.as_slice());
        assert_eq!(site_payload(&outer, &[4]).unwrap(), inner.as_slice());

        // Rewriting through two levels changes only the inner state.
        let new_state = serum2_payload("Serum2", S2);
        let out = rewrite(
            &outer,
            &[NodeEdit {
                path: vec![4, 4],
                payload: new_state.clone(),
                plugin_name: Some("Serum 2".into()),
            }],
        )
        .unwrap();
        assert!(is_patcher(&out));
        let sites = plugin_sites(&out);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].path, vec![4, 4]);
        assert_eq!(sites[0].plugin_name, "Serum 2");
        assert_eq!(sites[0].payload, new_state.as_slice());
        // Identity through two levels is byte-identical.
        assert_eq!(rewrite(&outer, &identity_edits(&outer)).unwrap(), outer);
    }

    #[test]
    fn nesting_beyond_the_cap_is_ignored() {
        let mut ev = patcher_event(
            &[patcher_plugin_node(
                0x3ec,
                "Serum",
                "Deep",
                &serum1_payload("Serum", S1),
            )],
            &[0x3ec],
        );
        for _ in 0..MAX_DEPTH + 2 {
            ev = patcher_event(
                &[patcher_plugin_node(0x3f5, "Patcher", "Wrap", &ev)],
                &[0x3f5],
            );
        }
        assert!(plugin_sites(&ev).is_empty());
    }

    #[test]
    fn empty_and_identity_rewrites_are_byte_identical() {
        let (ev, _, _) = two_node_patcher();
        assert_eq!(rewrite(&ev, &[]).unwrap(), ev);
        assert_eq!(rewrite(&ev, &identity_edits(&ev)).unwrap(), ev);
        // Renaming to the existing name is a no-op too.
        let mut edits = identity_edits(&ev);
        edits[0].plugin_name = Some("Serum 2".into());
        assert_eq!(rewrite(&ev, &edits).unwrap(), ev);
    }

    #[test]
    fn edit_growing_a_node_fixes_sizes() {
        let (ev, old_state, vital) = two_node_patcher();
        let mut grown = serum1_payload("Serum", S1);
        grown.extend_from_slice(&rec(99, &[7u8; 5000]));
        let out = rewrite(
            &ev,
            &[NodeEdit {
                path: vec![4],
                payload: grown.clone(),
                plugin_name: Some("Serum".into()),
            }],
        )
        .unwrap();
        // The 5008 name shrinks from "Serum 2" to "Serum" (14 B -> 10 B).
        assert_eq!(out.len(), ev.len() + grown.len() - old_state.len() - 4);
        // Still a well-formed Patcher whose sites reflect the edit.
        assert!(is_patcher(&out));
        let sites = plugin_sites(&out);
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].payload, grown.as_slice());
        assert_eq!(sites[0].plugin_name, "Serum");
        assert_eq!(sites[0].labels, vec!["Serum 2 #3".to_string()]);
        assert_eq!(sites[1].payload, vital.as_slice());
        // Every other top-level record is byte-identical.
        let a = patcher_records(&ev).unwrap();
        let b = patcher_records(&out).unwrap();
        assert_eq!(a.len(), b.len());
        for (i, ((ca, ra), (cb, rb))) in a.iter().zip(&b).enumerate() {
            assert_eq!(ca, cb);
            if i != 4 {
                assert_eq!(&ev[ra.clone()], &out[rb.clone()], "record {i} changed");
            }
        }
        // Inside the edited node only 5005 and 5008 differ.
        let node_a = &ev[a[4].1.clone()];
        let node_b = &out[b[4].1.clone()];
        let ia = walk(node_a, 0).unwrap();
        let ib = walk(node_b, 0).unwrap();
        assert_eq!(
            ia.iter().map(|r| r.0).collect::<Vec<_>>(),
            ib.iter().map(|r| r.0).collect::<Vec<_>>()
        );
        for ((cid, ra), (_, rb)) in ia.iter().zip(&ib) {
            if *cid == NODE_STATE || *cid == NODE_PLUGIN_NAME {
                continue;
            }
            assert_eq!(&node_a[ra.clone()], &node_b[rb.clone()], "inner {cid}");
        }
        // The node's declared record size matches its content (the walk of
        // the whole payload is exact, checked by is_patcher above) and the
        // name record was re-encoded without a terminator.
        assert_eq!(
            find(node_b, &ib, NODE_PLUGIN_NAME).unwrap(),
            utf16le("Serum")
        );
    }

    #[test]
    fn several_nodes_in_one_patcher() {
        let s1a = serum1_payload("Serum", S1);
        let s1b = serum1_payload("Serum", S1);
        let vital = other_plugin_payload("Vital", "/VST/Vital.vst3");
        let ev = patcher_event(
            &[
                patcher_plugin_node(0x3ec, "Serum", "A", &s1a),
                patcher_plugin_node(0x3f5, "Vital", "V", &vital),
                patcher_plugin_node(0x3fe, "Serum", "B", &s1b),
            ],
            &[0x3ec, 0x3f5, 0x3fe],
        );
        let sites = plugin_sites(&ev);
        assert_eq!(
            sites.iter().map(|s| s.path.clone()).collect::<Vec<_>>(),
            vec![vec![4], vec![5], vec![6]]
        );
        let s2 = serum2_payload("Serum2", S2);
        let out = rewrite(
            &ev,
            &[
                NodeEdit {
                    path: vec![6],
                    payload: s2.clone(),
                    plugin_name: Some("Serum 2".into()),
                },
                NodeEdit {
                    path: vec![4],
                    payload: s2.clone(),
                    plugin_name: Some("Serum 2".into()),
                },
            ],
        )
        .unwrap();
        let sites = plugin_sites(&out);
        assert_eq!(sites.len(), 3);
        assert_eq!(sites[0].payload, s2.as_slice());
        assert_eq!(sites[0].labels, vec!["A".to_string()]);
        assert_eq!(sites[1].payload, vital.as_slice());
        assert_eq!(sites[1].plugin_name, "Vital");
        assert_eq!(sites[2].payload, s2.as_slice());
        assert_eq!(sites[2].labels, vec!["B".to_string()]);
    }

    #[test]
    fn invalid_edits_are_rejected() {
        let (ev, s2, _) = two_node_patcher();
        let edit = |path: Vec<usize>, payload: Vec<u8>, name: Option<&str>| NodeEdit {
            path,
            payload,
            plugin_name: name.map(str::to_string),
        };
        // Not a Patcher.
        assert!(rewrite(&s2, &[]).is_err());
        assert!(rewrite(&s2, &[edit(vec![0], s2.clone(), None)]).is_err());
        // Empty / out-of-range / non-node / Control Surface paths.
        assert!(rewrite(&ev, &[edit(vec![], s2.clone(), None)]).is_err());
        assert!(rewrite(&ev, &[edit(vec![99], s2.clone(), None)]).is_err());
        assert!(rewrite(&ev, &[edit(vec![0], s2.clone(), None)]).is_err());
        assert!(rewrite(&ev, &[edit(vec![3], s2.clone(), None)]).is_err());
        // Path through a node that hosts a plugin, not a Patcher.
        assert!(rewrite(&ev, &[edit(vec![4, 4], s2.clone(), None)]).is_err());
        // Replacement is not a plugin state.
        assert!(rewrite(&ev, &[edit(vec![4], vec![1, 2, 3], None)]).is_err());
        assert!(rewrite(&ev, &[edit(vec![4], 4u32.to_le_bytes().to_vec(), None)]).is_err());
        // Duplicate paths.
        assert!(
            rewrite(
                &ev,
                &[
                    edit(vec![4], s2.clone(), None),
                    edit(vec![4], s2.clone(), None)
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn conflicting_nested_edits_are_rejected() {
        let s1 = serum1_payload("Serum", S1);
        let inner = patcher_event(&[patcher_plugin_node(0x3ec, "Serum", "S", &s1)], &[0x3ec]);
        let outer = patcher_event(
            &[patcher_plugin_node(0x3f5, "Patcher", "P", &inner)],
            &[0x3f5],
        );
        let edit = |path: Vec<usize>, payload: Vec<u8>| NodeEdit {
            path,
            payload,
            plugin_name: None,
        };
        // A Patcher node is not itself a plugin state, so editing it
        // directly fails; editing it together with a node below fails too.
        assert!(rewrite(&outer, &[edit(vec![4], inner.clone())]).is_err());
        assert!(
            rewrite(
                &outer,
                &[edit(vec![4], s1.clone()), edit(vec![4, 4], s1.clone())]
            )
            .is_err()
        );
    }

    #[test]
    fn missing_name_record_is_an_error_only_when_renaming() {
        let s1 = serum1_payload("Serum", S1);
        // A hand-built node without 5008.
        let node = patcher_container(&[
            (5000, vec![0u8; 40]),
            (5010, utf16le("NoName")),
            (5005, s1.clone()),
            (5001, vec![]),
        ]);
        let mut ev = patcher_event(&[], &[]);
        // Splice the node in before the trailer (after record 3 = Control
        // Surface): rebuild the payload record by record.
        let recs = patcher_records(&ev).unwrap();
        let mut rebuilt = ev[..4].to_vec();
        for (i, (_, r)) in recs.iter().enumerate() {
            rebuilt.extend_from_slice(&ev[r.start - 12..r.end]);
            if i == 3 {
                rebuilt.extend_from_slice(&node);
            }
        }
        ev = rebuilt;
        assert_eq!(plugin_sites(&ev).len(), 1);
        let keep = NodeEdit {
            path: vec![4],
            payload: s1.clone(),
            plugin_name: None,
        };
        assert_eq!(rewrite(&ev, std::slice::from_ref(&keep)).unwrap(), ev);
        let rename = NodeEdit {
            plugin_name: Some("X".into()),
            ..keep
        };
        assert!(rewrite(&ev, &[rename]).is_err());
    }

    #[test]
    fn control_surface_node_is_not_a_site() {
        let cs = patcher_control_surface_node();
        // Sanity: the builder's Control Surface node is a cid-0 record whose
        // 5005 is a version-1 blob.
        assert_eq!(&cs[..4], &0u32.to_le_bytes());
        let ev = patcher_event(&[], &[]);
        assert!(plugin_sites(&ev).is_empty());
    }
}

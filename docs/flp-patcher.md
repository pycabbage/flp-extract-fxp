# FLP: plugins hosted inside FL Studio Patcher

Status: `list` / `extract` / `convert` / `patch` see and rewrite Serum and
Serum2 instances inside **Patcher** nodes. The record layout below was read
off one real FL Studio **26.1.6.5639** project; the Serum → Serum2 conversion
path inside a Patcher is verified **structurally only** (see §6 "NOT
verified"). Companion docs: `flp-serum2-conversion.md` (event-213 byte rules
for plain plugin instances) and `flp-conversion.md` (the converter).

Module: `src/patcher.rs`. Real-sample numbers in this document are from a
local, untracked project (`tests/fixtures/patcher_serum2.flp`, never
committed; tests that need it skip when it is absent).

## 1. Why Patcher was invisible

A plain plugin instance is an event-213 (`PluginParams`) event whose payload
is `u32 version (12)` + `[u32 cid][u64 size][data]` chunks 1, 2, 30, …, 53
(`flp-serum2-conversion.md` §2). A **Patcher** instance is *also* an event 213
(the event 201 a few events earlier is the UTF-16LE plugin name `Patcher`, the
channel name is the user's), but its payload is the Patcher's own graph
document. It starts with `u32 version = 4`, and `flp::parse_plugin_params`
treats `version < 5` as a bare legacy state blob with an empty plugin name —
so every scanner (`core::scan_serum_instances`, `core::scan_serum2_instances`,
`flpconv::scan_convertible_detailed`, `flpconv::patch_serum_metadata`)
silently skipped it, together with every plugin hosted inside.

The sample (FL 26.1.6.5639, 2,281 events) has 12 event-213 events: 3 plain
Serum 2 channels, mixer-slot FX plugins (kHs Limiter, …) and **two Patchers**:

| channel name | event payload | nodes |
|---|---:|---|
| `Serum 2 #3` | 443,408 B | Control Surface, Serum 2 (`Serum 2 #3`), Vital |
| `Intro Chord Patch` | 8,605,162 B | Control Surface, Serum 2 (`Serum 2 #4`), Serum 2, Serum 2 |

## 2. Patcher event-213 payload

Same framing as plugin chunks: `u32 LE version` followed by
`[u32 cid][u64 size][data]` records (what `flp::Records` walks).

**Detection** (`patcher::is_patcher`) is structural: `payload[0..4] == 4`,
`payload[4..8] == 1000` (first record is cid 1000) and the record walk
consumes the payload *exactly*. A plain plugin event starts with version 12
and cid 1 and can never match.

Top-level records, in the order seen in both Patchers:

| record | size | content |
|---|---:|---|
| `1000` | 64 B | first record; content not understood |
| `1002` | 44 B | **unknown** (contains floats) |
| `1004` | 324 B | **unknown**; only the first 72 B are non-zero |
| `0` — Control Surface node | 394 B | node container (below); its 5005 is a 116-byte version-1 blob, **not** a plugin payload |
| `0` — one node per hosted plugin | varies | node container (below) |
| `0` — `Notes` | 74 B | inner 1400 (28 B) + 1402 (UTF-16LE label) + 1401 (empty) |
| `0` — `Audio output` | 88 B | inner 1400 (28 B) + 1402 (UTF-16LE label) + 1401 (empty) |
| `0` — wires | 56 B each | inner 1300 (32 B) + 1301 (empty); references node/port ids (4 wires in the first, 6 in the second Patcher) |
| `1500` | 40 B | **unknown** |
| `0`, `0` | 0 B | two empty containers |
| `0` — one per plugin node | 36 B each | inner 1600 (24 B; starts with the node id — remainder **unknown**) |
| `1001` | 0 B | trailer |

Records with outer `cid = 0` are containers whose data is again a
`[u32 cid][u64 size][data]` sequence. A **node** container holds:

| inner record | size | content |
|---|---:|---|
| `5000` | 40 B | node header: `u32 kind` (2 = Control Surface, 3 = plugin), `u32 node id` (e.g. `0x3ec`), then fields whose meaning is **not known** (position-like numbers 1199/382 were seen) |
| `5004` | 28 / 30 B | UTF-16LE, no terminator: `Fruity Wrapper` (Control Surface: `Control Surface`) |
| `5008` | varies | UTF-16LE, no terminator: hosted plugin's display name (`Serum 2`, `Vital`) |
| `5010` | varies | UTF-16LE, no terminator: node label (`Serum 2 #3`, `Vital`, `Surface`) |
| **`5005`** | varies | **the hosted plugin's saved state** (below) |
| `5007` | 40 B | identical in every node of the sample; content unknown |
| `5009` × 8 | 28 B each | port records; port ids are `node id + 1 … + 8`. Serum 2 and Vital nodes carry the same eight ports, only the ids differ (the Control Surface node has one) |
| `5001` | 0 B | end marker |

What `1002`, `1004`, `1500`, `1600`, `5007` and the individual port fields
mean is **not known**; the tool never interprets them, it only copies them.

### 2.1 5005 is a plain event-213 payload

The 5005 data of a hosted VST node has byte-for-byte the layout of a plain
plugin event-213 payload (`u32 version = 12`; chunks `1` (20 B), `2` (25 B),
`30`, `32`, `50`, `52`, `54` name, `55` filename, `56` vendor, `53` FL VST3
wrapper). Evidence, from all **7** Serum 2 instances of the sample (3 plain +
4 inside Patchers):

- identical top-level cid order `1, 2, 30, 32, 50, 52, 54, 55, 56, 53`;
- `cid 1` `u32@8 = 1` in all seven;
- `cid 52` = `XESVsfsPerum 2` (+ 2 NULs) in all seven;
- `cid 54` = `Serum2`;
- wrapper (cid 53): prologue `u32 = 1`, then inner records `1` (64 B),
  `3` (processor `XferJson`), `2` (controller `XferJson`), `4` (10,496 B).

Consequently the existing recognisers (`serum::is_serum1*`, `is_serum2`,
`serum1_chunk_from_state`, `serum2_records_from_state`) and the rewrite
`flpconv::build_serum2_payload` apply to a node's 5005 payload unchanged.

## 3. What the tool does

`patcher::plugin_sites(event_payload)` yields one `PluginSite` per hosted
plugin: a plain event is its own site (empty `path`, empty label); a Patcher
yields one site per node that has a 5005 record in the chunked plugin layout
(`version >= 5`; this drops the Control Surface), in record order. A site
carries the node `path` (record index inside the Patcher; one entry per
nesting level), the label (5010), the plugin name (5008) and the payload
slice. A Patcher hosted in a Patcher is followed recursively up to
`MAX_NESTING = 4` levels (never observed, see §6).

Every event-213 scanner iterates `plugin_sites` instead of `ev.data`
(`core::scan_serum_instances`, `core::scan_serum2_instances`,
`flpconv::scan_convertible_detailed`, `flpconv::patch_serum_metadata`), so row
numbering stays consistent: `InstancePlan::instance_index` and the core scan
rows count plugin sites in the same file order, nested ones included. Serum2
inside a Patcher is counted / listed / extracted; Serum FX inside a Patcher
is left untouched with the same warning as on a plain slot.

**Location format.** `Instance::channel_name`, `Serum2Instance::channel_name`,
`InstancePlan::channel_name` and the convert / patch report entries become
`<channel or FX slot name> > <node label>` for a hosted plugin
(`Intro Chord Patch > Serum 2 #4`); nested labels are joined with ` > `. Plain
instances keep their old value, so existing output is unchanged.

**`convert`.** `InstancePlan` has a new `node_path` (empty = the plugin owns
its event) and its `payload` is the *site* payload. `flpconv::apply` checks
every plan against the nested payload (`patcher::site_payload`; a stale plan
is rejected), builds the new Serum2 payload with the unchanged
`build_serum2_payload`, and for nested plans calls
`patcher::rewrite(event, &[NodeEdit])`: it replaces only the targeted nodes'
5005 (and sets 5008 to `Serum 2`), recomputes the node record's size, copies
every other record verbatim, and the event varint length and FLdt u32 length
are re-framed by the existing splice. Kept untouched: node label 5010, header
5000, ports, wires and every other node.

**`patch`.** The same path with the patched cid-3 chunk and no name change.

`patcher::rewrite` is byte-exact by construction: an empty edit list, or an
edit that restates the existing bytes, returns the input byte for byte.
Errors: input is not a Patcher, empty path, a path that is not a hosted
plugin node (bad index, Control Surface, …), conflicting edits (same node
twice, a node and one below it).

## 4. How it was verified

On the real project (release binary):

```
$ flp-extract-fxp list patcher_serum2.flp
patcher_serum2.flp: 0 Serum preset(s), 7 Serum2 instance(s)

$ flp-extract-fxp extract --serum2 -o out patcher_serum2.flp
  [S2 01] channel 'Serum 2 #2' -> preset 'MO_HA_SY_Chord_Edm_Saw' …
  [S2 02] channel 'Serum 2' -> preset 'TSP_S2DG_Bass_mono_reese' …
  [S2 03] channel 'Serum 2 #3 > Serum 2 #3' -> preset 'JJP_S2RNBE_LEAD_rockstar_lead' …
  [S2 04] channel 'Intro Chord Patch > Serum 2 #4' -> preset 'RU_VB_pad_bloom' …
  [S2 05] channel 'Intro Chord Patch > Serum 2' -> preset 'SM101 - DRONE - Transmitter' …
  [S2 06] channel 'Intro Chord Patch > Serum 2' -> preset 'WW_NA_Synth_Pad_Through_the_Ringer' …
  [S2 07] channel 'Serum 2 #4' -> preset ' - Init - ' …
```

(before the change `list` reported only the 3 plain instances.)

Gated tests (`tests/integration.rs`, skipped when
`tests/fixtures/patcher_serum2.flp` is absent):

- `real_patcher_project_lists_nested_serum2` — 7 Serum 2, 4 of them with
  ` > ` in the location, preset names in file order;
- `real_patcher_events_rewrite_byte_identically` — for both real Patchers
  `rewrite(ev, &[])`, a per-node identity edit and an all-node identity edit
  are byte-identical to the input;
- `real_patcher_project_converts_a_transplanted_serum_node` — a synthetic
  Serum 1 payload (node name `Serum`) is transplanted into the first Serum 2
  node of the first real Patcher, `convert` runs through the CLI: exactly 1
  converted, 7 Serum 2 afterwards, every other event / record / node record
  byte-identical.

Ungated tests use synthetic Patchers built from records **copied from the real
sample** (`src/testutil.rs`, `tests/integration.rs`: 1000, 1002, 1004 without
its zero tail, a wire, 1500, 5007, port records, the Control Surface node):
detection vs plain events, site enumeration, nested Patchers and the depth
cap, byte-identical empty / no-op rewrite, growing and shrinking a node, size
fix-ups, several nodes in one Patcher, invalid / conflicting edits, stale-plan
rejection, Serum FX and Serum2 in a Patcher, patch inside a Patcher, and CLI
`list` / `extract` / `validate` / `convert` / `patch` (the rescanned converted
node's processor + controller records equal what the same Serum becomes on a
plain channel).

## 5. Limitations

- Automation clips and Patcher **control-surface links** that target a
  Serum parameter are not remapped (they live in records the tool does not
  interpret, see §2); after a conversion they still point at the old
  parameter ids.
- The node label (5010, e.g. `Serum 2 #3`) is kept as is; only the plugin
  display name (5008) changes to `Serum 2`.
- Serum FX hosted in a Patcher is not converted (same as on plain slots).
- Only the Serum nodes are touched; wires and ports are copied verbatim,
  which assumes the Serum2 node exposes the same ports the Serum node had
  (unverified, see §6).

## 6. NOT verified

- **No real Serum 1 inside a Patcher was available.** The sample contains
  Serum 2 (and Vital) only. The Serum → Serum2 conversion inside a Patcher is
  verified structurally: synthetic Serum nodes, and a Serum payload
  transplanted into a real Serum 2 node. Whether a real Serum 1 node has
  exactly the same 5000 / 5007 / 5009 records as the Serum 2 nodes is an
  assumption.
- **No FL Studio load test.** Nobody opened a converted Patcher project in FL
  Studio; "converted" means structurally valid and byte-localised, not that
  FL Studio loads it.
- **Nested Patchers were never observed.** The recursive handling (`path`
  with several entries, depth cap 4) is exercised by synthetic tests only.
- Only FL Studio **26.1.6.5639** was seen; other FL versions may store
  Patchers differently (the detector requires version 4 and first cid 1000
  and otherwise falls back to the old behaviour: the event is ignored).

# FL Studio Patcher: plugins hosted inside Patcher nodes

Status: implemented (`src/patcher.rs`) and verified **structurally** on one
real project (FL Studio 26.1.6.5639) that holds Serum 2 inside two Patchers.
There is no real project with Serum 1 inside a Patcher, and nothing was loaded
into FL Studio — see §6 ("NOT verified") before relying on the convert path.
Companion docs: `flp-serum2-conversion.md` (plain event-213 byte rules),
`flp-conversion.md` (the shipped convert feature).

## 1. Why Patcher was invisible

FL Studio's **Patcher** is itself a plugin. Its saved state is an ordinary
`PluginParams` event (id 213) — but with `u32 LE version = 4`, followed by the
Patcher's own records. `flp::parse_plugin_params` treats every version below 5
as a bare state blob with empty name / filename, so each scanner saw "a plugin
without a name" and skipped the event; the plugins *hosted by* the Patcher live
one level deeper and were never looked at. `list`, `extract`, `convert` and
`patch` therefore all ignored Serum / Serum 2 inside a Patcher.

In the real sample the Patcher event is preceded by an event 201 holding the
UTF-16LE string `Patcher` (the plugin name FL shows for the channel), then the
channel's name event (203). The tool does not rely on that: detection is
structural (§2.1).

## 2. Record layout

All multi-byte values are little-endian. A *record* is
`[u32 cid][u64 size][data]` — the same framing as the plugin chunks of a plain
event-213 payload.

### 2.1 Patcher payload

`u32 version = 4`, then records until the end of the payload. A payload is
treated as a Patcher when **all** hold: version is 4, the first record has cid
1000, and the record walk consumes the payload exactly (`patcher::is_patcher`).

Top-level record order observed in both real Patchers (sizes in bytes):

| cid | size | seen | meaning |
|---|---|---|---|
| 1000 | 64 | first | header record (first u32 differs per Patcher: 0x3fe / 0x407; **meaning unknown**) |
| 1002 | 44 | 1x | **unknown** (guess: contains f32 values, e.g. a view position) |
| 1004 | 324 | 1x | **unknown** |
| 0 | 394 | 1x | node: Control Surface (kind 2) |
| 0 | varies | 1 per hosted plugin | node: plugin (kind 3), §2.2 |
| 0 | 74 | 1x | node `Notes` (inner 1400 / 1402 / 1401) |
| 0 | 88 | 1x | node `Audio output` (inner 1400 / 1402 / 1401) |
| 0 | 56 | N | wire: inner 1300 (32 B) + 1301 (empty); 1300 references port ids |
| 1500 | 40 | 1x | **unknown** |
| 0 | 0 | 2x | empty containers |
| 0 | 36 | 1 per node, Control Surface included, same order | inner 1600 (24 B; starts with the node id (u64), rest **unknown**) |
| 1001 | 0 | last | trailer |

Records with outer `cid = 0` are *containers*: their data is again a
`[u32 cid][u64 size][data]` sequence. The tool treats a `cid 0` record as a
**node** only if it holds an inner record 5005.

### 2.2 Plugin node (inner records)

Observed order: 5000, 5004, 5008, 5010, 5005, 5007, 5009 x8, 5001.

| inner cid | size | meaning |
|---|---|---|
| 5000 | 40 | node header: `u32 kind` (2 = Control Surface, 3 = plugin), `u32 node id` (e.g. 0x3ec), rest unknown |
| 5004 | var | UTF-16LE, no terminator: `Fruity Wrapper` for plugin nodes (the wrapper type) |
| 5008 | var | UTF-16LE, no terminator: plugin display name (`Serum 2`, `Vital`) |
| 5010 | var | UTF-16LE, no terminator: node **label** (e.g. `Serum 2 #3`); guess: the title FL shows on the node |
| **5005** | var | **the hosted plugin's saved state** — see §3 |
| 5007 | 40 | identical in every node; **meaning unknown** |
| 5009 | 28 | port record x8 (Control Surface: x1); the port ids are `node id + 1 .. + 8` in a shuffled order; Serum 2 and Vital nodes carry the same 8 ports (only the ids differ). Wire records reference these ids |
| 5001 | 0 | end marker |

The Control Surface node also has a 5005, but it is a 116-byte **version-1**
blob (first record cid 2000), not a plugin payload.

What 1002, 1004, 1500, 1600 and 5007 contain is **not known**; the tool never
interprets them and copies them verbatim.

## 3. 5005 is a plain plugin event-213 payload

The 5005 payload of a hosted VST node has byte-for-byte the layout of a plain
event-213 payload: `u32 version = 12`, then chunks cid 1 (20 B), 2 (25 B), 30,
32, 50, 52, 54 (name), 55 (filename), 56 (vendor), 53 (FL VST3 wrapper).

Evidence, checked on all 7 Serum 2 instances of the real project (3 on plain
channels, 4 inside Patchers): identical cid order `1, 2, 30, 32, 50, 52, 54,
55, 56, 53`; cid 1 is 20 B with `u32@8 = 1`; cid 52 = `XESVsfsPerum 2\0\0`;
cid 54 = `Serum2`; cid 55 a `...Serum2.vst3` path; wrapper prologue `u32 = 1`
followed by inner records 1 (64 B), 3 (processor `XferJson`), 2 (controller
`XferJson`) and 4 (10,496 B parameter list). The Vital node's 5005 has the same
shape (version 12, cid 54 `Vital`).

Consequently the existing recognisers (`serum::is_serum1*`, `is_serum2`,
`serum1_chunk_from_state`, `serum2_records_from_state`) and the existing
rewrite (`flpconv::build_serum2_payload`) apply to a node's 5005 payload
unchanged.

A 5005 counts as a plugin state when its version is >= 5 and its record walk
consumes it exactly; this excludes the Control Surface blob.

## 4. What the tool does

`patcher::plugin_sites(event_payload)` yields the plugin states inside an
event: a plain plugin event yields itself (empty path); a Patcher yields one
site per node with a plugin-state 5005, in file order. A site carries the node
path (top-level record indices), the node labels, the display name (5008) and
the payload slice. A node whose 5005 is itself a Patcher is followed
recursively up to a depth cap of 8 (untested, §6).

Every event-213 scanner iterates the sites instead of the raw event payload:
`core::scan_serum_instances`, `core::scan_serum2_instances`,
`flpconv::scan_convertible_detailed`, `flpconv::patch_serum_metadata`. A plugin
hosted in a Patcher is reported as `<channel or FX slot name> > <node label>`
(e.g. `Intro Chord Patch > Serum 2 #4`) in `list`, `extract`, `convert`,
`patch` and the JSON / wasm reports (`Instance::channel_name`,
`Serum2Instance::channel_name`, `InstancePlan::channel_name`). Row numbering
(the web UI's selection indices, `InstancePlan::instance_index`) counts hosted
instances in file order exactly like plain ones.

- **list / extract**: Serum presets and Serum 2 instances inside Patchers are
  listed / extracted like plain ones (`extract --serum2` writes one
  `.SerumPreset` per Serum 2 instance, nested included). `{channel}` in
  `--name-template` expands to the `>`-joined location, with `>` sanitised to
  `_` in file names.
- **convert**: `InstancePlan::node_path` is empty for a plugin that owns its
  event. For a hosted Serum synth `apply` verifies the plan against the nested
  payload (stale-plan detection), builds the new Serum 2 payload with the
  existing recipe and calls `patcher::rewrite`, which replaces only that node's
  5005 and sets its display name 5008 to `Serum 2`; the node label (5010),
  header (5000), ports, wires and every other node stay byte-identical. The
  node record size is recomputed, then the event varint length and the FLdt
  length are re-framed by the existing splice code.
  `ConvertedInstance::new_payload_len` is the length of the new *plugin*
  payload (not of the whole Patcher event).
- **patch**: the preset metadata of a hosted Serum synth is rewritten in place
  the same way (no name change of the node).
- Serum FX inside a Patcher stays untouched with the same warning as on plain
  slots; Serum 2 inside a Patcher is counted / listed, never planned.
- `patcher::rewrite` copies every record it does not edit verbatim: an empty or
  no-op edit list reproduces the input byte for byte (tested).

## 5. How it was verified (real project)

Sample: a 16.6 MB project saved by FL Studio 26.1.6.5639, 12 event-213 events:
3 Serum 2 on plain channels, mixer-slot FX plugins, and two Patchers —
channel `Serum 2 #3` (443,408 B: Control Surface + Serum 2 + Vital) and channel
`Intro Chord Patch` (8,605,162 B: Control Surface + three Serum 2).

- Before the change `list` reported 3 Serum 2 instances; now `list` reports 7
  and `extract --serum2` writes 7 `.SerumPreset` files, in file order:
  `MO_HA_SY_Chord_Edm_Saw`, `TSP_S2DG_Bass_mono_reese`,
  `JJP_S2RNBE_LEAD_rockstar_lead` (in Patcher `Serum 2 #3`), `RU_VB_pad_bloom`,
  `SM101 - DRONE - Transmitter`, `WW_NA_Synth_Pad_Through_the_Ringer` (the last
  three in `Intro Chord Patch`) and ` - Init - ` (plain channel `Serum 2 #4`).
- For both real Patchers `rewrite(event, &[])`, one identity edit per node
  (including re-encoding the unchanged display name) and all identity edits
  together reproduce the event byte for byte.
- Because no Serum 1 exists inside a Patcher in any available project, the
  convert path was exercised by **transplanting** a synthetic Serum 1 payload
  (node name `Serum`) into one real Serum 2 node of the first Patcher, running
  `convert` through the CLI, and checking that exactly 1 instance was converted,
  that 7 Serum 2 instances exist afterwards, and that every other event and
  every other node is byte-identical; only the node's 5005 and 5008 changed.

All of this is gated test data: `tests/fixtures/patcher_serum2.flp` is
untracked (third-party project content) and the `real_patcher_*` tests skip
silently when it is absent (see AGENTS.md). Synthetic Patcher fixtures built
from small records copied from that project (`src/testutil.rs`,
`tests/integration.rs`) cover the same paths without the file.

## 6. NOT verified

- **No real Serum 1 inside a Patcher.** The only available project hosts
  Serum 2. The Serum -> Serum 2 convert path inside a Patcher is verified only
  structurally (transplanted synthetic payload; layout reasoning from §3).
- **No FL Studio load test.** Neither converted nor patched Patchers were
  opened in FL Studio, so it is unknown whether FL accepts a Patcher node whose
  display name / state were swapped, or whether it needs the unknown records
  (1002 / 1004 / 1500 / 1600 / 5007) to be consistent with the hosted plugin.
- **Nested Patchers (Patcher inside a Patcher) were never observed.** The
  recursion and the `node_path` handling for them are unit-tested on synthetic
  input only, and assume the inner Patcher sits in the node's 5005 record.
- **Only FL Studio 26.1.6.5639 was seen.** Other FL versions may order or
  version the Patcher records differently; a Patcher that does not match §2.1
  is ignored, as before.

## 7. Limitations

- Automation clips and Patcher control-surface links that point at Serum
  parameters are **not** remapped: Serum and Serum 2 parameter ids differ, so
  after a conversion such links keep their old ids. (The same applies to plain
  channels; it is noted here because Patcher exposes parameters through its
  Control Surface.)
- The node label (5010) is kept, so a converted node may still be called e.g.
  `Serum`; only the plugin display name (5008) becomes `Serum 2`.
- Serum FX inside a Patcher is not converted (same as on plain slots).
- Hosted instances share the `<channel> > <label>` location format; two nodes
  with the same label in one Patcher are told apart only by their order.

# Serum → Serum2 FLP conversion (offline converter)

Status: shipped and verified against real Serum2.vst3 2.0.23 (byte-identity +
dynamic acceptance; see §Verification). Companion docs: `flp-serum2-conversion.md`
(event-213 byte rules), `s1-to-s2-mapping.md` (static RE of the real importer),
`s2-runtime-tables.md` (runtime-dumped conversion tables), `serum2-state-format.md`
(CBOR body).

## What it does

Rewrites every Serum **synth** instance inside an FLP into a Serum2 instance
with a fully converted preset state, so opening the converted file in FL Studio
shows Serum2 already loaded with the preset. This replaces the manual
per-instance workflow (swap the plugin slot to Serum2, then import the
extracted .fxp through Serum2's UI) with a single one-shot conversion.

The FLP container, all other events (patterns, mixer, channel/track names) and
non-Serum plugins are left untouched; only the Serum instances' event-213
payloads are replaced and the `FLdt` chunk length is fixed (see
`flp-serum2-conversion.md` §5–§6: nothing outside event 213 needs to change).

This is distinct from `extract`: `extract` produces Serum `.fxp` files for
Serum2's *manual* import; `convert` produces an FLP whose plugin slots already
hold converted Serum2 states.

## Pipeline

```
input (plain .flp, or zipped loop package → unpacked in memory       src/zip.rs,
  one document per *.flp member                        core::flp_inputs)
  → locate event 213 (PluginParams) payloads          src/flp.rs, src/flpconv.rs
  → inner cid-3 chunk = Serum chunk                 (zlib streams + u32 LE trailer)
  → s1state::parse_preset → 172,736 B state blob      src/s1state.rs
  → importer: port of s1state_load (RVA 0x4DABC0)     src/importer.rs
        descriptor tables runtime-dumped from the DLL (docs/s2-runtime-tables.md)
        generated into src/s2tables.rs by tools/gen_s2tables.py from docs/data/*.json
  → merge over init-body skeleton (INIT_BODY)         docs/data/init_body.cbor
  → canonical CBOR body                               src/s2tree.rs
  → XferJson processor record                         src/serum2state.rs
        fresh JSON header (productVersion 2.0.23, version 9.0)
        + md5(frame) + one libzstd level-3 zstd frame
  → XferJson controller record                        src/flpconv.rs
        template docs/data/serum2_controller_record.bin,
        JSON header patched with presetName/presetAuthor/presetDescription
  → FL VST3 wrapper rebuild                           src/flpconv.rs
        inner cid-1 (64 B, unchanged), cid-3 = processor record,
        cid-2 = controller record (inserted after cid-3),
        cid-4 = docs/data/serum2_cid4.bin (10,496 B param list)
        top-level cid-1 A=1, cid-52 = "XESVsfsPerum 2",
        cid-54 "Serum2", cid-55 …/Serum2.vst3 (directory prefix kept),
        cid-56 unchanged
  → event splice + FLdt u32 length fix                src/flpconv.rs::apply
```

| Stage | Module | Ground truth |
|---|---|---|
| event scan / planning | `src/flp.rs`, `src/serum.rs`, `src/flpconv.rs::scan_convertible_detailed` | `flp-serum2-conversion.md` §1–§5 |
| S1 state parse | `src/s1state.rs` | `serum-fxp-format.md` |
| conversion | `src/importer.rs` — faithful port of `s1state_load` | `s1-to-s2-mapping.md`, `s2-runtime-tables.md` |
| tables | `src/s2tables.rs` — **GENERATED**, do not hand-edit | `s2-runtime-tables.md` |
| CBOR + container | `src/s2tree.rs`, `src/serum2state.rs` | `serum2-state-format.md` |
| FLP rewrite | `src/flpconv.rs::apply` | `flp-serum2-conversion.md` §6 |

Notes:

- `src/importer.rs` implements both the synth path (`flag = 0`, what FLP
  conversion wires) and the FX build (`flag = 1`, drops the oscillator WTOsc
  nodes) — the latter was used during RE calibration; FLP conversion itself
  only converts synths.
- The conversion data tables baked into `src/s2tables.rs` (343 descriptors,
  source-enum / aux-remap / defaults tables) and the three embedded binary
  assets under `docs/data/` (`init_body.cbor`, `serum2_cid4.bin`,
  `serum2_controller_record.bin` — compile-time `include_bytes!` inputs) are
  runtime dumps of Serum2.vst3 2.0.23, not hand-written constants (see
  `s2-runtime-tables.md`). They are functional inputs of the shipped
  converter and therefore tracked.

## Untracked verification artifacts

Everything that exists only to *verify* the converter is deliberately kept
out of the repo (third-party preset content and research scaffolding); the
files remain on the working machine and the tests that need them skip
silently when absent, so a fresh clone builds and tests green.

| untracked path | content | how to reproduce |
|---|---|---|
| `tests/fixtures/serina1/*.fxp` | the 5 Serum presets of the sample project | `flp-extract-fxp extract <sample>.flp --out tests/fixtures/serina1 --overwrite`, then rename to `0N.fxp` |
| `tests/fixtures/serina1.flp` | the sample FL Studio project (5 Serum + 1 Serum2) | copy from the local sample library (`assets/`, untracked) |
| `tests/fixtures/golden_s2/0N_processor_state.bin` | converted processor records produced by the REAL importer | call `s1state_load` at runtime per `docs/s2-runtime-tables.md` (harness method), wrap with `serum2state::build_processor_record`-equivalent container rules |
| `tests/fixtures/extracted_serum1.fxp` | extraction-feature fixture | `flp-extract-fxp extract` on the sample project, keep preset 01 |
| `tests/fixtures/legacy/*.fxp` + `*_importer_tree.cbor` | 6 legacy (2015-era) presets and the REAL importer's converted trees for them | call `s1state_load` on each raw fxp chunk (ctypes harness, §Verification), dump the walked tree as canonical CBOR (`*_importer_tree.cbor`) |
| `docs/data/*.json` | runtime-dumped descriptor / remap / defaults tables | dump per `docs/s2-runtime-tables.md` (LoadLibraryW + InitDll + memory reads) |
| `C:\Users\cabbage\Documents\Xfer\Serum 2 Presets\Presets\Factory\**\*.SerumPreset` | the 626 real Serum2 factory presets (read-only, never copied into the repo) | shipped with any Serum2 install; the `corpus_preset_round_trip` test reads 5 of them only when `FLPX_S2_CORPUS_DIR` points there |
| `tools/` | table generator (`gen_s2tables.py`), template extractor, canonical CBOR reference encoder | session scaffolding; `src/s2tables.rs` is committed, so nothing in the repo needs them |

Provenance of every dumped constant is documented in
`docs/s2-runtime-tables.md` (entry layout, RVAs, cross-checks), which is the
authoritative description if regeneration is ever needed.

## Surfaces

Zipped FL Studio loop packages (`PK`-prefixed ZIP) are accepted everywhere a
plain `.flp` is: `src/zip.rs` unpacks the archive in memory (store + deflate
entries; encrypted and Zip64 archives are rejected; the combined decompressed
size is capped at 256 MiB; zip-in-zip is never recursed into) and every
`*.flp` member — case-insensitive — is processed as its own document
(`core::flp_inputs`). CLI convert derives per-member output names
(`<input>_<member>_serum2.flp`), and `--out` requires a single-document
input; the wasm `convert_flp` exposes one document per member through
`doc_count` / `doc_name_at` / `flp_at` (`flp` keeps returning the first
document). Unparseable members inside an archive are skipped with a warning
instead of aborting.

**Caveat:** no real FL Studio "Zipped loop package" export was available to
test against (the export needs the FL Studio UI). The reader implements the
standard ZIP structures such exports use, and every `.flp` entry is probed
rather than assuming a fixed member layout; confirm against a real export
when one can be produced.

| Surface | Entry point | Behavior |
|---|---|---|
| CLI | `flp-extract-fxp convert <input.flp|input.zip|dir|glob> [--out <path>] [--dry-run] [--json]` | default output `<input>_serum2.flp` next to the input, or `<input>_<member>_serum2.flp` per archive member (`--out` accepted for a single-document input only — counted after input resolution, so a directory/glob that resolves to one file is fine); `--dry-run` prints the per-instance plan without writing; an instance that fails to convert aborts the file with an error naming the instance; `--json` prints a structured `ConvertReport` (camelCase keys aligned with the wasm report) on stdout and moves progress to stderr. Inputs go through the shared `resolve_inputs` layer: files pass through, directories are walked recursively collecting `.flp` (case-insensitive), `*`/`?`/`**` glob patterns are expanded in-process; results are sorted and de-duplicated, and zero matches abort with `error: no .flp files found in <path>` |
| CLI | `flp-extract-fxp convert-fxp <inputs.fxp...> [--out <dir>] [--overwrite]` | standalone-preset variant (see §convert-fxp below); default output `<stem>.SerumPreset` next to each input; a failing input aborts with an error |
| wasm | `convert_flp(data)` / `convert_flp_selected(data, indices)` -> `ConvertReport` (`converted_count`, `doc_count`/`doc_name_at`/`flp_at`, `flp`, `warnings_json`, `details_json`) | per-instance failures become warnings in the report; those instances are left as Serum |
| web | "Convert to Serum2" button in the browser UI (one per uploaded project) | converts in-browser per project, then downloads `<name>-serum2.flp` (a multi-member zip input downloads `<name>-serum2.zip` with one converted .flp per member); the UI accepts multiple .flp files and .zip archives (only `.flp` entries inside are scanned), and each project keeps its own card, selection, and conversion state; with preset-table rows selected, only those instances of that project are converted (no selection = all; selection is a plain-FLP feature, see below); a per-project conversion report card shows the per-instance details table (channel / preset / state → cid3 size / notes), the before/after FLP size comparison, and the full warnings list (skipped instances with their reasons: Serum FX left untouched, per-instance failures, ...) |

### Per-instance selection (subset conversion)

`convert_flp_selected(data, indices)` converts only the selected Serum synth
instances. `indices` are scan-order instance indices — the same numbers the
web UI's preset table shows (`WasmPreset::index`), **not** plan positions:
Serum FX rows exist in the table but are never planned, so plan index ≠ row
index in general. Every planned instance therefore carries its table row in
`flpconv::InstancePlan::instance_index` (`None` when the preset chunk could
not be recovered and the core scan produced no row);
`flpconv::filter_plans_by_rows` maps a selection to plan positions and
collects the per-instance warnings. Rules:

- an empty selection converts everything (`convert_flp` behavior);
- unselected instances stay byte-identical (`apply` only rewrites planned
  instances) and each one is reported in `warnings_json`;
- a selected index with no convertible Serum synth behind it (a Serum FX
  row, an out-of-range index) is reported in `warnings_json` and skipped;
- selection is a **plain-`.flp` feature**: for a zipped loop package the
  table's row numbers are global across members while plans are per
  document, so a selection cannot be mapped unambiguously —
  `convert_flp_selected` rejects archive inputs and the archive is always
  converted whole.

## Serum2 preset extraction (`extract --serum2`)

While `extract` normally skips Serum2 instances (their `XferJson` state is not
a Serum chunk), `flp-extract-fxp extract --serum2` additionally writes one
`<nn>_<presetName>.SerumPreset` per Serum2 instance — Serum2's native preset
format — into the regular output directory. Without the flag, behavior is
unchanged. Each instance's inner cid-3 (processor) record supplies the preset
body; the inner cid-2 (controller) record's JSON header supplies
`presetName`/`presetAuthor`/`presetDescription` (fallback file name
`Instance N`).

The authored body is derived from the instantiated state body by
`serum2state::state_body_to_authored` (grounded against the 626-file factory
corpus, `s2-param-corpus.md` §3/§9): drop the state-only `component` key,
copy all per-instance sections and meta keys 1:1, add the 14 authored-only
keys — 8 engine-type UI keys (`WTOsc`/`Osc`/`MultiSampleOsc`/`SpectralOsc`/
`GranularOsc` arrays, `Filter`/`ClipPlayer`/`SerumGUI` maps) with
corpus-consensus default values, and 6 metadata keys (`fileType`,
`presetName`, `presetAuthor`, `presetDescription`, empty
`arpBankDisplayName`/`clipBankDisplayName`). 162 − 1 + 14 = the standard
175 top-level keys; the container is re-assembled with a recomputed md5
(`build_preset_file`), with `productVersion`/`version` carried from the
state body (fallback `2.0.23`/`9.0`).

Verification status — honest:

- **Structural**: the generated file parses as a standard `XferJson`
  container (one zstd frame, `hash` = md5(frame), declared size exact); its
  top-level key set is **identical** to real factory presets (175 keys,
  zero diff against the corpus), and its per-instance sections are byte-form
  copies of a state Serum2 itself accepts (the FLP-conversion goldens).
  An env-gated test (`FLPX_S2_CORPUS_DIR`) round-trips 5 real factory
  presets through the same encode/decode path.
- **NOT yet verified: does the Serum2 UI load the file?** The preset browser
  cannot be driven by the dynamic-verification harness, so no live
  load-acceptance test exists for `.SerumPreset` files (unlike the FLP
  conversion path, whose processor states were accepted via `setState`).
  Risk areas are the synthesized UI-map default values (pure UI state, no
  engine parameters) and the optional authored-only naming keys that a state
  does not carry (`Macro.name`, `displayName`, `curveDisplayName`,
  `arpBankDisplayName`/`clipBankDisplayName`) — all optional in the corpus,
  where they vary per file. Treat the output as structurally valid until a
  manual load check is done.

## Verification (real Serum2.vst3 2.0.23)

- **Byte-identity vs the real importer**: golden converted states for the 5
  presets of the sample project were produced by calling the REAL
  `s1state_load` at runtime (ctypes harness: `LoadLibraryW` + `InitDll`, call
  at `base+0x4DABC0` with derived args). Unit tests
  `golden_byte_identical_01..05` (`src/importer.rs`) require the Rust
  converter's CBOR bodies to equal them; the fixtures are untracked (see
  above) and the tests skip when they are absent. We emit libzstd level-3
  zstd frames (`s2tree::zstd_frame`) — the same encoder that produced the
  goldens, so converted states are byte-comparable end to end.
- **Legacy byte-identity**: the same harness called `s1state_load` on the 6
  legacy fixtures in `tests/fixtures/legacy/`; the resulting json trees were
  serialized to canonical CBOR and merged over the init-body skeleton exactly
  like the modern path. `legacy_golden_fl_*` (`src/importer/tests.rs`)
  compare the Rust converter's CBOR bodies against them: 4 of 6 byte-identical,
  `FL_FMItUp` / `FL_BASS_Adventure` differ in the single `RoutingSlot4` leaf
  (limitation (a)).

- **Dynamic acceptance**: the 5 converted cid-3 processor states inside a
  converted real FLP (`tests/fixtures/serina1.flp`) were fed via `setState` to
  fresh real Serum2 instances: all returned kResultOk (0), all post-load
  states carried valid md5 hashes, and the post-load states were byte-identical
  to the post-states of the real importer's output (4 of 5 exactly; the 5th
  differed by one 1-ULP leaf value in one build — see limitation (c)).
- **FLP-level integration tests** (`tests/integration.rs`):
  `convert_writes_output`, `converted_flp_scans_clean` (the converted file
  scans as 6 Serum2 instances: 5 converted + the pre-existing one),
  `converted_flp_diff_is_localized` (the diff is limited to the Serum
  instances' event-213 payloads), and the `subset_convert_*` tests (a
  row-subset conversion leaves every unselected event-213 payload
  byte-identical and the remaining Serum instances untouched).
- **Browser flow** verified end-to-end (scan → convert → download).

## Limitations

Known and deliberate; none hidden from the user (the tools report them in
warnings/output):

- **(a) Legacy (2015-era) presets are upgraded, with one known per-preset
  gap**: state blobs of 21,808 / 28,232 bytes (Serum ≈ 1.0.x–1.1, chunk
  version floats ≈ 0.131–0.1531) are zero-padded to 172,736 bytes — exactly
  what the real Serum2 importer does — and flow through the standard
  conversion pipeline, which applies the importer's version-gated legacy
  migrations (mod-slot dest restamps, per-FX level-out/aux defaults, classic
  LFO region normalization, version ladders for `serum1Version`/routing/
  MPE, legacy stream layout `[frames][tuning][noise]`, and the pre-reorder
  distortion-menu remap). Verified byte-identical (CBOR-body compare against
  the REAL importer called at runtime, same harness as the modern goldens)
  for four of the six legacy fixtures in `tests/fixtures/legacy/`
  (untracked; regenerate with `conv_work/legacy_golden.py`-equivalent calls
  to `s1state_load`, see §Verification); `FL_FMItUp` (0.147, 21,808 B) and
  `FL_BASS_Adventure` (0.1531, 28,232 B) each differ in exactly ONE leaf:
  `RoutingSlot4.kParamRoutingDest` converts to `kRoutingDestMaster` where
  the real importer produces `kRoutingDestDirect`. The value that drives
  that slot for legacy presets was not recoverable from the static
  disassembly nor from any raw blob field (no stored field reproduces the
  per-fixture pattern), so it is left honest instead of guessed. Covered
  version floats: 0.1470 and 0.1531 (the two legacy layouts 21,808 B /
  28,232 B); other legacy versions convert through the same migrations but
  are not golden-verified.
- **(b) Serum FX instances are not converted** — there is no calibrated
  Serum2-FX FLP template. They are left untouched and reported (warning +
  skipped).
- **(c) `pow()` 1-ULP divergence**: `pow()` differs by 1 ULP between the
  native (CRT) and wasm (Rust libm) builds, which can shift ONE leaf value per
  affected preset by 1 ULP (observed: `Global0.kParamPortamentoTime` in preset
  01, `Env0.kParamAttack` in preset 05). Serum2's own re-serialization is
  more nondeterministic than this (double-vs-f32 re-emission between
  sessions), so this is cosmetic. CLI and wasm outputs are otherwise
  byte-identical.
- **(d) Wavetable data is embedded**: the converted state references
  wavetable/noise data via embedded CBOR byte strings
  (`embeddedWTData`/`embeddedNoiseData`), exactly like the real importer — no
  external files are needed, and nothing is written next to the FLP.
- **(e) Controller template is 2.0.22-era**: the controller record template
  (`docs/data/serum2_controller_record.bin`, lifted from the genuine Serum2
  instance in the calibration project) gets its JSON header patched per preset
  (preset name/author/description), but its `productVersion` strings remain
  those of the embedded calibration record (2.0.22). FL and Serum2 tolerate
  this — proven by the sample project (the converted FLP loads and the states
  are accepted). The processor record, which we synthesize fresh, carries
  2.0.23 / version 9.0.

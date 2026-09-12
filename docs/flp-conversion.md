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
        + md5(frame) + one raw-block zstd frame
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
| `docs/data/*.json` | runtime-dumped descriptor / remap / defaults tables | dump per `docs/s2-runtime-tables.md` (LoadLibraryW + InitDll + memory reads) |
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
| CLI | `flp-extract-fxp convert <input.flp|input.zip> [--out <path>] [--dry-run]` | default output `<input>_serum2.flp` next to the input, or `<input>_<member>_serum2.flp` per archive member (`--out` accepted for a single-document input only); `--dry-run` prints the per-instance plan without writing; an instance that fails to convert aborts the file with an error naming the instance |
| wasm | `convert_flp(data) -> ConvertReport` (`converted_count`, `doc_count`/`doc_name_at`/`flp_at`, `flp`, `warnings_json`, `details_json`) | per-instance failures become warnings in the report; those instances are left as Serum |
| web | "Convert to Serum2" button in the browser UI | converts in-browser, then downloads `<name>-serum2.flp` (a multi-member zip input downloads `<name>-serum2.zip` with one converted .flp per member) |

## Verification (real Serum2.vst3 2.0.23)

- **Byte-identity vs the real importer**: golden converted states for the 5
  presets of the sample project were produced by calling the REAL
  `s1state_load` at runtime (ctypes harness: `LoadLibraryW` + `InitDll`, call
  at `base+0x4DABC0` with derived args). Unit tests
  `golden_byte_identical_01..05` (`src/importer.rs`) require the Rust
  converter's CBOR bodies to equal them; the fixtures are untracked (see
  above) and the tests skip when they are absent. The zstd frame differs only
  in compression level (we emit raw-block frames; the goldens used libzstd
  level 3) — both are standard frames, and both are accepted by the plugin.
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
  instances' event-213 payloads).
- **Browser flow** verified end-to-end (scan → convert → download).

## Limitations

Known and deliberate; none hidden from the user (the tools report them in
warnings/output):

- **(a) Only modern-format Serum presets** (172,736-byte state blobs,
  Serum ≥ ~1.2) are converted. Old-format (2015-era, 21,808 / 28,232-byte
  blobs) presets are rejected by the S1 parser: the wasm/web path leaves the
  instance untouched with a warning, the CLI aborts the file with an error
  naming the instance.
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
- **(e) Raw-block zstd frames** (uncompressed) grow the FLP by ~0.4 MB per
  converted instance compared to Serum2's own compressed frames. The plugin
  accepts them; smaller frames are a future optimization.
- **(f) Controller template is 2.0.22-era**: the controller record template
  (`docs/data/serum2_controller_record.bin`, lifted from the genuine Serum2
  instance in the calibration project) gets its JSON header patched per preset
  (preset name/author/description), but its `productVersion` strings remain
  those of the embedded calibration record (2.0.22). FL and Serum2 tolerate
  this — proven by the sample project (the converted FLP loads and the states
  are accepted). The processor record, which we synthesize fresh, carries
  2.0.23 / version 9.0.

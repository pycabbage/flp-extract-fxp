# AGENTS.md

## What this is

One Cargo crate (`flp-extract-fxp`) that is simultaneously:
- a native CLI binary (`src/main.rs`), and
- a `cdylib` compiled to `wasm32-unknown-unknown` (`src/web.rs`, wasm-bindgen)
  that powers the React frontend in `front/` (deployed to GitHub Pages).

Both surfaces share the same core logic in `src/core.rs`, `src/flp.rs`,
`src/serum.rs`, `src/fxp.rs`, plus the Serum → Serum2 conversion stack:
`src/s1state.rs` (Serum state parser), `src/importer.rs` (faithful port of
Serum2.vst3's `s1state_load`), `src/s2tree.rs` + `src/serum2state.rs`
(canonical CBOR + XferJson container), `src/flpconv.rs` (FLP event-213
rewrite), and `src/s2tables.rs` — **GENERATED** (runtime-dumped descriptor
tables, provenance in `docs/s2-runtime-tables.md`; the generator and its
`docs/data/*.json` inputs are untracked — never hand-edit the file).
`src/web.rs` is `#[cfg]`-gated to wasm targets only, so native
`cargo build`/`cargo test` never touch it.

## Rust CLI (repo root)

- Build: `cargo build --release` → `target/release/flp-extract-fxp`.
- Test: `cargo test` — runs unit tests plus `tests/integration.rs`, which
  spawns the actual compiled binary via `CARGO_BIN_EXE_flp-extract-fxp`
  (cargo builds it automatically first; no separate build step needed).
- Edition 2024 (Cargo.toml) → requires a recent stable Rust toolchain
  (1.85+). No `rustfmt.toml`/`clippy.toml` in the repo; there's also no CI
  job running `cargo test`/`clippy`/`fmt` (the only workflow is
  `.github/workflows/pages.yml`, which just builds the wasm+frontend and
  deploys). Run `cargo test` yourself before considering work done.
- Only dependencies are `clap`, `flate2` and `md-5` (native); `wasm-bindgen`
  is a target-specific dep for `wasm32-unknown-unknown` only, `ruzstd` is a
  dev-dependency (test-only zstd decoding). Keep new dependencies wasm32-safe
  — the conversion stack must compile identically for both targets.
- CLI subcommands: `list`, `extract`, `validate`, and
  `convert <input.flp> [--out <path>] [--dry-run]` (rewrites Serum instances
  inside an FLP as Serum2 instances; see `docs/flp-conversion.md`).

## Frontend + wasm (`front/`)

- **Critical, non-obvious**: `front/src/lib/wasm.ts` imports
  `../../pkg/flp_extract_fxp.js`. That `front/pkg/` directory is
  **gitignored and not checked in** — it must be generated with
  `wasm-pack` before `pnpm dev`/`pnpm build` will even typecheck:
  ```sh
  wasm-pack build --target web --out-dir front/pkg --out-name flp_extract_fxp .
  ```
  Run this from the **repo root** (not `front/`), matching
  `.github/workflows/pages.yml`. Requires the `wasm32-unknown-unknown`
  target and `wasm-pack` installed.
- Package manager is **pnpm** (`front/pnpm-lock.yaml`, lockfile v9). CI uses
  `pnpm/action-setup@v4` with version `12` and Node 22.
- From `front/`: `pnpm install`, `pnpm dev`, `pnpm build` (= `tsc -b && vite
  build`, needs `front/pkg/` to exist first), `pnpm preview`. There is no
  separate format script — `pnpm lint` runs `oxlint --fix` and `oxfmt`
  concurrently (auto-fixing lint issues and formatting in one command; not
  eslint/prettier). Type-aware lint rules are on (`oxlint-tsgolint`, see
  `front/.oxlintrc.json`); formatter config is `front/.oxfmtrc.json`.
- `vite.config.ts`'s `base` comes from `process.env.VITE_BASE`, which is
  **unset locally** (so `pnpm dev`/`pnpm build` serve from `/`); only
  `.github/workflows/pages.yml` sets `VITE_BASE=/flp-extract-fxp/` for the
  GitHub Pages build. It also excludes `flp_extract_fxp` from
  `optimizeDeps` (the wasm module must not be pre-bundled by Vite) and sets
  `server.fs.allow` (workspace root + `..`) so `pnpm dev` can import the
  generated `../pkg` package — dev-only, no effect on the Pages build.
- UI is shadcn/radix components already generated under
  `front/src/components/ui/` — reuse them rather than re-adding via the
  `shadcn` CLI.

## Format/domain reference docs

Before touching FLP/Serum parsing, `.fxp` construction, or the conversion
stack (`src/flp.rs`, `src/serum.rs`, `src/fxp.rs`, `src/s1state.rs`,
`src/importer.rs`, `src/s2tree.rs`, `src/serum2state.rs`, `src/flpconv.rs`),
read the relevant doc — they are the verified source of truth (static
reverse-engineering + real-file calibration + dynamic Serum2 verification),
not just design notes:
- `docs/serum-fxp-format.md` — byte-level Serum `.fxp` spec.
- `docs/serum2-importer-analysis.md` — Serum2's import validation rules.
- `docs/s1-to-s2-mapping.md` + `docs/s2-runtime-tables.md` — the real Serum
  → Serum2 importer (`s1state_load`, RVA 0x4DABC0) and its runtime-dumped
  conversion tables (baked into `src/s2tables.rs`).
- `docs/flp-serum2-conversion.md` — FLP event-213 byte-level rules for Serum
  vs Serum2 instances (the rewrite recipe).
- `docs/flp-conversion.md` — the shipped FLP conversion feature (pipeline,
  surfaces, verification, limitations).
- `docs/serum2-dynamic-verification.md` — live VST3-host verification. Read
  the CORRECTION section at the top first: `setState` **rejects** Serum
  data (the old "dynamically verified acceptance" conclusion was a false
  positive); the real import path is `s1state_load`.

Key constraints the code encodes (don't "fix" these without re-checking the
docs above):
- fxp header fields are **big-endian**; `byteSize` (offset 0x04) is the
  literal total file length, not the Steinberg-spec `fileLen − 8`.
- Serum preset state is 172,736 bytes for modern presets; legacy (2015-era)
  presets carry 21,808 / 28,232-byte blobs and are zero-padded to 172,736 at
  parse time (`src/s1state.rs::parse_preset`) — the same thing the real
  Serum2 importer does. Embedded metadata lives at fixed offsets (name
  0x4972, version f32 0x4994, author 0x49A0, category 0x49D0).
- Serum2 plugin instances are intentionally never extracted (they use an
  `XferJson`-prefixed state, not the Serum chunk layout) — only counted.
- Zip-packed FLPs (`PK`-prefixed "loop package" exports) are unsupported by
  design; the FLP must be extracted first.
- `src/importer.rs` correctness is proven by **byte-identity tests** against
  golden states produced by the REAL importer (called at runtime) — modern
  presets (`golden_byte_identical_*`) and legacy presets
  (`legacy_golden_fl_*`, trees in `tests/fixtures/legacy/`). Do not
  "simplify" importer logic without re-running those tests.
- Legacy (pre-0.162) presets need version-gated migrations that are no-ops
  for modern blobs (dest restamps, classic LFO regions, aux-region shifts,
  the pre-reorder distortion menu, legacy `[frames][tuning][noise]` stream
  order, per-env enable flags). Known gap: `RoutingSlot4.kParamRoutingDest`
  for `FL_FMItUp`/`FL_BASS_Adventure` (see docs/flp-conversion.md
  limitation (a)) — don't "fix" it by guessing a value.
- Converted processor states use **raw-block zstd frames** (uncompressed) on
  purpose — plugin-accepted; the goldens use libzstd level 3, both are
  standard frames. Smaller frames are future work, not a bug to fix.

## Tests

The real-preset fixtures and golden files are **untracked verification data**
(third-party preset content; see `docs/flp-conversion.md` → "Untracked
verification artifacts"). They live on the working machine only; every test
that needs one skips silently when the file is absent, so a fresh clone (and
CI) passes `cargo test` without them:

- `tests/fixtures/extracted_serum1.fxp` — real extracted fixture used by
  `validates_real_fixture`; pins real-world validation behavior.
- `tests/fixtures/serina1/*.fxp` (5 real Serum presets) and
  `tests/fixtures/serina1.flp` (real project: 5 Serum + 1 Serum2 instance)
  drive the converter tests.
- `tests/fixtures/golden_s2/0N_processor_state.bin` — golden converted
  processor states produced by the REAL importer (called at runtime) and
  accepted by the real plugin — ground truth for `golden_byte_identical_*`;
  don't regenerate/edit them casually.
- `tests/fixtures/legacy/*.fxp` + `*_importer_tree.cbor` — 6 legacy
  (2015-era) presets and the REAL importer's converted trees; ground truth
  for `legacy_golden_fl_*` (4/6 byte-identical, 2 with the documented
  RoutingSlot4 gap). Regenerate via the s1state_load harness (see
  docs/flp-conversion.md §Verification).
- `assets/` is gitignored and not present in the repo (used locally to hold
  real-world Serum fxp samples during format research) — don't expect it
  to exist or add tests that depend on it.

# Serum 1 parameter table & state-blob layouts (172,736-byte preset state)

**Purpose:** definitive, implementation-ready reference for reading a Serum 1.x
preset state blob (the 172,736-byte stream-0 payload of a `.fxp`) into named,
physically-interpreted values. Companion machine-readable file:
`C:/Users/cabbage\\AppData\\Local\\Temp/opencode/conv_work/s1params/s1_params.json`
(not part of this repo; the tables below are the canonical copy).

**Primary source:** `btesser/serum2vital` (plugin-verified Serum 1 -> Vital
converter) — `serum_params.py` (the 299-name table from Serum's own
`SYParameters` listing for build 1.334, with name corrections verified against
the plugin), `serum_tables.py` (unit curves + menus read from the plugin's
display), `serum1.py` + `docs/FORMATS.md` (binary layout, plugin-verified with
single-change fixture presets and rendered probes).

**Local cross-check (2026-09-11):** every structural claim below was re-verified
on ten 172,736-byte state blobs — the five FL-state presets in
`conv_work/fxps` (stream 0 extracted per `docs/serum-fxp-format.md`) plus the
five reference blobs in `refs/serum_fxp/*.fxp.s0.bin`. Confidence markers:
**verified** (plugin read-out / fixture / byte-verified on real blobs) vs
**inferred** (calibrated approximation, plausible reading).

---

## 1. Stored-value conventions

* All 299 parameters are **float32 LE**, normalised to **0..1** (`n` below).
  Parser should clamp to 0..1 and map NaN -> 0 (serum2vital's reader does).
* **Display value** = `display_min + (display_max − display_min) × n`.
  The `display_min/max/unit/default` per parameter come from Serum's own
  parameter listing; the *display* value is what the GUI shows.
* On top of the linear display law, most knobs have a **unit curve** (§4) that
  maps the stored value (or the display value) to the physical quantity.
* Locations (byte offsets in the decompressed blob):
  * params **0..227** at `0x3460 + 4*index`
  * params **228..298** at `0x4AE0 + 4*(index−228)`
* The second block (228..298) is absent or partly uninitialised in presets
  written by older builds; §9 gives the size rules for falling back to the
  documented defaults.

## 2. State-blob region map (172,736 = 0x2A300 bytes, modern builds)

| Region | Contents |
|--------|----------|
| `0x0000`..`0x0280` | mod slots 1–16, 16 records × 40 B (§5) |
| `0x0280`..`0x3460` | classic-layout LFO region — **all zero in modern blobs** (its emptiness identifies the "new" layout) |
| `0x3460`..`0x37F0` | parameters 0–227 (228 × f32) |
| `0x37F0`..`0x3BE0` | per-effect record: byte mirrors of each effect's knobs/mode/enable (dist mode/enable `0x396C`/`0x396E`, delay `0x3A7C`/`0x3A7E`, reverb `0x3B04`/`0x3B06`, hyper unison/enable/retrig `0x3BD0`/`0x3BD2`/`0x3BD9`); the only byte not duplicated by a parameter is the reverb Plate/Hall switch at `0x3B04` (1 = Hall, default) |
| `0x3BE0`..`0x3C08` | FX rack order, 10 × int32 LE (§8) |
| `0x3C08`, `0x3E08`, `0x4008` | wavetable name A / B / noise sample name (512 B NUL-terminated each) |
| `0x4972` | preset name (32 B) |
| `0x4994` | f32 build marker: `0.1631` (1.334-era) / `0.1621` (earlier) |
| `0x49A0` | author (48 B) |
| `0x49D0` | menu / bank (48 B) |
| `0x4A58` | 8-byte per-preset id (e.g. `NDDEKICF`); preserve verbatim |
| `0x4A60`..`0x4AE0` | macro 1–4 names, 4 × 32 B |
| `0x4AE0`..`0x4BFC` | parameters 228–298 (71 × f32) |
| `0x4BFC`..`0x4C48` | zero padding (76 B, verified on all 10 blobs) |
| `0x4C48`..`0x4CAC` | global switches block, 100 B of f32 fields (§7) |
| `0x4CAC`..`0x50E0` | zero padding (1076 B, verified) |
| `0x50E0`..`0x5360` | **mod slots 17–32, 16 records × 40 B (fixed offset, §5)** |
| `0x5360`..`0x84D8` | zero padding with occasional heap-junk fragments |
| `0x84D8`..`0x1EE18` | LFO 1–8 blocks, 8 × 0x2D28 (§6) |
| `0x1EE18`..`0x2A2B8` | blocks 9–12 (warp remap graphs, same stride; not LFOs) |
| `0x2A2B8`..`0x2A300` | zero tail (in most presets; a few carry a 32-byte session blob) |

Roughly `0x2A270`..`0x2A2A8` usually holds 0.0/1.0 float64 pairs ending with a
u32 LE `1` (verified); in one of the ten blobs this tail area is heap junk, so
treat it as unstructured.

## 3. The 299 parameters

Columns: **i** parameter index (VST parameter order; the mod-matrix destination
uses this index), **name** (Serum's own; `Amp.` and `Mast.Tun` carry degenerate
display ranges in the listing), **display range** what the GUI shows, **stored
default** = normalized factory default, **transform** the normalized-to-physical
law (formulas in §4), **conf**idence.

| 0 | MasterVol | 0..100 % | 0.7 | amplitude = n^3; dB = 60*log10(n); 100% = 0 dB, 70% = -9.3 dB | verified |
| 1 | A Vol | 0..100 % | 0.75 | display = dmin + (dmax-dmin)*n; display % 0..100; amplitude = n^2 | verified |
| 2 | A Pan | -50..50 | 0.5 | display = dmin + (dmax-dmin)*n; display -50..50; physical pan = display/50 (-1..1) | verified |
| 3 | A Octave | -4..4 Oct | 0.5 | display = dmin + (dmax-dmin)*n; display -4..4, step 1 (stored multiples of 1/8); transpose_oct = round(display) | verified |
| 4 | A Semi | -12..12 semitones | 0.5 | display = dmin + (dmax-dmin)*n; display -12..12, step 1 (stored multiples of 1/24); transpose_st = round(display) | verified |
| 5 | A Fine | -100..100 cents | 0.5 | display = dmin + (dmax-dmin)*n; display -100..100 cents, continuous | verified |
| 6 | A Unison | 0..16 | 0 | display = dmin + (dmax-dmin)*n; display 0..16, integer voices = round(display) | verified |
| 7 | A UniDet | 0..1 | 0.5 | display = dmin + (dmax-dmin)*n; display 0..1; detune spread responds quadratically (n^2) over the global unison range (default 2 st); full-scale spread = 2x unison range | verified |
| 8 | A UniBlend | 0..100 | 0.75 | display = dmin + (dmax-dmin)*n; display 0..100 | verified |
| 9 | A Warp | 0..1 | 0 | amount = n (0..1); meaning depends on the warp mode of the same oscillator (WarpOscA/B index 168/169) | verified |
| 10 | A CoarsePit | 0..1 | 0.5 | semitones = (n-0.5)*128, integer (measured; +-64 st full scale) | verified |
| 11 | A WTPos | 1..256 | 0 | frame_index = 1 + 255*n (display 1..256; continuous, fractional = interpolated frames) | verified |
| 12 | A RandPhase | 0..100 % | 1 | display = dmin + (dmax-dmin)*n; display 0..100 (random-phase probability) | verified |
| 13 | A Phase | 0..1 ° | 0.5 | phase = n (0..1 of a cycle, 0..360 deg) | verified |
| 14 | B Vol | 0..100 % | 0.75 | display = dmin + (dmax-dmin)*n; display % 0..100; amplitude = n^2 | verified |
| 15 | B Pan | -50..50 | 0.5 | display = dmin + (dmax-dmin)*n; display -50..50; physical pan = display/50 (-1..1) | verified |
| 16 | B Octave | -4..4 Oct | 0.5 | display = dmin + (dmax-dmin)*n; display -4..4, step 1 (stored multiples of 1/8); transpose_oct = round(display) | verified |
| 17 | B Semi | -12..12 semitones | 0.5 | display = dmin + (dmax-dmin)*n; display -12..12, step 1 (stored multiples of 1/24); transpose_st = round(display) | verified |
| 18 | B Fine | -100..100 cents | 0.5 | display = dmin + (dmax-dmin)*n; display -100..100 cents, continuous | verified |
| 19 | B Unison | 0..16 | 0 | display = dmin + (dmax-dmin)*n; display 0..16, integer voices = round(display) | verified |
| 20 | B UniDet | 0..1 | 0.5 | display = dmin + (dmax-dmin)*n; display 0..1; detune spread responds quadratically (n^2) over the global unison range (default 2 st); full-scale spread = 2x unison range | verified |
| 21 | B UniBlend | 0..100 | 0.75 | display = dmin + (dmax-dmin)*n; display 0..100 | verified |
| 22 | B Warp | 0..1 | 0 | amount = n (0..1); meaning depends on the warp mode of the same oscillator (WarpOscA/B index 168/169) | verified |
| 23 | B CoarsePit | 0..1 | 0.5 | semitones = (n-0.5)*128, integer (measured; +-64 st full scale) | verified |
| 24 | B WTPos | 1..256 | 0 | frame_index = 1 + 255*n (display 1..256; continuous, fractional = interpolated frames) | verified |
| 25 | B RandPhase | 0..100 % | 1 | display = dmin + (dmax-dmin)*n; display 0..100 (random-phase probability) | verified |
| 26 | B Phase | 0..1 ° | 0.5 | phase = n (0..1 of a cycle, 0..360 deg) | verified |
| 27 | Noise Level | 0..100 % | 0.25 | display = dmin + (dmax-dmin)*n; display % 0..100; amplitude = n^2 | verified |
| 28 | Noise Pitch | 0..100 % | 0.5 | n>=0.5: semitones = 96*(n-0.5) (+48 st at 1.0); n<0.5: semitones = 100*log2(2n) (playback-rate collapse; -48 st floor reached below n=0.36) | verified |
| 29 | Noise Fine | -1..1 % | 0.5 | display = dmin + (dmax-dmin)*n; display -1..1 (%) | verified |
| 30 | Noise Pan | -50..50 | 0.5 | display = dmin + (dmax-dmin)*n; display -50..50; physical pan = display/50 (-1..1) | verified |
| 31 | Noise RandPhase | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; display 0..100 (random-phase probability) | verified |
| 32 | Noise Phase | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; display 0..100 | verified |
| 33 | Sub Osc Level | 0..100 % | 0.75 | display = dmin + (dmax-dmin)*n; display % 0..100; sub level is LINEAR in amplitude (unlike A/B Vol which are n^2) | verified |
| 34 | Sub Osc Pan | -50..50 % | 0.5 | display = dmin + (dmax-dmin)*n; display -50..50 % pan | verified |
| 35 | Env1 Atk | 0..1 ms | 0.11 | seconds = 32*n^5 (0.5 -> 1.000 s, 0.25 -> 0.031 s, 0.75 -> 7.59 s; full scale 32 s) | verified |
| 36 | Env1 Hold | 0..1 ms | 0 | seconds = 32*n^5 (0.5 -> 1.000 s, 0.25 -> 0.031 s, 0.75 -> 7.59 s; full scale 32 s) | verified |
| 37 | Env1 Dec | 0..1 s | 0.5 | seconds = 32*n^5 (0.5 -> 1.000 s, 0.25 -> 0.031 s, 0.75 -> 7.59 s; full scale 32 s) | verified |
| 38 | Env1 Sus | 0..1 dB | 1 | n used directly; sustain is quadratic in amplitude (n^2), matching Vital's squared amplitude envelope | verified |
| 39 | Env1 Rel | 0..1 ms | 0.215 | seconds = 32*n^5 (0.5 -> 1.000 s, 0.25 -> 0.031 s, 0.75 -> 7.59 s; full scale 32 s) | verified |
| 40 | OscA>Fil | 0..1 | 1 | display = dmin + (dmax-dmin)*n; 0/1: route into filter (>0.5) vs direct to FX | verified |
| 41 | OscB>Fil | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: route into filter (>0.5) vs direct to FX | verified |
| 42 | OscN>Fil | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: route into filter (>0.5) vs direct to FX | verified |
| 43 | OscS>Fil | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: route into filter (>0.5) vs direct to FX | verified |
| 44 | Fil Type | 0..1 | 0.01136 | index = round(n*div) for the first div in [95, 89, 88] that lands within 0.02 of an integer [enum list: MG Low 6 .. MG Low 12 .. Scream BP (96 entries)] | verified |
| 45 | Fil Cutoff | 0..1 Hz | 0.5 | Hz = 8 * (22050/8)^n (log-linear; equivalent to MIDI notes -49.2..135: note = 69 + 12*log2(Hz/440)) | verified |
| 46 | Fil Reso | 0..100 % | 0.1 | display = dmin + (dmax-dmin)*n; resonance % | verified |
| 47 | Fil Driv | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; drive %; plain gain into the filter (+7.6 dB at 25%, +16 dB at 100%, measured) | verified |
| 48 | Fil Var | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; VAR: morphs the filter response per type (LP/BP/HP blend on morphing types, formant X/Y, etc.) | verified |
| 49 | Fil Mix | 0..100 % | 1 | display = dmin + (dmax-dmin)*n; filter mix | verified |
| 50 | Fil Stereo | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; stereo spread | verified |
| 51 | Env2 Atk | 0..1 ms | 0.11 | seconds = 32*n^5 | verified |
| 52 | Env2 Hld | 0..1 ms | 0 | seconds = 32*n^5 | verified |
| 53 | Env2 Dec | 0..1 s | 0.5 | seconds = 32*n^5 | verified |
| 54 | Env2 Sus | 0..100 % | 1 | display = dmin + (dmax-dmin)*n; sustain % (linear, unlike Env1 Sus) | verified |
| 55 | Env2 Rel | 0..1 ms | 0.215 | seconds = 32*n^5 | verified |
| 56 | Env3 Atk | 0..1 ms | 0.11 | seconds = 32*n^5 | verified |
| 57 | Env3 Hld | 0..1 ms | 0 | seconds = 32*n^5 | verified |
| 58 | Env3 Dec | 0..1 s | 0.5 | seconds = 32*n^5 | verified |
| 59 | Env3 Sus | 0..100 % | 1 | display = dmin + (dmax-dmin)*n; sustain % (linear, unlike Env1 Sus) | verified |
| 60 | Env3 Rel | 0..1 ms | 0.215 | seconds = 32*n^5 | verified |
| 61 | LFO1Rate | 0..1 | 0.5 | step = round(n*228); BPM mode (flag off): 229-step division ladder LFO_BPM_RUNS (16 steps/octave, 32 bar at step 0, 1/4 at step 112-127 i.e. n=0.5); Hz mode (flag on): Hz = 100*n^4 | verified |
| 62 | LFO2Rate | 0..1 | 0.5 | step = round(n*228); BPM mode (flag off): 229-step division ladder LFO_BPM_RUNS (16 steps/octave, 32 bar at step 0, 1/4 at step 112-127 i.e. n=0.5); Hz mode (flag on): Hz = 100*n^4 | verified |
| 63 | LFO3Rate | 0..1 | 0.5 | step = round(n*228); BPM mode (flag off): 229-step division ladder LFO_BPM_RUNS (16 steps/octave, 32 bar at step 0, 1/4 at step 112-127 i.e. n=0.5); Hz mode (flag on): Hz = 100*n^4 | verified |
| 64 | LFO4Rate | 0..1 | 0.5 | step = round(n*228); BPM mode (flag off): 229-step division ladder LFO_BPM_RUNS (16 steps/octave, 32 bar at step 0, 1/4 at step 112-127 i.e. n=0.5); Hz mode (flag on): Hz = 100*n^4 | verified |
| 65 | PortTime | 0..1 ms | 0 | seconds = 8*n^5 | verified |
| 66 | PortCurve | -100..100 % | 0.5 | display = dmin + (dmax-dmin)*n; display -100..100 (portamento curve shape) | verified |
| 67 | Chaos1 BPM | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: chaos 1 rate follows the BPM ladder instead of Hz | verified |
| 68 | Chaos2 BPM | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: chaos 2 BPM flag | verified |
| 69 | Chaos1 Rate | 0..1 | 0.2512 | unsynced: Hz = 1000*n^5 (quintic); when Chaos BPM flag set: 229-step division ladder like LFO rates | verified |
| 70 | Chaos2 Rate | 0..1 | 0.2512 | unsynced: Hz = 1000*n^5 (quintic); when Chaos BPM flag set: 229-step division ladder like LFO rates | verified |
| 71 | A curve1 | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; display 0..100 %, 50% = straight segment; >50% bends one way, <50% the other (Vital power approx -17.5*(d/100-0.5) + stage offset) | verified |
| 72 | D curve1 | 0..100 % | 0.67 | display = dmin + (dmax-dmin)*n; display 0..100 %, 50% = straight segment; >50% bends one way, <50% the other (Vital power approx -17.5*(d/100-0.5) + stage offset) | verified |
| 73 | R curve1 | 0..100 % | 0.67 | display = dmin + (dmax-dmin)*n; display 0..100 %, 50% = straight segment; >50% bends one way, <50% the other (Vital power approx -17.5*(d/100-0.5) + stage offset) | verified |
| 74 | A curve2 | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; display 0..100 %, 50% = straight segment; >50% bends one way, <50% the other (Vital power approx -17.5*(d/100-0.5) + stage offset) | verified |
| 75 | D curve2 | 0..100 % | 0.67 | display = dmin + (dmax-dmin)*n; display 0..100 %, 50% = straight segment; >50% bends one way, <50% the other (Vital power approx -17.5*(d/100-0.5) + stage offset) | verified |
| 76 | R curve2 | 0..100 % | 0.67 | display = dmin + (dmax-dmin)*n; display 0..100 %, 50% = straight segment; >50% bends one way, <50% the other (Vital power approx -17.5*(d/100-0.5) + stage offset) | verified |
| 77 | A curve3 | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; display 0..100 %, 50% = straight segment; >50% bends one way, <50% the other (Vital power approx -17.5*(d/100-0.5) + stage offset) | verified |
| 78 | D curve3 | 0..100 % | 0.67 | display = dmin + (dmax-dmin)*n; display 0..100 %, 50% = straight segment; >50% bends one way, <50% the other (Vital power approx -17.5*(d/100-0.5) + stage offset) | verified |
| 79 | R curve3 | 0..100 % | 0.67 | display = dmin + (dmax-dmin)*n; display 0..100 %, 50% = straight segment; >50% bends one way, <50% the other (Vital power approx -17.5*(d/100-0.5) + stage offset) | verified |
| 80 | Mast.Tun | degenerate (0..0) | 0.5 | semitones = (n-0.5)*128 (measured at two points: +-0.1 -> +-12.8 st) | verified |
| 81 | Verb Wet | 0..100 % | 0.33 | display = dmin + (dmax-dmin)*n; wet % (wet-path gain law sin^2(pi*w/2) measured) | verified |
| 82 | VerbSize | 0..100 % | 0.33 | display = dmin + (dmax-dmin)*n; size % (RT60 floor: 0.9 s at 20%, 1.8 at 50%, 5.3 at 80%) | verified |
| 83 | VerbDecay | 0.8..12 s | 0.12 | seconds = 0.8 + 11.2*n (0.8..12 s linear; plugin name DECAY - the gist mislabels it pre-delay) | verified |
| 84 | VerbLoCt | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; LO CUT % (pre-low-pass opens as 30+60*l on the Vital-mapped scale) | verified |
| 85 | VerbSpinRate | 0..100 % | 0.25 | display = dmin + (dmax-dmin)*n; SPIN RATE % (rate knob 20*n^4 Hz when rendered; gist mislabels it 'damp') | verified |
| 86 | VerbHiCt | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; HI CUT % (128-35*h on the Vital-mapped scale) | verified |
| 87 | VerbSpinDepth | 0..100 % | 0.2 | display = dmin + (dmax-dmin)*n; SPIN DEPTH % (gist mislabels it 'width') | verified |
| 88 | EQ FrqL | 0..1 Hz | 0.333 | Hz = 22 * (20000/22)^n | verified |
| 89 | EQ FrqH | 0..1 Hz | 0.666 | Hz = 22 * (20000/22)^n | verified |
| 90 | EQ Q L | 0..100 % | 0.6 | display = dmin + (dmax-dmin)*n; low Q | verified |
| 91 | EQ Q H | 0..100 % | 0.6 | display = dmin + (dmax-dmin)*n; high Q | verified |
| 92 | EQ VolL | -24..24 dB | 0.5 | display = dmin + (dmax-dmin)*n; low gain -24..+24 dB linear | verified |
| 93 | EQ VolH | -24..24 dB | 0.5 | display = dmin + (dmax-dmin)*n; high gain -24..+24 dB linear | verified |
| 94 | EQ TypL | 0..1 | 0 | index = round(n*div) for the first div in [2] that lands within 0.02 of an integer [enum list: Shelf / Peak / LPF] | verified |
| 95 | EQ TypH | 0..1 | 0 | index = round(n*div) for the first div in [2] that lands within 0.02 of an integer [enum list: Shelf / Peak / LPF] | verified |
| 96 | Dist_Wet | 0..100 % | 1 | display = dmin + (dmax-dmin)*n; wet % (gain law sin^2(pi*w/2) measured) | verified |
| 97 | Dist_Drv | 0..100 % | 0.25 | display = dmin + (dmax-dmin)*n; drive % (per-mode level law; every mode sits -6 dB at zero drive) | verified |
| 98 | Dist_L/B/H | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; L/B/H blend 0..100 | verified |
| 99 | Dist_Mode | 0..1 | 0 | index = round(n*div) for the first div in [15, 12] that lands within 0.02 of an integer [enum list: Tube .. SoftClip .. Tape Sat. (16 entries)] | verified |
| 100 | Dist_Freq | 0..1 Hz | 0.5 | Hz = 8 * (13290/8)^n (pre/post filter frequency) | verified |
| 101 | Dist_BW | 0..1 | 0.5 | display = dmin + (dmax-dmin)*n; filter bandwidth 0..1 | verified |
| 102 | Dist_PrePost | 0..1 | 0 | index = round(n*div) for the first div in [1] that lands within 0.02 of an integer [enum list: Pre / Post] | verified |
| 103 | Flg_Wet | 0..100 % | 1 | display = dmin + (dmax-dmin)*n; wet % | verified |
| 104 | Flg_BPM_Sync | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: BPM-synced rate | verified |
| 105 | Flg_Rate | 0..1 Hz | 0.25 | unsynced: Hz = 20*n^4; synced: 229-step 31-entry ladder FX_RATE_RUNS (Off, then plain/dotted/triplet 24 bar .. 1/32) | verified |
| 106 | Flg_Dep | 0..100 % | 1 | display = dmin + (dmax-dmin)*n; depth % | verified |
| 107 | Flg_Feed | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; feedback % (Vital map: 2*w-1 bipolar) | verified |
| 108 | Flg_Stereo | 0..360 ° | 0.5 | display = dmin + (dmax-dmin)*n; phase offset in degrees; 180 = 0.1 Vital phase offset | verified |
| 109 | Phs_Wet | 0..100 % | 1 | display = dmin + (dmax-dmin)*n; wet % | verified |
| 110 | Phs_BPM_Sync | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: BPM-synced rate | verified |
| 111 | Phs_Rate | 0..1 Hz | 0.25 | unsynced: Hz = 20*n^4; synced: FX_RATE_RUNS ladder | verified |
| 112 | Phs_Dpth | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; depth % (mod depth 48 semitones*w measured) | verified |
| 113 | Phs_Frq | 0..1 Hz | 0.5 | Hz = 20 * (18000/20)^n (notch centre) | verified |
| 114 | Phs_Feed | 0..100 % | 0.8 | display = dmin + (dmax-dmin)*n; feedback % | verified |
| 115 | Phs_Stereo | 0..360 ° | 0.5 | display = dmin + (dmax-dmin)*n; degrees; 180 = 0.02 Vital phase offset | verified |
| 116 | Cho_Wet | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; wet % | verified |
| 117 | Cho_BPM_Sync | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: BPM-synced rate | verified |
| 118 | Cho_Rate | 0..1 Hz | 0.25 | unsynced: Hz = 20*n^4; synced: FX_RATE_RUNS ladder | verified |
| 119 | Cho_Dly | 0..1 ms | 0.5 | 0..1; delay seconds ~= 0.02*n^2 (20 ms full scale, calibrated) | inferred |
| 120 | Cho_Dly2 | 0..1 ms | 0 | 0..1; delay seconds ~= 0.02*n^2 (delay 2) | inferred |
| 121 | Cho_Dep | 0..1 ms | 1 | 0..1; mod depth = n^2 (calibrated) | inferred |
| 122 | Cho_Feed | 0..1 % | 0.1 | 0..1; feedback = 0.95*n (calibrated); gist unit '%' is wrong | inferred |
| 123 | Cho_Filt | 0..1 Hz | 0.5 | Hz = 50 * (20000/50)^n (wet-path low-pass; 1 kHz default -> n ~= 0.576) | verified |
| 124 | Dly_Wet | 0..100 % | 0.3 | display = dmin + (dmax-dmin)*n; wet % | verified |
| 125 | Dly_Freq | 0..1 Hz | 0.5 | Hz = 40 * (18000/40)^n (delay filter cutoff) | verified |
| 126 | Dly_BW | 0.8..8.2 | 0.8108 | display = dmin + (dmax-dmin)*n; display 0.8..8.2; filter spread = (d-0.8)/7.4 | verified |
| 127 | Dly_BPM_Sync | 0..1 | 1 | display = dmin + (dmax-dmin)*n; 0/1: BPM-synced time (default 1 = synced) | verified |
| 128 | Dly_Link | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: link L/R time | verified |
| 129 | Dly_TimL | 0..1 | 0.625 | synced (flag 127=1): step = round(n*228) into DELAY_SYNC_RUNS ladder (fast..4 bar); unsynced: ms = 1 + 500*n^4 (1..501 ms) | verified |
| 130 | Dly_TimR | 0..1 | 0.625 | right channel, same law as 129 | verified |
| 131 | Dly_Mode | 0..1 | 0 | index = round(n*div) for the first div in [2] that lands within 0.02 of an integer [enum list: Normal / Ping-Pong / Tap->Delay] | verified |
| 132 | Dly_Feed | 0..100 % | 0.4 | display = dmin + (dmax-dmin)*n; feedback % | verified |
| 133 | Dly_Off L | 0..1 | 0.5 | multiplier = 0.5 + n (0.5x..1.5x; 'Dot 1/2' = 0.75, 'Dot' = 1.5) | verified |
| 134 | Dly_Off R | 0..1 | 0.5 | right channel offset multiplier = 0.5 + n | verified |
| 135 | Cmp_Thr | 0..1 dB | 0.5 | dB via piecewise-linear read-out table (0.5 -> -19.7 dB, 0.25 -> -7.2 dB, 1.0 -> -120 dB); see COMP_THRESHOLD anchors in this file | verified |
| 136 | Cmp_Rat | 0..1 | 0.75 | stored = 1 - 1/ratio (2:1 -> 0.478, 4:1 -> 0.75, 10:1 -> 0.9, Limit -> 1.0); 229-step read-out ladder COMP_RATIO_RUNS | verified |
| 137 | Cmp_Att | 0..1 ms | 0.3 | ms = max(0.1, 1000*n^2) (quadratic; default 0.3 -> 90 ms) | verified |
| 138 | Cmp_Rel | 0..1 ms | 0.3 | ms = max(0.1, 1000*n^2) | verified |
| 139 | CmpGain | 0..1 dB | 0 | dB = 20*log10(1 + 31*n^2) (0.1 -> 2.3, 0.5 -> 18.8, 1.0 -> 30.1 dB) | verified |
| 140 | CmpMBnd | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: multiband mode | verified |
| 141 | FX Fil Wet | 0..100 % | 1 | display = dmin + (dmax-dmin)*n; wet % | verified |
| 142 | FX Fil Type | 0..1 | 0 | index = round(n*div) for the first div in [95, 89, 88] that lands within 0.02 of an integer [enum list: MG Low 6 .. MG Low 12 .. Scream BP (96 entries)] | verified |
| 143 | FX Fil Freq | 0..1 Hz | 0.5 | Hz = 8 * (22050/8)^n | verified |
| 144 | FX Fil Reso | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; resonance | verified |
| 145 | FX Fil Drive | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; drive | verified |
| 146 | FX Fil Var | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; VAR | verified |
| 147 | Hyp_Wet | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; wet % | verified |
| 148 | Hyp_Rate | 0..1 % | 0.4 | Hz = 20*n^4 | verified |
| 149 | Hyp_Detune | 0..100 % | 0.25 | display = dmin + (dmax-dmin)*n; detune % | verified |
| 150 | Hyp_Unison | 0..1 | 0.5714 | index = round(n*div) for the first div in [7] that lands within 0.02 of an integer [enum list: 1 voices / 2 voices / 3 voices / 4 voices / 5 voices / 6 voices / 7 voices / 8 voices] | verified |
| 151 | Hyp_Retrig | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: retrigger per note | verified |
| 152 | HypDim_Size | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; Dimension size % | verified |
| 153 | HypDim_Mix | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; Dimension mix % | verified |
| 154 | Dist Enable | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: effect enabled | verified |
| 155 | Flg Enable | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: effect enabled | verified |
| 156 | Phs Enable | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: effect enabled | verified |
| 157 | Cho Enable | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: effect enabled | verified |
| 158 | Dly Enable | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: effect enabled | verified |
| 159 | Comp Enable | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: effect enabled | verified |
| 160 | Rev Enable | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: effect enabled | verified |
| 161 | EQ Enable | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: effect enabled | verified |
| 162 | FX Fil Enable | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: effect enabled | verified |
| 163 | Hyp Enable | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: effect enabled | verified |
| 164 | OscAPitchTrack | 0..1 | 1 | display = dmin + (dmax-dmin)*n; 0/1: pitch tracking | verified |
| 165 | OscBPitchTrack | 0..1 | 1 | display = dmin + (dmax-dmin)*n; 0/1: pitch tracking | verified |
| 166 | Bend U | -24..24 semitones | 0.5417 | display = dmin + (dmax-dmin)*n; display -24..+24 st linear (defaults +2/-2 = 0.5417/0.4583); step 1 (multiples of 1/48) | verified |
| 167 | Bend D | -24..24 semitones | 0.4583 | display = dmin + (dmax-dmin)*n; display -24..+24 st linear (defaults +2/-2 = 0.5417/0.4583); step 1 (multiples of 1/48) | verified |
| 168 | WarpOscA | 0..1 | 0 | index = round(n*div) for the first div in [23] that lands within 0.02 of an integer [enum list: Off .. Sync ..  (24 entries)] | verified |
| 169 | WarpOscB | 0..1 | 0 | index = round(n*div) for the first div in [23] that lands within 0.02 of an integer [enum list: Off .. Sync ..  (24 entries)] | verified |
| 170 | SubOscShape | 0..1 | 0 | index = round(n*div) for the first div in [4] that lands within 0.02 of an integer [enum list: Sine / RoundRect / Saw / Square / Pulse] | verified |
| 171 | SubOscOctave | -4..4 Oct | 0.5 | display = dmin + (dmax-dmin)*n; display -4..4, step 1 (stored multiples of 1/8); transpose_oct = round(display) | verified |
| 172 | A Uni LR | 0..100 | 1 | display = dmin + (dmax-dmin)*n; 0..100 (unison stereo spread) | verified |
| 173 | B Uni LR | 0..100 | 1 | display = dmin + (dmax-dmin)*n; 0..100 (unison stereo spread) | verified |
| 174 | A Uni Warp | -100..100 | 0.5 | display = dmin + (dmax-dmin)*n; display -100..100 (warp/frame spread) | verified |
| 175 | B Uni Warp | -100..100 | 0.5 | display = dmin + (dmax-dmin)*n; display -100..100 (warp/frame spread) | verified |
| 176 | A Uni WTPos | -100..100 | 0.5 | display = dmin + (dmax-dmin)*n; display -100..100 (warp/frame spread) | verified |
| 177 | B Uni WTPos | -100..100 | 0.5 | display = dmin + (dmax-dmin)*n; display -100..100 (warp/frame spread) | verified |
| 178 | A Uni Stack | 0..1 | 0 | index = round(n*div) for the first div in [8] that lands within 0.02 of an integer [enum list: off / 12 (1x) / 12 (2x) / 12 (3x) / 12+7(1x) / 12+7(2x) / 12+7(3x) / Center-12 / Center-24] | verified |
| 179 | B Uni Stack | 0..1 | 0 | index = round(n*div) for the first div in [8] that lands within 0.02 of an integer [enum list: off / 12 (1x) / 12 (2x) / 12 (3x) / 12+7(1x) / 12+7(2x) / 12+7(3x) / Center-12 / Center-24] | verified |
| 180 | Mod 1 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 181 | Mod 1 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 182 | Mod 2 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 183 | Mod 2 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 184 | Mod 3 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 185 | Mod 3 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 186 | Mod 4 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 187 | Mod 4 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 188 | Mod 5 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 189 | Mod 5 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 190 | Mod 6 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 191 | Mod 6 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 192 | Mod 7 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 193 | Mod 7 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 194 | Mod 8 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 195 | Mod 8 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 196 | Mod 9 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 197 | Mod 9 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 198 | Mod10 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 199 | Mod10 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 200 | Mod11 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 201 | Mod11 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 202 | Mod12 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 203 | Mod12 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 204 | Mod13 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 205 | Mod13 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 206 | Mod14 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 207 | Mod14 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 208 | Mod15 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 209 | Mod15 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 210 | Mod16 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 %; fraction of the destination parameter's full range | verified |
| 211 | Mod16 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler (1.0 = full) | verified |
| 212 | Osc A On | 0..1 | 1 | display = dmin + (dmax-dmin)*n; 0/1: module enabled | verified |
| 213 | Osc B On | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: module enabled | verified |
| 214 | Osc N On | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: module enabled | verified |
| 215 | Osc S On | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: module enabled | verified |
| 216 | Filter On | 0..1 | 0 | display = dmin + (dmax-dmin)*n; 0/1: module enabled | verified |
| 217 | Mod Wheel | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; mod-wheel value 0..100 % | verified |
| 218 | Macro 1 | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; macro value 0..100 % | verified |
| 219 | Macro 2 | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; macro value 0..100 % | verified |
| 220 | Macro 3 | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; macro value 0..100 % | verified |
| 221 | Macro 4 | 0..100 % | 0 | display = dmin + (dmax-dmin)*n; macro value 0..100 % | verified |
| 222 | Amp. | degenerate (0..0) | 0.5 | same cubic law as MasterVol (60*log10(n) dB); gist display range is degenerate (0..0) | inferred |
| 223 | LFO1 smooth | 0..100 | 0 | seconds ~= 0.5*n^6 (inert until the top: 8 ms at 50%, 0.5 s at 100%; measured by rendering) | verified |
| 224 | LFO2 smooth | 0..100 | 0 | seconds ~= 0.5*n^6 (inert until the top: 8 ms at 50%, 0.5 s at 100%; measured by rendering) | verified |
| 225 | LFO3 smooth | 0..100 | 0 | seconds ~= 0.5*n^6 (inert until the top: 8 ms at 50%, 0.5 s at 100%; measured by rendering) | verified |
| 226 | LFO4 smooth | 0..100 | 0 | seconds ~= 0.5*n^6 (inert until the top: 8 ms at 50%, 0.5 s at 100%; measured by rendering) | verified |
| 227 | Pitch Bend | 0..1 | 0.5 | 0..1 raw bend position at save time | verified |
| 228 | Mod17 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 229 | Mod17 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 230 | Mod18 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 231 | Mod18 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 232 | Mod19 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 233 | Mod19 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 234 | Mod20 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 235 | Mod20 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 236 | Mod21 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 237 | Mod21 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 238 | Mod22 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 239 | Mod22 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 240 | Mod23 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 241 | Mod23 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 242 | Mod24 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 243 | Mod24 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 244 | Mod25 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 245 | Mod25 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 246 | Mod26 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 247 | Mod26 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 248 | Mod27 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 249 | Mod27 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 250 | Mod28 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 251 | Mod28 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 252 | Mod29 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 253 | Mod29 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 254 | Mod30 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 255 | Mod30 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 256 | Mod31 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 257 | Mod31 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 258 | Mod32 amt | -100..100 | 0.5 | stored -1..1 = display -100..100 % | verified |
| 259 | Mod32 out | 0..100 | 1 | 0..1 = 0..100 % output-range scaler | verified |
| 260 | LFO5Rate | 0..1 | 0.5 | same law as LFO1-4 rates (61-64) | verified |
| 261 | LFO6Rate | 0..1 | 0.5 | same law as LFO1-4 rates (61-64) | verified |
| 262 | LFO7Rate | 0..1 | 0.5 | same law as LFO1-4 rates (61-64) | verified |
| 263 | LFO8Rate | 0..1 | 0.5 | same law as LFO1-4 rates (61-64) | verified |
| 264 | LFO5 smooth | 0..100 | 0 | seconds ~= 0.5*n^6 | verified |
| 265 | LFO6 smooth | 0..100 | 0 | seconds ~= 0.5*n^6 | verified |
| 266 | LFO7 smooth | 0..100 | 0 | seconds ~= 0.5*n^6 | verified |
| 267 | LFO8 smooth | 0..100 | 0 | seconds ~= 0.5*n^6 | verified |
| 268 | FXFil Pan | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; FX filter pan 0..100 | verified |
| 269 | Comp_Wet | 0..100 % | 0.5 | display = dmin + (dmax-dmin)*n; compressor mix % | verified |
| 270 | CompMB L | 0..200 % | 0.5 | display = dmin + (dmax-dmin)*n; display 0..200 %, 100% neutral; band gain dB ~= 33*log10(display/100) (multiband mode) | verified |
| 271 | CompMB M | 0..200 % | 0.5 | display = dmin + (dmax-dmin)*n; display 0..200 %, 100% neutral; band gain dB ~= 33*log10(display/100) (multiband mode) | verified |
| 272 | CompMB H | 0..200 % | 0.5 | display = dmin + (dmax-dmin)*n; display 0..200 %, 100% neutral; band gain dB ~= 33*log10(display/100) (multiband mode) | verified |
| 273 | LFO1 Rise | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 274 | LFO2 Rise | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 275 | LFO3 Rise | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 276 | LFO4 Rise | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 277 | LFO5 Rise | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 278 | LFO6 Rise | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 279 | LFO7 Rise | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 280 | LFO8 Rise | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 281 | LFO1 Delay | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 282 | LFO2 Delay | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 283 | LFO3 Delay | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 284 | LFO4 Delay | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 285 | LFO5 Delay | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 286 | LFO6 Delay | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 287 | LFO7 Delay | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 288 | LFO8 Delay | 0..1 | 0 | seconds = 4*n (0..4 s linear) | verified |
| 289 | FX Dist Level | 0..1 | 0.5 | dB = 40*log10(2n) (0.5 = 0 dB; -inf at 0, +12 dB at 1) | verified |
| 290 | FX Flg Level | 0..1 | 0.5 | dB = 40*log10(2n) (0.5 = 0 dB; -inf at 0, +12 dB at 1) | verified |
| 291 | FX Phaser Level | 0..1 | 0.5 | dB = 40*log10(2n) (0.5 = 0 dB; -inf at 0, +12 dB at 1) | verified |
| 292 | FX Chorus Level | 0..1 | 0.5 | dB = 40*log10(2n) (0.5 = 0 dB; -inf at 0, +12 dB at 1) | verified |
| 293 | FX Delay Level | 0..1 | 0.5 | dB = 40*log10(2n) (0.5 = 0 dB; -inf at 0, +12 dB at 1) | verified |
| 294 | FX Comp Level | 0..1 | 0.5 | dB = 40*log10(2n) (0.5 = 0 dB; -inf at 0, +12 dB at 1) | verified |
| 295 | FX Reverb Level | 0..1 | 0.5 | dB = 40*log10(2n) (0.5 = 0 dB; -inf at 0, +12 dB at 1) | verified |
| 296 | FX DimExp Level | 0..1 | 0.5 | dB = 40*log10(2n) (0.5 = 0 dB; -inf at 0, +12 dB at 1) | verified |
| 297 | FX Filter Level | 0..1 | 0.5 | dB = 40*log10(2n) (0.5 = 0 dB; -inf at 0, +12 dB at 1) | verified |
| 298 | FX Hyper Level | 0..1 | 0.5 | dB = 40*log10(2n) (0.5 = 0 dB; -inf at 0, +12 dB at 1) | verified |

### Thematic index

| Range | Contents |
|-------|----------|
| 0 | master volume |
| 1–13 / 14–26 | oscillator A / B (vol, pan, octave, semi, fine, unison ×5, warp, coarse pitch, wavetable position, random phase, phase) |
| 27–32 | noise oscillator |
| 33–34 | sub oscillator level/pan |
| 35–39 / 51–55 / 56–60 | envelopes 1 / 2 / 3 (atk, hold, dec, sus, rel) |
| 40–43 | oscillator→filter routing switches |
| 44–50 | voice filter (type, cutoff, reso, drive, var, mix, stereo) |
| 61–64 | LFO 1–4 rate |
| 65–66 | portamento time/curve |
| 67–70 | chaos 1/2 BPM flags + rates |
| 71–79 | envelope curves (A/D/R × env 1–3) |
| 80 | master tuning |
| 81–87 | reverb |
| 88–95 | EQ |
| 96–102 / 154 | distortion (+enable) |
| 103–108 / 155 | flanger (+enable) |
| 109–115 / 156 | phaser (+enable) |
| 116–123 / 157 | chorus (+enable) |
| 124–134 / 158 | delay (+enable) |
| 135–140 / 159 | compressor (+enable) |
| 141–146 / 162 | FX filter (+enable; pan at 268) |
| 147–153 / 163 | Hyper/Dimension (+enable) |
| 160 / 161 | reverb / EQ enable |
| 164–165 | oscillator pitch-tracking switches |
| 166–167 | pitch-bend range up/down |
| 168–169 | warp mode A/B (menu) |
| 170–171 | sub oscillator shape / octave |
| 172–179 | unison spreads + stack A/B |
| 180–211 | mod slots 1–16 amount/output (2 params per slot) |
| 212–216 | on/off switches (osc A/B/N/S, filter) |
| 217–221 | mod wheel value + macro 1–4 values |
| 222 | `Amp.` (alias of the master volume, degenerate listing entry) |
| 223–226 | LFO 1–4 smooth |
| 227 | pitch-bend position |
| 228–259 | mod slots 17–32 amount/output |
| 260–263 / 264–267 | LFO 5–8 rate / smooth |
| 268–272 | FX-filter pan, compressor wet, multiband L/M/H |
| 273–288 | LFO 1–8 rise / delay |
| 289–298 | per-effect output trims `FX * Level` |

## 4. Value-transform reference (exact formulas)

`n` = stored 0..1 value; `d` = display value = `dmin + (dmax−dmin)·n`.
Ampitude laws refer to linear gain (multiply the signal by).

| # | Curve | Formula | Evidence |
|---|-------|---------|----------|
| 1 | Master volume | dB = `60·log10(n)`; amplitude `n³` (70 % → −9.3 dB, 100 % → 0 dB) | verified (plugin read-out) |
| 2 | Oscillator level (A/B Vol, Noise Level), Env1 sustain, unison detune response | amplitude ∝ `n²` | verified |
| 3 | Sub level | amplitude ∝ `n` (linear — *unlike* A/B) | verified (A/B render) |
| 4 | Envelope times (attack/hold/decay/release, params 35–39/51–55/56–60) | seconds = `32·n⁵` (0.5 → 1.000 s; 0.25 → 31 ms; 0.75 → 7.59 s) | verified (plugin read-out) |
| 5 | Portamento time (65) | seconds = `8·n⁵` | verified |
| 6 | Filter cutoff (45, 143) | Hz = `8·(22050/8)^n` (log-linear 8 Hz..22.05 kHz; MIDI note = `69 + 12·log2(f/440)`, i.e. notes ≈ −49..135) | verified |
| 7 | Generic log-frequency knobs (EQ frq 88/89 = 22..20 kHz, phaser centre 113 = 20..18 kHz, delay filter 125 = 40..18 kHz, chorus filter 123 = 50..20 kHz, distortion filter 100 = 8..13290 Hz) | Hz = `lo·(hi/lo)^n` | verified |
| 8 | LFO rate (61–64, 260–263) | 229 steps: `step = round(n·228)`; **BPM mode** reads the division ladder (§4.1); **Hz mode** (flag byte 1 in the LFO block): Hz = `100·n⁴` | verified |
| 9 | FX modulation rates (flanger/phaser/chorus/hyper 105/111/118/148) | unsynced: Hz = `20·n⁴`; synced: the 31-entry ladder of §4.3 | verified |
| 10 | Chaos rate (69/70) | unsynced: Hz = `1000·n⁵` (quintic); with the Chaos BPM switch: the BPM ladder | verified |
| 11 | Compressor attack/release (137/138) | ms = `max(0.1, 1000·n²)` (default 0.3 → 90 ms) | verified |
| 12 | Compressor threshold (135) | piecewise-linear plugin read-out table (§4.4); 0.5 → −19.7 dB, 0.25 → −7.2 dB, 1.0 → −120 dB | verified |
| 13 | Compressor gain (139) | dB = `20·log10(1 + 31·n²)` (0.1 → 2.3, 0.5 → 18.8, 1.0 → 30.1) | verified |
| 14 | Compressor ratio (136) | stored `= 1 − 1/ratio` (2:1 → 0.478, 4:1 → 0.75, 10:1 → 0.9); display via the COMP_RATIO ladder (§4.5) | verified |
| 15 | Delay time (129/130) | synced (flag 127 = 1): `step = round(n·228)` into the DELAY_SYNC ladder (§4.6); unsynced: ms = `1 + 500·n⁴` (1..501 ms) | verified (plugin read-out) |
| 16 | Delay offset (133/134) | multiplier = `0.5 + n` (0.5x..1.5x; "Dot 1/2" = 0.75, "Dot" = 1.5) | verified |
| 17 | Noise pitch (28) | `n ≥ 0.5`: semitones = `96·(n−0.5)` (+48 st full up); `n < 0.5`: semitones = `100·log2(2n)` (−35 st at 0.4, −100 st at 0.25; floor −48 st below 0.36) | verified (spectrum) |
| 18 | `FX * Level` trims (289–298) | dB = `40·log10(2n)` (0.5 = 0 dB; 1.0 = +12 dB; 0 = −∞) | verified (plugin read-out) |
| 19 | `Mast.Tun` (80) and `A/B CoarsePit` (10/23) | semitones = `(n − 0.5)·128` (±64 st; Mast.Tun measured at two points ±0.1 → ±12.8 st) | verified |
| 20 | Menus (Fil Type 44/142, Warp 168/169, Dist Mode 99, Sub shape 170, EQ type 94/95, Delay mode 131, Hyp unison 150, Uni stack 178/179, Dist PrePost 102) | `index/(count−1)`; recover with `index = round(n·div)` for the first divisor in the build list that lands within 0.02 of an integer (order is append-only across builds; Serum re-bases on load) | verified |
| 21 | LFO smooth (223–227, 264–267) | seconds ≈ `0.5·n⁶` (inert until the top: 8 ms at 50 %, 0.5 s at 100 %) | verified (render) |
| 22 | LFO rise/delay (273–288) | seconds = `4·n` (0..4 s linear) | verified |
| 23 | Env curve knobs (71–79) | display 0..100 %, **50 % = straight**; mapped to Vital power ≈ `−17.5·(d/100 − 0.5)` (+2 decay/release, +3 attack offset on env 1) | verified |
| 24 | Chorus delay/depth/feedback (119–122) | calibrated approximations: delay s ≈ `0.02·n²` (20 ms full scale), depth `n²`, feedback `0.95·n` | inferred |
| 25 | `Amp.` (222) | same cubic law as MasterVol (60·log10(n) dB) | inferred |
| 26 | Wavetable position (11/24) | frame index = `1 + 255·n` (display 1..256; fractional values interpolate between frames) | verified |
| 27 | Effect wet knobs | wet-path gain law `sin²(π·w/2)` on the displayed percentage (delay/dist/reverb fixtures agree) | verified |
| 28 | Reverb decay (83) | seconds = `0.8 + 11.2·n` linear (0.8..12 s; the plugin's own name is DECAY — the published gist mislabels it pre-delay) | verified |
| 29 | Mod amount (180+2k, 228+2k) | stored −1..1 ↔ display −100..100 %; *fraction of the destination parameter's own range* (e.g. LFO → A Semi at +100 % = 24 st swing, because Semi spans −12..+12) | verified |
| 30 | Mod out (181+2k, 229+2k) | 0..1 ↔ 0..100 % output-range scaler; 1.0 = untouched | verified |

### 4.1 LFO rate ladder — BPM mode (229 steps, `step = round(n·228)`)

    step 0..13   32 bar     step 112..127  1/4      (default position, n = 0.5)
    step 14..29  16 bar     step 128..143  1/8
    step 30..46  8 bar      step 144..160  1/16
    step 47..62  4 bar      step 161..176  1/32
    step 63..78  2 bar      step 177..192  1/64
    step 79..94  bar        step 193..208  1/128
    step 95..111 1/2        step 209..225  1/256
                           step 226..228  fast

Sixteen steps per octave from 32 bars to 1/256. In Hz mode the knob is the
continuous `100·n⁴` Hz law instead (flag byte 1 of the LFO block).

### 4.2 Filter menu (96 entries, divisor 95; older builds 89/88, same order)

```
  0='MG Low 6'  1='MG Low 12'  2='MG Low 18'  3='MG Low 24'
  4='Low 6'  5='Low 12'  6='Low 18'  7='Low 24'
  8='High 6'  9='High 12'  10='High 18'  11='High 24'
  12='Band 12'  13='Band 24'  14='Peak 12'  15='Peak 24'
  16='Notch 12'  17='Notch 24'  18='LH 6'  19='LH 12'
  20='LB 12'  21='LP 12'  22='LN 12'  23='HB 12'
  24='HP 12'  25='HN 12'  26='BP 12'  27='BN 12'
  28='PP 12'  29='PN 12'  30='NN 12'  31='L/B/H 12'
  32='L/B/H 24'  33='L/P/H 12'  34='L/P/H 24'  35='L/N/H 12'
  36='L/N/H 24'  37='B/P/N 12'  38='B/P/N 24'  39='Cmb +'
  40='Cmb -'  41='Cmb L6+'  42='Cmb L6-'  43='Cmb H6+'
  44='Cmb H6-'  45='Cmb HL6+'  46='Cmb HL6-'  47='Flg +'
  48='Flg -'  49='Flg L6+'  50='Flg L6-'  51='Flg H6+'
  52='Flg H6-'  53='Flg HL6+'  54='Flg HL6-'  55='Phs 12+'
  56='Phs 12-'  57='Phs 24+'  58='Phs 24-'  59='Phs 36+'
  60='Phs 36-'  61='Phs 48+'  62='Phs 48-'  63='Phs 48L6+'
  64='Phs 48L6-'  65='Phs 48H6+'  66='Phs 48H6-'  67='Phs 48HL6+'
  68='Phs 48HL6-'  69='FPhs 12HL6+'  70='FPhs 12HL6-'  71='Low EQ 6'
  72='Low EQ 12'  73='Band EQ 12'  74='High EQ 6'  75='High EQ 12'
  76='Ring Mod'  77='Ring Modx2'  78='SampHold'  79='SampHold-'
  80='Combs'  81='Allpasses'  82='Reverb'  83='French LP'
  84='German LP'  85='Add Bass'  86='Formant-I'  87='Formant-II'
  88='Formant-III'  89='Bandreject'  90='Dist.Comb 1 LP'  91='Dist.Comb 1 BP'
  92='Dist.Comb 2 LP'  93='Dist.Comb 2 BP'  94='Scream LP'  95='Scream BP'
```

Index 0 = `MG Low 6`; **Serum's Init stores index 1 (`MG Low 12`)** — verified on
the `00 init` fixture and on all ten local blobs (`0.010526 = 1/95`).

### 4.3 Synced FX-rate ladder (chorus/flanger/phaser/hyper rate; 229 steps, 31 entries)

    step 0    Off (LFO stopped -> freeze)
    step 4    24 bar (= dotted 16 bar)     step 118  1/2.
    step 12   32 bar t                     step 126  bar t
    step 19   16 bar                       step 133  1/2
    step 27   12 bar (= dotted 8 bar)      step 141  1/4.
    step 35   16 bar t                     step 149  1/2 t
    step 42   8 bar                        step 156  1/4
    step 50   6 bar (= dotted 4 bar)       step 164  1/8.
    step 57   8 bar t                      step 171  1/4 t
    step 65   4 bar                        step 179  1/8
    step 73   3 bar (= dotted 2 bar)       step 187  1/16.
    step 80   4 bar t                      step 194  1/8 t
    step 88   2 bar                        step 202  1/16
    step 95   1.5 bar (= dotted bar)       step 209  1/32.
    step 103  2 bar t                      step 217  1/16 t
    step 111  bar                          step 225  1/32

### 4.4 Compressor threshold: knob → dB (piecewise-linear anchors)

    0.00 -> 0.0 dB    0.241 -> -7.2     0.482 -> -17.2    0.724 -> -33.5    0.917 -> -64.7
    0.048 -> -1.3     0.289 -> -8.9     0.531 -> -19.7    0.772 -> -38.5    0.965 -> -87.1
    0.096 -> -2.6     0.338 -> -10.7    0.579 -> -22.5    0.820 -> -44.7    1.00 -> -120
    0.145 -> -4.1     0.386 -> -12.7    0.627 -> -25.7    0.868 -> -52.8
    0.193 -> -5.6     0.434 -> -14.8    0.675 -> -29.3    0.917 -> -64.7

(Interpolate linearly between anchors; the full table is in the JSON as
`COMP_THRESHOLD_KNOB_TO_DB`.)

### 4.5 Compressor ratio ladder (229 steps, 22 entries)

    0          1:1        109..151  2:1     191..195  6:1
    1..5       1.0:1      152..170  3:1     196..199  7:1
    6..17      1.1:1      171..182  4:1     200..205  8:1
    18..28     1.2:1      183..190  5:1     206..208  10:1
    29..39     1.3:1      (86..96 1.8:1, 97..108 1.9:1 — see JSON)
    40..51     1.4:1      209..214  16:1
    52..62     1.5:1      215..223  32:1
    63..74     1.6:1      224..228  Limit
    75..85     1.7:1

The *stored* value is simply `1 − 1/ratio` — the ladder is only needed to
reproduce the GUI text.

### 4.6 Delay time ladder — synced (229 steps)

    step 0..10    fast      step 115..134  1/8
    step 11..31   1/256     step 135..155  1/4
    step 32..51   1/128     step 156..176  1/2
    step 52..72   1/64      step 177..196  bar
    step 73..93   1/32      step 197..217  2 bar
    step 94..114  1/16      step 218..228  4 bar

## 5. Modulation-slot record layout (40 bytes)

Slots 1–16 sit at `0x0000 + 40·(slot−1)`; slots 17–32 at `0x50E0 + 40·(slot−17)`
(fixed offset **verified on all ten 172,736-byte blobs** — the empty gap
`0x4CAC..0x50E0` and padding after are zero). Records are self-identifying:
bytes **`80 <slot−1> FF` at +0x21** (the middle byte is the 0-based slot, so
`80 10 FF` marks slot 17). The byte at +0x20 is *usually* `0x80` but is NOT part
of the marker — `0xFF` and arbitrary values occur in ~3 % of presets (mostly
LFO → level routings); a reader that requires `80 80` there silently drops
routings.

| Offset | Type | Meaning |
|-------:|------|---------|
| +0x00 | f32 | current/smoothed amount (0.5 in unused slots); not needed |
| +0x04 | f32 | **amount**, bipolar −1..1 |
| +0x08 | f32 | **output-range** scaler (1.0 = 100 %) |
| +0x0C | u8 | matrix **type**: 0 unipolar, 1 bipolar — bipolar means the source swings ±amount·range/2 *around the destination knob's value* (measured: LFO → Semi at +100 % plays 12 − 24·y st) |
| +0x14 | u16 | **source** id (table below) |
| +0x16 | u16 | **aux** source id (0 = none; multiplies the source) |
| +0x18 | u16 | bijective remap of dest in Serum's internal menu order — unverified, ignore |
| +0x1A | u16 | **destination** = VST parameter index 0..298 (§3) |
| +0x20..+0x23 | bytes | `80 80 <slot−1> FF` marker |

Unlisted bytes are uninitialised padding and leak heap fragments (they vary
between saves of the same patch). Accept a record only if the marker matches,
`amount` is not NaN and `|amount| ≤ 8`, `source ≤ 64`, `aux ≤ 64`,
`dest < 1024`. Unused slots carry `amount = 0, source = 0` and a garbage dest
(`316` or `315` observed) — never trust dest when source == 0. For maximum
robustness on non-standard blobs, fall back to scanning the whole state for the
marker (overlapping occurrences permitted) and validate as above.

### Source ids (verified via fixture presets + corpus correlation)

| id | source | id | source | id | source |
|----|--------|----|--------|----|--------|
| 1 | Mod Wheel | 13 | Velocity | 24–27 | Macro 1–4 |
| 2–4 | Env 1–3 | 14 | Note | 28 | Pitch Bend |
| 5–8 | LFO 1–4 | 15 | Aftertouch (channel) | 29–31 | MPE X/Y/Z |
| 9–12 | LFO 5–8 | 16 | Poly Aftertouch | 32 | Release Velocity |
| 17 | Chaos 1 | 19 | Noise OSC | 33 | Fixed |
| 18 | Chaos 2 | 20/21 | NoteOn Rand 1/2 | 22/23 | NoteOn Alt 1/2 |

## 6. LFO block layout

Two layouts exist; a blob uses the **new** one iff `len ≥ 0x84D8 + 8·0x2D28` AND
the classic region `0x0280..0x0680` is all zero (verified true on all ten
modern blobs).

### New layout (172,736-byte blobs)

Eight blocks, base `0x84D8`, stride `0x2D28`:

| Offset in block | Contents |
|--------|----------|
| +0x0000 | one float64, purpose unknown (0 in most files; skip) |
| +0x0008 | **curves/tension**, 480 × float64 (0.5 = straight segment) |
| +0x0F08 | **x**, 480 × float64 (0..1 non-decreasing; the shape ends at the first point reaching 1.0 — the rest is 1.0-padding) |
| +0x1E08 | **y**, 480 × float64 (**0 = top of the display**; the default triangle reads `1, 0, 1`; the loop is closed last→first, so the default plays as a triangle) |
| +0x2D08 | six flag bytes, same order as the classic record: **anchor, Hz, dotted, triplet, not-off, env** — mode OFF = `(0,0)`, TRIG = `(1,0)`, ENV = `(1,1)` on the (not-off, env) pair; anchor defaults 1 |
| +0x2D10 | int32 — **not a reliable point count** (reads 2 for the default 3-point shape, 4 for a 4-point shape, 1 in blocks 9–12; meaning unverified — derive the point count from the arrays) |
| +0x2D18 | int32 array length (481 in LFO blocks, 0 in blocks 9–12) |
| +0x2D1C | float32 **copy of the rate parameter** (verified equal on all 40 LFO blocks of the ten blobs) |

The three 480-double arrays plus the 8-byte lead end exactly at the flags
(8 + 3·480·8 = 0x2D08). y orientation and loop closure were settled by rendering
LFO → level routings through the plugin (correlation 0.99); an earlier reading
(inverted, open loop) anti-correlated at −0.44.

Blocks 9–12 (base `0x1EE18`, same stride) are the warp **remap graphs**, not
LFOs (flags all zero, arrlen 0).

### Classic layout (20–34 KB blobs, builds ≤ ~1.3) — brief

* `0x0280` LFO 1–4 shapes: 12 arrays × 65 float64, stride 520 (arrays 0–3
  tension, 4–7 x, 8–11 y); `0x1B70` the same for LFO 5–8.
* `0x1AE0` LFO 1–4 switches (144 B): +0x00 u8×4 point count, +0x04 f32×4 rate
  copy, +0x14 anchor, +0x18 Hz, +0x1C dotted, +0x20 triplet, +0x24 not-off,
  +0x28 env (four bytes each, one per LFO). LFO 5–8 switches were never saved —
  only the four anchor bytes at `0x33D0` are recognisable; assume synced,
  free-running for those.
* Rate/flag semantics identical to the new layout.

## 7. Global switches block (0x4C48, 100 bytes of float32)

Follows the parameter array; moves with the writing build (`0x4C48` current,
`0x4C44` / `0x4B9C` older, absent in 20–21 KB blobs). Locate by invariants —
0.5 at +0x00 and +0x20, 1.0 at +0x30, and an exact `(n−1)/31` at +0x2C — three
of the four are enough.

| Offset | Field | Encoding |
|-------:|-------|----------|
| +0x00 | A4 tuning reference | `430 + 20·v` Hz (0.5 = 440) |
| +0x04 | *unknown* | 0/1 in 1.5 % of presets; no rendered effect — GUI state |
| +0x08 / +0x0C | unison tuning A / B | `index/4`: 0 Linear, 1 Super, 2 Exp, 3 Inv, 4 Random |
| +0x10 | Mono | 0/1 |
| +0x14 | Legato | 0/1 |
| +0x18 | Porta "Always" | 0/1 |
| +0x1C | Porta "Scaled" | 0/1 |
| +0x20 | Oversampling | `index/2`: 0 = 1x, 1 = 2x (default 0.5), 2 = 4x |
| +0x24 | Noise one-shot | 0/1 (sample stops at its end) |
| +0x28 | Noise pitch-track | 0/1 (Pitch knob reads semitones) |
| +0x2C | Polyphony | `(voices − 1)/31` (default 8 voices = 7/31) |
| +0x30 | *unknown* | always 1.0 |
| +0x34 | Filter keytrack | 0/1 |
| +0x38 / +0x3C | Unison range A / B | semitones/48 (default 2 st = 0.041667) |
| +0x40 / +0x44 | Chaos 1/2 Mono | 0/1 |
| +0x48 | Chorus mono | 0/1 (chorus LFO in phase on L/R; 81 % of chorus presets) |
| +0x4C | *unknown* | 0/1 in 3 %; no rendered effect |
| +0x50 / +0x54 | Chaos 1/2 S&H | 0/1 (stepped output) |
| +0x58 | *unknown* | junk in old builds |
| +0x5C | Reverb Hall/Plate | 1 = Hall (default), 0 = Plate; byte copy at `0x3B04` |
| +0x60 | *unknown* | 0.1 default, 0..0.175; no rendered effect (probably display zoom) |

## 8. FX rack order & per-effect record

* **FX order**: 10 × int32 LE at `0x3BE0`, one per effect in enable-parameter
  order — `distortion, flanger, phaser, chorus, delay, compressor, reverb, eq,
  filter, hyper` — value = rack position (0 = top). Default
  `[1,2,3,4,5,6,7,8,9,0]` puts Hyper first, matching Serum's default rack.
  Validate as a permutation of 0..9 (valid in all ten local blobs).
* **Per-effect record** (`0x37F0`..`0x3BE0`): byte mirrors of each effect's
  knobs/mode/enable (see §2). Only `0x3B04` (reverb Plate/Hall) carries
  information not duplicated elsewhere; it mirrors switches +0x5C (verified
  equal in all ten blobs).

## 9. Older-build caveats (presets of 20–34 KB)

Older builds stopped the file before all of the second parameter block existed,
leaving uninitialised memory that Serum itself ignores. Reset the tail of the
high block to the documented defaults by decompressed size (checked against the
plugin's read-back):

| Decompressed size | Trustable to | Reset from |
|-------------------|--------------|------------|
| < 21,000 B | param 227 | 228 (`Mod17 amt`) |
| < 28,000 B | param 259 (`Mod32 out`) | 260 (`LFO5Rate`) |
| < 33,000 B | param 272 (`CompMB H`) | 273 (`LFO1 Rise`) |
| ≥ 33,000 B | all 299 | — |

## 10. Local verification (10 blobs)

All ten blobs: 172,736 bytes; classic region zero; FX order a valid permutation
at `0x3BE0`; landmark params match the documented defaults (`A Pan` 0.5,
`Bend U/D` 0.5417/0.4583, `Mod N out` 1.0, LFO smooth 0.0, `Mod17 amt` 0.5,
`CompMB H` 0.5, `LFO1 Rise` 0.0, `FX * Level` 0.5); global switches at `0x4C48`
(A4 440/450 Hz, poly 8, oversampling 2x); reverb Hall byte 1; LFO rate copies
equal their rate parameters. Default racks `[1,2,...,9,0]` in 6 of 10; two real
re-orders observed (compressor-first; flanger-first).

The five FL-state presets, one line each:

* `01_- Init -reese.fxp` — "- Init -reese": osc A+B (+sub) on, A 10-voice,
  MG Low 12 @ default cutoff, no FX, no mod slots, all LFOs default OFF 1/4,
  mono+legato on, custom rack (compressor first).
* `02_Chord_Hyperpop_Chord.fxp` — osc A (BSOD_Square) on, distortion HardClip
  @ 49 % drive + reverb on, default rack, LFOs default, no mod slots.
* `03_indigo - basic shapes sub.fxp` — osc A+B+noise (Basic Shapes /
  BrightWhite) on, B unison 16, MG Low 12, no FX, no mod slots, LFOs default.
* `04_- Init -.fxp` — stock-ish Init (A 16-voice, MG Low 12, no FX/mods),
  heap junk in the tail region (harmless).
* `05_BS - YUKIYANAGI UKHC BASS 01.fxp` — full patch: A "DS Saw and Tri"
  (frame 180), B "raw.wav", noise "SID noise.wav"; MG Low 24, cutoff max,
  filter drive 3 %; distortion Tape Sat. @ 90 % drive + phaser on; custom rack
  (flanger first); mods: LFO1→A Vol +51 %, LFO1→B Vol +46 %, LFO2→B Warp +15 %
  (all unipolar); LFO1 ENV mode 4 points @ 1/8, LFO2 ENV 3 points @ 1/4;
  A4 = 450 Hz.

## 11. Known unknowns & gotchas

* **`serum1.py` docstring vs reality:** its LFO description ("tension[481],
  x[481], y[479]") is the superseded pre-0.5.1 reading. Correct layout is the
  8-byte lead + 480/480/480 arrays of §6 (reading y from +0x1E10 shifts every
  curve by one point and flips shapes upside down).
* **+0x2D10 "point count"** (FORMATS.md) is not a plain point count — see §6.
  Derive the point count from the x array.
* **Unused mod slots** carry `dest = 316/315` (beyond the 299-parameter list)
  and heap junk elsewhere; marker + field validation is mandatory.
* **The +0x20 byte** of a mod record is not part of the marker (~3 % of
  presets break readers that require `80 80`).
* **`Mast.Tun`** display range in the published listing is degenerate (0..0);
  the measured law is `(n−0.5)·128` semitones. Same for `Amp.`.
* The gist's names for reverb 83/85/87 and slot 193 (`Mod 7 out`) and
  CompMB 270–272 are corrected here per plugin read-back.
* Wavetable/noise names are NUL-terminated paths resolved against Serum's
  `Tables` folder; an empty OSC A name with Osc A On means the built-in default
  saw table.
* Second zlib stream(s) after stream 0 are **embedded asset data** (raw f32 LE
  frames, 8192 B/frame; the default 16 KB stream is the built-in noise sample
  `AC hum1.wav`, md5 `1765102a…`) — must be preserved verbatim; the trailing
  u32 LE is the *compressed* length of stream 0.
* Zip-packed FLPs and Serum 2 (`XferJson`) state are out of scope here.

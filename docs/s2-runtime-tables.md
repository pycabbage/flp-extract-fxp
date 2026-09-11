# Serum 2 runtime-initialized tables (`Serum2.vst3` 2.0.23) — dumped after `InitDll`

Companion to `s1-to-s2-mapping.md` (static RE of the Serum-1 preset importer).
That document established that several conversion data tables live in `.data`
**beyond the file-mapped raw extent** (`.data` VirtSize 0xF82DE4 vs RawSize
0x99C00) and are therefore **runtime-relocated/filled by `InitDll`** — their
contents are not statically readable. This document dumps them **after
initialization**, from inside a live process.

Extraction method (reproducible):

```text
harness:  python 3.14 + ctypes (scratch: C:\Users\cabbage\AppData\Local\Temp\opencode\conv_work\s2runtime\)
1. LoadLibraryW("C:\Program Files\Common Files\VST3\Serum2.vst3\Contents\x86_64-win\Serum2.vst3")
2. GetProcAddress("InitDll"); InitDll(module)          ; return code varies per run
3. module handle = runtime ImageBase (0x7FFD1DF80000 in the session used here);
   VA(RVA) = base + RVA; reads are plain ctypes.string_at (same process)
4. dump tables, resolve pointer fields, and additionally CALL two of the
   importer's helper functions directly (fn_4d93b0, fn_4d9a90) to obtain the
   per-index kParam names / FX family ids — see §2/§7
```

The plugin was only loaded and initialized (no UI, no `createInstance`, no
state I/O); the `.vst3` file was opened read-only. `InitDll`'s return value
differed across runs (240777473 / -685357055 / -672999167) but the tables were
fully populated every time.

Machine-readable dumps (same folder as the generator scripts):
`fx_desc_table.json` (§1, raw 0x50-byte hex per entry), `source_enum.json`
(§3), `param_names.json` (§2 + §7 merged view), `misc_tables.json` (§5/§6),
`strings_pool.json` (§4).

## 1. FX/mod descriptor table — RVA 0x179B060, 343 entries, stride 0x50

Indexed by the **S2 parameter index** (the same numeric domain as the S1
master params and the S1 mod-matrix dest/source codes). The importer's
converter touches it at: descriptor fetch `fn_4d8ff0` (0x4D8FF0), value-write
`fn_4d9da0` (0x4D9DA0), name lookup `fn_4d93b0` (0x4D93B0), and the ModSlot
node builder (dest codes, `s1load.txt` 0x4DF760/0x4DF95B/0x4DFAFC).

Entry layout (all little-endian; verified from the runtime bytes + the
disassembly that reads each field):

| offset | type | meaning |
|---|---|---|
| +0x00 | f64 | default value (pushed into `plainParams` when nothing else sets the param) |
| +0x08 | f64 | min |
| +0x10 | f64 | max |
| +0x18 | f64 | step (compared against a 0.5 constant in `fn_4d9da0` write-mode 0) |
| +0x20 | u32 | count — integer-step count used by write-mode 0 (`v → min + clamp(round(v·(cnt+1)), 0, cnt)`) |
| +0x24 | u32 | write mode 0..3 (see below) |
| +0x28 | ptr | option-list vector begin (`std::vector`, MSVC 3-ptr: +0x28 begin, +0x30 end, +0x38 capacity end) |
| +0x30 | ptr | option-list vector end |
| +0x38 | ptr | option-list vector capacity end (null unless options exist) |
| +0x40 | `const char*` | **submap/section name** (NUL-terminated ASCII; becomes `destModuleTypeString`, the JSON section key, or the `"+ FX"` sub-engine name) |
| +0x48 | i32 | **submap instance id** (`Oscillator0..3`, `ModSlot0..31`, …; becomes `destModuleID` for non-FX dests — `s1load.txt` 0x4DFAFC) |
| +0x4C | i32 | **module-local param id** (becomes `destModuleParamID`; −1 = none) |

Corrections vs `s1-to-s2-mapping.md` (which could only see the *use sites*):
+0x00 is the default value (not "key storage"), +0x48 is the submap instance
id (matches its `destModuleID` use), +0x4C is the module-local param id (not
an "FX type id" — the FX family comes from `fn_4d9a90`, see §7).

Write-mode semantics (from `fn_4d9da0`, jump table 0xA56460; value first
clamped into `[min, max]`):

| mode | transform |
|---|---|
| 0 | `min + min(trunc(v'·(cnt+1)), cnt)` where `v' = round(v)` unless `step == 1.0` (integer-step snap; `cnt == 0` degenerates to the linear form) |
| 1 | normalize by `[min, max]`, round-to-nearest int (0x9B5DD0), re-scale |
| 2, 3 | linear `min + v·(max−min)` |
| any | if the option list is non-empty: `v = options[trunc(v')]` — **the written value is the option's NAME string** (a JSON string, built by `0x92170` from `*(char**)elem`), i.e. the S1 0…1 fraction selects an option index and the state stores the option identifier itself |

Option-list elements are 8-byte entries, each a pointer to a NUL-terminated
ASCII option name; +0x28/+0x30/+0x38 form the MSVC `std::vector` triple
(begin / end / capacity-end). 18 of the 343 entries carry lists (§2).

## 2. Complete descriptor table (all 343 entries)

Columns: S2/S1 index; `submap` = the string at +0x40 (the importer's
`destModuleTypeString` / JSON section / `"+ FX"` sub-engine); `inst` = +0x48;
`pid` = +0x4C; `kParam name` = the string returned by calling `fn_4d93b0(idx)`
at runtime (this is the name the importer writes into `plainParams`);
`mode`/`cnt` = the +0x24/+0x20 fields consumed by `fn_4d9da0`; `options` =
the resolved option-list strings (empty = no list; all pointer fields for
non-listed entries are null). Seven entries (315, 316, 330, 337, 338, 339,
342) have **no** submap/name — `fn_4d8ff0` returns null for them and
`fn_setval` drops those parameters (idx 315/316/341 are additionally
bitmask-dropped, idx 330 explicitly).

(Per-entry raw 0x50-byte hex is in `fx_desc_table.json`, field `raw_hex`.)

| idx | submap | inst | pid | kParam name | min | max | step | mode | cnt | options |
|---|---|---|---|---|---|---|---|---|---|---|
| 0 | Global | 0 | 0 | `kParamMasterVolume` | 0 | 0.8541468079 | 3 | 1 | 0 |  |
| 1 | Oscillator | 0 | 1 | `kParamVolume` | 0 | 1 | 2 | 1 | 0 |  |
| 2 | Oscillator | 0 | 2 | `kParamPan` | -50 | 50 | 1 | 1 | 0 |  |
| 3 | Oscillator | 0 | 3 | `kParamOctave` | -4 | 4 | 1 | 1 | 8 |  |
| 4 | Oscillator | 0 | 4 | `kParamPitch` | -12 | 12 | 1 | 1 | 24 |  |
| 5 | Oscillator | 0 | 5 | `kParamFine` | -100 | 100 | 1 | 1 | 0 |  |
| 6 | Oscillator | 0 | 24 | `kParamUnison` | 1 | 16 | 1 | 1 | 15 |  |
| 7 | Oscillator | 0 | 26 | `kParamDetune` | 0 | 1 | 2 | 1 | 0 |  |
| 8 | Oscillator | 0 | 27 | `kParamDetuneWid` | 0 | 100 | 1 | 1 | 0 |  |
| 9 | WTOsc | 0 | 0 | `kParamWarp` | 0 | 1 | 1 | 0 | 0 |  |
| 10 | Oscillator | 0 | 6 | `kParamCoarsePit` | -64 | 64 | 1 | 1 | 0 |  |
| 11 | WTOsc | 0 | 6 | `kParamTablePos` | 1 | 256 | 1 | 1 | 0 |  |
| 12 | WTOsc | 0 | 9 | `kParamRandomPhase` | 0 | 100 | 1 | 1 | 0 |  |
| 13 | WTOsc | 0 | 8 | `kParamInitialPhase` | 0 | 360 | 1 | 1 | 0 |  |
| 14 | Oscillator | 1 | 1 | `kParamVolume` | 0 | 1 | 2 | 1 | 0 |  |
| 15 | Oscillator | 1 | 2 | `kParamPan` | -50 | 50 | 1 | 1 | 0 |  |
| 16 | Oscillator | 1 | 3 | `kParamOctave` | -4 | 4 | 1 | 1 | 8 |  |
| 17 | Oscillator | 1 | 4 | `kParamPitch` | -12 | 12 | 1 | 1 | 24 |  |
| 18 | Oscillator | 1 | 5 | `kParamFine` | -100 | 100 | 1 | 1 | 0 |  |
| 19 | Oscillator | 1 | 24 | `kParamUnison` | 1 | 16 | 1 | 1 | 15 |  |
| 20 | Oscillator | 1 | 26 | `kParamDetune` | 0 | 1 | 2 | 1 | 0 |  |
| 21 | Oscillator | 1 | 27 | `kParamDetuneWid` | 0 | 100 | 1 | 1 | 0 |  |
| 22 | WTOsc | 1 | 0 | `kParamWarp` | 0 | 1 | 1 | 0 | 0 |  |
| 23 | Oscillator | 1 | 6 | `kParamCoarsePit` | -64 | 64 | 1 | 1 | 0 |  |
| 24 | WTOsc | 1 | 6 | `kParamTablePos` | 1 | 256 | 1 | 1 | 0 |  |
| 25 | WTOsc | 1 | 9 | `kParamRandomPhase` | 0 | 100 | 1 | 1 | 0 |  |
| 26 | WTOsc | 1 | 8 | `kParamInitialPhase` | 0 | 360 | 1 | 1 | 0 |  |
| 27 | Oscillator | 3 | 1 | `kParamVolume` | 0 | 1 | 2 | 1 | 0 |  |
| 28 | NoiseOsc | 3 | 0 | `kParamColor` | 0 | 1 | 1 | 0 | 0 |  |
| 29 | NoiseOsc | 3 | 1 | `kParamFine` | -1 | 1 | 1 | 1 | 0 |  |
| 30 | Oscillator | 3 | 2 | `kParamPan` | -50 | 50 | 1 | 1 | 0 |  |
| 31 | NoiseOsc | 3 | 3 | `kParamRandomPhase` | 0 | 100 | 1 | 1 | 0 |  |
| 32 | NoiseOsc | 3 | 2 | `kParamInitialPhase` | 0 | 100 | 1 | 1 | 0 |  |
| 33 | Oscillator | 4 | 1 | `kParamVolume` | 0 | 1 | 2 | 1 | 0 |  |
| 34 | Oscillator | 4 | 2 | `kParamPan` | -50 | 50 | 1 | 1 | 0 |  |
| 35 | Env | 0 | 0 | `kParamAttack` | 0 | 32 | 5 | 1 | 0 |  |
| 36 | Env | 0 | 1 | `kParamHold` | 0 | 32 | 5 | 1 | 0 |  |
| 37 | Env | 0 | 2 | `kParamDecay` | 0 | 32 | 5 | 1 | 0 |  |
| 38 | Env | 0 | 3 | `kParamSustain` | 0 | 1 | 1 | 0 | 0 |  |
| 39 | Env | 0 | 4 | `kParamRelease` | 0 | 32 | 5 | 1 | 0 |  |
| 40 | RoutingSlot | 0 | 3 | `kParamRoutingDest` | 0 | 3 | 1 | 1 | 3 | `kRoutingDestFilter`, `kRoutingDestMaster`, `kRoutingDestDirect`, `kRoutingDestNone` |
| 41 | RoutingSlot | 1 | 3 | `kParamRoutingDest` | 0 | 3 | 1 | 1 | 3 | `kRoutingDestFilter`, `kRoutingDestMaster`, `kRoutingDestDirect`, `kRoutingDestNone` |
| 42 | RoutingSlot | 3 | 3 | `kParamRoutingDest` | 0 | 3 | 1 | 1 | 3 | `kRoutingDestFilter`, `kRoutingDestMaster`, `kRoutingDestDirect`, `kRoutingDestNone` |
| 43 | RoutingSlot | 4 | 3 | `kParamRoutingDest` | 0 | 3 | 1 | 1 | 3 | `kRoutingDestFilter`, `kRoutingDestMaster`, `kRoutingDestDirect`, `kRoutingDestNone` |
| 44 | VoiceFilter | 0 | 2 | `kParamType` | 0 | 95 | 1 | 1 | 95 | `MgL6`, `MgL12`, `MgL18`, `MgL24`, `L6`, `L12`, `L18`, `L24`, `H6`, `H12`, `H18`, `H24`, `B12`, `B24`, `P12`, `P24`, `N12`, `N24`, `LH6`, `LH12`, `LB12`, `LP12`, `LN12`, `HB12`, `HP12`, `HN12`, `BP12`, `BN12`, `PP12`, `PN12`, `NN12`, `LBH12`, `LBH24`, `LPH12`, `LPH24`, `LNH12`, `LNH24`, `BPN12`, `BPN24`, `CombP`, `CombN`, `CombL6P`, `CombL6N`, `CombH6P`, `CombH6N`, `CombHL6P`, `CombHL6N`, `FlangeP`, `FlangeN`, `FlangeL6P`, `FlangeL6N`, `FlangeH6P`, `FlangeH6N`, `FlangeHL6P`, `FlangeHL6N`, `Phase12P`, `Phase12N`, `Phase24P`, `Phase24N`, `Phase36P`, `Phase36N`, `Phase48P`, `Phase48N`, `Phase48L6P`, `Phase48L6N`, `Phase48H6P`, `Phase48H6N`, `Phase48HL6P`, `Phase48HL6N`, `FlangePhase12HL6P`, `FlangePhase12HL6N`, `LEQ6`, `LEQ12`, `BEQ12`, `HEQ6`, `HEQ12`, `RM`, `RMT`, `SNH1`, `SNH2`, `Combs`, `Allpasses`, `Reverb1`, `Scream`, `ZDF_A`, `ADD_BASS`, `FormantONE`, `FormantTWO`, `FormantTWB`, `BandReject`, `DistComb1LP`, `DistComb1BP`, `DistComb2LP`, `DistComb2BP`, `Scream3LP`, `Scream3BP` |
| 45 | VoiceFilter | 0 | 3 | `kParamFreq` | 0 | 1 | 1 | 0 | 0 |  |
| 46 | VoiceFilter | 0 | 4 | `kParamReso` | 0 | 100 | 1 | 1 | 0 |  |
| 47 | VoiceFilter | 0 | 5 | `kParamDrive` | 0 | 100 | 1 | 1 | 0 |  |
| 48 | VoiceFilter | 0 | 6 | `kParamVar` | 0 | 100 | 1 | 1 | 0 |  |
| 49 | VoiceFilter | 0 | 1 | `kParamWet` | 0 | 100 | 1 | 1 | 0 |  |
| 50 | VoiceFilter | 0 | 7 | `kParamStereo` | 0 | 100 | 1 | 1 | 0 |  |
| 51 | Env | 1 | 0 | `kParamAttack` | 0 | 32 | 5 | 1 | 0 |  |
| 52 | Env | 1 | 1 | `kParamHold` | 0 | 32 | 5 | 1 | 0 |  |
| 53 | Env | 1 | 2 | `kParamDecay` | 0 | 32 | 5 | 1 | 0 |  |
| 54 | Env | 1 | 3 | `kParamSustain` | 0 | 1 | 1 | 0 | 0 |  |
| 55 | Env | 1 | 4 | `kParamRelease` | 0 | 32 | 5 | 1 | 0 |  |
| 56 | Env | 2 | 0 | `kParamAttack` | 0 | 32 | 5 | 1 | 0 |  |
| 57 | Env | 2 | 1 | `kParamHold` | 0 | 32 | 5 | 1 | 0 |  |
| 58 | Env | 2 | 2 | `kParamDecay` | 0 | 32 | 5 | 1 | 0 |  |
| 59 | Env | 2 | 3 | `kParamSustain` | 0 | 1 | 1 | 0 | 0 |  |
| 60 | Env | 2 | 4 | `kParamRelease` | 0 | 32 | 5 | 1 | 0 |  |
| 61 | LFO | 0 | 0 | `kParamRate` | 0 | 100 | 4 | 1 | 0 |  |
| 62 | LFO | 1 | 0 | `kParamRate` | 0 | 100 | 4 | 1 | 0 |  |
| 63 | LFO | 2 | 0 | `kParamRate` | 0 | 100 | 4 | 1 | 0 |  |
| 64 | LFO | 3 | 0 | `kParamRate` | 0 | 100 | 4 | 1 | 0 |  |
| 65 | Global | 0 | 3 | `kParamPortamentoTime` | 0 | 8 | 5 | 1 | 0 |  |
| 66 | Global | 0 | 4 | `kParamPortamentoCurve` | -100 | 100 | 1 | 1 | 0 |  |
| 67 | LFO | 8 | 6 | `kParamBeatSync` | 0 | 1 | 1 | 1 | 1 |  |
| 68 | LFO | 9 | 6 | `kParamBeatSync` | 0 | 1 | 1 | 1 | 1 |  |
| 69 | LFO | 8 | 0 | `kParamRate` | 0 | 100 | 5 | 1 | 0 |  |
| 70 | LFO | 9 | 0 | `kParamRate` | 0 | 100 | 5 | 1 | 0 |  |
| 71 | Env | 0 | 5 | `kParamCurve1` | 0 | 100 | 1 | 1 | 0 |  |
| 72 | Env | 0 | 6 | `kParamCurve2` | 0 | 100 | 1 | 1 | 0 |  |
| 73 | Env | 0 | 7 | `kParamCurve3` | 0 | 100 | 1 | 1 | 0 |  |
| 74 | Env | 1 | 5 | `kParamCurve1` | 0 | 100 | 1 | 1 | 0 |  |
| 75 | Env | 1 | 6 | `kParamCurve2` | 0 | 100 | 1 | 1 | 0 |  |
| 76 | Env | 1 | 7 | `kParamCurve3` | 0 | 100 | 1 | 1 | 0 |  |
| 77 | Env | 2 | 5 | `kParamCurve1` | 0 | 100 | 1 | 1 | 0 |  |
| 78 | Env | 2 | 6 | `kParamCurve2` | 0 | 100 | 1 | 1 | 0 |  |
| 79 | Env | 2 | 7 | `kParamCurve3` | 0 | 100 | 1 | 1 | 0 |  |
| 80 | Global | 0 | 1 | `kParamMasterTuning` | 0 | 1 | 1 | 0 | 0 |  |
| 81 | FXReverb | 0 | 1 | `kParamWet` | 0 | 100 | 1 | 1 | 0 |  |
| 82 | FXReverb | 0 | 2 | `kParamSize` | 0 | 100 | 1 | 1 | 0 |  |
| 83 | FXReverb | 0 | 3 | `kParamDelay` | 0 | 250 | 2 | 1 | 0 |  |
| 84 | FXReverb | 0 | 4 | `kParamFreq` | 0 | 100 | 1 | 1 | 0 |  |
| 85 | FXReverb | 0 | 5 | `kParamFeedback` | 0 | 100 | 1 | 1 | 0 |  |
| 86 | FXReverb | 0 | 6 | `kParamFreqB` | 0 | 100 | 1 | 1 | 0 |  |
| 87 | FXReverb | 0 | 7 | `kParamWidth` | 0 | 100 | 1 | 1 | 0 |  |
| 88 | FXEQ | 0 | 1 | `kParamFreq1` | 21.53320125 | 20000 | 1 | 2 | 0 |  |
| 89 | FXEQ | 0 | 2 | `kParamFreq2` | 21.53320125 | 20000 | 1 | 2 | 0 |  |
| 90 | FXEQ | 0 | 3 | `kParamReso1` | 0 | 100 | 1 | 1 | 0 |  |
| 91 | FXEQ | 0 | 4 | `kParamReso2` | 0 | 100 | 1 | 1 | 0 |  |
| 92 | FXEQ | 0 | 5 | `kParamGain1` | -24 | 24 | 1 | 1 | 0 |  |
| 93 | FXEQ | 0 | 6 | `kParamGain2` | -24 | 24 | 1 | 1 | 0 |  |
| 94 | FXEQ | 0 | 7 | `kParamType1` | 0 | 2 | 1 | 1 | 2 |  |
| 95 | FXEQ | 0 | 8 | `kParamType2` | 0 | 2 | 1 | 1 | 2 |  |
| 96 | FXDistortion | 0 | 1 | `kParamWet` | 0 | 100 | 1 | 1 | 0 |  |
| 97 | FXDistortion | 0 | 2 | `kParamDrive` | 0 | 100 | 1 | 1 | 0 |  |
| 98 | FXDistortion | 0 | 3 | `kParamLPHP` | 0 | 100 | 1 | 1 | 0 |  |
| 99 | FXDistortion | 0 | 4 | `kParamMode` | 0 | 15 | 1 | 1 | 15 | `kTube`, `kSoftClip`, `kHardClip`, `kDiode1`, `kDiode2`, `kLinFold`, `kSinFold`, `kZeroSquare`, `kDownsample`, `kAsym`, `kRectify`, `kXShaper`, `kXShaperAsym`, `kSineShaper`, `kStompBox`, `kTapeSat` |
| 100 | FXDistortion | 0 | 5 | `kParamFreq` | 0 | 1 | 1 | 0 | 0 |  |
| 101 | FXDistortion | 0 | 6 | `kParamBW` | 0.075 | 7.575 | 2 | 1 | 0 |  |
| 102 | FXDistortion | 0 | 7 | `kParamPrePost` | 0 | 2 | 1 | 1 | 2 |  |
| 103 | FXFlanger | 0 | 1 | `kParamWet` | 0 | 100 | 1 | 1 | 0 |  |
| 104 | FXFlanger | 0 | 2 | `kParamBeatSync` | 0 | 1 | 1 | 1 | 1 |  |
| 105 | FXFlanger | 0 | 3 | `kParamRate` | 0 | 20 | 4 | 1 | 0 |  |
| 106 | FXFlanger | 0 | 4 | `kParamDepth` | 0 | 100 | 1 | 1 | 0 |  |
| 107 | FXFlanger | 0 | 5 | `kParamFeedback` | 0 | 100 | 1 | 1 | 0 |  |
| 108 | FXFlanger | 0 | 6 | `kParamWidth` | 0 | 360 | 1 | 1 | 0 |  |
| 109 | FXPhaser | 0 | 1 | `kParamWet` | 0 | 100 | 1 | 1 | 0 |  |
| 110 | FXPhaser | 0 | 2 | `kParamBeatSync` | 0 | 1 | 1 | 1 | 1 |  |
| 111 | FXPhaser | 0 | 3 | `kParamRate` | 0 | 20 | 4 | 1 | 0 |  |
| 112 | FXPhaser | 0 | 4 | `kParamDepth` | 0 | 100 | 1 | 1 | 0 |  |
| 113 | FXPhaser | 0 | 6 | `kParamFreq` | 20 | 18000 | 1 | 2 | 0 |  |
| 114 | FXPhaser | 0 | 7 | `kParamFeedback` | 0 | 100 | 1 | 1 | 0 |  |
| 115 | FXPhaser | 0 | 8 | `kParamWidth` | 0 | 360 | 1 | 1 | 0 |  |
| 116 | FXChorus | 0 | 1 | `kParamWet` | 0 | 100 | 1 | 1 | 0 |  |
| 117 | FXChorus | 0 | 2 | `kParamBeatSync` | 0 | 1 | 1 | 1 | 1 |  |
| 118 | FXChorus | 0 | 3 | `kParamRate` | 0 | 20 | 4 | 1 | 0 |  |
| 119 | FXChorus | 0 | 4 | `kParamDelay` | 0 | 20 | 2 | 1 | 0 |  |
| 120 | FXChorus | 0 | 5 | `kParamDelay2` | 0 | 20 | 2 | 1 | 0 |  |
| 121 | FXChorus | 0 | 6 | `kParamDepth` | 0 | 26 | 2 | 1 | 0 |  |
| 122 | FXChorus | 0 | 7 | `kParamFeedback` | 0 | 95 | 1 | 1 | 0 |  |
| 123 | FXChorus | 0 | 8 | `kParamFilt` | 50 | 20000 | 1 | 2 | 0 |  |
| 124 | FXDelay | 0 | 1 | `kParamWet` | 0 | 100 | 1 | 1 | 0 |  |
| 125 | FXDelay | 0 | 2 | `kParamFreq` | 40 | 18000 | 1 | 2 | 0 |  |
| 126 | FXDelay | 0 | 3 | `kParamBW` | 0.75 | 8.25 | 1 | 1 | 0 |  |
| 127 | FXDelay | 0 | 4 | `kParamBeatSync` | 0 | 1 | 1 | 1 | 1 |  |
| 128 | FXDelay | 0 | 5 | `kParamLink` | 0 | 1 | 1 | 1 | 1 |  |
| 129 | FXDelay | 0 | 6 | `kParamTimeL` | 0.001 | 0.501 | 4 | 1 | 0 |  |
| 130 | FXDelay | 0 | 7 | `kParamTimeR` | 0.001 | 0.501 | 4 | 1 | 0 |  |
| 131 | FXDelay | 0 | 8 | `kParamMode` | 0 | 2 | 1 | 1 | 2 |  |
| 132 | FXDelay | 0 | 9 | `kParamFeedback` | 0 | 100 | 1 | 1 | 0 |  |
| 133 | FXDelay | 0 | 10 | `kParamOffsetL` | 0.5 | 1.5 | 1 | 1 | 0 |  |
| 134 | FXDelay | 0 | 11 | `kParamOffsetR` | 0.5 | 1.5 | 1 | 1 | 0 |  |
| 135 | FXComp | 0 | 2 | `kParamThresh` | 0 | 1 | 1 | 0 | 0 |  |
| 136 | FXComp | 0 | 3 | `kParamRatio` | 1 | 1000000 | 1 | 3 | 0 |  |
| 137 | FXComp | 0 | 4 | `kParamAttack` | 0.1000000015 | 1000 | 2 | 1 | 0 |  |
| 138 | FXComp | 0 | 5 | `kParamRelease` | 0.1000000015 | 1000 | 2 | 1 | 0 |  |
| 139 | FXComp | 0 | 6 | `kParamMakeup` | 1 | 31 | 2 | 1 | 0 |  |
| 140 | FXComp | 0 | 7 | `kParamMultiband` | 0 | 1 | 1 | 1 | 1 |  |
| 141 | FXFilter | 0 | 1 | `kParamWet` | 0 | 100 | 1 | 1 | 0 |  |
| 142 | FXFilter | 0 | 2 | `kParamType` | 0 | 95 | 1 | 1 | 95 | `MgL6`, `MgL12`, `MgL18`, `MgL24`, `L6`, `L12`, `L18`, `L24`, `H6`, `H12`, `H18`, `H24`, `B12`, `B24`, `P12`, `P24`, `N12`, `N24`, `LH6`, `LH12`, `LB12`, `LP12`, `LN12`, `HB12`, `HP12`, `HN12`, `BP12`, `BN12`, `PP12`, `PN12`, `NN12`, `LBH12`, `LBH24`, `LPH12`, `LPH24`, `LNH12`, `LNH24`, `BPN12`, `BPN24`, `CombP`, `CombN`, `CombL6P`, `CombL6N`, `CombH6P`, `CombH6N`, `CombHL6P`, `CombHL6N`, `FlangeP`, `FlangeN`, `FlangeL6P`, `FlangeL6N`, `FlangeH6P`, `FlangeH6N`, `FlangeHL6P`, `FlangeHL6N`, `Phase12P`, `Phase12N`, `Phase24P`, `Phase24N`, `Phase36P`, `Phase36N`, `Phase48P`, `Phase48N`, `Phase48L6P`, `Phase48L6N`, `Phase48H6P`, `Phase48H6N`, `Phase48HL6P`, `Phase48HL6N`, `FlangePhase12HL6P`, `FlangePhase12HL6N`, `LEQ6`, `LEQ12`, `BEQ12`, `HEQ6`, `HEQ12`, `RM`, `RMT`, `SNH1`, `SNH2`, `Combs`, `Allpasses`, `Reverb1`, `Scream`, `ZDF_A`, `ADD_BASS`, `FormantONE`, `FormantTWO`, `FormantTWB`, `BandReject`, `DistComb1LP`, `DistComb1BP`, `DistComb2LP`, `DistComb2BP`, `Scream3LP`, `Scream3BP` |
| 143 | FXFilter | 0 | 3 | `kParamFreq` | 0 | 1 | 1 | 0 | 0 |  |
| 144 | FXFilter | 0 | 4 | `kParamReso` | 0 | 100 | 1 | 1 | 0 |  |
| 145 | FXFilter | 0 | 5 | `kParamDrive` | 0 | 100 | 1 | 1 | 0 |  |
| 146 | FXFilter | 0 | 6 | `kParamVar` | 0 | 100 | 1 | 1 | 0 |  |
| 147 | FXHyperD | 0 | 1 | `kParamWet` | 0 | 100 | 1 | 1 | 0 |  |
| 148 | FXHyperD | 0 | 2 | `kParamRate` | 0 | 100 | 1 | 1 | 0 |  |
| 149 | FXHyperD | 0 | 3 | `kParamDetune` | 0 | 100 | 1 | 1 | 0 |  |
| 150 | FXHyperD | 0 | 4 | `kParamUnison` | 0 | 7 | 1 | 1 | 0 |  |
| 151 | FXHyperD | 0 | 5 | `kParamRetrig` | 0 | 1 | 1 | 1 | 1 |  |
| 152 | FXHyperD | 0 | 7 | `kParamDimESize` | 0 | 100 | 1 | 1 | 0 |  |
| 153 | FXHyperD | 0 | 8 | `kParamDimEWet` | 0 | 100 | 1 | 1 | 0 |  |
| 154 | FXDistortion | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 155 | FXFlanger | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 156 | FXPhaser | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 157 | FXChorus | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 158 | FXDelay | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 159 | FXComp | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 160 | FXReverb | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 161 | FXEQ | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 162 | FXFilter | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 163 | FXHyperD | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 164 | Oscillator | 0 | 9 | `kParamPitchTrack` | 0 | 1 | 1 | 1 | 1 |  |
| 165 | Oscillator | 1 | 9 | `kParamPitchTrack` | 0 | 1 | 1 | 1 | 1 |  |
| 166 | Global | 0 | 5 | `kParamBendRangeUp` | -24 | 24 | 1 | 1 | 48 |  |
| 167 | Global | 0 | 6 | `kParamBendRangeDn` | -24 | 24 | 1 | 1 | 48 |  |
| 168 | WTOsc | 0 | 2 | `kParamWarpMenu` | 0 | 22 | 1 | 1 | 22 | `kNoWarp`, `kHardSync`, `kSoftSync`, `kSofterSync`, `kBendPos`, `kBendNeg`, `kBendPosNeg`, `kPWM`, `kASYMPos`, `kASYMNeg`, `kASYMPosNeg`, `kFlip`, `kDLM`, `kRemap_1`, `kRemap_2`, `kRemap_3`, `kRemap_4`, `kQuantize`, `kPD_OSC`, `kAM_OSC`, `kRM_OSC`, `kFM_NOISE`, `kFM_SUB` |
| 169 | WTOsc | 1 | 2 | `kParamWarpMenu` | 0 | 22 | 1 | 1 | 22 | `kNoWarp`, `kHardSync`, `kSoftSync`, `kSofterSync`, `kBendPos`, `kBendNeg`, `kBendPosNeg`, `kPWM`, `kASYMPos`, `kASYMNeg`, `kASYMPosNeg`, `kFlip`, `kDLM`, `kRemap_1`, `kRemap_2`, `kRemap_3`, `kRemap_4`, `kQuantize`, `kPD_OSC`, `kAM_OSC`, `kRM_OSC`, `kFM_NOISE`, `kFM_SUB` |
| 170 | SubOsc | 4 | 0 | `kParamShape` | 0 | 5 | 1 | 1 | 5 | `kSine`, `kRoundRect`, `kTriangle`, `kSaw`, `kSquare`, `kPulse` |
| 171 | Oscillator | 4 | 3 | `kParamOctave` | -4 | 4 | 1 | 1 | 8 |  |
| 172 | Oscillator | 0 | 28 | `kParamUnisonStereo` | 0 | 100 | 1 | 1 | 0 |  |
| 173 | Oscillator | 1 | 28 | `kParamUnisonStereo` | 0 | 100 | 1 | 1 | 0 |  |
| 174 | Oscillator | 0 | 31 | `kParamUnisonWarp` | -100 | 100 | 1 | 1 | 0 |  |
| 175 | Oscillator | 1 | 31 | `kParamUnisonWarp` | -100 | 100 | 1 | 1 | 0 |  |
| 176 | WTOsc | 0 | 7 | `kParamUnisonWTPos` | -100 | 100 | 1 | 1 | 0 |  |
| 177 | WTOsc | 1 | 7 | `kParamUnisonWTPos` | -100 | 100 | 1 | 1 | 0 |  |
| 178 | Oscillator | 0 | 25 | `kParamUnisonStack` | 0 | 8 | 1 | 1 | 8 | `kNoUnisonStack`, `kOctave1`, `kOctave2`, `kOctave3`, `kOctaveFifth1`, `kOctaveFifth2`, `kOctaveFifth3`, `kCenter12`, `kCenter24` |
| 179 | Oscillator | 1 | 25 | `kParamUnisonStack` | 0 | 8 | 1 | 1 | 8 | `kNoUnisonStack`, `kOctave1`, `kOctave2`, `kOctave3`, `kOctaveFifth1`, `kOctaveFifth2`, `kOctaveFifth3`, `kCenter12`, `kCenter24` |
| 180 | ModSlot | 0 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 181 | ModSlot | 0 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 182 | ModSlot | 1 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 183 | ModSlot | 1 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 184 | ModSlot | 2 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 185 | ModSlot | 2 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 186 | ModSlot | 3 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 187 | ModSlot | 3 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 188 | ModSlot | 4 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 189 | ModSlot | 4 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 190 | ModSlot | 5 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 191 | ModSlot | 5 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 192 | ModSlot | 6 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 193 | ModSlot | 6 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 194 | ModSlot | 7 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 195 | ModSlot | 7 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 196 | ModSlot | 8 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 197 | ModSlot | 8 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 198 | ModSlot | 9 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 199 | ModSlot | 9 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 200 | ModSlot | 10 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 201 | ModSlot | 10 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 202 | ModSlot | 11 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 203 | ModSlot | 11 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 204 | ModSlot | 12 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 205 | ModSlot | 12 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 206 | ModSlot | 13 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 207 | ModSlot | 13 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 208 | ModSlot | 14 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 209 | ModSlot | 14 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 210 | ModSlot | 15 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 211 | ModSlot | 15 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 212 | Oscillator | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 213 | Oscillator | 1 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 214 | Oscillator | 3 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 215 | Oscillator | 4 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 216 | VoiceFilter | 0 | 0 | `kParamEnable` | 0 | 1 | 1 | 1 | 1 |  |
| 217 | Global | 0 | 8 | `kParamModWheel` | 0 | 100 | 1 | 1 | 0 |  |
| 218 | Macro | 0 | 0 | `kParamValue` | 0 | 100 | 1 | 1 | 0 |  |
| 219 | Macro | 1 | 0 | `kParamValue` | 0 | 100 | 1 | 1 | 0 |  |
| 220 | Macro | 2 | 0 | `kParamValue` | 0 | 100 | 1 | 1 | 0 |  |
| 221 | Macro | 3 | 0 | `kParamValue` | 0 | 100 | 1 | 1 | 0 |  |
| 222 | Global | 0 | 2 | `kParamVoiceAmp` | 0 | 1 | 1 | 0 | 0 |  |
| 223 | LFO | 0 | 1 | `kParamSmooth` | 0 | 100 | 1 | 1 | 0 |  |
| 224 | LFO | 1 | 1 | `kParamSmooth` | 0 | 100 | 1 | 1 | 0 |  |
| 225 | LFO | 2 | 1 | `kParamSmooth` | 0 | 100 | 1 | 1 | 0 |  |
| 226 | LFO | 3 | 1 | `kParamSmooth` | 0 | 100 | 1 | 1 | 0 |  |
| 227 | Global | 0 | 7 | `kParamPitchBendAuto` | 0 | 1 | 1 | 0 | 0 |  |
| 228 | ModSlot | 16 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 229 | ModSlot | 16 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 230 | ModSlot | 17 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 231 | ModSlot | 17 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 232 | ModSlot | 18 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 233 | ModSlot | 18 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 234 | ModSlot | 19 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 235 | ModSlot | 19 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 236 | ModSlot | 20 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 237 | ModSlot | 20 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 238 | ModSlot | 21 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 239 | ModSlot | 21 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 240 | ModSlot | 22 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 241 | ModSlot | 22 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 242 | ModSlot | 23 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 243 | ModSlot | 23 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 244 | ModSlot | 24 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 245 | ModSlot | 24 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 246 | ModSlot | 25 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 247 | ModSlot | 25 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 248 | ModSlot | 26 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 249 | ModSlot | 26 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 250 | ModSlot | 27 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 251 | ModSlot | 27 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 252 | ModSlot | 28 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 253 | ModSlot | 28 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 254 | ModSlot | 29 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 255 | ModSlot | 29 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 256 | ModSlot | 30 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 257 | ModSlot | 30 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 258 | ModSlot | 31 | 0 | `kParamAmount` | -100 | 100 | 1 | 1 | 0 |  |
| 259 | ModSlot | 31 | 1 | `kParamOut` | 0 | 100 | 1 | 1 | 0 |  |
| 260 | LFO | 4 | 0 | `kParamRate` | 0 | 100 | 4 | 1 | 0 |  |
| 261 | LFO | 5 | 0 | `kParamRate` | 0 | 100 | 4 | 1 | 0 |  |
| 262 | LFO | 6 | 0 | `kParamRate` | 0 | 100 | 4 | 1 | 0 |  |
| 263 | LFO | 7 | 0 | `kParamRate` | 0 | 100 | 4 | 1 | 0 |  |
| 264 | LFO | 4 | 1 | `kParamSmooth` | 0 | 100 | 1 | 1 | 0 |  |
| 265 | LFO | 5 | 1 | `kParamSmooth` | 0 | 100 | 1 | 1 | 0 |  |
| 266 | LFO | 6 | 1 | `kParamSmooth` | 0 | 100 | 1 | 1 | 0 |  |
| 267 | LFO | 7 | 1 | `kParamSmooth` | 0 | 100 | 1 | 1 | 0 |  |
| 268 | FXFilter | 0 | 7 | `kParamStereo` | 0 | 100 | 1 | 1 | 0 |  |
| 269 | FXComp | 0 | 1 | `kParamWet` | 0 | 100 | 1 | 1 | 0 |  |
| 270 | FXComp | 0 | 8 | `kParamThreshUD0` | 0 | 200 | 1 | 1 | 0 |  |
| 271 | FXComp | 0 | 9 | `kParamThreshUD1` | 0 | 200 | 1 | 1 | 0 |  |
| 272 | FXComp | 0 | 10 | `kParamThreshUD2` | 0 | 200 | 1 | 1 | 0 |  |
| 273 | LFO | 0 | 2 | `kParamRise` | 0 | 4 | 1 | 1 | 0 |  |
| 274 | LFO | 1 | 2 | `kParamRise` | 0 | 4 | 1 | 1 | 0 |  |
| 275 | LFO | 2 | 2 | `kParamRise` | 0 | 4 | 1 | 1 | 0 |  |
| 276 | LFO | 3 | 2 | `kParamRise` | 0 | 4 | 1 | 1 | 0 |  |
| 277 | LFO | 4 | 2 | `kParamRise` | 0 | 4 | 1 | 1 | 0 |  |
| 278 | LFO | 5 | 2 | `kParamRise` | 0 | 4 | 1 | 1 | 0 |  |
| 279 | LFO | 6 | 2 | `kParamRise` | 0 | 4 | 1 | 1 | 0 |  |
| 280 | LFO | 7 | 2 | `kParamRise` | 0 | 4 | 1 | 1 | 0 |  |
| 281 | LFO | 0 | 3 | `kParamDelay` | 0 | 4 | 1 | 1 | 0 |  |
| 282 | LFO | 1 | 3 | `kParamDelay` | 0 | 4 | 1 | 1 | 0 |  |
| 283 | LFO | 2 | 3 | `kParamDelay` | 0 | 4 | 1 | 1 | 0 |  |
| 284 | LFO | 3 | 3 | `kParamDelay` | 0 | 4 | 1 | 1 | 0 |  |
| 285 | LFO | 4 | 3 | `kParamDelay` | 0 | 4 | 1 | 1 | 0 |  |
| 286 | LFO | 5 | 3 | `kParamDelay` | 0 | 4 | 1 | 1 | 0 |  |
| 287 | LFO | 6 | 3 | `kParamDelay` | 0 | 4 | 1 | 1 | 0 |  |
| 288 | LFO | 7 | 3 | `kParamDelay` | 0 | 4 | 1 | 1 | 0 |  |
| 289 | FXDistortion | 0 | 8 | `kParamLevelOut` | 0 | 1 | 1 | 0 | 0 |  |
| 290 | FXFlanger | 0 | 7 | `kParamLevelOut` | 0 | 1 | 1 | 0 | 0 |  |
| 291 | FXPhaser | 0 | 9 | `kParamLevelOut` | 0 | 1 | 1 | 0 | 0 |  |
| 292 | FXChorus | 0 | 9 | `kParamLevelOut` | 0 | 1 | 1 | 0 | 0 |  |
| 293 | FXDelay | 0 | 12 | `kParamLevelOut` | 0 | 1 | 1 | 0 | 0 |  |
| 294 | FXComp | 0 | 11 | `kParamLevelOut` | 0 | 1 | 1 | 0 | 0 |  |
| 295 | FXReverb | 0 | 10 | `kParamLevelOut` | 0 | 1 | 1 | 0 | 0 |  |
| 296 | FXHyperD | 0 | 9 | `kParamDimELevelOut` | 0 | 1 | 1 | 0 | 0 |  |
| 297 | FXFilter | 0 | 8 | `kParamLevelOut` | 0 | 1 | 1 | 0 | 0 |  |
| 298 | FXHyperD | 0 | 6 | `kParamLevelOut` | 0 | 1 | 1 | 0 | 0 |  |
| 299 | LFOPointModBus | 0 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 300 | LFOPointModBus | 1 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 301 | LFOPointModBus | 2 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 302 | LFOPointModBus | 3 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 303 | LFOPointModBus | 4 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 304 | LFOPointModBus | 5 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 305 | LFOPointModBus | 6 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 306 | LFOPointModBus | 7 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 307 | LFOPointModBus | 8 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 308 | LFOPointModBus | 9 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 309 | LFOPointModBus | 10 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 310 | LFOPointModBus | 11 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 311 | LFOPointModBus | 12 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 312 | LFOPointModBus | 13 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 313 | LFOPointModBus | 14 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 314 | LFOPointModBus | 15 | 0 | `kParamValue` | 0 | 1 | 1 | 0 | 0 |  |
| 315 | — | 0 | -1 | `—` | 0 | 1 | 1 | 0 | 0 |  |
| 316 | — | 0 | -1 | `—` | 0 | 1 | 1 | 0 | 0 |  |
| 317 | RoutingSlot | 4 | 3 | `kParamRoutingDest` | 0 | 3 | 1 | 1 | 3 | `kRoutingDestFilter`, `kRoutingDestMaster`, `kRoutingDestDirect`, `kRoutingDestNone` |
| 318 | Global | 0 | 23 | `kParamGlobalTuning` | 430 | 450 | 1 | 1 | 0 |  |
| 319 | RoutingSlot | 3 | 3 | `kParamRoutingDest` | 0 | 3 | 1 | 1 | 3 | `kRoutingDestFilter`, `kRoutingDestMaster`, `kRoutingDestDirect`, `kRoutingDestNone` |
| 320 | Oscillator | 0 | 43 | `kParamDetuneMode` | 0 | 4 | 1 | 1 | 4 | `kDetuneLinear`, `kDetuneSuper`, `kDetuneExp`, `kDetuneInv`, `kDetuneRandom` |
| 321 | Oscillator | 1 | 43 | `kParamDetuneMode` | 0 | 4 | 1 | 1 | 4 | `kDetuneLinear`, `kDetuneSuper`, `kDetuneExp`, `kDetuneInv`, `kDetuneRandom` |
| 322 | Global | 0 | 10 | `kParamMonoToggle` | 0 | 1 | 1 | 1 | 1 |  |
| 323 | Global | 0 | 11 | `kParamLegato` | 0 | 1 | 1 | 1 | 1 |  |
| 324 | Global | 0 | 12 | `kParamPortaAlways` | 0 | 1 | 1 | 1 | 1 |  |
| 325 | Global | 0 | 13 | `kParamPortaScaled` | 0 | 1 | 1 | 1 | 1 |  |
| 326 | Global | 0 | 22 | `kParamOversampling` | 0 | 2 | 1 | 1 | 2 |  |
| 327 | NoiseOsc | 3 | 4 | `kParamOneShot` | 0 | 1 | 1 | 1 | 1 |  |
| 328 | Oscillator | 3 | 9 | `kParamPitchTrack` | 0 | 1 | 1 | 1 | 1 |  |
| 329 | Global | 0 | 21 | `kParamPolyCount` | 1 | 32 | 1 | 1 | 31 |  |
| 330 | — | 0 | -1 | `—` | 0 | 1 | 1 | 0 | 0 |  |
| 331 | VoiceFilter | 0 | 11 | `kParamKeyTrack` | 0 | 1 | 1 | 1 | 1 |  |
| 332 | Oscillator | 0 | 44 | `kParamUnisonRange` | 0 | 48 | 1 | 1 | 48 |  |
| 333 | Oscillator | 1 | 44 | `kParamUnisonRange` | 0 | 48 | 1 | 1 | 48 |  |
| 334 | LFO | 8 | 12 | `kParamMono` | 0 | 1 | 1 | 1 | 1 |  |
| 335 | LFO | 9 | 12 | `kParamMono` | 0 | 1 | 1 | 1 | 1 |  |
| 336 | FXChorus | 0 | 10 | `kParamFiltMode` | 0 | 1 | 1 | 1 | 1 |  |
| 337 | — | 0 | -1 | `—` | 0 | 1 | 1 | 0 | 0 |  |
| 338 | — | 0 | -1 | `—` | 0 | 1 | 1 | 0 | 0 |  |
| 339 | — | 0 | -1 | `—` | 0 | 1 | 1 | 0 | 0 |  |
| 340 | Global | 0 | 24 | `kParamNoteLatch` | 0 | 1 | 1 | 1 | 1 |  |
| 341 | FXReverb | 0 | 15 | `kParamType` | 0 | 1 | 1 | 1 | 1 | `kPlate`, `kHall` |
| 342 | — | 0 | -1 | `—` | 0 | 1 | 1 | 0 | 0 |  |

## 3. Source-enum name table — RVA 0xA20820 (static, image-internal pointers)

Array of 59 `const char*` (8 bytes each, 0xA20820…0xA20A47), indexed by
`id = index + 1`. Verified byte-identical between the on-disk `.rdata` and
the runtime image (it is *not* relocated — plain `.rdata` string pointers,
already valid pre-`InitDll`). This is the S1 mod-matrix **source-code → name**
table; it matches the list previously derived from static RE, field for field:

| id | name |
|---|---|
| 1 | `Mod Wheel` |
| 2 | `Env 1` |
| 3 | `Env 2` |
| 4 | `Env 3` |
| 5 | `Env 4` |
| 6 | `LFO 1` |
| 7 | `LFO 2` |
| 8 | `LFO 3` |
| 9 | `LFO 4` |
| 10 | `LFO 5` |
| 11 | `LFO 6` |
| 12 | `LFO 7` |
| 13 | `LFO 8` |
| 14 | `LFO 9` |
| 15 | `LFO 10` |
| 16 | `Velo` |
| 17 | `Note#` |
| 18 | `Aftertouch` |
| 19 | `Poly Aftertch` |
| 20 | `Noise OSC` |
| 21 | `NoteOn Rand1` |
| 22 | `NoteOn Rand2` |
| 23 | `NoteOn Alt.` |
| 24 | `NoteOn Alt.2` |
| 25 | `Macro 1` |
| 26 | `Macro 2` |
| 27 | `Macro 3` |
| 28 | `Macro 4` |
| 29 | `Macro 5` |
| 30 | `Macro 6` |
| 31 | `Macro 7` |
| 32 | `Macro 8` |
| 33 | `Pitch Bend` |
| 34 | `Expr X (Pan)` |
| 35 | `Expr Y (Timbre)` |
| 36 | `Expr Z (Press.)` |
| 37 | `Release Velo` |
| 38 | `Fixed` |
| 39 | `LFO 1 Y` |
| 40 | `LFO 2 Y` |
| 41 | `LFO 3 Y` |
| 42 | `LFO 4 Y` |
| 43 | `LFO 5 Y` |
| 44 | `LFO 6 Y` |
| 45 | `LFO 7 Y` |
| 46 | `LFO 8 Y` |
| 47 | `LFO 9 Y` |
| 48 | `LFO 10 Y` |
| 49 | `OSC A` |
| 50 | `OSC B` |
| 51 | `OSC C` |
| 52 | `SUB OSC` |
| 53 | `Filter 1` |
| 54 | `Filter 2` |
| 55 | `Active Voices` |
| 56 | `Voice Mod 1` |
| 57 | `Voice Mod 2` |
| 58 | `Voice Index` |
| 59 | `NoteOn Rand (Discrete)` |

## 4. Enum-string option tables (the runtime "enum" lists)

The importer's enum handling is *entirely* driven by the option vectors of §2
(`fn_4d9da0`'s options path, `s1load.txt`-external helper `0x92170` builds the
JSON string from `*(char**)elem`). Complete lists resolved at runtime (18
params carry lists; strings are pointers into the image string pool):

- **idx 99 `FXDistortion.kParamMode`** (16): `kTube kSoftClip kHardClip kDiode1 kDiode2 kLinFold kSinFold kZeroSquare kDownsample kAsym kRectify kXShaper kXShaperAsym kSineShaper kStompBox kTapeSat` — S1 master param 0x63 = distortion type; the importer's special case (`s1load.txt` 0x4F13C0: `kParamType=="kDiode1"` → `kParamDrive = v·0.875 + 12.5`) matches `kDiode1` here.
- **idx 44 `VoiceFilter0.kParamType` / idx 142 `FXFilter0.kParamType`** (96): `MgL6 … Scream3BP` (full list inline in §2's table).
- **idx 168/169 `WTOsc0/1.kParamWarpMenu`** (23): `kNoWarp … kFM_SUB`.
- **idx 170 `SubOsc.kParamShape`** (6): `kSine kRoundRect kTriangle kSaw kSquare kPulse`.
- **idx 178/179 `Oscillator0/1.kParamUnisonStack`** (9): `kNoUnisonStack … kCenter24`.
- **idx 320/321 `Oscillator0/1.kParamDetuneMode`** (5): `kDetuneLinear kDetuneSuper kDetuneExp kDetuneInv kDetuneRandom`.
- **idx 40/41/42/43/317/319 `RoutingSlot<N>.kParamRoutingDest`** (4): `kRoutingDestFilter kRoutingDestMaster kRoutingDestDirect kRoutingDestNone`.
- **idx 341 `FXReverb0.kParamType`** (2): `kPlate kHall` — the strings live at RVA 0xF8B0A8 (`kPlate`) and 0xF8B0B4/0xF9169A (`kHall`); the importer's reverb special case (`s1load.txt` 0x4F11F9, FX family 6 / idx 0x53) compares the cell's `kParamType` against `kPlate` before writing `kParamPreDelay`.

The `mixOrGain` **key strings** (S1 FX-slot mix/gain block writer,
`s1load.txt` 0x4E008F–0x4E02AD) are `mixOrGain1`…`mixOrGain10`:
`"mixOrGain1"` is a literal at RVA 0xF9C8C7 (loaded by `lea r12, [rip+…]` at
0x4E00AC), `"mixOrGain2"` at 0xF9C3BD; keys 3…10 are built at runtime by
appending the digit to the 9-char stem (string append loop at 0x4E013D–0x4E01F6).
Per slot `i` (0…9) the writer:
- when `version ≥ 0.05` **and** `byte[state + 68·i + 0x3976] == 1`: calls
  `fn_setval(ctx, 0x9A+i, 0.0)` (the S2 FX-enable param of family `i`, §7 —
  i.e. the S1 byte that is 1 switches the FX **off**; S2 keeps its default
  enable otherwise);
- writes the mix byte `byte[state + 68·i + 0x3978]` as a JSON **int** into
  `"+ FX"[rackCell].mixOrGain(i+1)`;
- writes the gain `f32[state + 0x4BD4 + 4·i]` through `fn_setval(ctx, 0x121+i, v)`.

The `0x121+i` targets per family (from §2): 0x121 FXDistortion `kParamLevelOut`,
0x122 FXFlanger, 0x123 FXPhaser, 0x124 FXChorus, 0x125 FXDelay, 0x126 FXComp,
0x127 FXReverb, 0x128 FXHyperD `kParamDimELevelOut`, 0x129 FXFilter,
0x12A FXHyperD `kParamLevelOut` (family 7 = FXEQ has no entry; family 9
appears twice).

Neighbouring string-pool literals used by the importer (from
`strings_pool.json`): `kDiode1` 0xF9C940 / `kDiode2` 0xF9C479 (distortion
special case), `Reverb1` 0xF9C963, `Mod1/2` 0xF9C948/0xF9C49B,
`NoteOn Rand1/2` 0xF9C94D/0xF9C4A0 (also source-enum names 21/22),
`kOctave1/2/3` `kOctaveFifth1/2/3` `kRemap_1/2/3/4` (option-string pools),
`SNH1` 0xF9C9C0. (The pool also contains unrelated look-alikes with trailing
digits — MSVC literal pooling; the meaningful importer strings are the ones
above and in §2's `options` columns.)

## 5. Defaults table — RVA 0x179F7A0 (31 × stride 0x50)

Same 0x50 layout as §1 (only the +0x00 f64 is consumed). The importer's
defaults loop (`s1load.txt` 0x4DBC15–0x4DBC82: `add rax, 0x50`, `cmp rcx, 0x1f`)
converts each f64 → f32 and stores it at `state + i*4 + 0x4AE0` for
`i = 0…0x1E` (31 f32; a compiler-generated special-case chain on
`i+0xE4 ∈ {0x1B, 0x28, 0x29, 0x2A, 0xD4}` is unreachable for that range).
A separate loop (`s1load.txt` 0x4DD6D5–0x4DD71C) then pushes
`fn_setval(ctx, i+0xE4, f32[state + i*4 + 0x4AE0])` for `i = 0x20…0x72` —
i.e. S2 indices 0x104…0x156 are fed from the *tail* of the `state+0x4AE0`
block (which the version-gated migrations fill).
Dumped values (all default to alternating 0.0/1.0):

| i | state+0x4AE0+4i default (f64) |
|---|---|
| 0 | 0 |
| 1 | 1 |
| 2 | 0 |
| 3 | 1 |
| 4 | 0 |
| 5 | 1 |
| 6 | 0 |
| 7 | 1 |
| 8 | 0 |
| 9 | 1 |
| 10 | 0 |
| 11 | 1 |
| 12 | 0 |
| 13 | 1 |
| 14 | 0 |
| 15 | 1 |
| 16 | 0 |
| 17 | 1 |
| 18 | 0 |
| 19 | 1 |
| 20 | 0 |
| 21 | 1 |
| 22 | 0 |
| 23 | 1 |
| 24 | 0 |
| 25 | 1 |
| 26 | 0 |
| 27 | 1 |
| 28 | 0 |
| 29 | 1 |
| 30 | 0 |

## 6. S1 source-code remap table — RVA 0xA55300 (static)

`aux = dword[0xA55300 + t*4]` for S1 source code `t ≤ 0x21` (`s1load.txt`
0x4DF501). 34 entries used; entries 34/35 are 0. Adjacent constants: env
scales `f32[0xA55390] = (8, 24, 8, 24)` and S1 param indices
`i32[0xA553A0] = (3, 4, 16, 17)` (the envelope loop, `s1load.txt` 0x4E3567).

| S1 source code t | aux (dword @0xA55300+4t) |
|---|---|
| 0 (0x0) | 0 |
| 1 (0x1) | 1 |
| 2 (0x2) | 2 |
| 3 (0x3) | 3 |
| 4 (0x4) | 4 |
| 5 (0x5) | 6 |
| 6 (0x6) | 7 |
| 7 (0x7) | 8 |
| 8 (0x8) | 9 |
| 9 (0x9) | 10 |
| 10 (0xa) | 11 |
| 11 (0xb) | 12 |
| 12 (0xc) | 13 |
| 13 (0xd) | 16 |
| 14 (0xe) | 17 |
| 15 (0xf) | 18 |
| 16 (0x10) | 19 |
| 17 (0x11) | 14 |
| 18 (0x12) | 15 |
| 19 (0x13) | 20 |
| 20 (0x14) | 21 |
| 21 (0x15) | 22 |
| 22 (0x16) | 23 |
| 23 (0x17) | 24 |
| 24 (0x18) | 25 |
| 25 (0x19) | 26 |
| 26 (0x1a) | 27 |
| 27 (0x1b) | 28 |
| 28 (0x1c) | 33 |
| 29 (0x1d) | 34 |
| 30 (0x1e) | 35 |
| 31 (0x1f) | 36 |
| 32 (0x20) | 37 |
| 33 (0x21) | 38 |

## 7. FX family id — `fn_4d9a90` (RVA 0x4D9A90)

Pure logic, no table (**correction** vs `s1-to-s2-mapping.md` §5, which
attributed this mapping to table +0x4C): given the S2/S1 index, returns the FX
**family id** used by the rack-cell lookups (`state + fam*4 + 0x3BE0` in
`fn_setval`, `s1load.txt` 0x4DFAA9) and by `fn_setval`'s FX-knob branch, or
−1 for non-FX params. Fully disassembled 0x4D9A90–0x4D9BA5; the static rule
matches the runtime calls (`fxtype` column of §2) for all 343 indices:

| family | submap | S1/S2 idx ranges |
|---|---|---|
| 0 | FXDistortion | 0x60-0x66, 0x9A, 0x121 |
| 1 | FXFlanger | 0x67-0x6C, 0x9B, 0x122 |
| 2 | FXPhaser | 0x6D-0x73, 0x9C, 0x123 |
| 3 | FXChorus | 0x74-0x7B, 0x9D, 0x124, 0x150 |
| 4 | FXDelay | 0x7C-0x86, 0x9E, 0x125 |
| 5 | FXComp | 0x87-0x8C, 0x9F, 0x10D-0x110, 0x126 |
| 6 | FXReverb | 0x51-0x57, 0xA0, 0x127, 0x155 |
| 7 | FXEQ | 0x58-0x5F, 0xA1 |
| 8 | FXFilter | 0x8D-0x92, 0xA2, 0x10C, 0x129 |
| 9 | FXHyperD | 0x93-0x99, 0xA3, 0x128, 0x12A |

(Enable knobs sit at `0x9A + fam`; the S1 gain write `fn_setval(0x121+i, v)`
lands on the 0x121…0x12A rows of §2 — FXEQ has no level-out entry, FXHyperD
has two: 0x128 `kParamDimELevelOut` and 0x12A `kParamLevelOut`.)

## 8. Cross-checks (static ↔ runtime)

1. **destModuleParamID (+0x4C)** — `s1load.txt` (ModSlot node builder):
   ```asm
   0x004DF760  lea      rax, [rax + rax*4]
   0x004DF764  shl      rax, 4
   0x004DF768  lea      rcx, [rip + 0x12bb8f1]  ; ->0x179b060
   0x004DF76F  mov      eax, dword ptr [rax + rcx + 0x4c]
   ```
   Runtime: S1 dest code 0x13C (316) → `desc[316].pid = −1` (matches the
   hardcoded `d == 0x13C → destModuleParamID = −1` at 0x4DF6FD; entry 316 has no
   submap/name either); dest code 3 → `desc[3].pid = 3` with `submap = "Oscillator"`.
2. **destModuleTypeString (+0x40)** — `s1load.txt`:
   ```asm
   0x004DF95B  movsx    rax, word ptr [rdi + 0x1a]
   0x004DF960  lea      rax, [rax + rax*4]
   0x004DF964  shl      rax, 4
   0x004DF968  lea      rcx, [rip + 0x12bb6f1]  ; ->0x179b060
   0x004DF96F  add      rcx, rax
   0x004DF972  add      rcx, 0x40
   0x004DF984  call     0x92170                ; json string from const char*
   ```
   Runtime: `desc[3].submap = "Oscillator"`, `desc[0x13C].submap = null` — the
   +0x40 field is a `const char*` resolved at runtime (the static doc's
   "sub-engine name" reading).
3. **destModuleID for FX vs non-FX** — `s1load.txt`:
   ```asm
   0x004DFA90  call     0x4d9a90               ; fx family of dest code
   0x004DFA95  test     eax, eax
   0x004DFA97  js       0x4dfafc               ; fam < 0 → static id
   0x004DFAA9  movsxd   rax, dword ptr [rcx + rax*4 + 0x3be0]   ; rack cell
   ...
   0x004DFAFC  movsxd   rax, esi               ; (non-FX branch)
   0x004DFAFF  lea      rax, [rax + rax*4]
   0x004DFB03  shl      rax, 4
   0x004DFB12  lea      rcx, [rip + 0x12bb547]  ; ->0x179b060
   0x004DFB19  movsxd   rax, dword ptr [rax + rcx + 0x48]       ; desc[d].inst
   ```
   Runtime: `desc[3].inst = 0` (→ `Oscillator0`), `desc[16].inst = 1` (→
   `Oscillator1`), `desc[180].inst = 0` (`ModSlot0`), matching the §2 table.
4. **Source remap** — `s1load.txt`:
   ```asm
   0x004DF4F7  movzx    ecx, word ptr [rdi + 0x16]
   0x004DF4FB  cmp      rcx, 0x21
   0x004DF4FF  ja       0x4df50b
   0x004DF501  lea      rax, [rip + 0x575df8]  ; ->0xa55300
   0x004DF508  mov      eax, dword ptr [rax + rcx*4]
   ```
   Runtime bytes = static bytes (34 dwords, §6).
5. **Defaults** — `s1load.txt`:
   ```asm
   0x004DBC15  lea      rax, [rip + 0x12c3b84]  ; ->0x179f7a0
   0x004DBC20  movsd    xmm0, qword ptr [rax]
   0x004DBC2F  movss    dword ptr [rdx + rcx*4 + 0x4ae0], xmm0
   0x004DBC3B  add      rax, 0x50
   0x004DBC3F  cmp      rcx, 0x1f
   ```
   Runtime f64 values at 0x179F7A0 + i·0x50 match the §5 dump.

## 9. How a Rust implementation should consume these tables

Embed the §2 table (generated from `param_names.json`) as a `const` array of

```rust
struct S2ParamDesc {
    submap: &'static str,     // +0x40 — JSON section / "+ FX" sub-engine / destModuleTypeString
    inst: i32,                // +0x48 — instance number (Oscillator0..3, ModSlot0..31, …)
    pid: i32,                 // +0x4C — module-local param id (destModuleParamID); -1 none
    kname: &'static str,      // fn_4d93b0(idx) — plainParams key ("kParamXxx"); absent = drop
    default_: f64,            // +0x00
    min: f64, max: f64, step: f64,   // +0x08/+0x10/+0x18
    cnt: u32,                 // +0x20 — used by write-mode 0
    mode: u32,                // +0x24 — 0/1/2/3 (see §1 table)
    options: &'static [&'static str], // option list; value = option index
}
```

Pipeline (exactly what the importer does, all inputs now available):

1. `S1 master param i (0…247) → S2 idx` (identity; `+4` for pre-0.009 chunks),
   clamp to [0,1], NaN→0, then drop idx ∈ {315, 316, 341} (bitmask
   `0x8400003`) and 330 (explicit), drop anything with no descriptor/name
   (runtime-verified: 337, 338, 339, 342 have none — 315/316/330/341 would
   resolve anyway).
2. Unit-domain conversion (`fn_4d9c50`, jump tables 0xA563F8 / +0x140): pass
   through ×1.0 or scale by {8, 4, 2, 48, 15, 24, 95, 23, 22, 5, 31} then
   `round(v·N)/N` snap; one class ×0.01 with the `v > 0.01 ? v : 1.0` rule —
   constants already resolved in `s1-to-s2-mapping.md` §3.3.
3. Write the value through the §1 write-mode semantics (clamp → mode → option
   snap) into `plainParams[kname]`; the FX-knob branch addresses the cell as
   `"+ FX"[rackCell].<submap>.plainParams[kname]` where
   `rackCell = state+0x3BE0[fxtype]` (inverted S1 order table, §8) and
   `fxtype = fn_4d9a90(idx)` (§7).
4. Mod matrix: `dest code d → (submap, inst, pid) = desc[d]` fields; `pid == −1`
   (hardcoded for `d = 0x13C`, matching the table) marks the env-amount
   repoint slot; source names from §3 (`id = code + 1`), aux indices from §6.
5. Defaults for the `state+0x4AE0` block come from §5; mixOrGain keys/indices
   from §4.

What remains runtime-only / not extractable:

- The **full 2 623-parameter registry** (controller-wide `kParamXxx` names)
  belongs to the edit controller and is only enumerable through
  `createInstance` + `IEditController` — out of scope by rule. What the
  importer itself consumes is exactly the 343-entry subset in §2 (336 named),
  which is what `param_names.json` provides.
- Option-list **element pointers** are per-process heap values; the *strings*
  they point to (the authoritative content) are dumped in §2 and
  `fx_desc_table.json`.
- Everything else dumped here was fully populated by `InitDll` alone — no
  statically-null tables remained except the seven nameless rows above (which
  the importer itself treats as absent).

## 10. Artifact list

| file | content |
|---|---|
| `docs/s2-runtime-tables.md` | this document |
| `s2runtime/fx_desc_table.json` | §1/§2 — 343 entries: raw hex + parsed fields + resolved submap/kname/options |
| `s2runtime/source_enum.json` | §3 — 59 source ids → names |
| `s2runtime/param_names.json` | merged registry view (idx → kname/fxtype/submap/inst/pid/range/mode/options) |
| `s2runtime/misc_tables.json` | §5 defaults (31 f64), §6 remap (36 dwords), env scales + S1 param idx |
| `s2runtime/strings_pool.json` | §4 string pool (RVA → string), 0xF98000–0xFA2000, 1 796 strings |

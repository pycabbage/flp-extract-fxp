# Serum2 parameter corpus (native `.SerumPreset` factory presets)

Status: **measured** — all 626 native Serum2 factory presets
(`C:\Users\cabbage\Documents\Xfer\Serum 2 Presets\Presets\Factory\**\*.SerumPreset`,
Xfer 2.0.11–2.0.15 installs) parsed with the verified CBOR decoder
(`conv_work\scripts\cbor_probe.py`; zero unexplained bytes in every body).
Raw aggregate data: `conv_work\s2corpus\param_stats_raw.json` +
`s2_param_corpus.json` (the machine-readable corpus, implementation source for
the Rust converter). This doc is the human-readable knowledge base for
synthesizing a converted instance state (`serum2-state-format.md` §2: CBOR map
with 162 top-level keys). Read with `serum2-state-format.md` (state format),
`flp-serum2-conversion.md` (FLP container), `serum2-importer-analysis.md`
(import validation).

## 1. Corpus stats

| metric | value |
|---|---|
| presets parsed | **626 / 626** (0 failures, every body byte-accounted) |
| container (`XferJson`) size | 2 311 … 53 153 B |
| decompressed CBOR body | 9 566 … 459 632 B |
| JSON header `version` | 3.0 ×6, 4.0 ×543, 5.0 ×64, 6.0 ×13 |
| JSON header `productVersion` | 2.0.11 ×411, 2.0.12 ×169, 2.0.13 ×39, 2.0.14 ×4, 2.0.15 ×3 |
| top-level keys | standard set = **175** (×553); ×66 add `lfoPointModAssignments`, ×6 add `midiMap`, ×2 add `tuningData`+`tuningName`; ×5 omit the `GranularOsc`+`SpectralOsc` engine keys |
| `product` | `Serum2` ×624, `Serum2FX` ×2 (the two `SerumFX - *` presets) |

## 2. Container & JSON header (preset files)

```
offset  size  content
0x00    9     "XferJson\0"            (same magic as the VST3 state container)
0x09    8     u64 LE  json header length
0x11    ..    JSON header (keys sorted)
..      4     u32 LE  decompressed body size (exact, verified 626/626)
..      4     u32 LE  format = 2
..      ..    exactly one zstd frame (magic 28 B5 2F FD)
```

JSON header keys (all 626): `fileType` ("SerumPreset"), `hash` (md5 of the zstd
frame — same rule as the state container), `presetAuthor`, `presetDescription`,
`presetName`, `product` ("Serum2"/"Serum2FX"), `productVersion`, `tags`
(**array**), `url`, `vendor`, `version` (f64: 3.0–6.0). Example:

```json
{"fileType":"SerumPreset","hash":"d5a9c4fb4af5ec301d34f711fca17477",
  "presetAuthor":"Audiotent","presetDescription":"www.audiotent.com",
  "presetName":"ARP - Aardvark","product":"Serum2","productVersion":"2.0.13",
  "tags":["Wavetable","Mono","Arp","Preview"],
  "url":"https://xferrecords.com/","vendor":"Xfer Records","version":5.0}
```

`tags` is a **variable-length array** (2–4 entries): `[oscTypeBadge, voicing,
category, "Preview"]`. Observed values:

| slot | values (count) |
|---|---|
| 0 osc type badge | `Wavetable` 528, `Multisample` 155, `Sample` 72, `Granular` 56, `Spectral` 43 |
| 1 voicing | `Poly` 414, `Mono` 212 |
| 2 category | `Arp` 33, `Clip` 27, `Embedded-Data` 23, `KB-Span` 17, `Custom-Tuning` 2 (usually absent) |
| 3 | `Preview` 611 (usually absent) |

The VST3 **state** format (§2.2 of `serum2-state-format.md`) keeps only the
first **two** tags (`[badge, 'Poly'|'Mono']`).

## 3. Authored format ↔ instantiated state format (the mapping rule)

The authored `.SerumPreset` body is a CBOR map with **175 top-level keys**; the
VST3 instance state body is a CBOR map with **162 top-level keys**
(`serum2-state-format.md` §2.1–2.2). The two formats share the per-instance
section grammar; a converter instantiates a preset into a state like this:

**Identity rule — per-instance sections are copied 1:1.** The authored body
already contains the *same* per-instance sections the state format uses, with
the *same* sub-engine naming and the *same* `kParamXxx` records:

* `Oscillator0..4` = `map{ WTOsc<N>/MultiSampleOsc<N>/SampleOsc<N>/SpectralOsc<N>/GranularOsc<N> (whichever engines exist for that slot), plainParams }`
  – Oscillator0..2 carry all 5 main engines, Oscillator3 carries `NoiseOsc3`,
  Oscillator4 carries `SubOsc4`. Inactive engines are `{plainParams:"default"}`.
  The **active** engine is NOT marked by a separate flag: the slot's
  `plainParams.kParamType` text selects it (`kOsc_WT` = default/absent for slots
  0–2, `kOsc_MultiSample`/`kOsc_Sample`/`kOsc_Granular`/`kOsc_Spectral` for slots
  0–2, slot 3 = noise, slot 4 = sub; binary enum:
  `kOsc_WT=0, kOsc_MultiSample, kOsc_Sample, kOsc_Granular, kOsc_Spectral,
  kOsc_Noise, kOsc_Sub`).
* `VoiceFilter0..1`, `Env0..3`, `LFO0..9`, `Macro0..7`, `FXRack0..2`,
  `ModSlot0..63`, `Global0`, `Arp0`, `ClipPlayer0`, `VoicePanel0`,
  `PitchQuantizer0`, `RetriggerState0`, `RoutingSlot0..6`,
  `LFOPointModBus0..15`, `MidiClip0..11`, `ArpClip0..11` — same names, same
  shapes on both sides (verified against `body_native.bin`/`body_init.bin`).

**Authored-only top-level keys** (present in the preset, NOT part of the 162-key
instance state — drop them when synthesizing a state):

| authored key | content |
|---|---|
| `WTOsc` | `array[3]` of per-WT-slot UI maps (`kUIParamWTOverviewMouseTag`) |
| `Osc` | `array[5]` of per-osc-slot UI maps (`kUIParamAutoSyncSlicing`, `kUIParamShowMarkerAnimations`, `kUIParamZoomToStartEnd`) |
| `MultiSampleOsc` | `array[3]` UI maps (`kUIParamMultiSampleOverviewMouseTag`) |
| `SpectralOsc` | `array[3]` UI maps (`kUIParamDisplayXYInput`, `kUIParamShowWaveformDisplay`) |
| `GranularOsc` | `array[3]` UI maps (`kUIParamDisplayXYInput`) |
| `Filter` | map `{kUIParamMixOrGain}` — the filter section's mix/gain knob position (authored UI only) |
| `ClipPlayer` | map `{kUIParamPianoRollNotePreview, kUIParamPreviewClip, kUIParamSelectedClip}` |
| `SerumGUI` | map `{kUIParamShowKeyboard, kUIParamShowMidiOut}` |
| `fileType`, `presetName`, `presetAuthor`, `presetDescription`, `arpBankDisplayName`, `clipBankDisplayName` | preset metadata |
| `lfoPointModAssignments`, `midiMap`, `tuningData`, `tuningName` | optional extras (see §5) |

The engine-type keys are **UI-state arrays indexed by slot** (WTOsc/MultiSample/
Spectral/Granular cover the 3 main osc slots 0–2, `Osc` covers all 5) — they do
not carry engine parameters; all engine parameters live inside the
`Oscillator<N>` sections already.

**State-only key**: `component` (text `"processor"` in the state body;
`"controller"` in the cid-2 controller record). A converter must add it.

**Other deltas when instantiating** (from the state side, verified bodies):
`Macro<N>` in the state has **no `name` key** (authored presets carry the macro
name; the state drops it); `MultiSampleOsc<N>` in states additionally carries
`defaultLoopMode` (`'no_loop'`) which authored presets omit; `tags` is truncated
to 2 entries. Preset name/author/description belong to the **controller** state
(cid-2 XferJson JSON header), not the processor body (see
`flp-serum2-conversion.md` §4).

**Value encoding reminder** (state §2): scalars as f32 when exactly representable,
else f64; enums as text; never ints for floats. The same rule holds inside the
authored presets (both widths observed everywhere; e.g. Env curves `50.0` f32 vs
`49.99999999999999` f64, `66.6` f64).

### Instantiation template (for a WT-osc conversion)

```
state = {  # 162 keys; omitted = {plainParams:"default"} like body_init.bin
  Arp0, ArpClip0..11 (clip: map[0]), ClipPlayer0, Env0..3, FXRack0..2 (FX: array[0]),
  Global0, LFO0..9 (curveData: map[0], pathData: map[0]), LFOPointModBus0..15,
  Macro0..7 (plainParams only, NO name), MidiClip0..11 (clip: map[0]),
  ModSlot0..63, Oscillator0..4, PitchQuantizer0 (scale: array[12] of 0),
  RetriggerState0, RoutingSlot0..6, VoiceFilter0..1, VoicePanel0,
  component:'processor', lockOversampling:false, lockTuning:false,
  mpeConfig:0, mpeEnabled:false, mpePitchBendRange:48,
  product:'Serum2', productVersion:'2.0.23', scalars:{note:map[0], velo:map[0]},
  tags:[badge,'Poly'|'Mono'], url, vendor, version: f32 9.0 }
```

## 4. ModSlot — dest + source encoding

`ModSlot<N>` (N = 0..63; inactive slots are just `{plainParams:"default"}`):

```
ModSlot<N> := map{
  destModuleTypeString: text   # section name, e.g. 'VoiceFilter', 'Oscillator', 'FXDelay'
  destModuleID:         uint   # instance index; FX modules: rackIndex*100 + fxSlotIndex
  destModuleParamID:    uint   # plain-parameter id of the destination (see map below)
  destModuleParamName:  text   # 'kParamXxx' (matches destModuleParamID)
  source:               array[2] of uint   # [mainSourceID, auxSourceID] (§4.1)
  plainParams:          map{ kParamAmount (±100), optional kParamBipolar/kParamCurveIn/
                             kParamCurveOut/kParamOut/kParamSmoothRise/kParamSmoothFall/
                             kParamSmoothLink/kParamMainCurveData/kParamAuxCurveData/
                             kParamAuxCurve/kParamAuxInverted/kParamBypass/kParamDelay*
  }
```

destModuleID semantics verified: plain sections use their instance index
(VoiceFilter0/1 → 0/1, Oscillator0..4, Env0..3, LFO0..9, Macro0..7,
RoutingSlot0..6, LFOPointModBus0..15, Arp/Global/VoicePanel/RetriggerState → 0);
every FX type uses `destModuleID = rackIndex*100 + fxSlotIndex` (FXReverb101 =
FXRack1 slot1, FXBode 0 = FXRack0 slot0). destModuleID values above the live FX
slot count (idle saved slots) were observed (e.g. `FXReverb5` with only 3 FX in
the rack) — the converter should only reference live slots.

destModuleParamName is the **stable** identifier: across product versions 2.0.11–2.0.15 every (type, param) pair keeps one id; the handful of conflicting records in the corpus (WTOsc kParamTablePos id=1 ×14, Oscillator kParamDetune id=10, kParamDetuneWid id=11 — all 2.0.11) are drifted legacy writes; a converter should key destinations by the param *name* (or re-emit name+id from the same table).

destModuleParamID → destModuleParamName (observed pairs, complete for the
types used by the converter):

| destModuleTypeString | destModuleID semantics | observed destModuleParamID → destModuleParamName |
|---|---|---|
| `Arp` | 0 | 1=`kParamRate`, 3=`kParamTransposeRange`, 6=`kParamGate`, 7=`kParamChance`, 10=`kParamVeloTarget`, 14=`kParamWrapPhantomNote` |
| `Env` | Env0..3 → 0..3 | 0=`kParamAttack`, 1=`kParamHold`, 2=`kParamDecay`, 3=`kParamSustain`, 4=`kParamRelease`, 5=`kParamCurve1`, 6=`kParamCurve2`, 7=`kParamCurve3` |
| `FXBode` | rack*100+slot | 1=`kParamWet`, 3=`kParamShift`, 4=`kParamRange`, 5=`kParamLevelOut`, 6=`kParamDelayTime`, 7=`kParamFeedback`, 10=`kParamOutputMix`, 11=`kParamOutputWidth`, 12=`kParamBlur` |
| `FXChorus` | rack*100+slot | 1=`kParamWet`, 3=`kParamRate`, 4=`kParamDelay`, 5=`kParamDelay2`, 6=`kParamDepth`, 7=`kParamFeedback`, 8=`kParamFilt`, 9=`kParamLevelOut` |
| `FXComp` | rack*100+slot | 1=`kParamWet`, 2=`kParamThresh`, 3=`kParamRatio`, 5=`kParamRelease`, 6=`kParamMakeup`, 10=`kParamThreshUD2`, 11=`kParamLevelOut`, 13=`kParamXoverHi`, 14=`kParamGain0`, 15=`kParamGain1`, 16=`kParamGain2`, 17=`kParamRatioBelow`, 20=`kParamRatio2`, 22=`kParamRatioBelow1`, 23=`kParamRatioBelow2` |
| `FXConv` | rack*100+slot | 1=`kParamWet`, 3=`kParamLevelOut`, 4=`kParamSize`, 6=`kParamDecay`, 7=`kParamIpTrim`, 8=`kParamTone`, 9=`kParamPredelay` |
| `FXDelay` | rack*100+slot | 1=`kParamWet`, 2=`kParamFreq`, 3=`kParamBW`, 5=`kParamLink`, 6=`kParamTimeL`, 7=`kParamTimeR`, 9=`kParamFeedback`, 10=`kParamOffsetL`, 11=`kParamOffsetR`, 12=`kParamLevelOut` |
| `FXDistortion` | rack*100+slot | 1=`kParamWet`, 2=`kParamDrive`, 3=`kParamLPHP`, 5=`kParamFreq`, 6=`kParamBW`, 8=`kParamLevelOut`, 9=`kParamNumStages` |
| `FXEQ` | rack*100+slot | 1=`kParamFreq1`, 2=`kParamFreq2`, 3=`kParamReso1`, 4=`kParamReso2`, 5=`kParamGain1`, 6=`kParamGain2`, 9=`kParamLevelOut` |
| `FXFilter` | rack*100+slot | 1=`kParamWet`, 3=`kParamFreq`, 4=`kParamReso`, 5=`kParamDrive`, 6=`kParamVar`, 7=`kParamStereo`, 8=`kParamLevelOut`, 9=`kParamX`, 10=`kParamY` |
| `FXFlanger` | rack*100+slot | 1=`kParamWet`, 3=`kParamRate`, 4=`kParamDepth`, 5=`kParamFeedback`, 6=`kParamWidth`, 7=`kParamLevelOut` |
| `FXHyperD` | rack*100+slot | 1=`kParamWet`, 2=`kParamRate`, 3=`kParamDetune`, 6=`kParamLevelOut`, 7=`kParamDimESize`, 8=`kParamDimEWet` |
| `FXPhaser` | rack*100+slot | 1=`kParamWet`, 3=`kParamRate`, 4=`kParamDepth`, 5=`kParamDepth2`, 6=`kParamFreq`, 7=`kParamFeedback`, 9=`kParamLevelOut` |
| `FXReverb` | rack*100+slot | 1=`kParamWet`, 2=`kParamSize`, 3=`kParamDelay`, 4=`kParamFreq`, 5=`kParamFeedback`, 6=`kParamFreqB`, 7=`kParamWidth`, 10=`kParamLevelOut`, 11=`kParamVintageScale` |
| `FXSplit` | rack*100+slot | 1=`kParamFreq` |
| `FXSplit3` | rack*100+slot | 1=`kParamFreq`, 2=`kParamFreq2` |
| `FXUtils` | rack*100+slot | 1=`kParamWet`, 2=`kParamLevelOut`, 3=`kParamWidth`, 4=`kParamBalance`, 6=`kParamLFXover`, 7=`kParamHPF`, 8=`kParamLPF` |
| `Global` | 0 | 1=`kParamMasterTuning`, 2=`kParamVoiceAmp`, 3=`kParamPortamentoTime`, 14=`kParamSwing`, 16=`kParamTranspose` |
| `GranularOsc` | 0..2 | 0=`kParamWarp`, 3=`kParamWarp2`, 6=`kParamDensity`, 7=`kParamGrainLength`, 9=`kParamWindowParam`, 12=`kParamRandomOffset`, 13=`kParamRandomDir`, 14=`kParamRandomPitch`, 15=`kParamRandomGrainLength`, 16=`kParamRandomPan`, 17=`kParamRandomGain`, 20=`kParamRandomWarp`, 23=`kParamDensityMode` |
| `LFO` | LFO0..9 → 0..9 | 0=`kParamRate`, 1=`kParamSmooth`, 2=`kParamRise`, 3=`kParamDelay`, 4=`kParamPhase` |
| `LFOPointModBus` | 0..15 | 0=`kParamValue` |
| `Macro` | 0..7 | 0=`kParamValue` |
| `MidiClip` | 0..11 | 8=`kParamRate` |
| `ModSlot` | 5 (self-mod observed) | 12=`kParamSmoothFall` |
| `MultiSampleOsc` | 0..2 | 0=`kParamWarp`, 3=`kParamWarp2`, 6=`kParamTimbreShift`, 12=`kParamEnvDecay`, 13=`kParamEnvSustain`, 14=`kParamEnvRelease` |
| `NoiseOsc` | 3 | 0=`kParamColor`, 1=`kParamFine`, 2=`kParamInitialPhase`, 3=`kParamRandomPhase` |
| `Oscillator` | 0..4 | 1=`kParamVolume`, 2=`kParamPan`, 3=`kParamOctave`, 4=`kParamPitch`, 5=`kParamFine`, 6=`kParamCoarsePit`, 7=`kParamPitchRatio`, 8=`kParamHzOffset`, 10=`kParamStart`, 13=`kParamScanRate`, 16=`kParamPosition`, 18=`kParamLoopEnd`, 28=`kParamUnisonStereo`, 31=`kParamUnisonWarp`, 10/26=`kParamDetune`, 11/27=`kParamDetuneWid` |
| `RetriggerState` | 0 | 2=`kParamOscB`, 3=`kParamOscC`, 5=`kParamNoise`, 7=`kParamEnv2`, 9=`kParamEnv4`, 10=`kParamLFO1`, 12=`kParamLFO3` |
| `RoutingSlot` | 0..6 | 0=`kParamFilterBalance`, 1=`kParamFXBus1Level`, 2=`kParamFXBus2Level` |
| `SampleOsc` | 0..2 | 0=`kParamWarp`, 3=`kParamWarp2` |
| `SpectralOsc` | 0..2 | 0=`kParamWarp`, 3=`kParamWarp2`, 6=`kParamSpecFltShift`, 7=`kParamSpecFltWetDry`, 8=`kParamFreqLo`, 9=`kParamFreqHi` |
| `VoiceFilter` | 0..1 | 1=`kParamWet`, 3=`kParamFreq`, 4=`kParamReso`, 5=`kParamDrive`, 6=`kParamVar`, 7=`kParamStereo`, 8=`kParamLevelOut`, 9=`kParamX`, 10=`kParamY` |
| `VoicePanel` | 0 | 58=`kParamGlobalScalingEnvTime`, 59=`kParamGlobalScalingLfoTime` |
| `WTOsc` | 0..2 | 0=`kParamWarp`, 1=`kParamWarpVar`, 3=`kParamWarp2`, 4=`kParamWarpVar2`, 7=`kParamUnisonWTPos`, 8=`kParamInitialPhase`, 9=`kParamRandomPhase`, 1/6=`kParamTablePos` |

### 4.1 Modulator source enum (`source = [main, aux]`)

`source` is a flat `array[2]` of **ints** (NOT nested arrays, NOT strings).
Both ints are ids from one shared enum; `0` = none (aux = 0 in 90.4% of live
slots). Decoded from the Serum2.vst3 x64 binary: the mod-source display-name
table sits contiguously at RVA 0xa20820 (`'Mod Wheel','Env 1'..'Env 4','LFO
1'..'LFO 10','Velo','Note#','Aftertouch','Poly Aftertch','Noise OSC','NoteOn
Rand1','NoteOn Rand2','NoteOn Alt.','NoteOn Alt.2','Macro 1'..'Macro 8','Pitch
Bend','Expr X (Pan)','Expr Y (Timbre)','Expr Z (Press.)','Release Velo','Fixed',
'LFO 1 Y'..'LFO 10 Y','OSC A','OSC B','OSC C','SUB OSC','Filter 1','Filter 2',
'Active Voices','Voice Mod 1','Voice Mod 2','Voice Index','NoteOn Rand
(Discrete)') and `sourceID = tableIndex + 1`. Verified against the corpus:
100% LFO-activeness lift for ids 6..15 (LFO 1..10), macro-name ↔ FX-dest
semantic matches (e.g. src 28 = Macro 4 'LFO RATES' → `VoicePanel.
kParamGlobalScalingLfoTime`; src 29 'Macro 5' 'ECHO' → FXDelay wet; src 32
'Macro 8' 'LENGTH' → Arp gate), and every observed id lands on a name (ids 19,
33, 45–46 and 53–57 are unused in factory presets — consistent).

| id | source name | 0-based section | observed in corpus (slots) |
|---|---|---|---|
| 1 | `Mod Wheel` | — | 1094 |
| 2 | `Env 1` | Env0 | 127 |
| 3 | `Env 2` | Env1 | 567 |
| 4 | `Env 3` | Env2 | 235 |
| 5 | `Env 4` | Env3 | 70 |
| 6 | `LFO 1` | LFO0 | 901 |
| 7 | `LFO 2` | LFO1 | 636 |
| 8 | `LFO 3` | LFO2 | 468 |
| 9 | `LFO 4` | LFO3 | 302 |
| 10 | `LFO 5` | LFO4 | 161 |
| 11 | `LFO 6` | LFO5 | 89 |
| 12 | `LFO 7` | LFO6 | 70 |
| 13 | `LFO 8` | LFO7 | 35 |
| 14 | `LFO 9` | LFO8 | 33 |
| 15 | `LFO 10` | LFO9 | 18 |
| 16 | `Velo` | — | 697 |
| 17 | `Note#` | — | 273 |
| 18 | `Aftertouch` | — | 300 |
| 19 | `Poly Aftertch` | — | 0 |
| 20 | `Noise OSC` | Oscillator3 (noise) | 4 |
| 21 | `NoteOn Rand1` | — | 148 |
| 22 | `NoteOn Rand2` | — | 16 |
| 23 | `NoteOn Alt.` | — | 58 |
| 24 | `NoteOn Alt.2` | — | 2 |
| 25 | `Macro 1` | Macro0 | 968 |
| 26 | `Macro 2` | Macro1 | 1035 |
| 27 | `Macro 3` | Macro2 | 978 |
| 28 | `Macro 4` | Macro3 | 992 |
| 29 | `Macro 5` | Macro4 | 1106 |
| 30 | `Macro 6` | Macro5 | 1033 |
| 31 | `Macro 7` | Macro6 | 920 |
| 32 | `Macro 8` | Macro7 | 969 |
| 33 | `Pitch Bend` | — | 0 |
| 34 | `Expr X (Pan)` | — | 6 |
| 35 | `Expr Y (Timbre)` | — | 3 |
| 36 | `Expr Z (Press.)` | — | 2 |
| 37 | `Release Velo` | — | 5 |
| 38 | `Fixed` | — | 25 |
| 39 | `LFO 1 Y` | LFO0 (Y output) | 36 |
| 40 | `LFO 2 Y` | LFO1 (Y output) | 32 |
| 41 | `LFO 3 Y` | LFO2 (Y output) | 14 |
| 42 | `LFO 4 Y` | LFO3 (Y output) | 19 |
| 43 | `LFO 5 Y` | LFO4 (Y output) | 2 |
| 44 | `LFO 6 Y` | LFO5 (Y output) | 2 |
| 45 | `LFO 7 Y` | LFO6 (Y output) | 0 |
| 46 | `LFO 8 Y` | LFO7 (Y output) | 0 |
| 47 | `LFO 9 Y` | LFO8 (Y output) | 2 |
| 48 | `LFO 10 Y` | LFO9 (Y output) | 2 |
| 49 | `OSC A` | Oscillator0 | 15 |
| 50 | `OSC B` | Oscillator1 | 3 |
| 51 | `OSC C` | Oscillator2 | 2 |
| 52 | `SUB OSC` | Oscillator4 | 9 |
| 53 | `Filter 1` | VoiceFilter0 | 0 |
| 54 | `Filter 2` | VoiceFilter1 | 0 |
| 55 | `Active Voices` | — | 0 |
| 56 | `Voice Mod 1` | — | 0 |
| 57 | `Voice Mod 2` | — | 0 |
| 58 | `Voice Index` | — | 1 |
| 59 | `NoteOn Rand (Discrete)` | — | 49 |

`ModSlot.plainParams` inventory (full stats in the JSON):

| plainParam | type | count | range |
|---|---|---|---|
| `plainParams:kParamAmount` | f64 | 13213 | -100..100 |
| `plainParams:kParamAuxCurve` | f64 | 3 | -24.9233..-4.6875 |
| `plainParams:kParamAuxCurveData` | f32 | 95 | 1..1 |
| `plainParams:kParamAuxInverted` | f32 | 52 | 1..1 |
| `plainParams:kParamBipolar` | f32 | 1549 | 1..1 |
| `plainParams:kParamBypass` | f32 | 24 | 1..1 |
| `plainParams:kParamCurveIn` | f32 | 277 | -100..100 |
| `plainParams:kParamCurveOut` | f64 | 51 | -94.5312..100 |
| `plainParams:kParamDelayBeatSync` | f32 | 7 | 1..1 |
| `plainParams:kParamDelayOffset` | f32 | 7 | 0.200131..0.859649 |
| `plainParams:kParamMainCurveData` | f32 | 423 | 1..1 |
| `plainParams:kParamOut` | f64 | 156 | 0..98.6667 |
| `plainParams:kParamSmoothFall` | f64 | 42 | 0.194932..100 |
| `plainParams:kParamSmoothLink` | f32 | 16 | 0..0 |
| `plainParams:kParamSmoothRise` | f64 | 43 | 0.194932..100 |

(`plainParams` itself is the text `'default'` for inactive slots — 32k+ occurrences.)

## 5. Authored-only records (lfoPointModAssignments / midiMap / tuning)

* `lfoPointModAssignments` (66 presets): `array` of `map{busID, lfoID, lfoType,
  pointID, target}` — links LFO curve **points** to the `LFOPointModBus<N>`
  instances (dest `kParamValue`). Not present in the 162-key state format.
* `midiMap` (6 presets): `array[8]` of `map{ccNum: uint, paramIDs: array of
  uint}` with param ids like `7000000 + 1000*i` (macro cc mapping).
* `tuningData` (2 presets): `array` of ~3 480 uints (tuning table);
  `tuningName` text (`'(special)'`).
* `LFOPointModBus<k>.plainParams.kParamValue` — the per-point bus output value
  (only 3 records in the whole corpus; buses are normally default).

## 6. Sub-engine records (inside Oscillator slots)

Wavetable (`Oscillator<k>.WTOsc<k>`): `flex` (map or `array` of XY value maps —
see below), `numChannels` 1–2, `numFrames` 2 048–524 288, `plainParams`
(`kParamInitialPhase` 0..360, `kParamPhaseMemory` text `kPerVoice|kContiguous`,
`kParamRandomPhase` 0..100, `kParamTablePos` ~0..256, `kParamUnisonWTPos`
−100..100, `kParamWarp` 0..1, `kParamWarp2` 0..1, `kParamWarpMenu`/`2` text,
`kParamWarpVar`/`2` 0..1, `kParamXfadeMode`), `relativePathToWT` (wav under
`Documents\Xfer\Serum 2 Presets\Tables\`; prefixes `S2 Tables/…`, `Analog/…`,
`/Analog/…` all observed), `sampleRate` 44 100, optional `embeddedWTData[]`
(f32 array, 159 744 samples observed = per-frame table data), `tableDisplayName`
('Custom'), `interpolateAfterLoad` 0–2, `storedPhasePos[]` (large uints).

Sample (`SampleOsc<k>`): `numChannels`, `numFrames`, `samplePathRelative`,
`sampleRate`, `baseNote`, `gain`, `trimHead`/`trimTail`, plainParams
(`kParamWarp`, `kParamWarp2`, `kParamWarpMenu`/`2`, `kParamWarpVar2`).
Granular (`GranularOsc<k>`): same file fields (`samplePathRelative`) +
`baseDetune` (nint), `edgeFadeMs`, plainParams incl. `kParamDensity` (0..800),
`kParamGrainLength` (0..10), `kParamDensityMode` (`kDensityFree|kDensityBPM`),
`kParamLengthMode` (`kLengthBPM`), `kParamWindowShape`
(`kWindowTukey|kWindowExpDec|kWindowTriangle|kWindowBlackmanHarris|
kWindowGaussian`), `kParamWindowParam`, `kParamWindowSkew` (−100..100),
`kParamRandom*` (Dir/Gain/GrainLength/Offset/Pan/Pitch/Warp/Warp2/
WindowAmount/WindowSkew), `kParamUnisonTrigPattern`
(`kRandom|kExponential|kEven`), `kParamYAxisAssignment` (`kYAxisOscVolume`).
Spectral (`SpectralOsc<k>`): file fields + plainParams (`kParamFreqLo/Hi` Hz,
`kParamLoHiIsSmooth`, `kParamPhaseLock`, `kParamSpecFltShift` −100..100,
`kParamSpecFltWetDry`, `kParamTransients`, `kParamWarp/Warp2/WarpMenu/2/
WarpVar/2` — warp menus are spectral ops like `kSpectralComb`, `kGate`,
`kAddharmonics`, `kSmear`, `kDetune`, `kPeakOctaveUp/Down`, `kShepardFilter`,
`kVocode_OSC/NOISE`, `kMask_OSC/NOISE`, `kMirror`, `kSpectralPitchShift`,
`kSpectralShift`, `kSpectralPhaseTwist`, plus shared dist/FM menus).
MultiSample (`MultiSampleOsc<k>`): `embedded_sfz` (full SFZ text, 245 presets,
`// SFZ Generated by libSFZ` header), `files` = map of relative `.flac` paths →
`{numChannels, numFrames, sampleRate}` (4–234 files each; sampleRate up to
88 200), `sfzPathRelative` ('Factory/…/*.sfz'), plainParams
(`kParamEnvAttack/Decay/Delay/Hold/Release/Sustain`, `kParamEnvOverride`,
`kParamVelTrack`, `kParamVelTrackOverride`, `kParamRandomPhase`,
`kParamTimbreShift` −18..64, `kParamWarp/Warp2/WarpMenu/2/WarpVar/2`).
Noise (`NoiseOsc3`, Oscillator3 only): `detuneFactor` (~0.476..0.518 f64),
`numChannels` 1–2, `numFrames`, `relativePathToNoiseSample`, `sampleRate`,
plainParams (`kParamColor` 0..1, `kParamFine` ~±0.6, `kParamInitialPhase`
0..100, `kParamNoiseType` `White|Pink|Brown|Geiger`, `kParamOneShot`).
Sub (`SubOsc4`, Oscillator4): plainParams only (`kParamShape`
`kTriangle|kRoundRect|kSaw|kSquare|kPulse`, `kParamInitialPhase`,
`kParamContiguousPhase`).

`flex` (WTOsc/SpectralOsc, and FX `flex`): either `map[0]` (empty, init) or a
2×2 `array` — two arrays of two entries, each entry a small `map` (empty or with
numeric leaves; e.g. 4 mapkeys + 6 f32/uint leaves). Only a few numeric leaves —
editor marker data; safe to emit `map[0]`/empty for conversions.

`LFO<k>.curveData`: `{curveVals[], numPoints, xVals[], yVals[], loopbackPointNum}`
— arrays carry `numPoints+1` entries (numPoints 1..212, array len 2..213;
xVals 0..1, yVals 0..1, curveVals ~0..1). `LFO<k>.pathData`: `map[0]` or
`{isOpen: bool, numPoints}`. `curveDisplayName`/`pathDisplayName` text
('Custom'/'Default'/shape names). LFO plainParams: `kParamMode`
(`Free|Envelope`), `kParamRate` (0..100 Hz-ish), `kParamBeatSync`,
`kParamDotted`, `kParamTriplets`, `kParamRate10x`, `kParamDefaultMode` (0),
`kParamDelay`/`kParamRise` (envelope mode), `kParamDirection` (1|2),
`kParamMono`, `kParamAnchored`, `kParamPhase` (0..360), `kParamPhaseSnap`,
`kParamGridX/GridY`, `kParamSwing`, `kParamSmooth`, `kParamType` for the chaos
LFO types (`Rossler|Lorenz|RandomSH|Path`).

## 7. FXRack structure

`FXRack<rackIndex>` (0..2) = `{FX: array[max 16 slots], displayName: '',
plainParams: 'default'}`. Slot array length = highest used slot index + 1
(0..15; lengths up to 22 observed once in idle slots). Each entry:

```
map{ 'type': uint 0..15, '<FXName>': {plainParams: {...}, ...engine records},
      optional 'kUIParamMixOrGain': f32 0..1,      # mix/gain knob position
      optional 'flex': 2x2 maps }
```

`kUIParamMixOrGain` appears on both authored and state entries (2661 authored /
state-confirmed); off-but-saved slots keep `type` + submap (type 0 =
FXDistortion as the default empty-slot effect). `FXConv` additionally carries
the IR by reference: `relativePathToIR` (factory `.flac`), `numChannels`,
`numFrames`, `sampleRate`, optional `embeddedIR` (f32 array, 2 019 samples
observed), `sourceRate`; `FXFilter` carries `PZs` (map of PZ_SVF point maps);
`FXChorus`/`FXFlanger`/`FXPhaser` carry `lfophasor` (f32 0..1 phase snapshot);
`FXHyperD` carries `lfo` (`array[8]`).

### FX type ↔ submap name ↔ parameter table

| type | submap | entries |
|---|---|---|
| 0 | `FXDistortion` | 435 |
| 1 | `FXFlanger` | 52 |
| 2 | `FXPhaser` | 109 |
| 3 | `FXChorus` | 196 |
| 4 | `FXDelay` | 448 |
| 5 | `FXComp` | 540 |
| 6 | `FXReverb` | 505 |
| 7 | `FXEQ` | 687 |
| 8 | `FXFilter` | 365 |
| 9 | `FXHyperD` | 179 |
| 10 | `FXBode` | 130 |
| 11 | `FXConv` | 236 |
| 12 | `FXUtils` | 209 |
| 13 | `FXSplit` | 43 |
| 14 | `FXSplit3` | 19 |
| 15 | `FXSplitMS` | 15 |
### FXComp

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 5 | [default] |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamAttack` | f64 | 408 | 0.1..1000 |
| `kParamCompensatedWetDry` | f32 | 34 | 0..0 |
| `kParamDeadband0` | f64 | 12 | 7..7 |
| `kParamDeadband1` | f64 | 12 | 11.6..11.6 |
| `kParamDeadband2` | f64 | 12 | 5.3..5.3 |
| `kParamEnable` | f32 | 5 | 0..0 |
| `kParamGain0` | f32 | 114 | -24..16.5 |
| `kParamGain1` | f32 | 83 | -16.2115..16.25 |
| `kParamGain2` | f32 | 134 | -16.2143..24 |
| `kParamLevelOut` | f32 | 109 | 0..1 |
| `kParamMakeup` | f64 | 339 | 1.00058..31 |
| `kParamMultiband` | f32 | 205 | 1..1 |
| `kParamRatio` | f64 | 365 | 1..1000000 |
| `kParamRatio0` | f32 | 10 | 0..0.926667 |
| `kParamRatio1` | f32 | 13 | 0.333333..0.887619 |
| `kParamRatio2` | f32 | 21 | 0.186667..0.985875 |
| `kParamRatioBelow` | f32 | 128 | 0.0255505..1 |
| `kParamRatioBelow0` | f32 | 37 | 0..0.94 |
| `kParamRatioBelow1` | f32 | 32 | 0..0.974505 |
| `kParamRatioBelow2` | f32 | 45 | 0..0.97 |
| `kParamRelease` | f64 | 401 | 0.1..1000 |
| `kParamThresh` | f32 | 470 | 0..1 |
| `kParamThreshUD0` | f64 | 59 | 0..181.333 |
| `kParamThreshUD1` | f64 | 57 | 0..200 |
| `kParamThreshUD2` | f64 | 81 | 0..167.256 |
| `kParamWet` | f32 | 74 | 0..98.6196 |
| `kParamXoverHi` | f64 | 80 | 300..10594 |
| `kParamXoverLow` | f64 | 98 | 53.6085..5000 |

### FXDelay

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 1 | [default] |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamBW` | f64 | 403 | 0.75..8.25 |
| `kParamBeatSync` | f32 | 30 | 0..0 |
| `kParamEnable` | f32 | 4 | 0..0 |
| `kParamFeedback` | f64 | 342 | 0..94.3185 |
| `kParamFreq` | f64 | 388 | 40..18000 |
| `kParamHQ` | f32 | 1 | 0..0 |
| `kParamLevelOut` | f32 | 42 | 0..0.852618 |
| `kParamLink` | f32 | 52 | 1..1 |
| `kParamMode` | f32 | 266 | 1..2 |
| `kParamOffsetL` | f32 | 148 | 0.5..1.5 |
| `kParamOffsetR` | f32 | 143 | 0.5..1.5 |
| `kParamTimeL` | f64 | 324 | 0.001..0.342 |
| `kParamTimeR` | f64 | 316 | 0.001..0.324273 |
| `kParamWet` | f32 | 439 | 0..100 |

### FXDistortion

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 1 | [default] |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamBW` | f64 | 205 | 0.075..7.575 |
| `kParamDrive` | f64 | 427 | 0..100 |
| `kParamEnable` | f32 | 1 | 0..0 |
| `kParamFreq` | f32 | 217 | 0..1 |
| `kParamLPHP` | f32 | 105 | 0.129594..100 |
| `kParamLevelOut` | f32 | 85 | 0..0.95614 |
| `kParamMode` | text | 350 | [kOverdrive, kDownsample, kTapeSat, kDiode1, kSoftClip, kDiode2, kSoftSat, kAsym, kSineShaper, kHardClip, kStompBox, kZeroSquare, kXShaper, kSinFold, kRectify, kLinFold, kXShaperAsym] |
| `kParamNumStages` | f32 | 43 | 2..16 |
| `kParamPrePost` | f32 | 177 | 1..2 |
| `kParamWet` | f32 | 293 | 0..99.5614 |

### FXEQ

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 2 | [default] |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamEnable` | f32 | 2 | 0..0 |
| `kParamFreq1` | f64 | 554 | 21.5332..9453 |
| `kParamFreq2` | f64 | 535 | 21.5332..20000 |
| `kParamGain1` | f32 | 377 | -24..24 |
| `kParamGain2` | f32 | 478 | -24..24 |
| `kParamLevelOut` | f32 | 92 | 0..1 |
| `kParamReso1` | f64 | 530 | 0..100 |
| `kParamReso2` | f64 | 513 | 0..94.3065 |
| `kParamType1` | f32 | 479 | 1..2 |
| `kParamType2` | f32 | 312 | 1..2 |

### FXFilter

| key | type | count | range | enums |
|---|---|---|---|---|
| `PZs` | map | 28 | |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamDrive` | f64 | 161 | 0.397698..75.2621 |
| `kParamFreq` | f32 | 356 | 0..1 |
| `kParamLevelOut` | f32 | 34 | 0..1 |
| `kParamPad` | f32 | 3 | 1..1 |
| `kParamReso` | f64 | 189 | 0.760572..99.1201 |
| `kParamStereo` | f64 | 21 | 19.7368..75.4386 |
| `kParamType` | text | 304 | [MgL12, Diffuser, H12, MgL18, Reverb1, MgL24, H24, CombP, L12, DirtyMg, Combs, Allpasses, L6, LadderMg, PZ_SVF, H18, Phase24P, BandReject, FlangeN, RM, PP12, DJMixer, LadderAcid, LNH24, P12, SNH1, ...+43] |
| `kParamVar` | f64 | 151 | 3..100 |
| `kParamWet` | f32 | 145 | 0..99.8013 |
| `kParamX` | f32 | 5 | 0.00660066..1 |
| `kParamY` | f32 | 3 | 0.030303..0.136364 |

### FXReverb

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 1 | [default] |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamDelay` | f64 | 378 | 0..250 |
| `kParamFeedback` | f64 | 197 | 0..100 |
| `kParamFreq` | f64 | 286 | 4.47613..100 |
| `kParamFreqB` | f64 | 336 | 0..100 |
| `kParamFreqC` | f64 | 111 | 0..100 |
| `kParamLevelOut` | f32 | 102 | 0..1 |
| `kParamMode` | f64 | 113 | 0..100 |
| `kParamPreDelay` | f64 | 142 | 5.3433e-13..2.5 |
| `kParamPreDelayBeatSync` | f32 | 60 | 1..1 |
| `kParamSize` | f64 | 459 | 0..100 |
| `kParamType` | text | 404 | [kHall, kVintage, kAbyss, kSpace] |
| `kParamVintageScale` | f64 | 97 | 0..100 |
| `kParamVintageScaleB` | f64 | 75 | 0..100 |
| `kParamWet` | f32 | 501 | 0..100 |
| `kParamWidth` | f64 | 291 | 0..100 |

### FXConv

| key | type | count | range | enums |
|---|---|---|---|---|
| `embeddedIR` | array | 2 | |
| `numChannels` | uint | 235 | 1..2 |
| `numFrames` | uint | 235 | 112..899744 |
| `plainParams` | text | 2 | [default] |
| `relativePathToIR` | text | 235 | [Factory/Cab/Electric Guitar Cab 1.flac, Factory/Medium/Maze.flac, Factory/Medium/80sVerb Room A.flac, Factory/Long/Digital Chamber.flac, Factory/Long/Crisp.flac, Factory/Long/80sVerb Hall A.flac, Factory/Short/L90 Room - Small Chamber.flac, Factory/Long/Digital Hall 1.flac, Factory/Long/L90 Hall - Concert Hall.flac, Factory/Long/80sVerb Hall B.flac, Factory/Massive/Crystal Hall.flac, Factory/Short/Laptop Speaker Tiny.flac, Factory/Short/Hyper1.flac, Factory/Massive/Cathedral In The Sky.flac, Factory/Coloration/Pencil.flac, Factory/Weird/Stylo Unbent.flac, Factory/Weird/ClickVinyl.flac, Factory/Long/L90 Hall - DeepBlue.flac, Factory/Short/Digital Gated.flac, Factory/Cab/Electric Guitar Cab 2.flac, Factory/Coloration/Woody.flac, Factory/Short/DP4 Verb.flac, Factory/Medium/10m scatter.flac, Factory/Short/Tiny Diffuse.flac, Factory/Long/Caster Big Verb.flac, Factory/Short/L90 Plate - Drum - Short Plate.flac, ...+49] |
| `sampleRate` | uint | 235 | 44100..48000 |
| `sourceRate` | f32 | 1 | 48000..48000 |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamAttack` | f64 | 13 | 2.87514e-07..0.231929 |
| `kParamDamping` | f64 | 43 | 10.1672..100 |
| `kParamDecay` | f32 | 92 | 2.2399e-05..40 |
| `kParamEnable` | f32 | 3 | 0..0 |
| `kParamIpTrim` | f32 | 150 | -34..6 |
| `kParamLevelOut` | f32 | 73 | 0.0238343..0.813852 |
| `kParamMinPhase` | f32 | 17 | 1..1 |
| `kParamPredelay` | f64 | 35 | 8.3099e-09..0.354727 |
| `kParamPredelayBeatSync` | f32 | 5 | 1..1 |
| `kParamSize` | f64 | 108 | 10..1000 |
| `kParamTone` | f64 | 105 | -100..84.2105 |
| `kParamWet` | f32 | 230 | 0..100 |

### FXChorus

| key | type | count | range | enums |
|---|---|---|---|---|
| `lfophasor` | f32 | 196 | 0.00255525..0.988839 |
| `plainParams` | text | 1 | [default] |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamBeatSync` | f32 | 5 | 1..1 |
| `kParamDelay` | f64 | 81 | 0..12.8 |
| `kParamDelay2` | f64 | 65 | 0.0494169..10.4804 |
| `kParamDepth` | f64 | 79 | 0..25.2453 |
| `kParamEnable` | f32 | 1 | 0..0 |
| `kParamFeedback` | f64 | 121 | 0..58.1117 |
| `kParamFilt` | f64 | 143 | 50..20000 |
| `kParamFiltMode` | f32 | 17 | 1..1 |
| `kParamLevelOut` | f32 | 23 | 0.370148..1 |
| `kParamRate` | f64 | 111 | 0..1.37997 |
| `kParamWet` | f32 | 191 | 0..100 |

### FXPhaser

| key | type | count | range | enums |
|---|---|---|---|---|
| `lfophasor` | f32 | 109 | 0..0.991893 |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamBeatSync` | f32 | 37 | 1..1 |
| `kParamDepth` | f64 | 64 | 0..100 |
| `kParamDepth2` | f32 | 45 | 0..1 |
| `kParamEnable` | f32 | 5 | 0..0 |
| `kParamFeedback` | f64 | 76 | 0..100 |
| `kParamFreq` | f64 | 73 | 20..18000 |
| `kParamLevelOut` | f32 | 10 | 0.315789..0.762322 |
| `kParamNumPoles` | f32 | 48 | 1..18 |
| `kParamRate` | f64 | 80 | 0..20 |
| `kParamWet` | f32 | 100 | 0..82.6149 |
| `kParamWidth` | f64 | 15 | 0..313.396 |

### FXFlanger

| key | type | count | range | enums |
|---|---|---|---|---|
| `lfophasor` | f32 | 52 | 0.0327852..0.999654 |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamBeatSync` | f32 | 9 | 1..1 |
| `kParamDepth` | f64 | 27 | 0..99.9654 |
| `kParamFeedback` | f64 | 26 | 19.7368..90.9183 |
| `kParamLevelOut` | f32 | 5 | 0.397661..0.729482 |
| `kParamRate` | f64 | 35 | 0..8.52582 |
| `kParamWet` | f32 | 46 | 0..87.1345 |
| `kParamWidth` | f64 | 10 | 0..360 |

### FXBode

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 2 | [default] |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamBeatSync` | f32 | 27 | 1..1 |
| `kParamBlur` | f64 | 63 | 1.17182..100 |
| `kParamDelayBalance` | f64 | 25 | -92.5507..100 |
| `kParamDelayTime` | f64 | 41 | 0.000549449..1.7536 |
| `kParamEnable` | f32 | 1 | 0..0 |
| `kParamFeedback` | f64 | 45 | 8.33333..100 |
| `kParamLevelOut` | f32 | 27 | 0..0.743199 |
| `kParamMonoInput` | f32 | 10 | 1..1 |
| `kParamOutputMix` | f32 | 29 | -100..100 |
| `kParamOutputWidth` | f64 | 42 | 0..99.4152 |
| `kParamRange` | f64 | 82 | 0.1..3043 |
| `kParamRetrig` | f32 | 9 | 1..1 |
| `kParamShift` | f64 | 96 | -100..73.3827 |
| `kParamSwapAB` | f32 | 3 | 1..1 |
| `kParamWet` | f32 | 94 | 0..72.9225 |

### FXHyperD

| key | type | count | range | enums |
|---|---|---|---|---|
| `lfo` | array | 358 | |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamDetune` | f64 | 103 | 0..100 |
| `kParamDimELevelOut` | f32 | 3 | 0.29619..0.486842 |
| `kParamDimESize` | f64 | 134 | 0..100 |
| `kParamDimEWet` | f64 | 77 | 0.438596..100 |
| `kParamEnable` | f32 | 1 | 0..0 |
| `kParamLevelOut` | f32 | 21 | 0.162281..0.688596 |
| `kParamRate` | f64 | 130 | 0..100 |
| `kParamRetrig` | f32 | 12 | 1..1 |
| `kParamUnison` | f64 | 118 | 0..7 |
| `kParamWet` | f32 | 175 | 0..100 |

### FXUtils

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 27 | [default] |

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamBalance` | f32 | 15 | -100..100 |
| `kParamHPF` | f64 | 57 | 1.33517..400 |
| `kParamLFMono` | f32 | 67 | 1..1 |
| `kParamLFXover` | f64 | 68 | 20.25..400 |
| `kParamLPF` | f64 | 31 | 50..19924 |
| `kParamLevelOut` | f32 | 67 | 0..1 |
| `kParamPolarityL` | f32 | 1 | 1..1 |
| `kParamPolarityR` | f32 | 4 | 1..1 |
| `kParamWet` | f32 | 6 | 0..82.4561 |
| `kParamWidth` | f64 | 84 | 0..800 |

### FXSplit

| key | type | count | range | enums |
|---|---|---|---|---|

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamFreq` | f64 | 38 | 30.784..3517 |
| `kParamModuleCount1` | f32 | 22 | 1..3 |
| `kParamModuleCount2` | f32 | 37 | 1..5 |

### FXSplit3

| key | type | count | range | enums |
|---|---|---|---|---|

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamFreq` | f64 | 16 | 80..1202 |
| `kParamFreq2` | f64 | 17 | 309.492..8999 |
| `kParamModuleCount1` | f32 | 14 | 1..3 |
| `kParamModuleCount2` | f32 | 19 | 1..4 |
| `kParamModuleCount3` | f32 | 16 | 1..5 |

### FXSplitMS

| key | type | count | range | enums |
|---|---|---|---|---|

| plainParam | type | count | range | enums |
|---|---|---|---|---|
| `kParamModuleCount1` | f32 | 7 | 1..2 |
| `kParamModuleCount2` | f32 | 15 | 1..3 |


## 8. Per-section parameter tables

### Oscillator

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 711 | [default] |
| `plainParams:kParamAutoSliceThreshold` | f32 | 5 | 0.121212..0.396583 |
| `plainParams:kParamBaseTempo` | f64 | 3 | 138..156 |
| `plainParams:kParamCoarsePit` | f32 | 55 | -64..64 |
| `plainParams:kParamDetune` | f64 | 790 | 0..1 |
| `plainParams:kParamDetuneMode` | text | 34 | [kDetuneRandom, kDetuneSuper, kDetuneExp, kDetuneInv] |
| `plainParams:kParamDetuneWid` | f64 | 335 | 0..100 |
| `plainParams:kParamEnable` | f32 | 1511 | 0..1 |
| `plainParams:kParamEnd` | f64 | 24 | 7.36877..99.9031 |
| `plainParams:kParamFine` | f32 | 307 | -46.2341..76.1905 |
| `plainParams:kParamHzOffset` | f64 | 14 | -1000..500 |
| `plainParams:kParamKeyZoneMax` | f32 | 236 | 10..126 |
| `plainParams:kParamKeyZoneMin` | f32 | 101 | 11..127 |
| `plainParams:kParamKeyZoneWarp` | f32 | 2 | 1..1 |
| `plainParams:kParamLinkLoopLength` | f32 | 3 | 1..1 |
| `plainParams:kParamLoopCrossfade` | f64 | 34 | 0.5..63.1758 |
| `plainParams:kParamLoopEnd` | f64 | 118 | 0..99.9996 |
| `plainParams:kParamLoopEndsAtRelease` | f32 | 5 | 1..1 |
| `plainParams:kParamLoopMode` | text | 121 | [kForward, kPingPong, kTailed, kReverse] |
| `plainParams:kParamLoopStart` | f64 | 150 | 0.00102693..100 |
| `plainParams:kParamLoopStartLink` | f32 | 7 | 1..1 |
| `plainParams:kParamManualPositionMode` | f32 | 10 | 1..1 |
| `plainParams:kParamOctave` | f32 | 988 | -4..4 |
| `plainParams:kParamPan` | f32 | 205 | -50..50 |
| `plainParams:kParamPitch` | f32 | 126 | -12..12 |
| `plainParams:kParamPitchBendTrack` | f32 | 2 | 0..0 |
| `plainParams:kParamPitchMode` | text | 57 | [Ratio, Harmonics] |
| `plainParams:kParamPitchRatio` | f64 | 44 | -1..24 |
| `plainParams:kParamPitchRatioModMode` | text | 2 | [Coarse, Fine] |
| `plainParams:kParamPitchSource` | f32 | 2 | 0..4 |
| `plainParams:kParamPitchTrack` | f32 | 96 | 0..1 |
| `plainParams:kParamPosition` | f64 | 8 | 3.03762..72.2533 |
| `plainParams:kParamRandomStart` | f32 | 10 | 19.9219..100 |
| `plainParams:kParamReverse` | f32 | 2 | 1..1 |
| `plainParams:kParamScanBPMDivide` | f32 | 7 | 1..1 |
| `plainParams:kParamScanBPMRate` | f32 | 6 | 0..2 |
| `plainParams:kParamScanRange` | f32 | 13 | 1..2 |
| `plainParams:kParamScanRate` | f32 | 105 | -200..200 |
| `plainParams:kParamScanTempoLock` | f32 | 1 | 1..1 |
| `plainParams:kParamSlicingEnabled` | text | 5 | [Manual, Auto] |
| `plainParams:kParamSlicingRootNote` | f32 | 1 | 6..6 |
| `plainParams:kParamStart` | f64 | 64 | 0.0929182..67.1642 |
| `plainParams:kParamType` | text | 475 | [kOsc_MultiSample, kOsc_Sample, kOsc_Granular, kOsc_Spectral] |
| `plainParams:kParamUnison` | f32 | 625 | 2..16 |
| `plainParams:kParamUnisonRange` | f32 | 51 | 0..48 |
| `plainParams:kParamUnisonSpan` | f32 | 7 | 20.3125..59.4378 |
| `plainParams:kParamUnisonStack` | text | 45 | [kOctave1, kCenter12, kOctaveFifth2, kOctaveFifth1, kOctave2, kOctave3, kOctaveFifth3] |
| `plainParams:kParamUnisonStereo` | f32 | 139 | -100..79.8413 |
| `plainParams:kParamUnisonWarp` | f32 | 11 | -12.695..79.6356 |
| `plainParams:kParamUnisonWarp2` | f32 | 4 | -55.4688..18.0955 |
| `plainParams:kParamVelocityZoneMax` | f32 | 6 | 1..125 |
| `plainParams:kParamVelocityZoneMin` | f32 | 4 | 2..126 |
| `plainParams:kParamVolume` | f64 | 2144 | 0..1 |

### Oscillator.WTOsc

| key | type | count | range | enums |
|---|---|---|---|---|
| `embeddedWTData[]` | f32 | 159744 | -1..1 |
| `flex` | map | 1133 | |
| `flex:val` | f32 | 1914 | 0..5 |
| `interpolateAfterLoad` | uint | 34 | 0..2 |
| `numChannels` | uint | 1334 | 1..2 |
| `numFrames` | uint | 1334 | 2048..524288 |
| `plainParams` | text | 872 | [default] |
| `plainParams:kParamInitialPhase` | f32 | 125 | 0..360 |
| `plainParams:kParamPhaseMemory` | text | 30 | [kPerVoice, kContiguous] |
| `plainParams:kParamRandomPhase` | f32 | 224 | 0..99.8 |
| `plainParams:kParamTablePos` | f64 | 643 | 1.51003..256 |
| `plainParams:kParamUnisonWTPos` | f32 | 15 | -100..100 |
| `plainParams:kParamWarp` | f32 | 346 | 0.00199928..1 |
| `plainParams:kParamWarp2` | f32 | 114 | 0.000565378..1 |
| `plainParams:kParamWarpMenu` | text | 661 | [kPD_OSC, kSync, kFM_OSC, kPD_SUB, kDistTube, kFM_SUB, kBendPos, kPWM, kFM_NOISE, kBendPosNeg, kPD_OSC2, kBendNeg, kDistSoftSat, kRM_OSC, kFM_OSC2, kAM_OSC, kRM_SUB, kSelfPD, kQuantize, kDistSoftClip, kDistDiode1, kDistDiode2, kPD_NOISE, kEvenOdd, kFilterLPF, kDLM, ...+26] |
| `plainParams:kParamWarpMenu2` | text | 261 | [kSync, kPD_OSC2, kFM_OSC, kPD_SUB, kPD_OSC, kFilterLPF, kDistTube, kRM_OSC, kRM_SUB, kBendPosNeg, kFM_SUB, kBendPos, kPWM, kRM_OSC2, kDistSoftClip, kFM_OSC2, kDistTapeSat, kAM_OSC, kSelfPD, kFM_NOISE, kAM_NOISE, kPD_FILT2, kDistZeroSquare, kFilterHPF, kDistDiode2, kPD_FILT1, ...+19] |
| `plainParams:kParamWarpVar` | f32 | 20 | 0.153274..1 |
| `plainParams:kParamWarpVar2` | f32 | 5 | 0.005..0.991484 |
| `plainParams:kParamXfadeMode` | f32 | 28 | 1..1 |
| `relativePathToWT` | text | 1313 | [S2 Tables/Default Shapes.wav, S2 Tables/Analog/DM - OSCAR.wav, Analog/Basic Shapes.wav, Analog/PWM Juno.wav, Analog/Analog_BD_Sin.wav, Analog/Basic Mini.wav, /Analog/Basic Shapes.wav, /Analog/Analog_BD_Sin.wav, Analog/SawRounded.wav, S2 Tables/Digital/Basic OPL.wav, S2 Tables/Analog/DM FMOD 01.wav, S2 Tables/Analog/Warm Sub.wav, Analog/Jno.wav, Analog/Basic Mg.wav, S2 Tables/Digital/Dying Saw.wav, S2 Tables/Digital/Dying Sine.wav, Analog/PWM Mini.wav, Analog/Acid.wav, S2 Tables/Digital/AM Sine Harmonics.wav, S2 Tables/Digital/Harmonic Series Smooth.wav, /Analog/Basic Mini.wav, S2 Tables/Analog/DM FMOD 02.wav, Analog/PWM Genji.wav, Analog/Basic_Cjw.wav, S2 Tables/Digital/Memory Organ.wav, Analog/PWM MG.wav, ...+191] |
| `sampleRate` | uint | 1334 | 44100..44100 |
| `storedPhasePos[]` | uint | 307 | 0..8767947053021 |
| `tableDisplayName` | text | 27 | [Custom, Default Shapes] |

### Oscillator.MultiSampleOsc

| key | type | count | range | enums |
|---|---|---|---|---|
| `embedded_sfz` | text | 245 | |
| `files:#entries` | uint | 245 | 4..234 |
| `files:numChannels` | uint | 9706 | 1..2 |
| `files:numFrames` | uint | 9706 | 680..1232271 |
| `files:sampleRate` | uint | 9706 | 44100..88200 |
| `plainParams` | text | 1581 | [default] |
| `plainParams:kParamEnvAttack` | f64 | 292 | 0..0.39635 |
| `plainParams:kParamEnvDecay` | f32 | 289 | 0..32 |
| `plainParams:kParamEnvDelay` | f64 | 5 | 3.81697e-09..0.0124431 |
| `plainParams:kParamEnvHold` | f64 | 5 | 0.000244199..32 |
| `plainParams:kParamEnvOverride` | f32 | 67 | 1..1 |
| `plainParams:kParamEnvRelease` | f64 | 295 | 0..32 |
| `plainParams:kParamEnvSustain` | f32 | 11 | 0..0.956558 |
| `plainParams:kParamRandomPhase` | f64 | 25 | 0.144043..76.7664 |
| `plainParams:kParamTimbreShift` | f32 | 53 | -17.9649..64 |
| `plainParams:kParamVelTrack` | f32 | 36 | 0..95.7031 |
| `plainParams:kParamVelTrackOverride` | f32 | 23 | 1..1 |
| `plainParams:kParamWarp` | f32 | 38 | 0.002..1 |
| `plainParams:kParamWarp2` | f32 | 20 | 0.02..1 |
| `plainParams:kParamWarpMenu` | text | 81 | [kPD_SUB, kDistTube, kFilterLPF, kPD_OSC, kRM_OSC, kDistTapeSat, kDistStompBox, kFilterHPF, kFM_SUB, kFM_OSC2, kDistSoftSat, kSelfPD, kDistDiode1, kDistSinFold, kFM_NOISE, kAM_OSC, kRM_SUB, kDistSoftClip, kFM_OSC, kDistAsym, kDistSineShaper, kDistDiode2, kDistHardClip, kPD_OSC2, kFM_FILT1] |
| `plainParams:kParamWarpMenu2` | text | 49 | [kSelfPD, kFilterLPF, kPD_SUB, kFM_NOISE, kFM_OSC, kAM_SUB, kAM_OSC2, kDistTube, kFMX_OSC, kAM_OSC, kPD_OSC, kFM_SUB, kDistStompBox, kDistSineShaper, kPD_NOISE, kRM_OSC, kRM_NOISE, kDistSoftClip, kDistTapeSat, kPD_FILT1, kFM_OSC2, kFM_FILT2] |
| `plainParams:kParamWarpVar` | f32 | 2 | 0.595..0.681079 |
| `plainParams:kParamWarpVar2` | f32 | 1 | 0.681079..0.681079 |
| `sfzPathRelative` | text | 245 | [Factory/Synth/SuperJX 4 Chorus Pad.sfz, Factory/Strings/Full Strings LE.sfz, Factory/Keys/Baby Grand Piano.sfz, Factory/Synth/Arp Solina - Viola.sfz, Factory/Choir/Ah High.sfz, Factory/Mallet/Balafon.sfz, Factory/Plucked/Oud.sfz, Factory/Winds/French Horns.sfz, Factory/Keys/Elec.Piano Suitcase.sfz, Factory/Plucked/Gtr Harmonics.sfz, Factory/Synth/Arp Solina - Horn.sfz, Factory/Strings/Violins Half Tremolo LE.sfz, Factory/Strings/Cello LE.sfz, Factory/Plucked/Swarsangam.sfz, Factory/Plucked/Gtr Ac Martin Velo.sfz, Factory/Strings/Full Strings.sfz, Factory/Plucked/Harp (-12 dB sus).sfz, Factory/Plucked/Gtr Ac Martin Pick.sfz, Factory/Choir/Ah Both.sfz, Factory/Plucked/Gtr Ac Cheap Nylon.sfz, Factory/Plucked/Gtr Ac 12 String.sfz, Factory/Plucked/Kalimba.sfz, Factory/Winds/Trombones Cimbasso LE.sfz, Factory/Winds/Trumpets LE.sfz, Factory/Winds/Trombones Tenor LE.sfz, Factory/Drums/Acoustic/Kit DUP Live.sfz, ...+73] |

### Oscillator.SampleOsc

| key | type | count | range | enums |
|---|---|---|---|---|
| `baseNote` | uint | 18 | 36..72 |
| `gain` | f32 | 1 | 1.0217..1.0217 |
| `numChannels` | uint | 110 | 1..2 |
| `numFrames` | uint | 110 | 792..2810505 |
| `plainParams` | text | 1837 | [default] |
| `plainParams:kParamWarp` | f32 | 16 | 0.0219298..1 |
| `plainParams:kParamWarp2` | f32 | 10 | 0.0175439..1 |
| `plainParams:kParamWarpMenu` | text | 37 | [kDistTube, kFM_OSC, kDistAsym, kDistSineShaper, kFM_SUB, kFM_OSC2, kFilterLPF, kDistSoftClip, kDistSoftSat, kAM_SUB, kPD_OSC, kFMP_OSC, kDistDiode2, kSelfPD, kAM_OSC, kDistSinFold, kRM_SUB, kFM_FILT1, kDistZeroSquare] |
| `plainParams:kParamWarpMenu2` | text | 17 | [kFilterHPF, kFM_SUB, kFM_NOISE, kFM_OSC2, kPD_OSC2, kRM_OSC, kDistSoftClip, kPD_FILT1, kDistTapeSat, kDistLinFold, kFM_FILT2] |
| `plainParams:kParamWarpVar2` | f32 | 3 | 0.18848..0.546875 |
| `samplePathRelative` | text | 110 | [Factory/Piano/Breathy Pianoish.flac, Factory/Bass/Alu Slap.flac, Factory Non-Tonal/Drum/Perc/Guitar Body Percussion.flac, Factory/Plucked/Etheral Note.flac, Factory Non-Tonal/Noises/S2 Noises/Rain 100 High.flac, Factory/Flute/Flute Rainforest.flac, Factory/Flute/Pan Flute.flac, ../Multisamples/Factory/Keys/RDE88_V Samples/XFRde88 V6 C3.flac, Factory Non-Tonal/Drum/Kick/808 Kick A 01.flac, Factory Non-Tonal/Drum/Kick/808 Kick A 02.flac, Factory/Bass/Clean 808.flac, Factory Non-Tonal/Noises/S2 Noises/Fizz.flac, Factory/Bass/Round Comforting.flac, Factory/Bass/JB Fingerstyle.flac, Factory/Bass/PB Fingerstyle.flac, Factory/Plucked/On Far Away Planet.flac, Factory/Bass/Samo Op.flac, Factory/Brass/Brass Wall Low.flac, Factory/Brass/Trombone Alt.flac, Factory/Brass/Trombone.flac, ../Multisamples/Factory/Mallet/Marimba Samples/XFMarimba 02 C3.flac, Factory Non-Tonal/Drum/Kick/Duda Kicks/XFDuda Kick 06.flac, Factory Non-Tonal/Drum/Shaker/Rattle.flac, Factory Non-Tonal/SFX/Cracks N Rustle.flac, Factory Non-Tonal/Noises/S2 Noises/Blip.flac, Factory Non-Tonal/Noises/S2 Noises/Screws in Bowl.flac, ...+74] |
| `sampleRate` | uint | 110 | 44100..48000 |
| `trimHead` | uint | 1 | 271160..271160 |
| `trimTail` | uint | 1 | 117865..117865 |

### Oscillator.GranularOsc

| key | type | count | range | enums |
|---|---|---|---|---|
| `baseDetune` | nint | 1 | -23..-23 |
| `baseNote` | uint | 10 | 17..93 |
| `edgeFadeMs` | uint | 1 | 16..16 |
| `gain` | f32 | 2 | 5.25718..36.6532 |
| `numChannels` | uint | 65 | 1..2 |
| `numFrames` | uint | 65 | 6144..1300542 |
| `plainParams` | text | 1802 | [default] |
| `plainParams:kParamDensity` | f64 | 71 | 0..800.037 |
| `plainParams:kParamDensityMode` | text | 3 | [kDensityFree, kDensityBPM] |
| `plainParams:kParamGrainLength` | f64 | 63 | 0..10 |
| `plainParams:kParamLengthMode` | text | 2 | [kLengthBPM] |
| `plainParams:kParamRandomDir` | f64 | 34 | 10.0066..100 |
| `plainParams:kParamRandomGain` | f64 | 17 | 6.44703..73.835 |
| `plainParams:kParamRandomGrainLength` | f64 | 45 | 1.00055..100 |
| `plainParams:kParamRandomOffset` | f64 | 33 | 5.14682..100 |
| `plainParams:kParamRandomPan` | f64 | 44 | 3.63213..100 |
| `plainParams:kParamRandomPitch` | f64 | 23 | 2.39422e-08..12 |
| `plainParams:kParamRandomWarp` | f64 | 5 | 6.679..58.2253 |
| `plainParams:kParamRandomWarp2` | f64 | 1 | 73.4078..73.4078 |
| `plainParams:kParamRandomWindowAmount` | f64 | 3 | 22.4457..56.8591 |
| `plainParams:kParamRandomWindowSkew` | f32 | 4 | 16.1911..69.1579 |
| `plainParams:kParamUnisonTrigPattern` | text | 7 | [kRandom, kExponential, kEven] |
| `plainParams:kParamWarp` | f32 | 13 | 0.0332428..0.823453 |
| `plainParams:kParamWarp2` | f32 | 9 | 0.0504557..0.973186 |
| `plainParams:kParamWarpMenu` | text | 16 | [kDistTube, kFM_OSC, kDistAsym, kFilterLPF, kDistSoftSat, kDistDiode2, kFM_NOISE, kDistSoftClip, kDistTapeSat, kFM_OSC2, kPD_OSC] |
| `plainParams:kParamWarpMenu2` | text | 12 | [kFM_OSC, kDistTube, kAM_SUB, kDistSoftClip, kDistSoftSat, kFilterHPF, kAM_OSC, kFilterLPF, kDistDiode1] |
| `plainParams:kParamWindowParam` | f32 | 9 | 19.6304..95.3563 |
| `plainParams:kParamWindowShape` | text | 12 | [kWindowTukey, kWindowExpDec, kWindowTriangle, kWindowBlackmanHarris, kWindowGaussian] |
| `plainParams:kParamWindowSkew` | f32 | 10 | -100..34.1861 |
| `plainParams:kParamYAxisAssignment` | text | 1 | [kYAxisOscVolume] |
| `samplePath` | text | 1 | [] |
| `samplePathRelative` | text | 65 | [Factory/Piano/Piano High Long Tail.flac, Factory/Flute/Flute Rainforest.flac, Factory/Flute/Flute Delay.flac, Factory Non-Tonal/Noises/S2 Noises/Vinyl Crackle.flac, Factory/Synth/DX Brass2 C2.flac, Factory Non-Tonal/Noises/S2 Noises/Stretched Vinyl.flac, Factory/Plucked/Shankar.flac, Factory/Plucked/Etheral Note.flac, Factory/Vox/Ohaum Breathy Glissando High.flac, Factory/Plucked/Harp Higher.flac, Factory Non-Tonal/SFX/Brickbreak.flac, Factory Non-Tonal/Drum/Snare/808 Snare 01.flac, Factory/Plucked/Pizzicato Thing.flac, Factory/Flute/Flute Fula.flac, Factory/Plucked/Plucky Bell.flac, Factory/Spatial/Ambience Grains of Sand.flac, Factory/Flute/Flute Long Vibrato.flac, Factory/Bowed/Morin Khuur Sustained.flac, Factory/Vox/Oo Long Bright Angels.flac, Factory/Flute/Flute Warm.flac, Factory Non-Tonal/Noises/S2 Noises/Pebbles in Tube.flac, Factory/Bowed/Strings Crystal Joy.flac, Factory/Spatial/Drone Crimson.flac, Factory Non-Tonal/Noises/S2 Noises/Keys and Ball.flac, Factory/Bowed/Strings Heady.flac, Factory/Plucked/Harp Long Ringing.flac, ...+22] |
| `sampleRate` | uint | 66 | 44100..48000 |
| `trimHead` | uint | 1 | 53132..53132 |

### Oscillator.SpectralOsc

| key | type | count | range | enums |
|---|---|---|---|---|
| `baseDetune` | nint | 1 | -23..-23 |
| `baseNote` | uint | 6 | 17..72 |
| `flex:val` | f32 | 830 | 0..34 |
| `numChannels` | uint | 53 | 1..2 |
| `numFrames` | uint | 53 | 3660..1236088 |
| `plainParams` | text | 1811 | [default] |
| `plainParams:kParamFreqHi` | f64 | 8 | 371.078..17461 |
| `plainParams:kParamFreqLo` | f64 | 16 | 15.9452..4307 |
| `plainParams:kParamLoHiIsSmooth` | f32 | 2 | 1..1 |
| `plainParams:kParamPhaseLock` | f32 | 9 | 1..1 |
| `plainParams:kParamSpecFltShift` | f64 | 35 | -100..100 |
| `plainParams:kParamSpecFltWetDry` | f64 | 10 | 0..98.6842 |
| `plainParams:kParamTransients` | f32 | 13 | 1..1 |
| `plainParams:kParamWarp` | f32 | 34 | 0.0350877..1 |
| `plainParams:kParamWarp2` | f32 | 16 | 0.153509..1 |
| `plainParams:kParamWarpMenu` | text | 45 | [kSpectralComb, kGate, kAddsubharmonics, kSpread, kAddharmonics, kSmear, kDetune, kPeakOctaveDown, kShepardFilter, kFM_SUB, kPD_OSC, kDistDiode2, kDistDiode1, kSpectralPitchShift, kMask_OSC, kMask_NOISE, kFilterLPF, kDistSoftClip, kAM_OSC, kSpectralShift, kDistTube, kFilterHPF, kShepardNarrow, kVocode_OSC, kPeakOctaveUp, kVocode_NOISE] |
| `plainParams:kParamWarpMenu2` | text | 32 | [kDistDiode1, kDistTube, kFilterLPF, kPeakOctaveUp, kPD_OSC2, kDistSoftClip, kMirror, kDistTapeSat, kPeakHarmDown, kPD_OSC, kSpectralComb, kSpectralPhaseTwist, kPeakHarmUp, kSpread, kDistSoftSat, kVocode_OSC, kSpectralPitchShift, kDetune, kAddharmonics, kSelfPD] |
| `plainParams:kParamWarpVar` | f32 | 8 | 0.0117188..1 |
| `plainParams:kParamWarpVar2` | f32 | 2 | 0.292969..1 |
| `samplePathRelative` | text | 53 | [Factory Non-Tonal/SFX/Human Fly.flac, Factory Non-Tonal/SFX/UFO Wobble.flac, Factory Non-Tonal/Drum/Rim/505 Rim.flac, Factory/Flute/Flute Delay.flac, Factory/Vox/Oo Long High.flac, Factory/Spatial/Drone Creepy Cello Dive.flac, Factory/Plucked/Etheral Note.flac, Factory/Synth/Modern Wub.flac, Factory/Plucked/Balafon Short.flac, Factory Non-Tonal/Drum/Conga/808 Conga High.flac, Factory/Plucked/Egtr E1k Mid D1.flac, Factory/Plucked/Egtr E1k Mid A1.flac, Factory/Plucked/Egtr E1k Mid D2.flac, Factory/Piano/Piano High Long Tail.flac, Factory Non-Tonal/Loop/Drum Loop/80 Brooklyn Taped.flac, Factory Non-Tonal/Drum/Kick/Duda Kicks/XFDuda Kick 05.flac, Factory Non-Tonal/Drum/Hat/Hat Closed Schrott.flac, Factory/Spatial/Drone Floating Orb.flac, Factory/Flute/Flute Rainforest.flac, Factory/Bass/Swiss Submarine.flac, Factory/Bowed/Horsehead Fiddle.flac, Factory/Spatial/Ambience Grains of Sand.flac, Factory/Spatial/Chord Angeles.flac, Factory/Flute/Flute Staccato.flac, Factory/Spatial/Drone Crimson.flac, Factory/Piano/Sympathy Piano C4.flac, ...+19] |
| `sampleRate` | uint | 53 | 44100..48000 |

### Oscillator.NoiseOsc

| key | type | count | range | enums |
|---|---|---|---|---|
| `detuneFactor` | f64 | 626 | 0.475822..0.517901 |
| `numChannels` | uint | 624 | 1..2 |
| `numFrames` | uint | 624 | 90..1354752 |
| `plainParams` | text | 350 | [default] |
| `plainParams:kParamColor` | f32 | 219 | 0..1 |
| `plainParams:kParamFine` | f32 | 18 | -0.573934..0.245614 |
| `plainParams:kParamInitialPhase` | f32 | 76 | 0.9375..100 |
| `plainParams:kParamNoiseType` | text | 93 | [White, Geiger, Pink, Brown] |
| `plainParams:kParamOneShot` | f32 | 77 | 1..1 |
| `plainParams:kParamRandomPhase` | f32 | 56 | 1.17188..100 |
| `relativePathToNoiseSample` | text | 531 | [Organics/AC hum1.wav, Analog/BrightWhite.wav, S2 Noises/Vinyl Crackle.flac, Organics/Air Can 1.wav, Analog/ARP pink.wav, Analog/ARP white.wav, Organics/AC hum2.wav, Organics/H-Breath.wav, Analog/AlphaNz.wav, S2 Noises/Stretched Vinyl.flac, S2 Noises/FretNoise B.flac, Analog/ARP circuit.wav, Analog/J8.flac, Organics/CymMicBleed.wav, Analog/OrganNoise.wav, Analog/J106 HP.wav, S2 Noises/HP12 White Noise (Stereo).flac, Attacks_Kick/XF_KikAtk_31.wav, Attacks_Kick/XF_KikAtk_32.wav, S2 Noises/FretNoise C.flac, S2 Noises/Rain 100 High.flac, S2 Noises/Crackle.flac, Attacks_Misc/GlassLid 5.wav, Analog/MicrKrg Noise.wav, Attacks_Misc/TransPerc3.wav, Analog/J106 HP_Cho.wav, ...+69] |
| `sampleFromAudioInput` | bool | 2 | |
| `sampleRate` | uint | 624 | 44100..48000 |

### Oscillator.SubOsc

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 506 | [default] |
| `plainParams:kParamContiguousPhase` | f32 | 3 | 1..1 |
| `plainParams:kParamInitialPhase` | f32 | 2 | 130.343..180 |
| `plainParams:kParamShape` | text | 118 | [kTriangle, kRoundRect, kSaw, kSquare, kPulse] |

### VoiceFilter

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 338 | [default] |
| `plainParams:kParamDrive` | f64 | 476 | 0.347793..100 |
| `plainParams:kParamEnable` | f32 | 859 | 1..1 |
| `plainParams:kParamFreq` | f32 | 876 | 0..1 |
| `plainParams:kParamKeyTrack` | f32 | 96 | 1..1 |
| `plainParams:kParamLevelOut` | f32 | 231 | 0..1 |
| `plainParams:kParamPad` | f32 | 25 | 1..1 |
| `plainParams:kParamReso` | f64 | 812 | 0..100 |
| `plainParams:kParamStereo` | f64 | 45 | 29.1084..70.1078 |
| `plainParams:kParamType` | text | 673 | [MgL24, LadderEMS, LadderMg, MgL18, H18, H12, MgL6, H24, Diffuser, LH12, DirtyMg, LadderAcid, HP12, L18, L12, L24, RM, CombP, B12, B24, Wsp, Reverb1, Phase24P, Exp, ExpBPF, LNH24, ...+57] |
| `plainParams:kParamVar` | f64 | 353 | 1.60603..100 |
| `plainParams:kParamWet` | f32 | 152 | 0..98.6842 |
| `plainParams:kParamX` | f32 | 7 | 0.0441989..1 |
| `plainParams:kParamY` | f32 | 6 | 0.025..1 |

### Env

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 1 | [default] |
| `plainParams:kParamAttack` | f64 | 1091 | 0..9.46078 |
| `plainParams:kParamBeatSync` | f32 | 68 | 1..1 |
| `plainParams:kParamCurve1` | f32 | 2498 | 5.88235..93.8442 |
| `plainParams:kParamCurve2` | f64 | 2500 | 0..100 |
| `plainParams:kParamCurve3` | f64 | 2500 | 1.99996e-10..100 |
| `plainParams:kParamDecay` | f64 | 942 | 0..32 |
| `plainParams:kParamEnd` | f32 | 6 | 0.00735295..1 |
| `plainParams:kParamHold` | f64 | 92 | 8.44065e-12..5.13677 |
| `plainParams:kParamLegatoInverted` | f32 | 5 | 1..1 |
| `plainParams:kParamRelease` | f64 | 1305 | 0..32 |
| `plainParams:kParamStart` | f32 | 14 | 0.0808824..1 |
| `plainParams:kParamSustain` | f32 | 924 | 0..0.997664 |

### LFO

| key | type | count | range | enums |
|---|---|---|---|---|
| `curveData` | map | 4556 | |
| `curveData:curveVals` | f32 | 9144 | 1e-12..1 |
| `curveData:curveVals.len` | uint | 1704 | 2..213 |
| `curveData:loopbackPointNum` | uint | 1008 | 0..481 |
| `curveData:numPoints` | uint | 1704 | 1..212 |
| `curveData:xVals` | f32 | 9144 | 0..1 |
| `curveData:xVals.len` | uint | 1704 | 2..213 |
| `curveData:yVals` | f32 | 9144 | 0..1 |
| `curveData:yVals.len` | uint | 1704 | 2..213 |
| `curveDisplayName` | text | 840 | [Custom, Default, triangle 0, 7 SKIES Sig, sine, square, HALF RANDOM, Unstable Triangles, Half pan, SC6, Dualism, triangle, saw down, saw down curved, Wonky Sines, AT Random 01, flat 0, seq 3, Slight Drift, SC4, Sine, Band Pass Shifter, saw up, HalfBar Backbeat 2] |
| `pathData` | map | 6211 | |
| `pathData:isOpen` | bool | 49 | |
| `pathData:numPoints` | uint | 49 | 1..65 |
| `pathDisplayName` | text | 143 | [Default, Custom, Crystals, The 90's, SERUM, CA_Heart, 2 Point Circle] |
| `plainParams` | text | 3608 | [default] |
| `plainParams:kParamAnchored` | f32 | 20 | 0..0 |
| `plainParams:kParamBeatSync` | f32 | 864 | 0..0 |
| `plainParams:kParamDefaultMode` | f32 | 1940 | 0..0 |
| `plainParams:kParamDelay` | f32 | 42 | 0.0175439..3.59862 |
| `plainParams:kParamDirection` | f32 | 21 | 1..2 |
| `plainParams:kParamDotted` | f32 | 338 | 1..1 |
| `plainParams:kParamGridX` | f32 | 60 | 4..32 |
| `plainParams:kParamGridY` | f32 | 22 | 4..32 |
| `plainParams:kParamMode` | text | 1680 | [Free, Envelope] |
| `plainParams:kParamMono` | f32 | 35 | 1..1 |
| `plainParams:kParamPhase` | f64 | 13 | 30..359.161 |
| `plainParams:kParamPhaseSnap` | f32 | 1 | 1..1 |
| `plainParams:kParamRate` | f64 | 1838 | 0..100 |
| `plainParams:kParamRate10x` | f32 | 290 | 1..1 |
| `plainParams:kParamRise` | f32 | 86 | 0.0175439..4 |
| `plainParams:kParamSmooth` | f64 | 159 | 1.04624..100 |
| `plainParams:kParamSwing` | f32 | 6 | 1..1 |
| `plainParams:kParamTriplets` | f32 | 329 | 1..1 |
| `plainParams:kParamType` | text | 502 | [Rossler, Lorenz, RandomSH, Path] |

### Macro

| key | type | count | range | enums |
|---|---|---|---|---|
| `name` | text | 4887 | [REVERB, DELAY, CHORUS, NOISE, WIDTH, FILTER, CUTOFF, DRIVE, DELAY MIX, REVERB MIX, HPF, TONE, DETUNE, LPF CUTOFF, SOFT ATTACK, PHASER, LPF, VERB, LPF FREQ, TIMBRE, CHARACTER, ATTACK FADE, LPF RES, FM, PLUCK, HARMONICS, ...+2122] |
| `plainParams` | text | 2072 | [default] |
| `plainParams:kParamValue` | f64 | 2936 | 0.0808662..100 |

### ModSlot

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams:kParamAmount` | f64 | 13213 | -100..100 |
| `plainParams:kParamAuxCurve` | f64 | 3 | -24.9233..-4.6875 |
| `plainParams:kParamAuxCurveData` | f32 | 95 | 1..1 |
| `plainParams:kParamAuxInverted` | f32 | 52 | 1..1 |
| `plainParams:kParamBipolar` | f32 | 1549 | 1..1 |
| `plainParams:kParamBypass` | f32 | 24 | 1..1 |
| `plainParams:kParamCurveIn` | f32 | 277 | -100..100 |
| `plainParams:kParamCurveOut` | f64 | 51 | -94.5312..100 |
| `plainParams:kParamDelayBeatSync` | f32 | 7 | 1..1 |
| `plainParams:kParamDelayOffset` | f32 | 7 | 0.200131..0.859649 |
| `plainParams:kParamMainCurveData` | f32 | 423 | 1..1 |
| `plainParams:kParamOut` | f64 | 156 | 0..98.6667 |
| `plainParams:kParamSmoothFall` | f64 | 42 | 0.194932..100 |
| `plainParams:kParamSmoothLink` | f32 | 16 | 0..0 |
| `plainParams:kParamSmoothRise` | f64 | 43 | 0.194932..100 |

### Global

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 13 | [default] |
| `plainParams:kParamBendRangeDn` | f32 | 37 | -12..-1 |
| `plainParams:kParamBendRangeUp` | f32 | 38 | 1..24 |
| `plainParams:kParamDirectVol` | f64 | 17 | 0.104032..1.71287 |
| `plainParams:kParamFXBus1Dest` | f32 | 19 | 1..2 |
| `plainParams:kParamFXBus1Vol` | f64 | 47 | 0.00865651..2 |
| `plainParams:kParamFXBus2Dest` | f32 | 8 | 1..2 |
| `plainParams:kParamFXBus2Vol` | f64 | 41 | 0..1.60111 |
| `plainParams:kParamGlobalTuning` | f32 | 2 | 432..435 |
| `plainParams:kParamLegato` | f32 | 74 | 1..1 |
| `plainParams:kParamLimitSameNotePolyphony` | f32 | 171 | 1..1 |
| `plainParams:kParamMasterVolume` | f64 | 568 | 0.0499725..0.885403 |
| `plainParams:kParamMidiOut` | text | 8 | [ClipPlayer, Synth] |
| `plainParams:kParamModWheel` | f64 | 121 | 2.23517e-06..100 |
| `plainParams:kParamMonoToggle` | f32 | 208 | 1..1 |
| `plainParams:kParamNoteLatch` | f32 | 1 | 1..1 |
| `plainParams:kParamOversampling` | f32 | 26 | 0..2 |
| `plainParams:kParamPolyCount` | f32 | 191 | 1..32 |
| `plainParams:kParamPortaAlways` | f32 | 41 | 1..1 |
| `plainParams:kParamPortaScaled` | f32 | 17 | 1..1 |
| `plainParams:kParamPortamentoCurve` | f64 | 5 | 11.25..100 |
| `plainParams:kParamPortamentoTime` | f64 | 121 | 1.34957e-05..2.6146 |
| `plainParams:kParamProgram` | f32 | 32 | 6..32 |
| `plainParams:kParamSwing` | f64 | 27 | 12.5..66.2429 |
| `plainParams:kParamSwingDiv` | f32 | 2 | 1..2 |
| `plainParams:kParamTranspose` | f32 | 5 | -24..12 |
| `plainParams:kParamUseUltraOnRender` | f32 | 7 | 1..1 |
| `plainParams:kParamVoiceAmp` | f32 | 1 | 0.430105..0.430105 |
| `plainParams:kParamVoicePriority` | text | 1 | [Low] |

### Arp

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 389 | [default] |
| `plainParams:kParamActiveClipID` | f32 | 22 | 1..11 |
| `plainParams:kParamEnabled` | f32 | 33 | 1..1 |
| `plainParams:kParamKeyZoneMax` | f32 | 198 | 0..126 |
| `plainParams:kParamKeyZoneMin` | f32 | 9 | 12..84 |
| `plainParams:kParamLaunchQuantize` | f32 | 3 | 9..12 |
| `plainParams:kParamMidiSelectOctave` | f32 | 3 | -2..1 |

### ClipPlayer

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 450 | [default] |
| `plainParams:kParamEnabled` | f32 | 83 | 1..1 |
| `plainParams:kParamMetronomeEnabled` | f32 | 81 | 0..0 |
| `plainParams:kParamMidiSelectOctave` | f32 | 3 | -3..-3 |
| `plainParams:kParamMonoClipTrigger` | f32 | 18 | 0..0 |
| `plainParams:kParamRecordMode` | text | 23 | [Extend] |
| `plainParams:kParamSpanKeyboardClip` | f32 | 13 | 0..5 |

### RoutingSlot

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 2790 | [default] |
| `plainParams:kParamFXBus1Level` | f32 | 163 | 2.63158..100 |
| `plainParams:kParamFXBus2Level` | f32 | 122 | 0.109649..100 |
| `plainParams:kParamFilterBalance` | f32 | 305 | -98.5965..100 |
| `plainParams:kParamRoutingDest` | text | 1442 | [kRoutingDestFilter, kRoutingDestNone, kRoutingDestDirect, kRoutingDestMaster] |
| `plainParams:kParamViaEnv1` | f32 | 49 | 0..0 |

### LFOPointModBus

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 10013 | [default] |
| `plainParams:kParamValue` | f32 | 3 | 0.00438595..1 |

### PitchQuantizer

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 616 | [default] |
| `plainParams:kParamKey` | f32 | 2 | 1..5 |
| `plainParams:kParamScale` | f32 | 9 | 1..51 |
| `scaleName` | text | 626 | [Major, Minor, ---, Minor (Harmonic), Nine Tone, Dorian] |

### RetriggerState

| key | type | count | range | enums |
|---|---|---|---|---|
| `plainParams` | text | 626 | [default] |

### VoicePanel

| key | type | count | range | enums |
|---|---|---|---|---|
| `displayName` | text | 76 | [, Subtle Analog] |
| `plainParams` | text | 574 | [default] |
| `plainParams:kParamGlobalRandomEnvTime` | f64 | 5 | 3..36 |
| `plainParams:kParamGlobalRandomFilterCutoff` | f64 | 5 | 2..38 |
| `plainParams:kParamGlobalRandomOscDetune` | f64 | 16 | 1..26 |
| `plainParams:kParamGlobalRandomOscDetune10x` | f32 | 3 | 1..1 |
| `plainParams:kParamGlobalRandomOscPan` | f64 | 8 | 4.2..52 |
| `plainParams:kParamGlobalScalingEnvTime` | f64 | 2 | 25.704..74.131 |
| `plainParams:kParamGlobalScalingLfoTime` | f64 | 1 | 66.0693..66.0693 |
| `plainParams:kParamGlobalScalingLfoTimeSnap` | f32 | 1 | 1..1 |
| `plainParams:kParamOscA` | f32 | 2 | 0..0 |
| `plainParams:kParamOscB` | f32 | 1 | 0..0 |
| `plainParams:kParamOscC` | f32 | 1 | 0..0 |
| `plainParams:kParamOscN` | f32 | 1 | 0..0 |
| `plainParams:kParamOscS` | f32 | 2 | 0..0 |
| `plainParams:kParamVoice1Detune` | f32 | 28 | -23.3766..15.2886 |
| `plainParams:kParamVoice1EnvTime` | f64 | 1 | -14.2857..-14.2857 |
| `plainParams:kParamVoice1FilterCutoff` | f32 | 5 | -19.4805..3.65646 |
| `plainParams:kParamVoice1Mod1` | f64 | 1 | -25.974..-25.974 |
| `plainParams:kParamVoice1Mod2` | f64 | 1 | -27.2727..-27.2727 |
| `plainParams:kParamVoice1Pan` | f64 | 5 | -8.21883..12.6319 |
| `plainParams:kParamVoice2Detune` | f32 | 31 | -13.3356..19.4805 |
| `plainParams:kParamVoice2EnvTime` | f64 | 1 | 11.6883..11.6883 |
| `plainParams:kParamVoice2FilterCutoff` | f32 | 5 | -5.19481..-1.2987 |
| `plainParams:kParamVoice2Mod1` | f32 | 1 | 3.89611..3.89611 |
| `plainParams:kParamVoice2Mod2` | f64 | 1 | -12.987..-12.987 |
| `plainParams:kParamVoice2Pan` | f64 | 5 | -17.4132..12.6866 |
| `plainParams:kParamVoice3Detune` | f32 | 30 | -14.4785..12.5407 |
| `plainParams:kParamVoice3EnvTime` | f32 | 1 | -7.79221..-7.79221 |
| `plainParams:kParamVoice3FilterCutoff` | f32 | 5 | -19.4805..6.49351 |
| `plainParams:kParamVoice3Mod1` | f32 | 1 | -3.89611..-3.89611 |
| `plainParams:kParamVoice3Mod2` | f64 | 1 | 9.09091..9.09091 |
| `plainParams:kParamVoice3Pan` | f64 | 5 | -29.1045..2.5974 |
| `plainParams:kParamVoice4Detune` | f32 | 29 | -11.3393..10.3896 |
| `plainParams:kParamVoice4EnvTime` | f64 | 1 | -12.987..-12.987 |
| `plainParams:kParamVoice4FilterCutoff` | f32 | 5 | -9.09091..7.79221 |
| `plainParams:kParamVoice4Mod1` | f64 | 1 | -46.7532..-46.7532 |
| `plainParams:kParamVoice4Mod2` | f64 | 1 | 16.8831..16.8831 |
| `plainParams:kParamVoice4Pan` | f64 | 5 | -5.84415..29.8701 |
| `plainParams:kParamVoice5Detune` | f32 | 30 | -14.2857..9.14913 |
| `plainParams:kParamVoice5EnvTime` | f32 | 1 | 3.89611..3.89611 |
| `plainParams:kParamVoice5FilterCutoff` | f32 | 5 | -25.974..6.49351 |
| `plainParams:kParamVoice5Mod1` | f64 | 1 | 20.7792..20.7792 |
| `plainParams:kParamVoice5Mod2` | f64 | 1 | -9.09091..-9.09091 |
| `plainParams:kParamVoice5Pan` | f64 | 5 | -29.4922..17.1642 |
| `plainParams:kParamVoice6Detune` | f32 | 30 | -5.60253..13.2914 |
| `plainParams:kParamVoice6EnvTime` | f32 | 1 | 10.3896..10.3896 |
| `plainParams:kParamVoice6FilterCutoff` | f32 | 5 | -9.09091..1.22857 |
| `plainParams:kParamVoice6Mod1` | f64 | 1 | -37.6623..-37.6623 |
| `plainParams:kParamVoice6Mod2` | f64 | 1 | 18.1818..18.1818 |
| `plainParams:kParamVoice6Pan` | f32 | 4 | -6.71642..27.9779 |
| `plainParams:kParamVoice7Detune` | f32 | 29 | -10.3979..13.0796 |
| `plainParams:kParamVoice7EnvTime` | f64 | 1 | -20.7792..-20.7792 |
| `plainParams:kParamVoice7FilterCutoff` | f32 | 6 | -15.5844..1.2987 |
| `plainParams:kParamVoice7Mod1` | f64 | 1 | -10.3896..-10.3896 |
| `plainParams:kParamVoice7Mod2` | f32 | 1 | -5.19481..-5.19481 |
| `plainParams:kParamVoice7Pan` | f64 | 5 | -17.1038..5.84416 |
| `plainParams:kParamVoice8Detune` | f32 | 29 | -14.2857..3.62672 |
| `plainParams:kParamVoice8EnvTime` | f64 | 1 | -20.7792..-20.7792 |
| `plainParams:kParamVoice8FilterCutoff` | f32 | 6 | -11.6883..5.19481 |
| `plainParams:kParamVoice8Mod2` | f32 | 1 | -5.19481..-5.19481 |
| `plainParams:kParamVoice8Pan` | f64 | 5 | -23.3766..34.9711 |
| `plainParams:kParamVoiceCount` | f32 | 1 | 2..2 |

### FXRack

| key | type | count | range | enums |
|---|---|---|---|---|
| `FX:flex:val` | f32 | 1959 | 0..8 |
| `FX:kUIParamMixOrGain` | f32 | 2661 | 0..1 |
| `FX:type` | uint | 4168 | 0..15 |
| `displayName` | text | 1878 | [, Vintage Chorused, EPiano 1] |
| `plainParams` | text | 1878 | [default] |
| `proxyParams` | map | 1 | |


## 9. Authored UI keys and metadata

### authored UI keys (ui:)

| key | type | count | range | enums |
|---|---|---|---|---|
| `ClipPlayer:kUIParamPianoRollNotePreview` | f32 | 76 | 1..1 |
| `ClipPlayer:kUIParamPreviewClip` | f64 | 626 | 0..1 |
| `ClipPlayer:kUIParamSelectedClip` | f32 | 626 | 0..1 |
| `Filter:kUIParamMixOrGain` | f32 | 626 | 0..1 |
| `GranularOsc[0]:kUIParamDisplayXYInput` | f32 | 620 | 0..0 |
| `GranularOsc[1]:kUIParamDisplayXYInput` | f32 | 620 | 0..0 |
| `GranularOsc[2]:kUIParamDisplayXYInput` | f32 | 620 | 0..0 |
| `MultiSampleOsc[0]:kUIParamMultiSampleOverviewMouseTag` | f32 | 626 | 0..1 |
| `MultiSampleOsc[1]:kUIParamMultiSampleOverviewMouseTag` | f32 | 626 | 0..1 |
| `MultiSampleOsc[2]:kUIParamMultiSampleOverviewMouseTag` | f32 | 626 | 0..1 |
| `Osc[0]:kUIParamAutoSyncSlicing` | f32 | 626 | 0..0 |
| `Osc[0]:kUIParamShowMarkerAnimations` | f32 | 626 | 0..1 |
| `Osc[0]:kUIParamZoomToStartEnd` | f32 | 626 | 0..0 |
| `Osc[1]:kUIParamAutoSyncSlicing` | f32 | 626 | 0..0 |
| `Osc[1]:kUIParamShowMarkerAnimations` | f32 | 626 | 0..0 |
| `Osc[1]:kUIParamZoomToStartEnd` | f32 | 626 | 0..1 |
| `Osc[2]:kUIParamAutoSyncSlicing` | f32 | 626 | 0..0 |
| `Osc[2]:kUIParamShowMarkerAnimations` | f32 | 626 | 0..1 |
| `Osc[2]:kUIParamZoomToStartEnd` | f32 | 626 | 0..1 |
| `Osc[3]:kUIParamAutoSyncSlicing` | f32 | 626 | 0..0 |
| `Osc[3]:kUIParamShowMarkerAnimations` | f32 | 626 | 0..0 |
| `Osc[3]:kUIParamZoomToStartEnd` | f32 | 626 | 0..0 |
| `Osc[4]:kUIParamAutoSyncSlicing` | f32 | 626 | 0..0 |
| `Osc[4]:kUIParamShowMarkerAnimations` | f32 | 626 | 0..0 |
| `Osc[4]:kUIParamZoomToStartEnd` | f32 | 626 | 0..0 |
| `SerumGUI:kUIParamShowKeyboard` | f32 | 626 | 1..1 |
| `SerumGUI:kUIParamShowMidiOut` | f32 | 79 | 0..1 |
| `SpectralOsc[0]:kUIParamDisplayXYInput` | f32 | 620 | 0..0 |
| `SpectralOsc[0]:kUIParamShowWaveformDisplay` | f32 | 620 | 0..0 |
| `SpectralOsc[1]:kUIParamDisplayXYInput` | f32 | 620 | 0..0 |
| `SpectralOsc[1]:kUIParamShowWaveformDisplay` | f32 | 620 | 0..0 |
| `SpectralOsc[2]:kUIParamDisplayXYInput` | f32 | 620 | 0..0 |
| `SpectralOsc[2]:kUIParamShowWaveformDisplay` | f32 | 620 | 0..0 |
| `WTOsc[0]:kUIParamWTOverviewMouseTag` | f32 | 626 | 0..1 |
| `WTOsc[1]:kUIParamWTOverviewMouseTag` | f32 | 626 | 0..1 |
| `WTOsc[2]:kUIParamWTOverviewMouseTag` | f32 | 626 | 0..1 |

### meta

| key | type | count | range | enums |
|---|---|---|---|---|
| `arpBankDisplayName` | text | 626 | [, 5 Note 8 Step, acid] |
| `clipBankDisplayName` | text | 626 | [, Init, Duda Chords, Darkness, CA_Above, Lofi YACHT, Kit Drum Patterns, Aux Grooves 1, Drum Machine Basics, Aggro Git, One Bar Cluster Chords, New Dawn vinyl, Cowboy Chords, Dracula Reborn] |
| `fileType` | text | 626 | [SerumPreset] |
| `lockOversampling` | bool | 626 | |
| `lockTuning` | bool | 626 | |
| `mpeConfig` | uint | 626 | 0..0 |
| `mpeEnabled` | bool | 626 | |
| `mpePitchBendRange` | uint | 626 | 48..48 |
| `presetAuthor` | text | 626 | [Electric Himalaya, Steve Duda, SynthHacker, Level 8, Tunecraft, Splice, @mrbillstunes, Audiotent, J. Scott G. / Libra Rising, CFA-Sound , Van Derand, Endov Lane, CFA-Sound, Beatdemon, Caster, 7 SKIES, Alice Efe, NEST Acoustics, Matt Aimonetti, DnBline Smith, Gigantor, Paul Laski (P-LASK), Xfer, LP24 Audio, Wisteria Motif, Shreddward, ...+15] |
| `presetDescription` | text | 626 | [, www.audiotent.com,  , beatdemon.com, Mercurial Tones, librarisingmusic.com, http://standalone-music.com, www.tunecraft-sounds.com, https://sonicarmory.com, MW= Vibrato, Play Chords / Try changing Clip and Arp patterns, Orchestral Layers, Everyone needs analog pads, Glassy pluck great for melodic riffs, Turn on CLIP for preview & use case, Melancholic ambient wash, Ring-Mod + FM Bells, delicate yet complex, play that funky riff!, fat and filthy FM bass, Mod Wheel for Cutoff, PWM lead mixed with lush Solina layer, Orchestral layers, You want warm pads, wa got warm pads, Hold a chord..., needs a 4-4 beat, ...+344] |
| `presetName` | text | 626 | [ARP - Aardvark, ARP - Acid101, ARP - Altar, ARP - Bell Arperium, ARP - Blossom Tree Sprites, ARP - Cascading Etheric Paths, ARP - Cosmic Lives Forever, ARP - Daftronic, ARP - Etheric Aluminium, ARP - Fun ElectroAcoustic Jam, ARP - Kalimba Arps, ARP - Legend Epix, ARP - Mallet Magium, ARP - Opus Arpeggio, ARP - Pizz Orch Arp, ARP - Proudly Digital, ARP - Rainfall, ARP - Strum Multi, 808 - Decomposed, 808 - Diesel, 808 - Drill, 808 - Simple Electro, 808 - Straight, 808 - Texture, 808 - To 909 BD Maker, BA - 303 Die Treppe, ...+600] |
| `product` | text | 626 | [Serum2, Serum2FX] |
| `productVersion` | text | 626 | [2.0.11, 2.0.12, 2.0.13, 2.0.14, 2.0.15] |
| `tags` | text | 2193 | [Preview, Wavetable, Poly, Mono, Multisample, Sample, Granular, Spectral, Arp, Clip, Embedded-Data, KB-Span, Custom-Tuning] |
| `url` | text | 626 | [https://xferrecords.com/] |
| `vendor` | text | 626 | [Xfer Records] |
| `version` | f32 | 626 | 3..6 |


## 10. Reproduction

* Scan script: `conv_work\s2corpus\scan_corpus.py` (parses all 626 presets,
  writes the `.txt` dumps + `param_stats_raw.json` + `per_preset_summary.json`).
* JSON assembly: `conv_work\s2corpus\build_json.py` →
  `conv_work\s2corpus\s2_param_corpus.json`.
* Corpus dumps: `corpus_sections.txt` (all 752 paths w/ stats),
  `fx_types.txt`, `modslot_dest.txt`, `modslot_src.txt`, `curve_shapes.txt`,
  `top_keys.txt`, `meta_tags.txt`.
* Source-enum decode: `Serum2.vst3` x64 string table at RVA 0xa20820
  (script `conv_work\s2corpus\pe_strings.py`); intra-preset verification in
  `src_attr.py` / `src_ctx.py`.

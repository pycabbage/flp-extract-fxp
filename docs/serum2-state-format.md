# Serum2 Component State Format (VST3 `IComponent::setState/getState`)

Status: **verified against real Serum2 2.0.23 (x64 Windows) round trips** captured via a
ctypes/VST3 harness (`state_experiment.py`, `serum2_probe.py`; see
`docs/serum2-dynamic-verification.md`). Ground-truth bodies re-decoded in this session:

| tag | source file (state container) | container size | decompressed body |
|---|---|---|---|
| `init` | `state_default.bin` | 1 252 B | **6 460 B** |
| `after05` | `state_after_05_BS - YUKIYANAGI UKHC BASS 01.fxp.bin` | 33 908 B | **455 171 B** |
| `native` | `state_after_state_02_Serum2.bin.01.cid3.bin.bin` | 33 914 B | **455 157 B** |
| `fileA` (reference) | `fileA_inner.bin` = body of the *original* FL-extracted `state_02_Serum2.bin.01.cid3.bin` | (33 751 B) | **454 879 B** |

All four bodies parse as **one standard CBOR (RFC 8949) document each**, with **zero
unexplained bytes** (see §7).

---

## 1. State container recap (verified)

```
offset  size  content
0x00    9     magic: "XferJson\0"
0x09    8     u64 LE  = length of JSON header text (183)
0x11    183   JSON header (ASCII), keys sorted:
              {"component":"processor","hash":"<md5>","product":"Serum2",
               "productVersion":"2.0.23","url":"https://xferrecords.com/",
               "vendor":"Xfer Records","version":9.0}
0xC8    4     u32 LE = decompressed body size (exact)
0xCC    4     u32 LE = format (2)
0xD0    ..    one or more zstd frames until EOF
```

* `hash` = **md5 of everything from the first zstd magic (offset 0xD0) to EOF** —
  re-verified for all three ground-truth states.
* Declared size at 0xC8 == actual decompressed size (re-verified on all three).
* All observed states carry exactly **one** zstd frame; frame parameters:
  `window_size == frame_content_size` (content size written into the frame header,
  window shrunk to fit), `has_checksum = false`, `dict_id = 0`.
* Note the 9-byte magic: `XferJson` + NUL, *then* the u64 length (offset 9, not 8).

---

## 2. Body protocol: standard CBOR

The decompressed body is **a single CBOR map with 162 text-keyed entries** — Serum2's
"plain parameter" serialization (not JSON, not msgpack; `XferJson` refers to the
*container*, the body itself is CBOR). Byte-level grammar (all three bodies conform):

| type | first byte(s) | notes |
|---|---|---|
| map | `0xA0+n` (n<24), `0xB8 +u8` (24…255), `0xB9 +u16`… | top map = `b8 a2` = 162 entries |
| array | `0x80+n` (n<24), `0x98 +u8`… | |
| text | `0x60+n` (n<24), `0x78 +u8` (24…255), `0x79 +u16`… | UTF-8, no NUL |
| integer ≥ 24 | `0x18 +u8` (also `0x19 +u16`, …) | CBOR major 0 |
| negative int | `0x20+n` (e.g. `20` = −1) | CBOR major 1 |
| float32 | `0xFA` + f32 big-endian | |
| float64 | `0xFB` + f64 big-endian | |
| `false` | `0xF4` | used by `lockOversampling`, `lockTuning`, `mpeEnabled` |
| `null` | `0xF6` | 15 545 occurrences in the Baby Grand body (empty `expressionEvents` arrays etc.) |
| **byte string** | major 2 (`0x40…`) | **count = 0 in all three bodies** — no binary blobs exist |

Numeric value rule (observed, relevant when *writing*): scalars are encoded as
`float32` when the value is exactly representable in f32 (e.g. `50.0`, `450.0`,
`1.0`), otherwise as `float64` (`66.6`, `49.99999999999999`). Enum parameters are
encoded as **text**, not ints: `'Free'`, `'Extend'`, `'no_loop'`, `'kDiode2'`,
`'kDownsample'`, `'kOsc_MultiSample'`. Reader accepts either float width (the
2.0.22 file `fileA_inner` uses more f32 and loads fine; see §4).

### 2.1 Node grammar (module-instance records)

The top map is a **flat registry of module instances**, keys `<Section><Index>`
(e.g. `Oscillator0`, `ModSlot23`, `MidiClip11`). Instance value grammar:

```
<Section><N>    := map {
  "<SubEngineType><N>" := map{          # exactly the engine type that is ACTIVE,
      ...engine fields...               # index suffix matches the section
  },                                    # inactive engines appear with {plainParams:"default"}
  "plainParams" := text "default"       # nothing customized
                |  map{ "<kParamX>" := value, ... }   # parameter records
  [optional per-section fields, e.g. Arp0:"activeClip", ModSlot:"destModule*"/"source"]
}
```

Verified example — init (`body_init.bin`, byte offsets verified by the parser of §7):

```
b8 a2                     map[162]           @0x0000 (marker @0, count byte @1)
  64 "Arp0"                                  @0x0002
  a2                     map[2]              @0x0007
    6a "activeClip"                        @0x0008
    20 (= -1)                              @0x0013
    6b "plainParams"                       @0x0014
    67 "default"                           @0x0020
  68 "ArpClip0"                              @0x0028
  a2                     map[2]  {"clip": map[0], "plainParams": "default"}
  ...
  6d "Oscillator0"                           @0x123F (value @0x124B)
  a6                     map[6]  { GranularOsc0, MultiSampleOsc0, SampleOsc0,
                                     SpectralOsc0, WTOsc0, plainParams }
      68 "WTOsc0"                            @0x12D5
      a6  map[6] { flex: map[0], numChannels: 1, numFrames: 18432,
                   plainParams: "default",
                   relativePathToWT: "S2 Tables/Default Shapes.wav",
                   sampleRate: 44100 }
      6b "plainParams"  67 "default"         (Oscillator-level params: none)
  ...
  67 "version"  fa 41 10 00 00   (f32 9.0)   key @0x192F, value @0x1937, ends 0x193C
```

Verified example — loaded native state (`body_native.bin`, Baby Grand multisample):

```
"Oscillator0" : map[6] {
  "GranularOsc0"   : { "plainParams": "default" },
  "MultiSampleOsc0": { "defaultLoopMode": "no_loop",
                       "embedded_sfz":   "// SFZ Generated by libSFZ\n<group> ... <region> ... sample=Baby Grand Samples/XFBabyGrand 05 A-1.flac\n...",
                       "files": { "Baby Grand Samples/XFBabyGrand 05 A-1.flac":
                                    {"numChannels":2, "numFrames":440388, "sampleRate":44100},
                                  ...28 file entries... },
                       "plainParams": "default" },     # 5 keys total
  "SampleOsc0"     : { "plainParams": "default" },
  "SpectralOsc0"   : { "plainParams": "default" },
  "WTOsc0"         : { "plainParams": "default" },
  "plainParams"    : { "kParamType": "kOsc_MultiSample", ... } }
```

### 2.2 Complete section inventory (all 162 top-level keys)

* Meta (13): `component`, `lockOversampling`, `lockTuning`, `mpeConfig`,
  `mpePitchBendRange`, `mpeEnabled`, `product`, `productVersion`, `scalars`
  (`note`, `velo`), `tags`, `url`, `vendor`, `version`.
* Modules (149):
  `Arp0`, `ArpClip0..11` (12), `ClipPlayer0`, `Env0..3` (4), `FXRack0..2` (3),
  `Global0`, `LFO0..9` (10), `LFOPointModBus0..15` (16), `Macro0..7` (8),
  `MidiClip0..11` (12), `ModSlot0..63` (64), `Oscillator0..4` (5),
  `PitchQuantizer0`, `RetriggerState0`, `RoutingSlot0..6` (7), `VoiceFilter0..1` (2),
  `VoicePanel0`.

`tags` is `array[2]` of text — first tag = main osc type badge
(`'Wavetable'` init / `'Multisample'` Baby Grand), second always `'Poly'`.

---

## 3. What "loading a preset" changes — the mapping surface

**Honest caveat about the ground truth:** the `after05` state was captured after
`setState()` of a real Serum fxp — but the call returned `kResultFalse` and
Serum2 **kept the previously loaded native state**. Proof:

* `body_after05` and `body_native` differ by exactly **one leaf**
  (`Oscillator1.MultiSampleOsc1.files."…XFBabyGrand 06 D#2.flac".sampleRate`, present only in after05);
* the state captured after setState of `state_06_Serum.bin.01.cid3.bin` (a *different*
  Serum state) is **byte-identical** to `after05` (same md5 `837c23ce62…`);
  a real YUKIYANAGI conversion cannot contain "Baby Grand" multisample data.

So Serum2 does **not** import Serum data through `setState`; the plugin must
already be given (or generate) a native state. The init↔native-state delta below is
therefore the mapping surface an offline converter must produce (wavetable-based
Serum presets would materialize as `WTOsc` sections + table files, cf. §4):

| section | records (init → loaded) | example changed/added records |
|---|---|---|
| `Arp0` | value change | `activeClip: -1 → 0` |
| `MidiClip0..11` | `clip` maps populated (54 424 new leaf records across 12 clips) | `MidiClip0.clip.notes#0.{attributes[8], channel=4, expressionEvents[5]=null, length=0.5, noteNum=40, timeStamp=0.0}`; `clip.automation#0.{dest='kAutoDest_ChanPressure', timeStamps[9], values[9]}` |
| `Oscillator0..2` | 11→101 each | `plainParams.kParamType = 'kOsc_MultiSample'` (new), `MultiSampleOsc0.{defaultLoopMode='no_loop', embedded_sfz=<SFZ text>, files{28×{numChannels,numFrames,sampleRate}}}` |
| `Oscillator3` (noise) | 7→10 | `NoiseOsc3.numChannels 1→2`, `numFrames 78241→100992`, `relativePathToNoiseSample 'Organics/AC hum1.wav' → 'Attacks_Misc/icon_kick a.wav'` |
| `FXRack0` | 2→131 | `FX` array 0→18 entries; each entry `{type:<uint>}` + typed submap, e.g. `#0 type=14 FXSplit3{freq,freq2,moduleCount*}`, `#1 type=5 FXComp{attack,release,ratio,thresh,…}`, `#4 type=1 FXFlanger{depth,rate,feedback,wet,…}`; off slots keep `type=0` + `flex[2]` |
| `ModSlot0..23` | 1→7 each (inactive 24..63 stay 1) | `destModuleID=3, destModuleParamID=1, destModuleParamName='kParamVolume', destModuleTypeString='Oscillator', plainParams.kParamAmount=28.63…, source=[3,0]` |
| `Env0..3` | 3→5/7 | `kParamCurve1 50.0(f32) → 49.99999999999999(f64)`, `kParamCurve2/3 66.6`, `kParamAttack`, `kParamSustain`, `kParamVoiceStealRestart` appear |
| `LFO2`/`LFO3` | `plainParams` records appear (3 resp. 16 new leaves); LFO3 also gets populated `curveData` | `kParamMode = 'Free'` (text), `kParamRate = 1.6269…`; `LFO3.curveData = {curveVals[4], numPoints=3, xVals[4], yVals[4]}` (arrays carry numPoints+1 entries) |
| `Global0` | 1→4 | `kParamGlobalTuning=450.0, kParamMasterVolume=0.7079…, kParamModWheel=4.6875, kParamPolyCount=18.0` |
| `ClipPlayer0` | 1→3 | `kParamMetronomeEnabled=0.0, kParamRecordMode='Extend', kParamRegionOffsetQuantize=7.0` |
| `Macro0..7` | leaf-path replaced | `Macro0.plainParams = 'default'` → `Macro0.plainParams.kParamValue = 52.43…` |
| `tags` | same 2 | `#0 'Wavetable' → 'Multisample'` |
| unchanged | `VoicePanel0`, `VoiceFilter*`, `RoutingSlot*`, `RetriggerState0`, `PitchQuantizer0`, `ArpClip*`, `LFOPointModBus*`, `scalars`, meta | stay `plainParams:"default"` |

Quantitative diff (`body_init.bin` vs `body_after05.bin`, leaf = scalar/text/bool/null
record): **270 → 55 516 leaves**; 197 leaf-paths common (**17 with changed values**),
**55 319 new**, **73 init leaf-paths superseded** (the parent map became a populated
container, e.g. `MidiClip0.clip` or `Oscillator0.plainParams`). New/changed leaves per
section:
MidiClips 54 424, Oscillator0-2 540, ModSlots 170, FXRack0 130, Env0-3 24, LFOs 19,
Oscillator3 8, Global0 4, ClipPlayer0 3, VoiceFilter0 3, Macros 5, RoutingSlots 4,
Arp0 1, tags 1.

30 example records (name: init → loaded):

```
 1. Arp0.activeClip:                                  -1 → 0
 2. Env0.plainParams.kParamCurve1:                    f32 50.0 → f64 49.99999999999999
 3. Env0.plainParams.kParamCurve2:                    66.60000000000001 → 66.6
 4. Env0.plainParams.kParamCurve3:                    66.60000000000001 → 66.6
 5. Env1.plainParams.kParamCurve1:                    f32 50.0 → f64 49.99999999999999
 6. Env1.plainParams.kParamCurve2:                    66.60000000000001 → 66.6
 7. Env1.plainParams.kParamCurve3:                    66.60000000000001 → 66.6
 8. Env2.plainParams.kParamCurve1:                    f32 50.0 → f64 49.99999999999999
 9. Env2.plainParams.kParamCurve2:                    66.60000000000001 → 66.6
10. Env2.plainParams.kParamCurve3:                    66.60000000000001 → 66.6
11. Env3.plainParams.kParamCurve1:                    f32 50.0 → f64 49.99999999999999
12. Env3.plainParams.kParamCurve2:                    66.60000000000001 → 66.6
13. Env3.plainParams.kParamCurve3:                    66.60000000000001 → 66.6
14. Oscillator3.NoiseOsc3.numChannels:                1 → 2
15. Oscillator3.NoiseOsc3.numFrames:                  78241 → 100992
16. Oscillator3.NoiseOsc3.relativePathToNoiseSample:  'Organics/AC hum1.wav' → 'Attacks_Misc/icon_kick a.wav'
17. tags.#0:                                          'Wavetable' → 'Multisample'
18. Oscillator0.MultiSampleOsc0.defaultLoopMode:      (new) 'no_loop'
19. Oscillator0.MultiSampleOsc0.embedded_sfz:         (new) SFZ text (~3 KB)
20. Oscillator1.MultiSampleOsc1.defaultLoopMode:      (new) 'no_loop'
21. Oscillator1.MultiSampleOsc1.embedded_sfz:         (new) SFZ text (~11 KB)
22. Oscillator2.MultiSampleOsc2.defaultLoopMode:      (new) 'no_loop'
23. Oscillator3.NoiseOsc3.plainParams.kParamColor:    (new) 0.5651359558105469
24. Oscillator3.NoiseOsc3.plainParams.kParamFine:     (new) -1.0
25. Oscillator3.NoiseOsc3.plainParams.kParamOneShot:  (new) 1.0
26. Env0.plainParams.kParamDecay:                     (new) 0.3241028521069286
27. Env0.plainParams.kParamRelease:                   (new) 0.004774210692838813
28. Env3.plainParams.kParamAttack:                    (new) 0.0
29. Env3.plainParams.kParamDecay:                     (new) 0.45328633135764806
30. Env3.plainParams.kParamSustain:                   (new) 0.0
```

---

## 4. Verdict: what an offline Serum→Serum2 state converter must produce

**Verdict: (i) — the body is fully converted typed parameters.** There is no raw
Serum chunk, no embedded wavetable bytes, no base64/zlib, no CBOR byte-strings
anywhere in any ground-truth body. The plugin does not accept Serum data through
`setState` at all (`kResultFalse` in every recorded attempt), so there is no
"inject raw Serum state" shortcut. Note the flip side: `embedded_sfz` is stored
as **plain text** inside the body (that is how the Baby Grand multisample rides in a
state), but sample *audio* never is.

A converter must therefore emit, offline:

1. **Body**: one CBOR map with exactly the 162 keys of §2.2 (the parser in §7 is the
   ground truth for the shape). Parameter records use Serum2 `kParamXxx` names
   (text keys) with the value encodings of §2 — f32 when exact, else f64; enums as
   text; inactive engines as `{plainParams:"default"}`.
2. **Serum → Serum2 parameter mapping** — the part that must be built from
   Serum fxp params (`docs/serum-fxp-format.md`) into: `OscillatorN.plainParams`
   (`kParamEnable`, `kParamVolume`, `kParamType`, loop points, …), `EnvN.plainParams`
   (`kParamAttack/Decay/Sustain/Release`, `kParamCurve1..3`), `LFOk.plainParams` +
   `LFOk.curveData` (`{curveVals, numPoints, xVals, yVals}`, arrays = numPoints+1),
   `ModSlotI` (destModuleID/destModuleParamID/destModuleTypeString/source pair/kParamAmount),
   `FXRack0.FX` entries (type uint + `FX<Name>` submap with `kParamXxx`), `Global0`,
   `MacroK.kParamValue`, `tags[0]` (`'Wavetable'`), meta `version`/`productVersion`.
3. **Wavetable/samples as files**: the state stores audio data **by reference** —
   `WTOsck.relativePathToWT` (`'S2 Tables/Default Shapes.wav'`, resolved under
   `Documents\Xfer\Serum 2 Presets\Tables\`), `NoiseOscK.relativePathToNoiseSample`
   (`Organics/AC hum1.wav`), `MultiSampleOscK.files` keyed by relative `.flac` paths
   (samples under `…\Multisamples\Factory\Keys\Baby Grand Samples\`). The 288 KiB
   YUKIYANAGI wavetable must therefore be **written to disk as a `.wav`** (numFrames /
   numChannels / sampleRate must be duplicated in the CBOR record) and referenced by
   a relative path; it can never live inside the 33.9 KB container.
4. **Container**: recompress body with zstd (any level works for acceptance by a
   standard decoder; see §5), then rebuild: `XferJson\0` + u64(183) + JSON header
   (recompute `hash` = md5 of zstd stream, set `productVersion`/`version` to values
   the installed build accepts) + u32 bodyLen + u32 format=2 + frames.
   Observed acceptance evidence: the installed 2.0.23 loaded a 2.0.22 state
   (`version` f32 8.0, `productVersion '2.0.23'→ '2.0.22'`) and re-emitted it as
   `version 9.0` / `'2.0.23'` — i.e. it upgrades on load; emitting `9.0`/`2.0.23`
   is the safe target.

Round-trip quirks observed (do not rely on re-serialization for fidelity):
on `getState` after `setState` of a foreign version-8 state, Serum2 2.0.23 re-emits
f64 for f32-exact values after its internal f64 pipeline (`50.0 → 49.99999999999999`),
bumps `version` 8.0→9.0, and **drops** `PitchQuantizer0.scaleName ('Major' → '')`
and `scale` entries (`2 → 0`).

---

## 5. zstd compression parameters (decompression-only test)

Recompressed `body_native.bin` with python-`zstandard`
(`ZstdCompressor(level=L, write_content_size=True, write_checksum=False,
write_dict_id=False)`), for L ∈ {3, 9, 15, 19, 22}; all five streams round-trip
byte-identically through a standard decompressor and produce frames with
`window_size = frame_content_size = 455157`, `has_checksum = false`, `dict_id = 0` —
the same parameter class Serum2 itself emits (Serum2's own frame of the same body
is 33 706 B; L=3 gives 33 660 B, so its producer is roughly a low zstd level).
Plugin acceptance was **not** tested (no harness run was performed per rules);
correctness of `md5 hash` + declared size were verified instead.

**Update (real zstd in the converter)**: the converter now emits genuine
libzstd level-3 frames (`zstd::bulk::compress(data, 3)`, content size
declared, single-segment frames chosen by libzstd itself: 1/2/4-byte FCS
depending on body size). These have been **dynamically verified**: every
converted state of the sample project was accepted by the real plugin via
`setState` (kResultOk, valid post-state hashes, post-states matching the real
importer's). Standard frames of any level are accepted; level 3 matches the
golden states' size class. Frame-header parsing note: for a 1-byte FCS field
(FCS flag 0 + single segment) the stored byte is the raw size, no offset —
only the 2-byte field (flag 1) carries a +256 offset.

## 6. Reproduction pointers

* Container parser/decompressor: `conv_work/scripts/decomp_states.py` (probe dir)
* CBOR decoder w/ byte accounting: `conv_work/scripts/cbor_probe.py`
* Leaf diff: `conv_work/scripts/diff_bodies.py`, section dumps:
  `dump_leaves.py`, `dump_sections.py`, `struct1.py`, `struct2.py`

## 7. Parser verification (task 5)

`conv_work/scripts/parse_fileA.py` walks the entire `fileA_inner.bin`
(454 879 B) record-by-record with a strict CBOR decoder that consumes every byte:

```
root: map[162], parse ends at offset 454879 == 454879 -> unexplained bytes: 0
total CBOR nodes: 65366
  container nodes: 9902 (map 3611, array 6291; 41 of them empty)
  non-container records: 55464
record-type histogram (non-container records):
  27371  float64          15545  null
   6848  uint             5494  float32
     203  text                 3  false
```

**zero unexplained bytes**; the top-level key list matches §2.2 exactly. The same
decoder also fully parses `body_init.bin` (6 460 B, 270 records), `body_after05.bin`
(455 171 B) and `body_native.bin` (455 157 B) with zero unexplained bytes in each.

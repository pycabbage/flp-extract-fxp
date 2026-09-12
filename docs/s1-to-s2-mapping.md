# Serum → Serum2 preset conversion — static RE of `Serum2.vst3` 2.0.23

Target: `C:\Program Files\Common Files\VST3\Serum2.vst3\Contents\x86_64-win\Serum2.vst3`
(SHA256 `9293EB90…BF9B3`, ImageBase 0x180000000, all RVAs below = VA − 0x180000000).
Static analysis only (PE/capstone; no execution of the plugin). All addresses cited as
RVA (VA = 0x180000000 + RVA). Companion docs: `serum2-importer-analysis.md`
(validation + `s1state_load` prologue), `serum2-state-format.md` (CBOR body),
`serum-fxp-format.md` (S1 chunk layout). Scratch scripts/dumps:
`C:\Users\cabbage\AppData\Local\Temp\opencode\conv_work\s2re\` (`s1load.txt` =
full annotated disassembly of 0x4DABC0–0x4E61CA, 8 770 instructions).

## 0. Executive answer

There is **one** conversion function: `s1state_load` **0x4DABC0–0x4E61CA** (~46 KB).
Everything the importer produces — the CBOR/JSON parameter tree, FX rack, mod
matrix, LFO curves, wavetable references, embedded audio blobs, metadata — is
built *inside that single function* as a `nlohmann::json` tree (which is later
CBOR-encoded by the state serializer). It never calls the plugin engine; it
writes a JSON document whose keys are exactly the `kParamXxx`/section names of
the serialized state. Sections not covered by S1 data stay untouched (they keep
whatever was in the JSON before / defaults).

## 1. Call graph (verified)

```
s1state_load(rcx=out ctrl, rdx=json*, r8=state buf 172736B, r9=len, [+5 flag, +6 name])
│
├─ 0x96eaa0 / 0x96eadc          operator new / delete
├─ 0x9e2e30  memset(state, 0, 172736)
├─ 0x552a20  bounds-checked copy of chunk (truncation tolerated)
├─ 0x4db45b..0x4db525  version float (state+0x4994) gates: <0.002 fail,
│                      <0.009 "Patch is Old!!!" warn, >0.999 "newer version" fail
├─ 0x4db533  legacy migration loop (version < 0.01): FX mirror dword shift
│            state+0x3940/44/48 → +8  (3 floats per 0x13A record, ~0x47 records)
├─ 0x4db956..  version-gated data migrations (§10)
├─ 0x4dd98c..0x4ddd57  "+ FX" rack-slot inversion of order table (state+0x3BE0 int32[10])
├─ 0x4ddd5d..0x4de26f  "lfophasor" defaults + "+ FX".lfo = 2x0x2D28 phasor blocks
│                      (only when version > 0.161; static json 0x9F0720/05E0/0620/0820)
├─ 0x4de27b..0x4de6c6  LFO blocks 1–8  → modern graph blocks (fn_4f1eb0 normalizer,
│                      fn_4f2a70 curveData writer; classic-layout source at state+0x84E0
│                      or +0x5558, 8 x 0x2D28)
├─ 0x4de6cc..0x4de797  LFO 9–10 "flex"  (state+0x1EE20, 2 x 0x2D28, fn_4f1eb0/fn_4f2a70)
├─ 0x4dea8c..0x4deb1e  scalars.velo / scalars.note curve blocks (fn_4f3e00, state+0x4220)
├─ 0x4deb2b..0x4decbe  MASTER PARAM LOOP (see §3)
├─ 0x4decf6  version-float compare + noise-rescale migration (0x4DD6AD family)
├─ 0x4ded25..0x4dee23  lfoPointModAssignments[]  (state+0x6E48, 0x2C-stride, count state+0x8448)
├─ 0x4dee80..0x4df16d  S1 mod-slot staging: per-slot dest/src code migrations +
│                      S2 mod amounts idx 0xDE (default 1.0), 0xB4+2k, 0xB5+2k  (§4.1)
├─ 0x4df170..0x4dfce7  ModSlot node builder: kParamCurveIn / kParamAuxCurve /
│                      kParamBipolar / kParamAuxInverted / kParamBypass / source /
│                      destModuleParamID / destModuleTypeString / destModuleID  (§4.2)
├─ 0x4dfd25..0x4e0025  midiMap {ccNum, paramIDs[]}  (state+0x3840 byte[247], +0x5360)
├─ 0x4e008f..0x4e02ad  mixOrGain1..10 → "+ FX"[order[i]].plainParams (state+0x3976/78, 0x4BD4)
├─ 0x4e02ce..0x4e041e  presetName (state+0x4972) / presetAuthor (0x49A0) / presetDescription (0x49D0)
├─ 0x4e0423..0x4e083c  WTOsc / NoiseOsc name fields 0x3C08/0x3E08/0x4008 → relativePathToWT,
│                      pathToNoiseSample, relativePathToNoiseSample; "Audio In" →
│                      sampleFromAudioInput (0x4E0C2F)
├─ 0x4e083e..0x4e08cc  embedded WT migration (0x24EAC buffer, 0x124 shift, 0x18F8 copy)
├─ 0x4e08cc..0x4e0ab0  macro names 0x4A60[4x32] → "name1..4" (version > 0.134)
├─ 0x4e0ab2..0x4e0b4f  lockOversampling / lockTuning (state+0x4A50 bits 1/2)
├─ 0x4e0b54..0x4e0d5e  appended-streams accounting (W bytes → state+0x4968/0x5540/0x5544)
├─ 0x4e0d5e..0x4e1884  embeddedWTData / tuningData / loopback64 / boundary64 /
│                      embeddedNoiseData writers (fn_4f4c40/cd0/d60/dc0, fn_4f4b10/bb0)
├─ 0x4e1885..0x4e19ae  WTOsc.kUIParamWTOverviewMouseTag ← state+0x4998 / +0x499C (version > 0.03)
├─ 0x4e19af..0x4e1a0f  defaults: S2 idx 0x14C/0x14D = 1.0, 0x14E/0x14F = 0.0 (version > 0.146)
├─ 0x4e1a17..0x4e1bde  storedPhasePos (2 x 0x88-byte phasor blocks, state+0x5418, version > 0.147)
├─ 0x4e1bde..0x4e1c8d  Global0.kParamMonoToggle=0, kParamPolyCount (version > 0.147);
│                      else S2 idx 0x142 = 1.0 / 0x154 = 1.0
├─ 0x4e1cc1..0x4e22a6  FX-rack dead-slot masking (state+0x396E..0x3B4A words, version > 0.05)
├─ 0x4e22a7..0x4e295b  "+ FX" sub-engine map (fxID/paramIndex/proxyParams) + order inversion
├─ 0x4e295c..0x4e350e  ModSlot post-pass over the built nodes (kParamOut rescales,
│                      destModuleID flagging)
├─ 0x4e350f..0x4e3e58  ENVELOPE→ModSlot loop (4 envs, kParamAmount = frac·100)  (§4.3)
├─ 0x4e45b4..0x4e4b4a  ModSlot rewrite: destModuleTypeString=="VoiceFilter" &&
│                      destModuleParamID==1 → paramID 8
├─ 0x4e4b4b..0x4e581c  ModSlot kParamOut rescales ("FXFilter" p3 → v/1.06875,
│                      "FXDistortion" p5 → v/1.06875, "None" p28 → v·0.5)
└─ 0x4e581d..0x4e5f46  Global0.kParamS1Compatibility=1, kParamLimitSameNotePolyphony=1,
                       meta: fileType/vendor/url/product/schema_version(9.0)/
                       productVersion "Version 2.0.23"/serum1ChunkVersion/serum1Version
```

JSON-primitive helpers (all verified by disassembly):

| fn | role |
|---|---|
| `0x213B0` | `json_at(parent, key)` — get-or-create child object |
| `0x23450` | `operator[](json, const char*)` |
| `0x226A0` | assign value + free old (the ubiquitous "write" primitive) |
| `0x225D0` | json → int (throws "type must be number, but is …") |
| `0x79720` | json → double |
| `0x2A490` | int → json (text "0"…"9" via `0x22080` itoa) |
| `0x2A420` | double → json number |
| `0x2BA020` | std::string ← json string |
| `0x92040` | make json string from C-literal |
| `0x92170` | make json string from buffer |
| `0xBE100` | `operator[](json array, index)` |
| `0x2829C0`/`0x282890` | make **CBOR byte-string** node from (ptr,len) |
| `0x242B0` | assign any json value |
| `0x24470`/`0x245D0` | assign with default/static-template fallback |
| `0x263630` | assign f32 |
| `0x4DA030` | *(node, static-template, f64)* — template-normalized numeric write |
| `0x4D9DA0` | S2-param write with **enum round/snap** (see §3.3) |
| `0x4D93B0` | S2 param index → `"kParamXxx"` name (table 0x179B060+0x48) |
| `0x4D9A90` | S2 param index → FX type id (table 0x179B060+0x4C) |
| `0x4D8FF0` | S2 param descriptor fetch (name/type/min/max; null ⇒ param dropped) |
| `0x4D9C50` | S1 0..1 float → S2-domain converters (unit table, §3.2) |
| `0x2AC50` | `"ModSlot<N>"` key maker (prefix global 0x10EE870 = `"ModSlot"`) |
| `0x2A940` | `"FX<N>"`/`"+ FX"` sub-engine key maker |
| `0x4F10A0` | **fn_setval** — the per-parameter S1→S2 writer, `f(ctx, S2Idx, value)` where `ctx = {state @+0x00, root json @+0x08, scratch @+0x10}` (the context triple is built at 0x4DB42A–0x4DB454) |
| `0x4F1EB0` | classic→modern LFO block normalizer |
| `0x4F2A70` | LFO → `curveData{numPoints,curveVals,xVals,yVals}` writer |
| `0x4F3E00` | scalars curve writer (`curveVals/xVals/yVals/numPoints/legato`) |
| `0x4F48F0` | records → json array |
| `0x4F49F0` | midiMap proxy-param registration (`0x1036640`-based ids) |
| `0x4F4B10/BB0` | u32 range → byte-string json |
| `0x4F4C40/CD0/D60/DC0` | 8-byte binary → byte-string json (`loopback64`, `boundary64`, `storedPhasePos`) |
| `0x4FD420/4FD520/3F8240` | lfoPointModAssignments record emplace |
| `0x4F5080` | json array erase |
| `0x9B2A40`/`0x9B5DD0`/`0x9B2980` | scalar math helpers — `0x9B2A40` = **floor(x)** (f32; sign split + exp mask + `subss 1.0`), `0x9B2980` = **floor(x)** (f64 twin), `0x9B5DD0` = **round-to-nearest int** (f64; classify `0x9D0280` + mantissa-shift round `0x9B7CA0`) — all three verified by disassembly |

The tables at **0x179B060** (stride 0x50: +0x00 key storage, +0x24 min, +0x2C
marker, +0x40 sub-engine name `"WTOsc"/"NoiseOsc"/"SubOsc"/"FXComp"/…`, +0x48
param-name `"kParamXxx"`, +0x4C FX type id) and **0x179F7A0** (31 x {f64,f32}
defaults for the state+0x4AE0 block) live in `.data` **beyond the file-mapped
raw extent** (RVA > 0x1177C00, `.data` VirtSize 0xF82DE4 vs RawSize 0x99C00) —
they are runtime-relocated, so their *contents* are not statically readable;
their *addresses and use* are.

## 2. Where the S1 buffer is consumed

`s1state_load` receives the zero-filled 172 736-byte buffer (copied from
file+0x3C, length N ≤ 172 736, tail zeroed) in `rdi` (kept at `[rbp+0x26D60]`).
Every read in the function is one of:

| S1 region | blob offset | consumer |
|---|---|---|
| params 0..227 | `+0x3460` (f32 x228) | master loop 0x4DEBA0 (`state + i*4 + 0x3460`) |
| FX mirror head (master params 228..247) | `+0x37F0..0x383F` | same loop (i = 228..247) |
| FX-enable byte[247] | `+0x3840` | midiMap builder 0x4DFDA7 |
| FX knob mirror / reverb type byte | `+0x3B04`, `+0x3976+68i`, `+0x3978+68i`, `+0x4BF0`, `+0x4BD4+4i` | 0x4E02C5, 0x4E00DB, 0x4E0118, 0x4E0275, 0x4E0285 |
| FX rack order int32[10] | `+0x3BE0` | inversion 0x4DAD3B–0x4DADE4, lookups 0x4DFAA9, 0x4E021F, fn_setval 0x4F1188 |
| WT/noise/filter names | `+0x3C08 / +0x3E08 / +0x4008` | 0x4E0436–0x4E07E4 |
| preset name/author/category | `+0x4972 / +0x49A0 / +0x49D0` | 0x4E02CE–0x4E0417 |
| version f32 | `+0x4994` | 0x4DB45B |
| osc table-pos f32s | `+0x4998 / +0x499C` | 0x4E189A / 0x4E192A |
| interpolate-after-load byte | `+0x4970` | 0x4E112E |
| per-osc WT frame counts int32[2] | `+0x4968 / +0x496C` | 0x4E0DFF, 0x4E134A |
| defaults scratch (re-initialized) | `+0x4AE0` | defaults table 0x4DBC20–0x4DBC82 |
| oversampling/tuning lock bits | `+0x4A50` | 0x4E0AC6–0x4E0B4F |
| macro names 4x32 | `+0x4A60` | 0x4E08F0 |
| global switches (polyphony, mono, filter) | `+0x4C48..0x4CFC` | migrations 0x4DCE08–0x4DCFA6 |
| per-mod-slot word codes (S1 layout) | `+0x18+40k … +0x22+40k` (slots 1–16), `+0x50E0..0x5354` (slots 17–32) | 0x4DEEB4–0x4DEFBE, 0x4DBC89–0x4DC57D |
| LFO graph blocks x8 | `+0x84E0` (0x2D28 each) | fn_4f1eb0 / fn_4f2a70 0x4DE2D0–0x4DE6C6 |
| classic-layout LFO 1–8 | `+0x280 / +0x1B70 / +0x5558` | 0x4DE2EB–0x4DE375 |
| LFO point-mod array | `+0x6E48 (+0x2C·i)`, count `+0x8448` | 0x4DED25–0x4DEE23 |
| embedded WT/noise counters + tuning | `+0x4968..0x4970, +0x53E0..0x53E4, +0x5540..0x5544` | §7 |
| appended stream (wavetable) | file region after state, W = V−N−4 | 0x4E0BE8–0x4E0D5E |

## 3. Master parameter conversion (`fn_setval` 0x4F10A0)

### 3.1 The loop (0x4DEB6E–0x4DECBE)

```asm
0x4DEB6E  xor  esi, esi                     ; S1 master param index i
0x4DEB70  movss xmm10, [0xA55938]           ; 0.008   (old-version marker gate)
0x4DEB80  movss xmm13, [0x9EBCC0]           ; 1.0
0x4DEB89  movss xmm6,  [0xA55A70]           ; 0.1099
loop (esi < 0xF8 = 248):
0x4DEBA0    movss xmm0, [state + i*4 + 0x3460]
0x4DEBB3    cvtss2sd xmm2, xmm0             ; f32 → f64
0x4DEBB7    mov rcx, rdi                    ; ctx {state, root, scratch}
0x4DEBBA    mov edx, esi
0x4DEBBC    call 0x4F10A0                   ; fn_setval(ctx, idx, value)
; old-preset index shift (both gates write idx+4):
0x4DEBFC/0x4DEC43  lea edx, [rsi+4]         ; version < 0.008 & i ≥ 178, or
                                            ; version < 0.009 & i ≥ 180
; clamps before the call:
0x4DEC50    if (value < 0)      state[i] = 0.0f        ; 0x4DEC55
0x4DEC76    if (value > 1.0f)   state[i] = 1.0f        ; 0x3F800000
0x4DEC91    if (value is NaN)   state[i] = 0.0f        ; ucomiss xmm0,xmm0 / jp
0x4DEC96    if (version ≥ 0.1099 && i == 227)  call fn_setval(ctx, 0xE3, 0.5)
          ; (0x4DECAE: idx 0xE3 = 227, xmm2 = 0.5 @0x9E6C10 — forced overwrite)
```
So: **S1 master params 0…247 (blob 0x3460, incl. the FX-mirror head as 228…247)
→ S2 plain parameter `idx` (or `idx+4` for pre-0.009 chunks)**, value first
clamped to [0,1], NaN→0.

### 3.2 `fn_setval` (0x4F10A0) — per-parameter writer

```asm
0x4F10D6  test edx, edx ; js ret            ; idx < 0 → drop
0x4F10DF  lea eax, [rsi-0x13B] ; cmp eax,0x1B ; ja ok
0x4F10EA  mov ecx, 0x8400003 ; bt ecx, eax ; jae ok
          ; idx ∈ {0x13B, 0x13C, 0x155} (315,316,341) → silently dropped
0x4F1116  call 0x4D9C50 (idx, value)        ; domain convert → xmm6
0x4F1121  cmp esi, 0x14A ; je ret           ; idx 330 → dropped
0x4F112D  call 0x4D8FF0 (idx)               ; S2 param descriptor
0x4F113B  cmp qword [rbp+0x180], 0 ; je ret ; no descriptor ⇒ param has NO S2
                                            ; counterpart ⇒ dropped
0x4F1149  call 0x4D9A90 (idx) → ebx         ; FX type id this param belongs to
; ---- FX-knob branch (ebx ≥ 0): ----
0x4F1186  r14d = dword [state + ebx*4 + 0x3BE0]   ; rack cell of that FX type
0x4F11B3  json_at(root, "+ FX") → BE100(fxArray, r14d) → json_at(cell, <descriptor
          sub-engine string from table 0x179B060+ebx*0x50+0x40>) → plainParams
0x4F11F9  (ebx==6 && idx==0x53) → kParamType=="kPlate" ? then
              kParamPreDelay = fn_4d9da0(…,0x53,v) then x0.001
0x4F13C0  (ebx==0x63 && idx==0x63) → kParamType=="kDiode1" ? then
              kParamDrive = v·0.875 + 12.5
0x4F1520  (ebx==5 && idx==0x87) → hard-coded writes:
              kParamGain0 = 4.65, kParamGain2 = 4.65,
              kParamRatioBelow = 0.75, kParamCompensatedWetDry = 0.0
0x4F1648  amount = (ebx==8 && idx==0x8F) ? v : v        ; 0x64 → plain
0x4F1633  amount = (r13 flag) ? v/1.06875 : v           ; diode-drive path
0x4F1673  fn_4d9da0(node, idx, amount); name = fn_4d93b0(idx) → assign
0x4F169C  (ebx==7 && idx==0x5C) → FXPhaser cell:
              fn_4da030(&tmp, 0xA19FC0 template, xmm7 = v/1.06875)
              → cell.plainParams["kParamDepth2"] = result
; ---- non-FX branch (0x4F12BD): ----
0x4F12D8  descriptor string at 0x179B060+type·0x50+0x40 ; strcmp against
          "WTOsc" / "NoiseOsc" / "SubOsc" (globals 0x10EE948/0x10EE9D8/0x10EEA10)
0x4F12FF  match → json_at(root, "OscillatorN") … actually:
          json_at(root, typeStr) — but typeStr for non-osc comes from
          the same table; osc params are handled by the osc writer below
0x4F1316  idx ∈ [0x13D,0x153] → jump table 0xA56470 (23 entries)
0x4F1325  idx ∈ [0x28,0x2C] → global section:
0x4F1361      node = json_at(root, "plainParams")[fn_4d93b0(idx)]
0x4F139D      xmm6 = min(xmm6, 0.3333333333333333)      ; clamp ⅓
0x4F17CF      fn_4d9da0(node, idx, xmm6) → assign
0x4F1837  (idx==0x141 or 0x14A-family, jtable 21/22):
0x4F185D      fn_4da030(&tmp, 0x9F0720 template, 1.0)
0x4F1862      json_at(root,"LFO8").plainParams["kParamType"] ← value   ; =1.0
0x4F18AC      same for "LFO9"
0x4F17BD  default: plain numeric write through fn_4d9da0
```

**Formula summary (all verified from the instructions):**

| S1 → S2 | transform | site |
|---|---|---|
| generic scalar | `S2 = S1` (after clamp 0…1, NaN→0) | 0x4F1129–0x4F1129 |
| reverb predelay (FX6 / idx 0x53) | `kParamType="kPlate"`; `kParamPreDelay = round⁻¹(v)·0.001` | 0x4F1206–0x4F12A3 |
| dist drive (FX0x63 / idx 0x63) | `kParamType="kDiode1"`; `kParamDrive = v·0.875 + 12.5` | 0x4F13DB–0x4F146D |
| comp (FX5 / idx 0x87) | `kParamGain0 = kParamGain2 = 4.65; kParamRatioBelow = 0.75; kParamCompensatedWetDry = 0.0` | 0x4F151F–0x4F1627 |
| phaser depth (FX7 / idx 0x5C) | `kParamDepth2 = v / 1.06875` | 0x4F16A7–0x4F175A |
| FX8 / idx 0x8F | amount written raw (v), others `v/1.06875` when the diode flag is set | 0x4F162F–0x4F1666 |
| LFO8/LFO9 kParamType | constant 1.0 via template 0x9F0720 | 0x4F1837–0x4F18FC |
| anything else | `S2[name(idx)] = fn_4d9da0(0..1 → S2 unit)` | 0x4F1673 |

### 3.3 The two converters

**`fn_4d9c50` (0x4D9C50) — S1 0…1 → S2 native unit.** Jump tables
0xA563F8 (S1 idx 3…0xB3, 22 entries) and +0x140… (idx 0x140…0x155): each entry
either passes the value through (`xorpd xmm1` → x1.0) or multiplies by a unit
constant — resolved constants: **x8, x4, x2, x48, x15, x24, x95, x23, x22, x5,
x31** — then, for the multiplied classes, the common tail at 0x4D9D61–0x4D9D94
does `v = floor(v·N + 0.5) / N` (verified: `mulsd v, xmm2(N)`,
`addsd 0.5` @0x9E6C10, `call 0x9B2980` = **floor(x)** f64, `divsd N`) — the
classic `round(v·N)/N` enum-fraction snapping. One class multiplies by **0.01**
(`0x9EBD40`) then applies `v = (v > 0.01) ? v : 1.0`
(`0x4D9D24`: `movsd xmm1, 0.01; cmpltsd xmm1, v; movsd 1.0; andpd` — keeps v
when v > 0.01, else 1.0). Unlisted idx ⇒ identity.

**`fn_4d9da0` (0x4D9DA0) — S2 param write with enum rounding.** Loads the S2
descriptor (0x179B060 + idx·0x50: +0x24 = dst int32 id, +0x08 min f64, +0x10 max
f64, +0x18 step/count f64, +0x20 flags, +0x28 option-list ptr), clamps the
value into [min,max] (`0x4D9E74–0x4D9EB1`), then per `edx` (write-mode field of
the descriptor, ≤3 else linear): jump table 0xA56460 —
`0`: nearest int with fractional dither `v·(n+1)` → `cvttsd2si` + `cmovl`
(`0x4D9EC3–0x4D9F10`); `1`: normalize by [min,max] then `0x9B5DD0`
(**round-to-nearest int** — verified by disassembly) and re-scale
(`0x4D9F23–0x4D9F3C`); `2/3`: linear `min + v·(max−min)`
(`0x4D9F48–0x4D9FA5`, and 0x4DA26D in fn_setparam);
options-array path snaps to a discrete entry (`0x4D9F1F–0x4D9F46`).

**`fn_4da030` (0x4DA030)** — same shape but value comes from a *static JSON
template* (0x9F0520 mode, 0x9F0560 beat-sync, 0x9F05A0 anchored, 0x9F05E0
dotted, 0x9F0620 triplets, 0x9F0720 lfophasor-type, 0x9F0820 rate10x) +
f64 arg; used for the LFO8/LFO9 defaults, the FXPhaser kParamDepth2 template,
and the phase-memory template 0xA223C0.

## 4. Modulation matrix (S1 slots 1…32 → S2 ModSlotN)

`0x2AC50` builds the `"ModSlot<N>"` key. Four loops touch the ModSlot nodes, in
this order:

### 4.1 Staging loop (0x4DEE80–0x4DF16D) — S1 slot record → S2 mod amounts

For each of 32 S1 slot records (state + 40·slot; fields `+0x18`/`+0x1A` = src /
dest word codes, `+4`/`+8` = two amount f32s):

```asm
0x4DEEC5  if (version < 0.0058) re-stamp marker: byte[slotrec+0x22]=slot,
0x4DEEDC      word[slotrec+0x20]=0x8080          ; 0xA55A64 = 0.0058
0x4DEEEA  if (dest == 0xDE) dest = 0xDF          ; code migration
0x4DEF08  fn_setval(ctx, 0xDE, 1.0)              ; unconditional default amount
0x4DEF0D  if (version == 0.008) { dest ≥ 0xB4 → dest += 4 ; srcA ≥ 0x3A → srcA += 3 }
0x4DEF38  if (version ≤ 0.009) { dest ≥ 0xB4 → dest += 2 ; srcA ≥ 0x41 → srcA += 1 }
0x4DEF68  if (version < 0.008) { dest ≥ 0xB2 → dest += 1 } ; xmm10 = 0.008, jbe
0x4DEF7F  if (version < 0.007) { srcA ≥ 0x2F → srcA −= 9 } ; xmm0 = 0.007 @0xA1E80C
0x4DEF9E  if (version > 0.0299) { dest ≥ 0xDF → dest += 4 } ; xmm0 = 0.0299 @0xA55A68
0x4DEFBE  a1 = (f32[slotrec+4] + 0.5) · 0.5      ; addss xmm13(0.5), mulss 0.5
0x4DEFD7  fn_setval(ctx, 2·slot + 0xB4, a1)
0x4DEFE6  a2 = f32[slotrec+8]
0x4DEFFE  fn_setval(ctx, 2·slot + 0xB5, a2)
```
So **S1 slot k → S2 plain params `0xB4 + 2k` / `0xB5 + 2k`** (amount A scaled
x0.5, amount B raw), plus the dest/src-code migrations above.

### 4.2 ModSlot node builder (0x4DF170–0x4DFCE7)

For the same 32 slots (`fn_2AC50(slot)` → key, `json_at(root, key)`):
```asm
0x4DF273  b  = byte  [slotrec + 0x20]             ; S1 curve byte A
0x4DF27E  v  = (b < 128) ? b·k : ((b−256)·k…)     ; fn-scale (xmm9/xmm12/xmm6/xmm11/xmm15)
0x4DF2C8  plainParams.kParamCurveIn  = v
0x4DF30B  b  = byte  [slotrec + 0x21] → same math → plainParams.kParamAuxCurve
0x4DF39B  sx = signed byte [slotrec + 0x0C]
0x4DF3B1  fn_4da030(&tmp, template 0xA20B70, sx) → plainParams.kParamBipolar
0x4DF3F9  w = word [slotrec+0x1C]; ((w==1) || (w==2 && word[+0x1E]==1)) ?
0x4DF41E      fn_4da030(0xA20BB0, 1.0) → plainParams.kParamAuxInverted
0x4DF472  w == 2 ? fn_4da030(0xA20BF0, 1.0) → plainParams.kParamBypass
0x4DF4F7  t = word [slotrec + 0x16] (≤0x21 else table skip)
0x4DF501  aux = dword [0xA55300 + t·4]           ; identity-remap table (0,1,2,3,4,6,7,8…)
0x4DF549  node["source"] = pair built by fn_2829C0 from (marker-2 string, aux-int)
0x4DF654  (aux == 0 && plainParams.kParamAuxInverted != 0) ?
0x4DF6A8      fn_4da030(0xA20BF0, 1.0) → plainParams.kParamBypass = 1
0x4DF6F0  d = word [slotrec + 0x1A]                ; S1 dest code
0x4DF6FD  d == 0x13C ? destModuleParamID = -1
0x4DF768  : destModuleParamID = dword [0x179B060 + d·0x50 + 0x4C]
0x4DF95B  destModuleTypeString = cstr [0x179B060 + d·0x50 + 0x40]
0x4DFA8A  t = fn_4d9a90(d)                          ; S2 type id for dest code
0x4DFAA9  t ≥ 0 ? destModuleID = int32 [state + t*4 + 0x3BE0]   ; FX rack cell
0x4DFAFC        : destModuleID = dword [0x179B060 + t·0x50 + 0x48]
```
**`destModuleTypeString`** = the S2 engine-family name for the S1 dest code
(static table +0x40), **`destModuleParamID`** = the S2 param id of that code
(table +0x4C), **`destModuleID`** = the FX-rack cell for FX-family dests
(order table at blob+0x3BE0) or the static id (table +0x48) otherwise.

### 4.3 Envelope loop (0x4E350F–0x4E3E58) — S1 env amounts → ModSlot kParamAmount

Per env `esi = 0…3`, with per-env tables
`S1param[4] = int32@0xA553A0 = (3, 4, 16, 17)` and `scale[4] = f32@0xA55390 =
(8, 24, 8, 24)`:

```asm
0x4E3567  r14 = table[esi]                       ; S1 master param idx
0x4E3572  a  = scale[esi]                        ; 8 or 24
0x4E3578  x  = f32[state + r14*4 + 0x3460]       ; S1 env param value
0x4E3582  x  = x·a + 0.5                         ; mulss xmm9, addss 0.5(0x9EBD84)
0x4E359D  f  = fn_9b2a40(x)                      ; floor helper (disassembled:
          ;   9b2a88: |x|<1.0 → return −1.0 / 0.0 by sign; 9b2aa8: exp-based
          ;   mantissa mask + `subss 1.0` → floor(x) for |x| ≥ 1)
0x4E35A2  xmm8 = {x, f} ; xmm11 = {a+1.0, a}
0x4E35AA  xmm8 /= xmm11                          ; {x/(a+1), f/a}
0x4E35B7  xmm8.x = x/(a+1) − f/a                 ; fractional residue
0x4E35BC  if (residue == 0) next env
0x4E35C8  if (flag[slot] != 1) next env          ; gate set in 4.4's flag pass
0x4E3604..0x4E3742  find ModSlot whose destModuleParamID == -1  (r12d = last match)
0x4E3780  if (none) next env
0x4E3796  srcval = 0x26 (38) → byte-string node
0x4E37B0  destModuleParamID  = int  [0x179B060 + r14·0x50 + 0x4C]
0x4E38DE  destModuleTypeString = cstr[0x179B060 + r14·0x50 + 0x40]
0x4E3A03  destModuleID       = int  [0x179B060 + r14·0x50 + 0x48]
0x4E3BFD  source             = pair(byte-string 38, byte-string slot-id)
0x4E3D00  plainParams.kParamAmount = residue · 100.0      ; mulsd xmm10(100.0)
```

### 4.4 Post-passes over the built ModSlot nodes

**Post-pass 1 (0x4E29AB–0x4E350E)** — for each ModSlot node:
```asm
0x4E2C12  s = destModuleTypeString; strcmp(s, global 0x10EDB08 = "Oscillator")
0x4E2DA7  p = destModuleParamID (fn_225D0)
0x4E2DAC  p == 3 ?  kParamOut = old·8.0/9.0      ; xmm7=8.0, xmm8=9.0
0x4E3090  d = destModuleID (fn_225D0); d ≥ 1 ? flag[slot*2+0x26C44] = 1
0x4E31A3  p == 4 ?  kParamOut = old·24.0/25.0    ; xmm9=24.0, xmm10=25.0
```
**Post-pass 2 (0x4E45B4–0x4E4B4A)** — `destModuleTypeString == "VoiceFilter"`
(0x10ED700) && `destModuleParamID == 1` → `destModuleParamID = 8`
(0x4E4A35/0x4E4A50, int json type 5).

**Post-pass 3 (0x4E4B4B–0x4E581C)** — rescales `plainParams.kParamOut`:
`"FXFilter"` (0x10ED6F8) with paramID 3 → `v/1.06875` (0x4E52AC, xmm7 =
1.06875 @0x9EBB60); `"FXDistortion"` (0x10ED6B0) with paramID 5 → `v/1.06875`
(0x4E514C gate); `"None"` (0x10ED708) with paramID 28 → `v·0.5` (0x4E5781,
xmm8 = 0.5 @0x9E6C10).

**LFO-point mod bus** (`lfoPointModAssignments`, 0x4DED25–0x4DEE23): records at
`state + 0x6E48 + i·0x2C` (count at `state+0x8448`) are appended via
fn_4FD520/fn_3F8240 into an array assigned to
`root["lfoPointModAssignments"]` (0x4DEE06). Fields copied: `+0x6E48` u32,
`+0x6E4C` u32, `+0x6E50` u32 (clamped to ≤3, 0x4DED95), `+0x6E54` u32 **− 147**
(0x4DEDBB, `add edx, 0xFFFFFF6D`).

## 5. FX rack

The S1 order table `int32[10] at blob+0x3BE0` is inverted at function entry
(0x4DAD3B–0x4DADE4) into `[rbp+0x26C60]`: `inv[type] = rack_cell`.
Rack cells are addressed as `"+ FX"` array nodes (`fn_2A940` builds the cell
key, e.g. `"+ FX0"`…`"+ FX9"`); each cell's sub-engine map is selected by the
S2 type id (`0x179B060 + type·0x50 + 0x40` → `"FXComp"`, `"FXFlanger"`,
`"FXReverb"`… string set listed in `strings_named.json`).

Knob writes happen exclusively through `fn_setval` (§3.2) — every FX knob of
the master param block that has an S2 descriptor is written into
`"+ FX"<cell>.<subEngine>.plainParams["kParamXxx"]` where the cell index is
`state+0x3BE0[fxType]` and `kParamXxx` comes from `fn_4d93b0(idx)`
(table +0x48). The special-cased rows are the ones in §3.2's formula table.

**S1 FX id → S2 type id:** `0x4D9A90` maps an S1 master param index to the S2
FX `type` uint via table `0x179B060+0x4C` (int32 per 0x50-stride entry). The
same table's +0x40 field names the sub-engine and +0x48 the kParam name, so the
triple (type-id, submap string, param name) always comes from one descriptor.
The per-FX enable/mix knobs are **not** in the master loop: they are the
`mixOrGain1…10` block (0x4E008F–0x4E02AD), sourced from
`state+0x3976+68i` (enable byte) / `state+0x3978+68i` (mix byte, cast int→json)
/ `state+0x4BD4+4i` (gain f32 → S2 idx `esi+0x121`), gated by version
(0.05 / 0.159 / 0.146 thresholds at 0x4E0091/0x4E009A/0x4E02AF).

Dead rack cells are masked before serialization (0x4E1CD6–0x4E1DB2):
`word state+0x3B4A / 0x3B06 / 0x3AC2 / 0x3A7E / 0x3A3A / 0x39F6 / 0x39B2 /
0x396E` → per-cell flags array `[rbp+0x26CD0]` (8 cells + 2 extra bytes at
0x3B8E/0x3BD2), then the `"+ FX"` array is compacted (`fn_4F5080` erase).

**fxID/paramIndex/proxyParams block (0x4E2520–0x4E2926):** for every FX
sub-engine the importer emits `{fxID: <int>, paramIndex: <int>, proxyParams:
[<int>…]}` triples — this is how S2 keeps a *stable numeric identity* for each
imported FX knob (the `destModuleParamID` values of the mod matrix refer to
these). The per-FX record is validated at 0x4E2340: if the "+ FX" cell is not a
JSON object the function throws `"Unsupported file type"` (0x4E23C3) — the
only exception path in the converter.

## 6. LFO / curves

* **Normalization** `fn_4f1eb0` (0x4F1EB0): copies a 0x2D28-byte classic block
  (`state+0x280` for LFO 1–4, `state+0x1B70` / `+0x5558` for 9/10 or
  `state+0x84E0 + i·0x2D28` for 1–8, selected by `version` gates 0x4DE2D6,
  0x4DE6F4) into the modern layout: the three 0xF00-byte f64 arrays are copied
  to the new block (+0x0000/+0x0F00/+0x1E00; verified 0x4F2440/0x4F24E0/0x4F2550
  and the SSE bulk copies 0x4F25A2/0x4F27A4), the tail fields are
  `numPoints ← sbyte[block+0x1860]` → `dword` (0x4F2207),
  `+0x1874→+0x2D00, +0x1878→+0x2D01, +0x1888→+0x2D05` (mode/sync bytes),
  `f32 +0x1864→+0x2D14 (rate), +0x18A0→+0x2D0C (phase), +0x18C0→+0x2D18 (rise),
  +0x18D0→+0x2D1C (smooth), +0x18E0→+0x2D20 (delay)`,
  `+0x187C→+0x2D02, +0x1880→+0x2D03, +0x1884→+0x2D04, +0x188C→+0x2D06`
  (anchored/dotted/triplets/beat-sync flags) — verified at 0x4F2992–0x4F2A5E.
* **curveData writer** `fn_4f2a70` (0x4F2A70, args: node, 0x2D28-block, mode,
  flag): first creates `curveData` (0x4F2AB7) and sets
  `numPoints = min(u32[block+0x2D08], 0x1E1)` (0x4F2AC9–0x4F2AE7), then copies
  three 0xF00-byte f64 arrays from the block — `curveVals` ← block+0x0000
  (0x4F2BB7, 480 f64), `xVals` ← block+0x0F00 (0x4F2CC4, 480 f64), `yVals` ←
  block+0x1E00 (0x4F2E03, 480 f64) — as CBOR arrays (marker 2), then writes
  `plainParams` fields sourced from the block's tail: `kParamRate ← f32
  block+0x2D14 (idx 0x3D, 0x4F2EB4), kParamSmooth ← +0x2D18 (idx 0xDF, 0x4F2F23),
  kParamRise ← +0x2D20 (idx 0x111, 0x4F2F95), kParamDelay ← +0x2D1C (idx 0x119,
  0x4F3007), kParamMode (template 0x9F0520), kParamBeatSync (template 0x9F0560),
  kParamAnchored (template 0x9F05A0), kParamDotted (template 0x9F05E0),
  kParamTriplets (template 0x9F0620), kParamPhase (from block+0x2D0C, template
  0x9F04E0), loopbackPointNum ← int32 block+0x2D10 (0x4F3384)` — keys verified
  at 0x4F2EE0 (kParamRate) / 0x4F2F52 (kParamSmooth) / 0x4F2FC4 (kParamRise) /
  0x4F3036 (kParamDelay) / 0x4F30C4 (kParamMode) / 0x4F313C (kParamBeatSync) /
  0x4F31CE (kParamAnchored) / 0x4F3241 (kParamDotted) / 0x4F32B4 (kParamTriplets) /
  0x4F3336 (kParamPhase) /
  0x4F3399 (loopbackPointNum). Curves are
  therefore **pass-through float64 copies** of the S1 tension/x/y arrays (no
  resampling, no re-anchoring in this path). All constants resolved in
  `consts.txt`.
* **LFO 8/9 (phasor)**: `LFO8`/`LFO9` get exactly four plainParams —
  `kParamType, kParamDotted, kParamTriplets, kParamRate10x` — via fn_4da030 with
  templates 0x9F0720/0x9F05E0/0x9F0620/0x9F0820 and constants 0.5/1.0/1.0/0.75
  (0x4DB032–0x4DB3F6; fn_4da030/4da026 calls at 0x4DB03A/0x4DB0BA/0x4DB13A/
  0x4DB1BA and the LFO9 block 0x4DB23D–0x4DB3C0), gated by version > 0.155
  (0x4DEE33; xmm8 = 0.155 @0xA55994).
* **scalars** (`velo`, `note`): two f32 blocks at `state+0x4220`
  (`r8 = state+0x4220`, selected by `r9 = 0/1`, stride 0x200 bytes) are
  converted by `fn_4f3e00` (0x4F3E00) into
  `root["scalars"]["velo"/"note"]` = `{curveVals, xVals, yVals, numPoints,
  legato}` — `curveVals` is a 128-f64 array converted from the block head
  (cvtps2pd loop 0x4F3E36–0x4F41F2), `xVals`/`yVals` are 0x88-byte sub-array
  copies from block+0x510 and block+0x620 respectively (r12 = 136·scalarIdx;
  copies at 0x4F4329 and 0x4F43C0; the +0x400 copy at 0x4F429E feeds a third
  sub-array), `numPoints =
  sbyte[block+0x730]` (0x4F445E), `legato = bool(f32[block+0x738] ≠ const)`
  (0x4F44C6) — verified keys `curveVals`, `xVals`, `yVals`, `numPoints`,
  `legato` at 0x4F42E0/0x4F4377/0x4F440E/0x4F4475/0x4F44F1.

## 7. Wavetable / embedded streams — verdict

**Reference, not disk-write.** In the analyzed path the importer:

1. reads the S1 name fields raw and assigns them as JSON strings:
   * `state+0x3C08` → `…["relativePathToWT"]` (0x4E0436–0x4E0520)
   * `state+0x3E08` → second `relativePathToWT` (0x4E0525–0x4E05CF)
   * `state+0x4008` → `pathToNoiseSample` (0x4E17EC–0x4E1868) and, when the
     controller string equals it, `relativePathToNoiseSample`
     (0x4E0706–0x4E081A);
   * `"Audio In"` in 0x3C08/0x3E08/0x4008 sets `sampleFromAudioInput = true`
     (0x4E0436, 0x4E05E4, 0x4E0C2F) — no file I/O;
2. copies the appended zlib-decompressed streams into the JSON as **CBOR
   byte strings**, keyed `"embeddedWTData"` (0x4E1095), `"embeddedNoiseData"`
   (0x4E1629/0x4E16D7/0x4E178F) — built by fn_282890 (byte-string ctor) from
   slices of the appended-data buffer (`[rbp+0x26CD0]`, offset tracked in
   `r15`, length = `frameCount·4·…` div at 0x4E165D–0x4E168C);
   per-osc frame counts come from `state+0x4968/+0x496C`, noise size from
   `state+0x5544`, filter-table size from `state+0x5540`;
3. copies the raw 8-byte little-endian values at `state+0x5528 / +0x5530 /
   +0x5538` into byte-string fields `"storedPhasePos" / "loopback64" /
   "boundary64"` via fn_4F4D60;
4. writes `"interpolateAfterLoad": bool(state+0x4970)` (0x4E12F5),
   `"tuningData": <bytes 0x53E0…>` + `"tuningName": cstr(state+0x53E4)`
   (0x4E1415–0x4E1487).

**Verdict: the S1 wavetable handling is pass-through reference + embedded CBOR
byte strings; there is no `.wav`/`.SerumTable` file-name construction and no
disk write anywhere in 0x4DABC0–0x4E61CA.** (The only disk-flavored strings in
the binary — `could not write temporary wavetable` 0xF94678, `Save WaveTable
as...` 0xF9EC60 — belong to the editor/save paths, not to this importer.)
Migration-only shuffling of the embedded blob happens at 0x4E0882–0x4E08C7
(0x24EAC stack buffer, +0x124 shift into `state+0x5418`, 0x18F8 copy into
`state+0x5550`), gated on `state+0x5548 ∈ (1.104, 1.106)` — again purely
in-memory.

## 8. Globals / meta / scalars

| S1 field | S2 target | transform | site |
|---|---|---|---|
| `state+0x4994` f32 | `serum1ChunkVersion` | f32→f64 | 0x4E5BD3 |
| `state+0x4994` | `serum1Version` | table below | 0x4E5C42–0x4E5EEF |
| state+0x4A50 bit1 / bit2 | `lockOversampling` / `lockTuning` | bit → bool | 0x4E0AC6/0x4E0B17 |
| state+0x4C94 f32 | `VoiceFilter0.kParamWet` | constant 100.0 then `kParamLevelOut = √v·0.05` | 0x4E4476–0x4E454E |
| macro names 0x4A60[4x32] | `"name1..4"` (cstr(state+0x4A60+32i), keys `"name"` + int i via fn_22080) — written verbatim, version > 0.134 | none | 0x4E08CC–0x4E0AB2 |
| `state+0x4972/0x49A0/0x49D0` | `presetName/presetAuthor/presetDescription` | verbatim | 0x4E02CE–0x4E0417 |
| — | `Global0.kParamMonoToggle = 0`, `kParamPolyCount = 1.0` | constants | 0x4E1BF1–0x4E1C6C |
| — | `Global0.kParamS1Compatibility = 1`, `kParamLimitSameNotePolyphony = 1` | constants | 0x4E583A/0x4E58BE |
| — | `fileType` (static json 0xA553B0), `vendor "Xfer Records"`, `url`, `product "Serum2"/"Serum2FX"`, `schema_version 9.0`, `productVersion "Version 2.0.23"` | constants | 0x4E5936–0x4E5BB1 |
| f32 curve blocks @ `state+0x4220` | `scalars.velo` / `scalars.note` | fn_4f3e00 | 0x4DEA8C–0x4DEB1E |
| `state+0x5550` f32 | `detuneFactor` (else `oldSerum1Preset=true` when version ≤ 0.149) | none | 0x4DD853–0x4DD954 |

**`serum1Version` mapping chain (all values read from .rdata, verified at
0x4E5C42–0x4E5E90):**
```
v > 0.150  → f32[state+0x5548]        (read from the state itself)
v ≤ 0.100  → 1.005 (v>0.10999→skip: fallthrough)   — chain:
v ≤ 0.109999 → 1.005    (0x4E5C86, f32@0xA55A00)
v < 0.131  → 1.010      (0x4E5CA1, f32@0xA55A04)
v < 0.133  → 1.023      (0x4E5CBC, f32@0xA55A0C)
v < 0.134  → 1.026      (0x4E5CD7, f32@0xA55A14)
v < 0.135  → 1.032      (0x4E5CF2, f32@0xA55A18)
v < 0.136  → 1.036      (0x4E5D48, f32@0xA55A1C)
v < 0.137  → 1.035      (0x4E5D63, f32@0xA55A24)
v < 0.138  → 1.044      (0x4E5D7E, f32@0xA55A2C)
v < 0.139  → 1.051      (0x4E5D99, f32@0xA55A30)
v < 0.141  → 1.068      (0x4E5DB4, f32@0xA55A34)
v < 0.142  → 1.071      (0x4E5DD4, f32@0xA55A3C)
v < 0.143  → 1.072      (0x4E5DF4, f32@0xA55A40)
v < 0.144  → 1.082      (0x4E5E14, f32@0xA55A48)
v < 0.146  → 1.092      (0x4E5E25, f32@0xA55A4C)
v < 0.147  → 1.095      (0x4E5E59, f32@0xA55A50)
v ≥ 0.148  → f32[0xA55A58] = 1.105   /   v < 0.148 → f32[0xA55A5C] = 1.103
```
(the chain's exact layout: each entry is a `movss xmm13, [const]` +
`ucomiss xmm1, v` pair; the final `seta al` selects 0xA55A58 vs 0xA55A5C.)

## 9. ModSlot ↔ S1 slot correspondence (mod matrix)

S1 slot record = 40 bytes (slots 1–16 at blob+0x0000, 17–32 mirrored at
state+0x50E0+0x2C·(n−17), self-identified by the `80 <slot> FF` marker at +0x21
— re-stamped when version < 0.0058, 0x4DEEC5). Word fields: `+0x18` src A,
`+0x1A` dest, amounts at `+4 / +8` f32. Full conversion in **§4.1–§4.4**
(staging → node builder → envelope loop → three post-passes). Net effect:
**S1 mod slots 1…32 → S2 plain params `0xB4 + 2k` / `0xB5 + 2k` (amount A
scaled x0.5, amount B raw) + `ModSlotN` nodes (curve/bypass/aux fields,
dest/source) + `LFOPointModBus` records; dest codes are translated through the
static descriptor table rather than kept raw.**

## 10. Version-gated migrations (summary; all sites verified)

Thresholds (f32/f64, compared against `state+0x4994`; addresses from
`consts.txt`, which resolves each rip-target with the correct PE section math):
```
0.002 0x4DB464 · 0.009 0x4DB476(0xA55934)/0x4DEC10(0xA55934)/0x4DEF38(0xA55934)
0.008 0x4DEB70(0xA55938) · 0.01 0x4DB525(0x9EBE54) · 0.0599 0x4DB5E1(0xA55944)
0.0699 0x4DB59E(0xA5593C) · 0.078 0x4DB631/0x4DB652(0xA55910)
0.089 0x4DB63F/0x4DB6B3(0xA55970) · 0.109 0x4DB6F3(0xA55978)
0.1099 0x4DEB89(0xA55A70) · 0.12999 0x4DB956 (mod-slot dest + FX-mirror migration)
0.148 0x4DC094/0x4DC618(0xA55918)/0x4DD3EF/0x4DF103/0x4E081F/0x4E0CAC
0.151 0x4DC605(0xA5591C) · 0.159 0x4DCB97(0xA55920) · 0.16 0x4DCC28/0x4DCDB8
0.158 0x4DCCAF/0x4DCDCA(0xA55928) · 0.163 0x4DCCC3/0x4DCE85(0xA55930)
0.13199 0x4DCD8E/0x4DCE97(0xA5597C) · 0.13299 0x4DCEDA(0xA55980)
0.135 0x4DCEFA/0x4DCF38(0xA55984) · 0.138 0x4DCF08/0x4DCF57(0xA55988)
0.139 0x4DCF28/0x4DCF65(0xA5598C) · 0.154 0x4DCFD9/0x4DD061(0xA55990)
0.155 0x4DD0D6/0x4DD1FD/0x4DEE33(0xA55994) · 0.142 0x4DD1DE(0xA55998; x88/89
delay-time migration) · 0.156 0x4DD26E(0xA559A4) · 0.157 0x4DD295(0xA559A8)
0.149 0x4DD853 (oldSerum1Preset gate) · 0.161 0x4DD959(0xA559BC)
0.162 0x4DE28C(0xA55A74) · 0.0058 0x4DEEC5(0xA55A64) · 0.007 0x4DEF7F(0xA1E80C)
0.0299 0x4DEF9E(0xA55A68) · 0.137 0x4DF0A5(0xA55A28) · 0.14 0x4DF0D3(0xA55A6C)
0.1299 0x4DFE2C(0xA559E0; midiMap gate) · 0.05 0x4E0091/0x4E1CC1(0x9FCB70)
0.159 0x4E009A(0xA55A60) · 0.146 0x4E02AF(0xA559E4) · 0.150 0x4E0845(0xA1BFD8)
1.104/1.106 0x4E086C/0x4E0879(0xA559E8/EC; WT interleave migration)
0.134 0x4E0AB2(0xA559F0; macro-name gate) · 0.03 0x4E1885(0xA559F4)
0.147 0x4E1A0F/0x4E1BDE(0xA559F8) · 0.144 0x4E1C8E(0xA559FC)
0.154 0x4E3F20(0xA55990) · 0.05 0x4E454E (VoiceFilter; mulsd 0.05)
0.150 0x4E5C51(0xA1BFD8; serum1Version escape)
```

Representative migrations: mod-slot dest codes (`0xE3→0xE4→0x13C`,
0x4DB968–0x4DB956), default `0x13C00AD` dword writes for dead slots
(0x4DC125–0x4DC57D), FX mirror `state+0x37F4..0x3834 → 0x4B64`
(0x4DBBB9–0x4DBC13), default block `0x179F7A0 → state+0x4AE0`
(31 f64→f32 pairs, 0x4DBC15–0x4DBC82), mod-slot 17–32 mirror init
(0x4DBC89–0x4DC57D), state+0x3688 rescale `v·999/999.9` (0x4DD6A5–0x4DD6D5).

## 11. Coverage summary (299 S1 params)

* **248** master params (blob `0x3460`, incl. master params 228–247 = FX-mirror
  head) are individually pushed through `fn_setval` → S2 `plainParams` /
  FX-knob cells; each either lands on a `kParamXxx` name (descriptor non-null)
  or is **silently dropped** (descriptor null — count not statically
  determinable, see below).
* **Explicitly dropped** by `fn_setval`: idx 315, 316, 341 (bitmask
  `0x8400003`), idx 330 (`cmp esi, 0x14A`).
* **Explicit special-cased**: (FX6, 0x53)→kPlate+PreDelay·0.001;
  (FX0x63, 0x63)→kDiode1+Drive·0.875+12.5; (FX5, 0x87)→4 hardcoded comp values;
  (FX7, 0x5C)→FXPhaser kParamDepth2 / 1.06875; (FX8, 0x8F) raw; idx 0x64 raw.
* The `state+0x4AE0` block (71 f32, "params 228–298" in the fxp doc's
  numbering) is **not read** by the converter's master loop (which reads only
  0x3460+4·i, i<248); it is *written with defaults* from table 0x179F7A0 and
  used as migration scratch (0x4DC590 copy, 0x4DD1DE 88/89 migration). Net:
  51 chunk-level params in that model are **not converted** by this path.
* Non-param S1 data all accounted for: order table (10), mod slots (32 x 2
  amounts + dest/src codes), LFO blocks (8 + 2 flex + 2 phasor), scalars (2),
  macro names (4), WT/noise names (3), embedded streams (3 kinds), tuning
  (2 fields), locks (2 bits), meta (8 keys).
* **Rough count**: 248 master params converted-or-dropped-by-descriptor
  + 51 `0x4AE0` params never read + explicit drops {315,316,330,341} ≈
  **248 mapped (with unknown descriptor-null subtractions), 51+4 not** —
  i.e. the S2-visible surface the importer produces is
  `plainParams` + `"+ FX"` cells + `ModSlot0..31` + `LFO0..9` + `scalars` +
  `Global0` + `VoiceFilter0` + `midiMap` + meta, matching the 162-key layout of
  `serum2-state-format.md` §2.2.

**Confidence tags** used above: `[certain]` = instruction-verified in this
session (all formulas/addresses quoted); `[descriptor]` = the param-name/FX-type
tables at 0x179B060 are runtime-relocated, so individual `kParamXxx`
strings/type ids per S1 index are referenced but not enumerable statically;
`[layout]` = the 0x4AE0 numbering discrepancy vs `serum-fxp-format.md`.

## 12. Sanity check vs. Factory presets — skipped (documented)

`Documents\Xfer\Serum 2 Presets\Presets\Factory\` holds 626 `.SerumPreset`
files, but none matches a Serum factory name (checked for YUKIYANAGI / UKHC /
IMPOSE / Reese-blind etc.); the S1 factory library is not installed on this
machine (`Tables\User` contains only `SaveYourTablesHere.txt`; `Samples\` holds
only Factory `.flac` sets). No S1/S2 pair was available, so task 4's empirical
cross-check was **skipped** — per instructions the mapping above rests on the
binary analysis alone. (Consistency checks that *were* possible: the emitted
field names/paths match the CBOR grammar of `serum2-state-format.md` §2.1–2.2 —
`plainParams`, `curveData`, `relativePathToWT`, `destModule*`, `embedded*` —
and the two state-level round-trip quirks described there.)

## 13. Scratch artifacts

`C:\Users\cabbage\AppData\Local\Temp\opencode\conv_work\s2re\`:
`disfn.py` (annotated ranged disassembler), `s1load.txt` (full 8 770-insn dump
of 0x4DABC0–0x4E61CA), `fn_setval.txt`, `fn_4d9da0.txt`, `fn_4d9c50.txt`,
`fn_4d93b0.txt`, `fn_curve.txt` (0x4F2A70), `fn_4f1eb0.txt`, `fn_4f3e00.txt`,
`consts.txt` (169 resolved float operands), `consts.py/consts2.py`,
`counts.py/counts2.py`, `setval_sites.py` (xmm2 provenance at all 18
`fn_setval` call sites), `probe2.py/probe3.py` (rip-target/string resolution),
`opt_scan2.py` (option-list scan), `strings_named.json` (840 name strings),
`macro_gate.txt` and prior-session tooling reused from
`...\Temp\opencode\work\` (strings_scan, xref, ref_hits.json).

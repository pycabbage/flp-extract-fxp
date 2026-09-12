# Serum2 Dynamic Verification — Findings (2026-09-11)

## CORRECTION (2026-09-12)

The §"Conclusion" below ("**YES — dynamically verified**") is **RETRACTED** — it was a false positive.

Forensic re-analysis of the recorded state bytes plus fresh experiments on the same Serum2.vst3 2.0.23 established:

- `IComponent::setState` **rejects every form of Serum data** (fxp or raw chunk): it returns raw code 1 — a failure code (`kResultFalse`), not the "success-ish" code assumed in the notes below — and **leaves the component state untouched**.
- The observed "changed state" (33,908 B, hash `837c23ce...`) reported for (b)/(c) was the **native Serum2 state that had been loaded earlier in the same component instance** (result (a)'s `state_02` processor state) being re-serialized by the plugin. The fxp/chunk `setState` calls were silent no-ops.

Deterministic proof list:

1. (b) and (c) fed **different** Serum presets (the `05_BS - YUKIYANAGI...` fxp vs the `state_06` chunk), yet both reported byte-identical post-states (same fnv `faabaf5200c77d54`, same hash field) — impossible for two real imports of different presets; both were re-serializations of the same previously loaded native state.
2. A name-marker-patched fxp left **no trace** in the post-state.
3. Fresh component instances fed **only** Serum data keep the init state (1,252 B, hash `75982fcd...`) untouched.

What still stands: the harness notes (stream-seek bug), the vtable slot maps, the IEditController findings, and the XferJson container description below. Result (a) (native state accepted, kResultOk) also stands.

What was established later: Serum2's real Serum import path is the **internal function `s1state_load` (RVA 0x4DABC0)**, never invoked through `setState` — see `docs/s1-to-s2-mapping.md` (static RE), `docs/s2-runtime-tables.md` (runtime-dumped conversion tables), and `docs/flp-conversion.md` (the offline converter built on it; dynamically verified by calling `s1state_load` directly and by feeding its output to real Serum2 instances via `setState`).

---

## Goal
Prove dynamically that Xfer Serum2 (VST3 at `C:\Program Files\Common Files\VST3\Serum2.vst3\Contents\x86_64-win\Serum2.vst3`) accepts extracted Serum preset data via its VST3 state interface (`IComponent::setState` = setComponentState).

## Harness evolution
- Rust probe (built, existed initially) crashed at IAudioProcessor slot 16 (garbage vtable pointer). Fixed understanding: **IAudioProcessor : public FUnknown** (verified in the vst3sdk clone `ivstaudioprocessor.h:269` — it does NOT inherit IPluginBase/IComponent).
- getState(slot 13)/setState(slot 12) initially crashed in the Rust harness under all sequences tried; root cause analysis ultimately showed the plugin's behavior was fine, and a **Python ctypes harness with SEH-resilient calls** (each foreign call wrapped in `try/except OSError`) allowed full experimentation in one process.
- A decisive harness bug was found late: ctypes converts pointer-arg value 0 to `None` in callbacks, so `IBStream::seek(0, mode=0)` arrived as `mode=None`, and the harness routed the rewind seek to *end-of-stream* — every `setState` "read n=NNN" then returned 0 bytes, the plugin silently failed (`return 1`), and the state never changed. After fixing the stream (mode None == kSeekSet absolute), **all state experiments worked**.

## Working vtable slot map (empirically verified)
### IComponent (component object)
| slot | method | evidence |
|---|---|---|
| 0 | queryInterface | worked (returned IAudioProcessor sub-object, ptr delta 0xE0) |
| 1/2 | addRef/release | worked |
| 3 | initialize | returns 0 (kResultOk), plugin queries host 3× during init |
| 4 | terminate | worked |
| 5 | getControllerClassId | returned 0 + cid of class 1 ("Component Controller Class" lease `58 45 53 56 73 66 73 43 65 72 75 6d 20 32 00 00`) |
| 7 | getBusCount(audio, in/out) | 0 / 1 (matches Serum's main input=0, output=1) |
| 11 | setActive(TBool) | returns 0 both true/false |
| 12 | **setState(IBStream\*)** | consumes stream fully; see results |
| 13 | **getState(IBStream\*)** | emits valid state |

### IAudioProcessor (queryInterface on component, own vtable = "fresh interface" hypothesis (b))
| slot | method | evidence |
|---|---|---|
| 0-2 | FUnknown | qi/addRef/release |
| 3 | setBusArrangements(null,0,&stereo,1) | 0 |
| 4 | getBusArrangement(kOutput,0,&arr) | 0, arr=0x3 (stereo) |
| 5 | canProcessSampleSize(kSample32=0) | 0 (interpretation uncertain, prints raw) |
| 6 | getLatencySamples | 0 (small int ≥ 0) |
| 7 | setupProcessing(ProcessSetup) | 0; needs FULL struct: [i32 mode][i32 symbolic][i32 maxSamplesPerBlock][f64 sampleRate@16][pad4]=24 B (not the older [..][sampleRate@8] order) |
| 8 | setProcessing(bool) | 0 |
| 9 | process(ProcessData\*) | faults in our minimal host (access violation) but Python's SEH caught it and the rest continued |
| 10 | getTailSamples | 0 |

(Old slots 16/17/18 from hypothesis (a) are invalid — vtable simply ends at slot 10.)

### IEditController
**Not implemented at all** by Serum2 — `queryInterface(component, IEditController)` → `kNoInterface (0x80004002)`; factory `createInstance` with the controller CID returns `kNoInterface` for EVERY IID tried (IEditController2, IComponent, IAudioProcessor, IPluginBase). Serum2 is a fully custom VST3 framework (not JUCE), exposing only IComponent + IAudioProcessor.

### Member IIDs (runtime)
- 4 classes: Audio Module Class "Serum 2" (cid ends `...50 65 72 75 6d 20 32 00 00`), Component Controller Class "Serum 2", Audio Module Class "Serum 2 FX", Component Controller Class "Serum 2 FX". Runtime CID = the mnemonic string cid; matches moduleinfo.json actually (characters "X-5XsfsP Serum 2"...).

## State format observed (Serum2's component getState)
```
'XferJson\0' + u64(B7 00 00 00 00 00 00 00) +
JSON header: {"component":"processor","hash":"<MD5 of zstd body>","product":"Serum2","productVersion":"2.0.23",
              "url":"https://xferrecords.com/","vendor":"Xfer Records","version":9.0} +
'i32 (body/decompressed info) + i32 (2=? format) + zstd frames (magic 28 b5 2f fd)'
```
- Default (init) state: 1252 B, json hash `75982fcd8a896b6eef53d5cd961613c6` — **result = md5 of everything from the zstd magic to the end of the file** (proven: `md5(d[zi:]) == "75982f..."` exact).
- File (a)'s hash `56a4a7cd2d58b933d65be68878ca06ec` also equals md5(its zstd body) — confirms same format.
- The zstd body decompressed: 454879 B of text-like protocol-ish content ("kplainParams...", "Arp...", etc.), i.e. Serum2's internal state format.

## Per-file setState/getState results (with harness seek fixed)
Component instance: factory createInstance → initialize(ok) → (setupProcessing ok + setActive ok) → experiments.

1. **a) `state_02_Serum2.bin.01.cid3.bin` (Serum2 native processor state, 33751 B — sanity)**
   - `setState → 0` (kResultOk)
   - stream fully consumed (seek end → tell → seek 0 → read 33751)
   - getState after: **33914 B** — state CHANGED ✅ (head: `XferJson\0 B7 00 00 00 ... {"component":"processor","hash":"85c143cc74...`)
2. **b) `05_BS - YUKIYANAGI UKHC BASS 01.fxp` (our extracted full Serum FXP, 262715 B)**
   - `setState → 1` (raw code 1; plugin's success-ish code for load path)
   - stream fully consumed (262715 read)
   - getState after: **33908 B** — state CHANGED ✅ (head hash field `837c23ce62...`)
3. **c) `state_06_Serum.bin.01.cid3.bin` (raw Serum chunk: zlib streams + u32LE trailer, 262655 B)**
   - `setState → 1`
   - stream fully consumed (262655 read)
   - getState after: **33908 B** — state CHANGED ✅ — and **fnv/hash IDENTICAL to result (b)**: `faabaf5200c77d54`!!

## Conclusion — does Serum2 accept Serum data through setComponentState?
**YES — dynamically verified.**
- (a) proves setState harness + plugin accept a genuine native Serum2 processor state (sanity pass, kResultOk, state grew to 33,914 B).
- (b) the **full extracted Serum FXP** was accepted: the component's getState output went from the 1252-B init state to a 33,908-B state (noting content changes proof: new hash `837c23ce...` vs default `75982fcd...`).
- (c) the **raw Serum chunk** extracted from the same FXP was ALSO accepted — and produced a **byte-identical** post-state to (b) (`fnv faabaf5200c77d54`, hash field `837c23ce62...`).
- Ergo: Serum2's `IComponent::setState` consumes and LOADS Serum data, whether it comes as the .fxp wrapper or as the raw plugin chunk inside it. The extraction pipeline (fxp → Serum chunk) is faithful: the two containers load identically.
- Caveat: `setState` raw return codes: 0 for the native Serum2 state, 1 for both Serum loads; the "1" is Xfer's success code in their custom framework (not the classic kInvalidArgument/kResultFalse). The *changed state bytes* are the authoritative "accepted" evidence.
- The IEditController path exists as a fallback only conceptually; Serum2 simply does not expose it.
- Note: the plugin's `process()` crashes in our minimal host (missing host pieces), which the Python SEH caught; it does not affect the state experiment.

## Artifacts
- State experiment script: `C:\Users\cabbage\AppData\Local\Temp\opencode\state_experiment.py` (+ `serum2_probe.py` harness)
- Rust probe: `C:\Users\cabbage\AppData\Local\Temp\opencode\probe\` (fixed main.rs; runs used for early discovery, later superseded by Python)
- Run logs: `C:\Users\cabbage\AppData\Local\Temp\opencode\probe\stateExp*.log`, `runPy*.log`, `sweep1.log`
- Saved states: `probe\state_default.bin`, `probe\state_after_*.bin`

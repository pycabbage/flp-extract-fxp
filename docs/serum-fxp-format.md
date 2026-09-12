# Serum (.fxp) Preset File Format — Byte-Level Specification

**Purpose:** enable constructing valid Xfer Serum 1.x `.fxp` preset files (importable by
Serum2, e.g. `C:\Program Files\Common Files\VST3\Serum2.vst3`) from an extracted
172,736-byte Serum state "chunk" plus optional appended data streams
(as found decompressed inside FL Studio VST3 plugin states).

**Date of research:** 2026-09-11
**Method:** 25 real Serum `.fxp` files downloaded from public GitHub repos
(2015-era through 2026-era, i.e. Serum 1.0x → 1.334), byte-level parsing, zlib stream
analysis, cross-checked against three independent implementations:
`btesser/serum2vital` (plugin-verified parser + `craft_fxp.py` writer),
`potatoTeto/SerumPresetGenerator` (C#, partially incorrect — see notes), and string
analysis of the Serum2.vst3 binary itself. Round-trip construction from the FL-state
sections in `C:\Users\cabbage\AppData\Local\Temp\opencode\serum_states\sections\`
was performed and validated structurally.

---

## 1. File container (60-byte header + chunk)

Serum `.fxp` is a VST2 *opaque chunk program* file ("FPCh" flavor of the Steinberg
`CcnK` preset format). All multi-byte integers below are **big-endian** unless marked LE.

| Offset | Size | Field                | Value in every Serum file examined |
|-------:|-----:|----------------------|--------------------------------------|
| 0x00   | 4    | chunkMagic           | ASCII `CcnK` (43 63 6E 4B) |
| 0x04   | 4    | byteSize (BE)        | **the whole file length** (= 60 + chunkSize). *Serum deviates from the Steinberg spec (which says fileLen−8); both Serum and Serum2 accept the file-length value Serum writes.* |
| 0x08   | 4    | fxMagic              | ASCII `FPCh` (46 50 43 68) |
| 0x0C   | 4    | format version (BE)  | `1` |
| 0x10   | 4    | fxProgramID          | ASCII `XfsX` (58 66 73 58) — Serum's VST plugin ID |
| 0x14   | 4    | fxVersion (BE)       | `1` (constant; not a real version number) |
| 0x18   | 4    | numParams (BE)       | `1` (constant, despite Serum having 299 params — chunk-based presets just use 1) |
| 0x1C   | 28   | prgName              | **preset name #1**, ASCII, NUL-padded to 28 bytes |
| 0x38   | 4    | chunkSize (BE)       | length in bytes of everything that follows |
| 0x3C   | …    | chunk                | `chunkSize` bytes (see §2) |

Verified relationships (all 25 files):

```
fileLen  = 60 + chunkSize        (no trailing bytes after the chunk, ever)
byteSize = fileLen               (== 8 + 52 + chunkSize; NOT fileLen − 8)
```

Reference hexdump of a real header (`IMPOSE_00.fxp`, prgName
"AU_MT_bass_memories_imposing", chunkSize 0x18E9 = 6377):

```
0000  43 63 6E 4B 00 00 19 25 46 50 43 68 00 00 00 01   CcnK...%FPCh....
0010  58 66 73 58 00 00 00 01 00 00 00 01 41 55 5F 4D   XfsX........AU_M
0020  54 5F 62 61 73 73 5F 6D 65 6D 6F 72 69 65 73 5F   T_bass_memories_
0030  69 6D 70 6F 73 69 6E 67 00 00 18 E9 78 01 ED 9D   imposing....x...
                                                  ^^ ^^ zlib stream starts (78 01)
```

## 2. The chunk: concatenated zlib streams + trailing length word

The chunk is **not** a single zlib stream. It is:

```
chunk = zlib_stream_0            state blob, decompresses to 172,736 bytes (Serum ≥ ~1.2)
      [ zlib_stream_1 … ]        0 or more appended data streams (embedded wavetable /
                                 noise / filter-table data), directly concatenated,
                                 no separators, no per-stream headers
      u32 LE                     compressed size of zlib_stream_0 (bytes)
```

Rules verified across every sample:

* Streams are **back-to-back**; each is a complete RFC-1950 zlib stream. Serum writes
  them at **zlib level 1** (`78 01` header). Any compression level works for readers
  (level 6 → `78 9C` also parses), but use level 1 for byte-fidelity with Serum output.
* The final **u32 little-endian** equals the *compressed* byte length of the *first*
  stream only. Example (`IMPOSE_00.fxp`): stream0 comp=3027 (0x0BD3) → tail bytes
  `D3 0B 00 00`.
* There are **never** bytes after that u32 (fileLen = 60 + chunkSize exactly).
* **Serum refuses chunks whose appended streams or final length word are missing or
  wrong** — it silently keeps its previous state (preset "loads as Init"). This is
  plugin-verified by serum2vital's `tools/craft_fxp.py` and matches the identical
  trailer seen in FL Studio's VST3 state storage (the "trailing u32" you observed).
* Even presets with nothing custom embedded carry at least one second stream:
  * current builds: a 16,384-byte stream (2 frames of the default saw wavetable —
    md5 `1765102abf33a9e773051f1ffa006ace`, identical e.g. in `IMPOSE_00.fxp` and
    FL-state `state_00…s1.bin`);
  * 2015-era builds: an *empty* zlib stream (8 bytes: `78 01 03 00 00 00 00 01`).

### 2.1 Appended data streams (embedded wavetable / noise data)

* Decompressed content of a wavetable stream = **raw float32 LE sample frames**,
  **2048 samples per frame → 8192 bytes per frame**, mono. Stream sizes are therefore
  always multiples of 8192 (observed: 16384=2 frames, 24576=3, 229376=28, 294912=36,
  475136=58, 2088960=255, 4161536=508 frames …).
  * Default/Init data: 2 identical sawtooth frames (value ramp ±0.999 in 1/1024 steps).
  * A preset whose OSC A and OSC B each load a 1-frame custom table carries one stream
    with both frames concatenated (e.g. `ba_bl1nd3rr.fxp`: names `User\VECNA13`,
    `User\VECNA14`, one 16,384-byte stream with 2 different frames).
* 2015-era (old-format) presets instead embed the **raw bytes of a `.wt` text file**
  (AnaMark format, starts with `"; AnaMark section…"`) — evidence that the streams are
  simply "the referenced data blob, zlib-compressed".
* The Serum2 binary references `embeddedWTData`, `embeddedNoiseData`,
  `embeddedFilterTableData` — three kinds of embeddable blobs exist. **Order and
  contents must be preserved verbatim from the source state; do not reorder.**

### 2.2 Old-format presets (background — not needed for construction)

Presets written by Serum ≤ ~1.07 (2015) have a **variable-length** state blob
(21,808 / 28,232 bytes observed) instead of 172,736, with a different internal layout
("classic" LFO block at 0x0280). Serum2 still imports them. If you ever need to emit
one, the container rules are identical.

## 3. The 172,736-byte state blob (stream 0 contents)

Offsets below verified byte-level on modern (172,736-byte) presets and identical in
structure to the FL-state `*.s0.bin` files (99.6–99.7 % byte-identical to real presets;
36 of 43 4-KiB blocks fully identical — differences are parameter values only).

| Offset | Size | Contents |
|-------:|-----:|----------|
| 0x0000 | 640  | modulation slots 1–16: 16 records × 40 bytes; each record self-identifies with bytes `80 <slot> FF` at +0x21 (byte at +0x20 is usually 0x80) |
| 0x0280 | …    | classic-layout LFO 1–4 shapes (12 arrays × 65 float64, stride 520) — **all zero in 172,736-byte blobs** |
| 0x1AE0 | …    | classic-layout LFO 1–4 switch flags |
| 0x1B70 | …    | classic-layout LFO 5–8 shapes (zeroed in new layout) |
| 0x33D0 | …    | classic-layout LFO 5–8 anchors |
| 0x3460 | 912  | parameters 0–227 (228 × float32 LE, normalized 0..1) |
| 0x37F0 | 1008 | per-effect record region (FX knob mirrors; reverb Plate/Hall byte at 0x3B04) |
| 0x3BE0 | 40   | FX rack order (10 × int32 LE) |
| 0x3C08 | 512  | OSC A wavetable name (NUL-terminated string, e.g. `User\VECNA13`, `Analog/Basic Shapes.wav`; empty when the built-in " - Init -" saw is used) |
| 0x3E08 | 512  | OSC B wavetable name |
| 0x4008 | 512  | noise sample name (e.g. `Organics\AC hum1.wav`) |
| 0x4972 | 32   | **preset name #2** (NUL-padded; this is what Serum's browser shows) |
| 0x4994 | 4    | float32 (varies slightly per preset; unknown) |
| 0x49A0 | 48   | **author** string (NUL-padded; 11×0x20 spaces when unset) |
| 0x49D0 | 48   | **menu / category / bank** string (NUL-padded) |
| 0x4A58 | 8    | per-preset binary ID (random bytes; e.g. `NDDEKICF`, `QDCELMLD`, `F4F3F8F2…` — *not* a constant magic; preserve as-is) |
| 0x4A60 | 128  | macro 1–4 names (4 × 32 bytes, NUL-padded; defaults `Macro 1`…`Macro 4`) |
| 0x4AE0 | 284  | parameters 228–298 (71 × float32 LE) |
| 0x4C48 | 100  | global switches block (voicing, tuning, polyphony, oversampling…; float32 fields) |
| 0x84D8 | …    | 8 LFO blocks × 0x2D28 (tension/x/y arrays of 480 float64 + flags + point count); further graph blocks follow |
| varies | …    | modulation slots 17–32 (located by their `80 <slot> FF` markers) |
| …0x2A2C0 | ~64 | tail: repeating `00 00 00 00 00 00 F0 3F` float pairs, then `u32 LE = 1`; some bank presets additionally carry a 32-byte session-specific blob (`F6 0A 15 12 …`) — preserve verbatim |

Total size: **172,736 = 0x2A300 bytes** (current builds; constant).

First bytes of a default (Init-like) state — matches FL-state `state_00…s0.bin`
byte-for-byte in the default regions:

```
000000  00 00 00 3F 00 00 00 00 00 00 80 3F 00 00 00 00   (0.5f, 0.0f, 1.0f, 0.0f …)
000010  00 00 00 00 00 00 00 00 AD 00 3C 01 00 00 00 00
000020  80 80 00 FF 00 00 00 00 …                          (mod slot 1 marker at +0x21)
```

## 4. Preset name — where it goes

1. **FXP header, offset 0x1C, 28 bytes**, NUL-padded ASCII (Steinberg prgName).
2. **Inside the state blob at 0x4972, 32 bytes**, NUL-padded (Serum's own name field,
   shown in the preset browser; `state_00`'s value ` - Init -reese` sits exactly here).

**Set both to the same string** (≤ 27 chars + NUL is always safe; the 0x4972 field
allows 31). Optional: author at 0x49A0 (48 B), category/menu at 0x49D0 (48 B).

`flp-extract-fxp patch <file.fxp> --name/--author/--category` implements exactly
this (header mirror + state fields, stream-0 recompressed at zlib level 1,
trailer/chunkSize/byteSize recomputed, everything else byte-identical); the same
patching also works on every Serum instance of an FLP
(`patch project.flp --name … --out …`). Verified dynamically: the real
Serum2.vst3 `s1state_load` accepts a patched fxp and reports the new name.

### 4.1 About the JSON metadata block

No `{"author":…,"preset_name":…}` JSON block exists in **any** of the 25 real Serum
`.fxp` files examined (2015→2026, incl. files saved 2026 by Serum 1.334 with author/menu
text filled in — those use the plain 0x49A0/0x49D0 fields). The Serum2 binary contains
zero occurrences of `preset_name` / `"author"` — its Serum metadata re-import uses the
fixed fields above plus its own database. The JSON-with-`preset_name` pattern matches
**Serum2** `.SerumPreset` files (`XferJson\0` container; keys `presetName`,
`presetAuthor`), which is almost certainly what was remembered. **Conclusion: no JSON
is needed or read; do not emit one.**

## 5. Construction recipe

Given: `state` (172,736 bytes, your `s0`), `extras` = [`s1`, `s2`, …] appended data
streams in original order (may be empty), and `name`:

```python
import struct, zlib

def build_serum1_fxp(state: bytes, extras: list[bytes], name: str) -> bytes:
    z0 = zlib.compress(state, 1)                    # level 1 => 78 01, as Serum writes
    chunk = z0
    for s in extras:                                # embedded WT/noise blobs, verbatim
        chunk += zlib.compress(s, 1)
    if not extras:                                  # fallback: keep the mandatory
        chunk += zlib.compress(b'', 1)              #   (possibly empty) second stream
    chunk += struct.pack('<I', len(z0))             # trailing length word (REQUIRED)

    nb = name.encode('ascii', 'replace')[:28]
    hdr  = b'CcnK'
    hdr += struct.pack('>I', 60 + len(chunk))       # byteSize = whole file length
    hdr += b'FPCh'
    hdr += struct.pack('>I', 1)                     # version
    hdr += b'XfsX'                                  # fxProgramID
    hdr += struct.pack('>I', 1)                     # fxVersion
    hdr += struct.pack('>I', 1)                     # numParams
    hdr += nb.ljust(28, b'\x00')                    # prgName (28 bytes)
    hdr += struct.pack('>I', len(chunk))            # chunkSize
    return hdr + chunk
```

When patching the name into a state blob you did not generate, write the NUL-terminated
name at **0x4972** (32-byte field) and mirror it into the header's prgName. Leave
everything else — including the 8-byte ID at 0x4A58 and the tail marker bytes —
untouched.

**Critical failure modes (verified):**
* omitting the trailing u32, or setting it ≠ compressed size of stream 0 → Serum
  silently ignores the preset (keeps previous state / looks like Init);
* `byteSize` wrong → hosts may truncate; use file length as Serum does;
* name only in the header but not at 0x4972 → preset works but shows the old/internal
  name in the browser.

## 6. Validation performed

* Round-trip: built `BUILT_state00/01/06.fxp` from the FL-state sections; structure
  re-parsed identically to genuine files (streams `[172736, 16384|24576|294912]`,
  trailing u32 == comp(stream0), byteSize == fileLen == 60+chunkSize).
* `state_00…s1.bin` is byte-identical (md5 `1765102a…`) to the second stream of a real
  2026 commercial preset using only default tables — confirming FL-state sections and
  fxp streams are the same objects.
* Layout comparison `state_00.s0` vs real fxp s0 blobs: 99.6–99.7 % identical;
  identical anchors (mod markers at +0x21, wt-name fields, `Macro 1` at 0x4A60,
  1.0f-array tail) — same format.

## 7. Reference files

Downloaded to `C:\Users\cabbage\AppData\Local\Temp\opencode\refs\serum_fxp\`:

| File | Size | Source | Era / state size |
|------|-----:|--------|------------------|
| `IMPOSE_00.fxp`, `IMPOSE_01.fxp`, `IMPOSE_09.fxp` | ~6.4 KB | github.com/djOffstage/impose-serum-bass-pack | 2026, Serum 1.334, 172736 + 16384 |
| `ba_bl1nd3rr.fxp`, `ba_c1ass1c.fxp`, `ba_t1r3d.fxp`, `ba_vaccuum.fxp`, `ba_gr1t.fxp`, `ba_hUnnA.fxp`, `fx_w0aH.fxp`, `ld_b@r5.fxp` | 18 KB–3.7 MB | github.com/AqtoPy/SerumPresets3 | modern, custom wavetables (2–508 frames) |
| `FL_Heavenly.fxp`, `FL_Cryptic.fxp`, `FL_Downpour.fxp`, `FL_BASS_Adventure.fxp`, `FL_BeautyBeast.fxp`, `FL_FMItUp.fxp` | 3–10 KB | github.com/Flmastersv/SerumPresets | 2015-era, 21808/28232-byte states |
| `2080.fxp`, `808_boomer.fxp`, `808_basic_bitch.fxp`, `808_harmonic.fxp`, `808_kossilator.fxp`, `808_p_bass.fxp`, `808_from_the_dysopian_future.fxp`, `808_microkorg_buzzer.fxp` | 2 KB–1.9 MB | github.com/Miserlou/SynthRecipies | 2015-era |
| `*_ref_*` | — | serum2vital sources (serum1.py, FORMATS.md, craft_fxp.py, wavetables.py, serum2.py), potatoTeto C# | cross-check references |

Extracted artifacts: `*.fxp.s0.bin` (172,736-byte states), `*.fxp.s1.bin` (embedded
data), `BUILT_state*.fxp` (round-trip test outputs), analysis scripts
(`analyze_chunk.py`, `deep_analyze.py`, `region_dump.py`, `sweep.py`,
`roundtrip_test.py`).

### Note on potatoTeto/SerumPresetGenerator

Its `FxpHeader.cs` misreads the header (claims a 32-byte preset name at 0x1C and
little-endian version fields — real files use 28 bytes at 0x1C and big-endian).
Do not use it as a reference; serum2vital + direct byte analysis agree with the tables
above.

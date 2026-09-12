# FLP: PluginParams structure for Serum vs Serum2 (real-sample calibration)

Empirical, byte-level reference for FL Studio's `PluginParams` event (id 213)
as saved by **FL Studio 24.2.2.4259** (version string in event 199 of the
samples), calibrated against a real project containing both a Serum and a
Serum2 VST3 instance.

All observations below come from the sample `assets/serina flps/serina1/serina1.flp`
(5 Serum instances + 1 genuine Serum2 instance + 40 other plugin instances),
cross-checked against `serina4.flp` (7 × Serum, 1 × Serum2) and
`serina3/Project_7.flp` (2 × Serum, no Serum2). Scripts/logs used for the
calibration are not in the repo; numbers here are transcribed from those dumps.

Related docs: `serum-fxp-format.md` (Serum fxp), `serum2-importer-analysis.md`
(importer validation), `serum2-dynamic-verification.md` (live VST3 probes).

## 1. Container recap

- File = `FLhd` + u32 LE length + payload, then `FLdt` + u32 LE length + event
  stream. In **all three sample FLPs the `FLdt` payload is a raw event stream —
  not zlib-compressed** (first byte is an event id, not `0x78`), and there are
  **zero trailing bytes** after the declared `FLdt` length (file length equals
  `FLhd` + `FLdt` + `dtlen` exactly).
- `FLhd` payload in the samples: `00 00 46 00 60 00` (version 0, 70 channels,
  ...), unchanged relative to older FLPs.
- Event framing (see `src/flp.rs`): `0..63` byte, `64..127` word,
  `128..191` dword, `>=192` varint-length payload. Event 213 = `0xD5` followed
  by a varint (e.g. 16542 → `9E 81 01`, 47990 → `F6 F6 02`).
- Zip-packed (`PK`-prefixed) FLPs are unpacked in memory before parsing
  (`src/zip.rs` via `core::flp_inputs`; see AGENTS.md). Nested archives
  (zip-in-zip) are not recursed into.

## 2. `PluginParams` (event 213) top-level payload

Layout: `u32 LE format version`, then a sequence of records
`[u32 LE cid][u64 LE size][data]`. This matches `parse_plugin_params` in
`src/flp.rs`. All samples use **version 12**.

Record order is fixed and identical for Serum and Serum2 (and every other
VST3 plugin observed):

```
cid 1, cid 2, cid 30, cid 32, cid 50, cid 52, cid 54 (name),
cid 55 (filename), cid 56 (vendor), cid 53 (state, LAST)
```

### 2.1 Record inventory — Serum instance #0 (stream offset 0x2E6B)

Total event payload 16,542 bytes.

| cid | meaning        | size   | payload                                                        |
|-----|----------------|--------|----------------------------------------------------------------|
| 1   | slot flags?    | 20     | see §2.3                                                       |
| 2   | runtime record | 25     | see §2.4                                                       |
| 30  | ?              | 16     | `00 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00`              |
| 32  | ?              | 12     | `00 00 00 00 01 00 00 00 00 00 00 00`                          |
| 50  | ?              | 16     | `08 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00`              |
| 52  | plugin UID     | 16     | `58 54 53 56 73 66 73 58 65 72 75 6D 00 00 00 00` (= ASCII `XTSVsfsXerum`) |
| 54  | name           | 5      | `53 65 72 75 6D` = `Serum`                                     |
| 55  | filename       | 39     | `/Library/Audio/Plug-Ins/VST3/Serum.vst3`                      |
| 56  | vendor         | 12     | `58 66 65 72 20 52 65 63 6F 72 64 73` = `Xfer Records`          |
| 53  | state          | 16,257 | FL VST3 wrapper (§3)                                           |

### 2.2 Record inventory — Serum2 instance (stream offset 0xE69C)

Total event payload 47,990 bytes.

| cid | meaning        | size   | payload                                                        |
|-----|----------------|--------|----------------------------------------------------------------|
| 1   | slot flags?    | 20     | see §2.3 — **u32@8 = 1, not 12**                                |
| 2   | runtime record | 25     | see §2.4                                                       |
| 30  | ?              | 16     | identical to Serum                                           |
| 32  | ?              | 12     | identical to Serum                                           |
| 50  | ?              | 16     | identical to Serum                                           |
| 52  | plugin UID     | 16     | `58 45 53 56 73 66 73 50 65 72 75 6D 20 32 00 00` (= ASCII `XESVsfsPerum 2`) |
| 54  | name           | 6      | `53 65 72 75 6D 32` = `Serum2`                                 |
| 55  | filename       | 40     | `/Library/Audio/Plug-Ins/VST3/Serum2.vst3`                     |
| 56  | vendor         | 12     | identical to Serum (`Xfer Records`)                          |
| 53  | state          | 47,703 | FL VST3 wrapper (§3, §4)                                       |

### 2.3 Top-level cid 1 — the only 20-byte record that differs between S1/S2

```
S1: FF FF FF FF FF FF FF FF  0C 00 00 00  00 00 00 00 00 00 00 00
S2: FF FF FF FF FF FF FF FF  01 00 00 00  00 00 00 00 00 00 00 00
```

Structure: `u64 -1 (0xFF×8) + u32 A + u32 B + u32 C` with `B = C = 0`.
Across **all 60 plugin instances in the three sample projects** (Vital,
Kilohearts ×5, OTT, Disperser, Maim, Chroma, Xpand!2, GClip (VST2), Youlean,
Spectral Gate, Ozone Imager 2, Serum ×14), `A = 12` for **every** plugin
except the Serum2 instances, where `A = 1`. The value is stable per plugin
across all three files, so it is plugin-keyed, not save-time noise. Semantics
unknown (candidate: FL's plugin-handler/format code). A converter that wants
to mirror a genuine Serum2 instance byte-for-byte should write `01 00 00 00`
here; all other observed plugins (including other VST3s) use `0C 00 00 00`.

### 2.4 Top-level cid 2 — runtime/opaque, not plugin-keyed

```
S1 (serina1): 00 A0 00 00 00 19 00 00 00 8D 7D 20 A4 00 00 00 00 01 00 00 00 00 00 00 00
S2 (serina1): 00 A0 00 00 00 19 00 00 00 8C 7D 20 A4 00 00 00 00 01 00 00 00 00 00 00 00
S1 (serina4, some): ... 8D 7D 20 24 ...   (differs from other S1 instances in the same file)
```

Only the 4 bytes at offsets 9–12 differ, and they differ *between instances of
the same plugin inside one file* (`A4207D8D` vs `A4207D24`), so they are
per-instance runtime state (looks like a save-time counter/timestamp), not
plugin identity. **No change needed when swapping plugins.**

### 2.5 String encodings (54/55/56)

Name, filename and vendor payloads are **raw UTF-8/ASCII, no NUL terminator,
no length prefix inside the record** (the record header already carries the
size). They are **not** UTF-16. Verified values:

```
S1 name     : 53 65 72 75 6D                                  -> "Serum"
S2 name     : 53 65 72 75 6D 32                               -> "Serum2"
S1 filename : 2F 4C 69 62 72 61 72 79 2F 41 75 64 69 6F 2F 50 6C 75 67 2D 49 6E 73 2F
              56 53 54 33 2F 53 65 72 75 6D 2E 76 73 74 33    -> "/Library/Audio/Plug-Ins/VST3/Serum.vst3"
S2 filename : (same prefix) ... 53 65 72 75 6D 32 2E 76 73 74 33 -> "/Library/Audio/Plug-Ins/VST3/Serum2.vst3"
S1/S2 vendor: 58 66 65 72 20 52 65 63 6F 72 64 73             -> "Xfer Records"
```

(The samples were saved on macOS; on Windows FL writes backslash paths such as
`C:\...\Serum.vst3` — `is_serum1`/`is_serum2` in `src/serum.rs` already handle
both separators.)

### 2.6 Top-level cid 52 — 16-byte plugin UID

Per-plugin unique 16 bytes; Xfer embeds ASCII in theirs (Kilohearts/iZotope
use Steinberg-style random FUID bytes, e.g. kHs Filter
`CB 55 3F 79 50 00 A6 49 00 BF 8D AB 00 B4 EF A0`):

```
Serum : 58 54 53 56 73 66 73 58 65 72 75 6D 00 00 00 00   "XTSVsfsXerum"
Serum2 : 58 45 53 56 73 66 73 50 65 72 75 6D 20 32 00 00   "XESVsfsPerum 2"
OTT     : 58 54 53 56 54 66 6F 54 74 74 00 00 00 00 00 00   "XTSVTfoTtt"
```

Most plausibly the VST3 processor class ID (FUID) FL caches for matching the
plugin slot to the plugin — note it is **not** inside the cid-53 wrapper (see
§3.1: FL's wrapper cid-1 record is *not* the class id, despite what the
comments in `src/serum.rs` imply). It must be swapped when converting S1→S2.

## 3. cid 53 — FL's VST3 wrapper state

Layout: `u32 LE prologue` then records `[u32 LE cid][u64 LE size][data]`
(matches `fl_vst3_wrapper_cid3` in `src/serum.rs`, which scans start offsets
0..=8; empirically the records start at offset 4, i.e. prologue is exactly the
first u32).

```
prologue = 01 00 00 00   (u32 LE 1) — identical for S1 and S2
```

### 3.1 Inner record inventory

| cid | Serum (inst #0)               | Serum2                          |
|-----|---------------------------------|----------------------------------|
| 1   | 64 bytes                        | 64 bytes                         |
| 3   | 6,561  (zlib chunk, §3.3)       | 33,751 (XferJson processor, §4)  |
| 2   | — (absent)                      | 3,340  (XferJson controller, §4) |
| 4   | 9,592  (param list, §3.4)       | 10,496 (param list, §3.4)        |

Order as saved: `1, 3, 4` (S1) and `1, 3, 2, 4` (S2). The extra **inner cid 2
(controller state) is the main structural difference**: it is present for
Serum2 (and iZotope plugins — Ozone Imager 2 also saves `[1, 3, 2, 4]` with
`size(cid2) == size(cid3)` = 1415) and absent for Serum, whose controller
component has no state. S1's inner cid 4 (9,592) and S2's (10,496) differ only
because the parameter counts differ (§3.4).

### 3.2 Inner cid 1 (64 bytes) — identical for S1 and S2, NOT a class id

```
01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

Only byte 0 is nonzero. Identical across all 54 VST3-wrapped instances in the
samples (Serum, Serum2, Vital, Kilohearts, OTT, iZotope...). Treat it as a
fixed `u32 version = 1` + 60 zero bytes; the repo comment calling it "the VST3
class id" is inaccurate — the per-plugin class id appears in **top-level cid
52** instead. No change needed S1→S2.

### 3.3 Serum inner cid 3 — zlib streams + u32 LE trailer

Byte-exact for instance #0: 6,561 bytes = zlib stream (3,211 compressed →
**172,736 decompressed** = `SERUM1_STATE_SIZE`) + second zlib stream (3,346
compressed → 16,384 decompressed, embedded wavetable/noise) + 4-byte trailer:

```
78 01 ED 9D 0B 70 54 D5 19 C7 6F C2 ...        (stream 0, 3211 bytes)
78 01 ...                                      (stream 1, 3346 bytes)
8B 0C 00 00                                    (u32 LE 3211 = compressed size of stream 0)
```

Matches `analyze_chunk` in `src/serum.rs`; the decompressed stream 0 carries
the fixed-offset preset name (0x4972) / version f32 (0x4994) / author (0x49A0)
/ category (0x49D0) metadata — see `serum-fxp-format.md`.

### 3.4 Inner cid 4 — parameter-index list

Layout: `[u32 LE count][count × u32 LE param id]` (size = 4 + 4·count).

- Serum: count = 0x95D = **2,397**; list ≈ `0,1,2,...,2396` with a handful of
  vendor-range ids (e.g. `707Fxxxx`-class values around entry 315).
- Serum2: count = 0xA3F = **2,623**; list starts `0..8`, skips 9, continues
  `10,11,...`, includes dense vendor ranges (`0xF4240+`, `51 50 00 00`+ etc.).
- Other plugins: e.g. Ozone Imager 2 count = 4; Vital count = 2,855.

So cid 4 is FL's saved-parameter id list and is plugin-version-specific: copy
it from a genuine Serum2 state when converting (do not try to reuse S1's).

## 4. Serum2 XferJson records (inner cid 3 = processor, inner cid 2 = controller)

Byte-exact container, identical shape for both records:

```
"XferJson" 00                       (9 bytes incl. NUL)
u64 LE json_len                     (cid3: 0xB7 = 183;  cid2: 0x113 = 275)
<json_len bytes: UTF-8 JSON, no NUL> (metadata header)
u32 LE uncompressed_size            (cid3: 454879; cid2: 11570)
u32 LE 02                           (constant 2 in both samples; semantics unknown)
<single zstd frame (magic 28 B5 2F FD), decompressing to uncompressed_size>
```

Verified header consistency: exactly one zstd frame, decompressed length ==
declared `uncompressed_size` (454,879 and 11,570).

JSON headers, verbatim (complete, these are the entire JSON):

```
cid3: {"component":"processor","hash":"56a4a7cd2d58b933d65be68878ca06ec",
       "product":"Serum2","productVersion":"2.0.22","url":"https://xferrecords.com/",
       "vendor":"Xfer Records","version":8.0}

cid2: {"component":"controller","hash":"06efd53571617f74445b1037e0ce055a",
       "presetAuthor":"Kagi","presetDescription":"kagimusic.com",
       "presetName":"Release Cut Piano","product":"Serum2","productVersion":"2.0.22",
       "url":"https://xferrecords.com/","vendor":"Xfer Records","version":8.0}
```

**`hash` = MD5 of the compressed zstd frame** (the bytes starting at the
`28 B5 2F FD` magic, i.e. everything after the 8 header words shown above).
Verified for both records: md5(frame) == the JSON `hash`. Any synthesized or
recompressed state must recompute it (matches the note in
`serum2-dynamic-verification.md`).

The zstd-decompressed bodies are **not JSON**; they are Xfer's own binary
serialization (same family as `.SerumPreset` — see `serum-fxp-format.md`
§"Serum2"): 7-bit-ASCII length-prefixed keys/values, marker bytes `A0`/`A1`,
and floats encoded as marker `FA` + big-endian f32 (tail of both bodies:
`curl x 18 "https://xferrecords.com/" fvendor l"Xfer Records" gversion
FA 41 00 00 00` = f32 BE 8.0, matching JSON `version:8.0`). The processor body
(454,879 B) is the full parameter dump (`kplainParams ...`); the controller
body (11,570 B) additionally carries the preset path
`.../Serum 2 Presets/Presets/User/Release Cut Piano.SerumPreset`.

**Serum2 preset identity lives inside the cid-2 XferJson record
(JSON `presetName` + decompressed controller body), not in any separate FLP
event.** For Serum the equivalent lives inside the zlib stream at offset
0x4972 (see §3.3).

## 5. What FL stores OUTSIDE event 213 (Serum-related string scan)

Scanned every event of serina1.flp (6,875 events, 46 of them event 213) for
`Serum` in both encodings. Hits outside event 213:

| event | stream offset | payload (UTF-16LE, NUL-terminated) |
|-------|---------------|------------------------------------|
| 203 (channel name) | 0x2E51 | `Serum`     |
| 203 | 0x7236 | `Serum #2` |
| 203 | 0xE67E | `Serum 2`   ← the real Serum2 channel |
| 203 | 0x20B88 | `Serum #3` |
| 203 | 0x25CF9 | `Serum #4` |
| 203 | 0x2B873 | `Serum #5` |
| 204 (FX track name) | 0xFCAE5 | `Serum` |
| 204 | 0xFCEF5 | `Serum #2` |
| 204 | 0xFF637 | `Serum 2`   |
| 204 | 0x104BED | `Serum #3` |
| 204 | 0x104FAE | `Serum #4` |
| 204 | 0x1055A7 | `Serum #4` |
| 204 | 0x10D08C | `Serum #5` |

- Channel/track names are **UTF-16LE with a 16-bit NUL terminator**
  (`53 00 65 00 72 00 75 00 6D 00 00 00`) — different encoding from the
  PluginParams strings (§2.5).
- These are pure display labels: they are user-editable, independent of the
  loaded plugin (the S1 channels are named `Serum #N`, the Serum2 channel
  `Serum 2`, and a `Vital` channel is named `Vital`), and FL never reads the
  plugin from them.
- **No other event type contains `Serum` bytes.** There is no event 205 and no
  separate "plugin display name"/"preset name" event; preset names live inside
  the plugin state (§3.3/§4). Events 49/50/212/215 etc. near the instances are
  binary channel/mixer data with no Serum strings.

**Conclusion: swapping Serum → Serum2 requires changing only the event-213
payload (plus lengths). No other events must change.**

## 6. Rewrite recipe: Serum event 213 → Serum2 event 213

Given the S1 instance's 213 payload and a genuine Serum2 XferJson state
(ideally lifted from a real Serum2 instance in some FLP, as done for this
doc), the deltas are:

Keep (identical bytes):
- `u32 version = 12` and records `cid 2, cid 30, cid 32, cid 50` (§2.2);
- top-level `cid 56` (`Xfer Records`);
- wrapper `prologue = 01 00 00 00` and inner `cid 1` (64 B, §3.2).

Change:

| # | where | from | to |
|---|-------|------|----|
| 1 | top cid 1, byte offset 8 (u32 LE) | `0C 00 00 00` | `01 00 00 00` (real Serum2 value; semantics unknown, §2.3) |
| 2 | top cid 52 (16 B) | `XTSVsfsXerum…` | `XESVsfsPerum 2…` (§2.6 hex) |
| 3 | top cid 54 | `Serum` (5 B) | `Serum2` (6 B) |
| 4 | top cid 55 | `…/Serum.vst3` (39 B) | `…/Serum2.vst3` (40 B) — match the new cid-52 plugin |
| 5 | top cid 53, record size (u64 LE) | 16,257 | size of the new Serum2 wrapper state |
| 6 | top cid 53 payload | S1 wrapper `[prol=1][cid1 64][cid3 zlib chunk][cid4 S1]` | `[prol=1][cid1 64][cid3 XferJson processor][cid2 XferJson controller][cid4 S2]` |
| 7 | inner cid 3 | zlib chunk (streams + 4-byte trailer) | XferJson processor record (§4): recompress state to one zstd frame, header = `u32 uncomp_size, u32 2`, JSON with `component=processor`, `hash = MD5(frame)`, `product=Serum2`, `productVersion`, `vendor`, `url`, `version:8.0` |
| 8 | inner cid 2 | — (absent for S1) | insert XferJson controller record **after** cid 3 (JSON `component=controller` + `presetName/presetAuthor/presetDescription`, `hash = MD5(frame)`) |
| 9 | inner cid 4 | S1's 2,397-entry list | Serum2's list (2,623 entries for 2.0.22) — copy from a genuine Serum2 instance, do not synthesize |

Then re-frame the event:
- event id byte `0xD5` + varint of the new payload length (3-byte varints as
  in §1 for these sizes);
- update the containing record's u64 size (cid 53) — offset arithmetic: the
  u64 size field of the top-level cid-53 record sits immediately before the
  wrapper payload;
- update the `FLdt` u32 LE chunk length at file offset 12 (uncompressed FLdt
  means a plain splice + length fix works; see §1).

Everything else in the FLP (channel/track name events 203/204, patterns,
mixer events) stays untouched (§5).

## 7. Sample-file provenance / cross-checks

| file | FLdt compressed | trailing bytes | events | event-213 count | Serum | Serum2 |
|------|-----------------|----------------|--------|-----------------|---------|---------|
| `serina1/serina1.flp` | no (raw events) | 0 | 6,875 | 46 | **5** | **1** |
| `serina4/serina4.flp` | no | 0 | 6,211 | 69 | 7 | 1 |
| `serina3/Project_7.flp` | no | 0 | 6,622 | 38 | 2 | 0 |

serina1 counts (5 × Serum + 1 × Serum2) agree with the extraction CLI's
report. Serum instance stream offsets in serina1.flp: `0x2E6B, 0x7256,
0x20BA8, 0x25D19, 0x2B893`; the Serum2 instance: `0xE69C`. The one real Serum2
instance sits among 40 other VST3/VST2 plugin instances; its plugin state
(respectively 33,751 + 3,340 + 10,496 bytes) is the calibration source for §4.

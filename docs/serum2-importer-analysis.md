# Serum2 (2.0.23) — Static RE notes: "Load Serum 1 preset (.fxp)" validation path

Target: `C:\Program Files\Common Files\VST3\Serum2.vst3\Contents\x86_64-win\Serum2.vst3`
SHA256: `9293EB90FC9FC890FD2505272ABD6172CEE5BD32B1FB20BE22531810702BF9B3`
Date of analysis: 2026-09-11. Analysis: static only (no execution of the plugin).

## 1. Tools used

- Python 3.14 + `pefile` 2024.8.26 + `capstone` 5.0.9 (`pip install --break-system-packages --user`)
- Custom scripts (in `C:\Users\cabbage\AppData\Local\Temp\opencode\work\`): full-file ASCII/UTF-16 string scan with RVA mapping; .pdata-based function enumeration (32,927 functions); linear capstone disassembly of every function indexing (a) rip-relative refs into `.rdata`/`.data` strings, (b) magic immediates; rel32 call xref scan.
- Cross-validation data files on disk (read-only): `C:\Users\cabbage\Documents\Xfer\Serum 2 Presets\Presets\Factory\*.SerumPreset` (these are "XferJson" JSON files, NOT CcnK — so the CcnK path analyzed here is exclusively the Serum fxp path).

## 2. PE facts

- Machine 0x8664 (x64), Magic 0x20B (PE32+), Characteristics 0x2022 → **64-bit DLL**
- ImageBase 0x180000000, exports: `InitDll`, `ExitDll`, `GetPluginFactory` (VST3)
- Sections: `.text` VA 0x1000 VSZ 0x9E42E6 (entropy 6.43 — NOT packed), `.rdata` 0x9E6000, `.data` 0x10DE000, `.pdata` 0x2061000, ...
- Build string found: "Version 2.0.23"

## 3. Relevant strings (RVA → VA = 0x180000000+RVA)

| RVA | String | Role |
|---|---|---|
| 0x00F9A3E0 | `CcnK` | magic constant string (only occurrence; referenced via `lea`+memcmp) |
| 0x00F9C8D2 | `Syl1` | Sylenth1 magic at fxp+0x10 (error branch) |
| 0x00F8318B | `Unable to Load Preset` | error title |
| 0x00F9E85C | `: It Is Too Small to be a Valid fxp File.` | size < 0x3D |
| 0x00FA25B4 | `: It is Made for Sylenth, Not Serum!` | Syl1 detected |
| 0x00FA258B | `: It Is Not a Valid Serum Preset!` | generic invalid |
| 0x00F91289 | `This Preset was made with a newer version of Serum! Update Serum on www.xferrecords.com` | version > max |
| 0x00FA26BC | `WARNING - This Patch is Old!!!` (+ volume text 0xF8C666/0xF8C696) | old-version warning (non-fatal) |
| 0x00F944B0 / 0x00F94510 | `Serum 1 Preset File` / `Serum 1 FX Rack File` | file-dialog filter names |
| 0x00F830C2 | `oldSerum1Preset` | JSON flag for old chunk versions |
| 0x00F90BC8 / 0x00F90BDB | `serum1ChunkVersion` / `serum1Version` | JSON metadata out |
| 0x00F9B9E9 `LFO8`/`LFO9`, 0xF84CE3 `plainParams`, 0xF93C12 `kParamType` | state→JSON conversion keys |
| 0x00F9C9B5 | `ATTACH DATABASE '%q' AS S1` | S1 database attach during metadata import |

Notably: **no "FPCh"/"FBCh"/"FxBk"/"FxCk"/"PTCH" strings or immediates exist anywhere in .text/.rdata** (byte-scanned LE and BE). fxMagic is never validated.

## 4. Call chain (RVAs)

```
load_entry  0xC26C0–0xC30E4          ("load preset file" dispatcher; called from 0xAD940, 0xC3FE0)
  ├─ has_ext(path,"fxp")  0x257000   (case-insensitive; '.' must not be first char of component)
  ├─ read_file             0x25A3E0
  ├─ validator             0x4D9BB0–0x4D9C4D   returns V (>0 ok, 0/-2/-3 fail codes)
  └─ s1state_load          0x4DABC0–0x4E61CA   (rcx=out, rdx=ctrl, r8=file+0x3C, r9d=V,
                                                 stack5 = (file[0x13]=='Y'), stack6 = filename)

Second, equivalent parser (preset-database / metadata import path):
  0x511A30 → 0x4DA2F0–0x4DAA6B  ("parse Serum fxp": same header checks, own 172736-byte state buffer,
                                  version migrations, then SQLite S1 metadata extraction)
```

## 5. Disassembly — validator (0x4D9BB0)

```asm
0x4D9BC5  or   r9b, r8b                  ; null checks
0x4D9BC8  jne  ret0
0x4D9BCA  mov  eax, 0fffffffeh           ; -2
0x4D9BCF  cmp  rdx, 3Dh                  ; filesize >= 0x3D (61)
0x4D9BD3  jb   ret                       ; -> -2 ("too small")
0x4D9BD5  lea  rdx, ["CcnK"] ; r8d=4
0x4D9BE5  call memcmp                    ; data[0..4) == "CcnK"
0x4D9BF1  test edx,edx ; jne ret0 (mismatch -> 0)
0x4D9BFB  cmp  byte [rsi+10h], 58h       ; 'X'
0x4D9C01  cmp  byte [rsi+11h], 66h       ; 'f'
0x4D9C07  cmp  byte [rsi+12h], 73h       ; 's'    (fxp+0x10..0x12 == "Xfs")
0x4D9C0D  mov  ecx, [rsi+38h] ; bswap ecx
0x4D9C14  cmp  ecx, 4000001h ; cmovb eax,ecx   ; V in [1, 0x4000000] -> return V
          ; else fallthrough to "Syl1" memcmp at data+0x10 -> -3, or 0/-1
```

## 6. Disassembly — load_entry fxp branch (0xC26C0)

```asm
0xC280D  lea rdx, ["fxp"]; call 0x257000     ; ends_with .fxp (case-insensitive)
0xC281E  je  .SerumPreset_branch             ; else XferJson path ("Presets\User\DefaultFX.SerumPreset")
0xC282B  call 0x25A3E0                       ; read whole file -> std::string
0xC283B  call 0x4D9BB0                       ; validator(data, size)
0xC2845  jle  fail                           ; v must be > 0
0xC286A  lea r8, [rax+3Ch]                   ; state buffer = file + 0x3C
0xC286E  cmp byte [rax+13h], 59h             ; 'Y' flag -> passed as 5th arg (enables extra
0xC2877  sete byte [rsp+20h]                 ;   osc/"Audio In" handling; NOT required)
0xC2886  mov r9d, r14d                       ; r9d = V  (buffer length!)
0xC2889  call 0x4DABC0
0xC288E  test al,al ; je fail                ; -> ": It Is Not a Valid Serum Preset!"
fail codes: -2 -> "Too Small...", -3 -> "Made for Sylenth..."
```

## 7. Disassembly — s1state_load prologue (0x4DABC0), the chunk trailer logic

```asm
0x4DAC4A  cmp  r9d, 28h          ; V >= 0x28 (40)
0x4DAC4E  jl   fail
0x4DAC56  movzx ecx, byte [r8+rax-1]   ; last byte of buf (buf = file+0x3C, len = V)
0x4DAC5C  cmp  cl, 3 ; jbe ok          ; top byte of trailer <= 3  (N <= 0x03FFFFFF)
0x4DAC6E  movzx eax, word [r8+rax-4]   ; trailer = LE32(buf + V - 4) = N
          ;           = LE32(file + 0x38 + V)
0x4DAC96  mov  [rbp+26860h], 2A2C0h    ; 172736 capacity
0x4DACDB  call memset(state, 0, 172736)
0x4DAD05  call checked_copy(dst=state, &cap=172736, src=buf, count=N)   ; 0x552a20
0x4DAD0A  test eax,eax; sets cl        ; eax<0 and eax != -5 -> fail  (-5 = truncation, tolerated)
```

Layout model (all offsets in file):
```
0x00  "CcnK"                     4   memcmp'd
0x04  chunkByteSize (BE)         4   NEVER CHECKED
0x08  fxMagic ("FPCh" etc.)      4   NEVER CHECKED (no FPCh/FBCh/FxBk constant exists in binary)
0x0C  fxVersion (BE)             4   NEVER CHECKED
0x10  "Xfs"                      3   byte-compared (fxProgramID field)
0x13  flag byte                  1   'Y' (0x59) optional; enables Audio-In handling
0x14  numParams (BE)             4   NEVER CHECKED
0x18  prgName[28]                28  NEVER READ on this path (name comes from filename)
0x34  chunkSize (BE)             4   NEVER CHECKED
0x38  V = BE32                   4   offset from 0x38 to trailer; 1 <= V <= 0x4000000;
                                     V + 0x3C <= filesize; V >= 0x28
0x3C  state blob, N bytes            copied into zeroed 172736-byte buffer (truncation tolerated)
...   appended data (wavetable), W = V - N - 4 bytes (optional, may be 0)
0x38+V  trailer = LE32 = N       4   N >= 1; N top byte <= 3 (N <= 0x03FFFFFF);
                                     import path (0x4DA2F0) additionally requires N <= 0xFFFFF
```
Standard no-wavetable preset: N = 172736 (0x2A2C0), V = 172740 (0x2A2C4), file size = 0x3C+V = 172800 (0x2A300). Wavetable-embedded: W > 0 between state and trailer; file grows by W.

## 8. Version checks (after state copy)

Version float = `float32` at **state + 0x4994** (file offset 0x3C+0x4994 = 0x49D0; chunk-relative 0x4998). Both implementations:

```asm
; 0x4DB45B (s1state_load) / 0x4DA4D7 (import parser)
movss xmm14, [state+4994h]
movss xmm0, [0.002]  ; 0xA55908
ucomiss xmm0, xmm14 ; ja  fail            ; version >= 0.002  (else silent fail / "Not a Valid")
movss xmm0, [0.999] ; 0xA5590C
ucomiss xmm14, xmm0 ; ja  newer_version   ; version <= 0.999  ("newer version of Serum!" -> fail)
; non-fatal:
;  version < 0.009 (0xA55934) -> "WARNING - This Patch is Old!!!" dialog, continues
;  version < 0.149 (0xA559B8) -> JSON flag oldSerum1Preset=true, detuneFactor path otherwise
;  version < 0.01  -> legacy param-table migration loop (0x4DB533)
```
Version-float → Serum-version mapping table at 0xA559F0–0xA55A74 (e.g. 0.131→1.023, 0.134→1.032, 0.162/0.163 = newest entries; >0.999 never valid). Metadata written: `serum1ChunkVersion` (raw float), `serum1Version` (mapped), `productVersion` "Version 2.0.23", `schema_version` 9.0 (0x4022000000000000).

Because the 172736-byte state buffer is zero-filled before the copy, **N < 0x4998 ⇒ version reads 0.0 ⇒ fail** (0.002 check).

## 9. Other observations

- 0x552a20 = bounds-checked copy helper (dst, &dstCapacity, src, count); returns 0 ok, -5 = truncated (tolerated), other negative = fail. Memset(0) first means short N yields zeroed tail.
- 0x9e3480 = memcmp; 0x257000 = case-insensitive ".fxp" extension test (also rejects "/.fxp" style dotless names).
- The appended wavetable region W = V-N-4 bytes is copied (0x4E0BE8–0x4E0D5E) and later parsed as embedded table data; W = 0 is fine.
- "Serum 1 FX Rack File" (.fxp for FX racks) is a separate path (filter strings 0xF944B0/0xF94510, loaders around 0xC8410/0xC9280); same family of checks expected but not fully analyzed.
- Bank formats ("FBCh"/"FxBk") are never referenced anywhere in the binary — only single-program fxp.
- 'Syl1' at fxp+0x10 yields the dedicated "Made for Sylenth" error (validator 0x4D9C23).

## 10. Requirements for a Serum2-loadable Serum fxp (FINAL)

Every check the loader performs, in order:

1. **Filename** ends with `.fxp` (case-insensitive), '.' preceded by a non-separator char. [certain]
2. **filesize >= 0x3D (61)** bytes. [certain]
3. **file[0..4) == "CcnK"** (memcmp). [certain]
4. **file[0x10..0x13) == "Xfs"** (three byte compares; 4th byte free). [certain]
5. **V = BE32(file+0x38)** must satisfy **1 <= V <= 0x4000000**, **V >= 0x28**, and **V + 0x3C <= filesize**. [certain]
6. **Trailer N = LE32(file + 0x38 + V)** (last 4 bytes of the chunk region): **top byte <= 3** (N <= 0x03FFFFFF), **N >= 1**; database-import parser additionally bounds **1 <= N <= 0xFFFFF**; practical requirement **N == 172736** for the fixed state buffer. [certain for bounds; buffer size inference high-confidence]
7. **State = file[0x3C .. 0x3C+N)** copied into a 172736-byte zeroed buffer (truncation tolerated); therefore **N >= 0x4998** so the version float is real data, and in practice **N = 172736**. [certain]
8. **version float32 at state+0x4994 (file offset 0x49D0)** must be in **[0.002, 0.999]**. Use a real Serum chunk version (e.g. 0.163, 0.162) to land in the newest migration bucket; >= 0.149 avoids the `oldSerum1Preset` flag; >= 0.009 avoids the "Patch is Old" warning. [certain on bounds; recommendation-level on values]
9. Optional: file[0x13] == 'Y' enables extra Audio-In osc handling — **not required**. [certain]
10. NOT checked anywhere: chunkByteSize@0x04, fxMagic@0x08 ("FPCh" conventional but ignored), fxVersion@0x0C, numParams@0x14, prgName@0x18 (28 bytes), chunkSize@0x34. Bytes after 0x3C+V in the file are ignored. [certain — full-binary scan found no other references]

Canonical minimal file (no embedded wavetable):
```
size  = 0x38 + 4 + 172736 + 4 = 172800 (0x2A300)
V     = 0x2A2C4 (BE32 @0x38)
N     = 0x2A2C0 (LE32 trailer, last 4 bytes of file)
state = 172736 bytes; float32 @ state+0x4994 = e.g. 0.163f
```

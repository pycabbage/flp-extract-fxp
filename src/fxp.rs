//! Building and validating Serum `.fxp` preset files.
//!
//! File layout (all multi-byte header fields big-endian unless noted),
//! verified against 25 genuine Serum presets (2015-2026) and against the
//! Serum2 importer disassembly (Serum2.vst3 2.0.23, see README):
//!
//! ```text
//! 0x00  "CcnK"               chunk magic
//! 0x04  u32    byteSize      == whole file length
//! 0x08  "FPCh"               chunk preset magic
//! 0x0C  u32    version       == 1
//! 0x10  "XfsX"               fxProgramID (Serum's fourCC)
//! 0x14  u32    fxVersion     == 1
//! 0x18  u32    numParams     == 1
//! 0x1C  [28]   prgName       preset name, NUL padded
//! 0x38  u32    chunkSize     == file length - 60 (includes the trailer)
//! 0x3C  chunk: [zlib(preset state 172736 B)][zlib(wavetable)]... [u32 LE N]
//! ```
//!
//! The trailing `N` (little-endian) is the compressed size of the first zlib
//! stream; Serum2's importer uses it to slice the state out of the chunk.

pub const FXP_HEADER_LEN: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Serum2 refuses to import the file.
    Fatal,
    /// The file loads, but deviates from what genuine Serum writes.
    Warning,
}

#[derive(Debug)]
pub struct ValidationIssue {
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    fn fatal(&mut self, msg: impl Into<String>) {
        self.issues.push(ValidationIssue {
            severity: Severity::Fatal,
            message: msg.into(),
        });
    }
    fn warn(&mut self, msg: impl Into<String>) {
        self.issues.push(ValidationIssue {
            severity: Severity::Warning,
            message: msg.into(),
        });
    }
    pub fn is_ok(&self) -> bool {
        !self.issues.iter().any(|i| i.severity == Severity::Fatal)
    }
    pub fn fatals(&self) -> impl Iterator<Item = &str> {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Fatal)
            .map(|i| i.message.as_str())
    }
    pub fn warnings(&self) -> impl Iterator<Item = &str> {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .map(|i| i.message.as_str())
    }
}

/// Assemble a complete Serum `.fxp` from a chunk (zlib streams + trailer).
pub fn build_fxp(chunk: &[u8], preset_name: &str) -> Vec<u8> {
    let total = FXP_HEADER_LEN + chunk.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"CcnK");
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(b"FPCh");
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(b"XfsX");
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    // 28-byte NUL-padded preset name, cut on a char boundary.
    let mut name = [0u8; 28];
    let bytes = preset_name.as_bytes();
    let mut take = bytes.len().min(27);
    while take > 0 && !std::str::from_utf8(&bytes[..take]).is_ok() {
        take -= 1;
    }
    name[..take].copy_from_slice(&bytes[..take]);
    out.extend_from_slice(&name);
    out.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk);
    debug_assert_eq!(out.len(), total);
    out
}

/// Validate an fxp against the checks the Serum2 importer performs.
///
/// The rule set below mirrors the disassembled import path of
/// Serum2.vst3 2.0.23 (`load_entry` -> validator -> `s1state_load`); a file
/// with no [`Severity::Fatal`] issue is accepted by that code path.
pub fn validate_fxp(data: &[u8]) -> ValidationReport {
    let mut r = ValidationReport::default();
    if !check_importer_rules(data, &mut r) {
        return r;
    }
    check_structure(data, &mut r);
    r
}

/// Rules reimplemented from the Serum2 importer. Returns `false` when the
/// file is too malformed to continue (the report already carries the
/// fatals); the structural checks are skipped in that case.
fn check_importer_rules(data: &[u8], r: &mut ValidationReport) -> bool {
    if data.len() < 0x3D {
        r.fatal(format!("file too small ({} bytes, need >= 61)", data.len()));
        return false;
    }
    if &data[0..4] != b"CcnK" {
        r.fatal("missing \"CcnK\" magic at offset 0");
        return false;
    }
    if &data[0x10..0x13] != b"Xfs" {
        r.fatal(format!(
            "fxProgramID at 0x10 is {:?}, expected \"XfsX\"",
            &data[0x10..0x14]
        ));
    }
    let v = u32::from_be_bytes(data[0x38..0x3C].try_into().unwrap());
    if v < 0x28 {
        r.fatal(format!("chunkSize {v} below the minimum of 40"));
    }
    if v > 0x4000_0000 {
        r.fatal(format!("chunkSize {v} above the maximum of 0x40000000"));
    }
    if v as usize + 0x3C > data.len() {
        r.fatal(format!(
            "chunkSize {v} overruns the file ({} bytes)",
            data.len()
        ));
    }
    if r.is_ok() {
        let n = u32::from_le_bytes(
            data[0x38 + v as usize..0x38 + v as usize + 4]
                .try_into()
                .unwrap(),
        );
        if n < 1 {
            r.fatal("trailer state length is 0");
        }
        if n > 0xF_FFFF {
            r.fatal(format!(
                "trailer state length {n} above the import cap 0xFFFFF"
            ));
        }
        if n >> 24 > 3 {
            r.fatal("trailer state length has an out-of-range top byte");
        }
        if r.is_ok() {
            let blob = &data[0x3C..(0x3C + n as usize).min(data.len())];
            // Serum presets store the state zlib-compressed; the loader
            // also tolerates raw (uncompressed) state bytes.
            let (state, _consumed): (Vec<u8>, usize) = if blob.first() == Some(&0x78) {
                match crate::zlibio::inflate(blob, crate::zlibio::MAX_STREAM) {
                    Err(e) => {
                        r.fatal(format!("state zlib stream does not inflate: {e}"));
                        (Vec::new(), 0)
                    }
                    Ok((s, c)) => {
                        if c != n as usize {
                            r.warn(format!("state stream consumes {c} of {n} trailer bytes"));
                        }
                        (s, c)
                    }
                }
            } else {
                (blob.to_vec(), blob.len())
            };
            if !state.is_empty() || n == 0 {
                if state.len() > crate::serum::SERUM1_STATE_SIZE {
                    r.fatal(format!(
                        "decompressed state is {} bytes (max {})",
                        state.len(),
                        crate::serum::SERUM1_STATE_SIZE
                    ));
                } else if state.len() < crate::serum::SERUM1_STATE_SIZE {
                    r.warn(format!(
                        "decompressed state is {} bytes; Serum2 zero-pads to {}",
                        state.len(),
                        crate::serum::SERUM1_STATE_SIZE
                    ));
                }
                if state.len() >= crate::serum::OFF_VERSION_F32 + 4 {
                    let ver = f32::from_le_bytes(
                        state[crate::serum::OFF_VERSION_F32..crate::serum::OFF_VERSION_F32 + 4]
                            .try_into()
                            .unwrap(),
                    );
                    if !(0.002..=0.999).contains(&ver) {
                        r.fatal(format!(
                            "preset version float at state+0x4994 is {ver}, outside [0.002, 0.999]"
                        ));
                    } else if ver < 0.009 {
                        r.warn(format!(
                            "preset version {ver} triggers Serum2's old-patch warning"
                        ));
                    } else if ver < 0.149 {
                        r.warn(format!(
                            "preset version {ver} sets Serum2's oldSerum1Preset compatibility flag"
                        ));
                    }
                } else {
                    r.warn("state too short to contain a version float");
                }
            }
        }
    }
    true
}

/// Extra structural checks (how genuine Serum files look).
fn check_structure(data: &[u8], r: &mut ValidationReport) {
    let byte_size = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
    if byte_size != data.len() {
        r.warn(format!(
            "byteSize {byte_size} != file length {}",
            data.len()
        ));
    }
    if &data[8..12] != b"FPCh" {
        r.warn(format!(
            "fxMagic is {:?}, genuine files use \"FPCh\"",
            &data[8..12]
        ));
    }
    let v2 = u32::from_be_bytes(data[0x38..0x3C].try_into().unwrap()) as usize;
    if 0x3C + v2 != data.len() {
        r.warn(format!(
            "file has {} bytes but header implies {}",
            data.len(),
            0x3C + v2
        ));
    }
}

/// Validate a bare chunk (zlib streams + trailer) against the Serum2 rules
/// by round-tripping it through the fxp container.
pub fn validate_chunk_report(chunk: &[u8]) -> ValidationReport {
    validate_fxp(&build_fxp(chunk, ""))
}

/// One metadata field to rewrite in a Serum preset.
///
/// `Name` goes to BOTH the fxp header's `prgName` (28 B, doc §4 #1) and the
/// state blob's 32-byte field at 0x4972 (doc §4 #2); `Author`/`Category`
/// live only in the state blob (48 B each at 0x49A0 / 0x49D0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchField {
    Name(String),
    Author(String),
    Category(String),
}

/// Encode `value` into a NUL-padded field of `len` bytes, truncating on a
/// UTF-8 char boundary and always keeping at least one NUL terminator
/// (same truncation rule as [`build_fxp`]).
fn encode_field(value: &str, len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    let bytes = value.as_bytes();
    let mut take = bytes.len().min(len - 1);
    while take > 0 && std::str::from_utf8(&bytes[..take]).is_err() {
        take -= 1;
    }
    out[..take].copy_from_slice(&bytes[..take]);
    out
}

/// zlib-compress at level 1 — what genuine Serum writes (doc §5, `78 01`).
fn deflate_zlib_level1(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(1));
    enc.write_all(data)
        .map_err(|e| format!("zlib compression failed: {e}"))?;
    enc.finish()
        .map_err(|e| format!("zlib compression failed: {e}"))
}

/// Rewrite metadata inside a bare Serum chunk (`[streams...][u32 LE trailer]`,
/// the fxp `chunk` region): inflate stream 0, overwrite the target fields,
/// recompress at zlib level 1, keep every other stream verbatim and rebuild
/// the trailer as the new stream-0 length. `Name` here only touches the
/// 32-byte state field (no fxp header exists in a bare chunk).
pub fn patch_chunk_fields(chunk: &[u8], patches: &[PatchField]) -> Result<Vec<u8>, String> {
    if patches.is_empty() {
        return Ok(chunk.to_vec());
    }
    if chunk.len() < 12 {
        return Err("chunk too small to hold a state stream and trailer".into());
    }
    let trailer = u32::from_le_bytes(chunk[chunk.len() - 4..].try_into().unwrap()) as usize;
    if trailer < 1 || trailer > chunk.len() - 4 {
        return Err(format!(
            "chunk trailer {trailer} does not describe a stream-0 length"
        ));
    }
    if chunk[0] != 0x78 {
        return Err("chunk does not start with a zlib stream".into());
    }
    let (mut state, consumed) =
        crate::zlibio::inflate(&chunk[..trailer], crate::zlibio::MAX_STREAM)?;
    if consumed != trailer {
        return Err(format!(
            "chunk trailer ({trailer}) does not match stream 0's compressed size ({consumed})"
        ));
    }
    for patch in patches {
        let (off, flen, bytes): (usize, usize, Vec<u8>) = match patch {
            PatchField::Name(s) => (crate::serum::OFF_PRESET_NAME, 32, encode_field(s, 32)),
            PatchField::Author(s) => (crate::serum::OFF_AUTHOR, 48, encode_field(s, 48)),
            PatchField::Category(s) => (crate::serum::OFF_CATEGORY, 48, encode_field(s, 48)),
        };
        if off + flen > state.len() {
            return Err(format!(
                "decompressed state is {} bytes; the field at {off:#x}+{flen} does not fit",
                state.len()
            ));
        }
        state[off..off + flen].copy_from_slice(&bytes);
    }
    let z0 = deflate_zlib_level1(&state)?;
    let mut out = Vec::with_capacity(z0.len() + chunk.len() - trailer);
    out.extend_from_slice(&z0);
    out.extend_from_slice(&chunk[trailer..chunk.len() - 4]);
    out.extend_from_slice(&(z0.len() as u32).to_le_bytes());
    Ok(out)
}

/// Patch metadata in a complete `.fxp` file, in place in the buffer.
///
/// The container is validated first (errors on malformed input). A
/// [`PatchField::Name`] is mirrored into the header's 28-byte `prgName` at
/// 0x1C (truncated at 27 chars on a UTF-8 boundary, NUL-padded) AND into the
/// state blob (see [`patch_chunk_fields`]); every patch recompresses stream 0
/// at zlib level 1 and rebuilds the chunk + trailer, then fixes `chunkSize`
/// (0x38, BE) and `byteSize` (0x04, BE = file length). No other bytes are
/// touched.
pub fn patch_metadata(fxp: &mut Vec<u8>, patches: &[PatchField]) -> Result<(), String> {
    if patches.is_empty() {
        return Ok(());
    }
    if fxp.len() < 0x3D {
        return Err(format!("file too small ({} bytes, need >= 61)", fxp.len()));
    }
    if &fxp[0..4] != b"CcnK" {
        return Err("missing \"CcnK\" magic at offset 0".into());
    }
    let v = u32::from_be_bytes(fxp[0x38..0x3C].try_into().unwrap()) as usize;
    if v < 0x28 || 0x38 + v + 4 > fxp.len() {
        return Err(format!(
            "chunkSize {v} does not describe the file ({} bytes)",
            fxp.len()
        ));
    }
    // Header prgName mirror: the last Name patch wins.
    if let Some(name) = patches.iter().rev().find_map(|p| match p {
        PatchField::Name(s) => Some(s),
        _ => None,
    }) {
        let encoded = encode_field(name, 28);
        fxp[0x1C..0x38].copy_from_slice(&encoded);
    }
    // State blob patches: rebuild the chunk region in place.
    let chunk = fxp[0x3C..0x38 + v + 4].to_vec();
    let new_chunk = patch_chunk_fields(&chunk, patches)?;
    fxp.truncate(0x3C);
    fxp.extend_from_slice(&new_chunk);
    let total = fxp.len();
    fxp[0x38..0x3C].copy_from_slice(&(new_chunk.len() as u32).to_be_bytes());
    fxp[4..8].copy_from_slice(&(total as u32).to_be_bytes());
    Ok(())
}

/// Inflate the preset state out of a complete fxp file.
pub fn inflate_state(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
    if data.len() < 0x40 || &data[0..4] != b"CcnK" {
        return Err("not an fxp file".into());
    }
    let v = u32::from_be_bytes(data[0x38..0x3C].try_into().unwrap()) as usize;
    if 0x38 + v + 4 > data.len() {
        return Err("chunkSize overruns file".into());
    }
    let n = u32::from_le_bytes(data[0x38 + v..0x38 + v + 4].try_into().unwrap()) as usize;
    crate::zlibio::inflate(
        &data[0x3C..(0x3C + n).min(data.len())],
        crate::zlibio::MAX_STREAM,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serum::SERUM1_STATE_SIZE;
    use crate::testutil::zlib_stream;

    fn sample_chunk() -> Vec<u8> {
        let mut s0 = vec![0u8; SERUM1_STATE_SIZE];
        s0[crate::serum::OFF_VERSION_F32..crate::serum::OFF_VERSION_F32 + 4]
            .copy_from_slice(&0.1631f32.to_le_bytes());
        s0[crate::serum::OFF_PRESET_NAME..crate::serum::OFF_PRESET_NAME + 4]
            .copy_from_slice(b"abc\0");
        let z0 = zlib_stream(&s0);
        let z1 = zlib_stream(&vec![0u8; 16384]);
        let mut chunk = z0.clone();
        chunk.extend_from_slice(&z1);
        chunk.extend_from_slice(&(z0.len() as u32).to_le_bytes());
        chunk
    }

    #[test]
    fn header_layout_is_exact() {
        let chunk = sample_chunk();
        let fxp = build_fxp(&chunk, "Hello");
        assert_eq!(&fxp[0..4], b"CcnK");
        assert_eq!(
            u32::from_be_bytes(fxp[4..8].try_into().unwrap()) as usize,
            fxp.len()
        );
        assert_eq!(&fxp[8..12], b"FPCh");
        assert_eq!(u32::from_be_bytes(fxp[12..16].try_into().unwrap()), 1);
        assert_eq!(&fxp[16..20], b"XfsX");
        assert_eq!(u32::from_be_bytes(fxp[20..24].try_into().unwrap()), 1);
        assert_eq!(u32::from_be_bytes(fxp[24..28].try_into().unwrap()), 1);
        assert_eq!(&fxp[28..35], b"Hello\0\0");
        assert_eq!(
            u32::from_be_bytes(fxp[0x38..0x3C].try_into().unwrap()) as usize,
            chunk.len()
        );
        assert_eq!(&fxp[0x3C..], &chunk[..]);
    }

    #[test]
    fn built_file_passes_serum2_rules() {
        let fxp = build_fxp(&sample_chunk(), "Test");
        let r = validate_fxp(&fxp);
        assert!(r.is_ok(), "issues: {:?}", r.issues);
        assert_eq!(r.warnings().count(), 0);
    }

    #[test]
    fn detects_broken_magic() {
        let mut fxp = build_fxp(&sample_chunk(), "Test");
        fxp[0x10] = b'Z';
        assert!(!validate_fxp(&fxp).is_ok());
        fxp[0] = b'X';
        assert!(!validate_fxp(&fxp).is_ok());
    }

    #[test]
    fn rejects_oversized_state() {
        let big = vec![0u8; SERUM1_STATE_SIZE + 4];
        let z0 = zlib_stream(&big);
        let mut chunk = z0.clone();
        chunk.extend_from_slice(&(z0.len() as u32).to_le_bytes());
        let fxp = build_fxp(&chunk, "Big");
        assert!(!validate_fxp(&fxp).is_ok());
    }

    #[test]
    fn rejects_bad_trailer() {
        let chunk = sample_chunk();
        let mut bad = chunk.clone();
        let last = bad.len() - 4;
        bad[last..last + 4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        let fxp = build_fxp(&bad, "Bad");
        // The importer would slice the wrong number of bytes; validation of
        // the state stream then fails or misparses.
        let r = validate_fxp(&fxp);
        assert!(!r.is_ok() || r.warnings().count() > 0);
    }

    #[test]
    fn version_out_of_range_is_fatal() {
        let mut s0 = vec![0u8; SERUM1_STATE_SIZE];
        s0[crate::serum::OFF_VERSION_F32..crate::serum::OFF_VERSION_F32 + 4]
            .copy_from_slice(&2.0f32.to_le_bytes());
        let z0 = zlib_stream(&s0);
        let mut chunk = z0.clone();
        chunk.extend_from_slice(&(z0.len() as u32).to_le_bytes());
        let fxp = build_fxp(&chunk, "Future");
        assert!(!validate_fxp(&fxp).is_ok());
    }

    fn patched(mut fxp: Vec<u8>, patches: &[PatchField]) -> Vec<u8> {
        patch_metadata(&mut fxp, patches).expect("patch_metadata");
        fxp
    }

    fn state_name(state: &[u8]) -> String {
        String::from_utf8_lossy(
            &state[crate::serum::OFF_PRESET_NAME..crate::serum::OFF_PRESET_NAME + 32],
        )
        .trim_end_matches('\0')
        .to_string()
    }

    fn prg_name(fxp: &[u8]) -> String {
        String::from_utf8_lossy(&fxp[0x1C..0x38])
            .trim_end_matches('\0')
            .to_string()
    }

    #[test]
    fn patch_name_updates_header_state_and_sizes() {
        let original = build_fxp(&sample_chunk(), "Old");
        let out = patched(original.clone(), &[PatchField::Name("Renamed".into())]);

        // Header mirror (byteSize at 0x04 is legitimately rewritten below).
        assert_eq!(prg_name(&out), "Renamed");
        assert_eq!(&out[0x08..0x1C], &original[0x08..0x1C]);

        // State blob field.
        let (state, consumed) = inflate_state(&out).unwrap();
        assert_eq!(state_name(&state), "Renamed");
        // Trailer == stream-0 compressed size and the stream consumes it all.
        let v = u32::from_be_bytes(out[0x38..0x3C].try_into().unwrap()) as usize;
        let n = u32::from_le_bytes(out[0x38 + v..0x38 + v + 4].try_into().unwrap()) as usize;
        assert_eq!(n, consumed);
        assert_eq!(0x3C + v, out.len());

        // Size fields fixed: chunkSize (BE) == chunk length, byteSize (BE) ==
        // whole file length.
        assert_eq!(v, out.len() - 0x3C);
        assert_eq!(
            u32::from_be_bytes(out[4..8].try_into().unwrap()) as usize,
            out.len()
        );

        // Level-1 zlib stream (Serum writes 78 01).
        assert_eq!(out[0x3C], 0x78);
        assert_eq!(out[0x3D], 0x01);

        // Result passes Serum2's import rules with no warnings.
        let r = validate_fxp(&out);
        assert!(r.is_ok(), "issues: {:?}", r.issues);
        assert_eq!(r.warnings().count(), 0);
    }

    #[test]
    fn patch_author_category_only_touches_state_fields() {
        let original = build_fxp(&sample_chunk(), "KeepMe");
        let (old_state, _) = inflate_state(&original).unwrap();
        let out = patched(
            original.clone(),
            &[
                PatchField::Author("Jane Doe".into()),
                PatchField::Category("Bass".into()),
            ],
        );

        // prgName untouched (no Name patch); ids/magics untouched (byteSize
        // at 0x04 is legitimately rewritten by the recompressed chunk).
        assert_eq!(prg_name(&out), "KeepMe");
        assert_eq!(&out[0x08..0x38], &original[0x08..0x38]);
        assert_eq!(
            u32::from_be_bytes(out[4..8].try_into().unwrap()) as usize,
            out.len()
        );

        let (state, _) = inflate_state(&out).unwrap();
        let author = &state[crate::serum::OFF_AUTHOR..crate::serum::OFF_AUTHOR + 48];
        let cat = &state[crate::serum::OFF_CATEGORY..crate::serum::OFF_CATEGORY + 48];
        assert_eq!(
            String::from_utf8_lossy(author).trim_end_matches('\0'),
            "Jane Doe"
        );
        assert_eq!(String::from_utf8_lossy(cat).trim_end_matches('\0'), "Bass");
        assert_eq!(state_name(&state), "abc");

        // Only the author + category ranges differ in the whole blob.
        assert_eq!(state.len(), old_state.len());
        for i in 0..state.len() {
            let in_author = (crate::serum::OFF_AUTHOR..crate::serum::OFF_AUTHOR + 48).contains(&i);
            let in_cat = (crate::serum::OFF_CATEGORY..crate::serum::OFF_CATEGORY + 48).contains(&i);
            if !in_author && !in_cat {
                assert_eq!(state[i], old_state[i], "byte {i} changed unexpectedly");
            }
        }
    }

    #[test]
    fn patch_name_keeps_other_state_bytes_identical() {
        let original = build_fxp(&sample_chunk(), "abc");
        let (old_state, _) = inflate_state(&original).unwrap();
        let out = patched(original, &[PatchField::Name("Totally New".into())]);
        let (state, _) = inflate_state(&out).unwrap();
        assert_eq!(state.len(), old_state.len());
        for i in 0..state.len() {
            let in_name =
                (crate::serum::OFF_PRESET_NAME..crate::serum::OFF_PRESET_NAME + 32).contains(&i);
            if !in_name {
                assert_eq!(state[i], old_state[i], "byte {i} changed unexpectedly");
            }
        }
        // Version float untouched.
        assert_eq!(
            &state[crate::serum::OFF_VERSION_F32..crate::serum::OFF_VERSION_F32 + 4],
            &0.1631f32.to_le_bytes()
        );
    }

    #[test]
    fn patch_truncates_utf8_on_char_boundary() {
        // 30 'é' chars = 60 bytes; the 28-byte header field must cut at 27
        // bytes, backing off to 26 (13 chars); the 32-byte state field cuts
        // at 31, backing off to 30 (15 chars).
        let name = "é".repeat(30);
        let out = patched(build_fxp(&sample_chunk(), "x"), &[PatchField::Name(name)]);
        let hdr = &out[0x1C..0x38];
        assert_eq!(hdr[26], 0, "header field must end at the boundary");
        assert_eq!(
            String::from_utf8_lossy(&hdr[..26]),
            "é".repeat(13),
            "header"
        );
        let (state, _) = inflate_state(&out).unwrap();
        let field = &state[crate::serum::OFF_PRESET_NAME..crate::serum::OFF_PRESET_NAME + 32];
        assert_eq!(field[30], 0, "state field must end at the boundary");
        assert_eq!(
            String::from_utf8_lossy(&field[..30]),
            "é".repeat(15),
            "state"
        );
        assert!(validate_fxp(&out).is_ok());
    }

    #[test]
    fn patch_long_name_fits_header_and_state() {
        // 27 chars: exactly fills prgName (last byte stays NUL); state holds
        // all 27 + NUL.
        let name = "a".repeat(27);
        let out = patched(
            build_fxp(&sample_chunk(), "x"),
            &[PatchField::Name(name.clone())],
        );
        assert_eq!(prg_name(&out), name);
        let (state, _) = inflate_state(&out).unwrap();
        assert_eq!(state_name(&state), name);
    }

    #[test]
    fn patch_all_fields_at_once() {
        let out = patched(
            build_fxp(&sample_chunk(), "Old"),
            &[
                PatchField::Name("New".into()),
                PatchField::Author("Auth".into()),
                PatchField::Category("Cat".into()),
            ],
        );
        assert_eq!(prg_name(&out), "New");
        let (state, _) = inflate_state(&out).unwrap();
        assert_eq!(state_name(&state), "New");
        assert_eq!(
            String::from_utf8_lossy(
                &state[crate::serum::OFF_AUTHOR..crate::serum::OFF_AUTHOR + 48]
            )
            .trim_end_matches('\0'),
            "Auth"
        );
        assert_eq!(
            String::from_utf8_lossy(
                &state[crate::serum::OFF_CATEGORY..crate::serum::OFF_CATEGORY + 48]
            )
            .trim_end_matches('\0'),
            "Cat"
        );
        let r = validate_fxp(&out);
        assert!(r.is_ok(), "issues: {:?}", r.issues);
    }

    #[test]
    fn patch_rejects_malformed_containers() {
        let good = build_fxp(&sample_chunk(), "x");
        let patches = [PatchField::Name("N".into())];

        // Not an fxp at all.
        let mut bad = b"garbagegarbagegarbagegarbagegarbagegarbagegarbage".to_vec();
        bad.extend_from_slice(&good[50..]);
        assert!(patch_metadata(&mut bad, &patches).is_err());

        // Too small.
        let mut short = good.clone();
        short.truncate(30);
        assert!(patch_metadata(&mut short, &patches).is_err());

        // chunkSize overruns the file.
        let mut over = good.clone();
        over[0x38..0x3C].copy_from_slice(&0xFFFF_0000u32.to_be_bytes());
        assert!(patch_metadata(&mut over, &patches).is_err());

        // Corrupt stream 0 (not zlib).
        let mut corrupt = good.clone();
        corrupt[0x3C] = b'Z';
        assert!(patch_metadata(&mut corrupt, &patches).is_err());

        // Lying trailer: stream 0 is longer than claimed.
        let mut lying = good;
        let v = u32::from_be_bytes(lying[0x38..0x3C].try_into().unwrap()) as usize;
        let n = u32::from_le_bytes(lying[0x38 + v..0x38 + v + 4].try_into().unwrap());
        lying[0x38 + v..0x38 + v + 4].copy_from_slice(&(n / 2).to_le_bytes());
        assert!(patch_metadata(&mut lying, &patches).is_err());
    }

    #[test]
    fn patch_noop_when_no_patches() {
        let original = build_fxp(&sample_chunk(), "Same");
        let mut copy = original.clone();
        patch_metadata(&mut copy, &[]).unwrap();
        assert_eq!(copy, original);
    }
}

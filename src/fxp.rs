//! Building and validating Serum 1 `.fxp` preset files.
//!
//! File layout (all multi-byte header fields big-endian unless noted),
//! verified against 25 genuine Serum 1 presets (2015-2026) and against the
//! Serum 2 importer disassembly (Serum2.vst3 2.0.23, see README):
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
//! stream; Serum 2's importer uses it to slice the state out of the chunk.

pub const FXP_HEADER_LEN: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Serum 2 refuses to import the file.
    Fatal,
    /// The file loads, but deviates from what genuine Serum 1 writes.
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

/// Assemble a complete Serum 1 `.fxp` from a chunk (zlib streams + trailer).
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

/// Inflate one zlib stream; returns (data, consumed).
fn inflate_stream(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
    use std::io::Read;
    let mut dec = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::with_capacity(4096);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match dec.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&chunk[..n]);
                if out.len() > 16 * 1024 * 1024 {
                    return Err("decompressed stream exceeds sanity limit".into());
                }
            }
            Err(e) => return Err(format!("zlib error: {e}")),
        }
    }
    Ok((out, dec.total_in() as usize))
}

/// Validate an fxp against the checks the Serum 2 importer performs.
///
/// The rule set below mirrors the disassembled import path of
/// Serum2.vst3 2.0.23 (`load_entry` -> validator -> `s1state_load`); a file
/// with no [`Severity::Fatal`] issue is accepted by that code path.
pub fn validate_fxp(data: &[u8]) -> ValidationReport {
    let mut r = ValidationReport::default();

    // --- rules reimplemented from the Serum 2 importer ---
    if data.len() < 0x3D {
        r.fatal(format!("file too small ({} bytes, need >= 61)", data.len()));
        return r;
    }
    if &data[0..4] != b"CcnK" {
        r.fatal("missing \"CcnK\" magic at offset 0");
        return r;
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
            // Serum 1 presets store the state zlib-compressed; the loader
            // also tolerates raw (uncompressed) state bytes.
            let (state, _consumed): (Vec<u8>, usize) = if blob.first() == Some(&0x78) {
                match inflate_stream(blob) {
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
                        "decompressed state is {} bytes; Serum 2 zero-pads to {}",
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
                            "preset version {ver} triggers Serum 2's old-patch warning"
                        ));
                    } else if ver < 0.149 {
                        r.warn(format!(
                            "preset version {ver} sets Serum 2's oldSerum1Preset compatibility flag"
                        ));
                    }
                } else {
                    r.warn("state too short to contain a version float");
                }
            }
        }
    }

    // --- extra structural checks (how genuine Serum 1 files look) ---
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
    r
}

/// Validate a bare chunk (zlib streams + trailer) against the Serum 2 rules
/// by round-tripping it through the fxp container.
pub fn validate_chunk_report(chunk: &[u8]) -> ValidationReport {
    validate_fxp(&build_fxp(chunk, ""))
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
    inflate_stream(&data[0x3C..(0x3C + n).min(data.len())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serum::SERUM1_STATE_SIZE;

    fn zlib_stream(data: &[u8]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;
        let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

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
}

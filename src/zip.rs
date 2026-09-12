//! Minimal in-memory ZIP reader for zipped FL Studio "loop package" exports.
//!
//! Implements just enough of the ZIP specification to unpack an archive
//! without adding a dependency:
//!
//! - The End-of-Central-Directory record is located by a backwards scan,
//!   the Central Directory is walked, and each member's data is resolved
//!   through its Local File Header (sizes/offsets come from the Central
//!   Directory, so data-descriptor archives work).
//! - Stored (method 0) and Deflate (method 8) members are supported;
//!   deflate is inflated with `flate2`'s pure-Rust `miniz_oxide` backend,
//!   which compiles unchanged on `wasm32-unknown-unknown`.
//! - Encrypted members (general-purpose flag bits 0 / 6) and Zip64 archives
//!   are rejected with explicit errors.
//! - The total decompressed size is capped at [`MAX_TOTAL_UNCOMPRESSED`],
//!   enforced both on the sizes declared in the Central Directory (before
//!   any allocation) and per member while inflating.
//! - Members are returned as raw bytes and never inspected further, so a
//!   zip-in-zip is never recursed into (a nested archive simply fails FLP
//!   parsing downstream).
//!
//! No real FL Studio loop-package sample was available during development
//! (the export needs the FL Studio UI); the format handled here is the
//! standard ZIP structure such exports use, and every entry of the archive
//! is probed for a `.flp` name rather than assuming a fixed layout.

/// Sanity cap on the total decompressed size of one archive (all members
/// combined), mirroring the per-stream caps in [`crate::zlibio`].
pub const MAX_TOTAL_UNCOMPRESSED: usize = 256 * 1024 * 1024;

/// One decompressed archive member.
#[derive(Debug)]
pub struct ZipMember {
    /// Member name as stored in the archive (directories included).
    pub name: String,
    /// Decompressed member bytes.
    pub data: Vec<u8>,
}

/// True when `buf` starts with the ZIP signature (`PK`).
pub fn is_zip(buf: &[u8]) -> bool {
    buf.len() >= 2 && &buf[0..2] == b"PK"
}

/// Unpack every supported member of the archive in `buf`.
pub fn unzip(buf: &[u8]) -> Result<Vec<ZipMember>, String> {
    unzip_with_limit(buf, MAX_TOTAL_UNCOMPRESSED)
}

/// Like [`unzip`], but with an explicit total decompressed-size cap
/// (exposed for tests; production callers use [`unzip`]).
pub fn unzip_with_limit(buf: &[u8], max_total: usize) -> Result<Vec<ZipMember>, String> {
    let dir = read_central_directory(buf)?;
    let declared_total = dir
        .iter()
        .fold(0usize, |acc, e| acc.saturating_add(e.uncompressed_size));
    if declared_total > max_total {
        return Err(format!(
            "decompressed archive exceeds the sanity limit of {} MiB",
            max_total / (1024 * 1024)
        ));
    }
    let mut members = Vec::with_capacity(dir.len());
    for entry in dir {
        if entry.name.ends_with('/') {
            continue;
        }
        let data = member_data(buf, &entry, max_total)?;
        members.push(ZipMember {
            name: entry.name,
            data,
        });
    }
    Ok(members)
}

/// Unpack only the `*.flp` members (case-insensitive) of the archive.
pub fn unzip_flp_members(buf: &[u8]) -> Result<Vec<ZipMember>, String> {
    let members = unzip(buf)?;
    Ok(members
        .into_iter()
        .filter(|m| m.name.to_lowercase().ends_with(".flp"))
        .collect())
}

struct CentralEntry {
    method: u16,
    compressed_size: usize,
    uncompressed_size: usize,
    local_offset: usize,
    name: String,
}

fn u16le(buf: &[u8], off: usize) -> Result<u16, String> {
    let field = buf
        .get(off..off + 2)
        .ok_or("truncated ZIP structure (field out of range)")?;
    Ok(u16::from_le_bytes(field.try_into().unwrap()))
}

fn u32le(buf: &[u8], off: usize) -> Result<u32, String> {
    let field = buf
        .get(off..off + 4)
        .ok_or("truncated ZIP structure (field out of range)")?;
    Ok(u32::from_le_bytes(field.try_into().unwrap()))
}

/// Locate the End-of-Central-Directory record (scanning backwards over the
/// maximum possible archive comment).
fn find_eocd(buf: &[u8]) -> Result<usize, String> {
    if buf.len() < 22 {
        return Err("too small to be a ZIP archive".into());
    }
    let mut pos = buf.len() - 22;
    let lowest = pos.saturating_sub(0xFFFF);
    loop {
        if &buf[pos..pos + 4] == b"PK\x05\x06" {
            let comment_len = u16le(buf, pos + 20)? as usize;
            if pos + 22 + comment_len <= buf.len() {
                return Ok(pos);
            }
        }
        if pos == lowest {
            break;
        }
        pos -= 1;
    }
    Err("no ZIP End-of-Central-Directory record found (not a ZIP archive?)".into())
}

fn read_central_directory(buf: &[u8]) -> Result<Vec<CentralEntry>, String> {
    let eocd = find_eocd(buf)?;
    let total_entries = u16le(buf, eocd + 10)? as usize;
    let cd_size = u32le(buf, eocd + 12)? as usize;
    let cd_offset = u32le(buf, eocd + 16)? as usize;
    if u16le(buf, eocd + 4)? != 0 || u16le(buf, eocd + 6)? != 0 {
        return Err("multi-disk ZIP archives are not supported".into());
    }
    const U32_MAX: usize = u32::MAX as usize;
    if total_entries == 0xFFFF || cd_size == U32_MAX || cd_offset == U32_MAX {
        return Err("Zip64 ZIP archives are not supported".into());
    }
    let mut entries = Vec::with_capacity(total_entries);
    let mut pos = cd_offset;
    for _ in 0..total_entries {
        if buf.get(pos..pos + 4) != Some(&b"PK\x01\x02"[..]) {
            return Err("corrupt ZIP central directory (bad entry signature)".into());
        }
        let flags = u16le(buf, pos + 8)?;
        let method = u16le(buf, pos + 10)?;
        let compressed_size = u32le(buf, pos + 20)? as usize;
        let uncompressed_size = u32le(buf, pos + 24)? as usize;
        let name_len = u16le(buf, pos + 28)? as usize;
        let extra_len = u16le(buf, pos + 30)? as usize;
        let comment_len = u16le(buf, pos + 32)? as usize;
        let local_offset = u32le(buf, pos + 42)? as usize;
        let name_bytes = buf
            .get(pos + 46..pos + 46 + name_len)
            .ok_or("truncated ZIP central directory entry")?;
        if compressed_size == U32_MAX || uncompressed_size == U32_MAX || local_offset == U32_MAX {
            return Err(format!(
                "member '{}' uses Zip64 sizes/offsets (not supported)",
                String::from_utf8_lossy(name_bytes)
            ));
        }
        if flags & 0x0001 != 0 || flags & 0x0040 != 0 {
            return Err(format!(
                "member '{}' is encrypted (not supported)",
                String::from_utf8_lossy(name_bytes)
            ));
        }
        entries.push(CentralEntry {
            method,
            compressed_size,
            uncompressed_size,
            local_offset,
            name: String::from_utf8_lossy(name_bytes).into_owned(),
        });
        pos = match pos.checked_add(46 + name_len + extra_len + comment_len) {
            Some(p) => p,
            None => return Err("corrupt ZIP central directory (entry size overflow)".into()),
        };
    }
    Ok(entries)
}

/// Decompress one member, resolving its data through the Local File Header.
fn member_data(buf: &[u8], entry: &CentralEntry, max_total: usize) -> Result<Vec<u8>, String> {
    let lfh = entry.local_offset;
    if buf.get(lfh..lfh + 4) != Some(&b"PK\x03\x04"[..]) {
        return Err(format!(
            "member '{}' has a missing or corrupt local file header",
            entry.name
        ));
    }
    let name_len = u16le(buf, lfh + 26)? as usize;
    let extra_len = u16le(buf, lfh + 28)? as usize;
    let data_start = lfh
        .checked_add(30 + name_len + extra_len)
        .ok_or_else(|| format!("member '{}' data overruns the archive", entry.name))?;
    let data_end = data_start
        .checked_add(entry.compressed_size)
        .ok_or_else(|| format!("member '{}' data overruns the archive", entry.name))?;
    let raw = buf
        .get(data_start..data_end)
        .ok_or_else(|| format!("member '{}' data overruns the archive", entry.name))?;
    match entry.method {
        0 => {
            if raw.len() != entry.uncompressed_size {
                return Err(format!("member '{}' stored size mismatch", entry.name));
            }
            Ok(raw.to_vec())
        }
        8 => inflate_member(raw, entry, max_total),
        other => Err(format!(
            "member '{}' uses unsupported compression method {other}",
            entry.name
        )),
    }
}

/// Inflate a raw deflate stream and pin the result to the size declared in
/// the Central Directory (`read_exact` + one extra byte, so both a short and
/// an oversized stream are rejected while the allocation stays bounded).
fn inflate_member(raw: &[u8], entry: &CentralEntry, max_total: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if entry.uncompressed_size > max_total {
        return Err(format!(
            "decompressed archive exceeds the sanity limit of {} MiB",
            max_total / (1024 * 1024)
        ));
    }
    let mut dec = flate2::read::DeflateDecoder::new(raw);
    let mut out = vec![0u8; entry.uncompressed_size];
    if let Err(e) = dec.read_exact(&mut out) {
        return Err(format!("member '{}' failed to inflate: {e}", entry.name));
    }
    let mut extra = [0u8; 1];
    if matches!(dec.read(&mut extra), Ok(n) if n > 0) {
        return Err(format!(
            "member '{}' inflates to more bytes than declared",
            entry.name
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::zip_archive;

    #[test]
    fn detects_zip_prefix() {
        assert!(is_zip(b"PK\x03\x04rest"));
        assert!(is_zip(b"PK\x05\x06"));
        assert!(!is_zip(b"FLhd...."));
        // Same predicate the FLP parser uses; a bare "PK" fails downstream
        // in unzip() with a clear error.
        assert!(is_zip(b"PK"));
    }

    #[test]
    fn unpacks_stored_and_deflated_members() {
        let members = vec![
            ("one.flp", vec![1u8, 2, 3, 4, 5]),
            ("two.txt", b"hello hello hello hello".to_vec()),
            ("THREE.FLP", vec![9u8; 4096]),
        ];
        let archive = zip_archive(&members, false);
        let all = unzip(&archive).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].name, "one.flp");
        assert_eq!(all[0].data, vec![1, 2, 3, 4, 5]);
        assert_eq!(all[1].name, "two.txt");
        assert_eq!(all[2].data, vec![9u8; 4096]);

        let flps = unzip_flp_members(&archive).unwrap();
        assert_eq!(
            flps.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            vec!["one.flp", "THREE.FLP"]
        );
        assert_eq!(flps[1].data, vec![9u8; 4096]);
    }

    #[test]
    fn rejects_encrypted_member() {
        let archive = zip_archive(&[("secret.flp", vec![1, 2, 3])], false);
        // Flip the encryption bit in the central directory entry.
        let mut patched = archive.clone();
        let cd = find_eocd(&patched).unwrap();
        let cd_offset = u32le(&patched, cd + 16).unwrap() as usize;
        let flags = u16le(&patched, cd_offset + 8).unwrap() | 0x0001;
        patched[cd_offset + 8..cd_offset + 10].copy_from_slice(&flags.to_le_bytes());
        let err = unzip(&patched).unwrap_err();
        assert!(err.contains("encrypted"), "{err}");

        // Strong-encryption bit rejected too.
        let mut patched2 = archive;
        patched2[cd_offset + 8..cd_offset + 10].copy_from_slice(&(flags | 0x0040).to_le_bytes());
        let err = unzip(&patched2).unwrap_err();
        assert!(err.contains("encrypted"), "{err}");
    }

    #[test]
    fn rejects_zip64_markers() {
        let archive = zip_archive(&[("a.flp", vec![1])], false);
        let eocd = find_eocd(&archive).unwrap();
        // 0xFFFF total entries in the EOCD marks a Zip64 archive.
        let mut patched = archive.clone();
        patched[eocd + 10..eocd + 12].copy_from_slice(&0xFFFFu16.to_le_bytes());
        let err = unzip(&patched).unwrap_err();
        assert!(err.contains("Zip64"), "{err}");

        // 0xFFFFFFFF central-directory offset marks a Zip64 archive.
        let mut patched2 = archive;
        patched2[eocd + 16..eocd + 20].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());
        let err = unzip(&patched2).unwrap_err();
        assert!(err.contains("Zip64"), "{err}");
    }

    #[test]
    fn rejects_truncated_and_non_zip_input() {
        let archive = zip_archive(&[("a.flp", vec![1, 2, 3])], false);
        let err = unzip(&archive[..archive.len() - 10]).unwrap_err();
        assert!(!err.is_empty());
        assert!(unzip(b"not a zip at all...........").is_err());
        assert!(unzip(b"PK").is_err());
    }

    #[test]
    fn wrong_extension_only_yields_no_flp_members() {
        let archive = zip_archive(
            &[
                ("readme.txt", b"x".to_vec()),
                ("nested.zip", b"PK".to_vec()),
            ],
            true,
        );
        let members = unzip_flp_members(&archive).unwrap();
        assert!(members.is_empty());
        // ...but unzip itself still sees them.
        assert_eq!(unzip(&archive).unwrap().len(), 2);
    }

    #[test]
    fn oversized_archive_is_rejected_before_inflating() {
        // Declared (Central Directory) sizes already exceed the cap: the
        // deflate encoder output is tiny, but the header lies big.
        let archive = zip_archive(&[("bomb.flp", vec![0u8; 64])], true);
        // Patch the declared uncompressed size up to 2 GiB in CD + LFH.
        let eocd = find_eocd(&archive).unwrap();
        let cd_offset = u32le(&archive, eocd + 16).unwrap() as usize;
        let mut patched = archive.clone();
        let big = (2u64 * 1024 * 1024 * 1024) as u32;
        patched[cd_offset + 24..cd_offset + 28].copy_from_slice(&big.to_le_bytes());
        let err = unzip(&patched).unwrap_err();
        assert!(err.contains("sanity limit"), "{err}");
    }

    #[test]
    fn oversized_stream_is_rejected_while_inflating() {
        // The header declares small sizes (passes the pre-check), but the
        // deflate stream inflates beyond the limit.
        let mut archive = zip_archive(&[("bomb.flp", vec![0u8; 4096])], true);
        let eocd = find_eocd(&archive).unwrap();
        let cd_offset = u32le(&archive, eocd + 16).unwrap() as usize;
        let small = 16u32;
        archive[cd_offset + 24..cd_offset + 28].copy_from_slice(&small.to_le_bytes());
        // Local header uncompressed-size field sits at the same offset +4.
        let lfh = u32le(&archive, cd_offset + 42).unwrap() as usize;
        archive[lfh + 22..lfh + 26].copy_from_slice(&small.to_le_bytes());
        let err = unzip_with_limit(&archive, 1024).unwrap_err();
        assert!(err.contains("more bytes than declared"), "{err}");
    }

    #[test]
    fn handles_archive_comment_and_empty_archive() {
        let archive = zip_archive(&[], false);
        assert!(unzip(&archive).unwrap().is_empty());
        // A comment after the EOCD must not break the scan.
        let mut with_comment = zip_archive(&[("a.flp", vec![1])], false);
        with_comment.extend_from_slice(b"FL Studio loop package");
        let before = unzip(&with_comment).unwrap().len();
        // Re-write the EOCD comment length to cover the appended bytes.
        let eocd = find_eocd(&with_comment).unwrap();
        let comment_len = (with_comment.len() - eocd - 22) as u16;
        with_comment[eocd + 20..eocd + 22].copy_from_slice(&comment_len.to_le_bytes());
        assert_eq!(unzip(&with_comment).unwrap().len(), before);
    }
}

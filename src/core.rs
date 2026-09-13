//! Reusable scanning logic shared by the CLI and the WebAssembly bindings.
//!
//! [`scan_serum_instances`] walks the FLP event stream once and collects
//! every usable Serum plugin instance (see [`Instance`]) together with
//! diagnostic counters ([`ScanStats`]). Everything in this module is
//! IO-free and platform-independent, so it behaves identically on native
//! targets and on `wasm32-unknown-unknown`.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::flp;
use crate::serum;
use crate::serum2state;
use crate::zip;

/// One FLP document ready for scanning: either the raw input bytes or a
/// single `*.flp` member unpacked in memory from a zipped loop package
/// (see [`flp_inputs`]).
#[derive(Debug)]
pub struct FlpInput {
    /// Display name: the input's own name for plain FLPs, or
    /// `<input>#<member>` for archive members (empty input name yields just
    /// the member name).
    pub name: String,
    /// FLP bytes (decompressed for archive members).
    pub data: Vec<u8>,
    /// True when this document was unpacked from a ZIP archive.
    pub from_archive: bool,
}

/// Resolve raw input bytes into the FLP documents to process.
///
/// Plain FLP bytes yield a single document named `input_name`. A
/// `PK`-prefixed ZIP archive (FL Studio "zipped loop package") is unpacked
/// in memory (see [`crate::zip`]) and every `*.flp` member
/// (case-insensitive) becomes one document. Nested archives are never
/// unpacked: a `.flp` member that is itself a ZIP reaches the scanner and
/// fails there with a clear error. A corrupt archive, or one without any
/// `.flp` member, is a fatal error.
pub fn flp_inputs(input_name: &str, buf: &[u8]) -> Result<Vec<FlpInput>, String> {
    if !zip::is_zip(buf) {
        return Ok(vec![FlpInput {
            name: input_name.to_string(),
            data: buf.to_vec(),
            from_archive: false,
        }]);
    }
    let members = zip::unzip_flp_members(buf)?;
    if members.is_empty() {
        return if input_name.is_empty() {
            Err("the ZIP archive does not contain any .flp member".into())
        } else {
            Err(format!(
                "'{input_name}' is a ZIP archive without any .flp member"
            ))
        };
    }
    Ok(members
        .into_iter()
        .map(|m| {
            let name = if input_name.is_empty() {
                m.name
            } else {
                format!("{input_name}#{}", m.name)
            };
            FlpInput {
                name,
                data: m.data,
                from_archive: true,
            }
        })
        .collect())
}

/// Diagnostics collected while scanning an FLP.
#[derive(Debug, Default)]
pub struct ScanStats {
    /// Number of Serum2 plugin instances encountered (never extracted).
    pub serum2_count: usize,
    /// Human-readable descriptions of Serum plugin states that could not
    /// be converted into a preset chunk. Per-instance failures never abort
    /// the scan; callers decide whether to surface them.
    pub failed: Vec<String>,
}

/// A Serum instance discovered in an FLP.
#[derive(Debug)]
pub struct Instance {
    /// Numeric FL Studio channel the plugin was inserted on, if known.
    pub channel: Option<u16>,
    /// Channel name, falling back to the FX track name.
    pub channel_name: String,
    /// Plugin display name as stored in the FLP.
    pub plugin_name: String,
    /// Recovered Serum preset chunk plus parsed metadata.
    pub chunk: serum::Serum1Chunk,
}

/// A Serum2 instance discovered in an FLP: the inner `XferJson` processor
/// (cid 3) and controller (cid 2) records of FL's VST3 wrapper state
/// (`docs/flp-serum2-conversion.md` §3.1).
#[derive(Debug)]
pub struct Serum2Instance {
    /// Numeric FL Studio channel the plugin was inserted on, if known.
    pub channel: Option<u16>,
    /// Channel name, falling back to the FX track name.
    pub channel_name: String,
    /// Plugin display name as stored in the FLP.
    pub plugin_name: String,
    /// Inner cid-3 `XferJson` record (instantiated processor state).
    pub processor: Vec<u8>,
    /// Inner cid-2 `XferJson` record (controller state; its JSON header
    /// carries the preset identity).
    pub controller: Vec<u8>,
    /// Preset name/author/description from the controller JSON header.
    pub meta: serum2state::ControllerMeta,
}

/// Walk the events once, associating plugin params with channel / FX names.
///
/// Returns the discovered instances in file order plus [`ScanStats`].
/// Unparseable FLP data (ZIP archive, missing `FLhd`, truncated events, ...)
/// is a fatal `Err(String)` with a human-readable message; per-instance
/// conversion failures are only recorded in [`ScanStats::failed`]. ZIP
/// inputs must be resolved through [`flp_inputs`] first.
pub fn scan_serum_instances(buf: &[u8]) -> Result<(Vec<Instance>, ScanStats), String> {
    let events = flp::parse_events(buf)?;
    let mut channels: HashMap<u16, String> = HashMap::new();
    let mut cur_channel: Option<u16> = None;
    let mut cur_fx_name = String::new();
    let mut instances = Vec::new();
    let mut stats = ScanStats::default();

    for ev in &events {
        match ev.id {
            flp::EV_NEW_CHANNEL => {
                if ev.data.len() >= 2 {
                    cur_channel = Some(u16::from_le_bytes([ev.data[0], ev.data[1]]));
                }
            }
            flp::EV_TEXT_CHANNEL_NAME => {
                if let Some(ch) = cur_channel {
                    channels.insert(ch, text(ev.data));
                }
            }
            flp::EV_TEXT_FX_TRACK_NAME => {
                cur_fx_name = text(ev.data);
            }
            flp::EV_PLUGIN_PARAMS => {
                let Ok(pp) = flp::parse_plugin_params(ev.data) else {
                    continue;
                };
                if serum::is_serum2(pp.name, pp.filename) {
                    stats.serum2_count += 1;
                    continue;
                }
                if !serum::is_serum1(pp.name, pp.filename) || pp.state.is_empty() {
                    continue;
                }
                match serum::serum1_chunk_from_state(pp.state) {
                    Ok(chunk) => {
                        let channel = cur_channel;
                        instances.push(Instance {
                            channel,
                            channel_name: channel
                                .and_then(|c| channels.get(&c).cloned())
                                .unwrap_or_else(|| cur_fx_name.clone()),
                            plugin_name: text(pp.name),
                            chunk,
                        });
                    }
                    Err(e) => {
                        let where_ = cur_channel
                            .and_then(|c| channels.get(&c).cloned())
                            .unwrap_or_else(|| cur_fx_name.clone());
                        stats
                            .failed
                            .push(format!("skipped a Serum plugin state ({where_}): {e}"));
                    }
                }
            }
            _ => {}
        }
    }
    Ok((instances, stats))
}

/// Like [`scan_serum_instances`], but for Serum2 instances: returns every
/// Serum2 plugin instance's processor + controller records in file order
/// plus per-instance skip warnings (instances whose wrapper state does not
/// match the documented record layout are reported, not fatal).
pub fn scan_serum2_instances(buf: &[u8]) -> Result<(Vec<Serum2Instance>, Vec<String>), String> {
    let events = flp::parse_events(buf)?;
    let mut channels: HashMap<u16, String> = HashMap::new();
    let mut cur_channel: Option<u16> = None;
    let mut cur_fx_name = String::new();
    let mut instances = Vec::new();
    let mut warnings = Vec::new();

    for ev in &events {
        match ev.id {
            flp::EV_NEW_CHANNEL => {
                if ev.data.len() >= 2 {
                    cur_channel = Some(u16::from_le_bytes([ev.data[0], ev.data[1]]));
                }
            }
            flp::EV_TEXT_CHANNEL_NAME => {
                if let Some(ch) = cur_channel {
                    channels.insert(ch, text(ev.data));
                }
            }
            flp::EV_TEXT_FX_TRACK_NAME => {
                cur_fx_name = text(ev.data);
            }
            flp::EV_PLUGIN_PARAMS => {
                let Ok(pp) = flp::parse_plugin_params(ev.data) else {
                    continue;
                };
                if !serum::is_serum2(pp.name, pp.filename) || pp.state.is_empty() {
                    continue;
                }
                let where_ = cur_channel
                    .and_then(|c| channels.get(&c).cloned())
                    .unwrap_or_else(|| cur_fx_name.clone());
                match serum::serum2_records_from_state(pp.state) {
                    Some((processor, controller)) => {
                        let meta = match serum2state::parse_xfer_json(controller) {
                            Ok((json, ..)) => serum2state::controller_meta_from_json(&json),
                            Err(_) => serum2state::ControllerMeta::default(),
                        };
                        instances.push(Serum2Instance {
                            channel: cur_channel,
                            channel_name: where_,
                            plugin_name: text(pp.name),
                            processor: processor.to_vec(),
                            controller: controller.to_vec(),
                            meta,
                        });
                    }
                    None => warnings.push(format!(
                        "skipped a Serum2 instance ({where_}): unrecognized VST3 wrapper state"
                    )),
                }
            }
            _ => {}
        }
    }
    Ok((instances, warnings))
}

/// Fallback channel label for report messages (`-` when empty).
pub(crate) fn display_name(name: &str) -> &str {
    if name.is_empty() { "-" } else { name }
}

/// Read a NUL-terminated fixed-width string field at `buf[off..off+len]`.
/// Out-of-range fields read as empty; `trim` strips surrounding whitespace.
pub(crate) fn cstr(buf: &[u8], off: usize, len: usize, trim: bool) -> String {
    let Some(field) = buf.get(off..off + len) else {
        return String::new();
    };
    let nul = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    let s = String::from_utf8_lossy(&field[..nul]);
    if trim {
        s.trim().to_string()
    } else {
        s.into_owned()
    }
}

/// Decode an FL Studio text event: UTF-16LE when the bytes look like it
/// (NUL-interleaved ASCII), UTF-8 otherwise. Trailing NULs and whitespace
/// are trimmed.
pub fn text(bytes: &[u8]) -> String {
    if bytes.len() >= 2
        && bytes.len().is_multiple_of(2)
        && bytes.iter().skip(1).step_by(2).all(|&x| x == 0)
        && bytes.iter().step_by(2).any(|&x| x != 0)
    {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .trim()
            .to_string();
    }
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .trim()
        .to_string()
}

/// Deterministic content hash of a preset chunk (std `DefaultHasher`).
///
/// `DefaultHasher` uses fixed SipHash-1-3 keys, so the value is stable
/// within a process session on every platform - good enough to dedupe
/// identical chunks in the CLI output and in the web UI.
pub fn hash_bytes(b: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

/// Map a preset / channel name to a safe file name base: filesystem-hostile
/// characters become `_`, surrounding dots and whitespace are trimmed, the
/// result is capped at 80 chars and never empty (`Untitled`).
pub fn sanitize_filename(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = mapped.trim().trim_matches('.').trim().to_string();
    if trimmed.is_empty() {
        "Untitled".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

/// Default output directory for an FLP: `<flp name>_serum_fxp` next to it.
pub fn default_out_dir(flp: &Path) -> PathBuf {
    let stem = flp
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".into());
    let mut name = sanitize_filename(&stem);
    if name.is_empty() {
        name = "output".into();
    }
    name.push_str("_serum_fxp");
    flp.parent()
        .map(|p| p.join(&name))
        .unwrap_or_else(|| name.into())
}

/// Human-readable byte count (`977 B`, `8.0 KiB`, `1.2 MiB`).
pub fn format_bytes(n: usize) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{build_flp, zip_archive};

    #[test]
    fn plain_input_yields_single_document() {
        let flp = build_flp(&[(9, vec![1])]);
        let docs = flp_inputs("song.flp", &flp).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name, "song.flp");
        assert_eq!(docs[0].data, flp);
        assert!(!docs[0].from_archive);
    }

    #[test]
    fn zip_input_yields_flp_member_documents() {
        let flp_a = build_flp(&[(9, vec![1])]);
        let flp_b = build_flp(&[(9, vec![2])]);
        let archive = zip_archive(
            &[
                ("a.flp", flp_a.clone()),
                ("notes.txt", b"hi".to_vec()),
                ("dir/b.FLP", flp_b.clone()),
            ],
            true,
        );
        let docs = flp_inputs("pack.zip", &archive).unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].name, "pack.zip#a.flp");
        assert_eq!(docs[0].data, flp_a);
        assert_eq!(docs[1].name, "pack.zip#dir/b.FLP");
        assert_eq!(docs[1].data, flp_b);
        assert!(docs.iter().all(|d| d.from_archive));
    }

    #[test]
    fn zip_without_flp_members_is_an_error() {
        let archive = zip_archive(&[("only.txt", b"x".to_vec())], false);
        let err = flp_inputs("pack.zip", &archive).unwrap_err();
        assert!(err.contains("without any .flp member"), "{err}");
    }

    #[test]
    fn nested_zip_member_is_never_recursed() {
        // A member named *.flp that is itself a ZIP is returned as-is and
        // must fail FLP parsing (no recursion into zip-in-zip).
        let inner = zip_archive(&[("deep.flp", build_flp(&[(9, vec![1])]))], false);
        let archive = zip_archive(&[("outer.flp", inner)], false);
        let docs = flp_inputs("pack.zip", &archive).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(&docs[0].data[..2], b"PK");
        let scan = scan_serum_instances(&docs[0].data);
        assert!(scan.is_err());
    }

    #[test]
    fn sanitizes_filenames() {
        assert_eq!(sanitize_filename("a/b:c*d?"), "a_b_c_d_");
        assert_eq!(sanitize_filename("  ..name.. "), "name");
        assert_eq!(sanitize_filename("..."), "Untitled");
        assert_eq!(sanitize_filename("\u{1}ok"), "_ok");
    }

    #[test]
    fn decodes_utf16le_and_utf8_text() {
        let utf16: Vec<u8> = "Ab".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(text(&utf16), "Ab");
        assert_eq!(text(b"plain\0"), "plain");
        assert_eq!(text(b""), "");
    }

    #[test]
    fn content_hash_is_stable() {
        assert_eq!(hash_bytes(b"abc"), hash_bytes(b"abc"));
        assert_ne!(hash_bytes(b"abc"), hash_bytes(b"abd"));
    }
}

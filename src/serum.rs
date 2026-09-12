//! Serum plugin state handling: detecting Serum instances and recovering
//! the Serum preset chunk (the exact `chunk` region of a Serum `.fxp`).

/// Size of the decompressed Serum preset state for contemporary presets.
pub const SERUM1_STATE_SIZE: usize = 172_736;
/// Offset of the 32-byte preset name inside the decompressed state.
pub const OFF_PRESET_NAME: usize = 0x4972;
/// Offset of the f32 preset-format version inside the decompressed state.
pub const OFF_VERSION_F32: usize = 0x4994;
/// Offset of the 48-byte author string inside the decompressed state.
pub const OFF_AUTHOR: usize = 0x49A0;
/// Offset of the 48-byte category string inside the decompressed state.
pub const OFF_CATEGORY: usize = 0x49D0;

fn err<T>(msg: impl Into<String>) -> Result<T, String> {
    Err(msg.into())
}

/// Where the Serum chunk was recovered from (for diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// FL Studio VST3 wrapper: `[prologue][chunks..., state = cid 3]`, the
    /// cid-3 payload being the zlib-stream preset chunk.
    FlVst3Wrapper,
    /// A bare zlib-stream chunk stored directly in the state event.
    RawZlib,
    /// A complete `CcnK`/`FPCh` preset embedded in the state.
    CcnKFxPreset,
    /// An `FPCh` preset inside FL's `VstW` VST2 wrapper.
    VstWFxPreset,
}

/// Metadata read from the decompressed 172,736-byte preset state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresetMeta {
    pub preset_name: String,
    pub author: String,
    pub category: String,
    pub version_f32: f32,
}

/// The recovered Serum chunk plus parsed metadata.
#[derive(Debug)]
pub struct Serum1Chunk {
    /// Exact bytes of the fxp `chunk` region: one or more concatenated zlib
    /// streams followed by a `u32 LE` trailer with the compressed size of the
    /// first stream.
    pub chunk: Vec<u8>,
    pub source: SourceKind,
    pub meta: PresetMeta,
    /// Decompressed sizes of every zlib stream (stream 0 = the preset state,
    /// later streams = embedded wavetable / noise data).
    pub stream_sizes: Vec<usize>,
}

/// True when the plugin described by `name`/`filename` is Serum
/// (the synth or "Serum FX"), i.e. not Serum2 and not something unrelated.
pub fn is_serum1(name: &[u8], filename: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name).trim().to_ascii_lowercase();
    let filename = String::from_utf8_lossy(filename)
        .trim()
        .to_ascii_lowercase();
    let basename = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim_end_matches(".vst3")
        .trim_end_matches(".vst")
        .trim_end_matches(".dll")
        .trim_end_matches(".vstpreset")
        .to_ascii_lowercase();

    if name == "serum2" || name.starts_with("serum 2") || basename == "serum2" {
        return false;
    }
    name == "serum"
        || name == "serum fx"
        || name == "serum_x64"
        || basename == "serum"
        || basename == "serum fx"
        || basename == "serum_x64"
}

/// True when the plugin is Serum2 (reported separately, never extracted).
pub fn is_serum2(name: &[u8], filename: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name).trim().to_ascii_lowercase();
    let filename = String::from_utf8_lossy(filename)
        .trim()
        .to_ascii_lowercase();
    let basename = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    name == "serum2" || name.starts_with("serum 2") || basename.starts_with("serum2")
}

/// Basename of a plugin path with known extensions stripped
/// (`.vst3`/`.vst`/`.dll`/`.vstpreset`), lowercased.
pub fn plugin_basename(filename: &[u8]) -> String {
    String::from_utf8_lossy(filename)
        .trim()
        .to_ascii_lowercase()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim_end_matches(".vst3")
        .trim_end_matches(".vst")
        .trim_end_matches(".dll")
        .trim_end_matches(".vstpreset")
        .to_string()
}

/// Serum *synth* only — excludes "Serum FX" and Serum2.
pub fn is_serum1_synth(name: &[u8], filename: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name).trim().to_ascii_lowercase();
    let base = plugin_basename(filename);
    if name == "serum2"
        || name.starts_with("serum 2")
        || base == "serum2"
        || base.starts_with("serum2")
    {
        return false;
    }
    name == "serum" || name == "serum_x64" || base == "serum" || base == "serum_x64"
}

/// True for the "Serum FX" plugin variant (the FX is never converted).
pub fn is_serum_fx(name: &[u8], filename: &[u8]) -> bool {
    String::from_utf8_lossy(name)
        .trim()
        .eq_ignore_ascii_case("serum fx")
        || plugin_basename(filename) == "serum fx"
}

fn parse_meta(state: &[u8]) -> PresetMeta {
    PresetMeta {
        preset_name: crate::core::cstr(state, OFF_PRESET_NAME, 32, true),
        author: crate::core::cstr(state, OFF_AUTHOR, 48, true),
        category: crate::core::cstr(state, OFF_CATEGORY, 48, true),
        version_f32: if state.len() >= OFF_VERSION_F32 + 4 {
            f32::from_le_bytes(
                state[OFF_VERSION_F32..OFF_VERSION_F32 + 4]
                    .try_into()
                    .unwrap(),
            )
        } else {
            0.0
        },
    }
}

/// Walk FL Studio's VST3 wrapper state and return the cid-3 payload
/// (the plugin's own saved state), if the layout matches.
///
/// Acceptance requires the record walk to consume the state exactly, with at
/// least two records including the 64-byte cid-1 header record.
fn fl_vst3_wrapper_cid3(state: &[u8]) -> Option<&[u8]> {
    for start in 0..=8usize {
        let Some(rest) = state.get(start..) else {
            continue;
        };
        let Some(mut recs) = crate::flp::records(rest) else {
            continue;
        };
        let mut found: Option<&[u8]> = None;
        let mut nchunks = 0usize;
        let mut has_cid1 = false;
        for (cid, data) in recs.by_ref() {
            if cid == 1 && data.len() == 64 {
                has_cid1 = true;
            }
            if cid == 3 && found.is_none() {
                found = Some(data);
            }
            nchunks += 1;
        }
        if !recs.overran() && recs.pos() == rest.len() && nchunks >= 2 && has_cid1 {
            return found;
        }
    }
    None
}

/// Extract the `CcnK` preset chunk out of a complete VST2 fxp/fxb blob.
fn chunk_from_ccnk(blob: &[u8]) -> Result<&[u8], String> {
    if blob.len() < 0x3C || &blob[0..4] != b"CcnK" {
        return err("bad CcnK blob");
    }
    let magic = &blob[8..12];
    if magic == b"FPCh" {
        let cs = u32::from_be_bytes(blob[0x38..0x3C].try_into().unwrap()) as usize;
        if 0x3C + cs > blob.len() {
            return err("FPCh chunk overruns blob");
        }
        Ok(&blob[0x3C..0x3C + cs])
    } else if magic == b"FBCh" {
        // Chunk bank: CcnK(4) size(4) FBCh(4) ver(4) uid(4) fxVer(4)
        // numPrograms(4) chunkSize(4) chunk...
        let cs = u32::from_be_bytes(blob[28..32].try_into().unwrap()) as usize;
        if 32 + cs > blob.len() {
            return err("FBCh chunk overruns blob");
        }
        Ok(&blob[32..32 + cs])
    } else {
        err(format!("unsupported fxMagic {:?}", magic))
    }
}

/// Recover the Serum preset chunk from a `PluginParams` state payload.
pub fn serum1_chunk_from_state(state: &[u8]) -> Result<Serum1Chunk, String> {
    if state.is_empty() {
        return err("plugin state is empty");
    }

    let (raw, source): (&[u8], SourceKind) = if let Some(cid3) = fl_vst3_wrapper_cid3(state) {
        if cid3.starts_with(b"XferJson") {
            return err("cid-3 state is Serum2 XferJson (not Serum)");
        }
        (cid3, SourceKind::FlVst3Wrapper)
    } else if state.starts_with(b"VstW") {
        let off = state[..64.min(state.len())]
            .windows(4)
            .position(|w| w == b"CcnK")
            .ok_or_else(|| String::from("VstW wrapper without CcnK blob"))?;
        (chunk_from_ccnk(&state[off..])?, SourceKind::VstWFxPreset)
    } else if state.starts_with(b"CcnK") {
        (chunk_from_ccnk(state)?, SourceKind::CcnKFxPreset)
    } else if state[0] == 0x78 {
        (state, SourceKind::RawZlib)
    } else {
        return err("unrecognized plugin state layout");
    };

    let (streams, has_trailer) = crate::zlibio::split_chunk(raw)?;
    let stream0 = &streams[0];
    if stream0.len() > SERUM1_STATE_SIZE {
        return err(format!(
            "preset state is {} bytes (expected at most {SERUM1_STATE_SIZE})",
            stream0.len()
        ));
    }
    let meta = parse_meta(stream0);

    let mut chunk = raw.to_vec();
    if !has_trailer {
        // Serum2's importer silently rejects chunks without the trailer
        // word; append it (compressed size of stream 0) at the end.
        let (_, s0_len) = crate::zlibio::inflate(raw, crate::zlibio::MAX_STREAM)?;
        chunk.extend_from_slice(&(s0_len as u32).to_le_bytes());
    }

    Ok(Serum1Chunk {
        chunk,
        source,
        meta,
        stream_sizes: streams.iter().map(|s| s.len()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::zlib_stream;

    fn synthetic_state() -> Vec<u8> {
        let mut s0 = vec![0u8; SERUM1_STATE_SIZE];
        s0[OFF_PRESET_NAME..OFF_PRESET_NAME + 5].copy_from_slice(b"Test\0");
        s0[OFF_VERSION_F32..OFF_VERSION_F32 + 4].copy_from_slice(&0.1631f32.to_le_bytes());
        let z0 = zlib_stream(&s0);
        let z1 = zlib_stream(&vec![0u8; 2048 * 4]);
        let mut out = Vec::new();
        out.extend_from_slice(&z0);
        out.extend_from_slice(&z1);
        out.extend_from_slice(&(z0.len() as u32).to_le_bytes());
        out
    }

    #[test]
    fn extracts_from_raw_zlib() {
        let state = synthetic_state();
        let c = serum1_chunk_from_state(&state).unwrap();
        assert_eq!(c.source, SourceKind::RawZlib);
        assert_eq!(c.stream_sizes, vec![SERUM1_STATE_SIZE, 8192]);
        assert_eq!(c.meta.preset_name, "Test");
        assert!((c.meta.version_f32 - 0.1631).abs() < 1e-6);
    }

    #[test]
    fn extracts_from_fl_vst3_wrapper() {
        let cid3 = synthetic_state();
        let mut state = Vec::new();
        state.extend_from_slice(&[1, 0, 0, 0]); // prologue
        // cid 1, 64 bytes
        state.extend_from_slice(&1u32.to_le_bytes());
        state.extend_from_slice(&64u64.to_le_bytes());
        state.extend_from_slice(&[0u8; 64]);
        // cid 3
        state.extend_from_slice(&3u32.to_le_bytes());
        state.extend_from_slice(&(cid3.len() as u64).to_le_bytes());
        state.extend_from_slice(&cid3);
        // cid 4
        state.extend_from_slice(&4u32.to_le_bytes());
        state.extend_from_slice(&8u64.to_le_bytes());
        state.extend_from_slice(&[0u8; 8]);
        let c = serum1_chunk_from_state(&state).unwrap();
        assert_eq!(c.source, SourceKind::FlVst3Wrapper);
        assert_eq!(c.chunk, cid3);
        assert_eq!(c.meta.preset_name, "Test");
    }

    #[test]
    fn detects_serum2_xferjson() {
        let mut cid3 = b"XferJson\0".to_vec();
        cid3.extend_from_slice(&[0u8; 32]);
        let mut state = Vec::new();
        state.extend_from_slice(&[1, 0, 0, 0]);
        state.extend_from_slice(&1u32.to_le_bytes());
        state.extend_from_slice(&64u64.to_le_bytes());
        state.extend_from_slice(&[0u8; 64]);
        state.extend_from_slice(&3u32.to_le_bytes());
        state.extend_from_slice(&(cid3.len() as u64).to_le_bytes());
        state.extend_from_slice(&cid3);
        assert!(serum1_chunk_from_state(&state).is_err());
    }

    #[test]
    fn repairs_missing_trailer() {
        let state = synthetic_state();
        let mut truncated = state.clone();
        truncated.truncate(state.len() - 4);
        let c = serum1_chunk_from_state(&truncated).unwrap();
        assert_eq!(c.chunk, state);
    }

    #[test]
    fn plugin_detection() {
        assert!(is_serum1(
            b"Serum",
            b"/Library/Audio/Plug-Ins/VST3/Serum.vst3"
        ));
        assert!(is_serum1(b"Serum", b"C:\\VST\\Serum_x64.dll"));
        assert!(is_serum1(
            b"Serum FX",
            b"/Library/Audio/Plug-Ins/VST3/Serum FX.vst3"
        ));
        assert!(!is_serum1(
            b"Serum2",
            b"/Library/Audio/Plug-Ins/VST3/Serum2.vst3"
        ));
        assert!(!is_serum1(b"Serum 2", b""));
        assert!(!is_serum1(b"Vital", b"/Vital.vst3"));
        assert!(is_serum2(b"Serum2", b"/Serum2.vst3"));
        assert!(!is_serum2(b"Serum", b"/Serum.vst3"));
    }
}

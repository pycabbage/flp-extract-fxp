//! Meta / globals / name writers: preset + WT/noise names, macro names, the
//! MIDI map, lock bits and Global0 switches, embedded streams/tuning and the
//! top-level meta keys.

use super::Ctx;
use crate::s1state;
use crate::s2tree::Val;

/// Preset name / author / description plus the WT and noise sample names.
pub(super) fn write_names(ctx: &mut Ctx) {
    // ---- preset name / author / description ----
    ctx.root.set(
        "presetName",
        Val::Text(ctx.cstr_at(s1state::OFF_PRESET_NAME, 32)),
    );
    ctx.root.set(
        "presetAuthor",
        Val::Text(ctx.cstr_at(s1state::OFF_AUTHOR, 48)),
    );
    ctx.root.set(
        "presetDescription",
        Val::Text(ctx.cstr_at(s1state::OFF_CATEGORY, 48)),
    );

    // ---- WT / noise names ----
    let wt_a = ctx.cstr_at(s1state::OFF_WT_NAME_A, 512);
    let wt_b = ctx.cstr_at(s1state::OFF_WT_NAME_B, 512);
    let noise = ctx.cstr_at(s1state::OFF_NOISE_NAME, 512);
    for (osc, name) in [(0usize, &wt_a), (1usize, &wt_b)] {
        let sec = format!("Oscillator{osc}");
        let sub = format!("WTOsc{osc}");
        if name == "Audio In" {
            ctx.root
                .obj_at(&sec)
                .obj_at(&sub)
                .set("sampleFromAudioInput", Val::Bool(true));
        } else {
            ctx.root
                .obj_at(&sec)
                .obj_at(&sub)
                .set("relativePathToWT", Val::Text(name.clone()));
        }
        // written only for oscillators with an embedded wavetable
        if ctx.preset.osc_wt_frames()[osc] != 0 {
            let interp = u64::from(ctx.st[s1state::OFF_INTERPOLATE_AFTER_LOAD + osc]);
            ctx.root
                .obj_at(&sec)
                .obj_at(&sub)
                .set("interpolateAfterLoad", Val::UInt(interp));
        }
    }
    if noise == "Audio In" {
        ctx.root
            .obj_at("Oscillator3")
            .obj_at("NoiseOsc3")
            .set("sampleFromAudioInput", Val::Bool(true));
    } else {
        ctx.root
            .obj_at("Oscillator3")
            .obj_at("NoiseOsc3")
            .set("relativePathToNoiseSample", Val::Text(noise));
    }
}

/// Macro names (version > 0.134).
pub(super) fn write_macro_names(ctx: &mut Ctx, ver: f32) {
    if ver <= 0.134 {
        return;
    }
    for k in 0..4usize {
        let raw = ctx.cstr_at(s1state::OFF_MACRO_NAMES + 32 * k, 32);
        let name = if raw.is_empty() {
            format!("Macro {}", k + 1)
        } else {
            raw
        };
        ctx.root
            .obj_at(&format!("Macro{k}"))
            .set("name", Val::Text(name));
    }
}

// MIDI map (version > 0.1299). Only the 247 CC bytes at blob+0x3840 are
// read by the importer (0x4DFD92 loop); the 0x5360 extension is ignored.
pub(super) fn write_midi_map(ctx: &mut Ctx, ver: f32) {
    if ver <= 0.1299 {
        return;
    }
    let mut entries = Val::arr();
    for i in 0..247usize {
        let cc = ctx.st[s1state::OFF_MIDI_MAP + i];
        if cc != 0 && cc < 128 {
            let mut e = Val::obj();
            e.set("ccNum", Val::UInt(u64::from(cc)));
            let mut ids = Val::arr();
            ids.push(Val::UInt(i as u64));
            e.set("paramIDs", ids);
            entries.push(e);
        }
    }
    if !matches!(entries, Val::Array(ref a) if a.is_empty()) {
        let mut mm = Val::obj();
        mm.set("midiMap", entries);
        ctx.root.set("midiMap", mm);
    }
}

/// Lock bits plus the Global0 mono/poly switches (version > 0.147).
pub(super) fn write_globals(ctx: &mut Ctx, ver: f32) {
    // ---- lock bits ----
    let locks = ctx.st[s1state::OFF_LOCK_BITS];
    ctx.root.set("lockOversampling", Val::Bool(locks & 2 != 0));
    ctx.root.set("lockTuning", Val::Bool(locks & 4 != 0));

    // ---- Global0 mono/poly (version > 0.147) ----
    if ver > 0.147 {
        let mono = ctx.f32_at(0x4C58);
        let poly = ctx.f32_at(0x4C74);
        let mono_n = if mono != 0.0 { 1.0 } else { 0.0 };
        let poly_n = ((poly * 31.0) + 0.5).floor() + 1.0;
        let g = ctx.root.obj_at("Global0").obj_at("plainParams");
        g.set("kParamMonoToggle", Val::F64(mono_n));
        g.set("kParamPolyCount", Val::F64(f64::from(poly_n)));
    } else {
        ctx.fn_setval(0x142, 1.0);
        if ver < 0.144 {
            ctx.fn_setval(0x154, 1.0);
        }
    }
}

/// Embedded streams / tuning / loopback64 / boundary64 writers.
pub(super) fn write_embedded_streams(ctx: &mut Ctx) -> Result<(), String> {
    // storedPhasePos (SubOsc4): raw 8-byte value at blob+0x5528, omitted
    // when zero (importer 0x4F4D60). Not written for modern presets, where
    // the same data arrives through the 0x5418 phasor-memory blocks.
    let sub_phase = ctx.u64_at(s1state::OFF_STORED_PHASE_POS);
    if sub_phase != 0 {
        ctx.root
            .obj_at("Oscillator4")
            .obj_at("SubOsc4")
            .set("storedPhasePos", Val::UInt(sub_phase));
    }

    // noise sample: byte count at blob+0x5544; when present, the
    // loopback64/boundary64 words (blob+0x5530/0x5538) accompany it.
    let noise_size = ctx.u32_at(0x5544) as usize;
    if noise_size > 0 {
        for (off, key) in [
            (s1state::OFF_LOOPBACK64, "loopback64"),
            (s1state::OFF_BOUNDARY64, "boundary64"),
        ] {
            let raw = ctx.u64_at(off);
            if raw != 0 {
                ctx.root
                    .obj_at("Oscillator3")
                    .obj_at("NoiseOsc3")
                    .set(key, Val::UInt(raw));
            }
        }
    }

    // tuningData / embedded noise sample: the appended streams carry the
    // wavetable frames first (byte counts at blob+0x4968/0x496C), then the
    // tuning bytes (length at blob+0x53E0), then the noise sample (byte
    // count at blob+0x5544). Verified on a legacy preset embedding a tuning
    // and a noise sample in one stream (tuning text at stream offset 0).
    let tuning = ctx.preset.tuning_bytes();
    let frames: usize = ctx
        .preset
        .osc_wt_frames()
        .iter()
        .map(|f| (*f).max(0) as usize & !3)
        .sum();
    let tuning_len = if tuning.len >= 1 && tuning.len <= 0x8000 {
        tuning.len as usize
    } else {
        0
    };
    if tuning_len > 0 {
        let mut skip = frames;
        let mut data = Val::arr();
        let mut left = tuning_len;
        for s in ctx.streams {
            if left == 0 {
                break;
            }
            if skip >= s.len() {
                skip -= s.len();
                continue;
            }
            let take = left.min(s.len() - skip);
            if let Val::Array(a) = &mut data {
                a.extend(
                    s[skip..skip + take]
                        .iter()
                        .map(|b| Val::UInt(u64::from(*b))),
                );
            }
            skip = 0;
            left -= take;
        }
        ctx.root.set("tuningData", data);
        ctx.root.set("tuningName", Val::Text(tuning.name));
    }

    // embedded noise sample: two deinterleaved halves of f32 samples
    if noise_size > 0 {
        let mut skip = frames + tuning_len;
        let mut collected: Vec<u8> = Vec::new();
        let mut left = noise_size;
        for s in ctx.streams {
            if left == 0 {
                break;
            }
            if skip >= s.len() {
                skip -= s.len();
                continue;
            }
            let take = left.min(s.len() - skip);
            collected.extend_from_slice(&s[skip..skip + take]);
            skip = 0;
            left -= take;
        }
        let half = collected.len() / 2;
        let mut arr = Val::arr();
        for half_idx in 0..2usize {
            let mut sub = Val::arr();
            for chunk in collected[half_idx * half..half_idx * half + half]
                .as_chunks::<4>()
                .0
            {
                sub.push(Val::F64(f64::from(f32::from_le_bytes(*chunk))));
            }
            arr.push(sub);
        }
        let noise_name = crate::core::cstr(
            ctx.st.as_slice(),
            s1state::OFF_NOISE_NAME,
            s1state::NAME_FIELD_LEN,
            true,
        );
        let n3 = ctx.root.obj_at("Oscillator3").obj_at("NoiseOsc3");
        n3.set("embeddedNoiseData", arr);
        n3.set("pathToNoiseSample", Val::Text(noise_name));
    }
    Ok(())
}

/// Meta keys: fileType/vendor/url/product/version/productVersion/serum1*.
pub(super) fn write_meta(ctx: &mut Ctx, ver: f32) {
    ctx.root.set("fileType", Val::Text("SerumPreset".into()));
    ctx.root.set("vendor", Val::Text("Xfer Records".into()));
    ctx.root
        .set("url", Val::Text("https://xferrecords.com/".into()));
    ctx.root.set("product", Val::Text("Serum2".into()));
    ctx.root.set("productVersion", Val::Text("2.0.23".into()));
    ctx.root.set("version", Val::F32(9.0));
    let chunk_ver = ctx.f32_at(s1state::OFF_VERSION_F32);
    ctx.root
        .set("serum1ChunkVersion", Val::F64(f64::from(chunk_ver)));
    ctx.root.set(
        "serum1Version",
        Val::F64(f64::from(serum1_version(chunk_ver, ctx))),
    );
    // ---- MPE state (importer 0x4DD0D6, version > 0.155 only): mpeEnabled
    // from the f32 at blob+0x5414, the three config values from bytes at
    // blob+0x5415..0x5418. Older presets carry no MPE state at all. ----
    if ver > 0.155 {
        let mpe_enabled = ctx.f32_at(0x5414);
        ctx.root.set("mpeEnabled", Val::Bool(mpe_enabled != 0.0));
        ctx.root
            .set("mpeConfig", Val::Int(i64::from(ctx.st[0x5415] as i8)));
        ctx.root.set(
            "mpeGlobalPitchBendRange",
            Val::Int(i64::from(ctx.st[0x5416] as i8)),
        );
        ctx.root.set(
            "mpePitchBendRange",
            Val::Int(i64::from(ctx.st[0x5417] as i8)),
        );
    }
}

/// The `serum1Version` mapping chain (importer 0x4E5C42–0x4E5EEF): above
/// 0.150 the version stored in the state itself (blob+0x5548) is used,
/// otherwise a threshold ladder maps the chunk version onto the Serum 1.x
/// release it corresponds to.
fn serum1_version(v: f32, ctx: &Ctx) -> f32 {
    if v > 0.150 {
        return ctx.f32_at(0x5548);
    }
    const LADDER: [(f32, f32); 14] = [
        (0.131, 1.010),
        (0.133, 1.023),
        (0.134, 1.026),
        (0.135, 1.032),
        (0.136, 1.036),
        (0.137, 1.035),
        (0.138, 1.044),
        (0.139, 1.051),
        (0.141, 1.068),
        (0.142, 1.071),
        (0.143, 1.072),
        (0.144, 1.082),
        (0.146, 1.092),
        (0.147, 1.095),
    ];
    if v <= 0.109_999 {
        return 1.005;
    }
    for (threshold, mapped) in LADDER {
        if v < threshold {
            return mapped;
        }
    }
    if v < 0.148 { 1.103 } else { 1.105 }
}

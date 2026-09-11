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

/// MIDI map (version > 0.1299).
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
    for i in 0..37usize {
        let cc = ctx.st[s1state::OFF_MIDI_MAP_EXTRA + i];
        if cc != 0 && cc < 128 {
            let mut e = Val::obj();
            e.set("ccNum", Val::UInt(u64::from(cc)));
            let mut ids = Val::arr();
            ids.push(Val::UInt((248 + i) as u64));
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
        ctx.fn_setval(0x154, 1.0);
    }
}

/// Embedded streams / tuning / loopback64 / boundary64 writers.
pub(super) fn write_embedded_streams(ctx: &mut Ctx) -> Result<(), String> {
    let tuning = ctx.preset.tuning_bytes();
    if tuning.len >= 1 && tuning.len <= 0x8000 {
        let mut data = Val::Bytes(Vec::new());
        let mut left = tuning.len as usize;
        for s in ctx.streams {
            if left == 0 {
                break;
            }
            let take = left.min(s.len());
            if let Val::Bytes(b) = &mut data {
                b.extend_from_slice(&s[..take]);
            }
            left -= take;
        }
        ctx.root.set("tuningData", data);
        ctx.root.set("tuningName", Val::Text(tuning.name));
    }
    Ok(())
}

/// Meta keys: fileType/vendor/url/product/version/productVersion/serum1*.
pub(super) fn write_meta(ctx: &mut Ctx) {
    ctx.root.set("fileType", Val::Text("SerumPreset".into()));
    ctx.root.set("vendor", Val::Text("Xfer Records".into()));
    ctx.root
        .set("url", Val::Text("https://xferrecords.com/".into()));
    ctx.root.set("product", Val::Text("Serum2".into()));
    ctx.root.set("productVersion", Val::Text("2.0.23".into()));
    ctx.root.set("version", Val::F32(9.0));
    let chunk_ver = f64::from(ctx.f32_at(s1state::OFF_VERSION_F32));
    ctx.root.set("serum1ChunkVersion", Val::F64(chunk_ver));
    ctx.root
        .set("serum1Version", Val::F64(1.368_000_030_517_578_1));
    // mpe fields
    ctx.root.set("mpeEnabled", Val::Int(0));
    ctx.root.set("mpeConfig", Val::Int(0));
    ctx.root.set("mpeGlobalPitchBendRange", Val::Int(2));
    ctx.root.set("mpePitchBendRange", Val::Int(48));
}

//! Serum 1 → Serum 2 preset conversion: a faithful re-implementation of
//! Serum2.vst3 2.0.23's `s1state_load` importer (RVA 0x4DABC0–0x4E61CA).
//!
//! The importer builds a json tree from a parsed Serum 1 preset state; the
//! tree merged over the init-body skeleton (`s2tables::INIT_BODY`, with
//! `mpeEnabled` normalized to `Bool(false)`) must be byte-identical to the
//! committed golden fixtures (`tests/fixtures/golden_s2/*.bin`).
//!
//! Ground truth: `docs/s1-to-s2-mapping.md`, `docs/s2-runtime-tables.md`, the
//! annotated disassembly dumps, and the golden `.bin` fixtures.

use crate::s1state::{self, S1ModSlot, S1Preset};
use crate::s2tables::{S2_PARAM_DESCS, S2ParamDesc};
use crate::s2tree::Val;

/// Human-readable notes about params/records that could not be converted.
#[derive(Debug, Default)]
pub struct ConvertReport {
    pub notes: Vec<String>,
}

/// Final merged CBOR body plus conversion notes.
#[derive(Debug)]
pub struct Converted {
    pub body: Val,
    pub report: ConvertReport,
}

const FX_BASE_IDX: [usize; 10] = [0x60, 0x67, 0x6d, 0x74, 0x7c, 0x87, 0x51, 0x58, 0x8d, 0x93];

/// flag: 0 = synth import; 1 = FX build (drops Oscillator0/1 WTOsc nodes).
pub fn convert_s1_to_s2(preset: &S1Preset, flag: u8) -> Result<Converted, String> {
    if preset.blob.len() != s1state::S1_BLOB_SIZE {
        return Err(format!(
            "unsupported state blob size {} (expected {})",
            preset.blob.len(),
            s1state::S1_BLOB_SIZE
        ));
    }
    if !preset.meta.version_f32.is_finite() || preset.meta.version_f32 < 0.002 {
        return Err("preset is too old for the Serum 2 importer (version < 0.002)".into());
    }
    if preset.meta.version_f32 > 0.999 {
        return Err("preset was made with a newer Serum version".into());
    }
    let ver = preset.meta.version_f32;
    let order = preset.fx_order();
    let mut ctx = Ctx {
        st: preset.blob.clone(),
        root: Val::obj(),
        order,
        notes: Vec::new(),
        streams: &preset.streams,
        preset,
    };

    // ---- defaults fill: state+0x4AE0 f32 0..30 from the runtime table ----
    for i in 0..31usize {
        let d: f32 = if i & 1 == 1 { 1.0 } else { 0.0 };
        ctx.set_f32(s1state::OFF_AUX_PARAMS + 4 * i, d);
    }

    // ---- FX mirror 0x3700 migration (unconditional) ----
    {
        let mut xs = [0f32; 4];
        for (k, x) in xs.iter_mut().enumerate() {
            *x = ctx.f32_at(0x3700 + 4 * k);
        }
        for (k, x) in xs.into_iter().enumerate() {
            let q = ((x * 70.0 + 0.5).trunc()) / 70.0;
            ctx.set_f32(0x3700 + 4 * k, q);
        }
    }

    // ---- defaults push: S2 idx 0x104..0x155 from state+0x4AE0+4i ----
    for i in 0x20usize..0x72 {
        let v = f64::from(ctx.f32_at(s1state::OFF_AUX_PARAMS + 4 * i));
        ctx.fn_setval(i + 0xE4, v);
    }

    // ---- per-FX enable knobs 0x9A+i (version >= 0.05, byte == 1 → 0.0) ----
    if ver >= 0.05 {
        for i in 0..10usize {
            let en = ctx.st[OFF_MOD_BASE + 68 * i];
            if en == 1 {
                ctx.fn_setval(0x9A + i, 0.0);
            }
        }
    }

    // ---- LFO blocks 1..8 + 9..10 (curve writer; modern blocks for 0.1631) ----
    let lfo9_10_alt = (0.148..0.163).contains(&ver);
    for k in 0..8usize {
        let src: Vec<u8> = if ver >= 0.148 {
            preset
                .lfo_block(k)
                .map(|b| b.to_vec())
                .unwrap_or_else(|| vec![0u8; s1state::LFO_BLOCK_SIZE])
        } else {
            let classic = match (k, lfo9_10_alt) {
                (0..=3, _) => preset.classic_lfo_block(k),
                (4..=7, false) => preset.classic_lfo_block(k),
                (4..=7, true) => preset
                    .classic_lfo_5_8_alt()
                    .or_else(|| preset.classic_lfo_block(k)),
                _ => None,
            };
            match classic {
                Some(b) => normalize_classic_lfo(b),
                None => vec![0u8; s1state::LFO_BLOCK_SIZE],
            }
        };
        let node = write_lfo_curve(&ctx, k, &src);
        let key = format!("LFO{k}");
        ctx.root.set(&key, node);
    }
    for k in 0..2usize {
        let src: Vec<u8> = if ver >= 0.148 {
            preset
                .flex_lfo_block(k)
                .map(|b| b.to_vec())
                .unwrap_or_else(|| vec![0u8; s1state::LFO_BLOCK_SIZE])
        } else {
            match preset.classic_lfo_block(4 + k) {
                Some(b) => normalize_classic_lfo(b),
                None => vec![0u8; s1state::LFO_BLOCK_SIZE],
            }
        };
        let node = write_lfo_flex(&ctx, k, &src);
        let key = format!("LFO{}", 8 + k);
        ctx.root.set(&key, node);
    }

    // ---- WTOsc flex curve blocks (embedded default shapes, blob+0x24870) ----
    for k in 0..2usize {
        let base = 0x24870 + k * s1state::LFO_BLOCK_SIZE;
        let block = match ctx.st.get(base..base + s1state::LFO_BLOCK_SIZE) {
            Some(b) => b.to_vec(),
            None => vec![0u8; s1state::LFO_BLOCK_SIZE],
        };
        let mut curve = Val::obj();
        let np = u32::from_le_bytes(block[0x2D08..0x2D0C].try_into().unwrap_or([0; 4])).min(0x1E1);
        curve.set("numPoints", Val::UInt(u64::from(np)));
        let mut cv = Val::arr();
        for i in 0..480usize {
            cv.push(Val::F64(f64::from_le_bytes(
                block[i * 8..i * 8 + 8].try_into().unwrap(),
            )));
        }
        curve.set("curveVals", cv);
        let mut x = Val::arr();
        for i in 0..480usize {
            x.push(Val::F64(f64::from_le_bytes(
                block[0xF00 + i * 8..0xF00 + i * 8 + 8].try_into().unwrap(),
            )));
        }
        curve.set("xVals", x);
        let mut y = Val::arr();
        for i in 0..480usize {
            y.push(Val::F64(f64::from_le_bytes(
                block[0x1E00 + i * 8..0x1E00 + i * 8 + 8]
                    .try_into()
                    .unwrap(),
            )));
        }
        curve.set("yVals", y);
        ctx.root
            .obj_at(&format!("Oscillator{k}"))
            .obj_at(&format!("WTOsc{k}"))
            .set("flex", curve);
    }

    // ---- LFO 8/9 phasor defaults (version > 0.155) ----
    if ver > 0.155 {
        for (k, t) in [(8usize, "Lorenz"), (9, "Rossler")] {
            let pp = ctx.root.obj_at(&format!("LFO{k}")).obj_at("plainParams");
            pp.set("kParamType", Val::Text(t.into()));
            pp.set("kParamDotted", Val::F64(1.0));
            pp.set("kParamTriplets", Val::F64(1.0));
            pp.set("kParamRate10x", Val::F64(1.0));
            pp.set("kParamRate", Val::F64(0.10000069729266844));
            pp.set("kParamBeatSync", Val::F64(0.0));
            pp.set("kParamMono", Val::F64(0.0));
        }
    }

    // ---- detuneFactor / oldSerum1Preset ----
    if ver < 0.149 {
        ctx.root
            .obj_at("Oscillator3")
            .obj_at("NoiseOsc3")
            .set("oldSerum1Preset", Val::Bool(true));
    } else {
        let df = ctx
            .st
            .get(0x5550..0x5558)
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .map_or(0.0, f64::from_le_bytes);
        ctx.root
            .obj_at("Oscillator3")
            .obj_at("NoiseOsc3")
            .set("detuneFactor", Val::F64(df));
    }

    // ---- lfophasor writes (version > 0.161): each env with a value writes
    // `lfophasor` into its family's rack cell; the distortion cell additionally
    // receives a two-entry `flex` array of default phasor curves ----
    if ver > 0.161 {
        let mut any = false;
        for (env, fam) in ENV_FAM_TABLE.iter().enumerate() {
            let v = ctx.f32_at(0x844C + 4 * env);
            if !v.is_finite() || v == 0.0 {
                continue;
            }
            any = true;
            let cell = ctx.order[*fam].clamp(0, 9) as usize;
            let arr = ctx.root.obj_at("FXRack0").arr_at("FX");
            if let Val::Array(a) = arr {
                while a.len() <= cell {
                    a.push(Val::obj());
                }
                let cellnode = &mut a[cell];
                cellnode.set("type", Val::UInt((*fam) as u64));
                let submap = S2_PARAM_DESCS[FX_BASE_IDX[*fam]].submap;
                cellnode
                    .obj_at(submap)
                    .set("lfophasor", Val::F64(f64::from(v.abs())));
            }
        }
        if any {
            let cell = ctx.order[0].clamp(0, 9) as usize;
            let arr = ctx.root.obj_at("FXRack0").arr_at("FX");
            if let Val::Array(a) = arr {
                while a.len() <= cell {
                    a.push(Val::obj());
                }
                let cellnode = &mut a[cell];
                cellnode.set("type", Val::UInt(0));
                cellnode.set("flex", {
                    let mut pair = Val::arr();
                    pair.push(default_phasor_curve());
                    pair.push(default_phasor_curve());
                    pair
                });
            }
        }
        // the "+ FX" hyper/delay phasor state: 2 blocks of 8 f64s at blob+0x8460/0x84A0
        let mut lfo = Val::arr();
        for blk in [0x8460usize, 0x84A0usize] {
            let mut vals = Val::arr();
            for i in 0..8usize {
                let v = ctx
                    .st
                    .get(blk + 8 * i..blk + 8 * i + 8)
                    .and_then(|b| <[u8; 8]>::try_from(b).ok())
                    .map_or(0.0, f64::from_le_bytes);
                vals.push(Val::F64(v));
            }
            lfo.push(vals);
        }
        if ctx.u16_at(0x3BD2) != 0 {
            let cell = ctx.order[9].clamp(0, 9) as usize;
            let arr = ctx.root.obj_at("FXRack0").arr_at("FX");
            if let Val::Array(a) = arr {
                while a.len() <= cell {
                    a.push(Val::obj());
                }
                let cellnode = &mut a[cell];
                cellnode.set("type", Val::UInt(9));
                cellnode.obj_at("FXHyperD").set("lfo", lfo);
            }
        }
    }

    // ---- scalars velo/note (fn_4f3e00) ----
    for kind in 0..2usize {
        let node = write_scalars(&ctx, kind);
        let key = if kind == 0 { "velo" } else { "note" };
        ctx.root.obj_at("scalars").set(key, node);
    }

    // ---- release rescale (state+0x3688: v = (v*999 + 0.1 - 0.1)/999.9) ----
    {
        let v = ctx.f32_at(0x3688);
        let t = ((f64::from(v) * 999.0 + 0.10000000149011612 - 0.10000000149011612)
            / 999.9000244140625) as f32;
        ctx.set_f32(0x3688, t);
    }

    // ---- MASTER PARAM LOOP (blob+0x3460, 0..247) ----
    for i in 0..248usize {
        let mut v = ctx.f32_at(s1state::OFF_MASTER_PARAMS + 4 * i);
        if (ver < 0.008 && i >= 178) || (ver < 0.009 && i >= 180) {
            ctx.fn_setval(i + 4, f64::from(v));
        }
        if v.is_nan() {
            v = 0.0;
            ctx.set_f32(s1state::OFF_MASTER_PARAMS + 4 * i, 0.0);
        }
        if v < 0.0 {
            v = 0.0;
            ctx.set_f32(s1state::OFF_MASTER_PARAMS + 4 * i, 0.0);
        }
        if v > 1.0 {
            v = 1.0;
            ctx.set_f32(s1state::OFF_MASTER_PARAMS + 4 * i, 1.0);
        }
        ctx.fn_setval(i, f64::from(v));
        ctx.fn_setval_special(i, f64::from(v));
        if ver >= 0.1099 && i == 227 {
            ctx.fn_setval(0xE3, 0.5);
        }
    }

    // ---- S1 mod-slot staging + ModSlot node builder ----
    for k in 0..s1state::MOD_SLOT_COUNT {
        let Some(rec) = ctx.preset.mod_slots.iter().find(|s| s.slot as usize == k) else {
            continue;
        };
        let rec = rec.clone();
        let base = if k < 16 {
            40 * k
        } else {
            s1state::MOD_SLOTS_17_32 + 40 * (k - 16)
        };
        if ver < 0.0058 {
            ctx.st[base + 0x22] = k as u8;
            ctx.set_u16(base + 0x20, 0x8080);
        }
        let mut dest = ctx.u16_at(base + 0x1A);
        if dest == 0xDE {
            ctx.set_u16(base + 0x1A, 0xDF);
            dest = 0xDF;
        }
        if ver == 0.008 {
            if dest >= 0xB4 {
                dest += 4;
                ctx.set_u16(base + 0x1A, dest);
            }
            let mut src_a = ctx.u16_at(base + 0x18);
            if src_a >= 0x3A {
                src_a += 3;
                ctx.set_u16(base + 0x18, src_a);
            }
        }
        if ver <= 0.009 {
            if dest >= 0xB4 {
                dest += 2;
                ctx.set_u16(base + 0x1A, dest);
            }
            let mut src_a = ctx.u16_at(base + 0x18);
            if src_a >= 0x41 {
                src_a += 1;
                ctx.set_u16(base + 0x18, src_a);
            }
        }
        if ver < 0.008 && dest >= 0xB2 {
            dest += 1;
            ctx.set_u16(base + 0x1A, dest);
        }
        if ver < 0.007 {
            let mut src_a = ctx.u16_at(base + 0x18);
            if src_a >= 0x2F {
                src_a -= 9;
                ctx.set_u16(base + 0x18, src_a);
            }
        }
        if ver > 0.0299 && dest >= 0xDF {
            dest += 4;
            ctx.set_u16(base + 0x1A, dest);
        }
        let a1 = f64::from((rec.amount_a + 1.0) * 0.5);
        let a2 = f64::from(rec.amount_b);
        if k < 16 {
            ctx.fn_setval(0xB4 + 2 * k, a1);
            ctx.fn_setval(0xB5 + 2 * k, a2);
        } else {
            ctx.fn_setval(0xC4 + 2 * k, a1);
            ctx.fn_setval(0xC5 + 2 * k, a2);
        }

        // source/aux word migrations
        let mut src = ctx.u16_at(base + 0x14);
        let mut aux = ctx.u16_at(base + 0x16);
        if ver < 0.139 {
            if src >= 0xC {
                src += 1;
            }
            if aux >= 0xC {
                aux += 1;
            }
        }
        if ver < 0.13299 {
            if src >= 0x12 {
                src += 2;
            }
            if aux >= 0x12 {
                aux += 2;
            }
        }
        if ver < 0.148 {
            if src >= 9 {
                src += 4;
            }
            if aux >= 9 {
                aux += 4;
            }
        }
        ctx.set_u16(base + 0x14, src);
        ctx.set_u16(base + 0x16, aux);
        if src == 0 && aux == 0 {
            ctx.set_u32(base + 0x18, 0x013C_00AD);
        }
        if ver > 0.155 {
            let src = ctx.u16_at(base + 0x14);
            if src >= 0x1D {
                ctx.set_u16(base + 0x14, 0x21);
            }
            let aux = ctx.u16_at(base + 0x16);
            if aux >= 0x1D {
                ctx.set_u16(base + 0x16, 0x21);
            }
        }

        build_modslot_node(&mut ctx, k, &rec);
    }

    // ---- lfoPointModAssignments (only when the S1 count is nonzero) ----
    let mut lpm = Val::arr();
    for rec in ctx.preset.lfo_point_mods() {
        let mut r = Val::arr();
        r.push(Val::UInt(u64::from(rec.lfo)));
        r.push(Val::UInt(u64::from(rec.point)));
        r.push(Val::UInt(u64::from(rec.mode.min(3))));
        r.push(Val::Int(i64::from(rec.param) - 147));
        lpm.push(r);
    }
    let lpm_empty = matches!(&lpm, Val::Array(a) if a.is_empty());
    if !lpm_empty {
        ctx.root.set("lfoPointModAssignments", lpm);
    }

    // ---- midiMap (version > 0.1299) ----
    if ver > 0.1299 {
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

    // ---- mixOrGain1..10 (version >= 0.05) ----
    if ver >= 0.05 {
        for i in 0..10usize {
            let cell = ctx.order[i].clamp(0, 9) as usize;
            let mix = ctx.st[OFF_MOD_BASE + 68 * i + 2];
            let arr = ctx.root.obj_at("FXRack0").arr_at("FX");
            if let Val::Array(a) = arr
                && a.len() > cell
            {
                a[cell].set("mixOrGain1", Val::Bool(mix != 0));
            }
        }
    }

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

    // ---- embedded WT data ----
    let frames = ctx.preset.osc_wt_frames();
    let mut stream_off = 0usize;
    for (osc, fc) in frames.iter().enumerate() {
        let bytes = (*fc).max(0) as usize & !3;
        let mut data: Vec<f64> = Vec::new();
        let mut left = bytes;
        for s in ctx.streams {
            if stream_off >= s.len() {
                continue;
            }
            if left == 0 {
                break;
            }
            let take = left.min(s.len() - stream_off);
            let mut p = stream_off;
            while p + 4 <= stream_off + take {
                data.push(f64::from(f32::from_le_bytes(
                    s[p..p + 4].try_into().unwrap(),
                )));
                p += 4;
            }
            left -= take;
            stream_off += take;
        }
        if !data.is_empty() {
            let sec = format!("Oscillator{osc}");
            let sub = format!("WTOsc{osc}");
            let mut arr = Val::arr();
            for x in data {
                arr.push(Val::F64(x));
            }
            ctx.root
                .obj_at(&sec)
                .obj_at(&sub)
                .set("embeddedWTData", arr);
        }
    }

    // ---- macro names (version > 0.134) ----
    if ver > 0.134 {
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

    // ---- lock bits ----
    let locks = ctx.st[s1state::OFF_LOCK_BITS];
    ctx.root.set("lockOversampling", Val::Bool(locks & 2 != 0));
    ctx.root.set("lockTuning", Val::Bool(locks & 4 != 0));

    // ---- appended streams / embedded data / tuning ----
    write_embedded_streams(&mut ctx)?;

    // ---- WTOsc overview tags (version > 0.03) ----
    if ver > 0.03 {
        for osc in 0..2usize {
            let v = f64::from(ctx.f32_at(0x4998 + 4 * osc));
            let mut cell = Val::obj();
            cell.set("kUIParamWTOverviewMouseTag", Val::F64(v));
            let arr = ctx.root.arr_at("WTOsc");
            while matches!(arr, Val::Array(a) if a.len() <= osc) {
                arr.push(Val::obj());
            }
            if let Val::Array(a) = arr {
                a[osc] = cell;
            }
        }
    }

    // ---- defaults 0x14C/0x14D = 1.0, 0x14E/0x14F = 0.0 (version > 0.146) ----
    if ver > 0.146 {
        // exact-descriptor writes (bypassing the master-loop drop set):
        ctx.root
            .obj_at("Oscillator0")
            .obj_at("plainParams")
            .set("kParamUnisonRange", Val::F64(2.0));
        ctx.root
            .obj_at("Oscillator1")
            .obj_at("plainParams")
            .set("kParamUnisonRange", Val::F64(2.0));
        ctx.fn_setval(0x14E, 0.0);
        ctx.fn_setval(0x14F, 0.0);
    }

    // ---- storedPhasePos (version > 0.147) ----
    if ver > 0.147 {
        for idx in 0..2usize {
            let base = 0x5418 + idx * 0x88;
            let mut arr = Val::arr();
            for w in 0..17usize {
                let v = ctx.u32_at(base + 4 * w);
                arr.push(Val::UInt(u64::from(v)));
            }
            let sec = format!("WTOsc{idx}");
            let osc = format!("Oscillator{idx}");
            ctx.root
                .obj_at(&osc)
                .obj_at(&sec)
                .set("storedPhasePos", arr);
        }
        ctx.root
            .obj_at("Oscillator4")
            .obj_at("SubOsc4")
            .set("storedPhasePos", Val::Int(0));
    }

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

    // ---- FX dead-cell masking (version > 0.05) ----
    if ver > 0.05 {
        let mut flags = [0u8; 10];
        for (k, flag) in flags.iter_mut().enumerate().take(8) {
            *flag = u8::from(ctx.u16_at(0x396E + 68 * k) != 0);
        }
        flags[8] = u8::from(ctx.u16_at(0x3B8E) != 0);
        flags[9] = u8::from(ctx.u16_at(0x3BD2) != 0);
        for k in 0..s1state::MOD_SLOT_COUNT {
            let base = if k < 16 {
                40 * k
            } else {
                s1state::MOD_SLOTS_17_32 + 40 * (k - 16)
            };
            let d = ctx.u16_at(base + 0x1A) as usize;
            if let Some(desc) = S2_PARAM_DESCS.get(d)
                && desc.fxtype >= 0
                && (desc.fxtype as usize) < 10
            {
                flags[desc.fxtype as usize] = 1;
            }
        }
        let arr = ctx.root.obj_at("FXRack0").arr_at("FX");
        if let Val::Array(a) = arr {
            let mut kept: Vec<(usize, Val)> = a
                .iter()
                .enumerate()
                .filter(|(pos, cell)| {
                    let fam = cell
                        .get("type")
                        .and_then(|t| t.as_i64())
                        .map(|t| t as usize)
                        .unwrap_or(*pos);
                    fam >= 10 || flags[fam] != 0
                })
                .map(|(pos, c)| {
                    let fam = c
                        .get("type")
                        .and_then(|t| t.as_i64())
                        .map(|t| t as usize)
                        .unwrap_or(pos);
                    (ctx.order[fam.min(9)].max(0) as usize, c.clone())
                })
                .collect();
            kept.sort_by_key(|(cell, _)| *cell);
            a.clear();
            a.extend(kept.into_iter().map(|(_, c)| c));
        }
    }

    // ---- FX cell finalize: type + missing submap keys ----
    finalize_fx_cells(&mut ctx);

    // ---- ModSlot post-passes ----
    post_pass_1(&mut ctx);
    post_pass_2(&mut ctx);

    // ---- envelope loop (kParamAmount = residue·100) ----
    env_loop(&mut ctx);

    // ---- VoiceFilter rewrite (pid 1 → 8) ----
    post_pass_2b(&mut ctx);

    // ---- VoiceFilter0 wet / levelOut ----
    {
        let v = ctx.f32_at(0x4C94);
        let vf = ctx.root.obj_at("VoiceFilter0").obj_at("plainParams");
        vf.set("kParamWet", Val::F64(100.0));
        if v != 0.0 {
            vf.set("kParamLevelOut", Val::F64(f64::from(v.sqrt() * 0.05)));
        }
    }

    // ---- Global0 S1Compatibility / LimitSameNotePolyphony ----
    {
        let g = ctx.root.obj_at("Global0").obj_at("plainParams");
        g.set("kParamS1Compatibility", Val::Int(1));
        g.set("kParamLimitSameNotePolyphony", Val::Int(1));
    }

    // ---- meta ----
    write_meta(&mut ctx);

    if flag != 0 {
        // FX-build variant: the WTOsc nodes are not emitted
        for osc in 0..2usize {
            let key = format!("Oscillator{osc}");
            if let Some(Val::Map(m)) = ctx.root.get_mut(&key) {
                m.retain(|(k, _)| !k.starts_with("WTOsc"));
            }
        }
    }

    let root = match std::mem::replace(&mut ctx.root, Val::obj()) {
        Val::Map(entries) => entries,
        _ => Vec::new(),
    };

    // ---- merge over the init-body skeleton (top-level overlay) ----
    let mut body = crate::s2tree::decode_cbor(crate::s2tables::INIT_BODY)?;
    if let Val::Map(m) = &mut body {
        for (k, v) in root {
            m.retain(|(ek, _)| ek != &k);
            m.push((k, v));
        }
        for (k, v) in m.iter_mut() {
            if k == "mpeEnabled" {
                *v = Val::Bool(false);
            }
        }
    }
    Ok(Converted {
        body,
        report: ConvertReport { notes: ctx.notes },
    })
}

const OFF_MOD_BASE: usize = 0x3976;
const ENV_PARAM_IDX: [usize; 4] = [3, 4, 16, 17];
const ENV_SCALES: [f32; 4] = [8.0, 24.0, 8.0, 24.0];
/// env → FX family (i32 table @0xA553C0).
const ENV_FAM_TABLE: [usize; 3] = [1, 2, 3];

struct Ctx<'a> {
    st: Vec<u8>,
    root: Val,
    order: [i32; 10],
    notes: Vec<String>,
    streams: &'a [Vec<u8>],
    preset: &'a S1Preset,
}

impl<'a> Ctx<'a> {
    fn f32_at(&self, off: usize) -> f32 {
        self.st
            .get(off..off + 4)
            .map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()))
    }
    fn set_f32(&mut self, off: usize, v: f32) {
        if let Some(slot) = self.st.get_mut(off..off + 4) {
            slot.copy_from_slice(&v.to_le_bytes());
        }
    }
    fn u16_at(&self, off: usize) -> u16 {
        self.st
            .get(off..off + 2)
            .map_or(0, |b| u16::from_le_bytes(b.try_into().unwrap()))
    }
    fn set_u16(&mut self, off: usize, v: u16) {
        if let Some(slot) = self.st.get_mut(off..off + 2) {
            slot.copy_from_slice(&v.to_le_bytes());
        }
    }
    fn u32_at(&self, off: usize) -> u32 {
        self.st
            .get(off..off + 4)
            .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn set_u32(&mut self, off: usize, v: u32) {
        if let Some(slot) = self.st.get_mut(off..off + 4) {
            slot.copy_from_slice(&v.to_le_bytes());
        }
    }
    fn cstr_at(&self, off: usize, len: usize) -> String {
        match self.st.get(off..off + len) {
            None => String::new(),
            Some(field) => {
                let nul = field.iter().position(|&b| b == 0).unwrap_or(field.len());
                String::from_utf8_lossy(&field[..nul]).into_owned()
            }
        }
    }

    /// fn_4d9c50: quantize the 0..1 fraction per descriptor class.
    fn unit_convert(&self, idx: usize, v: f64) -> f64 {
        let (m, n) = match idx {
            3 | 16 | 171 | 178 | 179 => (8.0, 8.0),
            4 | 17 => (24.0, 24.0),
            44 | 142 => (95.0, 95.0),
            94 | 95 | 102 | 131 | 326 => (2.0, 2.0),
            99 => (15.0, 15.0),
            166 | 167 | 332 | 333 => (48.0, 48.0),
            168 | 169 => (23.0, 22.0),
            170 => (5.0, 5.0),
            320 | 321 => (4.0, 4.0),
            329 => (31.0, 31.0),
            341 => (1.0, 1.0),
            331 => return if 0.01 < v { 1.0 } else { 0.0 },
            _ => return v,
        };
        (v * m + 0.5).floor() / n
    }

    /// fn_4d9da0 semantics: clamp, mode transform, option snap.
    fn s2_value(&self, idx: usize, v_in: f64) -> Val {
        let Some(d) = S2_PARAM_DESCS.get(idx) else {
            return Val::F64(0.0);
        };
        let mut v_raw = v_in;
        if !v_raw.is_finite() || (v_raw != 0.0 && v_raw.abs() < 2.225_073_858_507_201_4e-308) {
            v_raw = 0.0;
        }
        let v3 = if v_raw.is_nan() { 0.0 } else { v_raw.min(1.0) };
        let upper = d.min.max(d.max);
        let lower = d.min.min(d.max);
        let v = if d.max < d.min { 1.0 - v3 } else { v3 };
        let out = match d.mode {
            0 => v,
            1 => {
                let v2 = if d.step != 1.0 { v.powf(d.step) } else { v };
                if d.cnt == 0 {
                    lower + v2 * (upper - lower)
                } else {
                    let t = (v2 * (f64::from(d.cnt) + 1.0)).trunc();
                    let t = t.min(f64::from(d.cnt));
                    lower + t
                }
            }
            2 => lower * (upper / lower).powf(v),
            3 => (d.max * d.min) / (upper - v * (upper - lower)),
            _ => 0.0,
        };
        if !d.options.is_empty() {
            let i = out.trunc() as i64;
            let i = i.clamp(0, d.options.len() as i64 - 1);
            return Val::Text(d.options[i as usize].to_string());
        }
        Val::F64(out)
    }

    /// fn_setval: master/aux/mod parameter write.
    fn fn_setval(&mut self, idx: usize, v: f64) {
        if idx > 342 {
            return;
        }
        if (315..=342).contains(&idx) {
            let bit = idx - 315;
            if idx == 330 || (0x0840_0003u32 >> bit) & 1 == 1 {
                return;
            }
        }
        let v = self.unit_convert(idx, v);
        if idx == 330 {
            return;
        }
        let Some(d) = S2_PARAM_DESCS.get(idx) else {
            return;
        };
        if d.kname.is_none() {
            return;
        }
        let kname = d.kname.unwrap();
        let value = if (d.fxtype == 0 && idx == 0x64) || (d.fxtype == 8 && idx == 0x8F) {
            let mut out = self.s2_value(idx, v / 1.06875);
            if let Val::F64(x) = &mut out {
                *x /= 1.0;
            }
            out
        } else if (40..=44).contains(&idx) {
            // globals branch: clamp to 1/3 first; the runtime option order for
            // RoutingSlot is [Master, Filter, Direct, None] (pointer order).
            let v = v.min(1.0 / 3.0);
            match self.s2_value(idx, v) {
                Val::Text(t) => {
                    if t == "kRoutingDestMaster" {
                        Val::Text("kRoutingDestFilter".into())
                    } else if t == "kRoutingDestFilter" {
                        Val::Text("kRoutingDestMaster".into())
                    } else {
                        Val::Text(t)
                    }
                }
                other => other,
            }
        } else {
            self.s2_value(idx, v)
        };
        if d.fxtype >= 0 {
            let cell = self.order[d.fxtype as usize].clamp(0, 9) as usize;
            let submap = S2_PARAM_DESCS[FX_BASE_IDX[d.fxtype as usize]].submap;
            let arr = self.root.obj_at("FXRack0").arr_at("FX");
            if let Val::Array(a) = arr {
                while a.len() <= cell {
                    a.push(Val::obj());
                }
                let cellnode = &mut a[cell];
                cellnode.set("type", Val::UInt(d.fxtype as u64));
                cellnode
                    .obj_at(submap)
                    .obj_at("plainParams")
                    .set(kname, value);
            }
        } else {
            let node = self.section(d);
            node.obj_at("plainParams").set(kname, value);
        }
    }

    /// fn_setval's FX special cases (§3.2 formula table).
    fn fn_setval_special(&mut self, idx: usize, v: f64) {
        if idx > 342 {
            return;
        }
        if (315..=342).contains(&idx) {
            let bit = idx - 315;
            if idx == 330 || (0x0840_0003u32 >> bit) & 1 == 1 {
                return;
            }
        }
        let v = self.unit_convert(idx, v);
        let Some(d) = S2_PARAM_DESCS.get(idx) else {
            return;
        };
        if d.fxtype < 0 {
            return;
        }
        let fam = d.fxtype as usize;
        let cell = self.order[fam].clamp(0, 9) as usize;
        let submap = S2_PARAM_DESCS[FX_BASE_IDX[fam]].submap;
        let kname = match d.kname {
            Some(k) => k,
            None => return,
        };
        let get_cell_type = |ctx: &Ctx| -> Option<String> {
            let arr = ctx.root.get("FXRack0")?.get("FX")?;
            match arr {
                Val::Array(a) if a.len() > cell => a[cell]
                    .get("kParamType")
                    .and_then(|t| t.as_str())
                    .map(String::from),
                _ => None,
            }
        };
        match (fam, idx) {
            (6, 0x53) => {
                // kParamType == "kPlate" gate, then kParamPreDelay
                if get_cell_type(self).as_deref() != Some("kPlate") {
                    return;
                }
                let mut value = self.s2_value(idx, v);
                if let Val::F64(x) = &mut value {
                    *x *= 0.001;
                }
                let arr = self.root.obj_at("FXRack0").arr_at("FX");
                if let Val::Array(a) = arr
                    && a.len() > cell
                {
                    a[cell]
                        .obj_at(submap)
                        .obj_at("plainParams")
                        .set("kParamPreDelay", value);
                }
            }
            (0, 0x63) => {
                if get_cell_type(self).as_deref() != Some("kDiode1") {
                    return;
                }
                let mut value = self.s2_value(idx, v);
                if let Val::F64(x) = &mut value {
                    *x = *x * 0.875 + 12.5;
                }
                let arr = self.root.obj_at("FXRack0").arr_at("FX");
                if let Val::Array(a) = arr
                    && a.len() > cell
                {
                    a[cell]
                        .obj_at(submap)
                        .obj_at("plainParams")
                        .set("kParamDrive", value);
                }
            }
            (5, 0x87) => {
                let arr = self.root.obj_at("FXRack0").arr_at("FX");
                if let Val::Array(a) = arr
                    && a.len() > cell
                {
                    let p = a[cell].obj_at(submap).obj_at("plainParams");
                    p.set("kParamGain0", Val::F64(4.6));
                    p.set("kParamGain2", Val::F64(4.6));
                    p.set("kParamRatioBelow", Val::F64(0.75));
                    p.set("kParamCompensatedWetDry", Val::F64(0.0));
                }
            }
            (8, 0x8f) | (0, 0x64) => {
                let value = self.s2_value(idx, v / 1.06875);
                let arr = self.root.obj_at("FXRack0").arr_at("FX");
                if let Val::Array(a) = arr
                    && a.len() > cell
                {
                    a[cell]
                        .obj_at(submap)
                        .obj_at("plainParams")
                        .set(kname, value);
                }
            }
            (7, 0x5c) => {
                let value = Val::F32(v as f32);
                // the special write targets the FXPhaser rack cell (order[2])
                let pcell = self.order[2].clamp(0, 9) as usize;
                let arr = self.root.obj_at("FXRack0").arr_at("FX");
                if let Val::Array(a) = arr {
                    while a.len() <= pcell {
                        a.push(Val::obj());
                    }
                    let phaser = "FXPhaser";
                    a[pcell].set("type", Val::UInt(2));
                    a[pcell]
                        .obj_at(phaser)
                        .obj_at("plainParams")
                        .set("kParamDepth2", value);
                }
            }
            _ => {}
        }
    }

    /// Section node for a non-FX descriptor.
    fn section(&mut self, d: &S2ParamDesc) -> &mut Val {
        if matches!(d.submap, "WTOsc" | "NoiseOsc" | "SubOsc") {
            let sec = format!("Oscillator{}", d.inst);
            let sub = format!("{}{}", d.submap, d.inst);
            self.root.obj_at(&sec).obj_at(&sub)
        } else {
            let key = format!("{}{}", d.submap, d.inst);
            self.root.obj_at(&key)
        }
    }
}

/// Classic (pre-0.148) LFO block → modern 0x2D28 layout (fn_4f1eb0).
fn normalize_classic_lfo(classic: &[u8]) -> Vec<u8> {
    if classic.len() < 0x1890 {
        return vec![0u8; s1state::LFO_BLOCK_SIZE];
    }
    let mut out = vec![0u8; s1state::LFO_BLOCK_SIZE];
    for (src, dst) in [0x0000usize, 0x0520, 0x0A40].iter().enumerate() {
        let _ = (src, dst);
    }
    // three f64 arrays (12 x 65 f64 with 520-byte stride)
    for arr in 0..3 {
        for p in 0..65usize {
            let src = arr * 0x520 + p * 8;
            let dst = arr * 0x0F00 + p * 8;
            if src + 8 <= classic.len() && dst + 8 <= out.len() {
                out[dst..dst + 8].copy_from_slice(&classic[src..src + 8]);
            }
        }
    }
    let take = |off: usize| -> u8 { classic.get(off).copied().unwrap_or(0) };
    let rate = f32::from_le_bytes(
        classic
            .get(0x1864..0x1868)
            .and_then(|b| b.try_into().ok())
            .unwrap_or([0; 4]),
    );
    let put = |out: &mut Vec<u8>, off: usize, b: &[u8]| {
        if off + b.len() <= out.len() {
            out[off..off + b.len()].copy_from_slice(b);
        }
    };
    put(&mut out, 0x2D00, &[take(0x1874)]);
    put(&mut out, 0x2D01, &[take(0x1878)]);
    put(&mut out, 0x2D02, &[take(0x187C)]);
    put(&mut out, 0x2D03, &[take(0x1880)]);
    put(&mut out, 0x2D04, &[take(0x1884)]);
    put(&mut out, 0x2D05, &[take(0x1888)]);
    put(&mut out, 0x2D06, &[take(0x188C)]);
    out[0x2D08..0x2D0C].copy_from_slice(&(take(0x1860) as u32).to_le_bytes());
    let phase = u32::from_le_bytes(
        classic
            .get(0x18A0..0x18A4)
            .and_then(|b| b.try_into().ok())
            .unwrap_or([0; 4]),
    );
    out[0x2D0C..0x2D10].copy_from_slice(&phase.to_le_bytes());
    let lb = i32::from_le_bytes(
        classic
            .get(0x18A4..0x18A8)
            .and_then(|b| b.try_into().ok())
            .unwrap_or([0; 4]),
    );
    out[0x2D10..0x2D14].copy_from_slice(&lb.to_le_bytes());
    out[0x2D14..0x2D18].copy_from_slice(&rate.to_le_bytes());
    for (src, dst) in [
        (0x18C0usize, 0x2D18usize),
        (0x18D0, 0x2D1C),
        (0x18E0, 0x2D20),
    ] {
        let v = f32::from_le_bytes(
            classic
                .get(src..src + 4)
                .and_then(|b| b.try_into().ok())
                .unwrap_or([0; 4]),
        );
        out[dst..dst + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

/// The default phasor curve block (blob+0x24870) as a curve object.
fn default_phasor_curve() -> Val {
    let mut curve = Val::obj();
    curve.set("numPoints", Val::UInt(1));
    let mut cv = Val::arr();
    for _ in 0..480usize {
        cv.push(Val::F64(0.4999999701976776));
    }
    curve.set("curveVals", cv);
    let mut x = Val::arr();
    x.push(Val::F64(0.0));
    for _ in 0..479usize {
        x.push(Val::F64(1.0));
    }
    curve.set("xVals", x);
    let mut y = Val::arr();
    y.push(Val::F64(1.0));
    y.push(Val::F64(0.0));
    for _ in 0..478usize {
        y.push(Val::F64(1.0));
    }
    curve.set("yVals", y);
    curve
}

/// fn_4f2a70 curve writer for LFO 0..7 (pass-through f64 curve copies).
fn write_lfo_curve(ctx: &Ctx, _k: usize, block: &[u8]) -> Val {
    let mut node = Val::obj();
    let mut curve = Val::obj();
    let np_raw = u32::from_le_bytes(
        block
            .get(0x2D08..0x2D0C)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .unwrap_or([0; 4]),
    );
    let np = np_raw.min(0x1E1);
    curve.set("numPoints", Val::Int(i64::from(np)));
    let mut cv = Val::arr();
    for i in 0..480usize {
        let v = f64::from_le_bytes(block[i * 8..i * 8 + 8].try_into().unwrap());
        cv.push(Val::F64(v));
    }
    curve.set("curveVals", cv);
    let mut xv: Vec<f64> = (0..480usize)
        .map(|i| f64::from_le_bytes(block[0xF00 + i * 8..0xF00 + i * 8 + 8].try_into().unwrap()))
        .collect();
    if (np as usize) < 480 {
        xv[np as usize] = 1.0;
    }
    let mut x = Val::arr();
    for v in xv {
        x.push(Val::F64(v));
    }
    curve.set("xVals", x);
    let mut y = Val::arr();
    for i in 0..480usize {
        let v = f64::from_le_bytes(
            block[0x1E00 + i * 8..0x1E00 + i * 8 + 8]
                .try_into()
                .unwrap(),
        );
        y.push(Val::F64(v));
    }
    curve.set("yVals", y);
    let lb = i32::from_le_bytes(
        block
            .get(0x2D10..0x2D14)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .unwrap_or([0; 4]),
    );
    curve.set("loopbackPointNum", Val::Int(i64::from(lb)));
    node.set("curveData", curve);

    let f32_at = |off: usize| -> f32 {
        block
            .get(off..off + 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map_or(0.0, f32::from_le_bytes)
    };
    let pp = node.obj_at("plainParams");
    pp.set("kParamRate", ctx.s2_value(0x3D, f64::from(f32_at(0x2D14))));
    pp.set(
        "kParamSmooth",
        ctx.s2_value(0xDF, f64::from(f32_at(0x2D18))),
    );
    pp.set("kParamRise", ctx.s2_value(0x111, f64::from(f32_at(0x2D20))));
    pp.set(
        "kParamDelay",
        ctx.s2_value(0x119, f64::from(f32_at(0x2D1C))),
    );
    let mode_byte = block.get(0x2D04).copied().unwrap_or(0) as i8;
    let env_byte = block.get(0x2D05).copied().unwrap_or(0);
    let sync = block.get(0x2D01).copied().unwrap_or(0);
    if env_byte != 0 {
        pp.set("kParamMode", Val::Text("Envelope".into()));
    } else {
        pp.set("kParamMode", Val::Text("Free".into()));
    }
    pp.set("kParamBeatSync", Val::F64(f64::from(sync ^ 1)));
    let anchored = block.get(0x2D00).copied().unwrap_or(0);
    let dotted = block.get(0x2D02).copied().unwrap_or(0) as i8;
    let triplets = block.get(0x2D03).copied().unwrap_or(0) as i8;
    if !(sync == 1 && mode_byte == 0 && env_byte == 1) {
        pp.set("kParamAnchored", Val::F64(f64::from(anchored)));
    }
    pp.set("kParamDotted", Val::F64(f64::from(dotted)));
    pp.set("kParamTriplets", Val::F64(f64::from(triplets)));
    let phase = u32::from_le_bytes(
        block
            .get(0x2D0C..0x2D10)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .unwrap_or([0; 4]),
    );
    if (1..=0x1DF).contains(&phase) {
        let i = phase as usize;
        let pv = f64::from_le_bytes(block[0xF00 + i * 8..0xF00 + i * 8 + 8].try_into().unwrap());
        pp.set("kParamPhase", Val::F64(pv));
    }
    node
}

/// LFO 8/9 flex writer (no curveData in the importer output).
fn write_lfo_flex(_ctx: &Ctx, _k: usize, _block: &[u8]) -> Val {
    Val::obj()
}

/// fn_4f3e00 scalars writer (velo/note curve nodes).
fn write_scalars(ctx: &Ctx, kind: usize) -> Val {
    let base = s1state::OFF_SCALARS;
    let sub = kind * 0x88;
    let blk = kind * 0x200;
    let f32_at = |off: usize| -> f64 {
        ctx.st
            .get(base + off..base + off + 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map_or(0.0, |b| f64::from(f32::from_le_bytes(b)))
    };
    let f64_at = |off: usize| -> f64 {
        ctx.st
            .get(base + off..base + off + 8)
            .and_then(|b| <[u8; 8]>::try_from(b).ok())
            .map_or(0.0, f64::from_le_bytes)
    };
    let mut curve = Val::obj();
    let mut cv = Val::arr();
    for i in 0..17usize {
        cv.push(Val::F64(f64_at(sub + 0x400 + 8 * i)));
    }
    curve.set("curveVals", cv);
    let mut table = Val::arr();
    for i in 0..128usize {
        table.push(Val::F64(f32_at(blk + 4 * i)));
    }
    curve.set("table", table);
    let mut xv = Val::arr();
    for i in 0..17usize {
        xv.push(Val::F64(f64_at(sub + 0x510 + 8 * i)));
    }
    curve.set("xVals", xv);
    let mut yv = Val::arr();
    for i in 0..17usize {
        yv.push(Val::F64(f64_at(sub + 0x620 + 8 * i)));
    }
    curve.set("yVals", yv);
    let np = (ctx.st.get(base + 0x730 + kind).copied().unwrap_or(0) as i8) as u32 as u64;
    curve.set("numPoints", Val::UInt(np));
    let legato = f32_at(0x738 + 4 * kind);
    curve.set("legato", Val::Bool(legato != 0.143_000_006_675_720_21));
    curve
}

/// ModSlot node builder (§4.2).
fn build_modslot_node(ctx: &mut Ctx, k: usize, rec: &S1ModSlot) {
    let key = format!("ModSlot{k}");
    let node = ctx.root.obj_at(&key);
    let mut pair = Val::arr();
    let t = usize::from(rec.source_t);
    let remap = crate::s2tables::AUX_REMAP.get(t).copied().unwrap_or(0);
    pair.push(Val::UInt(u64::from(remap)));
    pair.push(Val::UInt(u64::from(rec.aux)));
    node.set("source", pair);
    let d = rec.dest as usize;
    let _ = d;
    // kParamAmount / kParamOut come from the staged fn_setval writes
    // (master params 180..247 mirror the same amounts); the curve defaults
    // are the importer's plain zeros.
    {
        let pp = node.obj_at("plainParams");
        pp.set("kParamAuxCurve", Val::F64(0.0));
        pp.set("kParamBipolar", Val::F64(0.0));
        pp.set("kParamCurveIn", Val::F64(0.0));
    }
    if d == 0x13C {
        node.set("destModuleParamID", Val::UInt(4_294_967_295));
        return;
    }
    let Some(desc) = S2_PARAM_DESCS.get(d) else {
        node.set("destModuleParamID", Val::UInt(4_294_967_295));
        return;
    };
    node.set(
        "destModuleParamID",
        Val::UInt(u64::from(u32::try_from(desc.pid).unwrap_or(0))),
    );
    node.set("destModuleTypeString", Val::Text(desc.submap.into()));
    if desc.fxtype >= 0 {
        let cell = ctx.order[desc.fxtype as usize];
        node.set("destModuleID", Val::Int(i64::from(cell)));
    } else {
        node.set("destModuleID", Val::Int(i64::from(desc.inst)));
    }
}

/// Post-pass 1: kParamOut rescales + destModuleID flagging.
fn post_pass_1(ctx: &mut Ctx) {
    for k in 0..s1state::MOD_SLOT_COUNT {
        let key = format!("ModSlot{k}");
        let node = match ctx.root.get_mut(&key) {
            Some(n) => n,
            None => continue,
        };
        let type_str = node
            .get("destModuleTypeString")
            .and_then(|v| v.as_str())
            .map(String::from);
        let pid = node.get("destModuleParamID").and_then(|v| v.as_i64());
        let Some(type_str) = type_str else { continue };
        if type_str == "Oscillator" {
            if pid == Some(3) {
                let out = node
                    .obj_at("plainParams")
                    .get("kParamOut")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                node.obj_at("plainParams")
                    .set("kParamOut", Val::F64(out * 8.0 / 9.0));
            } else if pid == Some(4) {
                let out = node
                    .obj_at("plainParams")
                    .get("kParamOut")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                node.obj_at("plainParams")
                    .set("kParamOut", Val::F64(out * 24.0 / 25.0));
            }
        }
    }
}

/// Post-pass 2: VoiceFilter pid 1 → 8.
fn post_pass_2(ctx: &mut Ctx) {
    for k in 0..s1state::MOD_SLOT_COUNT {
        let key = format!("ModSlot{k}");
        let node = match ctx.root.get_mut(&key) {
            Some(n) => n,
            None => continue,
        };
        let is_vf =
            node.get("destModuleTypeString").and_then(|v| v.as_str()) == Some("VoiceFilter");
        if is_vf && node.get("destModuleParamID").and_then(|v| v.as_i64()) == Some(1) {
            node.set("destModuleParamID", Val::Int(8));
        }
    }
}

/// Envelope loop: kParamAmount = residue·100 into the env dest slot.
fn env_loop(ctx: &mut Ctx) {
    for env in 0..4usize {
        let idx = ENV_PARAM_IDX[env];
        let scale = ENV_SCALES[env];
        let x = f64::from(ctx.f32_at(s1state::OFF_MASTER_PARAMS + 4 * idx));
        let xf = x * f64::from(scale) + 0.5;
        let f = xf.floor();
        let residue = xf / (f64::from(scale) + 1.0) - f / f64::from(scale);
        if residue == 0.0 {
            continue;
        }
        // find the last ModSlot whose destModuleParamID == -1
        let mut slot: Option<usize> = None;
        for k in 0..s1state::MOD_SLOT_COUNT {
            let key = format!("ModSlot{k}");
            let pid = ctx
                .root
                .get(&key)
                .and_then(|n| n.get("destModuleParamID"))
                .and_then(|v| v.as_i64());
            if pid == Some(-1) {
                slot = Some(k);
            }
        }
        let Some(k) = slot else { continue };
        let key = format!("ModSlot{k}");
        let d = &S2_PARAM_DESCS[idx];
        let node = ctx.root.obj_at(&key);
        node.set(
            "destModuleParamID",
            Val::UInt(u64::try_from(d.pid).unwrap_or(0)),
        );
        node.set("destModuleTypeString", Val::Text(d.submap.into()));
        node.set("destModuleID", Val::Int(i64::from(d.inst)));
        node.obj_at("plainParams")
            .set("kParamAmount", Val::F64(residue * 100.0));
        let mut pair = Val::arr();
        pair.push(Val::Int(38));
        pair.push(Val::Int(i64::from(k as u32)));
        node.set("source", pair);
    }
}

/// VoiceFilter post-rewrite (pid 1 → 8) after the env loop.
fn post_pass_2b(ctx: &mut Ctx) {
    post_pass_2(ctx);
}

/// finalize FX cells: ensure "type" and "mixOrGain1" exist per cell.
fn finalize_fx_cells(ctx: &mut Ctx) {
    let arr = ctx.root.obj_at("FXRack0").arr_at("FX");
    if let Val::Array(a) = arr {
        for cell in a.iter_mut() {
            let sub = cell
                .get("type")
                .and_then(|t| t.as_i64())
                .map(|t| t as usize)
                .unwrap_or(0);
            if sub >= 10 {
                continue;
            }
            let submap = S2_PARAM_DESCS[FX_BASE_IDX[sub]].submap;
            if cell.get(submap).is_none() {
                cell.set(submap, Val::obj());
            }
        }
    }
}

/// Embedded streams / tuning / loopback64 / boundary64 writers.
fn write_embedded_streams(ctx: &mut Ctx) -> Result<(), String> {
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
fn write_meta(ctx: &mut Ctx) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fxp_chunk(bytes: &[u8]) -> Vec<u8> {
        assert_eq!(&bytes[..4], b"CcnK");
        let cs = u32::from_be_bytes(bytes[0x38..0x3C].try_into().unwrap()) as usize;
        bytes[0x3C..0x3C + cs].to_vec()
    }

    fn preset(nn: u8) -> S1Preset {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "\\tests\\fixtures\\serina1\\0");
        let full = format!("{path}{nn}.fxp");
        let bytes = std::fs::read(&full).expect("fixture fxp");
        s1state::parse_preset(&fxp_chunk(&bytes)).expect("parse preset")
    }

    fn golden(nn: u8) -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "\\tests\\fixtures\\golden_s2\\0"
        );
        let full = format!("{path}{nn}_processor_state.bin");
        std::fs::read(&full).expect("golden fixture")
    }

    fn diff_leaves(a: &Val, b: &Val, path: String, out: &mut Vec<(String, String, String)>) {
        match (a, b) {
            (Val::Map(ma), Val::Map(mb)) => {
                for (k, va) in ma {
                    match mb.iter().find(|(k2, _)| k2 == k) {
                        Some((_, vb)) => diff_leaves(va, vb, format!("{path}.{k}"), out),
                        None => {
                            out.push((format!("{path}.{k}"), "present".into(), "absent".into()))
                        }
                    }
                }
                for (k, _) in mb {
                    if !ma.iter().any(|(k2, _)| k2 == k) {
                        out.push((format!("{path}.{k}"), "absent".into(), "present".into()));
                    }
                }
            }
            (Val::Array(aa), Val::Array(ab)) => {
                if aa.len() != ab.len() {
                    out.push((
                        format!("{path}[len]"),
                        format!("{}", aa.len()),
                        format!("{}", ab.len()),
                    ));
                }
                for (i, (va, vb)) in aa.iter().zip(ab.iter()).enumerate() {
                    diff_leaves(va, vb, format!("{path}[{i}]"), out);
                }
            }
            _ => {
                if format!("{a:?}") != format!("{b:?}") {
                    out.push((path, format!("{a:?}"), format!("{b:?}")));
                }
            }
        }
    }

    #[test]
    fn golden_byte_identical_01() {
        golden_one(1);
    }
    #[test]
    fn golden_byte_identical_02() {
        golden_one(2);
    }
    #[test]
    fn golden_byte_identical_03() {
        golden_one(3);
    }
    #[test]
    fn golden_byte_identical_04() {
        golden_one(4);
    }
    #[test]
    fn golden_byte_identical_05() {
        golden_one(5);
    }

    fn golden_one(nn: u8) {
        let preset = preset(nn);
        let conv = convert_s1_to_s2(&preset, 0).expect("convert");
        let record = crate::serum2state::build_processor_record(&conv.body);
        let want = golden(nn);
        let (_, _, _, foff) = crate::serum2state::parse_xfer_json(&record).unwrap();
        let (_, _, _, woff) = crate::serum2state::parse_xfer_json(&want).unwrap();
        // Container fields must agree byte-for-byte; the JSON `hash` differs by
        // design (it is the md5 of the frame, and the golden frame is a real
        // zstd stream while build_processor_record emits a raw-block frame).
        // Container header must match except the frame md5 (`hash` field):
        // the golden frame is a real zstd stream, build_processor_record emits
        // a raw-block frame, so the md5 necessarily differs.
        let (m_json, m_uncomp, m_fmt, _) = crate::serum2state::parse_xfer_json(&record).unwrap();
        let (w_json, w_uncomp, w_fmt, _) = crate::serum2state::parse_xfer_json(&want).unwrap();
        let strip = |j: &str| -> String {
            let a = j.find("\"hash\":\"").unwrap() + 8;
            let b = j[a..].find('"').unwrap() + a;
            format!("{}{}", &j[..a], &j[b..])
        };
        assert_eq!(strip(&m_json), strip(&w_json), "container header {nn}");
        // The decisive check: the decoded CBOR body is byte-identical to the golden's.
        let a = decode_frame(&record[foff..]);
        let b = decode_frame(&want[woff..]);
        let my_cbor = crate::s2tree::encode_cbor(&a);
        let want_cbor = crate::s2tree::encode_cbor(&b);
        assert_eq!(my_cbor, want_cbor, "cbor body {nn}");
        assert_eq!(m_uncomp, w_uncomp, "uncompressed size {nn}");
        assert_eq!(m_fmt, w_fmt, "format {nn}");
        // Tree-level diff diagnostics on failure.
        let mut diffs = Vec::new();
        diff_leaves(&a, &b, String::new(), &mut diffs);
        assert!(
            diffs.is_empty(),
            "leaf diffs on {nn}: {:?}",
            &diffs[..8.min(diffs.len())]
        );
    }

    fn decode_frame(frame: &[u8]) -> Val {
        use std::io::Read;
        let mut dec = ruzstd::decoding::StreamingDecoder::new(std::io::Cursor::new(frame))
            .expect("zstd init");
        let mut out = Vec::new();
        dec.read_to_end(&mut out).expect("zstd read");
        crate::s2tree::decode_cbor(&out).expect("cbor")
    }

    #[test]
    fn report_sanity_05() {
        let preset = preset(5);
        let conv = convert_s1_to_s2(&preset, 0).expect("convert");
        eprintln!("notes: {:?}", conv.report.notes);
        assert!(conv.report.notes.is_empty(), "unexpected notes");
    }

    #[test]
    fn flag_semantics_fx_build() {
        let preset = preset(1);
        let conv = convert_s1_to_s2(&preset, 1).expect("convert");
        let osc0 = conv.body.get("Oscillator0").unwrap();
        assert!(osc0.get("WTOsc0").is_none(), "FX build drops WTOsc0");
    }
}

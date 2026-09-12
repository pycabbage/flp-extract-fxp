//! Serum → Serum2 preset conversion: a faithful re-implementation of
//! Serum2.vst3 2.0.23's `s1state_load` importer (RVA 0x4DABC0–0x4E61CA).
//!
//! The importer builds a json tree from a parsed Serum preset state; the
//! tree merged over the init-body skeleton (`s2tables::INIT_BODY`, with
//! `mpeEnabled` normalized to `Bool(false)`) is byte-identical to the golden
//! processor records the real importer produces. The golden fixtures are not
//! tracked in the repo (third-party preset content); the byte-identity tests
//! skip when they are absent. How they are produced: docs/flp-conversion.md.
//!
//! Ground truth: `docs/s1-to-s2-mapping.md` and `docs/s2-runtime-tables.md`.
//!
//! Submodules: [`params`] (per-index value machinery), [`modmatrix`]
//! (mod-slot staging/node building/env loop/post-passes), [`fxrack`] (FX rack
//! cell handling), [`lfo`] (LFO/phasor/scalars curve writers), [`meta`]
//! (meta/globals/name writers), [`tests`].

mod fxrack;
mod lfo;
mod meta;
mod modmatrix;
mod params;

use crate::s1state::{self, S1Preset};
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
const OFF_MOD_BASE: usize = 0x3976;
/// env → FX family (i32 table @0xA553C0).
const ENV_FAM_TABLE: [usize; 3] = [1, 2, 3];

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
        return Err("preset is too old for the Serum2 importer (version < 0.002)".into());
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

    // ---- FX mirror 0x3700 migration (importer 0x4DB5EF, version < 0.0599
    // only; modern blobs already carry quantized values) ----
    if ver < 0.0599 {
        let mut xs = [0f32; 4];
        for (k, x) in xs.iter_mut().enumerate() {
            *x = ctx.f32_at(0x3700 + 4 * k);
        }
        for (k, x) in xs.into_iter().enumerate() {
            let q = ((x * 70.0 + 0.5).trunc()) / 70.0;
            ctx.set_f32(0x3700 + 4 * k, q);
        }
    }

    // ---- delay-time re-quantization for master params 44/142 (importer
    // 0x4DD1DE–0x4DD26A, version < 0.155; K = 89 below 0.142, else 88) ----
    if ver < 0.155 {
        let k = if ver < 0.142 { 88.0f32 } else { 89.0f32 };
        for idx in [44usize, 142usize] {
            let off = s1state::OFF_MASTER_PARAMS + 4 * idx;
            let v = ctx.f32_at(off);
            let t = (v * k + 0.5).trunc() / 95.0;
            ctx.set_f32(off, t);
        }
    }

    // ---- switch-region migration (importer 0x4DC094–0x4DD2DD). The 2015-era
    // global-switch block is rebuilt through a version-dependent chain of
    // region shifts whose static thread is not fully recoverable; the
    // effective mappings below were calibrated against the real importer
    // (see docs/flp-conversion.md "Legacy presets"). Class A covers
    // 0.147..0.148 (constant defaults), class B 0.148..0.158 moves the
    // pre-migration aux slots 45..71 up by 43 slots (verified on the linear
    // observables: global tuning and poly count). ----
    if (0.147..0.148).contains(&ver) {
        const CLASS_A: [(usize, f32); 15] = [
            (40, 0.5), // FXFilter stereo 50
            (41, 1.0), // FXComp wet 100
            (43, 0.5), // FXComp thresh 100
            (44, 0.5),
            (89, 0.5), // RoutingSlot3 -> Direct
            (90, 0.5), // global tuning 440 Hz
            (91, 0.25),
            (94, 0.5),      // mono on
            (95, 0.5),      // legato on
            (96, 0.5),      // portamento always on
            (98, 0.25),     // oversampling 1x
            (101, 0.468),   // polyphony 16
            (104, 0.03125), // unison range
            (105, 0.03125),
            (113, 0.5), // FXReverb type kHall
        ];
        for (i, v) in CLASS_A {
            ctx.set_f32(s1state::OFF_AUX_PARAMS + 4 * i, v);
        }
    } else if (0.148..0.158).contains(&ver) {
        let mut moved = [0f32; 28]; // pre slots 44..71
        for (k, slot) in moved.iter_mut().enumerate() {
            *slot = ctx.f32_at(s1state::OFF_AUX_PARAMS + 4 * (44 + k));
        }
        ctx.set_f32(s1state::OFF_AUX_PARAMS + 4 * 87, moved[0]); // 87 <- 44
        for i in 88..=114usize {
            ctx.set_f32(s1state::OFF_AUX_PARAMS + 4 * i, moved[i - 87]);
        }
    }

    // ---- per-FX level-out migration + defaults (importer 0x4DD2DD–0x4DD69D).
    // Below 0.159 the pre-0.148 FX param slots (classic LFO region tails
    // 0x1B50/0x1B60 and, from 0.148, the blob tail 0x6E28/0x6E38) are copied
    // into the aux mirror at 0x4B94..0x4BD4; below 0.16 the ten per-FX
    // level-out slots at 0x4BD4+4i are reset to defaults (0.5); below 0.158
    // sixteen more aux slots at 0x4BFC+4i are reset (constants live in
    // runtime-relocated .data, derived empirically against the real
    // importer's output — see docs/flp-conversion.md). ----
    if ver < 0.159 {
        let copy = |ctx: &mut Ctx, src: usize, dst: usize| {
            let b: [u8; 16] = ctx.st[src..src + 16].try_into().unwrap();
            ctx.st[dst..dst + 16].copy_from_slice(&b);
        };
        copy(&mut ctx, 0x1B60, 0x4B94);
        if ver >= 0.148 {
            copy(&mut ctx, 0x6E38, 0x4BA4);
        }
        copy(&mut ctx, 0x1B50, 0x4BB4);
        if ver >= 0.148 {
            copy(&mut ctx, 0x6E28, 0x4BC4);
        }
    }
    if ver < 0.16 {
        for k in 0..10usize {
            ctx.set_f32(0x4BD4 + 4 * k, 0.5);
        }
        if ver < 0.158 {
            // sixteen aux defaults (importer 0x4DD565; the constants live in
            // runtime-relocated .data — all zero per the real importer's
            // LFOPointModBus output on legacy presets).
            for k in 0..16usize {
                ctx.set_f32(0x4BFC + 4 * k, 0.0);
            }
        }
    }

    // ---- release rescale (state+0x3688: v = (v*999 + 0.1 - 0.1)/999.9) ----
    {
        let v = ctx.f32_at(0x3688);
        let t = ((f64::from(v) * 999.0 + 0.10000000149011612 - 0.10000000149011612)
            / 999.9000244140625) as f32;
        ctx.set_f32(0x3688, t);
    }

    // ---- defaults push: S2 idx 0x104..0x155 from state+0x4AE0+4i
    // (importer 0x4DD705, after the level-out migration above) ----
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

    // ---- LFO blocks 1..8 + 9..10 (curve writer; source selected by
    // version: modern graph blocks at blob+0x84E0 for >= 0.162, classic
    // regions normalized through fn_4f1eb0 below that, and pure default
    // blocks for LFO 5-8 below 0.148) ----
    for k in 0..8usize {
        let src: Vec<u8> = if ver >= 0.162 {
            preset
                .lfo_block(k)
                .map(|b| b.to_vec())
                .unwrap_or_else(|| vec![0u8; s1state::LFO_BLOCK_SIZE])
        } else if ver >= 0.148 || k < 4 {
            match preset.classic_lfo_region(k, ver >= 0.148) {
                Some(r) => lfo::normalize_classic_lfo(r, k & 3),
                None => lfo::default_classic_block(false),
            }
        } else {
            lfo::default_classic_block(false)
        };
        let node = lfo::write_lfo_curve(&ctx, k, &src);
        let key = format!("LFO{k}");
        ctx.root.set(&key, node);
    }
    for k in 0..2usize {
        // LFO 9/10 flex blocks: the importer normalizes the classic region
        // into blob+0x1EE20 (flag=1 prefill) but fn_4f2a70's flag=1 mode
        // emits nothing into the tree for them — only the phasor defaults
        // below (importer 0x4DB024, unconditional) are visible.
        let src: Vec<u8> = if ver >= 0.162 {
            preset
                .flex_lfo_block(k)
                .map(|b| b.to_vec())
                .unwrap_or_else(|| vec![0u8; s1state::LFO_BLOCK_SIZE])
        } else {
            vec![0u8; s1state::LFO_BLOCK_SIZE]
        };
        let node = lfo::write_lfo_flex(&ctx, k, &src);
        let key = format!("LFO{}", 8 + k);
        ctx.root.set(&key, node);
    }

    // ---- WTOsc flex curve blocks: the init-body skeleton carries the
    // default phasor shapes; they are only replaced when the preset embeds
    // flex data at blob+0x24870 (legacy blobs do not). ----
    for k in 0..2usize {
        let base = 0x24870 + k * s1state::LFO_BLOCK_SIZE;
        let block = match ctx.st.get(base..base + s1state::LFO_BLOCK_SIZE) {
            Some(b) => b.to_vec(),
            None => vec![0u8; s1state::LFO_BLOCK_SIZE],
        };
        let empty = block.iter().all(|&b| b == 0);
        let mut curve = if empty {
            // empty block: the importer's default single-point phasor shape
            lfo::default_phasor_curve_legacy()
        } else {
            Val::obj()
        };
        if !empty {
            let np =
                u32::from_le_bytes(block[0x2D08..0x2D0C].try_into().unwrap_or([0; 4])).min(0x1E1);
            curve.set("numPoints", Val::UInt(u64::from(np)));
        }
        if !empty {
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
        }
        ctx.root
            .obj_at(&format!("Oscillator{k}"))
            .obj_at(&format!("WTOsc{k}"))
            .set("flex", curve);
    }

    // ---- LFO 8/9 phasor defaults (unconditional; importer 0x4DB024) ----
    lfo::write_phasor_defaults(&mut ctx);

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
    // `lfophasor` into its family's rack cell; the distortion cell
    // additionally receives a two-entry `flex` array of default phasor
    // curves. The flex pair itself is written for every version (importer
    // 0x4DDD5D block; observed on 0.147-0.153 presets). ----
    {
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
                let (a, b) = if ver > 0.161 {
                    (lfo::default_phasor_curve(), lfo::default_phasor_curve())
                } else {
                    (
                        lfo::default_phasor_curve_legacy(),
                        lfo::default_phasor_curve_legacy(),
                    )
                };
                pair.push(a);
                pair.push(b);
                pair
            });
        }
    }
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
                let submap = crate::s2tables::S2_PARAM_DESCS[FX_BASE_IDX[*fam]].submap;
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
                    pair.push(lfo::default_phasor_curve());
                    pair.push(lfo::default_phasor_curve());
                    pair
                });
            }
        }
        // the "+ FX" hyper/delay phasor state: 2 blocks of 8 f64s at blob+0x8460/0x84A0
        let mut lfoarr = Val::arr();
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
            lfoarr.push(vals);
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
                cellnode.obj_at("FXHyperD").set("lfo", lfoarr);
            }
        }
    }

    // ---- scalars velo/note (fn_4f3e00) ----
    for kind in 0..2usize {
        let node = lfo::write_scalars(&ctx, kind);
        let key = if kind == 0 { "velo" } else { "note" };
        ctx.root.obj_at("scalars").set(key, node);
    }

    // ---- MASTER PARAM LOOP (blob+0x3460, 0..247) ----
    for i in 0..248usize {
        let mut v = ctx.f32_at(s1state::OFF_MASTER_PARAMS + 4 * i);
        // legacy (< 0.162) distortion-mode menu: kDownsample sat at index 10
        // (after kAsym/kRectify) before the menu was reordered to index 8
        if ver < 0.162 && i == 99 {
            let j = (v * 15.0).round();
            let j = match j as i32 {
                8 => 9.0,
                9 => 10.0,
                10 => 8.0,
                _ => j,
            };
            v = j / 15.0;
            ctx.set_f32(s1state::OFF_MASTER_PARAMS + 4 * i, v);
        }
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
    modmatrix::stage_mod_slots(&mut ctx, ver);

    // ---- lfoPointModAssignments (only when the S1 count is nonzero) ----
    modmatrix::write_lfo_point_mods(&mut ctx);

    // ---- midiMap (version > 0.1299) ----
    meta::write_midi_map(&mut ctx, ver);

    // ---- mixOrGain1..10 (version >= 0.159, importer 0x4E00FD) ----
    fxrack::write_mix_or_gain(&mut ctx, ver);

    // ---- per-FX gain: S2 idx 0x121+i <- f32 blob+0x4BD4+4i, unconditional
    // (importer 0x4E0275-0x4E02A3; also mirrors 0x4BF0 -> 0x3BA4 each pass) ----
    {
        let mirror = ctx.f32_at(0x4BF0);
        ctx.set_f32(0x3BA4, mirror);
        for i in 0..10usize {
            let v = ctx.f32_at(0x4BD4 + 4 * i);
            ctx.fn_setval(0x121 + i, f64::from(v));
        }
    }

    // ---- preset name / author / description + WT / noise names ----
    meta::write_names(&mut ctx);

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
    meta::write_macro_names(&mut ctx, ver);

    // ---- lock bits + Global0 mono/poly (version > 0.147) ----
    meta::write_globals(&mut ctx, ver);

    // ---- appended streams / embedded data / tuning ----
    meta::write_embedded_streams(&mut ctx)?;

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
        if ctx.u64_at(s1state::OFF_STORED_PHASE_POS) == 0 {
            ctx.root
                .obj_at("Oscillator4")
                .obj_at("SubOsc4")
                .set("storedPhasePos", Val::Int(0));
        }
    }

    // ---- FX dead-cell masking (version > 0.05) ----
    fxrack::mask_dead_fx_cells(&mut ctx, ver);

    // ---- FX cell finalize: type + missing submap keys ----
    fxrack::finalize_fx_cells(&mut ctx);
    modmatrix::remap_dest_module_ids(&mut ctx);

    // ---- ModSlot post-passes ----
    modmatrix::post_pass_1(&mut ctx);
    modmatrix::post_pass_2(&mut ctx);
    modmatrix::post_pass_3(&mut ctx);

    // ---- envelope loop (kParamAmount = residue·100) ----
    modmatrix::env_loop(&mut ctx);

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
    meta::write_meta(&mut ctx, ver);

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
    fn u64_at(&self, off: usize) -> u64 {
        self.st
            .get(off..off + 8)
            .map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn set_u32(&mut self, off: usize, v: u32) {
        if let Some(slot) = self.st.get_mut(off..off + 4) {
            slot.copy_from_slice(&v.to_le_bytes());
        }
    }
    fn cstr_at(&self, off: usize, len: usize) -> String {
        crate::core::cstr(&self.st, off, len, false)
    }
}

#[cfg(test)]
mod tests;

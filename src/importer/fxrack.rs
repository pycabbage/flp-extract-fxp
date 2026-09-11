//! FX rack cell handling: mixOrGain writes, dead-cell masking (rack
//! inversion) and the per-cell finalize pass.

use super::Ctx;
use super::FX_BASE_IDX;
use crate::s1state;
use crate::s2tables::S2_PARAM_DESCS;
use crate::s2tree::Val;

/// mixOrGain1..10 (version >= 0.05).
pub(super) fn write_mix_or_gain(ctx: &mut Ctx, ver: f32) {
    if ver < 0.05 {
        return;
    }
    for i in 0..10usize {
        let cell = ctx.order[i].clamp(0, 9) as usize;
        let mix = ctx.st[super::OFF_MOD_BASE + 68 * i + 2];
        let arr = ctx.root.obj_at("FXRack0").arr_at("FX");
        if let Val::Array(a) = arr
            && a.len() > cell
        {
            a[cell].set("mixOrGain1", Val::Bool(mix != 0));
        }
    }
}

/// Dead-cell masking (version > 0.05): drop rack cells whose FX family is
/// disabled in the S1 state, re-sorting the kept cells by rack position.
pub(super) fn mask_dead_fx_cells(ctx: &mut Ctx, ver: f32) {
    if ver <= 0.05 {
        return;
    }
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

/// finalize FX cells: ensure "type" and "mixOrGain1" exist per cell.
pub(super) fn finalize_fx_cells(ctx: &mut Ctx) {
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

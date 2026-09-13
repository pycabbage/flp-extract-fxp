//! Modulation matrix: S1 mod-slot staging, the ModSlot node builder, the
//! envelope loop and the ModSlot post-passes, plus the
//! lfoPointModAssignments writer.

use super::Ctx;
use crate::s1state::{self, S1ModSlot};
use crate::s2tables::S2_PARAM_DESCS;
use crate::s2tree::Val;

const ENV_PARAM_IDX: [usize; 4] = [3, 4, 16, 17];
const ENV_SCALES: [f32; 4] = [8.0, 24.0, 8.0, 24.0];

/// Stage every S1 mod-slot record: apply version-dependent word migrations
/// to the state blob, mirror the amounts into the staging params, then build
/// the ModSlot node.
pub(super) fn stage_mod_slots(ctx: &mut Ctx, ver: f32) {
    for k in 0..s1state::MOD_SLOT_COUNT {
        let Some(rec) = ctx.preset.mod_slots.iter().find(|s| s.slot as usize == k) else {
            continue;
        };
        let mut rec = rec.clone();
        let base = if k < 16 {
            40 * k
        } else {
            s1state::MOD_SLOTS_17_32 + 40 * (k - 16)
        };
        if ver < 0.0058 {
            ctx.st[base + 0x22] = k as u8;
            ctx.set_u16(base + 0x20, 0x8080);
        }
        // importer 0x4DC0B9 (0.12999 <= ver < 0.148): slots whose dest code
        // is 0xE4 or higher predate the current destination table and are
        // restamped with the dead defaults (src_a 173, dest 316); the
        // 0.148..0.151 window (0x4DC618) restamps at 0x10C instead.
        let dead_threshold = if ver < 0.148 {
            Some(0xE4)
        } else if (0.148..0.151).contains(&ver) {
            Some(0x10C)
        } else {
            None
        };
        if let Some(threshold) = dead_threshold
            && ctx.u16_at(base + 0x1A) >= threshold
        {
            ctx.set_u32(base + 0x18, 0x013C_00AD);
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
        if ver < 0.0299 && dest >= 0xDF {
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

        // source/aux word migrations (0x4DF0A5: < 0.137 +1 at 0xC;
        // 0x4DF0D3: < 0.14 +2 at 0x12; 0x4DF103: < 0.148 +4 at 9)
        let mut src = ctx.u16_at(base + 0x14);
        let mut aux = ctx.u16_at(base + 0x16);
        if ver < 0.137 {
            if src >= 0xC {
                src += 1;
            }
            if aux >= 0xC {
                aux += 1;
            }
        }
        if ver < 0.14 {
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
        // the node builder reads the post-migration codes
        rec.source_t = ctx.u16_at(base + 0x14);
        rec.aux = ctx.u16_at(base + 0x16);
        rec.dest = ctx.u16_at(base + 0x1A);

        build_modslot_node(ctx, k, &rec);
    }
}

/// Post-pass 3: ModSlot kParamOut rescales for FX-family destinations
/// (importer 0x4E4B4B–0x4E581C).
pub(super) fn post_pass_3(ctx: &mut Ctx) {
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
        let Some(out) = node
            .obj_at("plainParams")
            .get("kParamOut")
            .and_then(|v| v.as_f64())
        else {
            continue;
        };
        let factor = match (type_str.as_deref(), pid) {
            (Some("FXFilter"), Some(3)) => Some(1.0 / 1.06875),
            (Some("FXDistortion"), Some(5)) => Some(1.0 / 1.06875),
            (Some("None"), Some(28)) => Some(0.5),
            _ => None,
        };
        if let Some(f) = factor {
            node.obj_at("plainParams")
                .set("kParamOut", Val::F64(out * f));
        }
    }
}

/// lfoPointModAssignments (only written when the S1 count is nonzero).
pub(super) fn write_lfo_point_mods(ctx: &mut Ctx) {
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
}

/// ModSlot node builder (§4.2).
fn build_modslot_node(ctx: &mut Ctx, k: usize, rec: &S1ModSlot) {
    let key = format!("ModSlot{k}");
    // importer 0x4DF3B1: template(0xA20B70, signed bipolar byte); aux-invert
    // and bypass flags from the aux words at +0x1C/+0x1E
    let bbase = if k < 16 {
        40 * k
    } else {
        s1state::MOD_SLOTS_17_32 + 40 * (k - 16)
    };
    let bipolar = ctx.st[bbase + 0x0C] as i8;
    let w = ctx.u16_at(bbase + 0x1C);
    let w2 = ctx.u16_at(bbase + 0x1E);
    let node = ctx.root.obj_at(&key);
    let mut pair = Val::arr();
    let t = usize::from(rec.source_t);
    let remap = crate::s2tables::AUX_REMAP.get(t).copied().unwrap_or(0);
    pair.push(Val::UInt(u64::from(remap)));
    let a = usize::from(rec.aux);
    let aux_remap = crate::s2tables::AUX_REMAP.get(a).copied().unwrap_or(0);
    pair.push(Val::UInt(u64::from(aux_remap)));
    node.set("source", pair);
    let d = rec.dest as usize;
    let _ = d;
    // kParamAmount / kParamOut come from the staged fn_setval writes
    // (master params 180..247 mirror the same amounts); the curve defaults
    // are the importer's plain zeros.
    {
        let pp = node.obj_at("plainParams");
        pp.set("kParamAuxCurve", Val::F64(0.0));
        pp.set("kParamBipolar", Val::F64(f64::from(bipolar)));
        pp.set("kParamCurveIn", Val::F64(0.0));
        // importer 0x4DF3F9/0x4DF472: aux-invert / bypass flags
        if w == 1 || (w == 2 && w2 == 1) {
            pp.set("kParamAuxInverted", Val::F64(1.0));
        }
        if w == 2 {
            pp.set("kParamBypass", Val::F64(1.0));
        }
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
pub(super) fn post_pass_1(ctx: &mut Ctx) {
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
pub(super) fn post_pass_2(ctx: &mut Ctx) {
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

/// Per-env enable flags (importer 0x4E295C/0x4E30BF/0x4E34B9): the flag for
/// env `2m` is raised by a ModSlot with an Oscillator destination on module
/// `m` and paramID 3 (the kParamOut ·8/9 rescale branch), the flag for env
/// `2m+1` by paramID 4 (the ·24/25 branch). Envs with a lowered flag are
/// skipped even when their amount residue is nonzero.
fn env_flags(ctx: &Ctx) -> [bool; 4] {
    let mut flags = [false; 4];
    for k in 0..s1state::MOD_SLOT_COUNT {
        let Some(node) = ctx.root.get(&format!("ModSlot{k}")) else {
            continue;
        };
        if node.get("destModuleTypeString").and_then(|v| v.as_str()) != Some("Oscillator") {
            continue;
        }
        let m = match node.get("destModuleID").map(|v| v.as_i64()) {
            Some(Some(m)) if (0..2).contains(&m) => m as usize,
            _ => continue,
        };
        match node.get("destModuleParamID").and_then(|v| v.as_i64()) {
            Some(3) => flags[2 * m] = true,
            Some(4) => flags[2 * m + 1] = true,
            _ => {}
        }
    }
    flags
}

/// Post-pass 4: rewrite FX-family `destModuleID` values from the raw rack
/// order cell to the final (post-compaction) rack index (the importer reads
/// the cell index out of the finalized `FXRack0.FX` array, whose empty cells
/// are dropped by the dead-cell masking pass).
pub(super) fn remap_dest_module_ids(ctx: &mut Ctx) {
    let mut cell_of_submap: Vec<(String, i64)> = Vec::new();
    if let Some(Val::Array(a)) = ctx.root.get("FXRack0").and_then(|r| r.get("FX")) {
        for (pos, cell) in a.iter().enumerate() {
            let fam = cell
                .get("type")
                .and_then(|t| t.as_i64())
                .map(|t| t as usize)
                .unwrap_or(usize::MAX);
            if fam < 10 {
                cell_of_submap.push((
                    S2_PARAM_DESCS[super::FX_BASE_IDX[fam]].submap.to_string(),
                    pos as i64,
                ));
            }
        }
    }
    for k in 0..s1state::MOD_SLOT_COUNT {
        let key = format!("ModSlot{k}");
        let Some(node) = ctx.root.get_mut(&key) else {
            continue;
        };
        let Some(type_str) = node
            .get("destModuleTypeString")
            .and_then(|v| v.as_str())
            .map(String::from)
        else {
            continue;
        };
        if let Some((_, pos)) = cell_of_submap.iter().find(|(s, _)| *s == type_str) {
            node.set("destModuleID", Val::Int(*pos));
        }
    }
}

/// Envelope loop: kParamAmount = residue·100 into the env dest slot.
pub(super) fn env_loop(ctx: &mut Ctx) {
    let flags = env_flags(ctx);
    for env in 0..4usize {
        let idx = ENV_PARAM_IDX[env];
        let scale = ENV_SCALES[env];
        // f32 arithmetic throughout (importer 0x4E3578–0x4E35B7)
        let x = ctx.f32_at(s1state::OFF_MASTER_PARAMS + 4 * idx);
        let xf = x.mul_add(scale, 0.5);
        let f = xf.floor();
        let residue = f64::from(xf / (scale + 1.0)) - f64::from(f / scale);
        if residue == 0.0 || !flags[env] {
            continue;
        }
        // find the first ModSlot whose destModuleParamID == -1 (0xFFFFFFFF);
        // envs allocate slots sequentially
        let mut slot: Option<usize> = None;
        for k in 0..s1state::MOD_SLOT_COUNT {
            let key = format!("ModSlot{k}");
            let pid = ctx
                .root
                .get(&key)
                .and_then(|n| n.get("destModuleParamID"))
                .and_then(|v| match v {
                    Val::UInt(u) if *u == u64::from(u32::MAX) => Some(-1i64),
                    other => other.as_i64(),
                });
            if pid == Some(-1) {
                slot = Some(k);
                break;
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
        // the env write replaces the whole plainParams set (importer
        // 0x4E3C3D+: env slots carry kParamAmount only — no kParamOut /
        // aux-curve / bipolar entries staged by the dead-slot builder)
        let mut pp = Val::obj();
        pp.set("kParamAmount", Val::F64(residue * 100.0));
        node.set("plainParams", pp);
        let mut pair = Val::arr();
        pair.push(Val::Int(38));
        pair.push(Val::Int(0));
        node.set("source", pair);
    }
}

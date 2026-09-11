//! The per-index value machinery: unit converters, `s2_value` mapping and
//! `fn_setval` / `fn_setval_special` (§3.2 formula table).

use super::Ctx;
use super::FX_BASE_IDX;
use crate::s2tables::{S2_PARAM_DESCS, S2ParamDesc};
use crate::s2tree::Val;

impl Ctx<'_> {
    /// fn_4d9c50: quantize the 0..1 fraction per descriptor class.
    pub(super) fn unit_convert(&self, idx: usize, v: f64) -> f64 {
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
    pub(super) fn s2_value(&self, idx: usize, v_in: f64) -> Val {
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
    pub(super) fn fn_setval(&mut self, idx: usize, v: f64) {
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
            self.s2_value(idx, v / 1.06875)
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
    pub(super) fn fn_setval_special(&mut self, idx: usize, v: f64) {
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

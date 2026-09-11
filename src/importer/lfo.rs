//! LFO / phasor / scalars curve writers: classic→modern LFO normalization,
//! the fn_4f2a70 curveData writer, phasor defaults and the fn_4f3e00 scalars
//! curves.

use super::Ctx;
use crate::s1state;
use crate::s2tree::Val;

/// Classic (pre-0.148) LFO block → modern 0x2D28 layout (fn_4f1eb0).
pub(super) fn normalize_classic_lfo(classic: &[u8]) -> Vec<u8> {
    if classic.len() < 0x1890 {
        return vec![0u8; s1state::LFO_BLOCK_SIZE];
    }
    let mut out = vec![0u8; s1state::LFO_BLOCK_SIZE];
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
pub(super) fn default_phasor_curve() -> Val {
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
pub(super) fn write_lfo_curve(ctx: &Ctx, _k: usize, block: &[u8]) -> Val {
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
pub(super) fn write_lfo_flex(_ctx: &Ctx, _k: usize, _block: &[u8]) -> Val {
    Val::obj()
}

/// LFO 8/9 phasor defaults (version > 0.155).
pub(super) fn write_phasor_defaults(ctx: &mut Ctx, ver: f32) {
    if ver <= 0.155 {
        return;
    }
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

/// fn_4f3e00 scalars writer (velo/note curve nodes).
pub(super) fn write_scalars(ctx: &Ctx, kind: usize) -> Val {
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

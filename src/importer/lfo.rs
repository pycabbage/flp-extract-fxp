//! LFO / phasor / scalars curve writers: classic→modern LFO normalization,
//! the fn_4f2a70 curveData writer, phasor defaults and the fn_4f3e00 scalars
//! curves.

use super::Ctx;
use crate::s1state;
use crate::s2tree::Val;

/// The importer's synthesized default block (fn_4f1eb0 0x4F1FB2 / inline
/// copy at 0x4DE37F): a 2-point curve with a fully-specified tail. `alt`
/// selects the flag=1 (numPoints=1) variant used for the LFO 9/10 flex
/// blocks, whose tail stays zero.
pub(super) fn default_classic_block(alt: bool) -> Vec<u8> {
    let mut out = vec![0u8; s1state::LFO_BLOCK_SIZE];
    let put_f64 = |out: &mut [u8], i: usize, v: f64| {
        out[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
    };
    // tension: all 0.5
    for i in 0..480usize {
        put_f64(&mut out, i, 0.5);
    }
    // x: 0, (alt ? 1.0 : 0.5), 1.0... (xVals live at +0x0F00 = element 480)
    put_f64(&mut out, 480, 0.0);
    put_f64(&mut out, 481, if alt { 1.0 } else { 0.5 });
    for i in 482..960usize {
        put_f64(&mut out, i, 1.0);
    }
    // y: 1, 0, 1.0... (yVals live at +0x1E00 = element 960)
    put_f64(&mut out, 960, 1.0);
    put_f64(&mut out, 961, 0.0);
    for i in 962..1440usize {
        put_f64(&mut out, i, 1.0);
    }
    if !alt {
        out[0x2D00] = 1; // anchored
        out[0x2D01] = 0; // beat-sync off
        // importer writes the qword 0x1E0FFFFFFFF at 0x2D0C:
        // phase = -1, loopbackPointNum = 0x1E0
        out[0x2D0C..0x2D14].copy_from_slice(&0x1E0_FFFF_FFFFu64.to_le_bytes());
        out[0x2D14..0x2D18].copy_from_slice(&0.5f32.to_le_bytes()); // rate
        out[0x2D08..0x2D0C].copy_from_slice(&2u32.to_le_bytes()); // numPoints
    } else {
        out[0x2D08..0x2D0C].copy_from_slice(&1u32.to_le_bytes()); // numPoints
    }
    out
}

/// Classic (pre-0.162) LFO region → modern 0x2D28 block (fn_4f1eb0 0x4F1EB0,
/// called with flag=0 from the LFO 1-8 loop). The destination is first filled
/// with the default block, then the 65-entry tension/x/y planes of sub-block
/// `sub` (`k & 3`) are copied over planes at +0x000/+0x820/+0x1040 with
/// 0x208-byte sub-block stride, and finally the per-LFO tail fields are
/// re-read from the region tail at 0x1860.
pub(super) fn normalize_classic_lfo(region: &[u8], sub: usize) -> Vec<u8> {
    let mut out = default_classic_block(false);
    let src = |off: usize, len: usize| -> Option<&[u8]> {
        let start = sub * 0x208 + off;
        region.get(start..start + len)
    };
    // three 65-f64 planes: tension, x, y. The importer's tension copy is
    // 65 entries (0x200-byte SSE block + 1), the x/y copies only 64
    // (0x4F2232/0x4F25A2/0x4F27A4) — plane element 64 keeps the prefill.
    for (plane, dst, count) in [
        (0usize, 0usize, 65usize),
        (0x820, 480, 64),
        (0x1040, 960, 64),
    ] {
        if let Some(bytes) = src(plane, count * 8) {
            out[dst * 8..dst * 8 + count * 8].copy_from_slice(bytes);
        }
    }
    let region = match region.len() >= 0x18F0 {
        true => region,
        false => return out,
    };
    let put = |out: &mut [u8], off: usize, b: &[u8]| out[off..off + b.len()].copy_from_slice(b);
    // per-LFO tail: byte-strided flags, 4-byte-strided numeric fields
    let b = |off: usize| region[sub + off];
    let w = |off: usize| {
        let s = sub * 4 + off;
        let mut a = [0u8; 4];
        a.copy_from_slice(&region[s..s + 4]);
        a
    };
    out[0x2D08..0x2D0C].copy_from_slice(&((b(0x1860) as i8) as i32).to_le_bytes()); // numPoints (sign-extended byte)
    put(&mut out, 0x2D00, &[b(0x1874)]); // anchored
    put(&mut out, 0x2D01, &[b(0x1878)]); // beat-sync
    put(&mut out, 0x2D02, &[b(0x187C)]); // dotted
    put(&mut out, 0x2D03, &[b(0x1880)]); // triplets
    put(&mut out, 0x2D04, &[b(0x1884)]); // not-off
    put(&mut out, 0x2D05, &[b(0x1888)]); // env flag
    put(&mut out, 0x2D06, &[b(0x188C)]);
    put(&mut out, 0x2D0C, &w(0x18A0)); // phase
    put(&mut out, 0x2D10, &w(0x18B0)); // loopbackPointNum
    put(&mut out, 0x2D14, &w(0x1864)); // rate
    put(&mut out, 0x2D18, &w(0x18C0)); // smooth
    put(&mut out, 0x2D1C, &w(0x18D0)); // delay
    put(&mut out, 0x2D20, &w(0x18E0)); // rise
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

/// The legacy default phasor curve (pre-0.162 WTOsc flex / FX-cell flex):
/// tension points 0..64 are f32 0.49999997, the tail is exact 0.5.
pub(super) fn default_phasor_curve_legacy() -> Val {
    let mut curve = default_phasor_curve();
    let Some(Val::Array(cv)) = curve.get_mut("curveVals") else {
        return curve;
    };
    for (i, v) in cv.iter_mut().enumerate() {
        if i >= 65 {
            *v = Val::F64(0.5);
        }
    }
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
    curve.set("numPoints", Val::UInt(u64::from(np)));
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
    // importer 0x4F3079: env flag set -> template(1.0) = "Envelope";
    // otherwise template(signed mode byte * 0.5): 0 -> "Free", 1 -> "Retrig".
    let mode_text = if env_byte != 0 {
        "Envelope"
    } else {
        match mode_byte {
            1 => "Retrig",
            0 | i8::MIN..=0 => "Free",
            _ => "Envelope",
        }
    };
    pp.set("kParamMode", Val::Text(mode_text.into()));
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
        // importer 0x4F32F7: phase anchor index -> kParamPhase through the
        // 0x9F04E0 template (anchor fraction scaled to degrees, x * 360).
        let i = phase as usize;
        let pv = f64::from_le_bytes(block[0xF00 + i * 8..0xF00 + i * 8 + 8].try_into().unwrap());
        pp.set("kParamPhase", Val::F64(pv * 360.0));
    }
    node
}

/// LFO 8/9 flex writer (no curveData in the importer output).
pub(super) fn write_lfo_flex(_ctx: &Ctx, _k: usize, _block: &[u8]) -> Val {
    Val::obj()
}

/// LFO 8/9 phasor defaults (importer 0x4DB024–0x4DB3F6, unconditional —
/// they are written for every supported version, including legacy ones).
pub(super) fn write_phasor_defaults(ctx: &mut Ctx) {
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
    // importer 0x4F44B2: the gate field (+0x73C) below the 0.143 constant
    // forces legato off; otherwise legato = (value at +0x738 != 0.143).
    let legato_gate = f32_at(0x73C);
    let legato = if legato_gate < 0.143 {
        false
    } else {
        f32_at(0x738 + 4 * kind) != 0.143_000_006_675_720_21
    };
    curve.set("legato", Val::Bool(legato));
    curve
}

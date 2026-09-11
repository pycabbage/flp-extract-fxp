//! Serum 1 preset-state parsing: splits a Serum 1 fxp `chunk` into its zlib
//! streams, inflates the 172,736-byte state blob and exposes it through typed,
//! bounds-checked accessors for the downstream S1 -> S2 converter.

use flate2::read::ZlibDecoder;

/// Decompressed size of a modern (Serum >= ~1.2) preset state blob.
pub const S1_BLOB_SIZE: usize = 172_736;
/// State-blob sizes of 2015-era (Serum <= ~1.07) presets, not supported here.
pub const S1_OLD_BLOB_SIZES: [usize; 2] = [21_808, 28_232];
/// Master parameters 0..247 (float32 LE) at `blob + 0x3460`.
pub const OFF_MASTER_PARAMS: usize = 0x3460;
pub const MASTER_PARAM_COUNT: usize = 248;
/// Parameters 228..298 (float32 LE) at `blob + 0x4AE0`.
pub const OFF_AUX_PARAMS: usize = 0x4AE0;
pub const AUX_PARAM_COUNT: usize = 71;
/// FX rack order, 10 x i32 LE.
pub const OFF_FX_ORDER: usize = 0x3BE0;
/// OSC A / OSC B wavetable and noise sample name fields (512 B, NUL-terminated).
pub const OFF_WT_NAME_A: usize = 0x3C08;
pub const OFF_WT_NAME_B: usize = 0x3E08;
pub const OFF_NOISE_NAME: usize = 0x4008;
pub const NAME_FIELD_LEN: usize = 512;
/// Preset name (32 B) / version f32 / author (48 B) / category (48 B).
pub const OFF_PRESET_NAME: usize = 0x4972;
pub const OFF_VERSION_F32: usize = 0x4994;
pub const OFF_AUTHOR: usize = 0x49A0;
pub const OFF_CATEGORY: usize = 0x49D0;
/// 8-byte per-preset id, preserved verbatim.
pub const OFF_PRESET_ID: usize = 0x4A58;
/// Oversampling / tuning lock bits byte.
pub const OFF_LOCK_BITS: usize = 0x4A50;
/// Macro 1..4 names, 4 x 32 B.
pub const OFF_MACRO_NAMES: usize = 0x4A60;
/// Per-osc embedded wavetable byte counters (i32 LE x2) and the
/// interpolate-after-load flag byte.
pub const OFF_OSC_WT_FRAMES: usize = 0x4968;
pub const OFF_INTERPOLATE_AFTER_LOAD: usize = 0x4970;
/// Velocity / note scalars curve blocks, 0x200 bytes each at `blob + 0x4220`.
pub const OFF_SCALARS: usize = 0x4220;
pub const SCALARS_BLOCK_LEN: usize = 0x200;
pub const SCALARS_BLOCK_COUNT: usize = 2;
/// Global switches block, 100 bytes of float32 fields.
pub const OFF_SWITCHES: usize = 0x4C48;
pub const SWITCHES_LEN: usize = 100;
pub const SWITCHES_SLICE_LEN: usize = 200;
/// MIDI map: CC assigned to param `i` at `blob + 0x3840 + i`, and to param
/// `248 + i` at `blob + 0x5360 + i` (37 entries, the Serum2 importer range).
pub const OFF_MIDI_MAP: usize = 0x3840;
pub const MIDI_MAP_LEN: usize = 247;
pub const OFF_MIDI_MAP_EXTRA: usize = 0x5360;
pub const MIDI_MAP_EXTRA_LEN: usize = 37;
/// tuningData length (u32 LE) and tuning name (NUL-terminated) fields.
pub const OFF_TUNING_LEN: usize = 0x53E0;
pub const OFF_TUNING_NAME: usize = 0x53E4;
pub const TUNING_NAME_MAX: usize = 0x34;
/// storedPhasePos / loopback64 / boundary64 raw 8-byte values.
pub const OFF_STORED_PHASE_POS: usize = 0x5528;
pub const OFF_LOOPBACK64: usize = 0x5530;
pub const OFF_BOUNDARY64: usize = 0x5538;
/// LFO point-mod records (0x2C bytes each) at `blob + 0x6E48`, count u32 LE
/// at `blob + 0x8448`.
pub const OFF_LFO_POINT_MODS: usize = 0x6E48;
pub const OFF_LFO_POINT_MOD_COUNT: usize = 0x8448;
pub const LFO_POINT_MOD_LEN: usize = 0x2C;
pub const LFO_POINT_MOD_MAX: usize = 4096;
/// Modern LFO graph blocks: importer block base, stride, count.
pub const LFO_BLOCK_BASE: usize = 0x84E0;
pub const LFO_BLOCK_SIZE: usize = 0x2D28;
pub const LFO_BLOCK_COUNT: usize = 8;
/// LFO 9/10 "flex" phasor blocks, same stride, base `0x1EE20`.
pub const LFO_FLEX_BASE: usize = 0x1EE20;
pub const LFO_FLEX_COUNT: usize = 2;
/// Classic-layout LFO shape regions (12 x 65 f64 + switch tail each).
pub const LFO_CLASSIC_1_4: usize = 0x0280;
pub const LFO_CLASSIC_5_8: usize = 0x1B70;
pub const LFO_CLASSIC_ALT_5_8: usize = 0x5558;
pub const LFO_CLASSIC_SIZE: usize = 0x18F0;
/// Modern-block field offsets (Serum2 importer `fn_4f2a70` layout).
pub const LFO_CURVE_POINTS: usize = 480;
pub const LFO_CURVE_BYTES: usize = LFO_CURVE_POINTS * 8;
pub const LFO_OFF_FLAGS: usize = 0x2D00;
pub const LFO_OFF_NUM_POINTS: usize = 0x2D08;
pub const LFO_OFF_PHASE: usize = 0x2D0C;
pub const LFO_OFF_LOOPBACK_POINT_NUM: usize = 0x2D10;
pub const LFO_OFF_RATE: usize = 0x2D14;
pub const LFO_OFF_SMOOTH: usize = 0x2D18;
pub const LFO_OFF_DELAY: usize = 0x2D1C;
pub const LFO_OFF_RISE: usize = 0x2D20;
/// Embedded wavetable / noise frames: 2048 float32 LE samples per frame.
pub const FRAME_BYTES: usize = 8192;
/// Sanity caps on decompressed sizes.
pub const MAX_STREAM_DECOMP: usize = 32 * 1024 * 1024;
pub const MAX_TOTAL_DECOMP: usize = 32 * 1024 * 1024;
/// Modulation-slot record geometry: slots 1-16 at 0x0000, 17-32 at 0x50E0.
pub const MOD_SLOT_LEN: usize = 40;
pub const MOD_SLOT_COUNT: usize = 32;
pub const MOD_SLOTS_1_16: usize = 0x0000;
pub const MOD_SLOTS_17_32: usize = 0x50E0;

/// Raw Serum 1 preset data ready for conversion.
#[derive(Debug)]
pub struct S1Preset {
    /// The full decompressed state blob (172,736 bytes for modern presets).
    pub blob: Vec<u8>,
    /// Appended data streams after the state (embedded wavetable/noise/filter
    /// data; raw float32 LE frames, 2048 samples per frame), in original order.
    pub streams: Vec<Vec<u8>>,
    /// Metadata read from fixed offsets (preset name 0x4972, author 0x49A0,
    /// category 0x49D0, version f32 0x4994).
    pub meta: PresetMeta,
    /// Parsed, validated view of the 32 modulation-slot records (slots 1-16 at
    /// blob+0x0000, 17-32 at blob+0x50E0; 40 bytes each), ordered by 0-based
    /// slot number. Records with a broken marker or out-of-range fields are
    /// dropped.
    pub mod_slots: Vec<S1ModSlot>,
}

/// Metadata read from the decompressed 172,736-byte preset state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresetMeta {
    pub preset_name: String,
    pub author: String,
    pub category: String,
    pub version_f32: f32,
}

/// One validated modulation-slot record (40 bytes, decoded fields + raw).
#[derive(Debug, Clone)]
pub struct S1ModSlot {
    /// 0-based slot number taken from the marker byte at record+0x22.
    pub slot: u8,
    /// S1 source code, word at record+0x14.
    pub source_t: u16,
    /// Third word at record+0x18 (the Serum2 importer staging's "srcA" code;
    /// menu-order remap of the destination per docs/s1-params.md).
    pub src_a: u16,
    /// Aux source id, word at record+0x16 (0 = none).
    pub aux: u16,
    /// S1 destination VST parameter index 0..298, word at record+0x1A.
    pub dest: u16,
    /// Amount A, f32 at record+0x04 (bipolar -1..1).
    pub amount_a: f32,
    /// Output-range scaler, f32 at record+0x08 (1.0 = 100%).
    pub amount_b: f32,
    /// Curve byte A at record+0x20 (not part of the marker).
    pub curve_a: u8,
    /// Aux curve byte at record+0x21 (0x80 in the marker).
    pub aux_curve: u8,
    /// Aux-invert / bypass word at record+0x1C.
    pub aux_word: u16,
    /// Second aux word at record+0x1E.
    pub aux_word2: u16,
    /// Matrix type byte at record+0x0C: 0 unipolar, 1 bipolar.
    pub bipolar_src: i8,
    /// The full raw record bytes.
    pub raw: [u8; 40],
}

/// The tuning fields the Serum2 importer reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct S1Tuning {
    /// u32 LE at blob+0x53E0: byte length of the tuning data carried in the
    /// appended data streams (0 = no custom tuning; the importer also requires
    /// 1..=0x8000 and a printable first name byte).
    pub len: u32,
    /// NUL-terminated name string at blob+0x53E4.
    pub name: String,
}

/// The MIDI-map byte arrays the Serum2 importer reads.
#[derive(Debug, Clone, PartialEq)]
pub struct S1MidiMap {
    /// 247 bytes at blob+0x3840: index `i` -> CC assigned to S1 param `i`
    /// (0 or >=128 = unassigned).
    pub param_cc: [u8; MIDI_MAP_LEN],
    /// 37 bytes at blob+0x5360: index `i` -> CC assigned to S1 param `248 + i`.
    pub extra_cc: [u8; MIDI_MAP_EXTRA_LEN],
}

impl Default for S1MidiMap {
    fn default() -> Self {
        Self {
            param_cc: [0; MIDI_MAP_LEN],
            extra_cc: [0; MIDI_MAP_EXTRA_LEN],
        }
    }
}

/// One lfoPointModAssignments record (raw u32 LE words; the importer clamps
/// `mode` to <=3 and subtracts 147 from `param`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct S1LfoPointMod {
    pub lfo: u32,
    pub point: u32,
    pub mode: u32,
    pub param: u32,
}

/// Failure of one zlib stream: either bad stream data (tolerated after the
/// first stream — a chunk boundary may carry non-stream bytes) or a sanity
/// cap hit (always fatal).
enum StreamError {
    Bad(String),
    Cap(String),
}

impl From<StreamError> for String {
    fn from(e: StreamError) -> String {
        match e {
            StreamError::Bad(msg) | StreamError::Cap(msg) => msg,
        }
    }
}

/// Inflate one zlib stream starting at `data[0]`; returns the decompressed
/// bytes and the number of input bytes consumed.
fn inflate_stream(data: &[u8]) -> Result<(Vec<u8>, usize), StreamError> {
    use std::io::Read;
    if data.is_empty() || data[0] != 0x78 {
        return Err(StreamError::Bad(
            "not a zlib stream (expected 0x78 header byte)".into(),
        ));
    }
    let mut dec = ZlibDecoder::new(data);
    let mut out = Vec::with_capacity(4096);
    let mut buf = [0u8; 64 * 1024];
    loop {
        match dec.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                if out.len() > MAX_STREAM_DECOMP {
                    return Err(StreamError::Cap(format!(
                        "decompressed stream exceeds sanity limit ({MAX_STREAM_DECOMP} bytes)"
                    )));
                }
            }
            Err(e) => return Err(StreamError::Bad(format!("zlib error: {e}"))),
        }
    }
    Ok((out, dec.total_in() as usize))
}

/// Split a Serum 1 chunk into its zlib streams.
///
/// Layout: `[zlib stream 0][zlib stream 1]...[u32 LE trailer]`, the trailer
/// holding the compressed size of stream 0. A minimal zlib stream is 8 bytes,
/// so a 4-byte tail can only be the trailer. The trailer word is tolerated
/// missing or stale (chunks recovered through `crate::serum` are repaired
/// before import).
fn split_streams(chunk: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    if chunk.len() < 8 {
        return Err("chunk too small to contain a zlib stream".into());
    }
    let mut pos = 0usize;
    let mut streams = Vec::new();
    let mut total = 0usize;
    while chunk.len() - pos > 4 {
        if chunk[pos] != 0x78 {
            if pos == 0 {
                return Err("chunk does not start with a zlib stream".into());
            }
            break;
        }
        let (out, consumed) = match inflate_stream(&chunk[pos..]) {
            Ok(v) => v,
            Err(e) => match e {
                StreamError::Cap(msg) => return Err(msg),
                StreamError::Bad(msg) => {
                    if pos == 0 {
                        return Err(msg);
                    }
                    break;
                }
            },
        };
        total += out.len();
        if total > MAX_TOTAL_DECOMP {
            return Err(format!(
                "decompressed chunk exceeds sanity limit ({MAX_TOTAL_DECOMP} bytes)"
            ));
        }
        streams.push(out);
        pos += consumed;
    }
    if streams.is_empty() {
        return Err("chunk contains no zlib streams".into());
    }
    Ok(streams)
}

fn f32_le(bytes: &[u8], off: usize) -> f32 {
    f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
}

fn u32_le(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
}

fn u16_le(bytes: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap())
}

fn read_cstr(buf: &[u8], off: usize, len: usize) -> String {
    let Some(field) = buf.get(off..off + len) else {
        return String::new();
    };
    let nul = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..nul]).trim().to_string()
}

/// Validate one 40-byte modulation-slot record; `slot` must match the marker
/// byte at record+0x22 (marker `80 <slot> FF` at +0x21, the +0x20 byte is not
/// part of the marker). Unused slots carry amount = 0, source = 0 and a
/// garbage dest (316/315 observed) and pass validation as-is.
fn read_mod_slot(blob: &[u8], off: usize, slot: u8) -> Option<S1ModSlot> {
    let raw: [u8; MOD_SLOT_LEN] = blob.get(off..off + MOD_SLOT_LEN)?.try_into().ok()?;
    if raw[0x21] != 0x80 || raw[0x22] != slot || raw[0x23] != 0xFF {
        return None;
    }
    let amount_a = f32_le(&raw, 0x04);
    let amount_b = f32_le(&raw, 0x08);
    let source = u16_le(&raw, 0x14);
    let aux = u16_le(&raw, 0x16);
    let dest = u16_le(&raw, 0x1A);
    if !amount_a.is_finite() || amount_a.abs() > 8.0 {
        return None;
    }
    if !amount_b.is_finite() || amount_b.abs() > 8.0 {
        return None;
    }
    if source > 64 || aux > 64 || dest as usize >= 1024 {
        return None;
    }
    Some(S1ModSlot {
        slot,
        source_t: source,
        src_a: u16_le(&raw, 0x18),
        aux,
        dest,
        amount_a,
        amount_b,
        curve_a: raw[0x20],
        aux_curve: raw[0x21],
        aux_word: u16_le(&raw, 0x1C),
        aux_word2: u16_le(&raw, 0x1E),
        bipolar_src: raw[0x0C] as i8,
        raw,
    })
}

/// Parse the 32 modulation-slot records: fixed positions first (slots 1-16 at
/// 0x0000+40k, 17-32 at the fixed 0x50E0+40k offset verified on all modern
/// blobs), then a whole-blob marker scan for slots missing there
/// (overlapping occurrences permitted).
fn parse_mod_slots(blob: &[u8]) -> Vec<S1ModSlot> {
    let mut found: Vec<Option<S1ModSlot>> = vec![None; MOD_SLOT_COUNT];
    for k in 0..MOD_SLOT_COUNT as u8 {
        let base = if k < 16 {
            MOD_SLOTS_1_16 + MOD_SLOT_LEN * k as usize
        } else {
            MOD_SLOTS_17_32 + MOD_SLOT_LEN * (k - 16) as usize
        };
        found[k as usize] = read_mod_slot(blob, base, k);
    }
    for i in 0..blob.len().saturating_sub(0x24) {
        let record = &blob[i..];
        if record[0x21] != 0x80 || record[0x23] != 0xFF {
            continue;
        }
        let slot = record[0x22];
        let k = slot as usize;
        if k >= MOD_SLOT_COUNT || found[k].is_some() {
            continue;
        }
        found[k] = read_mod_slot(blob, i, slot);
    }
    found.into_iter().flatten().collect()
}

/// Parse a raw Serum 1 preset chunk (concatenated zlib streams + u32 LE
/// trailer, i.e. `crate::serum::Serum1Chunk::chunk`) into a typed [`S1Preset`].
pub fn parse_preset(chunk: &[u8]) -> Result<S1Preset, String> {
    let streams = split_streams(chunk)?;
    let blob = streams[0].clone();
    if blob.len() != S1_BLOB_SIZE {
        return Err(format!(
            "old-format Serum 1 preset not supported by the converter \
             (state blob is {} bytes, expected {S1_BLOB_SIZE})",
            blob.len()
        ));
    }
    for (i, stream) in streams.iter().enumerate().skip(1) {
        if stream.len() % FRAME_BYTES != 0 {
            return Err(format!(
                "appended data stream {i} is {} bytes, not a multiple of the \
                 {FRAME_BYTES}-byte wavetable frame size",
                stream.len()
            ));
        }
    }
    let meta = PresetMeta {
        preset_name: read_cstr(&blob, OFF_PRESET_NAME, 32),
        author: read_cstr(&blob, OFF_AUTHOR, 48),
        category: read_cstr(&blob, OFF_CATEGORY, 48),
        version_f32: blob
            .get(OFF_VERSION_F32..OFF_VERSION_F32 + 4)
            .map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap())),
    };
    let mod_slots = parse_mod_slots(&blob);
    Ok(S1Preset {
        blob,
        streams: streams.into_iter().skip(1).collect(),
        meta,
        mod_slots,
    })
}

impl S1Preset {
    fn f32_at(&self, off: usize) -> f32 {
        self.blob
            .get(off..off + 4)
            .map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()))
    }

    fn u32_at(&self, off: usize) -> u32 {
        self.blob
            .get(off..off + 4)
            .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
    }

    fn bytes8(&self, off: usize) -> [u8; 8] {
        let mut out = [0u8; 8];
        if let Some(field) = self.blob.get(off..off + 8) {
            out.copy_from_slice(field);
        }
        out
    }

    /// Raw stored value of master parameter `i` (0..247) at `blob + 0x3460`;
    /// 0.0 out of range. No clamping (the importer clamps to [0,1] and maps
    /// NaN to 0 — that is the converter's job).
    pub fn master_param(&self, i: usize) -> f32 {
        if i >= MASTER_PARAM_COUNT {
            return 0.0;
        }
        self.f32_at(OFF_MASTER_PARAMS + 4 * i)
    }

    /// Raw stored value of parameter `228 + i` (0..70) at `blob + 0x4AE0`.
    pub fn aux_param(&self, i: usize) -> f32 {
        if i >= AUX_PARAM_COUNT {
            return 0.0;
        }
        self.f32_at(OFF_AUX_PARAMS + 4 * i)
    }

    /// FX rack order (10 x i32 LE at `blob + 0x3BE0`): one rack position
    /// (0 = top) per effect in enable-parameter order; zeros when short.
    pub fn fx_order(&self) -> [i32; 10] {
        let mut order = [0i32; 10];
        if let Some(region) = self.blob.get(OFF_FX_ORDER..OFF_FX_ORDER + 40) {
            for (i, cell) in order.iter_mut().enumerate() {
                *cell = i32::from_le_bytes(region[4 * i..4 * i + 4].try_into().unwrap());
            }
        }
        order
    }

    /// OSC A wavetable name (512-byte NUL-terminated field at 0x3C08).
    pub fn wt_name_a(&self) -> String {
        read_cstr(&self.blob, OFF_WT_NAME_A, NAME_FIELD_LEN)
    }

    /// OSC B wavetable name (at 0x3E08).
    pub fn wt_name_b(&self) -> String {
        read_cstr(&self.blob, OFF_WT_NAME_B, NAME_FIELD_LEN)
    }

    /// Noise sample name (at 0x4008).
    pub fn noise_name(&self) -> String {
        read_cstr(&self.blob, OFF_NOISE_NAME, NAME_FIELD_LEN)
    }

    /// Macro `k` (0..4) name, 32-byte NUL-terminated field at `0x4A60 + 32k`.
    pub fn macro_name(&self, k: usize) -> String {
        if k >= 4 {
            return String::new();
        }
        read_cstr(&self.blob, OFF_MACRO_NAMES + 32 * k, 32)
    }

    /// The 8-byte per-preset id at `blob + 0x4A58`, preserved verbatim.
    pub fn preset_id(&self) -> [u8; 8] {
        self.bytes8(OFF_PRESET_ID)
    }

    /// Per-osc embedded wavetable counters (i32 LE x2 at `0x4968/0x496C`).
    /// Raw values; multiples of 8192 (byte sizes) in modern presets.
    pub fn osc_wt_frames(&self) -> [i32; 2] {
        [
            self.blob
                .get(OFF_OSC_WT_FRAMES..OFF_OSC_WT_FRAMES + 4)
                .map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap())),
            self.blob
                .get(OFF_OSC_WT_FRAMES + 4..OFF_OSC_WT_FRAMES + 8)
                .map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap())),
        ]
    }

    /// The interpolate-after-load flag byte at `blob + 0x4970`.
    pub fn interpolate_after_load(&self) -> bool {
        self.blob
            .get(OFF_INTERPOLATE_AFTER_LOAD)
            .copied()
            .unwrap_or(0)
            != 0
    }

    /// Raw 8-byte value at `blob + 0x5528`, written to the S2
    /// `storedPhasePos` byte-string by the importer.
    pub fn stored_phase_pos(&self) -> [u8; 8] {
        self.bytes8(OFF_STORED_PHASE_POS)
    }

    /// Raw 8-byte value at `blob + 0x5530` (`loopback64`).
    pub fn loopback64(&self) -> [u8; 8] {
        self.bytes8(OFF_LOOPBACK64)
    }

    /// Raw 8-byte value at `blob + 0x5538` (`boundary64`).
    pub fn boundary64(&self) -> [u8; 8] {
        self.bytes8(OFF_BOUNDARY64)
    }

    /// The tuning fields the importer reads (see [`S1Tuning`]); the raw
    /// tuning bytes themselves are carried in the appended data streams.
    pub fn tuning_bytes(&self) -> S1Tuning {
        S1Tuning {
            len: self.u32_at(OFF_TUNING_LEN),
            name: read_cstr(&self.blob, OFF_TUNING_NAME, TUNING_NAME_MAX),
        }
    }

    /// Oversampling / tuning lock bits at `blob + 0x4A50` (bit 1 =
    /// lockOversampling, bit 2 = lockTuning).
    pub fn lock_bits(&self) -> u8 {
        self.blob.get(OFF_LOCK_BITS).copied().unwrap_or(0)
    }

    /// The `k`-th modern LFO graph block (k < 8): 0x2D28 raw bytes at
    /// `blob + 0x84E0 + 0x2D28*k` in the Serum2 importer block layout (curve
    /// arrays at +0x0000/+0x0F00/+0x1E00, tail fields at +0x2D00..).
    pub fn lfo_block(&self, k: usize) -> Option<&[u8]> {
        if k >= LFO_BLOCK_COUNT {
            return None;
        }
        let base = LFO_BLOCK_BASE + LFO_BLOCK_SIZE * k;
        self.blob.get(base..base + LFO_BLOCK_SIZE)
    }

    /// LFO 9/10 "flex" phasor block (k < 2), same layout, base `0x1EE20`.
    pub fn flex_lfo_block(&self, k: usize) -> Option<&[u8]> {
        if k >= LFO_FLEX_COUNT {
            return None;
        }
        let base = LFO_FLEX_BASE + LFO_BLOCK_SIZE * k;
        self.blob.get(base..base + LFO_BLOCK_SIZE)
    }

    /// Classic-layout LFO shape region for LFO `k` (k < 8), 0x18F0 bytes:
    /// LFO 1-4 at `blob + 0x0280`, LFO 5-8 at `blob + 0x1B70`. Layout: 12
    /// arrays x 65 float64 (stride 520; arrays 0-3 tension, 4-7 x, 8-11 y),
    /// then per-LFO switch bytes at region+0x1860+i (point count u8, rate
    /// f32 copy, anchor / Hz / dotted / triplet / not-off / env flag bytes).
    pub fn classic_lfo_block(&self, k: usize) -> Option<&[u8]> {
        let base = match k {
            0..=3 => LFO_CLASSIC_1_4,
            4..=7 => LFO_CLASSIC_5_8,
            _ => return None,
        };
        self.blob.get(base..base + LFO_CLASSIC_SIZE)
    }

    /// Mid-era (0.148-0.162 builds) classic-layout LFO 5-8 region at
    /// `blob + 0x5558`, same layout as [`S1Preset::classic_lfo_block`].
    pub fn classic_lfo_5_8_alt(&self) -> Option<&[u8]> {
        self.blob
            .get(LFO_CLASSIC_ALT_5_8..LFO_CLASSIC_ALT_5_8 + LFO_CLASSIC_SIZE)
    }

    /// The lfoPointModAssignments records (raw u32 words; the importer clamps
    /// `mode` to <=3 and offsets `param` by -147).
    pub fn lfo_point_mods(&self) -> Vec<S1LfoPointMod> {
        let count = self
            .u32_at(OFF_LFO_POINT_MOD_COUNT)
            .min(LFO_POINT_MOD_MAX as u32) as usize;
        let available = self.blob.len().saturating_sub(OFF_LFO_POINT_MODS) / LFO_POINT_MOD_LEN;
        let count = count.min(available);
        (0..count)
            .map(|i| {
                let record = &self.blob[OFF_LFO_POINT_MODS + LFO_POINT_MOD_LEN * i..];
                S1LfoPointMod {
                    lfo: u32_le(record, 0x00),
                    point: u32_le(record, 0x04),
                    mode: u32_le(record, 0x08),
                    param: u32_le(record, 0x0C),
                }
            })
            .collect()
    }

    /// The `kind`-th scalars curve block (0 = velo, 1 = note), 0x200 raw
    /// bytes at `blob + 0x4220 + 0x200*kind`; empty slice out of range.
    pub fn scalars_curve(&self, kind: usize) -> &[u8] {
        if kind >= SCALARS_BLOCK_COUNT {
            return &[];
        }
        let base = OFF_SCALARS + SCALARS_BLOCK_LEN * kind;
        self.blob.get(base..base + SCALARS_BLOCK_LEN).unwrap_or(&[])
    }

    /// The two MIDI-map byte arrays (see [`S1MidiMap`]).
    pub fn midi_map(&self) -> S1MidiMap {
        let mut map = S1MidiMap::default();
        if let Some(region) = self.blob.get(OFF_MIDI_MAP..OFF_MIDI_MAP + MIDI_MAP_LEN) {
            map.param_cc.copy_from_slice(region);
        }
        if let Some(region) = self
            .blob
            .get(OFF_MIDI_MAP_EXTRA..OFF_MIDI_MAP_EXTRA + MIDI_MAP_EXTRA_LEN)
        {
            map.extra_cc.copy_from_slice(region);
        }
        map
    }

    /// Raw bytes of the global switches block at `blob + 0x4C48` (up to
    /// [`SWITCHES_SLICE_LEN`] bytes, clamped to the blob end).
    pub fn switches(&self) -> &[u8] {
        let end = (OFF_SWITCHES + SWITCHES_SLICE_LEN).min(self.blob.len());
        self.blob.get(OFF_SWITCHES..end).unwrap_or(&[])
    }

    /// Float32 field at `switches + off` (0.0 when outside the documented
    /// 100-byte block).
    pub fn switch_f32(&self, off: usize) -> f32 {
        if off + 4 > SWITCHES_LEN {
            return 0.0;
        }
        self.f32_at(OFF_SWITCHES + off)
    }

    /// A4 tuning reference: Hz = 430 + 20*value (0.5 = 440 Hz).
    pub fn switch_a4(&self) -> f32 {
        self.switch_f32(0x00)
    }

    /// Unison tuning A: index/4 of Linear/Super/Exp/Inv/Random.
    pub fn switch_unison_tuning_a(&self) -> f32 {
        self.switch_f32(0x08)
    }

    /// Unison tuning B, same encoding.
    pub fn switch_unison_tuning_b(&self) -> f32 {
        self.switch_f32(0x0C)
    }

    /// Mono toggle (0/1).
    pub fn switch_mono(&self) -> f32 {
        self.switch_f32(0x10)
    }

    /// Legato toggle (0/1).
    pub fn switch_legato(&self) -> f32 {
        self.switch_f32(0x14)
    }

    /// Portamento "Always" toggle (0/1).
    pub fn switch_porta_always(&self) -> f32 {
        self.switch_f32(0x18)
    }

    /// Portamento "Scaled" toggle (0/1).
    pub fn switch_porta_scaled(&self) -> f32 {
        self.switch_f32(0x1C)
    }

    /// Oversampling: index/2 of 1x/2x/4x (0.5 = 2x).
    pub fn switch_oversampling(&self) -> f32 {
        self.switch_f32(0x20)
    }

    /// Noise one-shot toggle (0/1).
    pub fn switch_noise_one_shot(&self) -> f32 {
        self.switch_f32(0x24)
    }

    /// Noise pitch-track toggle (0/1).
    pub fn switch_noise_pitch_track(&self) -> f32 {
        self.switch_f32(0x28)
    }

    /// Polyphony: (voices - 1) / 31 (default 8 voices = 7/31).
    pub fn switch_polyphony(&self) -> f32 {
        self.switch_f32(0x2C)
    }

    /// Filter keytrack toggle (0/1).
    pub fn switch_filter_keytrack(&self) -> f32 {
        self.switch_f32(0x34)
    }

    /// Unison range A: semitones / 48 (default 2 st = 0.041667).
    pub fn switch_unison_range_a(&self) -> f32 {
        self.switch_f32(0x38)
    }

    /// Unison range B, same encoding.
    pub fn switch_unison_range_b(&self) -> f32 {
        self.switch_f32(0x3C)
    }

    /// Chaos 1 mono toggle (0/1).
    pub fn switch_chaos1_mono(&self) -> f32 {
        self.switch_f32(0x40)
    }

    /// Chaos 2 mono toggle (0/1).
    pub fn switch_chaos2_mono(&self) -> f32 {
        self.switch_f32(0x44)
    }

    /// Chorus mono toggle (0/1).
    pub fn switch_chorus_mono(&self) -> f32 {
        self.switch_f32(0x48)
    }

    /// Chaos 1 sample-and-hold toggle (0/1).
    pub fn switch_chaos1_sample_hold(&self) -> f32 {
        self.switch_f32(0x50)
    }

    /// Chaos 2 sample-and-hold toggle (0/1).
    pub fn switch_chaos2_sample_hold(&self) -> f32 {
        self.switch_f32(0x54)
    }

    /// Reverb Hall/Plate: 1 = Hall (default; mirrored by the 0x3B04 byte).
    pub fn switch_reverb_hall(&self) -> f32 {
        self.switch_f32(0x5C)
    }
}

/// Point count, u32 at block+0x2D08 (works for any 0x2D28 block).
pub fn lfo_num_points(block: &[u8]) -> u32 {
    block
        .get(LFO_OFF_NUM_POINTS..LFO_OFF_NUM_POINTS + 4)
        .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

pub fn lfo_phase_raw(block: &[u8]) -> u32 {
    block
        .get(LFO_OFF_PHASE..LFO_OFF_PHASE + 4)
        .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

pub fn lfo_loopback_point_num(block: &[u8]) -> i32 {
    block
        .get(LFO_OFF_LOOPBACK_POINT_NUM..LFO_OFF_LOOPBACK_POINT_NUM + 4)
        .map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()))
}

pub fn lfo_rate(block: &[u8]) -> f32 {
    block
        .get(LFO_OFF_RATE..LFO_OFF_RATE + 4)
        .map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()))
}

pub fn lfo_smooth(block: &[u8]) -> f32 {
    block
        .get(LFO_OFF_SMOOTH..LFO_OFF_SMOOTH + 4)
        .map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()))
}

pub fn lfo_delay(block: &[u8]) -> f32 {
    block
        .get(LFO_OFF_DELAY..LFO_OFF_DELAY + 4)
        .map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()))
}

pub fn lfo_rise(block: &[u8]) -> f32 {
    block
        .get(LFO_OFF_RISE..LFO_OFF_RISE + 4)
        .map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()))
}

pub fn lfo_flags(block: &[u8]) -> [u8; 7] {
    let mut flags = [0u8; 7];
    if let Some(region) = block.get(LFO_OFF_FLAGS..LFO_OFF_FLAGS + 7) {
        flags.copy_from_slice(region);
    }
    flags
}

pub fn lfo_curve_vals(block: &[u8]) -> Option<&[u8]> {
    block.get(..LFO_CURVE_BYTES)
}

pub fn lfo_x_vals(block: &[u8]) -> Option<&[u8]> {
    block.get(LFO_CURVE_BYTES..2 * LFO_CURVE_BYTES)
}

pub fn lfo_y_vals(block: &[u8]) -> Option<&[u8]> {
    block.get(2 * LFO_CURVE_BYTES..3 * LFO_CURVE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zlib_stream(data: &[u8]) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;
        let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn put_f32(blob: &mut [u8], off: usize, v: f32) {
        blob[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put_f64(blob: &mut [u8], off: usize, v: f64) {
        blob[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }

    fn put_i32(blob: &mut [u8], off: usize, v: i32) {
        blob[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put_u32(blob: &mut [u8], off: usize, v: u32) {
        blob[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put_u16(blob: &mut [u8], off: usize, v: u16) {
        blob[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn write_mod_slot(blob: &mut [u8], base: usize, slot: u8, source: u16, dest: u16, amount: f32) {
        blob[base + 0x20..base + 0x24].copy_from_slice(&[0x80, 0x80, slot, 0xFF]);
        put_f32(blob, base + 0x04, amount);
        put_f32(blob, base + 0x08, 1.0);
        put_u16(blob, base + 0x14, source);
        put_u16(blob, base + 0x1A, dest);
    }

    fn synth_state() -> Vec<u8> {
        let mut s0 = vec![0u8; S1_BLOB_SIZE];
        s0[OFF_PRESET_NAME..OFF_PRESET_NAME + 6].copy_from_slice(b"Synth\0");
        s0[OFF_AUTHOR..OFF_AUTHOR + 5].copy_from_slice(b"Jane\0");
        put_f32(&mut s0, OFF_VERSION_F32, 0.1631);
        put_f32(&mut s0, OFF_MASTER_PARAMS, 0.7);
        put_f32(&mut s0, OFF_MASTER_PARAMS + 4 * 61, 0.25);
        put_f32(&mut s0, OFF_AUX_PARAMS, 0.25);
        let order = [5, 0, 1, 2, 3, 6, 7, 8, 9, 4];
        for (i, v) in order.iter().enumerate() {
            put_i32(&mut s0, OFF_FX_ORDER + 4 * i, *v);
        }
        let name = b"Tables\\My Saw.wav\0";
        s0[OFF_WT_NAME_A..OFF_WT_NAME_A + name.len()].copy_from_slice(name);
        let noise = b"Noise/AC hum1.wav\0";
        s0[OFF_NOISE_NAME..OFF_NOISE_NAME + noise.len()].copy_from_slice(noise);
        for (k, label) in ["Macro A", "Macro B", "Macro C", "Macro D"]
            .iter()
            .enumerate()
        {
            let off = OFF_MACRO_NAMES + 32 * k;
            let bytes = format!("{label}\0").into_bytes();
            s0[off..off + bytes.len()].copy_from_slice(&bytes);
        }
        put_i32(&mut s0, OFF_OSC_WT_FRAMES, 8192);
        put_i32(&mut s0, OFF_OSC_WT_FRAMES + 4, 16384);
        s0[OFF_INTERPOLATE_AFTER_LOAD] = 1;
        s0[OFF_LOCK_BITS] = 0b0110;
        put_u32(&mut s0, OFF_STORED_PHASE_POS, 0x1122_3344);
        put_u32(&mut s0, OFF_STORED_PHASE_POS + 4, 0x5566_7788);
        put_u32(&mut s0, OFF_LOOPBACK64, 0xAABB_CCDD);
        put_u32(&mut s0, OFF_BOUNDARY64, 0xEEFF_0011);
        for k in 0..MOD_SLOT_COUNT as u8 {
            let base = if k < 16 {
                MOD_SLOTS_1_16 + MOD_SLOT_LEN * k as usize
            } else {
                MOD_SLOTS_17_32 + MOD_SLOT_LEN * (k - 16) as usize
            };
            write_mod_slot(&mut s0, base, k, 0, 316, 0.0);
            put_u16(&mut s0, base + 0x18, 173);
        }
        write_mod_slot(&mut s0, MOD_SLOTS_1_16, 0, 5, 1, 0.5);
        write_mod_slot(&mut s0, MOD_SLOTS_1_16 + MOD_SLOT_LEN, 1, 0, 316, 0.0);
        write_mod_slot(&mut s0, MOD_SLOTS_17_32, 16, 6, 22, -0.75);
        put_f32(&mut s0, OFF_SWITCHES, 1.0);
        put_f32(&mut s0, OFF_SWITCHES + 0x10, 1.0);
        put_f32(&mut s0, OFF_SWITCHES + 0x14, 1.0);
        put_f32(&mut s0, OFF_SWITCHES + 0x20, 0.5);
        put_f32(&mut s0, OFF_SWITCHES + 0x2C, 7.0 / 31.0);
        put_f32(&mut s0, OFF_SWITCHES + 0x5C, 1.0);
        put_f32(&mut s0, OFF_SCALARS, 0.25);
        put_f32(&mut s0, OFF_SCALARS + SCALARS_BLOCK_LEN, 0.5);
        put_u32(&mut s0, OFF_LFO_POINT_MOD_COUNT, 1);
        put_u32(&mut s0, OFF_LFO_POINT_MODS, 3);
        put_u32(&mut s0, OFF_LFO_POINT_MODS + 4, 1);
        put_u32(&mut s0, OFF_LFO_POINT_MODS + 8, 2);
        put_u32(&mut s0, OFF_LFO_POINT_MODS + 0x0C, 150);
        let lfo = &mut s0[LFO_BLOCK_BASE..LFO_BLOCK_BASE + LFO_BLOCK_SIZE];
        lfo[LFO_OFF_FLAGS..LFO_OFF_FLAGS + 7].copy_from_slice(&[1, 0, 0, 0, 1, 1, 0]);
        put_u32(lfo, LFO_OFF_NUM_POINTS, 4);
        put_u32(lfo, LFO_OFF_PHASE, 0xFFFF_FFFF);
        put_u32(lfo, LFO_OFF_LOOPBACK_POINT_NUM, 0x1E1);
        put_f32(lfo, LFO_OFF_RATE, 0.59211);
        put_f32(lfo, LFO_OFF_SMOOTH, 0.125);
        put_f32(lfo, LFO_OFF_DELAY, 0.25);
        put_f32(lfo, LFO_OFF_RISE, 0.75);
        put_f64(lfo, 0, 0.5);
        put_f64(lfo, LFO_CURVE_BYTES, 0.125);
        put_f64(lfo, 2 * LFO_CURVE_BYTES, 1.0);
        put_f64(lfo, 2 * LFO_CURVE_BYTES + 8, 0.0);
        s0
    }

    fn chunk_from(state: &[u8], streams: &[Vec<u8>]) -> Vec<u8> {
        let z0 = zlib_stream(state);
        let mut chunk = z0.clone();
        for s in streams {
            chunk.extend_from_slice(&zlib_stream(s));
        }
        chunk.extend_from_slice(&(z0.len() as u32).to_le_bytes());
        chunk
    }

    fn synth_chunk() -> Vec<u8> {
        chunk_from(&synth_state(), &[vec![0u8; 2 * FRAME_BYTES]])
    }

    #[test]
    fn parses_synthetic_preset() {
        let p = parse_preset(&synth_chunk()).unwrap();
        assert_eq!(p.blob.len(), S1_BLOB_SIZE);
        assert_eq!(p.streams.len(), 1);
        assert_eq!(p.streams[0].len(), 2 * FRAME_BYTES);
        assert_eq!(p.meta.preset_name, "Synth");
        assert_eq!(p.meta.author, "Jane");
        assert_eq!(p.meta.category, "");
        assert!((p.meta.version_f32 - 0.1631).abs() < 1e-6);
        assert!((p.master_param(0) - 0.7).abs() < 1e-6);
        assert!((p.master_param(61) - 0.25).abs() < 1e-6);
        assert_eq!(p.master_param(MASTER_PARAM_COUNT), 0.0);
        assert!((p.aux_param(0) - 0.25).abs() < 1e-6);
        assert_eq!(p.aux_param(AUX_PARAM_COUNT), 0.0);
        assert_eq!(p.fx_order(), [5, 0, 1, 2, 3, 6, 7, 8, 9, 4]);
        assert_eq!(p.wt_name_a(), r"Tables\My Saw.wav");
        assert_eq!(p.noise_name(), "Noise/AC hum1.wav");
        assert_eq!(p.macro_name(0), "Macro A");
        assert_eq!(p.macro_name(3), "Macro D");
        assert_eq!(p.macro_name(4), "");
        assert_eq!(p.osc_wt_frames(), [8192, 16384]);
        assert!(p.interpolate_after_load());
        assert_eq!(p.lock_bits(), 0b0110);
        assert_eq!(p.preset_id(), [0u8; 8]);
        assert_eq!(p.stored_phase_pos()[..4], 0x1122_3344u32.to_le_bytes());
        assert_eq!(p.stored_phase_pos()[4..], 0x5566_7788u32.to_le_bytes());
        assert_eq!(p.loopback64()[..4], 0xAABB_CCDDu32.to_le_bytes());
        assert_eq!(p.boundary64()[..4], 0xEEFF_0011u32.to_le_bytes());
        assert_eq!(p.tuning_bytes(), S1Tuning::default());
    }

    #[test]
    fn switches_helpers() {
        let p = parse_preset(&synth_chunk()).unwrap();
        assert!(p.switches().len() >= SWITCHES_SLICE_LEN);
        assert!((p.switch_a4() - 1.0).abs() < 1e-6);
        assert!((p.switch_polyphony() - 7.0 / 31.0).abs() < 1e-6);
        assert!((p.switch_mono() - 1.0).abs() < 1e-6);
        assert!((p.switch_legato() - 1.0).abs() < 1e-6);
        assert!((p.switch_reverb_hall() - 1.0).abs() < 1e-6);
        assert!((p.switch_oversampling() - 0.5).abs() < 1e-6);
        assert_eq!(p.switch_f32(0x04), 0.0);
        assert_eq!(p.switch_f32(SWITCHES_LEN), 0.0);
    }

    #[test]
    fn scalars_and_midi_map() {
        let p = parse_preset(&synth_chunk()).unwrap();
        let velo = p.scalars_curve(0);
        assert_eq!(velo.len(), SCALARS_BLOCK_LEN);
        assert!((f32::from_le_bytes(velo[..4].try_into().unwrap()) - 0.25).abs() < 1e-6);
        assert_eq!(p.scalars_curve(1).len(), SCALARS_BLOCK_LEN);
        assert!(p.scalars_curve(2).is_empty());
        assert_eq!(p.midi_map(), S1MidiMap::default());
        let mut state = synth_state();
        state[OFF_MIDI_MAP + 7] = 0x40;
        state[OFF_MIDI_MAP_EXTRA + 3] = 0x20;
        let q = parse_preset(&chunk_from(&state, &[vec![0u8; FRAME_BYTES]])).unwrap();
        let map = q.midi_map();
        assert_eq!(map.param_cc[7], 0x40);
        assert_eq!(map.extra_cc[3], 0x20);
    }

    #[test]
    fn lfo_block_fields() {
        let p = parse_preset(&synth_chunk()).unwrap();
        let block = p.lfo_block(0).unwrap();
        assert_eq!(block.len(), LFO_BLOCK_SIZE);
        assert_eq!(lfo_flags(block), [1, 0, 0, 0, 1, 1, 0]);
        assert_eq!(lfo_num_points(block), 4);
        assert_eq!(lfo_phase_raw(block), 0xFFFF_FFFF);
        assert_eq!(lfo_loopback_point_num(block), 0x1E1);
        assert!((lfo_rate(block) - 0.59211).abs() < 1e-6);
        assert!((lfo_smooth(block) - 0.125).abs() < 1e-6);
        assert!((lfo_delay(block) - 0.25).abs() < 1e-6);
        assert!((lfo_rise(block) - 0.75).abs() < 1e-6);
        assert!(p.lfo_block(1).is_some());
        assert!(p.lfo_block(LFO_BLOCK_COUNT).is_none());
        assert!(p.flex_lfo_block(1).is_some());
        assert!(p.flex_lfo_block(LFO_FLEX_COUNT).is_none());
        let curves = lfo_curve_vals(block).unwrap();
        assert_eq!(curves.len(), LFO_CURVE_BYTES);
        assert!((f64::from_le_bytes(curves[..8].try_into().unwrap()) - 0.5).abs() < 1e-12);
        let x = lfo_x_vals(block).unwrap();
        assert!((f64::from_le_bytes(x[..8].try_into().unwrap()) - 0.125).abs() < 1e-12);
        let y = lfo_y_vals(block).unwrap();
        assert!((f64::from_le_bytes(y[..8].try_into().unwrap()) - 1.0).abs() < 1e-12);
        assert!((f64::from_le_bytes(y[8..16].try_into().unwrap()) - 0.0).abs() < 1e-12);
        assert_eq!(lfo_num_points(&block[..0x100]), 0);
        assert_eq!(lfo_flags(&block[..10]), [0u8; 7]);
        assert!(lfo_curve_vals(&block[..16]).is_none());
    }

    #[test]
    fn classic_lfo_regions() {
        let p = parse_preset(&synth_chunk()).unwrap();
        for k in 0..LFO_BLOCK_COUNT {
            assert_eq!(p.classic_lfo_block(k).unwrap().len(), LFO_CLASSIC_SIZE);
        }
        assert!(p.classic_lfo_block(LFO_BLOCK_COUNT).is_none());
        assert!(p.classic_lfo_5_8_alt().is_some());
        assert!(p.classic_lfo_block(0).unwrap().iter().all(|&b| b == 0));
    }

    #[test]
    fn lfo_point_mods_records() {
        let p = parse_preset(&synth_chunk()).unwrap();
        let mods = p.lfo_point_mods();
        assert_eq!(
            mods,
            vec![S1LfoPointMod {
                lfo: 3,
                point: 1,
                mode: 2,
                param: 150
            }]
        );
        let mut state = synth_state();
        put_u32(&mut state, OFF_LFO_POINT_MOD_COUNT, 999_999);
        let q = parse_preset(&chunk_from(&state, &[])).unwrap();
        let cap = (S1_BLOB_SIZE - OFF_LFO_POINT_MODS) / LFO_POINT_MOD_LEN;
        assert!(q.lfo_point_mods().len() <= cap);
    }

    #[test]
    fn mod_slots_parsed_and_validated() {
        let p = parse_preset(&synth_chunk()).unwrap();
        assert_eq!(p.mod_slots.len(), MOD_SLOT_COUNT);
        let s0 = &p.mod_slots[0];
        assert_eq!(s0.slot, 0);
        assert_eq!(s0.source_t, 5);
        assert_eq!(s0.dest, 1);
        assert!((s0.amount_a - 0.5).abs() < 1e-6);
        assert!((s0.amount_b - 1.0).abs() < 1e-6);
        assert_eq!(s0.bipolar_src, 0);
        assert_eq!(s0.curve_a, 0x80);
        assert_eq!(s0.aux_curve, 0x80);
        assert_eq!(&s0.raw[0x20..0x24], &[0x80, 0x80, 0x00, 0xFF]);
        let unused = p.mod_slots.iter().find(|s| s.slot == 1).unwrap();
        assert_eq!(unused.source_t, 0);
        assert_eq!(unused.dest, 316);
        assert!((unused.amount_a).abs() < 1e-6);
        let s16 = p.mod_slots.iter().find(|s| s.slot == 16).unwrap();
        assert_eq!(s16.source_t, 6);
        assert_eq!(s16.dest, 22);
        assert!((s16.amount_a + 0.75).abs() < 1e-6);
    }

    #[test]
    fn mod_slot_marker_corruption_drops_record() {
        let mut state = synth_state();
        let off = MOD_SLOTS_1_16 + MOD_SLOT_LEN * 3;
        state[off + 0x23] = 0x00;
        let p = parse_preset(&chunk_from(&state, &[])).unwrap();
        assert!(p.mod_slots.iter().all(|s| s.slot != 3));
        assert_eq!(p.mod_slots.len(), MOD_SLOT_COUNT - 1);
    }

    #[test]
    fn mod_slot_field_validation() {
        let mut state = synth_state();
        write_mod_slot(
            &mut state,
            MOD_SLOTS_1_16 + 2 * MOD_SLOT_LEN,
            2,
            70,
            10,
            0.5,
        );
        let p = parse_preset(&chunk_from(&state, &[])).unwrap();
        assert!(p.mod_slots.iter().all(|s| s.slot != 2));

        let mut state = synth_state();
        write_mod_slot(&mut state, MOD_SLOTS_17_32, 16, 6, 2000, 0.5);
        let p = parse_preset(&chunk_from(&state, &[])).unwrap();
        assert!(p.mod_slots.iter().all(|s| s.slot != 16));

        let mut state = synth_state();
        write_mod_slot(
            &mut state,
            MOD_SLOTS_17_32 + MOD_SLOT_LEN,
            17,
            6,
            22,
            f32::NAN,
        );
        let p = parse_preset(&chunk_from(&state, &[])).unwrap();
        assert!(p.mod_slots.iter().all(|s| s.slot != 17));

        let mut state = synth_state();
        write_mod_slot(
            &mut state,
            MOD_SLOTS_17_32 + 2 * MOD_SLOT_LEN,
            18,
            6,
            22,
            9.0,
        );
        let p = parse_preset(&chunk_from(&state, &[])).unwrap();
        assert!(p.mod_slots.iter().all(|s| s.slot != 18));
    }

    #[test]
    fn mod_slot_fallback_scan() {
        let mut state = synth_state();
        let off = MOD_SLOTS_17_32 + 3 * MOD_SLOT_LEN;
        write_mod_slot(&mut state, off, 19, 6, 22, 0.5);
        let mut relocated = [0u8; MOD_SLOT_LEN];
        relocated.copy_from_slice(&state[off..off + MOD_SLOT_LEN]);
        state[off..off + MOD_SLOT_LEN].fill(0);
        let dest = 0x5390;
        state[dest..dest + MOD_SLOT_LEN].copy_from_slice(&relocated);
        let p = parse_preset(&chunk_from(&state, &[])).unwrap();
        let s19 = p.mod_slots.iter().find(|s| s.slot == 19).unwrap();
        assert_eq!(s19.source_t, 6);
        assert_eq!(s19.dest, 22);
        assert!((s19.amount_a - 0.5).abs() < 1e-6);
        assert_eq!(p.mod_slots.len(), MOD_SLOT_COUNT);
    }

    #[test]
    fn rejects_old_format_blob() {
        for size in S1_OLD_BLOB_SIZES {
            let chunk = chunk_from(&vec![0u8; size], &[]);
            let err = parse_preset(&chunk).unwrap_err();
            assert!(
                err.contains("old-format Serum 1 preset not supported by the converter"),
                "{err}"
            );
        }
    }

    #[test]
    fn rejects_non_frame_stream() {
        let chunk = chunk_from(&synth_state(), &[vec![0u8; 8000]]);
        let err = parse_preset(&chunk).unwrap_err();
        assert!(err.contains("not a multiple of the 8192-byte"), "{err}");
    }

    #[test]
    fn rejects_oversized_decompress() {
        let big = vec![0u8; MAX_STREAM_DECOMP + 1024];
        let chunk = chunk_from(&synth_state(), &[big]);
        let err = parse_preset(&chunk).unwrap_err();
        assert!(err.contains("sanity limit"), "{err}");
    }

    #[test]
    fn rejects_garbage_chunk() {
        assert!(parse_preset(&[]).is_err());
        assert!(parse_preset(&[0u8; 4]).is_err());
        assert!(parse_preset(b"garbage data here").is_err());
        let mut chunk = synth_chunk();
        chunk[0] = 0x79;
        assert!(parse_preset(&chunk).is_err());
    }

    // Real-fixture regression check, not run by default: parses the five
    // Serum 1 .fxp presets in the directory named by the S1_FXP_DIR
    // environment variable and asserts the byte-verified expectations from
    // docs/s1-params.md §10. Run with:
    //   S1_FXP_DIR=<path to the conv_work fxp fixture dir> cargo test s1state
    // (the test silently does nothing when the variable is unset, so plain
    // `cargo test` never depends on files outside the repo).
    #[test]
    fn real_fixture_presets() {
        let Ok(dir) = std::env::var("S1_FXP_DIR") else {
            return;
        };
        let expected = [
            "- Init -reese",
            "Chord_Hyperpop_Chord",
            "indigo - basic shapes sub",
            "- Init -",
            "BS - YUKIYANAGI UKHC BASS 01",
        ];
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "fxp"))
            .collect();
        paths.sort();
        assert_eq!(paths.len(), 5, "{paths:?}");
        let mut parsed = Vec::new();
        for path in &paths {
            let data = std::fs::read(path).unwrap();
            assert_eq!(&data[..4], b"CcnK");
            assert_eq!(&data[8..12], b"FPCh");
            let cs = u32::from_be_bytes(data[0x38..0x3C].try_into().unwrap()) as usize;
            let p = parse_preset(&data[0x3C..0x3C + cs]).unwrap();
            assert_eq!(p.blob.len(), S1_BLOB_SIZE);
            assert!((p.meta.version_f32 - 0.1631).abs() < 1e-4);
            assert!(
                expected.contains(&p.meta.preset_name.as_str()),
                "unexpected name {:?}",
                p.meta.preset_name
            );
            let mut sorted = p.fx_order();
            sorted.sort();
            assert_eq!(sorted, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
            for stream in &p.streams {
                assert_eq!(stream.len() % FRAME_BYTES, 0);
            }
            assert!(p.streams.iter().any(|s| !s.is_empty()));
            assert!(p.classic_lfo_block(0).unwrap().iter().all(|&b| b == 0));
            assert_eq!(p.mod_slots.len(), MOD_SLOT_COUNT);
            parsed.push(p);
        }
        let yuki = parsed
            .iter()
            .find(|p| p.meta.preset_name == "BS - YUKIYANAGI UKHC BASS 01")
            .unwrap();
        let used: Vec<&S1ModSlot> = yuki.mod_slots.iter().filter(|s| s.source_t != 0).collect();
        assert_eq!(used.len(), 3);
        let expect = [
            (0u8, 5u16, 1u16, 0.5132f32),
            (1, 5, 14, 0.4605),
            (2, 6, 22, 0.1491),
        ];
        for (slot, e) in used.iter().zip(expect) {
            assert_eq!(slot.slot, e.0);
            assert_eq!(slot.source_t, e.1);
            assert_eq!(slot.dest, e.2);
            assert!((slot.amount_a - e.3).abs() < 1e-3);
            assert_eq!(slot.bipolar_src, 0);
        }
        assert_eq!(yuki.fx_order(), [5, 0, 1, 2, 3, 7, 9, 4, 6, 8]);
        assert!((yuki.switch_a4() - 1.0).abs() < 1e-6);
        assert!((yuki.switch_polyphony() - 7.0 / 31.0).abs() < 1e-6);
        let lfo1 = yuki.lfo_block(0).unwrap();
        assert_eq!(lfo_flags(lfo1), [1, 0, 0, 0, 1, 1, 0]);
        assert_eq!(lfo_num_points(lfo1), 4);
        assert!((lfo_rate(lfo1) - yuki.master_param(61)).abs() < 1e-6);
        assert!((lfo_rate(lfo1) - 0.59211).abs() < 1e-4);
        assert!((lfo_smooth(lfo1) - 0.0).abs() < 1e-6);
        assert!((lfo_delay(lfo1) - 0.0).abs() < 1e-6);
        assert!((lfo_rise(lfo1) - 0.0).abs() < 1e-6);
        let lfo2 = yuki.lfo_block(1).unwrap();
        assert!((lfo_rate(lfo2) - yuki.master_param(62)).abs() < 1e-6);
        assert!((lfo_rate(lfo2) - 0.5).abs() < 1e-6);
        assert_eq!(lfo_num_points(lfo2), 2);
        assert_eq!(yuki.wt_name_a(), r"\Analog\DS Saw and Tri.wav");
        assert_eq!(yuki.wt_name_b(), r"\Adventure Kid\raw.wav");
        assert_eq!(yuki.noise_name(), "/Analog/SID noise.wav");
        let reese = parsed
            .iter()
            .find(|p| p.meta.preset_name == "- Init -reese")
            .unwrap();
        assert_eq!(reese.fx_order(), [2, 3, 4, 5, 6, 0, 7, 8, 9, 1]);
        assert!((reese.switch_mono() - 1.0).abs() < 1e-6);
        assert!((reese.switch_legato() - 1.0).abs() < 1e-6);
    }
}

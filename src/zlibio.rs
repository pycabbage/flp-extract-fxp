//! Shared zlib inflate and Serum chunk-stream splitting.
//!
//! All callers share a single sanity cap: [`MAX_STREAM`] (32 MiB) on the
//! decompressed size of one zlib stream. (The fxp validation path previously
//! used an arbitrary 16 MiB cap; it is unified here.)

use flate2::read::ZlibDecoder;

/// Sanity cap on the decompressed size of a single zlib stream.
pub const MAX_STREAM: usize = 32 * 1024 * 1024;

/// Inflate one zlib stream starting at `data[0]`.
/// Returns the decompressed bytes and the number of input bytes consumed.
pub fn inflate(data: &[u8], max_out: usize) -> Result<(Vec<u8>, usize), String> {
    use std::io::Read;
    if data.is_empty() || data[0] != 0x78 {
        return Err("not a zlib stream (expected 0x78 header byte)".into());
    }
    let mut dec = ZlibDecoder::new(data);
    let mut out = Vec::with_capacity(4096);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match dec.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&chunk[..n]);
                if out.len() > max_out {
                    return Err("decompressed stream exceeds sanity limit".into());
                }
            }
            Err(e) => return Err(format!("zlib error: {e}")),
        }
    }
    Ok((out, dec.total_in() as usize))
}

/// Split a Serum 1 chunk into its zlib streams and validate the trailer.
///
/// Layout: `[zlib stream 0][zlib stream 1]...[u32 LE trailer]` where the
/// trailer equals the compressed size of stream 0 (the preset state; the
/// remaining streams carry embedded wavetable / noise data). A minimal zlib
/// stream is 8 bytes, so a 4-byte tail can only be the trailer. A failure in
/// any stream after the first ends the stream walk (a chunk boundary may
/// carry non-stream bytes); a failure in the first stream is fatal.
pub fn split_chunk(chunk: &[u8]) -> Result<(Vec<Vec<u8>>, bool), String> {
    if chunk.len() < 8 {
        return Err("chunk too small to contain a zlib stream".into());
    }
    let candidate_trailer =
        u32::from_le_bytes(chunk[chunk.len() - 4..].try_into().unwrap()) as usize;
    let mut pos = 0usize;
    let mut streams = Vec::new();
    while chunk.len() - pos > 4 {
        if chunk[pos] != 0x78 {
            if pos == 0 {
                return Err("chunk does not start with a zlib stream".into());
            }
            break;
        }
        let (out, consumed) = match inflate(&chunk[pos..], MAX_STREAM) {
            Ok(v) => v,
            Err(e) => {
                if pos == 0 {
                    return Err(e);
                }
                break;
            }
        };
        streams.push(out);
        pos += consumed;
    }
    if streams.is_empty() {
        return Err("chunk contains no zlib streams".into());
    }
    // The consumed prefix must cover every byte except the trailer.
    let clean_tail = chunk.len() - pos == 4;
    let (_, s0_len) = inflate(chunk, MAX_STREAM)?;
    let has_trailer = clean_tail && candidate_trailer == s0_len;
    Ok((streams, has_trailer))
}

// src/lib.rs

pub mod header;
pub mod filter;
pub mod encode;
pub mod decode;

pub use header::{
    MpxHeader, ColorType, FilterType,
    HEADER_SIZE, FLAG_BYTE_PLANE_SPLIT, FLAG_INTER_CHANNEL_DELTA, FLAG_YCOCG_R,
};
pub use encode::encode;
pub use decode::decode;

use std::io;

/// Encode raw interleaved pixels to MPX.
///
/// Color transform selection for 8-bit RGB/RGBA:
///   • Compressible images (entropy < 7.5 bits/byte): YCoCg-R
///     Better decorrelation — Co/Cg near zero for natural photos.
///     Co/Cg stored as i16 (2 bytes/sample); hi-byte plane is near-uniform
///     (0x00 or 0xFF) and compresses to near zero via MBFA.
///   • Incompressible images (entropy ≥ 7.5 bits/byte): simple ICD (G=G-R, B=B-G)
///     YCoCg-R would expand Co/Cg planes to 2x size for no gain on random data.
///     ICD keeps planes at 1 byte/sample and passhthroughs at ~1.001x overhead.
///
/// 16-bit images: byte-plane split only, no color transform.
/// GrayA: no color transform (only 2 channels).
/// Gray: no transform.
pub fn encode_image(
    width:      u32,
    height:     u32,
    color_type: ColorType,
    bit_depth:  u8,
    filter:     FilterType,
    pixels:     &[u8],
) -> io::Result<Vec<u8>> {
    let mut flags = 0u8;

    if bit_depth == 16 {
        flags |= FLAG_BYTE_PLANE_SPLIT;
    }

    // Select color transform for 8-bit RGB and RGBA.
    if bit_depth == 8
        && (color_type == ColorType::Rgb || color_type == ColorType::Rgba)
    {
        if is_likely_compressible(pixels) {
            // YCoCg-R: better decorrelation for natural photos.
            // Co/Cg channels become i16, handled via per-channel sizing.
            flags |= FLAG_YCOCG_R;
        } else {
            // Fallback ICD: same size passthrough for incompressible/random data.
            flags |= FLAG_INTER_CHANNEL_DELTA;
        }
    }

    let header = MpxHeader {
        color_type,
        bit_depth,
        filter_type: filter,
        width,
        height,
        flags,
    };
    encode(&header, pixels)
}

/// Decode an MPX file. Returns (header, raw interleaved pixels).
pub fn decode_image(data: &[u8]) -> io::Result<(MpxHeader, Vec<u8>)> {
    decode(data)
}

pub fn pixel_buffer_size(w: u32, h: u32, ct: ColorType, bpp: u8) -> usize {
    w as usize * h as usize * ct.channel_count() * (bpp / 8) as usize
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Estimate whether pixel data is likely compressible by sampling Shannon entropy.
///
/// Random/incompressible RGB images score ≈ 7.9–8.0 bits/byte.
/// Natural photos typically score 5.0–7.2 bits/byte.
/// Synthetic gradients/pixel art score < 4 bits/byte.
///
/// Threshold 7.5 conservatively separates "likely compressible" from
/// "high-entropy / probably incompressible". False negatives (compressible
/// data above threshold) are rare and simply get ICD instead of YCoCg-R —
/// still lossless, just slightly less compression. False positives (random
/// data below threshold) are extremely rare.
#[inline]
fn is_likely_compressible(pixels: &[u8]) -> bool {
    const SAMPLE: usize = 8192;
    const THRESHOLD: f64 = 7.5;

    let sample = if pixels.len() > SAMPLE { &pixels[..SAMPLE] } else { pixels };
    if sample.is_empty() { return true; }

    let mut freq = [0u32; 256];
    for &b in sample { freq[b as usize] += 1; }

    let n: f64 = sample.len() as f64;
    let entropy: f64 = freq.iter()
        .filter(|&&c| c > 0)
        .map(|&c| { let p = c as f64 / n; -p * p.log2() })
        .sum();

    entropy < THRESHOLD
}

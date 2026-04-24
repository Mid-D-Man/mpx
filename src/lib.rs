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
/// RGB and RGBA images use YCoCg-R for better decorrelation on natural photos.
/// GrayA uses simple inter-channel delta.
/// Single-channel Gray uses neither.
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

    match color_type {
        ColorType::Rgb | ColorType::Rgba if bit_depth == 8 => {
            // YCoCg-R: better than simple ICD for natural photos.
            // For 16-bit we fall through to no color transform (rare case,
            // typically medical/scientific where channel independence matters).
            flags |= FLAG_YCOCG_R;
        }
        ColorType::GrayA => {
            // Simple delta between Gray and Alpha plane.
            flags |= FLAG_INTER_CHANNEL_DELTA;
        }
        _ => {}
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

// Auto-generated stub
// src/lib.rs

pub mod header;
pub mod filter;
pub mod encode;
pub mod decode;

pub use header::{
    MpxHeader, ColorType, FilterType,
    HEADER_SIZE, FLAG_BYTE_PLANE_SPLIT,
};
pub use encode::encode;
pub use decode::decode;

use std::io;

// ── Convenience API ───────────────────────────────────────────────────────────

/// Encode raw interleaved pixel bytes to MPX format.
///
/// `pixels` layout: row-major, channels interleaved.
///   8-bit RGB:   R G B R G B ...
///   16-bit RGBA: [R_lo R_hi G_lo G_hi B_lo B_hi A_lo A_hi] per pixel (LE u16)
///
/// For 16-bit images, FLAG_BYTE_PLANE_SPLIT is always set automatically.
///
/// Returns the complete MPX file as bytes.
pub fn encode_image(
    width:      u32,
    height:     u32,
    color_type: ColorType,
    bit_depth:  u8,
    filter:     FilterType,
    pixels:     &[u8],
) -> io::Result<Vec<u8>> {
    let flags = if bit_depth == 16 { FLAG_BYTE_PLANE_SPLIT } else { 0 };
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

/// Decode an MPX file.
///
/// Returns `(header, pixels)` with the same interleaved layout as
/// `encode_image` expects.
pub fn decode_image(data: &[u8]) -> io::Result<(MpxHeader, Vec<u8>)> {
    decode(data)
}

/// Compute the uncompressed pixel buffer size for given dimensions.
pub fn pixel_buffer_size(width: u32, height: u32, color_type: ColorType, bit_depth: u8) -> usize {
    width as usize
        * height as usize
        * color_type.channel_count()
        * (bit_depth / 8) as usize
                                    }

// src/lib.rs

pub mod header;
pub mod filter;
pub mod encode;
pub mod decode;

pub use header::{
    MpxHeader, ColorType, FilterType,
    HEADER_SIZE, FLAG_BYTE_PLANE_SPLIT, FLAG_INTER_CHANNEL_DELTA,
};
pub use encode::encode;
pub use decode::decode;

use std::io;

/// Encode raw interleaved pixels to MPX.
///
/// `inter_channel_delta`: set true for natural photos — makes G=(G-R), B=(B-G)
/// before spatial filtering. Creates near-zero residual planes that MBFA's
/// multi-fold compresses extremely well (long identical BACKREF token runs).
pub fn encode_image(
    width:               u32,
    height:              u32,
    color_type:          ColorType,
    bit_depth:           u8,
    filter:              FilterType,
    inter_channel_delta: bool,
    pixels:              &[u8],
) -> io::Result<Vec<u8>> {
    let mut flags = 0u8;
    if bit_depth == 16            { flags |= FLAG_BYTE_PLANE_SPLIT; }
    if inter_channel_delta
        && color_type.channel_count() > 1 { flags |= FLAG_INTER_CHANNEL_DELTA; }

    let header = MpxHeader { color_type, bit_depth, filter_type: filter, width, height, flags };
    encode(&header, pixels)
}

/// Decode an MPX file. Returns (header, raw interleaved pixels).
pub fn decode_image(data: &[u8]) -> io::Result<(MpxHeader, Vec<u8>)> {
    decode(data)
}

pub fn pixel_buffer_size(w: u32, h: u32, ct: ColorType, bpp: u8) -> usize {
    w as usize * h as usize * ct.channel_count() * (bpp / 8) as usize
}

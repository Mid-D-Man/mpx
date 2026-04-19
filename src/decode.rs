// src/decode.rs

use std::io;
use crate::header::{MpxHeader, HEADER_SIZE};
use crate::filter::undo_filter;

pub fn decode(data: &[u8]) -> io::Result<(MpxHeader, Vec<u8>)> {
    let header          = MpxHeader::parse(data)?;
    let w               = header.width  as usize;
    let h               = header.height as usize;
    let channels        = header.channel_count();
    let bps             = header.bytes_per_sample();
    let plane_bytes     = header.plane_bytes();
    let plane_row_bytes = header.plane_row_bytes();

    // ── 1. Decompress each channel block ──────────────────────────────────────
    let mut cursor = HEADER_SIZE;
    let mut planes: Vec<Vec<u8>> = Vec::with_capacity(channels);

    for ch in 0..channels {
        if cursor + 4 > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("channel {} block header truncated", ch),
            ));
        }
        let comp_len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;

        if cursor + comp_len > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("channel {} data truncated", ch),
            ));
        }

        let decompressed = mbfa::decompress(&data[cursor..cursor + comp_len])
            .map_err(|e| io::Error::new(e.kind(), format!("ch{}: {}", ch, e)))?;
        cursor += comp_len;

        if decompressed.len() != plane_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("ch{}: got {} bytes expected {}", ch, decompressed.len(), plane_bytes),
            ));
        }
        planes.push(decompressed);
    }

    // ── 2. Undo byte-plane split ──────────────────────────────────────────────
    if header.byte_plane_split() {
        for plane in &mut planes {
            let n  = plane.len() / 2;
            let hi = plane[..n].to_vec();
            let lo = plane[n..].to_vec();
            let mut out = vec![0u8; plane.len()];
            for i in 0..n {
                out[i * 2]     = lo[i];
                out[i * 2 + 1] = hi[i];
            }
            *plane = out;
        }
    }

    // ── 3. Undo spatial prediction filter ────────────────────────────────────
    let filter = header.filter_type;
    for plane in &mut planes {
        let mut prev_row = vec![0u8; plane_row_bytes];
        for row in 0..h {
            let row_off = row * plane_row_bytes;
            undo_filter(
                filter,
                &mut plane[row_off..row_off + plane_row_bytes],
                &prev_row,
                bps,
            );
            prev_row.copy_from_slice(&plane[row_off..row_off + plane_row_bytes]);
        }
    }

    // ── 4. Undo inter-channel delta ───────────────────────────────────────────
    // Inverse: G = (G-R)+R, B = (B-G)+G. Must add in forward order.
    if header.inter_channel_delta() {
        for c in 1..channels.min(3) {
            let (left, right) = planes.split_at_mut(c);
            let prev = left[c - 1].clone(); // reconstructed previous channel
            let curr = &mut right[0];
            for i in 0..plane_bytes {
                curr[i] = curr[i].wrapping_add(prev[i]);
            }
        }
    }

    // ── 5. Reassemble interleaved pixels ──────────────────────────────────────
    let mut pixels = vec![0u8; w * h * channels * bps];
    for row in 0..h {
        for col in 0..w {
            let pixel_base = (row * w + col) * channels * bps;
            for c in 0..channels {
                let src = (row * w + col) * bps;
                let dst = pixel_base + c * bps;
                pixels[dst..dst + bps].copy_from_slice(&planes[c][src..src + bps]);
            }
        }
    }

    Ok((header, pixels))
}

// Auto-generated stub
// src/decode.rs
//! MPX decoder pipeline — exact inverse of encode.rs.

use std::io;
use crate::header::{MpxHeader, HEADER_SIZE};
use crate::filter::undo_filter;

pub fn decode(data: &[u8]) -> io::Result<(MpxHeader, Vec<u8>)> {
    let header   = MpxHeader::parse(data)?;
    let w        = header.width  as usize;
    let h        = header.height as usize;
    let channels = header.channel_count();
    let bps      = header.bytes_per_sample();

    let plane_bytes     = header.plane_bytes();
    let plane_row_bytes = header.plane_row_bytes();
    let stride          = bps;

    // ── Step 1: read and decompress each channel block ────────────────────────
    let mut cursor = HEADER_SIZE;
    let mut planes: Vec<Vec<u8>> = Vec::with_capacity(channels);

    for ch in 0..channels {
        if cursor + 4 > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("MPX: truncated block header for channel {}", ch),
            ));
        }

        let compressed_len = u32::from_le_bytes(
            data[cursor..cursor + 4].try_into().unwrap()
        ) as usize;
        cursor += 4;

        if cursor + compressed_len > data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "MPX: channel {} block claims {} bytes but only {} remain",
                    ch, compressed_len, data.len() - cursor
                ),
            ));
        }

        let decompressed = mbfa::decompress(&data[cursor..cursor + compressed_len])
            .map_err(|e| io::Error::new(e.kind(), format!("channel {}: {}", ch, e)))?;
        cursor += compressed_len;

        // Validate decompressed size
        let expected = if header.byte_plane_split() {
            // Byte-plane split: same total bytes, just rearranged
            plane_bytes
        } else {
            plane_bytes
        };

        if decompressed.len() != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "MPX: channel {} decompressed to {} bytes, expected {}",
                    ch, decompressed.len(), expected
                ),
            ));
        }

        planes.push(decompressed);
    }

    // ── Step 2: undo byte-plane split ─────────────────────────────────────────
    if header.byte_plane_split() {
        for plane in &mut planes {
            let n = plane.len() / 2;
            let hi = plane[..n].to_vec();
            let lo = plane[n..].to_vec();

            let mut reassembled = vec![0u8; plane.len()];
            for i in 0..n {
                reassembled[i * 2]     = lo[i]; // low byte
                reassembled[i * 2 + 1] = hi[i]; // high byte
            }
            *plane = reassembled;
        }
    }

    // ── Step 3: undo spatial prediction filter ────────────────────────────────
    let filter = header.filter_type;

    for plane in &mut planes {
        let mut prev_row = vec![0u8; plane_row_bytes];

        for row in 0..h {
            let row_off = row * plane_row_bytes;
            undo_filter(
                filter,
                &mut plane[row_off..row_off + plane_row_bytes],
                &prev_row,
                stride,
            );
            // prev_row must be the RECONSTRUCTED row for the next iteration
            prev_row.copy_from_slice(&plane[row_off..row_off + plane_row_bytes]);
        }
    }

    // ── Step 4: reassemble interleaved pixels ─────────────────────────────────
    let total_bytes = w * h * channels * bps;
    let mut pixels  = vec![0u8; total_bytes];

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

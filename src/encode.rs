// Auto-generated stub
// src/encode.rs
//! MPX encoder pipeline.
//!
//! Pipeline:
//!   1. Channel separation:    interleaved RGBARGBA... → R-plane G-plane B-plane A-plane
//!   2. Spatial filter:        per row, per plane → residuals near zero
//!   3. Byte-plane split:      16-bit only → split high/low bytes of each plane
//!   4. MBFA compression:      per plane via mbfa::compress
//!   5. Serialisation:         header + [len: u32 LE][compressed bytes] per channel

use std::io;
use crate::header::{MpxHeader, FilterType, FLAG_BYTE_PLANE_SPLIT, HEADER_SIZE};
use crate::filter::{apply_filter, select_best_filter};

pub fn encode(header: &MpxHeader, pixels: &[u8]) -> io::Result<Vec<u8>> {
    let w        = header.width  as usize;
    let h        = header.height as usize;
    let channels = header.channel_count();
    let bps      = header.bytes_per_sample();
    let row_bytes = w * channels * bps;

    let expected = h * row_bytes;
    if pixels.len() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("pixel buffer size mismatch: expected {} bytes, got {}", expected, pixels.len()),
        ));
    }

    // ── Step 1: channel separation ────────────────────────────────────────────
    // Deinterleave RGBARGBA... into separate planes.
    // plane[c][row * w * bps .. (row+1) * w * bps] = channel c, all samples in row.
    let plane_bytes = w * h * bps;
    let mut planes: Vec<Vec<u8>> = (0..channels).map(|_| vec![0u8; plane_bytes]).collect();

    for row in 0..h {
        for col in 0..w {
            let pixel_base = (row * w + col) * channels * bps;
            for c in 0..channels {
                let src = pixel_base + c * bps;
                let dst = (row * w + col) * bps;
                planes[c][dst..dst + bps].copy_from_slice(&pixels[src..src + bps]);
            }
        }
    }

    // ── Step 2: spatial prediction filter ────────────────────────────────────
    let plane_row_bytes = header.plane_row_bytes();
    let stride          = bps;
    let filter          = header.filter_type;

    let mut filtered_planes: Vec<Vec<u8>> = planes.into_iter().map(|plane| {
        let mut out      = vec![0u8; plane_bytes];
        let mut prev_row = vec![0u8; plane_row_bytes];

        for row in 0..h {
            let row_off = row * plane_row_bytes;
            let cur_row = &plane[row_off..row_off + plane_row_bytes];

            // For Adaptive: select the best filter for this specific row.
            // The chosen filter gets stored in the compressed data implicitly
            // because Adaptive resolves to Paeth in v1. In v2, we'd store
            // a per-row filter byte prefix here.
            let effective_filter = if filter == FilterType::Adaptive {
                select_best_filter(cur_row, &prev_row, stride)
            } else {
                filter.effective()
            };

            apply_filter(
                effective_filter,
                cur_row,
                &prev_row,
                &mut out[row_off..row_off + plane_row_bytes],
                stride,
            );

            prev_row.copy_from_slice(cur_row);
        }
        out
    }).collect();

    // ── Step 3: byte-plane split (16-bit only) ────────────────────────────────
    // Split each 16-bit plane into a high-byte sub-plane and low-byte sub-plane.
    // Concatenated as [hi_bytes | lo_bytes] before MBFA.
    // Adjacent high bytes (MSB) of floating-point-like gradients are nearly
    // identical → MBFA LZ gets very long matches on the hi plane.
    if header.byte_plane_split() {
        for plane in &mut filtered_planes {
            let n  = plane.len() / 2; // number of u16 samples
            let mut hi = Vec::with_capacity(n);
            let mut lo = Vec::with_capacity(n);
            for chunk in plane.chunks_exact(2) {
                lo.push(chunk[0]); // little-endian: low byte first
                hi.push(chunk[1]); // high byte
            }
            plane.clear();
            plane.extend_from_slice(&hi);
            plane.extend_from_slice(&lo);
        }
    }

    // ── Step 4: MBFA compress each plane ─────────────────────────────────────
    let compressed_planes: Vec<io::Result<Vec<u8>>> = filtered_planes
        .iter()
        .map(|plane| mbfa::compress(plane, 8))
        .collect();

    // Check for errors before allocating output
    for result in &compressed_planes {
        if let Err(e) = result {
            return Err(io::Error::new(e.kind(), format!("MBFA compress: {}", e)));
        }
    }

    // ── Step 5: serialize ─────────────────────────────────────────────────────
    let total_payload: usize = compressed_planes
        .iter()
        .map(|r| 4 + r.as_ref().unwrap().len())
        .sum();

    let mut out = Vec::with_capacity(HEADER_SIZE + total_payload);
    out.extend_from_slice(&header.serialize());

    for result in compressed_planes {
        let compressed = result.unwrap();
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&compressed);
    }

    Ok(out)
      }

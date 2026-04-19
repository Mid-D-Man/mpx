// src/encode.rs

use std::io;
use crate::header::{
    MpxHeader, FilterType, FLAG_BYTE_PLANE_SPLIT,
    FLAG_INTER_CHANNEL_DELTA, HEADER_SIZE,
};
use crate::filter::{apply_filter, select_best_filter};

pub fn encode(header: &MpxHeader, pixels: &[u8]) -> io::Result<Vec<u8>> {
    let w         = header.width  as usize;
    let h         = header.height as usize;
    let channels  = header.channel_count();
    let bps       = header.bytes_per_sample();
    let row_bytes = w * channels * bps;

    if pixels.len() != h * row_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("pixel buffer: expected {} bytes got {}", h * row_bytes, pixels.len()),
        ));
    }

    // ── 1. Channel deinterleave ───────────────────────────────────────────────
    let plane_bytes = header.plane_bytes();
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

    // ── 2. Inter-channel delta (MBFA co-design) ───────────────────────────────
    // G = G-R, B = B-G (byte-wise wrapping).
    // For natural photos: G-R ≈ 0, B-G ≈ 0.
    // Combined with Paeth filter this produces near-all-zero residual planes.
    // MBFA fold-1 generates BACKREF(offset=row_width, length=long) tokens for
    // entire rows. Fold-2 pair encoding then compresses the ultra-regular token
    // stream further — this is the core MBFA flourishing mechanism.
    // Alpha stays independent (not correlated with B in general).
    if header.inter_channel_delta() {
        // c=1: subtract plane[0]; c=2: subtract plane[1] (before modification)
        for c in 1..channels.min(3) {
            let (left, right) = planes.split_at_mut(c);
            let prev = &left[c - 1];
            let curr = &mut right[0];
            for i in 0..plane_bytes {
                curr[i] = curr[i].wrapping_sub(prev[i]);
            }
        }
    }

    // ── 3. Spatial prediction filter ─────────────────────────────────────────
    let plane_row_bytes = header.plane_row_bytes();
    let stride          = bps;
    let filter          = header.filter_type;

    let mut filtered_planes: Vec<Vec<u8>> = planes.into_iter().map(|plane| {
        let mut out      = vec![0u8; plane_bytes];
        let mut prev_row = vec![0u8; plane_row_bytes];

        for row in 0..h {
            let row_off  = row * plane_row_bytes;
            let cur_row  = &plane[row_off..row_off + plane_row_bytes];

            let effective = if filter == FilterType::Adaptive {
                select_best_filter(cur_row, &prev_row, stride)
            } else {
                filter.effective()
            };

            apply_filter(
                effective,
                cur_row,
                &prev_row,
                &mut out[row_off..row_off + plane_row_bytes],
                stride,
            );

            prev_row.copy_from_slice(cur_row);
        }
        out
    }).collect();

    // ── 4. Byte-plane split (16-bit only) ─────────────────────────────────────
    if header.byte_plane_split() {
        for plane in &mut filtered_planes {
            let n = plane.len() / 2;
            let mut hi = Vec::with_capacity(n);
            let mut lo = Vec::with_capacity(n);
            for chunk in plane.chunks_exact(2) {
                lo.push(chunk[0]);
                hi.push(chunk[1]);
            }
            plane.clear();
            plane.extend_from_slice(&hi);
            plane.extend_from_slice(&lo);
        }
    }

    // ── 5. MBFA compress each plane ──────────────────────────────────────────
    let compressed_planes: Vec<io::Result<Vec<u8>>> = filtered_planes
        .iter()
        .map(|plane| mbfa::compress(plane, 8))
        .collect();

    for r in &compressed_planes {
        if let Err(e) = r {
            return Err(io::Error::new(e.kind(), format!("MBFA: {}", e)));
        }
    }

    // ── 6. Serialize ──────────────────────────────────────────────────────────
    let payload_total: usize = compressed_planes
        .iter()
        .map(|r| 4 + r.as_ref().unwrap().len())
        .sum();

    let mut out = Vec::with_capacity(HEADER_SIZE + payload_total);
    out.extend_from_slice(&header.serialize());

    for r in compressed_planes {
        let compressed = r.unwrap();
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&compressed);
    }

    Ok(out)
}

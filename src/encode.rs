// src/encode.rs

use std::io;
use crate::header::{
    MpxHeader, FilterType,
    FLAG_BYTE_PLANE_SPLIT, FLAG_INTER_CHANNEL_DELTA, FLAG_YCOCG_R,
    HEADER_SIZE,
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

    // ── 2a. YCoCg-R lossless color transform (RGB/RGBA 8-bit) ─────────────────
    // Much better decorrelation than simple ICD for natural photos.
    // Forward: Co = R-B, t = B+(Co>>1), Cg = G-t, Y = t+(Cg>>1)
    // Planes become [Y, Co, Cg, A] — near-zero Co/Cg for natural images.
    if header.use_ycocg() && bps == 1 {
        let n = plane_bytes;
        // planes[0]=R, planes[1]=G, planes[2]=B (planes[3]=A unchanged)
        let r_plane = planes[0].clone();
        let g_plane = planes[1].clone();
        let b_plane = planes[2].clone();
        for i in 0..n {
            let r = r_plane[i] as i16;
            let g = g_plane[i] as i16;
            let b = b_plane[i] as i16;
            let co = r - b;
            let t  = b + (co >> 1);
            let cg = g - t;
            let y  = t + (cg >> 1);
            // Y in [0,255], Co/Cg in [-128,127] — store as wrapping u8
            planes[0][i] = y  as u8;          // Y
            planes[1][i] = co as u8;          // Co (wrapping)
            planes[2][i] = cg as u8;          // Cg (wrapping)
            // planes[3] = A, unchanged
        }
    }

    // ── 2b. Simple inter-channel delta (GrayA only) ───────────────────────────
    if header.inter_channel_delta() {
        let originals: Vec<Vec<u8>> = (0..channels.min(3))
            .map(|c| planes[c].clone())
            .collect();
        for c in 1..channels.min(3) {
            let prev = &originals[c - 1];
            let curr = &mut planes[c];
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

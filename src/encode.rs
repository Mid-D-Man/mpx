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
    // All channels initially deinterleaved as u8 planes (or u16 for 16-bit).
    // YCoCg-R will expand Co/Cg to i16 in step 2a.
    let plane_bytes = header.plane_bytes(); // w * h * bps
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

    // ── 2a. YCoCg-R lossless color transform ──────────────────────────────────
    //
    // Converts [R(u8), G(u8), B(u8)] planes into [Y(u8), Co(i16 LE), Cg(i16 LE)].
    // Co and Cg range [-255, 255] — stored as i16 little-endian (2 bytes each).
    //
    // Forward transform (exact, invertible):
    //   Co = R - B                           range: [-255, 255]
    //   t  = B + (Co >> 1)                   range: [0, 255]  — arithmetic shift
    //   Cg = G - t                           range: [-255, 255]
    //   Y  = t + (Cg >> 1)                   range: [0, 255]
    //
    // For natural photos G ≈ (R+B)/2, giving Co ≈ 0 and Cg ≈ 0. The Paeth
    // filter then produces near-zero residuals and MBFA finds very long backrefs.
    // The Co/Cg hi-byte planes alternate only between 0x00 and 0xFF and compress
    // to near zero bytes via MBFA's repetition detection.
    if header.use_ycocg() {
        let n        = w * h; // pixels
        let r_plane  = planes[0].clone();
        let g_plane  = planes[1].clone();
        let b_plane  = planes[2].clone();

        let mut y_plane  = vec![0u8; n];     // Y:  u8  [0, 255]
        let mut co_plane = vec![0u8; n * 2]; // Co: i16 LE [-255, 255]
        let mut cg_plane = vec![0u8; n * 2]; // Cg: i16 LE [-255, 255]

        for i in 0..n {
            let r = r_plane[i] as i16;
            let g = g_plane[i] as i16;
            let b = b_plane[i] as i16;

            let co: i16 = r - b;                  // arithmetic, i16 is fine
            let t:  i16 = b + (co >> 1);          // >> on i16 = arithmetic right shift
            let cg: i16 = g - t;
            let y:  i16 = t + (cg >> 1);

            // Y is mathematically guaranteed in [0,255] — safe cast.
            y_plane[i] = y as u8;

            // Co/Cg stored as i16 little-endian.
            let co_b = co.to_le_bytes();
            co_plane[i * 2]     = co_b[0];
            co_plane[i * 2 + 1] = co_b[1];

            let cg_b = cg.to_le_bytes();
            cg_plane[i * 2]     = cg_b[0];
            cg_plane[i * 2 + 1] = cg_b[1];
        }

        planes[0] = y_plane;
        planes[1] = co_plane;  // now w*h*2 bytes
        planes[2] = cg_plane;  // now w*h*2 bytes
        // planes[3] (A if RGBA) unchanged — remains w*h bytes
    }

    // ── 2b. Simple inter-channel delta (fallback for incompressible RGB/RGBA) ──
    // G = G-R, B = B-G using wrapping u8 arithmetic.
    // Applied when FLAG_INTER_CHANNEL_DELTA is set (not FLAG_YCOCG_R).
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

    // ── 3. Spatial prediction filter (per channel) ────────────────────────────
    // Each channel may now have a different bytes-per-sample:
    //   YCoCg-R:  Y→1, Co→2, Cg→2, A→1
    //   16-bit:   all channels → 2
    //   8-bit:    all channels → 1
    let filter = header.filter_type;

    let mut filtered_planes: Vec<Vec<u8>> = planes
        .into_iter()
        .enumerate()
        .map(|(ch, plane)| {
            let ch_row_bytes = header.channel_plane_row_bytes(ch);
            let stride       = header.channel_bps(ch);
            let ch_bytes     = header.channel_plane_bytes(ch);

            let mut out      = vec![0u8; ch_bytes];
            let mut prev_row = vec![0u8; ch_row_bytes];

            for row in 0..h {
                let row_off = row * ch_row_bytes;
                let cur_row = &plane[row_off..row_off + ch_row_bytes];

                let effective = if filter == FilterType::Adaptive {
                    select_best_filter(cur_row, &prev_row, stride)
                } else {
                    filter.effective()
                };

                apply_filter(
                    effective,
                    cur_row,
                    &prev_row,
                    &mut out[row_off..row_off + ch_row_bytes],
                    stride,
                );

                prev_row.copy_from_slice(cur_row);
            }
            out
        })
        .collect();

    // ── 4. Byte-plane split (per channel) ─────────────────────────────────────
    // Applied to:
    //   • All channels for 16-bit images (FLAG_BYTE_PLANE_SPLIT)
    //   • Co and Cg channels for YCoCg-R 8-bit images
    // Separates lo-bytes and hi-bytes: [lo_0..lo_n, hi_0..hi_n].
    // For YCoCg Co/Cg: hi-bytes are 0x00 (Co>0) or 0xFF (Co<0) for most photos,
    // forming a near-uniform plane that MBFA compresses to near zero bytes.
    for (ch, plane) in filtered_planes.iter_mut().enumerate() {
        if header.channel_needs_byte_split(ch) {
            let n = plane.len() / 2;
            let mut hi = Vec::with_capacity(n);
            let mut lo = Vec::with_capacity(n);
            for chunk in plane.chunks_exact(2) {
                lo.push(chunk[0]); // LE: low byte
                hi.push(chunk[1]); // LE: high byte
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

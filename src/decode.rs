// src/decode.rs

use std::io;
use crate::header::{MpxHeader, HEADER_SIZE};
use crate::filter::undo_filter;

pub fn decode(data: &[u8]) -> io::Result<(MpxHeader, Vec<u8>)> {
    let header   = MpxHeader::parse(data)?;
    let w        = header.width  as usize;
    let h        = header.height as usize;
    let channels = header.channel_count();
    let bps      = header.bytes_per_sample();

    // ── 1. Decompress each channel block ──────────────────────────────────────
    // Each channel is preceded by a u32 LE compressed-length field.
    // The expected decompressed size varies per channel (YCoCg-R: Co/Cg are 2x).
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

        // Byte-plane split is size-preserving, so expected size is always
        // channel_plane_bytes(ch) regardless of whether the split was applied.
        let expected = header.channel_plane_bytes(ch);
        if decompressed.len() != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("ch{}: got {} bytes expected {}", ch, decompressed.len(), expected),
            ));
        }
        planes.push(decompressed);
    }

    // ── 2. Undo byte-plane split (per channel) ────────────────────────────────
    // Inverts the [hi_0..hi_n, lo_0..lo_n] → interleaved [lo_0,hi_0, lo_1,hi_1,...]
    // reconstruction applied per-channel based on channel_needs_byte_split.
    for (ch, plane) in planes.iter_mut().enumerate() {
        if header.channel_needs_byte_split(ch) {
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

    // ── 3. Undo spatial prediction filter (per channel) ───────────────────────
    let filter = header.filter_type;
    for (ch, plane) in planes.iter_mut().enumerate() {
        let ch_row_bytes = header.channel_plane_row_bytes(ch);
        let stride       = header.channel_bps(ch);
        let mut prev_row = vec![0u8; ch_row_bytes];

        for row in 0..h {
            let row_off = row * ch_row_bytes;
            undo_filter(
                filter,
                &mut plane[row_off..row_off + ch_row_bytes],
                &prev_row,
                stride,
            );
            prev_row.copy_from_slice(&plane[row_off..row_off + ch_row_bytes]);
        }
    }

    // ── 4a. Undo YCoCg-R transform ────────────────────────────────────────────
    // At this point: planes[0]=Y(u8 n bytes), planes[1]=Co(i16 LE 2n bytes),
    //                planes[2]=Cg(i16 LE 2n bytes), [planes[3]=A(u8 n bytes)]
    //
    // Inverse transform (exact):
    //   t = Y  - (Cg >> 1)    arithmetic right shift on signed i16
    //   G = Cg + t
    //   B = t  - (Co >> 1)
    //   R = B  + Co
    //
    // All results are mathematically in [0,255] for valid encoded data.
    // clamp() is defensive against any potential bit errors only.
    if header.use_ycocg() {
        let n        = w * h;
        let y_plane  = planes[0].clone();
        let co_plane = planes[1].clone();
        let cg_plane = planes[2].clone();

        let mut r_plane = vec![0u8; n];
        let mut g_plane = vec![0u8; n];
        let mut b_plane = vec![0u8; n];

        for i in 0..n {
            let y:  i16 = y_plane[i] as i16;
            let co: i16 = i16::from_le_bytes([co_plane[i * 2], co_plane[i * 2 + 1]]);
            let cg: i16 = i16::from_le_bytes([cg_plane[i * 2], cg_plane[i * 2 + 1]]);

            let t: i16 = y  - (cg >> 1); // arithmetic right shift on signed i16
            let g: i16 = cg + t;
            let b: i16 = t  - (co >> 1);
            let r: i16 = b  + co;

            // For valid encoded data these are always in [0,255].
            r_plane[i] = r.clamp(0, 255) as u8;
            g_plane[i] = g.clamp(0, 255) as u8;
            b_plane[i] = b.clamp(0, 255) as u8;
        }

        // Replace i16 planes with reconstructed u8 planes.
        // planes[0..2] go from mixed sizes back to uniform u8 (n bytes each).
        planes[0] = r_plane;
        planes[1] = g_plane;
        planes[2] = b_plane;
        // planes[3] (A) unchanged
    }

    // ── 4b. Undo simple inter-channel delta ───────────────────────────────────
    // G = (G-R)+R, B = (B-G)+G reconstructed in forward order.
    if header.inter_channel_delta() {
        let plane_bytes = header.plane_bytes();
        for c in 1..channels.min(3) {
            let (left, right) = planes.split_at_mut(c);
            let prev = left[c - 1].clone();
            let curr = &mut right[0];
            for i in 0..plane_bytes {
                curr[i] = curr[i].wrapping_add(prev[i]);
            }
        }
    }

    // ── 5. Reassemble interleaved pixels ──────────────────────────────────────
    // After steps 4a/4b all planes are u8 planes of w*h*bps bytes each.
    // (YCoCg-R restores Co/Cg from 2*n bytes back to n bytes in step 4a.)
    // bps = header.bytes_per_sample() = 1 for 8-bit, 2 for 16-bit.
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

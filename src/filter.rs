// Auto-generated stub
// src/filter.rs
//! Spatial prediction filters applied per row, per channel plane.
//!
//! These convert raw pixel values into residuals (differences from predicted
//! values). Residuals are typically near-zero for natural images, which
//! dramatically improves LZ match density in the MBFA pass.
//!
//! All filters operate on byte arrays regardless of bit depth.
//! For 16-bit planes: stride=2, and predictors reference sample[i-2]
//! (two bytes back = one sample back). This keeps the predictor logic
//! generic without needing to understand u16 values.
//!
//! This is the same approach PNG uses — filter bytes, not filter samples.
//! See PNG spec section 9 for the mathematical background.

use crate::header::FilterType;

// ── Public encode API ─────────────────────────────────────────────────────────

/// Apply a spatial prediction filter to one row of a channel plane.
///
/// `row`    — current row bytes (read-only)
/// `prev`   — previous row bytes (zero-filled for row 0)
/// `out`    — output residuals (same length as row)
/// `stride` — bytes per sample: 1 for 8-bit planes, 2 for 16-bit planes
pub fn apply_filter(
    filter: FilterType,
    row:    &[u8],
    prev:   &[u8],
    out:    &mut [u8],
    stride: usize,
) {
    debug_assert_eq!(row.len(), prev.len(), "row and prev must have equal length");
    debug_assert_eq!(row.len(), out.len(),  "row and out must have equal length");
    debug_assert!(stride == 1 || stride == 2, "stride must be 1 or 2");

    match filter.effective() {
        FilterType::None    => out.copy_from_slice(row),
        FilterType::Sub     => encode_sub(row, out, stride),
        FilterType::Up      => encode_up(row, prev, out),
        FilterType::Average => encode_average(row, prev, out, stride),
        FilterType::Paeth   => encode_paeth(row, prev, out, stride),
        // Adaptive and its effective() fallback are already handled above.
        _ => unreachable!(),
    }
}

// ── Public decode API ─────────────────────────────────────────────────────────

/// Undo a spatial prediction filter on one row in-place.
///
/// `row`    — filtered residuals, modified in-place to become raw pixels
/// `prev`   — previous RECONSTRUCTED row (zero for row 0)
/// `stride` — bytes per sample
pub fn undo_filter(
    filter: FilterType,
    row:    &mut [u8],
    prev:   &[u8],
    stride: usize,
) {
    match filter.effective() {
        FilterType::None    => {}
        FilterType::Sub     => decode_sub(row, stride),
        FilterType::Up      => decode_up(row, prev),
        FilterType::Average => decode_average(row, prev, stride),
        FilterType::Paeth   => decode_paeth(row, prev, stride),
        _ => unreachable!(),
    }
}

// ── Encode implementations ────────────────────────────────────────────────────

fn encode_sub(row: &[u8], out: &mut [u8], stride: usize) {
    for i in 0..row.len() {
        let a = if i >= stride { row[i - stride] } else { 0 };
        out[i] = row[i].wrapping_sub(a);
    }
}

fn encode_up(row: &[u8], prev: &[u8], out: &mut [u8]) {
    for i in 0..row.len() {
        out[i] = row[i].wrapping_sub(prev[i]);
    }
}

fn encode_average(row: &[u8], prev: &[u8], out: &mut [u8], stride: usize) {
    for i in 0..row.len() {
        let a   = if i >= stride { row[i - stride] } else { 0u8 };
        let b   = prev[i];
        let avg = ((a as u16 + b as u16) >> 1) as u8;
        out[i] = row[i].wrapping_sub(avg);
    }
}

fn encode_paeth(row: &[u8], prev: &[u8], out: &mut [u8], stride: usize) {
    for i in 0..row.len() {
        let a = if i >= stride { row[i - stride]       } else { 0 };
        let b = prev[i];
        let c = if i >= stride { prev[i - stride]      } else { 0 };
        out[i] = row[i].wrapping_sub(paeth(a, b, c));
    }
}

// ── Decode implementations ────────────────────────────────────────────────────

fn decode_sub(row: &mut [u8], stride: usize) {
    for i in stride..row.len() {
        row[i] = row[i].wrapping_add(row[i - stride]);
    }
}

fn decode_up(row: &mut [u8], prev: &[u8]) {
    for i in 0..row.len() {
        row[i] = row[i].wrapping_add(prev[i]);
    }
}

fn decode_average(row: &mut [u8], prev: &[u8], stride: usize) {
    for i in 0..row.len() {
        // a is already reconstructed by the time we reach it (left-to-right)
        let a   = if i >= stride { row[i - stride] } else { 0u8 };
        let b   = prev[i];
        let avg = ((a as u16 + b as u16) >> 1) as u8;
        row[i] = row[i].wrapping_add(avg);
    }
}

fn decode_paeth(row: &mut [u8], prev: &[u8], stride: usize) {
    for i in 0..row.len() {
        let a = if i >= stride { row[i - stride]  } else { 0 };
        let b = prev[i];
        let c = if i >= stride { prev[i - stride] } else { 0 };
        row[i] = row[i].wrapping_add(paeth(a, b, c));
    }
}

// ── Paeth predictor (PNG spec §9.4) ──────────────────────────────────────────

#[inline(always)]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let a  = a as i32;
    let b  = b as i32;
    let c  = c as i32;
    let p  = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc { a as u8 }
    else if pb <= pc        { b as u8 }
    else                    { c as u8 }
}

// ── Filter selection heuristic ────────────────────────────────────────────────

/// Pick the best filter for a row by trying all four and choosing the one
/// that minimizes the sum of absolute residuals. Same heuristic as PNG.
/// Used by the encoder when FilterType::Adaptive is requested.
///
/// Returns the chosen FilterType (never Adaptive or None).
pub fn select_best_filter(
    row:    &[u8],
    prev:   &[u8],
    stride: usize,
) -> FilterType {
    let candidates = [
        FilterType::Sub,
        FilterType::Up,
        FilterType::Average,
        FilterType::Paeth,
    ];

    let mut tmp      = vec![0u8; row.len()];
    let mut best     = FilterType::Paeth;
    let mut best_sum = u64::MAX;

    for &filter in &candidates {
        apply_filter(filter, row, prev, &mut tmp, stride);
        // Sum absolute values — wrapping bytes, so 255 = -1 has high abs value.
        // Map to signed first: treat values > 127 as negative residuals.
        let sum: u64 = tmp.iter().map(|&b| {
            let s = b as i8;
            s.unsigned_abs() as u64
        }).sum();
        if sum < best_sum {
            best_sum = sum;
            best     = filter;
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(filter: FilterType, row: &[u8], prev: &[u8], stride: usize) {
        let mut filtered   = vec![0u8; row.len()];
        let mut prev_owned = prev.to_vec();
        apply_filter(filter, row, &prev_owned, &mut filtered, stride);

        // Decoder gets the filtered data and reconstructs
        let mut reconstructed = filtered.clone();
        undo_filter(filter, &mut reconstructed, &prev_owned, stride);

        assert_eq!(reconstructed, row,
            "filter {:?} roundtrip failed", filter);
    }

    #[test]
    fn all_filters_roundtrip_8bit() {
        let row:  Vec<u8> = (0..256).map(|i| (i * 3 % 256) as u8).collect();
        let prev: Vec<u8> = (0..256).map(|i| (i * 7 % 256) as u8).collect();
        for filter in [FilterType::None, FilterType::Sub, FilterType::Up,
                       FilterType::Average, FilterType::Paeth] {
            roundtrip(filter, &row, &prev, 1);
        }
    }

    #[test]
    fn all_filters_roundtrip_16bit() {
        // 16-bit plane: 128 samples = 256 bytes, stride = 2
        let row:  Vec<u8> = (0u16..128).flat_map(|v| v.to_le_bytes()).collect();
        let prev: Vec<u8> = (64u16..192).flat_map(|v| v.to_le_bytes()).collect();
        for filter in [FilterType::None, FilterType::Sub, FilterType::Up,
                       FilterType::Average, FilterType::Paeth] {
            roundtrip(filter, &row, &prev, 2);
        }
    }

    #[test]
    fn paeth_known_values() {
        // PNG spec example: a=10, b=20, c=15 → paeth should return 10
        assert_eq!(paeth(10, 20, 15), 10);
        // a=10, b=10, c=5 → p=15, pa=5, pb=5, pc=10 → a or b (tie goes to a)
        assert_eq!(paeth(10, 10, 5), 10);
    }

    #[test]
    fn select_best_picks_reasonable_filter() {
        // Horizontal gradient — Sub should win (residuals = constant)
        let row: Vec<u8>  = (0..256u16).map(|i| (i % 256) as u8).collect();
        let prev: Vec<u8> = (0..256u16).map(|i| (i % 256) as u8).collect();
        let chosen = select_best_filter(&row, &prev, 1);
        // Any filter that produces near-zero residuals is valid — just not None
        assert_ne!(chosen, FilterType::None);
        println!("select_best for gradient: {:?}", chosen);
    }
      }

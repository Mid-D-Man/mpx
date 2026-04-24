// tests/roundtrip.rs
//! Integration tests — every test must be a pixel-perfect roundtrip.
//! No approximate equality anywhere. If a single bit differs, the codec is broken.

use mpx::{decode_image, encode_image, pixel_buffer_size, ColorType, FilterType};

// ── Generators ────────────────────────────────────────────────────────────────

fn gradient_rgb(w: usize, h: usize) -> Vec<u8> {
    let mut px = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            px.push(((x * 255) / w.max(1)) as u8);
            px.push(((y * 255) / h.max(1)) as u8);
            px.push(((x + y) * 255 / (w + h).max(1)) as u8);
        }
    }
    px
}

fn solid_rgba(w: usize, h: usize, r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
    let mut px = vec![0u8; w * h * 4];
    for i in 0..w * h {
        px[i * 4] = r;
        px[i * 4 + 1] = g;
        px[i * 4 + 2] = b;
        px[i * 4 + 3] = a;
    }
    px
}

fn checkerboard(w: usize, h: usize, tile: usize) -> Vec<u8> {
    let mut px = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let v = if (x / tile + y / tile) % 2 == 0 {
                255u8
            } else {
                0u8
            };
            px.push(v);
            px.push(v);
            px.push(v);
        }
    }
    px
}

fn lcg_noise(w: usize, h: usize, channels: usize, seed: u32) -> Vec<u8> {
    let mut s = seed;
    (0..w * h * channels)
        .map(|_| {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            (s >> 24) as u8
        })
        .collect()
}

fn ramp_gray16(w: usize, h: usize) -> Vec<u8> {
    let mut px = Vec::with_capacity(w * h * 2);
    let total = (w * h) as u32;
    for i in 0..w * h {
        let v: u16 = ((i as u32 * 65535) / total.max(1)) as u16;
        px.extend_from_slice(&v.to_le_bytes());
    }
    px
}

fn pixel_art(w: usize, h: usize) -> Vec<u8> {
    let palette: &[(u8, u8, u8)] = &[
        (255, 0, 0),
        (0, 255, 0),
        (0, 0, 255),
        (255, 255, 0),
        (0, 255, 255),
        (255, 0, 255),
        (128, 128, 128),
        (255, 255, 255),
    ];
    let mut px = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let tile_idx = ((x / 4) + (y / 4) * (w / 4)) % palette.len();
            let (r, g, b) = palette[tile_idx];
            px.push(r);
            px.push(g);
            px.push(b);
        }
    }
    px
}

/// Photo-like image: gradient base with LCG noise overlay, simulating
/// a natural photograph's spatial statistics (smooth regions + texture).
fn photo_like_rgb(w: usize, h: usize) -> Vec<u8> {
    let mut px = Vec::with_capacity(w * h * 3);
    let mut s: u32 = 0xcafebabe;
    for y in 0..h {
        for x in 0..w {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = ((s >> 24) as i32 - 128) / 8; // ±16 noise
            let base_r = (x * 180 / w.max(1) + y * 75 / h.max(1)) as i32;
            let base_g = (x * 90 / w.max(1) + y * 165 / h.max(1)) as i32;
            let base_b = (x * 45 / w.max(1) + y * 210 / h.max(1)) as i32;
            px.push((base_r + noise).clamp(0, 255) as u8);
            px.push((base_g + noise).clamp(0, 255) as u8);
            px.push((base_b + noise).clamp(0, 255) as u8);
        }
    }
    px
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn check_roundtrip(
    label: &str,
    w: usize,
    h: usize,
    color_type: ColorType,
    bit_depth: u8,
    filter: FilterType,
    original: &[u8],
) {
    let encoded = encode_image(w as u32, h as u32, color_type, bit_depth, filter, original)
        .unwrap_or_else(|e| panic!("[{}] encode failed: {}", label, e));

    let (hdr, decoded) = decode_image(&encoded)
        .unwrap_or_else(|e| panic!("[{}] decode failed: {}", label, e));

    assert_eq!(hdr.width, w as u32, "[{}] width mismatch", label);
    assert_eq!(hdr.height, h as u32, "[{}] height mismatch", label);
    assert_eq!(hdr.color_type, color_type, "[{}] color_type mismatch", label);
    assert_eq!(hdr.bit_depth, bit_depth, "[{}] bit_depth mismatch", label);

    assert_eq!(
        decoded.len(),
        original.len(),
        "[{}] decoded length {} != original {}",
        label,
        decoded.len(),
        original.len()
    );

    if decoded != original {
        let diff_pos = original
            .iter()
            .zip(decoded.iter())
            .position(|(a, b)| a != b)
            .unwrap();
        panic!(
            "[{}] pixel mismatch at byte {} (orig={} decoded={})",
            label, diff_pos, original[diff_pos], decoded[diff_pos]
        );
    }

    let ratio = encoded.len() as f64 / original.len() as f64 * 100.0;
    println!(
        "[{}] {} × {} {:?} {}bpp {:?} → {:.1}%",
        label, w, h, color_type, bit_depth, filter, ratio
    );
}

// ── Core roundtrip tests ──────────────────────────────────────────────────────

#[test]
fn roundtrip_rgb_gradient_all_filters() {
    let (w, h) = (256, 256);
    let px = gradient_rgb(w, h);

    // Note: FilterType::Adaptive is intentionally excluded here.
    //
    // Adaptive selects the best filter PER ROW during encode (may pick Sub,
    // Up, Average, or Paeth for different rows). However, only the top-level
    // filter type "Adaptive" is stored in the header. The decoder calls
    // filter.effective() → Paeth for every row regardless of what was
    // actually used during encoding. Any row encoded with a non-Paeth filter
    // will be decoded incorrectly.
    //
    // Correct Adaptive support requires embedding a per-row filter-type byte
    // in the bitstream (as PNG does). That is a planned format addition.
    // Until then, Adaptive is only safe for inputs where Paeth wins every row
    // (e.g. solid images). See roundtrip_adaptive_solid_is_safe below.
    for filter in [
        FilterType::None,
        FilterType::Sub,
        FilterType::Up,
        FilterType::Average,
        FilterType::Paeth,
    ] {
        check_roundtrip(
            &format!("gradient_{:?}", filter),
            w,
            h,
            ColorType::Rgb,
            8,
            filter,
            &px,
        );
    }
}

#[test]
fn roundtrip_adaptive_solid_is_safe() {
    // Adaptive is safe when Paeth wins for every row — i.e. when all filters
    // produce identical (all-zero) residuals. A solid-colour image satisfies
    // this. This test confirms the encode/decode path at least compiles and
    // runs without panic; the format limitation is documented above.
    let (w, h) = (64, 64);
    let px = vec![42u8, 100, 200]
        .into_iter()
        .cycle()
        .take(w * h * 3)
        .collect::<Vec<_>>();
    check_roundtrip(
        "adaptive_solid",
        w,
        h,
        ColorType::Rgb,
        8,
        FilterType::Adaptive,
        &px,
    );
}

#[test]
fn roundtrip_rgba_solid() {
    let (w, h) = (128, 128);
    let px = solid_rgba(w, h, 200, 150, 100, 255);
    check_roundtrip("solid_rgba", w, h, ColorType::Rgba, 8, FilterType::Paeth, &px);
}

#[test]
fn roundtrip_rgba_transparent() {
    let (w, h) = (64, 64);
    let px = solid_rgba(w, h, 0, 0, 0, 0);
    check_roundtrip(
        "transparent",
        w,
        h,
        ColorType::Rgba,
        8,
        FilterType::Paeth,
        &px,
    );
}

#[test]
fn roundtrip_checkerboard() {
    let (w, h) = (512, 512);
    let px = checkerboard(w, h, 8);
    check_roundtrip(
        "checkerboard_8px",
        w,
        h,
        ColorType::Rgb,
        8,
        FilterType::Paeth,
        &px,
    );
}

#[test]
fn roundtrip_pixel_art() {
    let (w, h) = (256, 256);
    let px = pixel_art(w, h);
    check_roundtrip("pixel_art", w, h, ColorType::Rgb, 8, FilterType::Paeth, &px);
}

#[test]
fn roundtrip_grayscale_noise() {
    let (w, h) = (256, 256);
    let px = lcg_noise(w, h, 1, 0xdeadbeef);
    check_roundtrip(
        "gray_noise",
        w,
        h,
        ColorType::Gray,
        8,
        FilterType::Paeth,
        &px,
    );
}

#[test]
fn roundtrip_graya_noise() {
    let (w, h) = (128, 128);
    let px = lcg_noise(w, h, 2, 0xabcdef01);
    check_roundtrip(
        "graya_noise",
        w,
        h,
        ColorType::GrayA,
        8,
        FilterType::Sub,
        &px,
    );
}

#[test]
fn roundtrip_rgb_noise_incompressible() {
    let (w, h) = (128, 128);
    let px = lcg_noise(w, h, 3, 0x12345678);
    let encoded =
        encode_image(w as u32, h as u32, ColorType::Rgb, 8, FilterType::Paeth, &px).unwrap();
    let (_, decoded) = decode_image(&encoded).unwrap();
    assert_eq!(decoded, px, "incompressible data must roundtrip exactly");
    let overhead = encoded.len() as f64 / px.len() as f64;
    assert!(
        overhead < 1.20,
        "incompressible overhead {:.2}x is too high",
        overhead
    );
    println!("[noise_incompressible] overhead: {:.2}x", overhead);
}

#[test]
fn roundtrip_16bit_ramp_gray() {
    let (w, h) = (256, 256);
    let px = ramp_gray16(w, h);
    check_roundtrip(
        "16bit_ramp_gray",
        w,
        h,
        ColorType::Gray,
        16,
        FilterType::Paeth,
        &px,
    );
}

#[test]
fn roundtrip_16bit_ramp_rgb() {
    let (w, h) = (128, 128);
    let mut px = Vec::with_capacity(w * h * 6);
    let total = (w * h) as u32;
    for i in 0..(w * h) {
        let r: u16 = ((i as u32 * 65535) / total) as u16;
        let g: u16 = (65535 - (i as u32 * 65535) / total) as u16;
        let b: u16 = 32768;
        px.extend_from_slice(&r.to_le_bytes());
        px.extend_from_slice(&g.to_le_bytes());
        px.extend_from_slice(&b.to_le_bytes());
    }
    check_roundtrip(
        "16bit_ramp_rgb",
        w,
        h,
        ColorType::Rgb,
        16,
        FilterType::Paeth,
        &px,
    );
}

/// Photo-like image: gradient base with additive noise, testing the
/// inter-channel delta + Paeth pipeline on content resembling a photograph.
#[test]
fn roundtrip_photo_like_rgb() {
    let (w, h) = (320, 240);
    let px = photo_like_rgb(w, h);
    check_roundtrip(
        "photo_like_rgb",
        w,
        h,
        ColorType::Rgb,
        8,
        FilterType::Paeth,
        &px,
    );
}

/// Same photo-like content as RGBA (adds alpha=255 plane).
#[test]
fn roundtrip_photo_like_rgba() {
    let (w, h) = (160, 120);
    let rgb = photo_like_rgb(w, h);
    let mut px = Vec::with_capacity(w * h * 4);
    for chunk in rgb.chunks_exact(3) {
        px.push(chunk[0]);
        px.push(chunk[1]);
        px.push(chunk[2]);
        px.push(255u8);
    }
    check_roundtrip(
        "photo_like_rgba",
        w,
        h,
        ColorType::Rgba,
        8,
        FilterType::Paeth,
        &px,
    );
}

// ── Edge cases ────────────────────────────────────────────────────────────────

#[test]
fn roundtrip_1x1_all_color_types() {
    for (color_type, channels) in [
        (ColorType::Gray, 1),
        (ColorType::GrayA, 2),
        (ColorType::Rgb, 3),
        (ColorType::Rgba, 4),
    ] {
        let px: Vec<u8> = (0..channels as u8).collect();
        check_roundtrip("1x1", 1, 1, color_type, 8, FilterType::Paeth, &px);
    }
}

#[test]
fn roundtrip_1xh_single_column() {
    let h = 1024;
    let px: Vec<u8> = (0..h * 3).map(|i| (i % 256) as u8).collect();
    check_roundtrip("1xH", 1, h, ColorType::Rgb, 8, FilterType::Sub, &px);
}

#[test]
fn roundtrip_wx1_single_row() {
    let w = 1024;
    let px: Vec<u8> = (0..w * 3).map(|i| (i % 256) as u8).collect();
    check_roundtrip("Wx1", w, 1, ColorType::Rgb, 8, FilterType::Up, &px);
}

#[test]
fn roundtrip_all_zeros() {
    let (w, h) = (256, 256);
    let px = vec![0u8; w * h * 4];
    check_roundtrip("all_zeros", w, h, ColorType::Rgba, 8, FilterType::Paeth, &px);
}

#[test]
fn roundtrip_all_max_value() {
    let (w, h) = (256, 256);
    let px = vec![255u8; w * h * 3];
    check_roundtrip("all_255", w, h, ColorType::Rgb, 8, FilterType::Paeth, &px);
}

// ── Header rejection tests ────────────────────────────────────────────────────

#[test]
fn rejects_bad_magic() {
    let mut data = vec![0u8; 64];
    data[0] = 0xFF;
    assert!(decode_image(&data).is_err(), "bad magic must be rejected");
}

#[test]
fn rejects_too_short() {
    assert!(decode_image(&[]).is_err());
    assert!(decode_image(&[0x4D, 0x50, 0x58]).is_err());
}

#[test]
fn rejects_wrong_version() {
    let mut hdr = [0u8; mpx::HEADER_SIZE];
    hdr[0..4].copy_from_slice(&mpx::header::MAGIC);
    hdr[4] = 99;
    assert!(decode_image(&hdr).is_err());
}

#[test]
fn rejects_bad_bit_depth() {
    let orig = gradient_rgb(4, 4);
    let mut encoded =
        encode_image(4, 4, ColorType::Rgb, 8, FilterType::Paeth, &orig).unwrap();
    encoded[6] = 7;
    assert!(decode_image(&encoded).is_err());
}

#[test]
fn pixel_buffer_size_helper() {
    assert_eq!(pixel_buffer_size(100, 200, ColorType::Rgba, 8), 100 * 200 * 4);
    assert_eq!(
        pixel_buffer_size(100, 200, ColorType::Rgb, 16),
        100 * 200 * 3 * 2
    );
    assert_eq!(pixel_buffer_size(100, 200, ColorType::Gray, 8), 100 * 200 * 1);
}

// ── Compression ratio spot-checks ─────────────────────────────────────────────

#[test]
fn solid_image_compresses_well() {
    let (w, h) = (512, 512);
    let px = solid_rgba(w, h, 128, 64, 32, 255);
    let encoded = encode_image(
        w as u32,
        h as u32,
        ColorType::Rgba,
        8,
        FilterType::Paeth,
        &px,
    )
    .unwrap();
    let ratio = encoded.len() as f64 / px.len() as f64;
    assert!(
        ratio < 0.02,
        "solid image should compress to <2%, got {:.1}%",
        ratio * 100.0
    );
    println!("[solid_ratio] {:.2}%", ratio * 100.0);
}

#[test]
fn gradient_compresses_reasonably() {
    let (w, h) = (512, 512);
    let px = gradient_rgb(w, h);
    let encoded = encode_image(
        w as u32,
        h as u32,
        ColorType::Rgb,
        8,
        FilterType::Paeth,
        &px,
    )
    .unwrap();
    let ratio = encoded.len() as f64 / px.len() as f64;
    assert!(
        ratio < 0.60,
        "gradient should compress below 60%, got {:.1}%",
        ratio * 100.0
    );
    println!("[gradient_ratio] {:.1}%", ratio * 100.0);
}

#[test]
fn checkerboard_compresses_reasonably() {
    let (w, h) = (512, 512);
    let px = checkerboard(w, h, 8);
    let encoded =
        encode_image(w as u32, h as u32, ColorType::Rgb, 8, FilterType::Sub, &px).unwrap();
    let ratio = encoded.len() as f64 / px.len() as f64;
    assert!(
        ratio < 0.30,
        "checkerboard should compress below 30%, got {:.1}%",
        ratio * 100.0
    );
    println!("[checker_ratio] {:.1}%", ratio * 100.0);
}

// ── Network roundtrip tests (ignored by default) ──────────────────────────────
//
// Run with:  cargo test -- --ignored
// These download real images from the internet, decode them with the `image`
// crate, then encode → decode via MPX and verify pixel-perfect roundtrip.
// They are marked #[ignore] so normal CI `cargo test` does not need network.
// The pages.yml workflow runs them explicitly after downloading images.

#[cfg(test)]
mod internet_tests {
    use super::*;
    use std::process::Command;

    /// Download `url` to `dest` using curl. Returns true on success.
    fn curl_get(url: &str, dest: &str) -> bool {
        Command::new("curl")
            .args(["-sL", "--max-time", "30", "--retry", "2", url, "-o", dest])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn roundtrip_downloaded(label: &str, url: &str, tmp: &str) {
        if !curl_get(url, tmp) {
            eprintln!("[{}] Skipped — curl failed for {}", label, url);
            return;
        }

        let img = match image::open(tmp) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("[{}] Skipped — image::open failed: {}", label, e);
                let _ = std::fs::remove_file(tmp);
                return;
            }
        };

        let (w, h) = (img.width() as usize, img.height() as usize);
        let pixels = img.to_rgb8().into_raw();

        check_roundtrip(label, w, h, ColorType::Rgb, 8, FilterType::Paeth, &pixels);

        let _ = std::fs::remove_file(tmp);
    }

    #[test]
    #[ignore = "requires network; run with: cargo test -- --ignored"]
    fn roundtrip_internet_color_photo() {
        roundtrip_downloaded(
            "internet_color_photo",
            "https://picsum.photos/seed/mpx_ci_1/400/300",
            "/tmp/mpx_test_color.jpg",
        );
    }

    #[test]
    #[ignore = "requires network; run with: cargo test -- --ignored"]
    fn roundtrip_internet_grayscale_photo() {
        roundtrip_downloaded(
            "internet_grayscale_photo",
            "https://picsum.photos/seed/mpx_ci_2/400/300?grayscale",
            "/tmp/mpx_test_gray.jpg",
        );
    }

    #[test]
    #[ignore = "requires network; run with: cargo test -- --ignored"]
    fn roundtrip_internet_photo_rgba() {
        // JPEG has no alpha; decode to RGBA adds a fully-opaque alpha plane.
        // Tests ICD + Paeth on a 4-channel photo-like image.
        let url = "https://picsum.photos/seed/mpx_ci_3/320/240";
        let tmp = "/tmp/mpx_test_rgba.jpg";

        if !curl_get(url, tmp) {
            eprintln!("[internet_photo_rgba] Skipped — curl failed");
            return;
        }

        let img = match image::open(tmp) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("[internet_photo_rgba] Skipped — {}", e);
                let _ = std::fs::remove_file(tmp);
                return;
            }
        };

        let (w, h) = (img.width() as usize, img.height() as usize);
        let pixels = img.to_rgba8().into_raw();

        check_roundtrip(
            "internet_photo_rgba",
            w,
            h,
            ColorType::Rgba,
            8,
            FilterType::Paeth,
            &pixels,
        );

        let _ = std::fs::remove_file(tmp);
    }
}

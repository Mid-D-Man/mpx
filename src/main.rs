// src/main.rs — MPX CLI
//
// Usage:
//   mpx encode    <raw_file> <width> <height> <color_type> <bit_depth> [filter] <output.mpx>
//   mpx decode    <input.mpx> <output_raw>
//   mpx from-png  <input.png> [filter] <output.mpx>
//   mpx to-png    <input.mpx> <output.png>
//   mpx view      <input.mpx>
//   mpx info      <input.mpx>
//   mpx bench     <input.mpx>
//   mpx roundtrip <raw> <w> <h> <color> <bpp>

use std::{env, fs, process, path::Path, time::Instant};
use image::DynamicImage;
use mpx::{ColorType, FilterType, MpxHeader, encode_image, decode_image};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 { usage(); process::exit(1); }

    match args[1].as_str() {
        "encode"    => cmd_encode(&args),
        "decode"    => cmd_decode(&args),
        "from-png"  => cmd_from_png(&args),
        "to-png"    => cmd_to_png(&args),
        "view"      => cmd_view(&args),
        "info"      => cmd_info(&args),
        "bench"     => cmd_bench(&args),
        "roundtrip" => cmd_roundtrip(&args),
        _           => { eprintln!("Unknown command: {}", args[1]); usage(); process::exit(1); }
    }
}

// ── from-png ──────────────────────────────────────────────────────────────────

fn cmd_from_png(args: &[String]) {
    // mpx from-png <input.png> [filter] <output.mpx>
    if args.len() < 4 { usage(); process::exit(1); }

    let png_path = &args[2];
    let (filter, out_path) = if args.len() >= 5 {
        (parse_filter_opt(&args[3]), args[4].as_str())
    } else {
        (FilterType::Paeth, args[3].as_str())
    };

    let img = image::open(png_path)
        .unwrap_or_else(|e| die(&format!("open '{}': {}", png_path, e)));

    let (w, h) = (img.width(), img.height());
    let color  = img.color();

    let (mpx_ct, bpp, raw): (ColorType, u8, Vec<u8>) = match color {
        image::ColorType::L8 => {
            (ColorType::Gray, 8, img.into_luma8().into_raw())
        }
        image::ColorType::La8 => {
            (ColorType::GrayA, 8, img.into_luma_alpha8().into_raw())
        }
        image::ColorType::Rgb8 => {
            (ColorType::Rgb, 8, img.into_rgb8().into_raw())
        }
        image::ColorType::Rgba8 => {
            (ColorType::Rgba, 8, img.into_rgba8().into_raw())
        }
        image::ColorType::L16 => {
            let raw: Vec<u8> = img.into_luma16().into_raw()
                .into_iter().flat_map(|v: u16| v.to_le_bytes()).collect();
            (ColorType::Gray, 16, raw)
        }
        image::ColorType::La16 => {
            let raw: Vec<u8> = img.into_luma_alpha16().into_raw()
                .into_iter().flat_map(|v: u16| v.to_le_bytes()).collect();
            (ColorType::GrayA, 16, raw)
        }
        image::ColorType::Rgb16 => {
            let raw: Vec<u8> = img.into_rgb16().into_raw()
                .into_iter().flat_map(|v: u16| v.to_le_bytes()).collect();
            (ColorType::Rgb, 16, raw)
        }
        image::ColorType::Rgba16 => {
            let raw: Vec<u8> = img.into_rgba16().into_raw()
                .into_iter().flat_map(|v: u16| v.to_le_bytes()).collect();
            (ColorType::Rgba, 16, raw)
        }
        other => {
            // Unknown/exotic color type — normalise to Rgb8
            eprintln!("Note: unsupported color type {:?} — converting to Rgb8", other);
            (ColorType::Rgb, 8, image::DynamicImage::from(
                image::open(png_path).unwrap().into_rgb8()
            ).into_rgb8().into_raw())
        }
    };

    let start   = Instant::now();
    let encoded = encode_image(w, h, mpx_ct, bpp, filter, &raw)
        .unwrap_or_else(|e| die(&format!("encode: {}", e)));
    let elapsed = start.elapsed();

    fs::write(out_path, &encoded)
        .unwrap_or_else(|e| die(&format!("write '{}': {}", out_path, e)));

    let ratio = encoded.len() as f64 / raw.len() as f64 * 100.0;
    println!("from-png: {} → {}", png_path, out_path);
    println!(
        "  {} × {} {:?} {}bpp  [filter={:?}]",
        w, h, mpx_ct, bpp, filter
    );
    println!(
        "  {} → {} bytes ({:.2}%)  in {:.2}ms",
        raw.len(), encoded.len(), ratio,
        elapsed.as_secs_f64() * 1000.0
    );
}

// ── to-png ────────────────────────────────────────────────────────────────────

fn cmd_to_png(args: &[String]) {
    // mpx to-png <input.mpx> <output.png>
    if args.len() < 4 { usage(); process::exit(1); }

    let mpx_path = &args[2];
    let png_path = &args[3];

    let data = fs::read(mpx_path)
        .unwrap_or_else(|e| die(&format!("read '{}': {}", mpx_path, e)));

    let start = Instant::now();
    let (header, pixels) = decode_image(&data)
        .unwrap_or_else(|e| die(&format!("decode: {}", e)));
    let elapsed = start.elapsed();

    let dynimg = pixels_to_dynimage(&header, pixels);

    dynimg.save(png_path)
        .unwrap_or_else(|e| die(&format!("save '{}': {}", png_path, e)));

    println!("to-png: {} → {}", mpx_path, png_path);
    println!(
        "  {} × {} {:?} {}bpp  decoded in {:.2}ms",
        header.width, header.height,
        header.color_type, header.bit_depth,
        elapsed.as_secs_f64() * 1000.0
    );
}

// ── view ──────────────────────────────────────────────────────────────────────

fn cmd_view(args: &[String]) {
    // mpx view <input.mpx>
    // Decodes to a temp PNG then launches the OS image viewer.
    if args.len() < 3 { usage(); process::exit(1); }

    let mpx_path = &args[2];
    let stem = Path::new(mpx_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mpx_preview");

    let tmp_path = std::env::temp_dir().join(format!("{}.png", stem));
    let tmp_str  = tmp_path.to_string_lossy().to_string();

    // Reuse to-png logic
    let to_png_args = vec![
        String::new(),
        "to-png".into(),
        mpx_path.clone(),
        tmp_str.clone(),
    ];
    cmd_to_png(&to_png_args);

    // Launch OS viewer
    let open_result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&tmp_str).spawn()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &tmp_str])
            .spawn()
    } else {
        // Linux / *BSD — try xdg-open, fall back to eog, feh
        std::process::Command::new("xdg-open").arg(&tmp_str).spawn()
            .or_else(|_| std::process::Command::new("eog").arg(&tmp_str).spawn())
            .or_else(|_| std::process::Command::new("feh").arg(&tmp_str).spawn())
    };

    match open_result {
        Ok(_)  => println!("Opened {} in system viewer.", tmp_str),
        Err(e) => eprintln!(
            "Could not launch viewer ({}). PNG saved to:\n  {}",
            e, tmp_str
        ),
    }
}

// ── encode ────────────────────────────────────────────────────────────────────

fn cmd_encode(args: &[String]) {
    // mpx encode <raw_file> <width> <height> <color_type> <bit_depth> [filter] <output.mpx>
    if args.len() < 8 { usage(); process::exit(1); }

    let raw_path      = &args[2];
    let width:  u32   = parse_or_die(&args[3], "width");
    let height: u32   = parse_or_die(&args[4], "height");
    let color_type    = parse_color_type(&args[5]);
    let bit_depth: u8 = parse_or_die(&args[6], "bit_depth");

    let (filter, out_path) = if args.len() >= 9 {
        (parse_filter_opt(&args[7]), args[8].as_str())
    } else {
        (FilterType::Paeth, args[7].as_str())
    };

    let pixels = fs::read(raw_path)
        .unwrap_or_else(|e| die(&format!("read '{}': {}", raw_path, e)));

    let start   = Instant::now();
    let encoded = encode_image(width, height, color_type, bit_depth, filter, &pixels)
        .unwrap_or_else(|e| die(&format!("encode failed: {}", e)));
    let elapsed = start.elapsed();

    fs::write(out_path, &encoded)
        .unwrap_or_else(|e| die(&format!("write '{}': {}", out_path, e)));

    let ratio = encoded.len() as f64 / pixels.len() as f64 * 100.0;
    println!(
        "Encoded: {} × {} {:?} {}bpp [filter={:?}]",
        width, height, color_type, bit_depth, filter
    );
    println!(
        "  {} → {} bytes ({:.1}%)  in {:.2}ms",
        pixels.len(), encoded.len(), ratio,
        elapsed.as_secs_f64() * 1000.0
    );
}

// ── decode ────────────────────────────────────────────────────────────────────

fn cmd_decode(args: &[String]) {
    if args.len() < 4 { usage(); process::exit(1); }

    let data = fs::read(&args[2])
        .unwrap_or_else(|e| die(&format!("read '{}': {}", &args[2], e)));

    let start = Instant::now();
    let (header, pixels) = decode_image(&data)
        .unwrap_or_else(|e| die(&format!("decode: {}", e)));
    let elapsed = start.elapsed();

    fs::write(&args[3], &pixels)
        .unwrap_or_else(|e| die(&format!("write '{}': {}", &args[3], e)));

    println!(
        "Decoded: {} × {} {:?} {}bpp → {} bytes  in {:.2}ms",
        header.width, header.height, header.color_type, header.bit_depth,
        pixels.len(),
        elapsed.as_secs_f64() * 1000.0
    );
}

// ── info ──────────────────────────────────────────────────────────────────────

fn cmd_info(args: &[String]) {
    if args.len() < 3 { usage(); process::exit(1); }

    let data = fs::read(&args[2])
        .unwrap_or_else(|e| die(&format!("read '{}': {}", &args[2], e)));
    let (header, pixels) = decode_image(&data)
        .unwrap_or_else(|e| die(&format!("decode: {}", e)));

    println!("MPX Image Info — {}", Path::new(&args[2]).file_name()
        .and_then(|n| n.to_str()).unwrap_or(&args[2]));
    println!("  Dimensions:    {} × {}", header.width, header.height);
    println!("  Color type:    {:?} ({} channel(s))", header.color_type, header.channel_count());
    println!("  Bit depth:     {}", header.bit_depth);
    println!("  Filter:        {:?}", header.filter_type);
    println!("  Flags:         0x{:02x}{}", header.flags,
        if header.byte_plane_split() { " (byte-plane-split)" } else { "" });
    println!("  File size:     {} bytes", data.len());
    println!("  Uncompressed:  {} bytes", pixels.len());
    println!("  Ratio:         {:.2}%",
        data.len() as f64 / pixels.len() as f64 * 100.0);
}

// ── bench ─────────────────────────────────────────────────────────────────────

fn cmd_bench(args: &[String]) {
    if args.len() < 3 { usage(); process::exit(1); }

    let data = fs::read(&args[2])
        .unwrap_or_else(|e| die(&format!("read '{}': {}", &args[2], e)));
    let runs = 10usize;

    let mut total_ns = 0u128;
    for _ in 0..runs {
        let start = Instant::now();
        let _ = decode_image(&data)
            .unwrap_or_else(|e| die(&format!("decode: {}", e)));
        total_ns += start.elapsed().as_nanos();
    }

    let avg_ms = total_ns as f64 / runs as f64 / 1_000_000.0;
    println!("Decode bench ({} runs): {:.3}ms avg", runs, avg_ms);
}

// ── roundtrip ─────────────────────────────────────────────────────────────────

fn cmd_roundtrip(args: &[String]) {
    if args.len() < 3 { usage(); process::exit(1); }

    let raw       = fs::read(&args[2])
        .unwrap_or_else(|e| die(&format!("read '{}': {}", &args[2], e)));
    let width:  u32 = parse_or_die(&args[3], "width");
    let height: u32 = parse_or_die(&args[4], "height");
    let color_type  = parse_color_type(&args[5]);
    let bit_depth: u8 = parse_or_die(&args[6], "bit_depth");

    let encoded = encode_image(width, height, color_type, bit_depth, FilterType::Paeth, &raw)
        .unwrap_or_else(|e| die(&format!("encode: {}", e)));

    let (_, decoded) = decode_image(&encoded)
        .unwrap_or_else(|e| die(&format!("decode: {}", e)));

    if decoded == raw {
        println!(
            "PASS — roundtrip pixel-perfect ({} bytes → {} bytes → {} bytes)",
            raw.len(), encoded.len(), decoded.len()
        );
    } else {
        eprintln!("FAIL — decoded output does not match input!");
        for (i, (a, b)) in raw.iter().zip(decoded.iter()).enumerate() {
            if a != b {
                eprintln!("  First diff at byte {}: original={} decoded={}", i, a, b);
                break;
            }
        }
        process::exit(1);
    }
}

// ── pixels_to_dynimage ────────────────────────────────────────────────────────

fn pixels_to_dynimage(header: &MpxHeader, pixels: Vec<u8>) -> DynamicImage {
    let w = header.width;
    let h = header.height;

    match (header.color_type, header.bit_depth) {
        (ColorType::Gray, 8) => DynamicImage::ImageLuma8(
            image::ImageBuffer::from_raw(w, h, pixels)
                .unwrap_or_else(|| die("Gray8 buffer size mismatch"))
        ),
        (ColorType::GrayA, 8) => DynamicImage::ImageLumaA8(
            image::ImageBuffer::from_raw(w, h, pixels)
                .unwrap_or_else(|| die("GrayA8 buffer size mismatch"))
        ),
        (ColorType::Rgb, 8) => DynamicImage::ImageRgb8(
            image::ImageBuffer::from_raw(w, h, pixels)
                .unwrap_or_else(|| die("Rgb8 buffer size mismatch"))
        ),
        (ColorType::Rgba, 8) => DynamicImage::ImageRgba8(
            image::ImageBuffer::from_raw(w, h, pixels)
                .unwrap_or_else(|| die("Rgba8 buffer size mismatch"))
        ),
        (ColorType::Gray, 16) => {
            let samples: Vec<u16> = pixels.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            DynamicImage::ImageLuma16(
                image::ImageBuffer::from_raw(w, h, samples)
                    .unwrap_or_else(|| die("Gray16 buffer size mismatch"))
            )
        }
        (ColorType::GrayA, 16) => {
            let samples: Vec<u16> = pixels.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            DynamicImage::ImageLumaA16(
                image::ImageBuffer::from_raw(w, h, samples)
                    .unwrap_or_else(|| die("GrayA16 buffer size mismatch"))
            )
        }
        (ColorType::Rgb, 16) => {
            let samples: Vec<u16> = pixels.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            DynamicImage::ImageRgb16(
                image::ImageBuffer::from_raw(w, h, samples)
                    .unwrap_or_else(|| die("Rgb16 buffer size mismatch"))
            )
        }
        (ColorType::Rgba, 16) => {
            let samples: Vec<u16> = pixels.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            DynamicImage::ImageRgba16(
                image::ImageBuffer::from_raw(w, h, samples)
                    .unwrap_or_else(|| die("Rgba16 buffer size mismatch"))
            )
        }
        (ct, bpp) => die(&format!("unsupported {:?} {}bpp for PNG output", ct, bpp)),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_color_type(s: &str) -> ColorType {
    match s.to_lowercase().as_str() {
        "gray" | "grey" | "l"  => ColorType::Gray,
        "graya" | "la"         => ColorType::GrayA,
        "rgb"                  => ColorType::Rgb,
        "rgba"                 => ColorType::Rgba,
        _ => die(&format!("unknown color type '{}' (gray | graya | rgb | rgba)", s)),
    }
}

fn parse_filter_opt(s: &str) -> FilterType {
    match s.to_lowercase().as_str() {
        "none"     => FilterType::None,
        "sub"      => FilterType::Sub,
        "up"       => FilterType::Up,
        "average"  => FilterType::Average,
        "paeth"    => FilterType::Paeth,
        "adaptive" => FilterType::Adaptive,
        _          => FilterType::Paeth,
    }
}

fn parse_or_die<T: std::str::FromStr>(s: &str, label: &str) -> T {
    s.parse::<T>().unwrap_or_else(|_| {
        die(&format!("{} must be a valid number, got '{}'", label, s))
    })
}

fn die(msg: &str) -> ! {
    eprintln!("Error: {}", msg);
    process::exit(1);
}

fn usage() {
    eprintln!("Usage:");
    eprintln!("  mpx encode    <raw> <w> <h> <color> <bpp> [filter] <out.mpx>");
    eprintln!("  mpx decode    <in.mpx> <out_raw>");
    eprintln!("  mpx from-png  <in.png> [filter] <out.mpx>");
    eprintln!("  mpx to-png    <in.mpx> <out.png>");
    eprintln!("  mpx view      <in.mpx>            (decodes + opens system viewer)");
    eprintln!("  mpx info      <in.mpx>");
    eprintln!("  mpx bench     <in.mpx>");
    eprintln!("  mpx roundtrip <raw> <w> <h> <color> <bpp>");
    eprintln!();
    eprintln!("  color:  gray | graya | rgb | rgba");
    eprintln!("  bpp:    8 | 16");
    eprintln!("  filter: none | sub | up | average | paeth | adaptive  (default: paeth)");
}

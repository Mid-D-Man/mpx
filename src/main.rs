// Auto-generated stub
// src/main.rs — MPX CLI
//
// Usage:
//   mpx encode <raw_file> <width> <height> <color_type> <bit_depth> [filter] <output.mpx>
//   mpx decode <input.mpx> <output_raw>
//   mpx info   <input.mpx>
//   mpx bench  <input.mpx>              -- decode timing only
//   mpx convert-png <input.png> <output.mpx>  -- requires 'image' feature

use std::{env, fs, process, path::Path, time::Instant};
use mpx::{ColorType, FilterType, encode_image, decode_image};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 { usage(); process::exit(1); }

    match args[1].as_str() {
        "encode"      => cmd_encode(&args),
        "decode"      => cmd_decode(&args),
        "info"        => cmd_info(&args),
        "bench"       => cmd_bench(&args),
        "roundtrip"   => cmd_roundtrip(&args),
        _             => { eprintln!("Unknown command: {}", args[1]); usage(); process::exit(1); }
    }
}

// ── encode ────────────────────────────────────────────────────────────────────

fn cmd_encode(args: &[String]) {
    // mpx encode <raw_file> <width> <height> <color_type> <bit_depth> [filter] <output.mpx>
    if args.len() < 8 { usage(); process::exit(1); }

    let raw_path   = &args[2];
    let width:  u32 = parse_or_die(&args[3], "width");
    let height: u32 = parse_or_die(&args[4], "height");
    let color_type  = parse_color_type(&args[5]);
    let bit_depth:u8 = parse_or_die(&args[6], "bit_depth");

    // Optional filter argument: if args[7] looks like a filter name, consume it;
    // otherwise the output path is at args[7].
    let (filter, out_path) = if args.len() >= 9 {
        (parse_filter_opt(&args[7]), args[8].as_str())
    } else {
        (FilterType::Paeth, args[7].as_str())
    };

    let pixels = fs::read(raw_path).unwrap_or_else(|e| die(&format!("read {}: {}", raw_path, e)));

    let start   = Instant::now();
    let encoded = encode_image(width, height, color_type, bit_depth, filter, &pixels)
        .unwrap_or_else(|e| die(&format!("encode failed: {}", e)));
    let elapsed = start.elapsed();

    fs::write(out_path, &encoded).unwrap_or_else(|e| die(&format!("write {}: {}", out_path, e)));

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
    let data = fs::read(&args[2]).unwrap_or_else(|e| die(&format!("read {}: {}", &args[2], e)));
    let start = Instant::now();
    let (header, pixels) = decode_image(&data).unwrap_or_else(|e| die(&format!("decode: {}", e)));
    let elapsed = start.elapsed();

    fs::write(&args[3], &pixels).unwrap_or_else(|e| die(&format!("write {}: {}", &args[3], e)));

    println!(
        "Decoded: {} × {} {:?} {}bpp → {} bytes  in {:.2}ms",
        header.width, header.height, header.color_type, header.bit_depth,
        pixels.len(),
        elapsed.as_secs_f64() * 1000.0,
    );
}

// ── info ──────────────────────────────────────────────────────────────────────

fn cmd_info(args: &[String]) {
    if args.len() < 3 { usage(); process::exit(1); }
    let data = fs::read(&args[2]).unwrap_or_else(|e| die(&format!("read {}: {}", &args[2], e)));
    let (header, pixels) = decode_image(&data).unwrap_or_else(|e| die(&format!("decode: {}", e)));

    println!("MPX Image Info — {:?}", Path::new(&args[2]).file_name().unwrap());
    println!("  Dimensions:    {} × {}", header.width, header.height);
    println!("  Color type:    {:?} ({} channel(s))", header.color_type, header.channel_count());
    println!("  Bit depth:     {}", header.bit_depth);
    println!("  Filter:        {:?}", header.filter_type);
    println!("  Flags:         0x{:02x}{}", header.flags,
        if header.byte_plane_split() { " (byte-plane-split)" } else { "" });
    println!("  File size:     {} bytes", data.len());
    println!("  Uncompressed:  {} bytes", pixels.len());
    println!("  Ratio:         {:.1}%", data.len() as f64 / pixels.len() as f64 * 100.0);
}

// ── bench ─────────────────────────────────────────────────────────────────────

fn cmd_bench(args: &[String]) {
    if args.len() < 3 { usage(); process::exit(1); }
    let data = fs::read(&args[2]).unwrap_or_else(|e| die(&format!("read {}: {}", &args[2], e)));
    let runs = 10usize;

    let mut total_ns = 0u128;
    for _ in 0..runs {
        let start = Instant::now();
        let _ = decode_image(&data).unwrap_or_else(|e| die(&format!("decode: {}", e)));
        total_ns += start.elapsed().as_nanos();
    }

    let avg_ms = total_ns as f64 / runs as f64 / 1_000_000.0;
    println!("Decode bench ({} runs): {:.3}ms avg", runs, avg_ms);
}

// ── roundtrip (self-test) ─────────────────────────────────────────────────────

fn cmd_roundtrip(args: &[String]) {
    if args.len() < 3 { usage(); process::exit(1); }
    let raw = fs::read(&args[2]).unwrap_or_else(|e| die(&format!("read {}: {}", &args[2], e)));

    let width:  u32 = parse_or_die(&args[3], "width");
    let height: u32 = parse_or_die(&args[4], "height");
    let color_type  = parse_color_type(&args[5]);
    let bit_depth:u8 = parse_or_die(&args[6], "bit_depth");

    let encoded = encode_image(width, height, color_type, bit_depth, FilterType::Paeth, &raw)
        .unwrap_or_else(|e| die(&format!("encode: {}", e)));

    let (_, decoded) = decode_image(&encoded)
        .unwrap_or_else(|e| die(&format!("decode: {}", e)));

    if decoded == raw {
        println!("PASS — roundtrip pixel-perfect ({} bytes → {} bytes → {} bytes)",
            raw.len(), encoded.len(), decoded.len());
    } else {
        eprintln!("FAIL — decoded output does not match input!");
        // Find first differing byte
        for (i, (a, b)) in raw.iter().zip(decoded.iter()).enumerate() {
            if a != b {
                eprintln!("  First diff at byte {}: original={} decoded={}", i, a, b);
                break;
            }
        }
        process::exit(1);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_color_type(s: &str) -> ColorType {
    match s.to_lowercase().as_str() {
        "gray" | "grey" | "l"    => ColorType::Gray,
        "graya" | "la"           => ColorType::GrayA,
        "rgb"                    => ColorType::Rgb,
        "rgba"                   => ColorType::Rgba,
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
        _          => FilterType::Paeth, // not a filter name → use default
    }
}

fn parse_or_die<T: std::str::FromStr>(s: &str, label: &str) -> T {
    s.parse::<T>().unwrap_or_else(|_| die(&format!("{} must be a valid number, got '{}'", label, s)))
}

fn die(msg: &str) -> ! {
    eprintln!("Error: {}", msg);
    process::exit(1);
}

fn usage() {
    eprintln!("Usage:");
    eprintln!("  mpx encode    <raw> <w> <h> <color> <bpp> [filter] <out.mpx>");
    eprintln!("  mpx decode    <in.mpx> <out_raw>");
    eprintln!("  mpx info      <in.mpx>");
    eprintln!("  mpx bench     <in.mpx>");
    eprintln!("  mpx roundtrip <raw> <w> <h> <color> <bpp>");
    eprintln!();
    eprintln!("  color:  gray | graya | rgb | rgba");
    eprintln!("  bpp:    8 | 16");
    eprintln!("  filter: none | sub | up | average | paeth | adaptive  (default: paeth)");
      }

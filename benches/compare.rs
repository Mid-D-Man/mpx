// benches/compare.rs
//! Criterion benchmarks for MPX encode/decode.
//!
//! Run:  cargo bench
//! Run:  cargo bench -- --output-format bencher 2>&1 | tee bench_results.txt
//!
//! IMPORTANT — keep image sizes SMALL (≤128×128).
//! MBFA's encode path does chain discovery and fold attempts that scale
//! super-linearly with input size. A 512×512 image takes ~500ms per encode;
//! Criterion would need ~50 s per benchmark group just to complete 100 samples.
//! Using 64×64 / 128×128 keeps each iteration under 5ms and lets the suite
//! finish in under 2 minutes total.
//!
//! What we measure:
//!   1. Encode time by image type and filter
//!   2. Decode time
//!   3. Throughput (MB/s)

use std::time::Duration;
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use mpx::{ColorType, FilterType, encode_image, decode_image};

// ── Test image generators ─────────────────────────────────────────────────────

fn gradient_rgb(w: usize, h: usize) -> Vec<u8> {
    let mut px = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            px.push(((x * 255) / w.max(1)) as u8);
            px.push(((y * 255) / h.max(1)) as u8);
            px.push(128u8);
        }
    }
    px
}

fn solid_rgba(w: usize, h: usize) -> Vec<u8> {
    vec![64u8, 128, 200, 255].into_iter().cycle().take(w * h * 4).collect()
}

fn lcg_noise_rgb(w: usize, h: usize) -> Vec<u8> {
    let mut s: u32 = 0xdeadbeef;
    (0..w * h * 3).map(|_| {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 24) as u8
    }).collect()
}

fn ramp_gray16(w: usize, h: usize) -> Vec<u8> {
    let total = (w * h) as u32;
    (0..w * h).flat_map(|i| {
        let v: u16 = ((i as u32 * 65535) / total.max(1)) as u16;
        v.to_le_bytes()
    }).collect()
}

// ── Encode benchmarks ─────────────────────────────────────────────────────────

fn bench_encode_gradient(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode/gradient_rgb");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(10);

    // Max 128×128 — 512×512 takes ~500ms/iter and would stall CI for hours.
    for &size in &[32usize, 64, 128] {
        let pixels = gradient_rgb(size, size);
        group.throughput(Throughput::Bytes(pixels.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", size, size)),
            &pixels,
            |b, px| {
                b.iter(|| {
                    encode_image(
                        size as u32, size as u32,
                        ColorType::Rgb, 8, FilterType::Paeth,
                        black_box(px),
                    ).unwrap()
                })
            },
        );
    }
    group.finish();
}

fn bench_encode_solid(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode/solid_rgba");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(10);

    for &size in &[32usize, 64, 128] {
        let pixels = solid_rgba(size, size);
        group.throughput(Throughput::Bytes(pixels.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", size, size)),
            &pixels,
            |b, px| {
                b.iter(|| {
                    encode_image(
                        size as u32, size as u32,
                        ColorType::Rgba, 8, FilterType::Paeth,
                        black_box(px),
                    ).unwrap()
                })
            },
        );
    }
    group.finish();
}

fn bench_encode_noise(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode/noise_rgb");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(10);

    // Noise is incompressible — MBFA exits early, so larger sizes are OK here.
    let size  = 128usize;
    let pixels = lcg_noise_rgb(size, size);
    group.throughput(Throughput::Bytes(pixels.len() as u64));
    group.bench_function("128x128", |b| {
        b.iter(|| {
            encode_image(
                size as u32, size as u32,
                ColorType::Rgb, 8, FilterType::Paeth,
                black_box(&pixels),
            ).unwrap()
        })
    });
    group.finish();
}

fn bench_encode_16bit(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode/16bit_gray");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(10);

    for &size in &[32usize, 64] {
        let pixels = ramp_gray16(size, size);
        group.throughput(Throughput::Bytes(pixels.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", size, size)),
            &pixels,
            |b, px| {
                b.iter(|| {
                    encode_image(
                        size as u32, size as u32,
                        ColorType::Gray, 16, FilterType::Paeth,
                        black_box(px),
                    ).unwrap()
                })
            },
        );
    }
    group.finish();
}

// ── Decode benchmarks ─────────────────────────────────────────────────────────

fn bench_decode_gradient(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode/gradient_rgb");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(10);

    for &size in &[32usize, 64, 128] {
        let pixels  = gradient_rgb(size, size);
        let encoded = encode_image(
            size as u32, size as u32,
            ColorType::Rgb, 8, FilterType::Paeth, &pixels,
        ).unwrap();
        group.throughput(Throughput::Bytes(encoded.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", size, size)),
            &encoded,
            |b, enc| b.iter(|| decode_image(black_box(enc)).unwrap()),
        );
    }
    group.finish();
}

fn bench_decode_solid(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode/solid_rgba");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(10);

    let size    = 64usize;
    let pixels  = solid_rgba(size, size);
    let encoded = encode_image(
        size as u32, size as u32,
        ColorType::Rgba, 8, FilterType::Paeth, &pixels,
    ).unwrap();
    group.throughput(Throughput::Bytes(encoded.len() as u64));
    group.bench_function("64x64", |b| {
        b.iter(|| decode_image(black_box(&encoded)).unwrap())
    });
    group.finish();
}

// ── Filter comparison ─────────────────────────────────────────────────────────

fn bench_filters_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode/filter_comparison");
    group.measurement_time(Duration::from_secs(3));
    group.sample_size(10);

    let (w, h) = (64usize, 64usize);
    let pixels = gradient_rgb(w, h);
    group.throughput(Throughput::Bytes(pixels.len() as u64));

    for filter in [
        FilterType::None, FilterType::Sub, FilterType::Up,
        FilterType::Average, FilterType::Paeth, FilterType::Adaptive,
    ] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{:?}", filter)),
            &pixels,
            |b, px| {
                b.iter(|| {
                    encode_image(w as u32, h as u32, ColorType::Rgb, 8, filter, black_box(px)).unwrap()
                })
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_encode_gradient,
    bench_encode_solid,
    bench_encode_noise,
    bench_encode_16bit,
    bench_decode_gradient,
    bench_decode_solid,
    bench_filters_encode,
);
criterion_main!(benches);

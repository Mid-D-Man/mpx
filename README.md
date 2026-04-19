# MPX — MidPixel Image Format

Lossless image format built on [MBFA](../mbfa) instruction-chain compression.

## Stack

- Spatial prediction filter (Sub / Up / Average / Paeth / Adaptive)
- Per-channel plane separation
- Byte-plane split for 16-bit (hi/lo byte planes compressed independently)
- MBFA multi-fold LZ + entropy coding

## Usage

```bash
# Encode a raw pixel file
cargo run --release -- encode image.raw 512 512 rgb 8 paeth image.mpx

# Decode
cargo run --release -- decode image.mpx recovered.raw

# Roundtrip self-test
cargo run --release -- roundtrip image.raw 512 512 rgb 8

# File info
cargo run --release -- info image.mpx
```

## Build

```bash
# Requires ../mbfa as a sibling directory
cargo build --release
cargo test
cargo bench
```

## License

MIT

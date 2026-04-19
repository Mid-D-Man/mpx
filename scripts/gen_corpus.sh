#!/usr/bin/env bash
# gen_corpus.sh — generate synthetic test corpus
# Requires: ImageMagick (convert command)
set -euo pipefail

OUT=corpus/synthetic
mkdir -p "$OUT"

echo 'Generating synthetic corpus...'

# Solid colour 512x512 RGB
convert -size 512x512 xc:"#4080C0" -depth 8 "$OUT/solid_512_rgb.png"
convert "$OUT/solid_512_rgb.png" rgb:"$OUT/solid_512_rgb.raw"
echo '512 512 rgb 8' > "$OUT/solid_512_rgb.meta"

# Horizontal gradient 512x512 RGB
convert -size 512x512 gradient:black-white -depth 8 "$OUT/grad_h_512_rgb.png"
convert "$OUT/grad_h_512_rgb.png" rgb:"$OUT/grad_h_512_rgb.raw"
echo '512 512 rgb 8' > "$OUT/grad_h_512_rgb.meta"

# Checkerboard 512x512 RGB (8px tiles)
convert -size 512x512 pattern:checkerboard -depth 8 "$OUT/checker_512_rgb.png"
convert "$OUT/checker_512_rgb.png" rgb:"$OUT/checker_512_rgb.raw"
echo '512 512 rgb 8' > "$OUT/checker_512_rgb.meta"

# Noise 256x256 RGB (incompressible baseline)
convert -size 256x256 xc: +noise Random -depth 8 "$OUT/noise_256_rgb.png"
convert "$OUT/noise_256_rgb.png" rgb:"$OUT/noise_256_rgb.raw"
echo '256 256 rgb 8' > "$OUT/noise_256_rgb.meta"

# 16-bit ramp grayscale 256x256
convert -size 256x256 gradient:gray0-gray100 -depth 16 -endian LSB "$OUT/ramp_256_gray16.png"
convert "$OUT/ramp_256_gray16.png" gray:"$OUT/ramp_256_gray16.raw"
echo '256 256 gray 16' > "$OUT/ramp_256_gray16.meta"

echo 'Corpus ready in corpus/synthetic/'

#!/usr/bin/env bash
# bench_corpus.sh — run MPX encode/decode against every .raw in corpus/
# and print a comparison table vs PNG (requires ImageMagick).
set -euo pipefail

BIN="./target/release/mpx"
if [ ! -f "$BIN" ]; then
  cargo build --release
fi

printf '%-35s %10s %10s %10s %10s %12s %12s\n' \
  FILE RAW_BYTES PNG_BYTES MPX_BYTES PNG_PCT MPX_PCT MPX_ENC_MS
printf '%s\n' "$(printf '%.0s-' {1..100})"

for meta in corpus/**/*.meta; do
  raw="${meta%.meta}.raw"
  [ -f "$raw" ] || continue

  read -r W H COLOR BPP < "$meta"
  RAW_BYTES=$(wc -c < "$raw")

  # MPX encode
  START=$(date +%s%N)
  "$BIN" encode "$raw" "$W" "$H" "$COLOR" "$BPP" paeth /tmp/out.mpx 2>/dev/null
  END=$(date +%s%N)
  MPX_BYTES=$(wc -c < /tmp/out.mpx)
  MPX_MS=$(( (END - START) / 1000000 ))

  # PNG encode via ImageMagick (for size comparison only)
  PNG_BYTES=0
  if command -v convert &>/dev/null; then
    convert -size "${W}x${H}" -depth "$BPP" "${COLOR}:${raw}" /tmp/out_cmp.png 2>/dev/null
    PNG_BYTES=$(wc -c < /tmp/out_cmp.png)
  fi

  PNG_PCT=$(awk "BEGIN { printf \"%.1f\", $PNG_BYTES / $RAW_BYTES * 100 }")
  MPX_PCT=$(awk "BEGIN { printf \"%.1f\", $MPX_BYTES / $RAW_BYTES * 100 }")
  FILE=$(basename "${raw%.raw}")

  printf '%-35s %10d %10d %10d %9s%% %11s%% %9dms\n' \
    "$FILE" "$RAW_BYTES" "$PNG_BYTES" "$MPX_BYTES" "$PNG_PCT" "$MPX_PCT" "$MPX_MS"
done

rm -f /tmp/out.mpx /tmp/out_cmp.png
echo
echo 'Done. Run cargo bench for detailed per-operation numbers.'

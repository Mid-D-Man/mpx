#!/usr/bin/env bash
# verify_corpus.sh — roundtrip every .raw in corpus/ and assert pixel-perfect output.
set -euo pipefail

BIN="./target/release/mpx"
[ -f "$BIN" ] || cargo build --release

PASS=0
FAIL=0

for meta in corpus/**/*.meta; do
  raw="${meta%.meta}.raw"
  [ -f "$raw" ] || continue
  read -r W H COLOR BPP < "$meta"
  FILE=$(basename "${raw%.raw}")

  if "$BIN" roundtrip "$raw" "$W" "$H" "$COLOR" "$BPP" 2>/dev/null; then
    echo "PASS  $FILE"
    PASS=$((PASS + 1))
  else
    echo "FAIL  $FILE"
    FAIL=$((FAIL + 1))
  fi
done

echo
echo "Results: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || exit 1

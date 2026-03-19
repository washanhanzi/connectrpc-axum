#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRITERION_DIR="${1:-$ROOT_DIR/target/criterion}"

if [[ ! -d "$CRITERION_DIR" ]]; then
  echo "criterion directory not found: $CRITERION_DIR" >&2
  exit 1
fi

groups=(
  proto_encode_hello_request
  proto_decode_hello_request
  json_encode_hello_request
  json_decode_hello_request
  connect_unary_proto_roundtrip
  connect_unary_json_roundtrip
  connect_stream_proto_roundtrip
)

sizes=(small medium large)

printf "| Benchmark | Size | Buffa median (ns) | Beta5 median (ns) | Buffa delta |\n"
printf "|---|---:|---:|---:|---:|\n"

for group in "${groups[@]}"; do
  for size in "${sizes[@]}"; do
    buffa_file="$CRITERION_DIR/$group/buffa/$size/new/estimates.json"
    beta5_file="$CRITERION_DIR/$group/beta5/$size/new/estimates.json"

    if [[ ! -f "$buffa_file" || ! -f "$beta5_file" ]]; then
      continue
    fi

    buffa_median="$(jq -r '.median.point_estimate' "$buffa_file")"
    beta5_median="$(jq -r '.median.point_estimate' "$beta5_file")"
    delta_pct="$(
      awk -v b="$buffa_median" -v p="$beta5_median" \
        'BEGIN { printf "%.1f%%", ((p - b) / p) * 100 }'
    )"

    printf "| %s | %s | %.3f | %.3f | %s |\n" \
      "$group" "$size" "$buffa_median" "$beta5_median" "$delta_pct"
  done
done

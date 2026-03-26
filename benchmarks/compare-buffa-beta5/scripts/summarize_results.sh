#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRITERION_DIR="${1:-$ROOT_DIR/target/criterion}"

if [[ ! -d "$CRITERION_DIR" ]]; then
  echo "criterion directory not found: $CRITERION_DIR" >&2
  exit 1
fi

groups=(
  connect_unary_proto_roundtrip
  connect_unary_json_roundtrip
  connect_stream_proto_roundtrip
)

sizes=(small medium large)

printf "| Benchmark | Size | Buffa median (ns) | 0.1.0 median (ns) | Connect-rust median (ns) | Buffa delta vs 0.1.0 | Connect-rust delta vs 0.1.0 |\n"
printf "|---|---:|---:|---:|---:|---:|---:|\n"

for group in "${groups[@]}"; do
  for size in "${sizes[@]}"; do
    buffa_file="$CRITERION_DIR/$group/buffa/$size/new/estimates.json"
    release_file="$CRITERION_DIR/$group/v0.1.0/$size/new/estimates.json"
    connectrust_file="$CRITERION_DIR/$group/connect-rust/$size/new/estimates.json"

    if [[ ! -f "$buffa_file" || ! -f "$release_file" || ! -f "$connectrust_file" ]]; then
      continue
    fi

    buffa_median="$(jq -r '.median.point_estimate' "$buffa_file")"
    release_median="$(jq -r '.median.point_estimate' "$release_file")"
    connectrust_median="$(jq -r '.median.point_estimate' "$connectrust_file")"
    buffa_delta_pct="$(
      awk -v b="$buffa_median" -v p="$release_median" \
        'BEGIN { printf "%.1f%%", ((p - b) / p) * 100 }'
    )"
    connectrust_delta_pct="$(
      awk -v c="$connectrust_median" -v p="$release_median" \
        'BEGIN { printf "%.1f%%", ((p - c) / p) * 100 }'
    )"

    printf "| %s | %s | %.3f | %.3f | %.3f | %s | %s |\n" \
      "$group" "$size" "$buffa_median" "$release_median" "$connectrust_median" \
      "$buffa_delta_pct" "$connectrust_delta_pct"
  done
done

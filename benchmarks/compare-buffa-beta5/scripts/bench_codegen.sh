#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if command -v hyperfine >/dev/null 2>&1; then
  hyperfine \
    --warmup 1 \
    --prepare 'rm -rf target/codegen-buffa target/codegen-beta5' \
    'CARGO_TARGET_DIR=target/codegen-buffa cargo check -p compare-buffa-beta5-cases-buffa' \
    'CARGO_TARGET_DIR=target/codegen-beta5 cargo check -p compare-buffa-beta5-cases-beta5'
  exit 0
fi

echo "hyperfine not found; falling back to shell builtin time -p"

for target in buffa beta5; do
  crate="compare-buffa-beta5-cases-${target}"
  target_dir="target/codegen-${target}"

  echo
  echo "==> ${crate}"
  rm -rf "$target_dir"
  time -p env CARGO_TARGET_DIR="$target_dir" cargo check -p "$crate"
done

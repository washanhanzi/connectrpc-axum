#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if command -v hyperfine >/dev/null 2>&1; then
  hyperfine \
    --warmup 1 \
    --prepare 'rm -rf target/codegen-buffa target/codegen-v0.1.0 target/codegen-connectrpc' \
    'CARGO_TARGET_DIR=target/codegen-buffa cargo check -p compare-buffa-beta5-cases-buffa' \
    'CARGO_TARGET_DIR=target/codegen-v0.1.0 cargo check -p compare-buffa-beta5-cases-release' \
    'CARGO_TARGET_DIR=target/codegen-connectrpc cargo check -p compare-buffa-beta5-cases-connectrpc'
  exit 0
fi

echo "hyperfine not found; falling back to shell builtin time -p"

run_case() {
  local label="$1"
  local crate="$2"
  local target_dir="$3"

  echo
  echo "==> ${label}"
  rm -rf "$target_dir"
  time -p env CARGO_TARGET_DIR="$target_dir" cargo check -p "$crate"
}

run_case "buffa" "compare-buffa-beta5-cases-buffa" "target/codegen-buffa"
run_case "connect-axum-0.1.0" "compare-buffa-beta5-cases-release" "target/codegen-v0.1.0"
run_case "connect-rust" "compare-buffa-beta5-cases-connectrpc" "target/codegen-connectrpc"

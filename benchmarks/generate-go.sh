#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
PATH="$HOME/go/bin:$PATH"

rm -rf connect-go/gen
buf generate

(cd connect-go && go mod tidy)

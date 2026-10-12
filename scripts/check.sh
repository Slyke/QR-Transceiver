#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --all -- --check
cargo test --locked
cargo test --locked -p qrtx-runtime --features camera
cargo clippy --locked --all-targets -- -D warnings
npm run check
npm run build

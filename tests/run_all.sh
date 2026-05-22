#!/usr/bin/env bash
# Run all Supvan workspace tests.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "=== Rust unit + integration tests ==="
cargo test --workspace --release
echo

echo "=== IPP golden fixture builder ==="
cargo test -p supvan-ipp --release --test golden_ipp
echo

echo "Done. For CUPS acceptance on hardware, see docs/CUPS_ACCEPTANCE.md"

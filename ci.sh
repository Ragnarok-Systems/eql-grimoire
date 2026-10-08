#!/bin/bash
set -euo pipefail

echo "Running: cargo fmt --all --check"
cargo fmt --all --check

echo "Running: cargo build --workspace --all-targets"
cargo build --workspace --all-targets

echo "Running: cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

echo "Running: cargo test --workspace"
cargo test --workspace

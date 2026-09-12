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

echo "Installing wasm target if needed..."
rustup target add wasm32-unknown-unknown

echo "Running: cargo build --release -p grimoire-wasm --target wasm32-unknown-unknown"
cargo build --release -p grimoire-wasm --target wasm32-unknown-unknown

echo "Asserting wasm module has no imports..."
WASM_FILE="target/wasm32-unknown-unknown/release/grimoire_wasm.wasm"
node -e "
const fs = require('fs');
const wasm = fs.readFileSync('$WASM_FILE');
let pos = 8;
let hasImportSection = false;
while (pos < wasm.length) {
  const sectionId = wasm[pos++];
  let size = 0, shift = 0;
  let byte;
  do {
    byte = wasm[pos++];
    size |= (byte & 0x7f) << shift;
    shift += 7;
  } while (byte & 0x80);

  if (sectionId === 2) {
    hasImportSection = true;
    let importCount = 0;
    let byte = wasm[pos++];
    importCount |= (byte & 0x7f);
    if (byte & 0x80) {
      byte = wasm[pos++];
      importCount |= (byte & 0x7f) << 7;
    }
    console.log('Imports: ' + importCount);
    if (importCount !== 0) {
      process.exit(1);
    }
    break;
  }
  pos += size;
}
if (!hasImportSection) {
  console.log('Imports: 0');
}
" || exit 1

#!/usr/bin/env bash
# EQL Grimoire — run it. Everything stays on this machine; the only listener is 127.0.0.1.
set -e
cd "$(dirname "$0")"

command -v cargo >/dev/null || { echo "Rust is not on PATH — https://rustup.rs"; exit 1; }
[ -f web/corpus.grim ] || {
  echo "web/corpus.grim is missing. Cut one with:"
  echo "  cargo run --release -p grimoire-forge -- corpus web/corpus.grim --from data"
  exit 1
}

echo "  Building..."
cargo build --release -p grimoire-forge

URL=http://127.0.0.1:8787/app.html
echo
echo "  EQL Grimoire is at  $URL"
echo "  Ctrl-C to stop."
echo
( sleep 2; (xdg-open "$URL" || open "$URL") >/dev/null 2>&1 || true ) &
exec target/release/grimoire serve --root web --corpus web/corpus.grim

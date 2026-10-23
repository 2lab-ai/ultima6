#!/usr/bin/env bash
# Builds the wasm engine twice (spec 13 Decision 2): the Original build into site/pkg and the
# Wide build (`--features wide`) into site/pkg-wide, each wasm-bindgen --target web output. The
# page loads one of them at boot (site/app.js). Prints the size of each wasm.
# Honours CARGO_TARGET_DIR (default: <repo>/target).
set -euo pipefail

# One build: cargo with the extra arguments given, then wasm-bindgen into `out`.
build() {
  local out="$1" bin target
  shift
  # `rustup run` does not put the toolchain first on PATH, so cargo would invoke
  # whatever rustc PATH has (Homebrew's cannot target wasm). Pin it explicitly.
  bin="$(dirname "$(rustup which --toolchain stable rustc)")"
  target="${CARGO_TARGET_DIR:-$PWD/target}"
  PATH="$bin:$PATH" rustup run stable cargo build -p u6-web --release \
    --target wasm32-unknown-unknown "$@"
  "$HOME/.cargo/bin/wasm-bindgen" --target web --no-typescript --out-dir "$out" \
    "$target/wasm32-unknown-unknown/release/u6_web.wasm"
  echo "$out/u6_web_bg.wasm: $(wc -c < "$out/u6_web_bg.wasm" | tr -d ' ') bytes"
}

main() {
  cd "$(dirname "$0")/.."
  build site/pkg
  build site/pkg-wide --features wide
}

main "$@"

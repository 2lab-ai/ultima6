#!/usr/bin/env bash
# Builds the wasm engine into site/pkg (wasm-bindgen --target web output).
set -euo pipefail

main() {
  cd "$(dirname "$0")/.."
  # `rustup run` does not put the toolchain first on PATH, so cargo would invoke
  # whatever rustc PATH has (Homebrew's cannot target wasm). Pin it explicitly.
  local bin
  bin="$(dirname "$(rustup which --toolchain stable rustc)")"
  PATH="$bin:$PATH" rustup run stable cargo build -p u6-web --release \
    --target wasm32-unknown-unknown
  "$HOME/.cargo/bin/wasm-bindgen" --target web --no-typescript --out-dir site/pkg \
    target/wasm32-unknown-unknown/release/u6_web.wasm
  echo "site/pkg/u6_web_bg.wasm: $(wc -c < site/pkg/u6_web_bg.wasm | tr -d ' ') bytes"
}

main "$@"

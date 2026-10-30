#!/usr/bin/env bash
# Builds the wasm engine twice (spec 13 Decision 2): the Original build into site/pkg and the
# Wide build (`--features wide`) into site/pkg-wide, each wasm-bindgen --target web output. The
# page loads one of them at boot (site/app.js). Prints the size of each wasm.
# Honours CARGO_TARGET_DIR (default: <repo>/target).
set -euo pipefail

# Keep Cargo's encoded-flags precedence (including an explicitly empty value).
# Plain RUSTFLAGS are whitespace-separated, without shell quoting or globbing.
# Encode the added arguments so a home or checkout containing spaces stays one path.
release_rustflags() (
  set -f
  local flags="${CARGO_ENCODED_RUSTFLAGS-}" flag sep=$'\x1f'
  local IFS=$' \t\n\r\v\f'
  if [[ ! ${CARGO_ENCODED_RUSTFLAGS+x} ]]; then
    for flag in ${RUSTFLAGS-}; do
      flags="${flags:+$flags$sep}$flag"
    done
  fi
  printf '%s' "${flags:+$flags$sep}--remap-path-prefix=$HOME=/user${sep}--remap-path-prefix=$PWD=/workspace"
)

# One build: cargo with the extra arguments given, then wasm-bindgen into `out`.
build() {
  local out="$1" bin target status=0 size
  shift
  # `rustup run` does not put the toolchain first on PATH, so cargo would invoke
  # whatever rustc PATH has (Homebrew's cannot target wasm). Pin it explicitly.
  bin="$(dirname "$(rustup which --toolchain stable rustc)")"
  target="${CARGO_TARGET_DIR:-$PWD/target}"
  PATH="$bin:$PATH" rustup run stable cargo build -p u6-web --release \
    --target wasm32-unknown-unknown "$@"
  "$HOME/.cargo/bin/wasm-bindgen" --target web --no-typescript --out-dir "$out" \
    "$target/wasm32-unknown-unknown/release/u6_web.wasm"
  LC_ALL=C grep -aFq -e "$HOME/" -e "$PWD/" "$out/u6_web_bg.wasm" 2>/dev/null || status=$?
  case "$status" in
    0)
      echo "error: release wasm contains an unremapped private build path" >&2
      return 1 ;;
    1) ;;
    *)
      echo "error: could not inspect release wasm for private build paths" >&2
      return 1 ;;
  esac
  if ! size="$(wc -c 2>/dev/null < "$out/u6_web_bg.wasm")"; then
    echo "error: could not read release wasm size" >&2
    return 1
  fi
  echo "$out/u6_web_bg.wasm: ${size//[[:space:]]/} bytes"
}

main() {
  cd "$(dirname "$0")/.."
  CARGO_ENCODED_RUSTFLAGS="$(release_rustflags)"
  export CARGO_ENCODED_RUSTFLAGS
  build site/pkg
  build site/pkg-wide --features wide
}

main "$@"

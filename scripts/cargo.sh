#!/usr/bin/env bash
# Runs cargo with the rustup stable toolchain first on PATH (Homebrew rustc
# would otherwise shadow it and cannot target wasm). Gate = scripts/cargo.sh <args>.
set -euo pipefail
bin="$(dirname "$(rustup which --toolchain stable rustc)")"
PATH="$bin:$PATH" exec "$bin/cargo" "$@"

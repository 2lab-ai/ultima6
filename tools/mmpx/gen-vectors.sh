#!/usr/bin/env bash
# Regenerates crates/u6-render/src/hd/mmpx-vectors.txt: synthetic images (vectors.cpp) run
# through the MMPX reference C++ of McGuire & Gagiu 2021 (MIT), the MMPXAlgorithm class of
# code/source/cppPerf.cpp in the paper's supplement. The supplement is fetched into a scratch
# directory (never into the repo), checked against its digest, and only that class is compiled.
#
#   bash tools/mmpx/gen-vectors.sh [scratch-dir]
set -euo pipefail

URL='https://casual-effects.com/research/McGuire2021PixelArt/McGuire2021PixelArt-supplement.zip'
ZIP_SHA256='cbd852c39f545d4142f648b6b9ff5fb9dce07dd6c63236b3d0bd090d2940a4ce'
CPP_SHA256='c391ea044b5cad4edd2e7dac3fa3e425c1ee3f5badeda2008292581f42fdfeb6'

tmp=''

main() {
  local here repo work
  here="$(cd "$(dirname "$0")" && pwd)"
  repo="$(cd "$here/../.." && pwd)"
  work="${1:-$(mktemp -d)}"
  mkdir -p "$work/supplement" "$work/build"
  curl -sSfL -o "$work/supplement/supplement.zip" "$URL"
  echo "$ZIP_SHA256  $work/supplement/supplement.zip" | shasum -a 256 -c -
  unzip -q -o "$work/supplement/supplement.zip" 'code/source/cppPerf.cpp' -d "$work/supplement"
  echo "$CPP_SHA256  $work/supplement/code/source/cppPerf.cpp" | shasum -a 256 -c -
  # The class from its declaration to its closing line, verbatim.
  awk '/^class MMPXAlgorithm/,/^}; \/\/ MMPX$/' "$work/supplement/code/source/cppPerf.cpp" \
    > "$work/build/mmpx_ref_body.inc"
  cp "$here/vectors.cpp" "$work/build/vectors.cpp"
  c++ -std=c++17 -O2 -Wall -o "$work/build/vectors" "$work/build/vectors.cpp"
  # Written beside the fixture and renamed over it only when the generator exited 0: a failed
  # run leaves the fixture as it was.
  local out="$repo/crates/u6-render/src/hd/mmpx-vectors.txt"
  tmp="$out.tmp.$$"
  trap 'rm -f "$tmp"' EXIT
  "$work/build/vectors" > "$tmp"
  mv -f "$tmp" "$out"
  echo "wrote crates/u6-render/src/hd/mmpx-vectors.txt"
}

main "$@"

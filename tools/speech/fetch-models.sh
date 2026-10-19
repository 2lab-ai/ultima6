#!/usr/bin/env bash
# Hosts the voice model (spec 09 Design 2) under site/models/ (gitignored: scripts/deploy-web.sh
# uploads it with the rest of site/):
#
#   site/models/supertonic3/<revision>/   the Supertonic-3 files of one pinned Hugging Face
#                                         revision (OpenRAIL-M)
#   site/models/onnxruntime-web/1.30.0/   the three onnxruntime-web files the worker runs (MIT)
#
# Every file is checked against its sha256 pin below before it is hosted. A file larger than
# PART_BYTES (24 MiB: Cloudflare Pages serves no file over 25 MiB) is cut into parts
# `<name>.part<k>`, joined in order they are the file unchanged; a smaller one is hosted whole
# under its own name. Each directory gets manifest.json (every file's size and sha256 and each
# part's), LICENSE.txt and NOTICE (the use restrictions, the note on parts, the hashes; also as
# NOTICE.txt, the copy the page links, served as text). No game data is involved.
#
#   tools/speech/fetch-models.sh [--from DIR] [--ort-from DIR]
#       download the pinned files (or copy them from DIR, a Supertonic-3 snapshot with onnx/,
#       voice_styles/ and LICENSE, and from DIR with the three ort files), check, host
#   tools/speech/fetch-models.sh --check
#       check site/models as hosted: every file at most PART_BYTES, every part's and every
#       file's hash against the manifest and the pins, LICENSE.txt and NOTICE present
set -euo pipefail

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
MODELS="$REPO/site/models"
PART_BYTES=25165824

MODEL_REPO="Supertone/supertonic-3"
REVISION="724fb5abbf5502583fb520898d45929e62f02c0b"
MODEL_URL="https://huggingface.co/$MODEL_REPO/resolve/$REVISION"
MODEL_DIR="$MODELS/supertonic3/$REVISION"
# path  sha256  size  (the onnx/ ones are the revision's LFS pins; all were checked on the
# revision's tree)
MODEL_PINS="
onnx/duration_predictor.onnx c3eb91414d5ff8a7a239b7fe9e34e7e2bf8a8140d8375ffb14718b1c639325db 3700147
onnx/text_encoder.onnx c7befd5ea8c3119769e8a6c1486c4edc6a3bc8365c67621c881bbb774b9902ff 36416150
onnx/vector_estimator.onnx 883ac868ea0275ef0e991524dc64f16b3c0376efd7c320af6b53f5b780d7c61c 256534781
onnx/vocoder.onnx 085de76dd8e8d5836d6ca66826601f615939218f90e519f70ee8a36ed2a4c4ba 101424195
onnx/tts.json 42078d3aef1cd43ab43021f3c54f47d2d75ceb4e75f627f118890128b06a0d09 8253
onnx/unicode_indexer.json 9bf7346e43883a81f8645c81224f786d43c5b57f3641f6e7671a7d6c493cb24f 277676
voice_styles/F1.json bbdec6ee00231c2c742ad05483df5334cab3b52fda3ba38e6a07059c4563dbc2 292046
voice_styles/F2.json 7c722c6a72707b1a77f035d67f0d1351ba187738e06f7683e8c72b1df3477fc6 292423
voice_styles/F3.json 12f6ef2573baa2defa1128069cb59f203e3ab67c92af77b42df8a0e3a2f7c6ab 290794
voice_styles/F4.json c2fa764c1225a76dfc3e2c73e8aa4f70d9ee48793860eb34c295fff01c2e032b 291808
voice_styles/F5.json 45966e73316415626cf41a7d1c6f3b4c70dbc1ba2bee5c1978ef0ce33244fc8d 291479
voice_styles/M1.json e35604687f5d23694b8e91593a93eec0e4eca6c0b02bb8ed69139ab2ea6b0a5b 291748
voice_styles/M2.json b76cbf62bac707c710cf0ae5aba5e31eea1a6339a9734bfae33ab98499534a50 292055
voice_styles/M3.json ea1ac35ccb91b0d7ecad533a2fbd0eec10c91513d8951e3b25fbba99954e159b 290198
voice_styles/M4.json ca8eefad4fcd989c9379032ff3e50738adc547eeb5e221b82593a6d7b3bac303 291522
voice_styles/M5.json dd22b92740314321f8ae11c5e87f8dd60d060f15dd3a632b5adf77f471f77af2 291469
"
MODEL_LICENSE_SHA256="0d944a9110fed9a9602d60e0423a272903e7bd21ab060490774efc77c2275e9f"

ORT_VERSION="1.30.0"
ORT_URL="https://cdn.jsdelivr.net/npm/onnxruntime-web@$ORT_VERSION/dist"
ORT_DIR="$MODELS/onnxruntime-web/$ORT_VERSION"
ORT_PINS="
ort.webgpu.min.mjs 3dffff71811bc13a3a3d9591c57ca5ee5735b8f5f3eaf789510b02bb040ae3a0 66349
ort-wasm-simd-threaded.asyncify.mjs 3d1c85995364bb643302fc6fd877a0c3ba5ae72401815e0f24828a53d9191e28 53057
ort-wasm-simd-threaded.asyncify.wasm 39f9f0894d478800487ed9f7dbe92618498db320cf55c8e3d89adff8dce658da 26781914
"
# ONNX Runtime's LICENSE at tag v1.30.0 (the npm package ships none), kept beside this script.
ORT_LICENSE="$REPO/tools/speech/onnxruntime-LICENSE.txt"
ORT_LICENSE_SHA256="2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c"

sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
size() { wc -c < "$1" | tr -d ' '; }
fail() { echo "fetch-models: $*" >&2; exit 1; }

# The use restrictions of the model's licence (its Attachment A), cut out of LICENSE.txt.
restrictions() {
  awk '/^Attachment A/{on=1} on' "$1"
}

# Checks `file` against `want` (sha256) and `bytes`.
check_pin() {
  local file="$1" want="$2" bytes="$3" label="$4"
  [ -f "$file" ] || fail "$label: missing"
  [ "$(size "$file")" = "$bytes" ] || fail "$label: size differs from the pin"
  [ "$(sha "$file")" = "$want" ] || fail "$label: sha256 differs from the pin"
}

# Fetches `url` (or copies `from` when given) to `out` and checks it against the pin.
obtain() {
  local url="$1" from="$2" out="$3" want="$4" bytes="$5" label="$6"
  mkdir -p "$(dirname "$out")"
  if [ -n "$from" ]; then
    [ -f "$from" ] || fail "$label: not in the --from directory"
    cp "$from" "$out.tmp"
  else
    curl -sSfL --retry 3 -o "$out.tmp" "$url" || fail "$label: download failed"
  fi
  check_pin "$out.tmp" "$want" "$bytes" "$label"
  mv "$out.tmp" "$out"
}

# Hosts `src` (checked) as `name` in `dir`: whole when it fits in a part, else in parts; prints
# its manifest entry.
host() {
  local dir="$1" name="$2" src="$3"
  local bytes digest parts="" k=0 part
  bytes="$(size "$src")"
  digest="$(sha "$src")"
  mkdir -p "$(dirname "$dir/$name")"
  if [ "$bytes" -le "$PART_BYTES" ]; then
    mv "$src" "$dir/$name"
    parts="{\"path\":\"$name\",\"size\":$bytes,\"sha256\":\"$digest\"}"
  else
    while [ $((k * PART_BYTES)) -lt "$bytes" ]; do
      part="$name.part$k"
      dd if="$src" of="$dir/$part" bs="$PART_BYTES" skip="$k" count=1 2>/dev/null
      [ -n "$parts" ] && parts="$parts,"
      parts="$parts{\"path\":\"$part\",\"size\":$(size "$dir/$part"),\"sha256\":\"$(sha "$dir/$part")\"}"
      k=$((k + 1))
    done
    rm -f "$src"
  fi
  printf '{"name":"%s","size":%s,"sha256":"%s","parts":[%s]}' "$name" "$bytes" "$digest" "$parts"
}

# Writes `dir`/manifest.json: `head` (JSON members) and the entries.
manifest() {
  local dir="$1" head="$2"; shift 2
  local IFS=,
  printf '{"format":"u6-voice-models/1",%s,"partBytes":%s,"files":[%s]}\n' \
    "$head" "$PART_BYTES" "$*" > "$dir/manifest.json"
}

# The NOTICE beside the parts of `dir`: `intro` lines, then the note on parts and the hashes.
notice() {
  local dir="$1" intro="$2" pins="$3"
  {
    printf '%s\n\n' "$intro"
    printf 'Served in parts: a file larger than %s bytes (24 MiB, the most the host serves in\n' "$PART_BYTES"
    printf 'one file) is cut into parts <name>.part0, <name>.part1, ... Joined in that order they\n'
    printf 'are the file unchanged; no file is modified. manifest.json lists each file with its\n'
    printf 'size and sha256 and each of its parts with theirs.\n\n'
    printf 'sha256 and size of each file:\n'
    echo "$pins" | awk 'NF == 3 { printf "  %s  %s  %s\n", $2, $3, $1 }'
  } > "$dir/NOTICE"
  # The same notice as text the page links to (a host types a file by its extension).
  cp "$dir/NOTICE" "$dir/NOTICE.txt"
}

fetch() {
  local from="" ort_from=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --from) from="${2:?--from needs a directory}"; shift 2 ;;
      --ort-from) ort_from="${2:?--ort-from needs a directory}"; shift 2 ;;
      *) fail "unknown argument: $1" ;;
    esac
  done
  local stage="$STAGE" entries=() path want bytes

  # The model: every pinned file and its LICENSE, checked, then hosted.
  obtain "$MODEL_URL/LICENSE" "${from:+$from/LICENSE}" "$stage/model/LICENSE" \
    "$MODEL_LICENSE_SHA256" 15007 "LICENSE"
  while read -r path want bytes; do
    [ -n "$path" ] || continue
    obtain "$MODEL_URL/$path" "${from:+$from/$path}" "$stage/model/$path" "$want" "$bytes" "$path"
  done <<< "$MODEL_PINS"
  rm -rf "$MODEL_DIR"
  mkdir -p "$MODEL_DIR"
  while read -r path want bytes; do
    [ -n "$path" ] || continue
    entries+=("$(host "$MODEL_DIR" "$path" "$stage/model/$path")")
  done <<< "$MODEL_PINS"
  manifest "$MODEL_DIR" "\"name\":\"supertonic-3\",\"source\":\"https://huggingface.co/$MODEL_REPO\",\"revision\":\"$REVISION\",\"licence\":\"OpenRAIL-M\"" "${entries[@]}"
  cp "$stage/model/LICENSE" "$MODEL_DIR/LICENSE.txt"
  notice "$MODEL_DIR" "Supertonic 3 by Supertone Inc.: the model's files, unmodified.
Source: https://huggingface.co/$MODEL_REPO at revision $REVISION.
Licence: the BigScience Open RAIL-M License (LICENSE.txt beside this file, the revision's own).
Hosting these files is distribution under that licence: the use restrictions of its paragraph 5
(its Attachment A, below) apply to every use of the model and of what it generates, and the
page asks a visitor to accept them before it downloads the model. Speech the page plays with
these files is generated on the visitor's device by this model; the page says so while voices
are on (\"voice generated on this device\").

$(restrictions "$MODEL_DIR/LICENSE.txt")" "$MODEL_PINS"

  # The runtime: the three files the worker loads, checked, then hosted.
  entries=()
  while read -r path want bytes; do
    [ -n "$path" ] || continue
    obtain "$ORT_URL/$path" "${ort_from:+$ort_from/$path}" "$stage/ort/$path" "$want" "$bytes" "$path"
  done <<< "$ORT_PINS"
  check_pin "$ORT_LICENSE" "$ORT_LICENSE_SHA256" "$(size "$ORT_LICENSE")" "onnxruntime LICENSE"
  rm -rf "$ORT_DIR"
  mkdir -p "$ORT_DIR"
  while read -r path want bytes; do
    [ -n "$path" ] || continue
    entries+=("$(host "$ORT_DIR" "$path" "$stage/ort/$path")")
  done <<< "$ORT_PINS"
  manifest "$ORT_DIR" "\"name\":\"onnxruntime-web\",\"source\":\"https://www.npmjs.com/package/onnxruntime-web\",\"revision\":\"$ORT_VERSION\",\"licence\":\"MIT\"" "${entries[@]}"
  cp "$ORT_LICENSE" "$ORT_DIR/LICENSE.txt"
  notice "$ORT_DIR" "ONNX Runtime Web $ORT_VERSION by Microsoft Corporation: three files of its npm package's
dist/, unmodified. Licence: MIT (LICENSE.txt beside this file)." "$ORT_PINS"

  for dir in "$MODEL_DIR" "$ORT_DIR"; do
    # Older revisions and versions are not served any more.
    for old in "$(dirname "$dir")"/*; do
      [ "$old" = "$dir" ] || rm -rf "$old"
    done
  done
  check
}

# Checks one hosted directory against its manifest and `pins`.
check_dir() {
  local dir="$1" pins="$2" label="$3"
  [ -f "$dir/manifest.json" ] || fail "$label: no manifest.json"
  [ -f "$dir/LICENSE.txt" ] && [ -f "$dir/NOTICE" ] && cmp -s "$dir/NOTICE" "$dir/NOTICE.txt" \
    || fail "$label: LICENSE.txt, NOTICE or its copy NOTICE.txt missing"
  local path want bytes rows joined
  while read -r path want bytes; do
    [ -n "$path" ] || continue
    rows="$(python3 -I -c '
import json, sys
m = json.load(open(sys.argv[1]))
f = [f for f in m["files"] if f["name"] == sys.argv[2]]
if len(f) != 1: sys.exit(1)
f = f[0]
print(f["size"], f["sha256"])
for p in f["parts"]: print(p["path"], p["size"], p["sha256"])
' "$dir/manifest.json" "$path")" || fail "$label: $path not in the manifest"
    [ "$(echo "$rows" | head -1)" = "$bytes $want" ] || fail "$label: $path: the manifest differs from the pin"
    joined="$STAGE/joined"
    : > "$joined"
    while read -r part psize psha; do
      [ -f "$dir/$part" ] || fail "$label: $part missing"
      [ "$(size "$dir/$part")" = "$psize" ] || fail "$label: $part: size"
      [ "$psize" -le "$PART_BYTES" ] || fail "$label: $part: over $PART_BYTES bytes"
      [ "$(sha "$dir/$part")" = "$psha" ] || fail "$label: $part: sha256"
      cat "$dir/$part" >> "$joined"
    done <<< "$(echo "$rows" | tail -n +2)"
    check_pin "$joined" "$want" "$bytes" "$label: $path (its parts joined)"
    rm -f "$joined"
  done <<< "$pins"
}

check() {
  check_dir "$MODEL_DIR" "$MODEL_PINS" "supertonic3/$REVISION"
  check_dir "$ORT_DIR" "$ORT_PINS" "onnxruntime-web/$ORT_VERSION"
  local over
  over="$(find "$MODELS" -type f -size +"$PART_BYTES"c | wc -l | tr -d ' ')"
  [ "$over" = 0 ] || fail "$over file(s) under site/models are over $PART_BYTES bytes"
  echo "fetch-models: site/models OK: $(find "$MODELS" -type f | wc -l | tr -d ' ') files, each at most $PART_BYTES bytes; every part and file matches its manifest and pin"
}

main() {
  # Scratch for the files being checked; never inside the repository.
  STAGE="$(mktemp -d "${TMPDIR:-/tmp}/u6-voice-models.XXXXXX")"
  trap 'rm -rf "$STAGE"' EXIT
  if [ "${1:-}" = "--check" ]; then
    check
  else
    fetch "$@"
  fi
}

main "$@"

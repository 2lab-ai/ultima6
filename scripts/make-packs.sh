#!/usr/bin/env bash
# Makes the packs the page loads from your own copy of the game (README "Packs from your own
# copy"): the Korean pack (tools/i18n/translate.mjs), the NPC voice pack (tools/speech/profile.mjs)
# and the HD pack (u6-hd, tools/hd), after u6-export (tools/export) has exported the game strings
# and the NPC bundles the first two read, as the page's downloads hold them. Everything is
# written under DIR, outside any git work tree: DIR/work/ (the exports and each tool's work) and
# DIR/packs/ (the packs and index.json in the site's listing form), both 0700.
#
# A tool sends the game's text (ko, voices) or art (hd) to its model service only with your yes:
# translate.mjs and profile.mjs ask for a typed `yes` (--send answers it), u6-hd sends only with
# --send, and --dry-run sends nothing (the node tools write their first request, u6-hd counts its
# sheets). Before each such step this prints where the tool sends. A lane whose pack is in
# DIR/packs/ is skipped unless --rebuild; the run stops at the first step that fails, naming it.
set -euo pipefail

USAGE='usage: scripts/make-packs.sh --game PATH --out DIR [--only hd,ko,voices] [--send | --dry-run]
                             [--rebuild] [--exclude FILE] [--llmux URL] [--model NAME]'
NODE_MIN=22
BUNDLES=ultima6-npc-bundles.json
STRINGS=ultima6-strings.json

# The address, provider and model a node tool names for its requests: its own defaults, or
# --llmux and --model as given (read from the tool's DEFAULTS and profile.mjs's providerOf).
# shellcheck disable=SC2016 # JavaScript: its ${} are the template literal's.
ROUTE_JS='
import { pathToFileURL } from "node:url";
const { DEFAULTS } = await import(pathToFileURL(process.env.MP_TOOL));
const { providerOf } = await import(pathToFileURL(process.env.MP_PROFILE));
const model = process.env.MP_MODEL || DEFAULTS.model;
process.stdout.write(`llmux at ${process.env.MP_LLMUX || DEFAULTS.llmux}, then ${providerOf(model)} (model ${model})`);
'

say() { printf 'make-packs: %s\n' "$*"; }

usage_error() {
  printf 'make-packs: %s\n%s\n' "$1" "$USAGE" >&2
  exit 2
}

# Ends the run at step $1, for reason $2.
fail() {
  printf 'make-packs: step "%s" failed: %s\n' "$1" "$2" >&2
  exit 1
}

# Runs step $1: the command after it; the run ends there when it fails.
step() {
  local name="$1" rc=0
  shift
  say "== $name"
  "$@" || rc=$?
  [ "$rc" -eq 0 ] || fail "$name" "exit $rc"
}

# An existing path, absolute and with its links resolved (u6-hd refuses a path with ..).
resolved() {
  local p="$1" dir base
  if [ -d "$p" ]; then
    (cd -- "$p" && pwd -P)
    return
  fi
  case "$p" in
    */*) dir="${p%/*}" base="${p##*/}" ;;
    *) dir=. base="$p" ;;
  esac
  printf '%s/%s\n' "$(cd -- "${dir:-/}" && pwd -P)" "$base"
}

# Whether the directory $1 (resolved) lies inside a git work tree.
inside_git() {
  local d="$1"
  while :; do
    if [ -e "$d/.git" ]; then return 0; fi
    if [ "$d" = / ] || [ -z "$d" ]; then return 1; fi
    d="${d%/*}"
    d="${d:-/}"
  done
}

# Whether lane $1 is chosen (--only).
chosen() {
  case ",$only," in
    *",$1,"*) return 0 ;;
    *) return 1 ;;
  esac
}

# Whether lane $1 runs now (chosen, and its pack not there or --rebuild).
runs() {
  case " $todo " in
    *" $1 "*) return 0 ;;
    *) return 1 ;;
  esac
}

pack_of() {
  case "$1" in
    hd) echo hd.zip ;;
    ko) echo ko.u6lp ;;
    voices) echo voices.u6vp ;;
  esac
}

# A fresh file of our own in DIR/packs/ for the file named $1 there (made exclusively: never an
# existing name or a link).
fresh() {
  mktemp "$packs/.$1.XXXXXX"
}

# Moves the fresh file $1 onto $2, refused when $2 is a link or a directory: a rename replaces
# the name, it never writes through it.
settle() {
  local tmp="$1" dest="$2"
  if [ -L "$dest" ] || [ -d "$dest" ]; then
    rm -f -- "$tmp"
    return 1
  fi
  mv -f -- "$tmp" "$dest"
}

# DIR/packs/index.json in the site's listing form, for the packs DIR/packs/ holds.
write_index() {
  local hd='[]' ko='' voices='' tmp
  if [ -f "$packs/hd.zip" ]; then hd='[{"file":"hd.zip","label":"HD"}]'; fi
  if [ -f "$packs/ko.u6lp" ]; then ko=',"ko":{"file":"ko.u6lp","label":"Korean"}'; fi
  if [ -f "$packs/voices.u6vp" ]; then voices=',"voices":{"file":"voices.u6vp","label":"Voices"}'; fi
  tmp="$(fresh index.json)" || fail index "no file can be made in $packs"
  printf '{"packs":%s%s%s}\n' "$hd" "$ko" "$voices" > "$tmp"
  settle "$tmp" "$packs/index.json" || fail index "$packs/index.json is a link or a directory: not replaced"
}

# What a node tool's exit $2 means for lane $1: 0 done, 3 written with lines or scripts left out
# or nothing written (the tool's own lines above say which); its pack $3 goes to DIR/packs/ as
# $4 when there is one, never in a dry run.
landed() {
  local name="$1" rc="$2" made="$3" pack="$4"
  case "$rc" in
    0 | 3) ;;
    1) fail "$name" "nothing was sent" ;;
    130) fail "$name" "interrupted" ;;
    *) fail "$name" "exit $rc (the tool's line above names what)" ;;
  esac
  if [ "$dry" = 1 ]; then
    say "$name: dry run, no pack made"
    return 0
  fi
  if [ ! -f "$made" ]; then fail "$name" "no pack was written"; fi
  local tmp
  tmp="$(fresh "${pack##*/}")" || fail "$name" "no file can be made in $packs"
  if ! cp -- "$made" "$tmp"; then
    rm -f -- "$tmp"
    fail "$name" "the pack could not be copied to $packs"
  fi
  settle "$tmp" "$pack" || fail "$name" "$pack is a link or a directory: not replaced"
  write_index
  if [ "$rc" -ne 0 ]; then say "$name: made with some left out (the tool's counts above; --rebuild asks again)"; fi
  say "$name: $pack"
}

# The route of node tool $1 (a path in the repository).
route() {
  MP_TOOL="$repo/$1" MP_PROFILE="$repo/tools/speech/profile.mjs" MP_LLMUX="$llmux" MP_MODEL="$model" \
    node --input-type=module -e "$ROUTE_JS"
}

lane_ko() {
  local n where rc=0
  # Every conversation script: --first takes the scripts with lines among the first N.
  n="$(node -e 'process.stdout.write(String(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).length))' \
    "$work/$BUNDLES")" || fail ko "the bundles do not read"
  where="$(route tools/i18n/translate.mjs)" || fail ko "the tool's route does not read"
  say "== ko: translate.mjs sends the game text to $where; $consent_note"
  node tools/i18n/translate.mjs "$work/$BUNDLES" --first "$n" --strings "$work/$STRINGS" \
    --only strings,look,books,u --out "$work/ko" \
    ${consent[@]+"${consent[@]}"} ${route_args[@]+"${route_args[@]}"} || rc=$?
  landed ko "$rc" "$work/ko/ko.u6lp" "$packs/ko.u6lp"
}

lane_voices() {
  local where rc=0
  where="$(route tools/speech/profile.mjs)" || fail voices "the tool's route does not read"
  say "== voices: profile.mjs sends the NPC text to $where; $consent_note"
  node tools/speech/profile.mjs "$work/$BUNDLES" --out "$work/voice" \
    ${consent[@]+"${consent[@]}"} ${route_args[@]+"${route_args[@]}"} || rc=$?
  landed voices "$rc" "$work/voice/voices.u6vp" "$packs/voices.u6vp"
}

lane_hd() {
  local hd="$work/hd" imagegen codex_args=() exclude_args=()
  if [ -n "${U6_CODEX:-}" ]; then codex_args=(--codex "$U6_CODEX"); fi
  if [ -n "$exclude" ]; then exclude_args=(--exclude "$exclude"); fi
  step "hd inventory" "$bin/u6-hd" inventory --game "$game" --work "$hd"
  # Never atlas --rebuild: a new atlas discards every image made for the one there.
  if [ -f "$hd/jobs.jsonl" ]; then
    say "== hd atlas: kept ($hd/jobs.jsonl is there)"
  else
    step "hd atlas" "$bin/u6-hd" atlas --game "$game" --work "$hd"
  fi
  # The model u6-hd names for every call (tools/hd/src/run.rs MODEL).
  imagegen="$(sed -n 's/^pub const MODEL: &str = "\([^"]*\)";$/\1/p' tools/hd/src/run.rs)"
  imagegen="${U6_CODEX:-codex} exec -m ${imagegen:-<the model u6-hd names>}"
  if [ "$send" != 1 ]; then
    step "hd run (u6-hd sends nothing without --send)" "$bin/u6-hd" run --work "$hd" \
      ${codex_args[@]+"${codex_args[@]}"}
    if [ "$dry" = 1 ]; then
      say "hd: dry run, no pack made"
      return 0
    fi
    fail "hd run" "nothing was sent (u6-hd sends only with --send)"
  fi
  say "== hd: u6-hd run sends the game's art on its sheets to OpenAI through the ChatGPT account Codex is signed in to ($imagegen)"
  step "hd run" "$bin/u6-hd" run --work "$hd" --send ${codex_args[@]+"${codex_args[@]}"}
  step "hd ingest" "$bin/u6-hd" ingest --game "$game" --work "$hd" ${exclude_args[@]+"${exclude_args[@]}"}
  step "hd pack" "$bin/u6-hd" pack --work "$hd" --out "$packs/hd.zip"
  write_index
  say "hd: $packs/hd.zip"
}

main() {
  local game='' out='' exclude='' rebuild=0 lane
  only='ko,voices,hd' send=0 dry=0 llmux='' model=''
  while [ $# -gt 0 ]; do
    case "$1" in
      --game | --out | --only | --exclude | --llmux | --model)
        if [ $# -lt 2 ]; then usage_error "$1 needs a value"; fi
        case "$1" in
          --game) game="$2" ;;
          --out) out="$2" ;;
          --only) only="$2" ;;
          --exclude) exclude="$2" ;;
          --llmux) llmux="$2" ;;
          --model) model="$2" ;;
        esac
        shift 2
        ;;
      --send) send=1 && shift ;;
      --dry-run) dry=1 && shift ;;
      --rebuild) rebuild=1 && shift ;;
      -h | --help) printf '%s\n' "$USAGE" && exit 0 ;;
      *) usage_error "unknown argument $1" ;;
    esac
  done
  if [ -z "$game" ]; then usage_error "give --game PATH (your game files)"; fi
  if [ -z "$out" ]; then usage_error "give --out DIR"; fi
  if [ ! -e "$game" ]; then usage_error "$game: no such file or folder"; fi
  if [ -n "$exclude" ] && [ ! -f "$exclude" ]; then usage_error "$exclude: no such file"; fi
  if [ "$send" = 1 ] && [ "$dry" = 1 ]; then usage_error "--send and --dry-run exclude each other"; fi
  local lanes=()
  IFS=, read -r -a lanes <<< "$only"
  if [ ${#lanes[@]} -eq 0 ]; then usage_error "--only names hd, ko or voices"; fi
  for lane in "${lanes[@]}"; do
    case "$lane" in
      hd | ko | voices) ;;
      *) usage_error "--only names hd, ko or voices" ;;
    esac
  done
  # DIR outside every git work tree: checked on what exists of it before it is made, and again
  # once it is resolved.
  local probe git_refused="lies inside a git work tree: the packs and their work hold your game's text and art, so DIR lies outside every repository"
  case "$out" in /*) ;; *) out="$PWD/$out" ;; esac
  probe="$out"
  while [ ! -d "$probe" ]; do
    probe="${probe%/*}"
    probe="${probe:-/}"
  done
  if inside_git "$(cd -- "$probe" && pwd -P)"; then usage_error "$out $git_refused"; fi
  if [ ! -d "$out" ]; then
    mkdir -p -- "$out"
    chmod 700 "$out"
  fi
  out="$(resolved "$out")"
  if inside_git "$out"; then usage_error "$out $git_refused"; fi
  game="$(resolved "$game")"
  if [ -n "$exclude" ]; then exclude="$(resolved "$exclude")"; fi
  work="$out/work" packs="$out/packs"
  # Every directory written into, where it is there already: a directory of DIR's own (not a link)
  # and outside every git work tree; and no .git anywhere under DIR. Checked before anything is
  # built, exported or sent.
  for d in "$work" "$packs" "$work/ko" "$work/ko/preview" "$work/voice" "$work/voice/preview" \
    "$work/hd" "$work/hd/sheets" "$work/hd/codex" "$work/hd/out" "$work/hd/slices" "$work/hd/pack"; do
    if [ -L "$d" ]; then usage_error "$d is a link: the script writes only into DIR's own directories"; fi
    if [ ! -e "$d" ]; then continue; fi
    if [ ! -d "$d" ]; then usage_error "$d is not a directory"; fi
    if inside_git "$(cd -- "$d" && pwd -P)"; then usage_error "$d $git_refused"; fi
  done
  local repos
  repos="$(find "$out" -name .git -prune -print)"
  if [ -n "$repos" ]; then usage_error "$out holds a git repository (${repos%%$'\n'*}): DIR lies outside every repository"; fi
  for d in "$work" "$packs"; do
    mkdir -p -- "$d"
    chmod 700 "$d"
  done

  todo=''
  for lane in ko voices hd; do
    if ! chosen "$lane"; then continue; fi
    if [ -f "$packs/$(pack_of "$lane")" ] && [ "$rebuild" != 1 ]; then
      say "$lane: $packs/$(pack_of "$lane") is there (--rebuild makes it again)"
    else
      todo="$todo $lane"
    fi
  done
  if [ -z "$todo" ]; then
    say "nothing to make"
    return 0
  fi

  repo="$(cd -- "$(dirname -- "$0")/.." && pwd -P)"
  cd -- "$repo"
  consent=() route_args=()
  consent_note='it asks for a typed yes first (--send answers it)'
  if [ "$send" = 1 ]; then consent=(--send) consent_note='--send: it sends without asking'; fi
  if [ "$dry" = 1 ]; then consent=(--dry-run) consent_note='dry run: it writes its first request and sends nothing'; fi
  if [ -n "$llmux" ]; then route_args+=(--llmux "$llmux"); fi
  if [ -n "$model" ]; then route_args+=(--model "$model"); fi
  if runs ko || runs voices; then
    local v
    v="$(node --version 2>/dev/null)" || fail node "Node $NODE_MIN or later is needed for ko and voices"
    v="${v#v}"
    if [ "${v%%.*}" -lt "$NODE_MIN" ]; then fail node "Node $NODE_MIN or later is needed for ko and voices (this is $v)"; fi
  fi

  local pkgs=()
  if runs ko || runs voices; then pkgs+=(-p u6-export); fi
  if runs hd; then pkgs+=(-p u6-hdtool); fi
  step build bash scripts/cargo.sh build --release "${pkgs[@]}"
  bin="${CARGO_TARGET_DIR:-$repo/target}/release"
  # From here every file the steps make is the user's alone (u6-hd writes its pack 0644).
  umask 077

  if runs ko || runs voices; then
    step "export bundles" "$bin/u6-export" bundles --game "$game" --out "$work/$BUNDLES"
  fi
  if runs ko; then
    step "export strings" "$bin/u6-export" strings --game "$game" --out "$work/$STRINGS"
    lane_ko
  fi
  if runs voices; then lane_voices; fi
  if runs hd; then lane_hd; fi

  if [ "$dry" = 1 ]; then
    say "dry run: nothing was sent and no pack made; the requests the tools would send are in $work"
    return 0
  fi
  say "packs in $packs (index.json lists them): drop each on the page, or host them as the site does under /packs/"
}

main "$@"

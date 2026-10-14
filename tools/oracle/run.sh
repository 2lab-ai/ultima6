#!/usr/bin/env bash
# Ultima VI (Amiga) oracle: vAmigaWeb + AROS ROM in headless Chromium.
#
#   run.sh setup                 fetch vAmigaWeb build + node deps (idempotent)
#   run.sh build                 original ADFs -> installer under AROS -> oracle HDFs
#   run.sh boot [step ...]       boot oracle HDFs, run steps, save shots (see boot.mjs)
#   run.sh serve                 control server on 127.0.0.1:${U6_ORACLE_PORT:-8765} (ctl.sh)
#
# Outputs: $U6_ORACLE_HOME (default ~/Games/ultima6/oracle): disks/, installed/, shots/, *.log
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ORACLE_HOME="${U6_ORACLE_HOME:-/Users/zhugehyuk/Games/ultima6/oracle}"
VAW_DIR="${U6_VAW_DIR:-$ORACLE_HOME/vamigaweb}"
VAW_URL="https://github.com/vAmigaWeb/vAmigaWeb.github.io.git"
VAW_COMMIT="bd0d2e9519c81af0a032d656723d56f7c4b36d81"
export U6_ORACLE_HOME="$ORACLE_HOME" U6_VAW_DIR="$VAW_DIR"
export U6_SHOTS_DIR="${U6_SHOTS_DIR:-$ORACLE_HOME/shots}"

setup() {
  mkdir -p "$ORACLE_HOME"
  if [ ! -f "$VAW_DIR/vAmiga.wasm" ]; then
    git clone --depth 1 "$VAW_URL" "$VAW_DIR"
  fi
  local head
  head="$(git -C "$VAW_DIR" rev-parse HEAD)"
  if [ "$head" != "$VAW_COMMIT" ]; then
    echo "warning: vAmigaWeb at $head, oracle notes were taken at $VAW_COMMIT" >&2
  fi
  if [ ! -d "$HERE/node_modules/playwright" ]; then
    npm --prefix "$HERE" ci --no-audit --no-fund
  fi
  echo "setup ok: vAmigaWeb $head, $(node -p "require('$HERE/node_modules/playwright/package.json').version" 2>/dev/null || echo '?') playwright"
}

need_disks() {
  local v missing=0
  for v in PROGRAM POPULACE PLAYER PORTRAIT; do
    [ -f "$ORACLE_HOME/disks/$v.hdf" ] || { echo "missing $ORACLE_HOME/disks/$v.hdf" >&2; missing=1; }
  done
  if [ "$missing" -ne 0 ]; then
    echo "build them first: $0 build" >&2
    return 1
  fi
}

main() {
  local cmd="${1:-boot}"
  [ "$#" -gt 0 ] && shift
  case "$cmd" in
    setup) setup ;;
    build) setup >/dev/null; node "$HERE/build.mjs" ;;
    boot)
      setup >/dev/null
      need_disks
      local out="$U6_SHOTS_DIR/boot-$(date +%Y%m%d-%H%M%S)"
      node "$HERE/boot.mjs" "$out" "$@"
      echo "shots: $out"
      ;;
    serve)
      setup >/dev/null
      if curl -s --max-time 2 "http://127.0.0.1:${U6_ORACLE_PORT:-8765}/status" >/dev/null 2>&1; then
        echo "port ${U6_ORACLE_PORT:-8765} already serves an oracle; stop it with: $HERE/ctl.sh quit" >&2
        return 1
      fi
      exec node "$HERE/server.mjs"
      ;;
    *) sed -n '2,9p' "$0" >&2; return 2 ;;
  esac
}

main "$@"

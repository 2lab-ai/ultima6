#!/usr/bin/env bash
# Send one command to a running oracle server (server.mjs).
#   ctl.sh key k=RETURN          ctl.sh type text=f            ctl.sh shot name=title modes=lores,hires
#   ctl.sh insert drive=0 path=/abs/x.adf                      ctl.sh stable frames=150
# Each extra arg is one query parameter (key=value), URL-encoded here.
set -euo pipefail

main() {
  local port="${U6_ORACLE_PORT:-8765}"
  local cmd="${1:?usage: ctl.sh <command> [key=value ...]}"
  shift
  local args=()
  local kv
  for kv in "$@"; do
    args+=(--data-urlencode "$kv")
  done
  curl -sS --fail-with-body --get --max-time "${U6_CTL_TIMEOUT:-900}" ${args[@]+"${args[@]}"} "http://127.0.0.1:${port}/${cmd}"
}

main "$@"

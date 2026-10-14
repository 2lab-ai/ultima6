#!/usr/bin/env bash
# Builds the wasm engine and publishes site/ to Cloudflare Pages project `ultima6`.
# Usage: scripts/deploy-web.sh [branch]   (main = production → https://ultima6.2lab.ai)
set -euo pipefail

main() {
  cd "$(dirname "$0")/.."
  local branch="${1:-preview}"
  bash scripts/build-web.sh
  npx --yes wrangler@4 pages deploy site --project-name ultima6 --branch "$branch" \
    --commit-hash "$(git rev-parse HEAD)" --commit-message "$(git log -1 --format=%s)" \
    --commit-dirty=false
}

main "$@"

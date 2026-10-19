#!/usr/bin/env bash
# Builds the wasm engine and publishes site/ to Cloudflare Pages project `ultima6`.
# Usage: scripts/deploy-web.sh [branch]   (main = production → https://ultima6.2lab.ai)
# The voice model (spec 09) goes up with site/ when tools/speech/fetch-models.sh has hosted it
# under site/models/ (gitignored); it is checked first, and a site/models that does not check out
# stops the deploy. Without it the page's Voices say the model could not be downloaded.
set -euo pipefail

main() {
  cd "$(dirname "$0")/.."
  local branch="${1:-preview}"
  if [ -d site/models ]; then bash tools/speech/fetch-models.sh --check; fi
  bash scripts/build-web.sh
  npx --yes wrangler@4 pages deploy site --project-name ultima6 --branch "$branch" \
    --commit-hash "$(git rev-parse HEAD)" --commit-message "$(git log -1 --format=%s)" \
    --commit-dirty=false
}

main "$@"

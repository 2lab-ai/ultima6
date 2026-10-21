#!/usr/bin/env bash
# Builds the wasm engine and publishes site/ to Cloudflare Pages project `ultima6`.
# Usage: scripts/deploy-web.sh [branch]   (main = production → https://ultima6.2lab.ai)
#        scripts/deploy-web.sh --packs-only   (fills site/packs/ as below and stops: no build,
#                                             no deploy)
# The packs the site serves under /packs/ behind the owner key (functions/_middleware.js) come
# from the private repository ultima6-deploy: first, site/packs/ (gitignored) is replaced with
# exactly that repository's packs/, fetched shallow and sparse (packs/ only) into a temporary
# directory from U6_DEPLOY_REPO (default below) at U6_DEPLOY_REF (default main: a branch, a tag
# or a full commit hash), or copied as it is from U6_DEPLOY_DIR, a local checkout, which is
# refused when packs/ there differs from its commit. packs/ must hold only files and
# directories, and its index.json must parse with every file it lists (packs[].file, ko.file,
# voices.file) a plain name present in packs/; otherwise the deploy stops and site/packs/ stays
# as it was. One line names the commit and the number of files; no file's content is printed.
# Runs in one checkout take a lock (below) before site/packs/ is touched and keep it to the end
# of the upload; a second run while it is held stops with "another deploy is running", and a
# lock left by a run that died stops it too, naming the lock to remove.
# The voice model (spec 09) goes up with site/ when tools/speech/fetch-models.sh has hosted it
# under site/models/ (gitignored); it is checked before the build, and a site/models that does
# not check out stops the deploy. Without it the page's Voices say the model could not be
# downloaded.
set -euo pipefail

DEPLOY_REPO="git@github.com:2lab-ai/ultima6-deploy.git"

# Checks packs/ (argv[1]): index.json is a JSON object, and each file it lists is a plain name
# (the page's PACK_NAME) of a file in packs/. Names a failure in one line, never the content.
CHECK_INDEX='
const fs = require("fs");
const dir = process.argv[1];
const NAME = /^\w[\w.-]*$/;
const stop = (why) => { console.error("deploy-web: packs/index.json: " + why); process.exit(1); };
let index;
try { index = JSON.parse(fs.readFileSync(dir + "/index.json", "utf8")); } catch { stop("missing or not JSON"); }
if (index === null || typeof index !== "object" || Array.isArray(index)) stop("not a JSON object");
const listed = [];
if (index.packs != null) {
  if (!Array.isArray(index.packs)) stop("packs is not a list");
  index.packs.forEach((entry, i) => listed.push(["packs[" + i + "]", entry]));
}
for (const key of ["ko", "voices"]) if (index[key] != null) listed.push([key, index[key]]);
for (const [where, entry] of listed) {
  if (typeof entry?.file !== "string") stop(where + " has no file");
  if (!NAME.test(entry.file)) stop(where + ".file is not a plain name");
  let stat = null;
  try { stat = fs.lstatSync(dir + "/" + entry.file); } catch {}
  if (!stat?.isFile()) stop(where + ".file " + entry.file + " is not a file in packs/");
}
'

fail() { echo "deploy-web: $*" >&2; exit 1; }

# The fetched repository, the new site/packs being made and the old one being replaced: removed
# on every exit, the old one put back first if the swap did not finish; then the lock is let go.
STAGE="" NEW="" OLD="" LOCK=""
cleanup() {
  if [ -n "$OLD" ] && [ -d "$OLD/packs" ] && [ ! -e "$PACKS" ]; then mv "$OLD/packs" "$PACKS"; fi
  rm -rf ${STAGE:+"$STAGE"} ${NEW:+"$NEW"} ${OLD:+"$OLD"}
  if [ -n "$LOCK" ] && [ "$(readlink "$LOCK" 2> /dev/null)" = "$$" ]; then rm -f "$LOCK"; fi
}

# One run at a time per checkout, from before site/packs/ is touched to the end of the upload.
# The lock is a symbolic link whose target is the holder's pid (made in one step, so it never
# exists without it), in this checkout's git directory: not under site/, which wrangler uploads
# whole. A run removes only the lock it made; one left by a run that died stops every later run
# until it is removed by hand (no run can tell safely that another is not taking it over).
take_lock() {
  local lock pid
  lock="$(git rev-parse --absolute-git-dir)/deploy-web.lock"
  # ln would make the link inside a directory of that name and succeed.
  if [ -d "$lock" ] && [ ! -L "$lock" ]; then fail "$lock is a directory, not a lock"; fi
  if ! ln -sn "$$" "$lock" 2> /dev/null; then
    [ -L "$lock" ] || fail "cannot make the lock $lock"
    pid="$(readlink "$lock" 2> /dev/null || true)"
    if [ -n "$pid" ] && ps -p "$pid" > /dev/null 2>&1; then
      fail "another deploy is running (pid $pid; lock $lock)"
    fi
    fail "the lock $lock is left by pid ${pid:-?}, which no longer runs: remove it if no deploy is running"
  fi
  LOCK="$lock"
}

# Replaces site/packs/ with the deploy repository's packs/, checked first.
fill_packs() {
  local src ref="${U6_DEPLOY_REF:-main}" err found commit count
  # What a run killed outright left of its swap, so that no deploy uploads it.
  rm -rf "$REPO"/site/.packs-new.* "$REPO"/site/.packs-old.*
  if [ -n "${U6_DEPLOY_DIR:-}" ]; then
    src="$U6_DEPLOY_DIR"
    [ -d "$src/packs" ] || fail "U6_DEPLOY_DIR: no packs/ in $src"
    git -C "$src" rev-parse -q --verify HEAD > /dev/null \
      || fail "U6_DEPLOY_DIR: $src is not a git checkout with a commit"
    found="$(git -C "$src" status --porcelain --ignored -- packs)"
    [ -z "$found" ] || fail "U6_DEPLOY_DIR: $src/packs has uncommitted, untracked or ignored files"
  else
    STAGE="$(mktemp -d "${TMPDIR:-/tmp}/u6-deploy-packs.XXXXXX")"
    src="$STAGE/repo"
    err="$STAGE/git.err"
    git -c init.defaultBranch=main init -q "$src"
    git -C "$src" remote add origin "${U6_DEPLOY_REPO:-$DEPLOY_REPO}"
    git -C "$src" sparse-checkout set packs
    # On a failure, git's first error line (else its last line), without any credentials a URL
    # in it carries.
    if ! { git -C "$src" fetch -q --depth 1 --filter=blob:none origin "$ref" \
      && git -C "$src" checkout -q --detach FETCH_HEAD; } 2> "$err"; then
      fail "cannot fetch $ref of the deploy repository: $( { grep -E -m 1 '^(fatal|error|ERROR):' "$err" \
        || tail -n 1 "$err"; } | sed 's#://[^/@]*@#://#g')"
    fi
    [ -d "$src/packs" ] || fail "the deploy repository has no packs/ at $ref"
  fi
  # A link could put a file from elsewhere on this machine onto the site.
  found="$(find "$src/packs" ! -type f ! -type d)"
  [ -z "$found" ] || fail "packs/ holds something other than files and directories"
  node -e "$CHECK_INDEX" "$src/packs"
  commit="$(git -C "$src" rev-parse --short HEAD)"

  # Made beside site/packs, so that the swap is two renames.
  NEW="$(mktemp -d "$REPO/site/.packs-new.XXXXXX")"
  cp -R "$src/packs/." "$NEW/"
  find "$NEW" -type d -exec chmod 0755 {} +
  find "$NEW" -type f -exec chmod 0644 {} +
  count="$(find "$NEW" -type f | wc -l | tr -d ' ')"
  if [ -e "$PACKS" ] || [ -L "$PACKS" ]; then
    OLD="$(mktemp -d "$REPO/site/.packs-old.XXXXXX")"
    mv "$PACKS" "$OLD/packs"
  fi
  mv "$NEW" "$PACKS"
  NEW=""
  rm -rf ${STAGE:+"$STAGE"} ${OLD:+"$OLD"}
  STAGE="" OLD=""
  echo "deploy-web: site/packs/ is packs/ of ultima6-deploy at $commit: $count files"
}

main() {
  cd "$(dirname "$0")/.."
  REPO="$PWD"
  PACKS="$REPO/site/packs"
  trap cleanup EXIT
  local branch="${1:-preview}"
  case "$branch" in
    --packs-only) take_lock; fill_packs; return ;;
    -*) fail "unknown option $branch (usage: scripts/deploy-web.sh [branch] | --packs-only)" ;;
  esac
  take_lock
  fill_packs
  if [ -d site/models ]; then bash tools/speech/fetch-models.sh --check; fi
  bash scripts/build-web.sh
  npx --yes wrangler@4 pages deploy site --project-name ultima6 --branch "$branch" \
    --commit-hash "$(git rev-parse HEAD)" --commit-message "$(git log -1 --format=%s)" \
    --commit-dirty=false
}

main "$@"

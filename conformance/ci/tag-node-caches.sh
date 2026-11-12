#!/usr/bin/env bash
# A tag run's npm and Angular caches (ADR 0085 §4 item 1, rule 78): the tag reads none that a branch run can write. Run instead
# of the node-caches action on a tag, from the checkout the build uses (the kept clone on a host):
#   bash conformance/ci/tag-node-caches.sh [<angular dir>]
# - npm_config_cache becomes a new folder under the runner's per-job $RUNNER_TEMP (appended to $GITHUB_ENV), so `npm ci`
#   downloads every package and checks it against the lockfile's integrity, with no kept cache folder in between.
# - <angular dir>/.angular/cache is removed when it is there: an earlier run of node-caches in a kept checkout left it as a
#   link to the kept Angular cache. A link (a junction on Windows) is removed as a link, never through it.
set -euo pipefail
angular="${1:-}"
: "${RUNNER_TEMP:?RUNNER_TEMP is not set: run this in a CI job}"
: "${GITHUB_ENV:?GITHUB_ENV is not set: run this in a CI job}"
cache="$RUNNER_TEMP/npm-cache-tag-${GITHUB_RUN_ID:-local}"
rm -rf "$cache"
mkdir -p "$cache"
if command -v cygpath >/dev/null; then echo "npm_config_cache=$(cygpath -w "$cache")" >> "$GITHUB_ENV"; else echo "npm_config_cache=$cache" >> "$GITHUB_ENV"; fi
echo "tag-node-caches: npm $cache (new, this run only)"
[ -n "$angular" ] || exit 0
link="$angular/.angular/cache"
if [ -L "$link" ]; then
  if command -v cygpath >/dev/null; then cmd //c rmdir "$(cygpath -w "$link")" >/dev/null 2>&1 || rm -f "$link"; else rm -f "$link"; fi
  echo "tag-node-caches: removed the link $link (the kept Angular cache is not read)"
elif [ -e "$link" ]; then
  rm -rf "$link"
  echo "tag-node-caches: removed $link (the build writes a new Angular cache)"
fi
if [ -e "$link" ] || [ -L "$link" ]; then
  echo "::error::$link is still there after removing it. Next step: remove it by hand on this runner; a tag must not read a kept Angular cache."
  exit 1
fi

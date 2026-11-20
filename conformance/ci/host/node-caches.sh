#!/usr/bin/env bash
# The shell copy of `dev ci node-caches` (docs/plans/ci-in-rust.md, WP-4): npm's cache and the Angular cache between runs. Run through
# conformance/ci/kit.sh (`kit.sh host node-caches <angular-dir> <name>`); held equal to the Rust command by
# tools/paddock-dev/tests/it/host_steps.rs; deleted with the release-path lane (WP-11). Inputs: ANGULAR_DIR, NAME.
here=$(cd "$(dirname "$0")" && pwd)
set -euo pipefail
iso="$here/../kit.sh"
# shellcheck source=../safe-rm.sh
. "$here/../safe-rm.sh"
folder=$(printf '%s' "$NAME" | tr -c 'A-Za-z0-9._-' '_')
if [ "$RUNNER_OS" = Linux ]; then
  # The job's own folders (ADR 0111): the action fills and keeps them through actions/cache; the runner's shared /sccache mount is
  # never written.
  base="${RUNNER_TEMP:?RUNNER_TEMP is not set}/node-caches"
  npm_cache="$base/npm"; ng_cache="$base/angular"
  mkdir -p "$npm_cache"
else
  # The host's kept folders (job-isolation): npm's cache and the Angular cache are the job slot's own.
  npm_cache="$(bash "$iso" isolation path npm)"
  ng_cache=""
  if [ -n "$ANGULAR_DIR" ]; then ng_cache="$(bash "$iso" isolation path angular "$folder")"; fi
fi
if [ "$RUNNER_OS" = Windows ]; then echo "npm_config_cache=$(cygpath -w "$npm_cache")" >> "$GITHUB_ENV"; else echo "npm_config_cache=$npm_cache" >> "$GITHUB_ENV"; fi
echo "node-caches: npm $npm_cache"
[ -n "$ANGULAR_DIR" ] || exit 0
# The tree of a host job is the kept clone (PADDOCK_WORKSPACE); on Windows the work folder is only the bootstrap checkout.
ws="${PADDOCK_WORKSPACE:-$GITHUB_WORKSPACE}"
if [ "$RUNNER_OS" = Windows ] && command -v cygpath >/dev/null; then ws="$(cygpath -u "$ws")"; fi
mkdir -p "$ng_cache" "$ws/$ANGULAR_DIR/.angular"
# angular.json turns the cache on in CI too (cli.cache.environment "all"); its folder is a link to the kept one.
link="$ws/$ANGULAR_DIR/.angular/cache"
safe_rm "$ws/$ANGULAR_DIR/.angular" "$link"
if [ "$RUNNER_OS" = Windows ]; then cmd //c mklink //J "$(cygpath -w "$link")" "$(cygpath -w "$ng_cache")" >/dev/null; else ln -s "$ng_cache" "$link"; fi
echo "node-caches: Angular $link -> $ng_cache"

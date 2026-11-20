#!/usr/bin/env bash
# The dev-tool cache steps of the setup-rust action (.gitea/actions/setup-rust; ADR 0111): the ./dev tool binary of a Linux job through
# the Actions cache instead of a folder shared between lanes.
#
#   dev-tool-cache.sh key                 the cache key and path of this checkout's tool (outputs: usable, hash, key, path)
#   dev-tool-cache.sh verify <hit>        runs the tool (./dev rebuilds a binary that does not run or is not for its hash), checks that
#                                         it reports the expected hash, and says what happened (outputs: state hit|miss|rejected, save)
#   dev-tool-cache.sh summary <state> <saved> <verified>   the job-summary line (cache key, hit or miss, save result)
#
# The key is exactly the launcher's input hash plus the OS, the architecture and a cache-version salt (PADDOCK_DEV_TOOL_CACHE_VERSION,
# bump it to retire every saved binary), with no restore-keys: a binary of another hash is never restored. Only a build that passed
# `verify` is saved; a miss says "miss", never "hit" or "ok" (ER-33). Every problem is one ::error:: or ::warning:: line with its next
# step; the cache slows a job down, never fails it, except a tool that does not report its own hash (never saved, the job stops).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="${PADDOCK_REPO:-$(cd "$here/../.." && pwd)}"
version="${PADDOCK_DEV_TOOL_CACHE_VERSION:-1}"
dev=(bash "$repo/dev")

out() { echo "$1=$2" >> "${GITHUB_OUTPUT:-/dev/null}"; echo "dev-tool-cache: $1=$2"; }

# The folder the launcher keeps this hash in: <home>/<bin|bin-source>/<hash> (the same rule as ./dev).
cache_path() {
  local home="${PADDOCK_DEV_HOME:-${HOME:?HOME is not set}/.paddock-dev}" root=bin
  [ "${PADDOCK_DEV_DOWNLOAD:-}" = 0 ] && root=bin-source
  printf '%s\n' "$home/$root/$1"
}

cmd_key() {
  local lines hash state
  if ! lines="$("${dev[@]}" --wrapper-hash 2>/dev/null)" || [ "$(printf '%s\n' "$lines" | wc -l)" -lt 3 ]; then
    echo "::warning::dev-tool-cache: ./dev --wrapper-hash gave no hash (no Rust compiler yet?), so the tool is not cached by key. Next step: run the setup-rust action before this one."
    out usable false
    return 0
  fi
  hash="$(printf '%s\n' "$lines" | sed -n 2p)"
  state="$(printf '%s\n' "$lines" | sed -n 3p)"
  if [ "$state" != clean ] || ! [[ "$hash" =~ ^[0-9a-f]{40}$ ]]; then
    echo "::notice::dev-tool-cache: the tool's inputs have uncommitted changes (state '$state'), so no committed hash names its binary; nothing is restored or saved."
    out usable false
    return 0
  fi
  out usable true
  out hash "$hash"
  out key "dev-tool-v${version}-${RUNNER_OS:?RUNNER_OS is not set}-${RUNNER_ARCH:?RUNNER_ARCH is not set}-$hash"
  out path "$(cache_path "$hash")"
}

cmd_verify() {
  local hit="${1:-}" hash log said state save=false
  hash="$("${dev[@]}" --wrapper-hash 2>/dev/null | sed -n 2p)"
  [ -n "$hash" ] || { echo "::error::dev-tool-cache: ./dev gave no hash. Next step: run ./dev --wrapper-hash in the job to see why."; exit 1; }
  log="$(mktemp)"
  # ./dev runs the binary only when it runs and is for this hash; otherwise it downloads or builds one (and says so).
  said="$("${dev[@]}" self hash 2>"$log")" || { cat "$log" >&2; echo "::error::dev-tool-cache: ./dev self hash failed (above). Next step: ./dev cannot build the tool in this job; fix that first."; exit 1; }
  cat "$log" >&2
  if [ "$said" != "embedded $hash" ]; then
    echo "::error::dev-tool-cache: the tool reports '$said', not 'embedded $hash'. It is not saved. Next step: bump PADDOCK_DEV_TOOL_CACHE_VERSION if a restored copy keeps being wrong."
    exit 1
  fi
  if grep -qE '^dev: (building|downloaded) paddock-dev' "$log"; then
    if [ "$hit" = true ]; then
      state=rejected   # the cache had the key but its binary was not usable: ./dev rebuilt it; the key exists, so it cannot be saved again
      echo "::warning::dev-tool-cache: the binary restored under this key was rejected and rebuilt (it did not run or was not for hash $hash). The key already exists, so the good build cannot be saved under it. Next step: bump PADDOCK_DEV_TOOL_CACHE_VERSION."
    else
      state=miss
      save=true
    fi
  else
    state=hit
  fi
  rm -f "$log"
  out state "$state"
  out save "$save"
}

cmd_summary() {
  local key="${KEY:-}" state="${1:-miss}" saved="${2:-}" verified="${3:-}" line
  case "$state" in
    hit) line="hit (restored, verified for its hash)" ;;
    rejected) line="rejected (restored but not usable; rebuilt; NOT saved)" ;;
    miss)
      if [ "$saved" = true ] && [ "$verified" = true ]; then line="miss (built; saved and found again by key)"
      elif [ "$saved" = true ]; then line="miss (built; save NOT confirmed: the cache lookup after the save found nothing)"
      else line="miss (built; not saved)"; fi ;;
    *) line="not measured ($state)" ;;
  esac
  echo "- dev-tool cache \`$key\`: $line" | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"
  case "$line" in *"NOT"*) echo "::warning::dev-tool-cache: $line. Next step: check the cache server (RustFS) and the runner's cache settings." ;; esac
}

case "${1:-}" in
  key) cmd_key ;;
  verify) shift; cmd_verify "$@" ;;
  summary) shift; cmd_summary "$@" ;;
  *) echo "usage: dev-tool-cache.sh key | verify <hit> | summary <state> <saved> <verified>" >&2; exit 2 ;;
esac

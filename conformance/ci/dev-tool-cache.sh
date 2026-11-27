#!/usr/bin/env bash
# The dev-tool cache steps of the setup-rust action (.gitea/actions/setup-rust; ADR 0111): the ./dev tool binary of a Linux job through
# the Actions cache instead of a folder shared between lanes.
#
#   dev-tool-cache.sh no-download         exit 0 when this job builds ./dev from source and never downloads it (no cache then)
#   dev-tool-cache.sh key                 the cache key and path of this checkout's tool (outputs: usable, hash, key, path)
#   dev-tool-cache.sh verify <hit> <matched-key>   runs the tool (./dev rebuilds a binary that does not run or is not for its hash), checks that
#                                         it reports the expected hash, and says what happened (outputs: state hit|miss|rejected, save)
#   dev-tool-cache.sh summary <state> <saved> <verified> [<hit> <matched-key>]   the job-summary line (cache key, restore, save result)
#   dev-tool-cache.sh probe               asks the Actions cache service once per job, with a short timeout, whether it answers (outputs: down);
#                                         when it does not, PADDOCK_ACTIONS_CACHE_DOWN=1 goes to $GITHUB_ENV and every actions/cache step of the job
#                                         (this action's and node-caches') is skipped, with one ::warning:: naming why. Never fails the job
#   dev-tool-cache.sh store-restore <hash>   the runner-local store first (user decision 109): copies /sccache/paddock-dev-tools/<hash>/ into the
#                                         job's own tool folder (output: hit); `verify` still checks the copy for its hash. The store is written
#                                         by `./dev ci dev-tool-store publish` (the plan job only; tools/paddock-dev/src/ci_dev_tool_store.rs)
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

# The one definition of "this job builds ./dev from source and never downloads it" (ADR 0022 section 3.3): PADDOCK_DEV_DOWNLOAD=0 (a
# release or tag job), or no read token (the job could not download anyway). setup-rust's compile-cache step asks this same command, so
# the two never disagree. Such a job keeps its tool in bin-source, which a downloaded binary must never reach: the cache is skipped
# for it, because a binary saved by a download job under the same key would be restored there and run as if built from source.
cmd_no_download() { [ "${PADDOCK_DEV_DOWNLOAD:-}" = 0 ] || [ -z "${PADDOCK_DEV_TOKEN-}${GITHUB_TOKEN-}" ]; }

# The folder the launcher keeps this hash in for a job that may download: <home>/bin/<hash>.
cache_path() {
  printf '%s\n' "${PADDOCK_DEV_HOME:-${HOME:?HOME is not set}/.paddock-dev}/bin/$1"
}

# The runner-local store of the tool (a folder of the /sccache volume the lanes already mount); PADDOCK_DEV_TOOL_STORE moves it.
store_dir() { printf '%s\n' "${PADDOCK_DEV_TOOL_STORE:-/sccache/paddock-dev-tools}"; }

# The circuit breaker (user decision 109): in run 9180 the Actions cache service timed out in every call of a Rust job (restore, save and
# the confirming lookup, 5 tries each, 171 s). One cheap lookup per job, as the sccache S3 health check does (curl --max-time 5): any HTTP
# answer, a 404 or 204 included, means the service is there; no answer in time means every actions/cache step of the job is skipped. A job
# without the service's address or without curl cannot ask: it is `not measured` and the steps run as before (ER-33).
cmd_probe() {
  [ "${PADDOCK_ACTIONS_CACHE_PROBED:-}" != 1 ] || { out down "${PADDOCK_ACTIONS_CACHE_DOWN:-0}"; return 0; }
  local url="${ACTIONS_CACHE_URL:-}" max="${PADDOCK_CACHE_PROBE_SECONDS:-5}" code
  if [ -n "${GITHUB_ENV:-}" ]; then echo "PADDOCK_ACTIONS_CACHE_PROBED=1" >> "$GITHUB_ENV"; fi
  if [ -z "$url" ] || ! command -v curl >/dev/null 2>&1; then
    echo "::notice::dev-tool-cache: the Actions cache service was not probed (no ACTIONS_CACHE_URL or no curl in this job); its steps run as they are. Not measured."
    out down false
    return 0
  fi
  # A lookup of a key that does not exist: the service answers 204 (or an error status); only no answer in time is a down service.
  code="$(curl -s -o /dev/null -m "$max" -w '%{http_code}' "${url%/}/_apis/artifactcache/cache?keys=paddock-probe&version=paddock-probe" 2>/dev/null)" || code=000
  if [ -z "$code" ] || [ "$code" = 000 ]; then
    echo "::warning::dev-tool-cache: the Actions cache service ${url} did not answer within ${max} s, so every actions/cache step of this job is skipped (the ./dev tool comes from the runner store or is built, npm and Angular caches start cold). Next step: check the runner's cache server (act_runner cache, RustFS); the job goes on."
    if [ -n "${GITHUB_ENV:-}" ]; then echo "PADDOCK_ACTIONS_CACHE_DOWN=1" >> "$GITHUB_ENV"; fi
    echo "- Actions cache service: NOT answering within ${max} s; the job's cache steps were skipped" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
    out down true
  else
    echo "dev-tool-cache: the Actions cache service answered (HTTP $code)."
    out down false
  fi
}

cmd_store_restore() {
  local hash="${1:-}" store dest tmp="" t0="$SECONDS"
  [[ "$hash" =~ ^[0-9a-f]{40}$ ]] || { echo "::error::dev-tool-cache: store-restore needs the 40-hex input hash, got '$hash'. Next step: pass the key step's hash output."; exit 1; }
  store="$(store_dir)/$hash"
  dest="$(cache_path "$hash")"
  if [ ! -d "$store" ]; then
    echo "dev-tool-cache: the runner store has no entry for $hash ($(store_dir)); the Actions cache is next."
    out hit false
    return 0
  fi
  # A folder this step made beside the destination (same volume, so the final move is a rename); `${tmp:?}` never expands to nothing.
  if ! { mkdir -p "$(dirname "$dest")" && tmp="$(mktemp -d "$(dirname "$dest")/.store-$hash.XXXXXX")" && cp -R "$store/." "${tmp:?}" && [ -n "$(ls -A "${tmp:?}")" ]; }; then
    [ -z "${tmp:-}" ] || rm -rf "${tmp:?}"
    echo "::warning::dev-tool-cache: the runner store entry $store could not be copied. Next step: the Actions cache or a build is used; check the /sccache volume."
    out hit false
    return 0
  fi
  # Our own copy replaces nothing running: an entry another step of this job made is only the same hash, as good as this one.
  if [ -e "$dest" ]; then rm -rf "${tmp:?}"; else mv "${tmp:?}" "$dest"; fi
  touch "$store" 2>/dev/null || true   # a use, for the store's eviction order
  echo "dev-tool-cache: restored $hash from the runner store in $((SECONDS - t0)) s."
  out hit true
}

cmd_key() {
  local lines hash state
  if cmd_no_download || [ "${PADDOCK_DEV_FRESH:-}" = 1 ]; then
    echo "::notice::dev-tool-cache: this job builds ./dev from source (PADDOCK_DEV_DOWNLOAD=0, no read token or PADDOCK_DEV_FRESH=1), so no cached binary is restored or saved: a binary downloaded by another job must never run as source-built (ADR 0022 section 3.3). Not measured."
    out usable false
    echo "- dev-tool cache: not used (this job builds ./dev from source: PADDOCK_DEV_DOWNLOAD=0, or no read token in this step's environment, or PADDOCK_DEV_FRESH=1)" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
    return 0
  fi
  # `./dev --wrapper-hash` prints the hash and `clean` or `dirty` on standard output (two lines; the inputs line is on standard error).
  # Reading line 2 and 3 here once made every Linux job miss the cache without a word (run 8509). A shape that is not exactly that is
  # an error, never a quiet miss (ER-33): the tool then builds in every job and the cost is invisible.
  if ! lines="$("${dev[@]}" --wrapper-hash 2>/dev/null)"; then
    echo "::error::dev-tool-cache: ./dev --wrapper-hash failed (no Rust compiler runs here?). Next step: run the setup-rust action's toolchain step before this one, then ./dev --wrapper-hash in the job to see why."
    exit 1
  fi
  hash="$(printf '%s\n' "$lines" | sed -n 1p)"
  state="$(printf '%s\n' "$lines" | sed -n 2p)"
  if [ "$(printf '%s\n' "$lines" | wc -l)" -ne 2 ] || ! [[ "$hash" =~ ^[0-9a-f]{40}$ ]] || { [ "$state" != clean ] && [ "$state" != dirty ]; }; then
    echo "::error::dev-tool-cache: ./dev --wrapper-hash printed an unexpected shape (want a 40-hex hash, then clean or dirty). Next step: compare the wrapper's --wrapper-hash output with conformance/ci/dev-tool-cache.sh."
    exit 1
  fi
  if [ "$state" != clean ]; then
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
  # <cache-hit> and <matched-key> are the restore step's own outputs (ADR 0115): the state is what the restore found, never what this
  # run of ./dev printed. An earlier step of the job may have built the tool already (run 8576), and a log of "no build" then said "hit".
  local hit="${1:-}" matched="${2:-}" hash log said embedded state save=false outcome
  outcome="$(bash "$here/cache-report.sh" outcome "$hit" "$matched")"
  hash="$("${dev[@]}" --wrapper-hash 2>/dev/null | sed -n 1p)"
  [[ "$hash" =~ ^[0-9a-f]{40}$ ]] || { echo "::error::dev-tool-cache: ./dev gave no hash. Next step: run ./dev --wrapper-hash in the job to see why."; exit 1; }
  log="$(mktemp)"
  # ./dev runs the binary only when it runs and is for this hash; otherwise it downloads or builds one (and says so).
  said="$("${dev[@]}" self hash --json 2>"$log")" || { cat "$log" >&2; echo "::error::dev-tool-cache: ./dev self hash failed (above). Next step: ./dev cannot build the tool in this job; fix that first."; exit 1; }
  cat "$log" >&2
  # `dev self hash` has a text form of several lines (hash, wrapper, embedded, dirty) and a JSON form; the JSON field is its contract
  # (tools/paddock-dev/src/input_hash.rs), so it is read by that key (no node or jq is needed on the runner), never matched in the text (ER-42; the text shape grew and broke this, run 8560).
  embedded="$(printf '%s' "$said" | sed -n 's/.*"embedded":"\([0-9a-f]*\)".*/\1/p')"
  [ -n "$embedded" ] || embedded=-   # null: a binary built outside the launcher
  if [ "$embedded" != "$hash" ]; then
    echo "::error::dev-tool-cache: the tool embeds '$embedded', not '$hash'. It is not saved. Next step: bump PADDOCK_DEV_TOOL_CACHE_VERSION if a restored copy keeps being wrong."
    exit 1
  fi
  case "$outcome" in
    exact)
      state=hit
      # A restored binary that ./dev had to rebuild is a restore that did not work. This reads the log only to warn; no save follows
      # either way (the key exists, so it cannot be saved again).
      if grep -qE '^dev: (building|downloaded) paddock-dev' "$log"; then
        state=rejected
        echo "::warning::dev-tool-cache: the binary restored under this key was rejected and rebuilt (it did not run or was not for hash $hash). The key already exists, so the good build cannot be saved under it. Next step: bump PADDOCK_DEV_TOOL_CACHE_VERSION."
      fi ;;
    *)
      # Nothing restored under the exact key (there are no restore-keys, so partial cannot happen): whatever ran here, built in this step
      # or earlier in the job, was verified above for its hash, and the key is new, so it is saved.
      state=miss
      save=true ;;
  esac
  rm -f "$log"
  out state "$state"
  out save "$save"
}

cmd_summary() {
  local key="${KEY:-}" state="${1:-miss}" saved="${2:-}" verified="${3:-}" hit="${4:-}" matched="${5:-}" line
  # SAVES=false: a job that only restores (the plan job is the one writer, user decision 109); unset keeps the writer's wording.
  # SOURCE: store (the runner-local store) or empty (the Actions cache); DOWN=1: the cache service did not answer the job's probe.
  local saves="${SAVES:-true}" source="${SOURCE:-}" down="${PADDOCK_ACTIONS_CACHE_DOWN:-}" from="the Actions cache"
  [ "$source" != store ] || from="the runner store"
  case "$state" in
    hit) line="restored: exact hit from $from (verified for its hash); saved: not needed" ;;
    rejected) line="restored: exact hit from $from but NOT usable (rebuilt); saved: NO (the key exists)" ;;
    miss)
      if [ "$saves" = false ]; then
        if [ "$down" = 1 ]; then line="restored: skipped (the Actions cache service did not answer; built); saved: not by this job (only the plan job saves)"
        else line="restored: miss (built); saved: not by this job (only the plan job saves)"; fi
      elif [ "$down" = 1 ]; then line="restored: skipped (the Actions cache service did not answer; built); saved: skipped (service down)"
      elif [ "$saved" = true ] && [ "$verified" = true ]; then line="restored: miss (built); saved: yes (found again by key)"
      elif [ "$saved" = true ]; then line="restored: miss (built); saved: NOT confirmed (the cache lookup after the save found nothing)"
      else line="restored: miss (built); saved: NO"; fi ;;
    *) line="not measured ($state)" ;;
  esac
  echo "- dev-tool cache \`$key\`: $line" | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"
  # The audit line (ci cache-audit): saved is yes only when the lookup after the save found the key; n/a for a job that never saves.
  if [ -n "$key" ] && { [ "$state" = hit ] || [ "$state" = miss ] || [ "$state" = rejected ]; }; then
    local sv=n/a
    if [ "$saves" != false ]; then
      case "$state" in miss) if [ "$saved" = true ] && [ "$verified" = true ]; then sv=yes; else sv=no; fi ;; rejected) sv=no ;; esac
    fi
    local o; o="$(bash "$here/cache-report.sh" outcome "$hit" "$matched")"
    echo "cache-report: name=dev-tool outcome=$o saved=$sv key=$key"
  fi
  case "$line" in *"NOT"*) echo "::warning::dev-tool-cache: $line. Next step: check the cache server (RustFS) and the runner's cache settings." ;; esac
}

case "${1:-}" in
  key) cmd_key ;;
  no-download) cmd_no_download ;;
  verify) shift; cmd_verify "$@" ;;
  summary) shift; cmd_summary "$@" ;;
  probe) cmd_probe ;;
  store-restore) shift; cmd_store_restore "$@" ;;
  *) echo "usage: dev-tool-cache.sh probe | store-restore <hash> | key | verify <hit> <matched-key> | summary <state> <saved> <verified> [<hit> <matched-key>]" >&2; exit 2 ;;
esac

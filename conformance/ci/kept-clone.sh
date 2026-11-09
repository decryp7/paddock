#!/usr/bin/env bash
# The kept clone of a host job (.gitea/actions/kept-clone; ADR 0025, "Kept clones"). A host runner checks out into a per-job folder that
# is deleted afterwards, so every job paid for a full fetch and a write of the whole tree (a checkout and stable-workspace were 38 % of
# a Windows job's step time, 28 % of a macOS job's; docs/benchmarks.md). Instead each job slot keeps ONE git clone in its own kept
# folder (job-isolation: src/<name>-<slot>), brought to the job's exact commit by a fetch of that commit and a checkout, so only what
# changed since the slot's last job crosses the network or the disk, and a profile (checkout-profiles.txt) leaves out the paths the job
# never reads (screenshots, review sheets).
#
# The caller (ci.yml) first runs actions/checkout sparse and partial (the `bootstrap` folders: the actions, their scripts and
# paddock-dev's inputs, which job-isolation needs), at the job's commit. This script then:
#   1. resolves the slot's kept folder (job-isolation.sh path src <name>): per slot, so concurrent jobs never share a clone;
#   2. creates the clone (no checkout, blob:none, depth 1) when it is missing, broken, of another repository or larger than
#      PADDOCK_KEPT_CLONE_MAX_MIB (default 3072);
#   3. cleans it (reset --hard, clean -ffdx: no file of the last job survives), fetches the exact commit, writes the profile's sparse
#      patterns, checks the commit out and re-applies the patterns;
#   4. verifies: HEAD is the commit, the tree has no change or stray file, no excluded path exists;
#   5. exports PADDOCK_WORKSPACE (the folder later steps build from) and, on macOS (where the work folder can be replaced), leaves a
#      link at the work folder.
# Every phase is bounded and logs its duration (ER-27). A clone that cannot be made never fails the job: the work folder is expanded
# to the profile instead (one ::warning::; only the compile cache of the workspace crates is lost).
# The checkout's credentials (its http extraheader) are passed to git through the environment, never in a command line, never kept.
# Environment: RUNNER_OS, GITHUB_WORKSPACE, GITHUB_ENV, NAME (the kept folder's name), PROFILE, KEPT_CLONE_PROFILES (the profile file,
# default checkout-profiles.txt beside this script), PADDOCK_KEPT_CLONE_TIMEOUT (seconds per phase, default 900).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tool=kept-clone
if [ "${RUNNER_OS:?RUNNER_OS is not set}" = Linux ]; then exit 0; fi # Linux jobs run in a container with a fixed workspace
started=$SECONDS
profiles="${KEPT_CLONE_PROFILES:-$here/checkout-profiles.txt}"
profile="${PROFILE:?PROFILE is not set}"
limit="${PADDOCK_KEPT_CLONE_TIMEOUT:-900}"
max_mib="${PADDOCK_KEPT_CLONE_MAX_MIB:-3072}"
hint="list and end processes using the CI home's src folder on the runner (a git or an editor left from an earlier job), then re-run the job."
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
# shellcheck source=bounded.sh
. "$here/bounded.sh"
export GIT_TERMINAL_PROMPT=0

# --- the profile -------------------------------------------------------------------------------------------------------------------
excludes=()
known=0
while read -r kind prof path _; do
  if [ "$kind" = exclude ]; then
    if [ "$prof" = "$profile" ]; then excludes+=("${path%/}"); fi
    known=1
  fi
done < <(grep -v '^[[:space:]]*#' "$profiles" | grep -v '^[[:space:]]*$' || true)
if [ "$profile" != full ] && [ "${#excludes[@]}" = 0 ]; then
  echo "::error::kept-clone: profile '$profile' is not declared in $profiles (an \`exclude $profile <path>\` line). Next step: declare it there, or use 'full'." ; exit 1
fi
[ "$known" = 1 ] || true

# apply_profile <tree>: the sparse patterns of the profile (everything but the excluded paths) for the tree's git.
apply_profile() {
  local tree="$1" p
  git -C "$tree" config core.sparseCheckout true
  git -C "$tree" config core.sparseCheckoutCone false
  mkdir -p "$tree/.git/info"
  { echo '/*'; for p in ${excludes[@]+"${excludes[@]}"}; do echo "!/$p/"; done; } > "$tree/.git/info/sparse-checkout"
}

# --- what the plain checkout left ----------------------------------------------------------------------------------------------------
ws="$(cd "$GITHUB_WORKSPACE" && pwd -P)"
sha="$(git -C "$ws" rev-parse HEAD)"
url="$(git -C "$ws" remote get-url origin)"
# The checkout's credential header, passed through the environment (git >= 2.31), never on a command line.
header="$(git -C "$ws" config --local --get-regexp '^http\..*\.extraheader$' 2>/dev/null | head -n 1 | cut -d' ' -f2- || true)"
if [ -n "$header" ]; then
  export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=http.extraheader GIT_CONFIG_VALUE_0="$header"
fi

# fallback <why>: build from the work folder, expanded to the profile; never fails the job for lack of a clone.
fallback() {
  echo "::warning::kept-clone: $1; building from the work folder (the compile cache misses the workspace crates)."
  apply_profile "$ws" || true
  git -C "$ws" read-tree -mu HEAD 2>&1 || echo "::warning::kept-clone: could not expand the work folder to the profile; steps that read paths outside the bootstrap will fail."
  exit 0
}

folder="$(printf '%s' "$NAME" | tr -c 'A-Za-z0-9._-' '_')"
resolve_stable() { bash "$here/job-isolation.sh" path src "$folder"; }
if ! bounded "resolve the job slot's kept folder" resolve_stable; then
  cat "$scratch/err" >&2
  fallback "no kept folder of this job's slot (see the message above)"
fi
cat "$scratch/err" >&2
stable="$(cat "$scratch/out")"

# --- the clone ---------------------------------------------------------------------------------------------------------------------
healthy() { # exit 0 when the kept folder holds a usable clone of this repository
  [ -d "$stable/.git" ] && git -C "$stable" rev-parse --git-dir >/dev/null 2>&1 &&
    [ "$(git -C "$stable" config remote.origin.url 2>/dev/null)" = "$url" ]
}
clone_kib() { # the clone's object size in KiB
  git -C "$stable" count-objects -v 2>/dev/null | awk '/^(size|size-pack): / { k += $2 } END { printf "%d", k }'
}
mode=reused
if healthy; then
  kib="$(clone_kib)"
  if [ "$kib" -gt $((max_mib * 1024)) ]; then echo "kept-clone: the clone at $stable is $((kib / 1024)) MiB, larger than $max_mib MiB; starting again."; mode=created; fi
else
  mode=created
fi

create_clone() { rm -rf "$stable" && mkdir -p "$(dirname "$stable")" && git clone -q --no-checkout --no-tags --depth=1 --filter=blob:none "$url" "$stable"; }
if [ "$mode" = created ]; then
  if ! bounded "create the clone" create_clone; then
    cat "$scratch/err" >&2
    fallback "could not create the clone at $stable ($(tail -c 200 "$scratch/err" | tr '\r\n' '  '))"
  fi
fi

clean_clone() {
  if git -C "$stable" rev-parse --verify -q HEAD >/dev/null; then git -C "$stable" reset -q --hard HEAD; fi
  git -C "$stable" clean -q -ffdx
}
fetch_commit() {
  git -C "$stable" fetch -q --depth=1 --no-tags --filter=blob:none origin "$sha" &&
    [ "$(git -C "$stable" rev-parse FETCH_HEAD)" = "$sha" ]
}
checkout_commit() {
  apply_profile "$stable" &&
    git -C "$stable" -c advice.detachedHead=false checkout -q -f --detach "$sha" &&
    git -C "$stable" read-tree -mu HEAD &&
    git -C "$stable" clean -q -ffdx &&
    drop_excluded
}
# git clean leaves untracked files outside the sparse patterns alone (git 2.34+), so a file a job wrote into an excluded folder would
# stay for ever: the excluded paths are removed by name.
drop_excluded() {
  local p d
  for p in ${excludes[@]+"${excludes[@]}"}; do
    while IFS= read -r d; do rm -rf "$d"; done < <(compgen -G "$stable/$p" || true)
  done
}
verify_clone() {
  local head p
  head="$(git -C "$stable" rev-parse HEAD)"
  [ "$head" = "$sha" ] || { echo "HEAD is $head, not the job's commit $sha" >&2; return 1; }
  [ -z "$(git -C "$stable" status --porcelain --untracked-files=all)" ] || { echo "the tree is not clean: $(git -C "$stable" status --porcelain | head -n 3 | tr '\n' ' ')" >&2; return 1; }
  for p in ${excludes[@]+"${excludes[@]}"}; do
    if compgen -G "$stable/$p" >/dev/null; then echo "excluded path $p exists" >&2; return 1; fi
  done
}
if ! bounded "clean" clean_clone || ! bounded "fetch $sha" fetch_commit || ! bounded "check out" checkout_commit || ! bounded "verify" verify_clone; then
  cat "$scratch/err" >&2
  fallback "the clone at $stable could not be brought to $sha ($(tail -c 200 "$scratch/err" | tr '\r\n' '  '))"
fi
files="$(git -C "$stable" ls-files -v | grep -vc '^S' || true)"

# --- hand over -----------------------------------------------------------------------------------------------------------------------
use="$stable"
if [ "$RUNNER_OS" = Windows ] && command -v cygpath >/dev/null 2>&1; then use="$(cygpath -w "$stable")"; fi
echo "PADDOCK_WORKSPACE=$use" >> "$GITHUB_ENV"
# Where the work folder can be replaced (not under act on Windows, which holds it open) a link keeps every path that names it valid.
link_work_folder() { cd "$(dirname "$ws")" && rm -rf "$ws" && ln -s "$stable" "$ws"; }
if [ "$RUNNER_OS" != Windows ]; then
  if ! bounded "link $ws to $stable" link_work_folder; then
    echo "::warning::kept-clone: could not link $ws to $stable ($(cat "$scratch/err")); steps build from PADDOCK_WORKSPACE."
  fi
fi
echo "kept-clone: ready: $sha ($mode, profile $profile, $files files) in $((SECONDS - started)) s"

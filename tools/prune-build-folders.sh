#!/usr/bin/env bash
# Keeps a persistent CI build folder (host runners) within a size cap, so nothing piles up on the owner's machines
# (plan X47; Dyno's ci-prune.sh).
#
#   bash paddock/tools/prune-build-folders.sh <folder> <max-gigabytes>
#
# Over the cap the incremental-compilation caches go first (the bulk, and cheap to rebuild); still over, the folder is deleted.
# A missing folder is fine. Only <folder> is touched; symlinks are never followed. PADDOCK_PRUNE_UNIT=kb makes the cap
# kilobytes instead of gigabytes (tests).
set -euo pipefail
[ $# -eq 2 ] || { echo "usage: prune-build-folders.sh <folder> <max-gigabytes>" >&2; exit 2; }
dir="$1"
max="$2"
[[ "$max" =~ ^[0-9]+$ ]] || { echo "prune: the cap must be a whole number, got '$max'" >&2; exit 2; }
[ -d "$dir" ] && [ ! -L "$dir" ] || exit 0

unit=1048576
[ "${PADDOCK_PRUNE_UNIT:-gb}" = kb ] && unit=1
# The larger of the allocated size and the length of the regular files (hard links once, the total rounded up to a kilobyte). A file
# system that allocates late (ZFS, btrfs, a compressed volume) reports next to nothing in `du` for data written a moment ago, which kept a
# full folder under the cap on a runner. The length comes from the kit's one GNU/BSD helper (`kit.sh portable file-bytes`), not from
# `du`: GNU `du --apparent-size` counts folders as 0 and BSD `du -A` counts them and rounds each entry up. What cannot be read counts
# 0 on both sides, as in `ci prune-folder` (tools/paddock-dev/src/ci_host/prune.rs, `sizes`), which counts the same way.
kit="$(cd "$(dirname "$0")/.." && pwd)/conformance/ci/kit.sh"
# The one guarded delete (conformance/ci/safe-rm.sh): the folder goes only strictly under its parent, as `ci prune-folder` does.
. "$(dirname "$kit")/safe-rm.sh"
size() {
  local used apparent
  used=$(du -sk "$dir" 2>/dev/null | cut -f1 || true)
  [[ "$used" =~ ^[0-9]+$ ]] || used=0
  apparent=$(sh "$kit" portable file-bytes "$dir")
  [[ "$apparent" =~ ^[0-9]+$ ]] || apparent=0
  apparent=$(( (apparent + 1023) / 1024 ))
  echo $(( (apparent > used ? apparent : used) / unit ))
}

if [ "$(size)" -ge "$max" ]; then
  echo "prune: $dir is $(size) (cap $max); removing incremental caches"
  # Each cache strictly under the folder, a refusal a warning, as `ci prune-folder` does (find -prune never enters a link).
  find "$dir" -type d -name incremental -prune 2>/dev/null | while IFS= read -r f; do
    SAFE_RM_ANNOTATION=warning safe_rm "$dir" "$f" || true
  done
fi
if [ "$(size)" -ge "$max" ]; then
  echo "prune: still $(size); removing $dir"
  safe_rm "$(dirname "$dir")" "$dir"
fi

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
# The larger of the allocated size and the length of the files. A file system that allocates late (ZFS, btrfs, a compressed volume) reports
# next to nothing in `du` for data written a moment ago, which kept a full folder under the cap on a runner; `du -A` is BSD/macOS,
# `--apparent-size` is GNU, and a `du` with neither simply counts allocated blocks.
size() {
  local used apparent
  used=$(du -sk "$dir" | cut -f1)
  apparent=$(du -skA "$dir" 2>/dev/null | cut -f1 || true)
  [[ "$apparent" =~ ^[0-9]+$ ]] || apparent=$(du -sk --apparent-size "$dir" 2>/dev/null | cut -f1 || true)
  [[ "$apparent" =~ ^[0-9]+$ ]] || apparent=0
  echo $(( (apparent > used ? apparent : used) / unit ))
}

if [ "$(size)" -ge "$max" ]; then
  echo "prune: $dir is $(size) (cap $max); removing incremental caches"
  find "$dir" -type d -name incremental -prune -exec rm -rf {} + 2>/dev/null || true
fi
if [ "$(size)" -ge "$max" ]; then
  echo "prune: still $(size); removing $dir"
  rm -rf "$dir"
fi

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
size() { echo $(( $(du -sk "$dir" | cut -f1) / unit )); }

if [ "$(size)" -ge "$max" ]; then
  echo "prune: $dir is $(size) (cap $max); removing incremental caches"
  find "$dir" -type d -name incremental -prune -exec rm -rf {} + 2>/dev/null || true
fi
if [ "$(size)" -ge "$max" ]; then
  echo "prune: still $(size); removing $dir"
  rm -rf "$dir"
fi

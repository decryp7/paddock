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
# The larger of the allocated size and the length of the regular files (hard links once, the total rounded up to a kilobyte), both
# from one pass of the kit's GNU/BSD helper (`kit.sh portable file-sizes`), never from `du`. A file system that allocates late (ZFS,
# btrfs, a compressed volume) reports next to nothing allocated for data written a moment ago, which kept a full folder under the cap
# on a runner. Folders are not content: GNU `du --apparent-size` counts them as 0, BSD `du -A` rounds each one up, and `du -sk` counts
# ext4's 4 KiB per folder (a folder of empty folders was 20 KiB, CI run 8138). What cannot be read counts 0. `ci prune-folder`
# (tools/paddock-dev/src/ci_host/prune.rs, `sizes`) counts the same way on Unix; on Windows it counts each file's length as its
# allocated size and does not find hard links, so the two agree on Unix only.
# A pass stats every file (about 7 s for 15,306 files on a Mac, where `du -sk` took 0.1 s): the size is measured once, and again
# only after the incremental caches went (docs/benchmarks.md, "Prune size, one pass").
kit="$(cd "$(dirname "$0")/.." && pwd)/conformance/ci/kit.sh"
size() {
  local sizes apparent used
  sizes=$(sh "$kit" portable file-sizes "$dir")
  apparent=${sizes% *}
  used=${sizes#* }
  [[ "$apparent" =~ ^[0-9]+$ ]] || apparent=0
  [[ "$used" =~ ^[0-9]+$ ]] || used=0
  apparent=$(( (apparent + 1023) / 1024 ))
  used=$(( (used + 1023) / 1024 ))
  echo $(( (apparent > used ? apparent : used) / unit ))
}

now=$(size)
if [ "$now" -ge "$max" ]; then
  echo "prune: $dir is $now (cap $max); removing incremental caches"
  find "$dir" -type d -name incremental -prune -exec rm -rf {} + 2>/dev/null || true
  now=$(size)
  if [ "$now" -ge "$max" ]; then
    echo "prune: still $now; removing $dir"
    rm -rf "$dir"
  fi
fi

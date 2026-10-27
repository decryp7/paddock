#!/usr/bin/env bash
# Keeps a persistent CI build folder (host runners) within a size cap, so nothing piles up on the owner's machines.
#   scripts/ci-prune.sh <folder> <max-gigabytes>
# Over the cap the incremental-compilation caches go first (the bulk, and cheap to rebuild); still over, the folder is
# deleted. A missing folder is fine.
set -euo pipefail
dir="${1:?folder}"
max_gb="${2:?max gigabytes}"
[[ -d "$dir" ]] || exit 0

size_gb() { echo $(( $(du -sk "$dir" | cut -f1) / 1048576 )); }

if (( $(size_gb) >= max_gb )); then
  echo "ci-prune: $dir is $(size_gb) GB (cap $max_gb GB); removing incremental caches"
  find "$dir" -type d -name incremental -prune -exec rm -rf {} + 2>/dev/null || true
fi
if (( $(size_gb) >= max_gb )); then
  echo "ci-prune: still $(size_gb) GB; removing $dir"
  rm -rf "$dir"
fi

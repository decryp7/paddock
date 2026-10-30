#!/usr/bin/env bash
# The CI form of prune-build-folders.sh: caps this job's kept build folder, but only a folder the build-folder action made
# for this job's slot.
#
#   bash paddock/tools/prune-ci-build-folder.sh <folder> <max-gigabytes> [--drop-bundles]
#
# Before anything is deleted the folder must be a kept build folder of this job's slot (conformance/ci/job-isolation.sh owns
# <folder> target: target-<name>-<slot> directly in the CI home, both resolved). A symlink, $HOME, /, a project folder, any
# path outside the CI home and another slot's folder (a concurrent job's) are refused with a notice. Always exits 0.
set -uo pipefail
dir="${1:-}"; max="${2:-}"; drop="${3:-}"
[ -n "$dir" ] || { echo "prune: no build folder was set in this job; nothing to prune"; exit 0; }
[ -n "$max" ] || { echo "prune: no cap given; nothing to prune"; exit 0; }
iso="$(dirname "$0")/../conformance/ci/job-isolation.sh"
if ! bash "$iso" owns "$dir" target 2>/dev/null; then
  echo "prune: $dir is not a kept build folder of this job's slot (target-<name>-${PADDOCK_CI_SLOT:-<slot>} in the CI home); leaving it alone"
  exit 0
fi
real="$(cd "$dir" && pwd -P)"
if [ "$drop" = --drop-bundles ]; then rm -rf "$real/dist/bundle" 2>/dev/null; fi
bash "$(dirname "$0")/prune-build-folders.sh" "$real" "$max" || true
exit 0

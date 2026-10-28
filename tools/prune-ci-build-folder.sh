#!/usr/bin/env bash
# The CI form of prune-build-folders.sh: caps this job's kept build folder, but only a folder the build-folder action made.
#
#   bash paddock/tools/prune-ci-build-folder.sh <folder> <max-gigabytes> [--drop-bundles]
#
# Before anything is deleted the folder must be named target-<something> and sit directly in ${PADDOCK_CI_HOME:-$HOME/.paddock-ci}
# (both resolved: a symlink, $HOME, /, a project folder or any path outside is refused with a notice). Always exits 0.
set -uo pipefail
dir="${1:-}"; max="${2:-}"; drop="${3:-}"
[ -n "$dir" ] || { echo "prune: no build folder was set in this job; nothing to prune"; exit 0; }
[ -n "$max" ] || { echo "prune: no cap given; nothing to prune"; exit 0; }
home="${PADDOCK_CI_HOME:-${HOME:-}/.paddock-ci}"
refuse() { echo "prune: $dir is not a kept build folder of this runner (target-* in $home); leaving it alone"; exit 0; }
[ -d "$dir" ] && [ ! -L "$dir" ] || refuse
[ -d "$home" ] && [ ! -L "$home" ] || refuse
case "$(basename "$dir")" in target-?*) ;; *) refuse ;; esac
real_parent="$(cd "$(dirname "$dir")" 2>/dev/null && pwd -P)" || refuse
real_home="$(cd "$home" 2>/dev/null && pwd -P)" || refuse
[ "$real_parent" = "$real_home" ] || refuse
if [ "$drop" = --drop-bundles ]; then rm -rf "$real_parent/$(basename "$dir")/dist/bundle" 2>/dev/null; fi
bash "$(dirname "$0")/prune-build-folders.sh" "$real_parent/$(basename "$dir")" "$max" || true
exit 0

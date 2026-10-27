#!/usr/bin/env bash
# The CI job's Rust build folder, set as cargo configuration and never with CARGO_TARGET_DIR: sccache puts every CARGO_*
# variable into its cache key (Paddock lessons, "Test and build speed").
#
#   bash scripts/ci/build-folder.sh
#
# Linux containers build in the checkout's target/ (a new container every run; sccache keeps the compiled crates). Host
# runners (macOS, Windows) build in a folder that outlives the job, one per job and outside the checkout:
# ${PADDOCK_CI_HOME:-$HOME/.paddock-ci}/target-debrief-<job>. It is written to .cargo/ci.toml (git-ignored, included by
# .cargo/config.toml), so every cargo command (nextest, tauri build, the sidecar build, cargo metadata) uses it.
# Exports DEBRIEF_TARGET (the folder) and DEBRIEF_E2E_APP (the debug app the smoke tests start) through $GITHUB_ENV.
set -euo pipefail
cd "$(dirname "$0")/../.."
out=${GITHUB_ENV:?GITHUB_ENV is not set}
ext=
case "${RUNNER_OS:?}" in
  Linux)
    rm -f .cargo/ci.toml
    dir="$PWD/target"
    ;;
  *)
    dir="${PADDOCK_CI_HOME:-$HOME/.paddock-ci}/target-debrief-${GITHUB_JOB:?GITHUB_JOB is not set}"
    mkdir -p "$dir"
    # Cargo on Windows wants a native path; forward slashes work in cargo, Node and Git bash alike.
    if [ "$RUNNER_OS" = Windows ]; then
      ext=.exe
      if command -v cygpath >/dev/null 2>&1; then dir=$(cygpath -m "$dir"); fi
    fi
    case "$dir" in *"'"*) echo "::error::the build folder $dir has a quote in its path"; exit 1 ;; esac
    printf "# Written by scripts/ci/build-folder.sh for this CI job (git-ignored).\n[build]\ntarget-dir = '%s'\n" "$dir" > .cargo/ci.toml
    ;;
esac
{
  echo "DEBRIEF_TARGET=$dir"
  echo "DEBRIEF_E2E_APP=$dir/debug/debrief-app$ext"
} >> "$out"
echo "build folder: $dir"

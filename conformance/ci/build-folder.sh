#!/usr/bin/env bash
# A CI job's Rust build folder, set as cargo configuration and never with CARGO_TARGET_DIR: sccache puts every CARGO_*
# variable into its cache key (Paddock lessons, "Test and build speed"). Shared form of Debrief's scripts/ci/build-folder.sh.
#
#   bash build-folder.sh <cargo-workspace> <name>
#
# Linux containers build in the workspace's target/ (a new container every run; sccache keeps the compiled crates). Host
# runners (macOS, Windows) build in a folder that outlives the job, outside the checkout: the kept folder `target` of the
# job's slot (job-isolation.sh path target <name>: <ci-home>/target-<name>-<slot>), so two jobs on one host never share one. It is written to <cargo-workspace>/.cargo/ci.toml (git-ignored,
# included by the workspace's .cargo/config.toml as `include = [{ path = "ci.toml", optional = true }]`), so every cargo
# command (clippy, nextest, doc tests, `tauri build`, cargo metadata) uses it with no flag and no environment variable.
# Exports PADDOCK_BUILD_FOLDER (the folder, for the steps that collect from it) through $GITHUB_ENV.
set -euo pipefail
ws="${1:?the Cargo workspace folder}"
name="${2:?a folder name, e.g. <app>-<job>}"
out=${GITHUB_ENV:?GITHUB_ENV is not set}
case "$name" in *[!A-Za-z0-9._-]*) echo "::error::build-folder: the name '$name' may only hold letters, digits, '.', '_' and '-'"; exit 1 ;; esac
ws=$(cd "$ws" && pwd)
config="$ws/.cargo/config.toml"
if ! grep -Eq '^include = \[.*\{ *path *= *"ci\.toml", *optional *= *true *\}' "$config" 2>/dev/null; then
  echo "::error::build-folder: $config must include the job's build folder: include = [{ path = \"ci.toml\", optional = true }]"
  exit 1
fi
case "${RUNNER_OS:?}" in
  Linux)
    rm -f "$ws/.cargo/ci.toml"
    dir="$ws/target"
    ;;
  *)
    dir="$(bash "$(dirname "$0")/job-isolation.sh" path target "$name")"
    # Cargo on Windows wants a native path; forward slashes work in cargo, Node and Git bash alike.
    if [ "$RUNNER_OS" = Windows ] && command -v cygpath >/dev/null 2>&1; then dir=$(cygpath -m "$dir"); fi
    case "$dir" in *"'"*) echo "::error::build-folder: the build folder $dir has a quote in its path"; exit 1 ;; esac
    printf "# Written by build-folder.sh for this CI job (git-ignored).\n[build]\ntarget-dir = '%s'\n" "$dir" > "$ws/.cargo/ci.toml"
    ;;
esac
echo "PADDOCK_BUILD_FOLDER=$dir" >> "$out"
echo "build folder: $dir"

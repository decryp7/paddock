#!/bin/sh
# The CI kit's bootstrap shim (docs/plans/ci-in-rust.md, WP-1 and section 5): runs a kit step in Rust (`./dev ci <step>`) or, on the
# release path, through the shell script that step had until the release-path lane deletes it. This is the one place that picks the
# implementation (setup-rust's Implementation step asks `kit.sh which`), so a workflow or action says
# `bash conformance/ci/kit.sh <step> ...` and nothing else.
#
#   kit.sh which                     prints dev or shell (the reason goes to stderr)
#   kit.sh isolation <args>          `./dev ci isolation <args>` (or job-isolation.sh <args>)
#   kit.sh build-jobs [<split>]      `./dev ci build-jobs [<split>]` (or build-jobs.sh [<split>])
#
# The shell is used for: PADDOCK_CI_KIT=shell (the fallback switch: one variable away, plan section 7), a tag run, a run that never
# downloads the tool (PADDOCK_DEV_DOWNLOAD=0: release and rule 54 jobs), a checkout without tools/paddock-dev (another repository
# using the actions) and a machine without rustc (./dev hashes and builds itself with it). Every call prints one `kit:` line on
# standard error naming the choice and why (ER-27). The scripts stay until WP-11 deletes them with this shim; both implementations are
# held equal by tools/paddock-dev/tests/it/build_jobs.rs and conformance/test/job-isolation.test.mjs (ISOLATION_CMD).
#
# One tool per job: on a host runner the tool is fetched or built in the CI home's shared `tools` folder, never in the runner's own
# home (a Windows job folder must stay deletable while the slot's holder runs). The folder is asked of job-isolation.sh in shell, so
# no tool is needed to find where the tool goes. `isolation acquire` pins the tool as PADDOCK_DEV for the later steps and for
# release, which must not build it after the job's temp folder is gone. A Linux container takes no slot: acquire and release do
# nothing there, and the wrapper keeps the tool on the /sccache mount.
#
# The job's compiler wrapper is unset before every ./dev call: ./dev builds the tool when its binary is missing, and at release the
# job's sccache server is stopped and its temp folder removed (runs 7773 to 7775).
#
# build-jobs: the tool's own first build needs a core limit before CARGO_BUILD_JOBS exists; it gets the default of 4 (the share of
# a runner on the shared Mac; on Linux measured in docs/benchmarks.md, "Kit shim default"), and `./dev ci build-jobs` then writes
# the job's real value.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

# kit_choice: prints "<dev|shell> <reason>".
kit_choice() {
  if [ "${PADDOCK_CI_KIT:-}" = shell ]; then echo "shell PADDOCK_CI_KIT=shell"; return; fi
  case "${GITHUB_REF:-}" in refs/tags/*) echo "shell a tag run"; return ;; esac
  if [ "${PADDOCK_DEV_DOWNLOAD:-}" = 0 ]; then echo "shell PADDOCK_DEV_DOWNLOAD=0 (a release or rule 54 run)"; return; fi
  # ./dev runs the tool of the checkout it is started in (as setup-rust does): another repository using these actions, or a
  # sparse checkout without tools/, has none.
  top=$(git rev-parse --show-toplevel 2>/dev/null || true)
  if [ -z "$top" ] || [ ! -f "$top/tools/paddock-dev/Cargo.toml" ]; then echo "shell no tools/paddock-dev in this checkout"; return; fi
  if [ ! -f "$root/dev" ]; then echo "shell no ./dev wrapper beside this script"; return; fi
  if ! command -v rustc >/dev/null 2>&1; then echo "shell no rustc on PATH"; return; fi
  echo "dev ./dev ci steps are available"
}

choice=$(kit_choice)
kit=${choice%% *}
echo "kit: $kit (${choice#* })" >&2

# Without the read credential ./dev builds itself from source and never takes a binary from the store (ADR 0022 section 3.3).
dev_env() {
  if [ -z "${PADDOCK_DEV_TOKEN-}${GITHUB_TOKEN-}" ]; then PADDOCK_DEV_DOWNLOAD=0; export PADDOCK_DEV_DOWNLOAD; fi
  unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER CARGO_BUILD_RUSTC_WRAPPER CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER
}

# tools_home: on a host runner, PADDOCK_DEV_HOME and PADDOCK_DEV_TARGET_DIR under the CI home's tools folder (unless the caller
# chose them), found without the tool.
tools_home() {
  [ "${RUNNER_OS:-}" != Linux ] || return 0
  [ -z "${PADDOCK_DEV_HOME:-}" ] || return 0
  tools="$(bash "$here/job-isolation.sh" path tools)"
  PADDOCK_DEV_HOME="$tools/paddock-dev"
  PADDOCK_DEV_TARGET_DIR="$tools/paddock-dev-target"
  export PADDOCK_DEV_HOME PADDOCK_DEV_TARGET_DIR
}

dev_isolation() {
  dev_env
  case "${1:-}" in
    acquire | release)
      [ "${RUNNER_OS:-}" != Linux ] || exit 0
      if [ "$1" = release ] && [ -n "${PADDOCK_DEV:-}" ]; then exec "$PADDOCK_DEV" ci isolation release; fi
      tools_home
      if [ "$1" = release ]; then exec bash "$root/dev" ci isolation release; fi
      bin="$(bash "$root/dev" --print-binary)" || bin=""
      if [ -n "$bin" ]; then
        PADDOCK_DEV="$bin"
        export PADDOCK_DEV
        exec "$bin" ci isolation acquire
      fi
      exec bash "$root/dev" ci isolation acquire
      ;;
    *)
      # The pinned tool when the job has one (acquire pinned it), else the wrapper.
      if [ -n "${PADDOCK_DEV:-}" ]; then exec "$PADDOCK_DEV" ci isolation "$@"; fi
      tools_home
      exec bash "$root/dev" ci isolation "$@"
      ;;
  esac
}

case "${1:-}" in
  which) echo "$kit" ;;
  isolation)
    shift
    if [ "$kit" = dev ]; then dev_isolation "$@"; fi
    exec bash "$here/job-isolation.sh" "$@"
    ;;
  build-jobs)
    shift
    if [ "$kit" = dev ]; then
      dev_env
      tools_home
      CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
      export CARGO_BUILD_JOBS
      exec bash "$root/dev" ci build-jobs "$@"
    fi
    exec bash "$here/build-jobs.sh" "$@"
    ;;
  *)
    echo "usage: kit.sh which | isolation <args> | build-jobs [<split>]" >&2
    exit 2
    ;;
esac

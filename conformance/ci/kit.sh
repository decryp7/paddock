#!/bin/sh
# The CI kit's bootstrap shim (docs/plans/ci-in-rust.md, WP-1 and section 5): runs a kit step in Rust (`./dev ci <step>`) or, on the
# release path, through the shell script that step had until the release-path lane deletes it. This is the one place that picks the
# implementation (setup-rust's Implementation step asks `kit.sh which`), so a workflow or action says
# `bash conformance/ci/kit.sh <step> ...` and nothing else.
#
#   kit.sh which                     prints dev or shell (the reason goes to stderr)
#   kit.sh isolation <args>          `./dev ci isolation <args>` (or job-isolation.sh <args>)
#   kit.sh build-jobs [<split>]      `./dev ci build-jobs [<split>]` (or build-jobs.sh [<split>])
#   kit.sh host <step> <args>        a host-runner step (WP-4), in Rust or in the shell it had:
#     stable-workspace <name>                     ci stable-workspace --name | host/stable-workspace.sh (NAME)
#     node-caches <angular-dir> <name>            ci node-caches --angular-dir --name | host/node-caches.sh (ANGULAR_DIR, NAME)
#     host-tools | sccache-stats                  ci host-tools | ci sccache-stats | host/<step>.sh
#     build-folder <workspace> <name>             ci build-folder | build-folder.sh
#     prune-build-folder <max-gb> [drop-bundles]  ci prune-build-folder --max-gb [--drop-bundles] | tools/prune-ci-build-folder.sh
#     prune-folder <folder> <max-gb>              ci prune-folder | tools/prune-build-folders.sh
#     host-cleanup <host-cleanup options>         ci host-cleanup | tools/host-cleanup.sh
#     kept-clone <name> <profile> [<fresh>]       ci kept-clone --name --profile [--fresh] | kept-clone.sh (NAME, PROFILE, FRESH)
#     guardian-as-user                            ci guardian-as-user | guardian-as-user.sh (the Linux crates job, from crates/)
#   (`ci boot-volume external|temp` has no shim route: job-tmpdir.sh sources boot-volume.sh, and paddock_testkit and Dyno's
#   check-bundle.sh run it; the Rust copy is job-tmpdir's rule in process. WP-5, docs/plans/ci-in-rust.md.)
#
# The shell is used for: PADDOCK_CI_KIT=shell (the fallback switch: one variable away, plan section 7), a tag run, a run that never
# downloads the tool (PADDOCK_DEV_DOWNLOAD=0: release and rule 54 jobs), a checkout without tools/paddock-dev (another repository
# using the actions) and a machine without rustc (./dev hashes and builds itself with it). Every call prints one `kit:` line on
# standard error naming the choice and why (ER-27). The scripts stay until WP-11 deletes them with this shim; both implementations are
# held equal by tools/paddock-dev/tests/it/build_jobs.rs, tools/paddock-dev/tests/it/host_steps.rs and
# conformance/test/job-isolation.test.mjs (ISOLATION_CMD).
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

# tools_home: in a CI job on a host runner, PADDOCK_DEV_HOME and PADDOCK_DEV_TARGET_DIR under the CI home's tools folder (unless the caller
# chose them), found without the tool.
tools_home() {
  # Only a CI job has a CI home: a person's machine keeps the tool where ./dev puts it (a RUNNER_OS in a shell profile must not move it).
  [ "${GITHUB_ACTIONS:-}" = true ] || return 0
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
        job_setup "$bin" ci isolation acquire
      fi
      job_setup bash "$root/dev" ci isolation acquire
      ;;
    *)
      # The pinned tool when the job has one (acquire pinned it), else the wrapper.
      if [ -n "${PADDOCK_DEV:-}" ]; then exec "$PADDOCK_DEV" ci isolation "$@"; fi
      tools_home
      exec bash "$root/dev" ci isolation "$@"
      ;;
  esac
}

# job_setup <command>: runs a job's setup step (build-jobs, the isolation acquire) and never returns. When it fails the job must not go
# on: later steps that run after a failure (`if: !cancelled()`, so one run reports every failed check) would test without the
# job's slot, temp folders or core share (run 8027, crate on Windows ran its tests after ./dev could not be built). The failure
# is written to GITHUB_ENV as PADDOCK_JOB_ABORTED=1, which those steps test, and said once as an annotation.
job_setup() {
  "$@" && exit 0
  rc=$?
  echo "::error::A job setup step failed (exit $rc: $*). The job's later steps are skipped, because they would run without the slot, temp folders and core share that step sets up. Read the error above it, fix that, and rerun the job." >&2
  if [ -n "${GITHUB_ENV:-}" ]; then echo "PADDOCK_JOB_ABORTED=1" >> "$GITHUB_ENV" || true; fi
  exit "$rc"
}

# first_build_jobs: the core limit for the tool's own first build (see the header: build-jobs).
first_build_jobs() {
  CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
  export CARGO_BUILD_JOBS
}

# dev_run <args>: `./dev ci <args>`: the pinned tool when the job has one, else the wrapper (which builds it in the shared tools folder).
dev_run() {
  dev_env
  if [ -n "${PADDOCK_DEV:-}" ]; then exec "$PADDOCK_DEV" ci "$@"; fi
  tools_home
  first_build_jobs
  exec bash "$root/dev" ci "$@"
}

host_usage() {
  echo "usage: kit.sh host stable-workspace <name> | node-caches <angular-dir> <name> | host-tools | sccache-stats | build-folder <workspace> <name> | prune-build-folder <max-gb> [drop-bundles] | prune-folder <folder> <max-gb> | host-cleanup <options> | kept-clone <name> <profile> [<fresh>] | guardian-as-user" >&2
  exit 2
}

# kit_host <step> <args>: a host step in Rust or in its shell (the runner's `bash -e -o pipefail` for the inline ones it had).
kit_host() {
  step="${1:-}"
  [ -n "$step" ] || host_usage
  shift
  # Nothing to do, so no tool is fetched or built for it: a Linux container has a fixed workspace, and without sccache there are no
  # statistics.
  case "$step" in
    stable-workspace | kept-clone) [ "${RUNNER_OS:-}" != Linux ] || exit 0 ;;
    sccache-stats) command -v sccache >/dev/null 2>&1 || exit 0 ;;
  esac
  if [ "$kit" = dev ]; then
    case "$step" in
      stable-workspace) dev_run stable-workspace --name "${1:?stable-workspace needs a name}" ;;
      node-caches) dev_run node-caches --angular-dir "${1-}" --name "${2:?node-caches needs a name}" ;;
      host-tools | sccache-stats | guardian-as-user) dev_run "$step" ;;
      kept-clone)
        if [ "${3:-}" = true ]; then dev_run kept-clone --name "${1:?kept-clone needs a name}" --profile "${2:?kept-clone needs a profile}" --fresh; fi
        dev_run kept-clone --name "${1:?kept-clone needs a name}" --profile "${2:?kept-clone needs a profile}"
        ;;
      build-folder | prune-folder | host-cleanup) dev_run "$step" "$@" ;;
      prune-build-folder)
        if [ "${2:-}" = true ]; then dev_run prune-build-folder --max-gb "${1:-20}" --drop-bundles; fi
        dev_run prune-build-folder --max-gb "${1:-20}"
        ;;
      *) host_usage ;;
    esac
  fi
  case "$step" in
    stable-workspace) NAME="${1:?stable-workspace needs a name}" exec bash -e -o pipefail "$here/host/stable-workspace.sh" ;;
    node-caches) ANGULAR_DIR="${1-}" NAME="${2:?node-caches needs a name}" exec bash -e -o pipefail "$here/host/node-caches.sh" ;;
    host-tools | sccache-stats) exec bash -e -o pipefail "$here/host/$step.sh" ;;
    kept-clone) NAME="${1:?kept-clone needs a name}" PROFILE="${2:?kept-clone needs a profile}" FRESH="${3:-false}" exec bash "$here/kept-clone.sh" ;;
    guardian-as-user) exec bash "$here/guardian-as-user.sh" ;;
    build-folder) exec bash "$here/build-folder.sh" "$@" ;;
    prune-folder) exec bash "$root/tools/prune-build-folders.sh" "$@" ;;
    host-cleanup) exec bash "$root/tools/host-cleanup.sh" "$@" ;;
    prune-build-folder)
      dir="${PADDOCK_BUILD_FOLDER:-}"
      if [ -n "$dir" ] && command -v cygpath >/dev/null; then dir="$(cygpath -u "$dir")"; fi
      extra=""; if [ "${2:-}" = true ]; then extra="--drop-bundles"; fi
      exec bash "$root/tools/prune-ci-build-folder.sh" "$dir" "${1:-20}" ${extra:+"$extra"}
      ;;
    *) host_usage ;;
  esac
}

case "${1:-}" in
  which) echo "$kit" ;;
  host)
    shift
    kit_host "$@"
    ;;
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
      first_build_jobs
      job_setup bash "$root/dev" ci build-jobs "$@"
    fi
    job_setup bash "$here/build-jobs.sh" "$@"
    ;;
  *)
    echo "usage: kit.sh which | isolation <args> | build-jobs [<split>] | host <step> <args>" >&2
    exit 2
    ;;
esac

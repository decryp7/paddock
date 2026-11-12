#!/bin/sh
# The CI kit's bootstrap shim (docs/plans/ci-in-rust.md, WP-1 and section 5): runs a kit step in Rust (`./dev ci <step>`) or, on the
# release path, through the shell script that step had until the release-path lane deletes it. One place picks the implementation,
# so a workflow or action says `bash conformance/ci/kit.sh <step> ...` and nothing else.
#
#   kit.sh which                     prints dev or shell
#   kit.sh isolation <args>          `./dev ci isolation <args>` (or job-isolation.sh <args>)
#   kit.sh build-jobs [<split>]      `./dev ci build-jobs [<split>]` (or build-jobs.sh [<split>])
#
# The shell is used (as setup-rust's Implementation step does, the same three rules) for a tag run, for a run that never
# downloads the tool (PADDOCK_DEV_DOWNLOAD=0: release and rule 54 jobs), and where ./dev cannot run: a checkout without
# tools/paddock-dev (another repository using the actions) or a machine without rustc (./dev hashes and builds itself with it).
# The shell scripts stay until WP-11 deletes them with the guard; both implementations are held equal by
# tools/paddock-dev/tests/it/build_jobs.rs and conformance/test/job-isolation.test.mjs (ISOLATION_CMD).
#
# isolation acquire and release, in Rust: the first `./dev` call finds the shared tools folder (a kept folder of the CI home); the
# tool the job runs from is then fetched or built there (PADDOCK_DEV_HOME and PADDOCK_DEV_TARGET_DIR under it, never inside a
# job folder: a Windows job folder must stay deletable while the slot's holder runs) and pinned as PADDOCK_DEV for the later steps
# and for release, which must not build the tool after the job's temp folder is gone. A Linux container takes no slot, so acquire
# and release do nothing there and no tool is fetched for them.
#
# build-jobs: the tool's own first build (when it has to build) needs a core limit before CARGO_BUILD_JOBS exists; it gets the
# default of 4 (the share of a runner on the shared Mac), and `./dev ci build-jobs` then writes the job's real value.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

kit() {
  case "${GITHUB_REF:-}" in refs/tags/*) echo shell; return ;; esac
  if [ "${PADDOCK_DEV_DOWNLOAD:-}" = 0 ]; then echo shell; return; fi
  # ./dev runs the tool of the checkout it is started in (as setup-rust does): another repository using these actions, or a
  # sparse checkout without tools/, has none.
  top=$(git rev-parse --show-toplevel 2>/dev/null || true)
  if [ -z "$top" ] || [ ! -f "$top/tools/paddock-dev/Cargo.toml" ] || [ ! -f "$root/dev" ]; then echo shell; return; fi
  if ! command -v rustc >/dev/null 2>&1; then echo shell; return; fi
  echo dev
}

# Without the read credential ./dev builds itself from source and never takes a binary from the store (ADR 0022 section 3.3).
no_token_no_download() {
  if [ -z "${PADDOCK_DEV_TOKEN-}${GITHUB_TOKEN-}" ]; then PADDOCK_DEV_DOWNLOAD=0; export PADDOCK_DEV_DOWNLOAD; fi
}

dev_isolation() {
  no_token_no_download
  case "${1:-}" in
    acquire | release)
      [ "${RUNNER_OS:-}" != Linux ] || exit 0
      if [ "$1" = release ] && [ -n "${PADDOCK_DEV:-}" ]; then exec "$PADDOCK_DEV" ci isolation release; fi
      tools="$(bash "$root/dev" ci isolation path tools)"
      PADDOCK_DEV_HOME="$tools/paddock-dev"
      PADDOCK_DEV_TARGET_DIR="$tools/paddock-dev-target"
      export PADDOCK_DEV_HOME PADDOCK_DEV_TARGET_DIR
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
      exec bash "$root/dev" ci isolation "$@"
      ;;
  esac
}

case "${1:-}" in
  which) kit ;;
  isolation)
    shift
    if [ "$(kit)" = dev ]; then dev_isolation "$@"; fi
    exec bash "$here/job-isolation.sh" "$@"
    ;;
  build-jobs)
    shift
    if [ "$(kit)" = dev ]; then
      no_token_no_download
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

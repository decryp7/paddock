#!/usr/bin/env bash
# CARGO_BUILD_JOBS for a CI job: the one place CI sets it (ER-22). The build-jobs action runs it as an early step of a host job
# (before job-isolation, which builds paddock-dev), and setup-rust runs it when no earlier step did, so every Rust job has it.
# A conformance test (conformance/test/ci-actions.test.mjs) fails when a workflow, action or CI script sets CARGO_BUILD_JOBS
# itself or passes a fixed -j/--jobs to cargo.
#
#   build-jobs.sh          writes CARGO_BUILD_JOBS=<n> to $GITHUB_ENV (or stdout when GITHUB_ENV is unset) and logs why
#
# The agents' local cap of 2 (ER-24) is for the shared developer Mac; a CI runner gets its share of the host's cores:
#
#   1. PADDOCK_CI_BUILD_JOBS (a runner's runner.envs), a whole number >= 1: used as it is.
#   2. Otherwise (the host's cores - PADDOCK_CI_RESERVED_CORES) / PADDOCK_CI_JOBS_PER_HOST, at least 1 (runner.envs:
#      the cores kept for work outside CI on that host, and how many jobs the host runs at once, the sum of its runners'
#      capacity). Without them, the runner class's known layout (verified 2026-10-30 from Gitea's job list: the most jobs
#      one runner ran at once over the last 100 runs):
#                 jobs/host  reserved
#        Linux    4          0   d-gtr9, capacity 4, containers without a CPU limit (cargo's own default: every core, per job)
#        Windows  2          0   decryp7ology-dev, two jobs at once; 16 cores, 32 GB: 8 each, as two builds of 16 ran out of
#                                memory
#        macOS    1          10  d7-mac-mini, capacity 1, on the developer Mac its agents share: up to 5 agents with
#                                CARGO_BUILD_JOBS=2 each (ER-24) keep 10 of its 14 cores, so CI gets 4, as before. Set
#                                PADDOCK_CI_RESERVED_CORES=1 in the runner's runner.envs when the Mac is CI's alone (13).
#
# Cores: nproc (Linux and Git Bash; honours the container's cpuset), else sysctl hw.ncpu (macOS), else NUMBER_OF_PROCESSORS,
# else getconf. PADDOCK_CI_CORES and RUNNER_OS replace the probes in tests. Runs under macOS' bash 3.2.
set -euo pipefail

is_count() { case "$1" in '' | *[!0-9]* | 0*) return 1 ;; *) return 0 ;; esac; }

cores() {
  if [ -n "${PADDOCK_CI_CORES:-}" ]; then echo "$PADDOCK_CI_CORES"; return; fi
  local n=""
  n=$(nproc 2>/dev/null) || n=""
  is_count "$n" || n=$(sysctl -n hw.ncpu 2>/dev/null) || n=""
  is_count "$n" || n="${NUMBER_OF_PROCESSORS:-}"
  is_count "$n" || n=$(getconf _NPROCESSORS_ONLN 2>/dev/null) || n=""
  echo "$n"
}

default_share() {
  case "${RUNNER_OS:-}" in
    Linux) echo 4 ;;
    Windows) echo 2 ;;
    macOS) echo 1 ;;
    *) echo 1 ;;
  esac
}

default_reserved() {
  case "${RUNNER_OS:-}" in
    macOS) echo 10 ;;
    *) echo 0 ;;
  esac
}

emit() { # <jobs> <why>
  echo "CARGO_BUILD_JOBS=$1 ($2)"
  if [ -n "${GITHUB_ENV:-}" ]; then echo "CARGO_BUILD_JOBS=$1" >> "$GITHUB_ENV"; else echo "CARGO_BUILD_JOBS=$1"; fi
}

if [ -n "${PADDOCK_CI_BUILD_JOBS:-}" ]; then
  is_count "$PADDOCK_CI_BUILD_JOBS" || { echo "::error::PADDOCK_CI_BUILD_JOBS must be a whole number >= 1, not '$PADDOCK_CI_BUILD_JOBS'"; exit 1; }
  emit "$PADDOCK_CI_BUILD_JOBS" "the runner's PADDOCK_CI_BUILD_JOBS"
  exit 0
fi

share="${PADDOCK_CI_JOBS_PER_HOST:-}"
src="the runner's PADDOCK_CI_JOBS_PER_HOST"
if [ -z "$share" ]; then share=$(default_share); src="the ${RUNNER_OS:-unknown} runner class's default"; fi
is_count "$share" || { echo "::error::PADDOCK_CI_JOBS_PER_HOST must be a whole number >= 1, not '$share'"; exit 1; }
reserved="${PADDOCK_CI_RESERVED_CORES:-}"
rsrc="the runner's PADDOCK_CI_RESERVED_CORES"
if [ -z "$reserved" ]; then reserved=$(default_reserved); rsrc="the ${RUNNER_OS:-unknown} runner class's default"; fi
case "$reserved" in '' | *[!0-9]*) echo "::error::PADDOCK_CI_RESERVED_CORES must be a whole number >= 0, not '$reserved'"; exit 1 ;; esac

n=$(cores)
if ! is_count "$n"; then
  # Unknown cores: leave cargo's own default (every core it can see) and say so; never fail a build over this.
  echo "::warning::could not count this host's cores; CARGO_BUILD_JOBS stays unset (cargo uses every core it sees)"
  exit 0
fi
jobs=$(((n - reserved) / share))
[ "$jobs" -ge 1 ] || jobs=1
emit "$jobs" "($n cores - $reserved reserved, $rsrc) / $share jobs per host, $src"

#!/usr/bin/env bash
# CARGO_BUILD_JOBS for a CI job: the one place CI sets it (ER-22). The build-jobs action runs it as an early step of a job
# (before job-isolation, which builds paddock-dev), and setup-rust runs it when no earlier step did, so every Rust job has it.
# A conformance test (conformance/test/ci-actions.test.mjs) fails when a workflow, action or CI script sets CARGO_BUILD_JOBS
# itself, passes a fixed -j/--jobs to cargo, or runs cargo in a job without build-jobs or setup-rust.
#
#   build-jobs.sh [<split>]   writes CARGO_BUILD_JOBS=<n> to $GITHUB_ENV (or prints it when GITHUB_ENV is unset), or leaves
#                             it unset where the runner class keeps cargo's own default; says why on stderr. <split>: how
#                             many builds this job runs at once (cargo mutants --jobs N), each getting 1/<split> of the share.
#
# The agents' local cap of 2 (ER-24) is for the shared developer Mac; a CI runner gets its share of the host:
#
#   1. PADDOCK_CI_BUILD_JOBS (a runner's runner.envs), a whole number >= 1: used as it is (divided by <split>).
#   2. Otherwise the smaller of
#        cores:  (the host's cores - PADDOCK_CI_RESERVED_CORES) / PADDOCK_CI_JOBS_PER_HOST
#        memory: (the host's RAM - PADDOCK_CI_RESERVED_RAM_MB) / (PADDOCK_CI_JOBS_PER_HOST x PADDOCK_CI_RUSTC_MB)
#      divided by <split>, at least 1. PADDOCK_CI_RUSTC_MB defaults to 1536: the Windows PC (32 GB) ran out of memory with
#      two builds of 16 rustc each (about 1 GB per rustc) and is fine with two of 8, so 1.5 GB per rustc keeps a margin.
#      Without the runner.envs, the runner class's known layout (verified 2026-10-30 from Gitea's job list: the most jobs
#      one runner ran at once over the last 100 runs):
#                 jobs/host  cores kept  RAM kept
#        Linux    -          -           -        d-gtr9, capacity 4, containers without a CPU limit. Left unset, as before
#                                                 (cargo: every core, per job), until a measured A/B of job durations
#                                                 decides the share (docs/benchmarks.md, Gaps); the log names the cores.
#        Windows  2          0           2 GB     decryp7ology-dev, two jobs at once; 16 cores, 32 GB: 8 each, as before
#        macOS    1          10          32 GB    d7-mac-mini, capacity 1, on the developer Mac its agents share: up to 5
#                                                 agents with CARGO_BUILD_JOBS=2 each (ER-24) keep 10 of its 14 cores, so
#                                                 CI gets 4, as before. PADDOCK_CI_RESERVED_CORES=1 when the Mac is CI's
#                                                 alone (13).
#      A Linux runner that sets PADDOCK_CI_JOBS_PER_HOST gets the formula (0 cores and 2 GB kept unless it sets them).
#
# Cores: nproc (Linux and Git Bash; honours the container's cpuset), else sysctl hw.ncpu (macOS), else NUMBER_OF_PROCESSORS,
# else getconf. RAM: the cgroup's memory.max when it is a number, else /proc/meminfo (Linux and Git Bash), else sysctl
# hw.memsize. PADDOCK_CI_CORES, PADDOCK_CI_RAM_MB and RUNNER_OS replace the probes in tests. Runs under macOS' bash 3.2.
set -euo pipefail

say() { echo "build-jobs: $*" >&2; }
fail() {
  echo "::error::$*"
  exit 1
}
# A whole number >= 1 / >= 0, written without leading zeros (bash reads 010 as octal 8 and 08 as an error).
is_count() { case "$1" in '' | *[!0-9]* | 0*) return 1 ;; *) return 0 ;; esac; }
is_amount() { [ "$1" = 0 ] || is_count "$1"; }

cores() {
  if [ -n "${PADDOCK_CI_CORES:-}" ]; then
    echo "$PADDOCK_CI_CORES"
    return
  fi
  local n=""
  n=$(nproc 2>/dev/null) || n=""
  is_count "$n" || n=$(sysctl -n hw.ncpu 2>/dev/null) || n=""
  is_count "$n" || n="${NUMBER_OF_PROCESSORS:-}"
  is_count "$n" || n=$(getconf _NPROCESSORS_ONLN 2>/dev/null) || n=""
  echo "$n"
}

ram_mb() {
  if [ -n "${PADDOCK_CI_RAM_MB:-}" ]; then
    echo "$PADDOCK_CI_RAM_MB"
    return
  fi
  local m=""
  if [ -r /sys/fs/cgroup/memory.max ]; then m=$(cat /sys/fs/cgroup/memory.max 2>/dev/null) || m=""; fi
  if is_count "$m"; then
    echo $((m / 1048576))
    return
  fi
  if [ -r /proc/meminfo ]; then
    m=$(awk '/^MemTotal:/ { print int($2 / 1024) }' /proc/meminfo 2>/dev/null) || m=""
    if is_count "$m"; then
      echo "$m"
      return
    fi
  fi
  m=$(sysctl -n hw.memsize 2>/dev/null) || m=""
  if is_count "$m"; then
    echo $((m / 1048576))
    return
  fi
  echo ""
}

# <variable> <class default>: the runner's value, else the default; "-" means the class leaves CARGO_BUILD_JOBS unset.
setting() {
  local v
  eval "v=\${$1:-}"
  if [ -n "$v" ]; then echo "$v"; else echo "$2"; fi
}
class_default() { # <share|cores|ram>
  case "${RUNNER_OS:-}:$1" in
    Linux:share) echo - ;;
    Windows:share) echo 2 ;;
    macOS:cores) echo 10 ;;
    macOS:ram) echo 32768 ;;
    *:share) echo 1 ;;
    *:cores) echo 0 ;;
    *:ram) echo 2048 ;;
  esac
}

emit() { # <jobs> <why>
  say "CARGO_BUILD_JOBS=$1 ($2)"
  if [ -n "${GITHUB_ENV:-}" ]; then echo "CARGO_BUILD_JOBS=$1" >>"$GITHUB_ENV"; else echo "CARGO_BUILD_JOBS=$1"; fi
}

split="${1:-1}"
is_count "$split" || fail "the split (builds at once in this job) must be a whole number >= 1, not '$split'"
n=$(cores)
mem=$(ram_mb)
say "this host: ${n:-unknown} cores, ${mem:-unknown} MB of RAM, runner class ${RUNNER_OS:-unknown}"

if [ -n "${PADDOCK_CI_BUILD_JOBS:-}" ]; then
  is_count "$PADDOCK_CI_BUILD_JOBS" || fail "PADDOCK_CI_BUILD_JOBS must be a whole number >= 1, not '$PADDOCK_CI_BUILD_JOBS'"
  jobs=$((PADDOCK_CI_BUILD_JOBS / split))
  [ "$jobs" -ge 1 ] || jobs=1
  emit "$jobs" "the runner's PADDOCK_CI_BUILD_JOBS / $split builds at once"
  exit 0
fi

share=$(setting PADDOCK_CI_JOBS_PER_HOST "$(class_default share)")
if [ "$share" = - ]; then
  # Linux keeps cargo's default (every core per job) until its share is measured. A split job (cargo mutants) still divides
  # the cores between its builds.
  if [ "$split" -gt 1 ] && is_count "$n"; then
    jobs=$((n / split))
    [ "$jobs" -ge 1 ] || jobs=1
    emit "$jobs" "$n cores / $split builds at once; otherwise the runner class keeps cargo's default"
  else
    say "CARGO_BUILD_JOBS stays unset: cargo uses all ${n:-its} cores, as before (the Linux share is not measured yet)"
  fi
  exit 0
fi
is_count "$share" || fail "PADDOCK_CI_JOBS_PER_HOST must be a whole number >= 1, not '$share'"
reserved=$(setting PADDOCK_CI_RESERVED_CORES "$(class_default cores)")
is_amount "$reserved" || fail "PADDOCK_CI_RESERVED_CORES must be a whole number >= 0 without leading zeros, not '$reserved'"
ram_kept=$(setting PADDOCK_CI_RESERVED_RAM_MB "$(class_default ram)")
is_amount "$ram_kept" || fail "PADDOCK_CI_RESERVED_RAM_MB must be a whole number >= 0 without leading zeros, not '$ram_kept'"
per_rustc=$(setting PADDOCK_CI_RUSTC_MB 1536)
is_count "$per_rustc" || fail "PADDOCK_CI_RUSTC_MB must be a whole number >= 1, not '$per_rustc'"

if ! is_count "$n"; then
  # Unknown cores: leave cargo's own default and say so; never fail a build over this.
  echo "::warning::could not count this host's cores; CARGO_BUILD_JOBS stays unset (cargo uses every core it sees)"
  exit 0
fi
if [ "$reserved" -ge "$n" ]; then
  echo "::warning::PADDOCK_CI_RESERVED_CORES ($reserved) keeps all of this host's $n cores; the job gets 1 build job"
fi
jobs=$(((n - reserved) / share))
why="($n cores - $reserved kept) / $share jobs per host"
if is_count "$mem"; then
  by_ram=$(((mem - ram_kept) / (share * per_rustc)))
  if [ "$by_ram" -lt "$jobs" ]; then
    jobs=$by_ram
    why="($mem MB - $ram_kept MB kept) / ($share jobs per host x $per_rustc MB per rustc)"
  fi
else
  say "RAM unknown: no memory ceiling"
fi
jobs=$((jobs / split))
[ "$jobs" -ge 1 ] || jobs=1
emit "$jobs" "$why / $split builds at once"

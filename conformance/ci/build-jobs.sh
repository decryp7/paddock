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
#      divided by <split>, at least 1. PADDOCK_CI_RUSTC_MB defaults to 1536, an estimate (about 1 GB per debug rustc plus
#      a margin), not a measurement: the A/B in docs/benchmarks.md (Gaps) is to measure it.
#      Without the runner.envs, the runner class's known layout (verified 2026-10-30 from Gitea's job list: the most jobs
#      one runner ran at once over the last 100 runs):
#                 jobs/host  cores kept  RAM kept
#        Linux    -          -           -        d-gtr9, capacity 4, 16 cores, 128 GB, containers without a CPU limit.
#                                                 Left unset, as before (cargo: every core, per job), until a measured
#                                                 A/B of job durations decides the share (docs/benchmarks.md, Gaps).
#        Windows  -          -           -        decryp7ology-dev, two jobs at once; 8 cores, 16 GB (its job log of
#                                                 2026-10-30). Left unset: cargo's default is its 8 cores, the 8 the
#                                                 workflows spelled before; the share (4 each) waits for the same A/B.
#        macOS    3          2           32 GB    d7-mac-mini, -2 and -3 (2026-11-08), three runners of capacity 1 on the
#                                                 developer Mac its agents share: (14 - 2) / 3 = 4 build jobs per job,
#                                                 the 4 each job had before, and 8 test threads (below). Measured
#                                                 2026-11-09 (docs/benchmarks.md, "CI share on the shared Mac"): 23 busy
#                                                 threads besides a CPU-bound test (3 x 4 rustc plus 10 for the agents)
#                                                 slowed it from 2.7 to 4.0 s; the 60 s timeouts of run 7775 came from
#                                                 all-core benchmarks in the same job, not from the share.
#
# NEXTEST_TEST_THREADS, written with CARGO_BUILD_JOBS and divided by <split>: a runner's PADDOCK_CI_TEST_THREADS
# (runner.envs), else the class's measured number (macOS 8: the tests mostly wait, so the 4 cores of the share made a dyno
# run 2.3 times longer than 14 threads, and 8 matched 14; docs/benchmarks.md, "CI share on the shared Mac"), else the job's
# core share ((cores - kept) / jobs per host, not capped by RAM: that ceiling is per rustc; PADDOCK_CI_BUILD_JOBS stands in
# for it). Left unset where CARGO_BUILD_JOBS is (nextest then runs the profile's num-cpus).
#      A Linux or Windows runner that sets PADDOCK_CI_JOBS_PER_HOST gets the formula (0 cores and 2 GB kept unless set).
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
    Windows:share) echo - ;;
    macOS:threads) echo 8 ;;
    macOS:share) echo 3 ;;
    macOS:cores) echo 2 ;;
    macOS:ram) echo 32768 ;;
    *:threads) echo - ;;
    *:share) echo 1 ;;
    *:cores) echo 0 ;;
    *:ram) echo 2048 ;;
  esac
}

put() { # <name> <value> <why>
  say "$1=$2 ($3)"
  if [ -n "${GITHUB_ENV:-}" ]; then echo "$1=$2" >>"$GITHUB_ENV"; else echo "$1=$2"; fi
}
emit() { put CARGO_BUILD_JOBS "$@"; }
# emit_threads <threads> <why>: nextest's test threads (NEXTEST_TEST_THREADS overrides the profile's num-cpus): the
# runner's PADDOCK_CI_TEST_THREADS, else the class's measured number, else the given share; divided by the split.
emit_threads() {
  local t="$1" why="$2" class
  class="$(class_default threads)"
  if [ -n "${PADDOCK_CI_TEST_THREADS:-}" ]; then
    t=$((PADDOCK_CI_TEST_THREADS / split))
    why="the runner's PADDOCK_CI_TEST_THREADS / $split test runs at once"
  elif [ "$class" != - ]; then
    t=$((class / split))
    why="the ${RUNNER_OS} class's measured test threads / $split test runs at once"
  fi
  [ "$t" -ge 1 ] || t=1
  put NEXTEST_TEST_THREADS "$t" "$why"
}

split="${1:-1}"
is_count "$split" || fail "the split (builds at once in this job) must be a whole number >= 1, not '$split'"
if [ -n "${PADDOCK_CI_TEST_THREADS:-}" ]; then
  is_count "$PADDOCK_CI_TEST_THREADS" || fail "PADDOCK_CI_TEST_THREADS must be a whole number >= 1, not '$PADDOCK_CI_TEST_THREADS'"
fi
n=$(cores)
mem=$(ram_mb)
say "this host: ${n:-unknown} cores, ${mem:-unknown} MB of RAM, runner class ${RUNNER_OS:-unknown}"

if [ -n "${PADDOCK_CI_BUILD_JOBS:-}" ]; then
  is_count "$PADDOCK_CI_BUILD_JOBS" || fail "PADDOCK_CI_BUILD_JOBS must be a whole number >= 1, not '$PADDOCK_CI_BUILD_JOBS'"
  jobs=$((PADDOCK_CI_BUILD_JOBS / split))
  [ "$jobs" -ge 1 ] || jobs=1
  emit "$jobs" "the runner's PADDOCK_CI_BUILD_JOBS / $split builds at once"
  emit_threads "$jobs" "the runner's PADDOCK_CI_BUILD_JOBS / $split test runs at once"
  exit 0
fi

share=$(setting PADDOCK_CI_JOBS_PER_HOST "$(class_default share)")
if [ "$share" = - ]; then
  # Linux and Windows keep cargo's default (every core per job) until their share is measured. A split job (cargo mutants)
  # still divides the cores between its builds.
  if [ "$split" -gt 1 ] && is_count "$n"; then
    jobs=$((n / split))
    [ "$jobs" -ge 1 ] || jobs=1
    emit "$jobs" "$n cores / $split builds at once; otherwise the runner class keeps cargo's default"
  else
    say "CARGO_BUILD_JOBS stays unset: cargo uses all ${n:-its} cores, as before (this runner class's share is not measured yet)"
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
# The tests' share is the cores alone: the memory ceiling below is per rustc, not per test.
threads=$((jobs / split))
threads_why="$why / $split test runs at once"
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
emit_threads "$threads" "$threads_why"

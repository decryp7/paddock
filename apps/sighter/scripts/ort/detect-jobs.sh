#!/usr/bin/env bash
# Prints the number of parallel C++ compile jobs for an ONNX Runtime build on this runner (rule 51 / ER-20), detected at
# run time; no machine is assumed:
#   - CPU-bound, so the usable cores minus one; in a Linux container the cgroup CPU quota (v2 cpu.max, v1
#     cpu.cfs_quota_us / cpu.cfs_period_us) caps the cores nproc reports;
#   - at most one job per 2 GB of the memory available now, after 2 GB kept for the linker and the rest of the system
#     (the largest translation units, MLAS and the contrib ops, need well over 1 GB each). Linux: MemAvailable, capped by
#     the cgroup memory limit (v2 memory.max, v1 memory.limit_in_bytes); Windows: free physical memory; macOS: total
#     memory minus 4 GB (macOS keeps most memory in caches, so "free" says little there).
# ORT_BUILD_JOBS, when set, wins. For tests, DETECT_CORES and DETECT_MEM_MB replace the measurements.
#
#   apps/sighter/scripts/ort/detect-jobs.sh
set -euo pipefail

if [ -n "${ORT_BUILD_JOBS:-}" ]; then
  case "$ORT_BUILD_JOBS" in ''|*[!0-9]*|0) echo "detect-jobs: ORT_BUILD_JOBS must be a positive number" >&2; exit 2 ;; esac
  echo "$ORT_BUILD_JOBS"
  exit 0
fi

num() { case "${1:-}" in ''|*[!0-9]*) return 1 ;; *) return 0 ;; esac; }
mb_of_bytes() { echo $(( $1 / 1048576 )); }

cores=${DETECT_CORES:-}
mem_mb=${DETECT_MEM_MB:-}
if [ -z "$cores" ] || [ -z "$mem_mb" ]; then
  case "$(uname -s)" in
    Darwin)
      c=$(sysctl -n hw.ncpu)
      m=$(( $(mb_of_bytes "$(sysctl -n hw.memsize)") - 2048 )) ;;
    Linux)
      c=$(nproc)
      quota=""; period=""
      if [ -r /sys/fs/cgroup/cpu.max ]; then
        read -r quota period </sys/fs/cgroup/cpu.max || true
      elif [ -r /sys/fs/cgroup/cpu/cpu.cfs_quota_us ]; then
        quota=$(cat /sys/fs/cgroup/cpu/cpu.cfs_quota_us); period=$(cat /sys/fs/cgroup/cpu/cpu.cfs_period_us)
      fi
      if num "$quota" && num "$period" && [ "$period" -gt 0 ]; then
        q=$(( (quota + period - 1) / period ))
        [ "$q" -ge 1 ] && [ "$q" -lt "$c" ] && c=$q
      fi
      m=$(( $(awk '/MemAvailable/ {print $2}' /proc/meminfo) / 1024 ))
      for f in /sys/fs/cgroup/memory.max /sys/fs/cgroup/memory/memory.limit_in_bytes; do
        if [ -r "$f" ]; then
          l=$(cat "$f")
          if num "$l"; then l=$(mb_of_bytes "$l"); [ "$l" -lt "$m" ] && m=$l; fi
          break
        fi
      done ;;
    MINGW* | MSYS* | CYGWIN*)
      c=${NUMBER_OF_PROCESSORS:-1}
      # FreePhysicalMemory is in KiB.
      m=$(( $(powershell -NoProfile -Command '(Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory' | tr -d '\r') / 1024 )) ;;
    *) echo "detect-jobs: unknown OS $(uname -s)" >&2; exit 2 ;;
  esac
  cores=${cores:-$c}
  mem_mb=${mem_mb:-$m}
fi

jobs=$(( cores > 1 ? cores - 1 : 1 ))
by_mem=$(( (mem_mb - 2048) / 2048 ))
[ "$by_mem" -lt 1 ] && by_mem=1
[ "$by_mem" -lt "$jobs" ] && jobs=$by_mem
echo "$jobs"

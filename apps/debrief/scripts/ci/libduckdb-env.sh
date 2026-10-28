#!/usr/bin/env bash
# Puts the pinned libduckdb on the CI job's paths through $GITHUB_ENV (DUCKDB_LIB_DIR, DUCKDB_INCLUDE_DIR and the loader path:
# LD_LIBRARY_PATH on Linux, DYLD_LIBRARY_PATH on macOS, PATH on Windows), for every job that builds or runs the workspace.
#
#   bash scripts/ci/libduckdb-env.sh
#
# In the paddock-ci image the library is already in DUCKDB_LIB_DIR (/opt/libduckdb): it is used when its header has the
# version crates/debrief-store-duck/scripts/fetch-libduckdb.sh pins. Otherwise the pinned, SHA-256-checked download is
# fetched by that script, into a folder that outlives the job on a host runner (${PADDOCK_CI_HOME:-$HOME/.paddock-ci}/
# libduckdb/<job>: one per job, so two jobs on one host never write the same archive) and into the job's temp folder in a
# container. Set on the loader path through GITHUB_ENV, never under `nice`/`env`, which macOS's SIP strips DYLD_* from.
set -euo pipefail
cd "$(dirname "$0")/../.."
fetch=${DEBRIEF_FETCH_LIBDUCKDB:-crates/debrief-store-duck/scripts/fetch-libduckdb.sh}
want=$(sed -n 's/^VERSION=//p' "$fetch" | head -n 1)
[ -n "$want" ] || { echo "::error::no VERSION= line in $fetch"; exit 1; }
out=${GITHUB_ENV:?GITHUB_ENV is not set}
# The loader paths as the job has them: fetch-libduckdb.sh's exports prepend to them too, and are not what is written.
ld_before=${LD_LIBRARY_PATH:-}
dyld_before=${DYLD_LIBRARY_PATH:-}

# MAJOR.MINOR.PATCH of the library: duckdb_v2.h (DUCKDB_V2_API_VERSION_*, DuckDB 2.x; its duckdb.h still says 1.5.6, the
# stable C API's version), else duckdb.h (DUCKDB_API_VERSION_*, DuckDB 1.x), or nothing.
header_version() {
  local dir="$1" h prefix v=
  if [ -f "$dir/duckdb_v2.h" ]; then h="$dir/duckdb_v2.h" prefix=DUCKDB_V2_API_VERSION
  elif [ -f "$dir/duckdb.h" ]; then h="$dir/duckdb.h" prefix=DUCKDB_API_VERSION
  else return 0; fi
  for part in MAJOR MINOR PATCH; do
    v="$v${v:+.}$(sed -n "s/^#define ${prefix}_$part \([0-9][0-9]*\).*/\1/p" "$h" | head -n 1)"
  done
  echo "$v"
}

image=${DUCKDB_LIB_DIR:-}
if [ -n "${PADDOCK_CI_IMAGE:-}" ] && [ -n "$image" ] && [ "$(header_version "$image")" = "$want" ]; then
  echo "libduckdb $want from the CI image: $image"
  lib=$image
  loader=$image
else
  if [ -n "${PADDOCK_CI_IMAGE:-}" ]; then echo "::notice::the CI image's libduckdb is not $want; fetching the pinned download."; fi
  if [ "${RUNNER_OS:?}" = Linux ]; then keep="${RUNNER_TEMP:-/tmp}/libduckdb"; else keep="${PADDOCK_CI_HOME:-$HOME/.paddock-ci}/libduckdb/${GITHUB_JOB:-job}"; fi
  # The script prints `export DUCKDB_LIB_DIR=... DUCKDB_INCLUDE_DIR=...` and the loader path.
  eval "$(bash "$fetch" "$keep")"
  lib=${DUCKDB_LIB_DIR:?$fetch did not set DUCKDB_LIB_DIR}
  loader=$lib
  echo "libduckdb $want: $lib"
fi

{
  echo "DUCKDB_LIB_DIR=$lib"
  echo "DUCKDB_INCLUDE_DIR=${DUCKDB_INCLUDE_DIR:-$lib}"
  case "$RUNNER_OS" in
    Linux) echo "LD_LIBRARY_PATH=$loader${ld_before:+:$ld_before}" ;;
    macOS) echo "DYLD_LIBRARY_PATH=$loader${dyld_before:+:$dyld_before}" ;;
  esac
} >> "$out"
if [ "$RUNNER_OS" = Windows ]; then cygpath -w "$loader" >> "${GITHUB_PATH:?GITHUB_PATH is not set}"; fi

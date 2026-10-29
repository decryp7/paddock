#!/usr/bin/env bash
# Moves the downloads an ONNX Runtime build may reuse between a job's work folder and a cache folder at a stable path in
# the checkout (apps/sighter/.ort-cache), which actions/cache saves and restores. The work folder's own path differs per
# run and per runner, so caching it directly would never hit.
#
#   apps/sighter/scripts/ort/cache-sync.sh restore <work> <cache>
#   apps/sighter/scripts/ort/cache-sync.sh save <work> <cache>
#
# What moves (everything is checked again when used, so a cache can only save time, never change a build):
#   - the third-party source archives CMake's FetchContent downloads, at their place in the build tree
#     (build/<variant>/Release/_deps/<name>-subbuild/<name>-populate-prefix/src/<archive>); CMake checks each against
#     its SHA-1 from cmake/deps.txt before use;
#   - the DirectML NuGet package (cache/directml/<version>/*.nupkg); fetch-directml.sh checks its size and NuGet SHA-512
#     on every run.
# The ONNX Runtime source tree is not moved here: build-ort.sh keeps it through ORT_SOURCE_CACHE, only on runs that do not
# publish, and checks it with check-source.sh.
set -euo pipefail

mode=${1:-}; work=${2:-}; cache=${3:-}
if [ -z "$work" ] || [ -z "$cache" ] || { [ "$mode" != restore ] && [ "$mode" != save ]; }; then
  echo "usage: cache-sync.sh <restore|save> <work> <cache>" >&2
  exit 2
fi

# copy_files <from> <to> <find predicates...>: every matching file under <from> to the same relative path under <to>.
copy_files() {
  local from=$1 to=$2 n=0 rel
  shift 2
  [ -d "$from" ] || { echo 0; return; }
  while IFS= read -r -d '' f; do
    rel=${f#"$from"/}
    mkdir -p "$to/$(dirname "$rel")"
    cp -p "$f" "$to/$rel"
    n=$((n + 1))
  done < <(find "$from" -type f "$@" -print0)
  echo "$n"
}

archives=( -path '*/_deps/*-subbuild/*-populate-prefix/src/*' \( -name '*.zip' -o -name '*.tar.gz' -o -name '*.tgz' -o -name '*.tar.xz' -o -name '*.tar.bz2' \) )
nupkg=( -path '*/directml/*' -name '*.nupkg' )
if [ "$mode" = restore ]; then
  a=$(copy_files "$cache/deps" "$work/build" "${archives[@]}")
  d=$(copy_files "$cache/directml" "$work/cache/directml" "${nupkg[@]}")
else
  a=$(copy_files "$work/build" "$cache/deps" "${archives[@]}")
  d=$(copy_files "$work/cache/directml" "$cache/directml" "${nupkg[@]}")
fi
echo "cache-sync: $mode: $a source archive(s), $d DirectML package(s)"

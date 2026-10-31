#!/usr/bin/env bash
# Checks an ONNX Runtime build tree for telemetry as CMake configured it: onnxruntime_USE_TELEMETRY and
# onnxruntime_USE_WINDOWS_TELEMETRY must be OFF in CMakeCache.txt, and no compile command may define USE_1DS_TELEMETRY or
# USE_WINDOWS_TELEMETRY. 1DS is not a cache entry: ONNX Runtime's CMakeLists.txt sets onnxruntime_USE_1DS_TELEMETRY only
# when onnxruntime_USE_TELEMETRY is on, and then compiles with USE_1DS_TELEMETRY. Generator files read: flags.make
# (Makefiles), build.ninja (Ninja), *.vcxproj (Visual Studio); third-party trees (_deps) are skipped.
#
#   apps/sighter/scripts/ort/check-cmake-telemetry.sh <build>/Release
#
# Prints one line for BUILD-INFO.txt and exits 0, or names the problem and exits 1 (2 when the tree has no cache).
set -euo pipefail

dir=${1:?usage: check-cmake-telemetry.sh <build>/Release}
cache="$dir/CMakeCache.txt"
[ -f "$cache" ] || { echo "check-cmake-telemetry: $cache is missing" >&2; exit 2; }
value() { sed -n "s/^$1:[A-Z]*=//p" "$cache" | tr -d '\r'; }
tel=$(value onnxruntime_USE_TELEMETRY)
wintel=$(value onnxruntime_USE_WINDOWS_TELEMETRY)
status=0
[ "$tel" = OFF ] || { echo "check-cmake-telemetry: onnxruntime_USE_TELEMETRY is '$tel', expected OFF" >&2; status=1; }
[ "$wintel" = OFF ] || { echo "check-cmake-telemetry: onnxruntime_USE_WINDOWS_TELEMETRY is '$wintel', expected OFF" >&2; status=1; }
defined=$(find "$dir" \( -name flags.make -o -name build.ninja -o -name '*.vcxproj' \) -not -path '*/_deps/*' -print0 \
  | xargs -0 grep -l -E '(^|[^A-Za-z0-9_]|[-/]D)USE_(1DS|WINDOWS)_TELEMETRY([^A-Za-z0-9_]|$)' 2>/dev/null || true)
if [ -n "$defined" ]; then
  echo "check-cmake-telemetry: telemetry is compiled in by:" >&2
  echo "$defined" >&2
  status=1
fi
[ "$status" = 0 ] && echo "onnxruntime_USE_TELEMETRY=$tel onnxruntime_USE_WINDOWS_TELEMETRY=$wintel; USE_1DS_TELEMETRY and USE_WINDOWS_TELEMETRY in no compile flags"
exit $status

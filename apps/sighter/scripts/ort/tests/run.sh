#!/usr/bin/env bash
# Tests of Sighter's ONNX Runtime pipeline scripts, on synthetic fixtures only (no build, no network, no real package
# server): check-catalogue.sh, check-cmake-telemetry.sh, detect-jobs.sh and publish-ort.sh (against mock-packages.mjs on
# 127.0.0.1). The byte and import checks have their own tests in conformance/test (telemetry-bytes, windows-crt-imports).
#
#   apps/sighter/scripts/ort/tests/run.sh
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
ort=$(cd "$here/.." && pwd)
fx="$here/fixtures"
scratch=$(mktemp -d)
server=""
cleanup() { if [ -n "$server" ]; then kill "$server" 2>/dev/null || true; wait "$server" 2>/dev/null || true; fi; rm -rf "$scratch"; }
trap cleanup EXIT

failures=0
pass=0
# expect <exit code> <description> <command...>: runs the command, output kept in $scratch/out.
expect() {
  local want=$1 what=$2 got=0
  shift 2
  "$@" >"$scratch/out" 2>&1 || got=$?
  if [ "$got" = "$want" ]; then
    pass=$((pass + 1))
  else
    failures=$((failures + 1))
    echo "FAIL: $what: exit $got, expected $want"
    sed 's/^/  | /' "$scratch/out"
  fi
}
# output_has <description> <fixed string>: the last command's output contains it.
output_has() {
  if grep -q -F -- "$2" "$scratch/out"; then pass=$((pass + 1)); else failures=$((failures + 1)); echo "FAIL: $1: no '$2' in:"; sed 's/^/  | /' "$scratch/out"; fi
}

# ---- check-catalogue.sh
expect 1 "a placeholder pin fails" "$ort/check-catalogue.sh" "$fx/placeholder.toml"
output_has "it names the placeholder" "1 placeholder pin(s)"
expect 0 "a pinned catalogue passes" "$ort/check-catalogue.sh" "$fx/pinned.toml"
expect 1 "one placeholder among several fails" "$ort/check-catalogue.sh" "$fx/pinned.toml" "$fx/placeholder.toml"

# ---- check-cmake-telemetry.sh
expect 0 "telemetry off passes" "$ort/check-cmake-telemetry.sh" "$fx/cmake-off"
output_has "the BUILD-INFO line" "onnxruntime_USE_TELEMETRY=OFF onnxruntime_USE_WINDOWS_TELEMETRY=OFF"
expect 1 "onnxruntime_USE_TELEMETRY=ON fails" "$ort/check-cmake-telemetry.sh" "$fx/cmake-on"
expect 1 "a USE_1DS_TELEMETRY define fails" "$ort/check-cmake-telemetry.sh" "$fx/cmake-1ds"
output_has "it names the file" "flags.make"
expect 1 "a USE_WINDOWS_TELEMETRY define in a Visual Studio project fails" "$ort/check-cmake-telemetry.sh" "$fx/cmake-wintel"
expect 0 "third-party trees (_deps) are not read" "$ort/check-cmake-telemetry.sh" "$fx/cmake-deps"
expect 2 "a tree without CMakeCache.txt" "$ort/check-cmake-telemetry.sh" "$fx"

# ---- detect-jobs.sh: cores minus one, one job per 2 GB after 2 GB kept back, at least one; the override wins.
jobs_for() { env -u ORT_BUILD_JOBS DETECT_CORES="$1" DETECT_MEM_MB="$2" "$ort/detect-jobs.sh"; }
for row in "8 16384 7" "8 12288 5" "8 8192 3" "32 8192 3" "1 1000 1" "4 1024 1" "16 65536 15"; do
  read -r -a f <<<"$row"; set -- "${f[@]}"
  expect 0 "detect-jobs $1 cores $2 MB" jobs_for "$1" "$2"
  output_has "detect-jobs $1 cores $2 MB gives $3" "$3"
  [ "$(cat "$scratch/out")" = "$3" ] || { failures=$((failures + 1)); echo "FAIL: detect-jobs $1 $2: $(cat "$scratch/out"), expected $3"; }
done
expect 0 "ORT_BUILD_JOBS wins" env ORT_BUILD_JOBS=4 DETECT_CORES=64 DETECT_MEM_MB=999999 "$ort/detect-jobs.sh"
output_has "ORT_BUILD_JOBS=4" "4"
expect 2 "ORT_BUILD_JOBS=0 is refused" env ORT_BUILD_JOBS=0 "$ort/detect-jobs.sh"
expect 0 "this runner gives a number" "$ort/detect-jobs.sh"

# ---- publish-ort.sh against a local stand-in for Gitea.
command -v node >/dev/null || { echo "tests need node"; exit 2; }
node "$here/mock-packages.mjs" >"$scratch/port" &
server=$!
for _ in $(seq 50); do [ -s "$scratch/port" ] && break; sleep 0.1; done
[ -s "$scratch/port" ] || { echo "the mock package server did not start"; exit 2; }
base="http://127.0.0.1:$(cat "$scratch/port")"
# shellcheck source-path=SCRIPTDIR source=../pins.env
. "$ort/pins.env"
name="onnxruntime-$ORT_VERSION-p$PIPELINE_REV-linux-x64-cpu"
make_archive() {
  rm -rf "$scratch/out-dir" && mkdir -p "$scratch/out-dir/artifact"
  printf '%s' "$1" >"$scratch/out-dir/artifact/$name.tar.gz"
  local sum
  sum=$(cd "$scratch/out-dir/artifact" && { sha256sum "$name.tar.gz" 2>/dev/null || shasum -a 256 "$name.tar.gz"; } | cut -d' ' -f1)
  echo "$sum  $name.tar.gz" >"$scratch/out-dir/artifact/$name.tar.gz.sha256"
  printf '[[platform]]\nkey = "linux-x86_64"\n' >"$scratch/out-dir/artifact/$name.catalogue.txt"
}
publish() { env PACKAGE_TOKEN=test-token PUBLISH_ORT_BASE_URL="$base" "$ort/publish-ort.sh" "$scratch/out-dir"; }

make_archive "synthetic archive one"
expect 0 "a first publish uploads the archive and its .sha256" publish
output_has "it prints the catalogue lines" 'key = "linux-x86_64"'
expect 0 "publishing the same file again is success (409, same content)" publish
output_has "it says so" "already published with the same content"
make_archive "synthetic archive two"
expect 1 "a different file under the same version fails (409, other content)" publish
output_has "it says to bump PIPELINE_REV" "bump PIPELINE_REV"
make_archive "synthetic archive three"
echo "0000000000000000000000000000000000000000000000000000000000000000  $name.tar.gz" >"$scratch/out-dir/artifact/$name.tar.gz.sha256"
expect 1 "an archive that does not match its .sha256 is never uploaded" publish
output_has "it names the mismatch" "its .sha256 says"
expect 1 "a wrong token fails" env PACKAGE_TOKEN=wrong PUBLISH_ORT_BASE_URL="$base" "$ort/publish-ort.sh" "$scratch/out-dir"
expect 1 "no token fails" env -u PACKAGE_TOKEN PUBLISH_ORT_BASE_URL="$base" "$ort/publish-ort.sh" "$scratch/out-dir"
rm -rf "$scratch/out-dir" && mkdir -p "$scratch/out-dir"
expect 1 "nothing to publish fails" publish

echo "ort script tests: $pass passed, $failures failed"
[ "$failures" = 0 ]

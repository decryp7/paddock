#!/usr/bin/env bash
# Builds one pinned ONNX Runtime variant for Sighter (stage-2 plan SW5, ADR 0001 decision 4 and P5): telemetry compiled
# out, static, Release, tests skipped; then merges the static libraries into one archive that `ort` links through
# ORT_LIB_PATH, checks it for telemetry and packs it with its notices and build record.
#
#   apps/sighter/scripts/ort/build-ort.sh <variant> <work-dir> [--jobs N] [--dry-run]
#
# Variants (catalogue file and key in parentheses; catalogue/README.md):
#   macos-arm64-coreml     macOS arm64 with the CoreML EP                (onnxruntime,     macos-aarch64)
#   windows-x64-directml   Windows x64 with the DirectML EP, static CRT  (onnxruntime,     windows-x86_64)
#   windows-x64-cpu        Windows x64, CPU only, static CRT             (onnxruntime-cpu, windows-x86_64)
#   linux-x64-cpu          Linux x64, CPU only, for CI and tests         (onnxruntime,     linux-x86_64)
#
# Each runs on its own OS (no cross builds). A full build is long (docs/research/2026-10-ort-builds.md, "Build times"):
# run it from .gitea/workflows/sighter-ort.yml, never in an app's pipeline. --dry-run prints the plan and the build
# command without fetching or compiling anything. Compile jobs: detect-jobs.sh (ORT_BUILD_JOBS or --jobs override).
# ORT_USE_CCACHE=1 compiles through ccache (Linux and macOS, when ccache is installed; never for a published build).
# Needs Node (the checks are conformance/release/*.mjs), Python 3.10+ for ONNX Runtime's build.py, CMake 3.28+, Git.
# On Windows the work folder must be short (at most 40 characters): ONNX Runtime's build tree is deep and MSVC tools
# still stop at 260-character paths.
#
# Output in <work-dir>/out: onnxruntime-<ver>-p<rev>-<variant>.tar.gz, its .sha256, and catalogue.txt (the catalogue lines
# for that archive). The archive holds:
#   lib/libonnxruntime.a or lib/onnxruntime.lib   everything ONNX Runtime needs, merged
#   include/                                      the C API headers
#   link.txt                                      system libraries the consumer adds (ort-sys adds them only for pyke's
#                                                 archives), and the DLLs to delay-load; read by sighter-infer's build.rs
#   LICENSE, ThirdPartyNotices.txt                ONNX Runtime's (MIT) and its dependencies' notices
#   BUILD-INFO.txt                                version, commit, flags, toolchain, the telemetry switches as CMake
#                                                 recorded them: the cache key
# DirectML.dll is not in the archive: fetch-directml.sh supplies it (pinned from NuGet), and the DirectML gate (P6) decides
# whether it ships.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=pins.env
. "$here/pins.env"

usage() { sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
variant=${1:-}; work=${2:-}
[ -n "$variant" ] && [ -n "$work" ] || usage
shift 2
jobs=auto
dry=0
while [ $# -gt 0 ]; do
  case "$1" in
    --jobs) jobs=${2:?--jobs needs a number}; shift 2 ;;
    --dry-run) dry=1; shift ;;
    *) usage ;;
  esac
done

host="$(uname -s)-$(uname -m)"
extra=()
dml_arch=""
case "$variant" in
  macos-arm64-coreml)
    [[ $host == Darwin-arm64 ]] || { echo "build-ort: $variant builds on macOS arm64 only (this is $host)" >&2; exit 2; }
    extra=(--use_coreml --osx_arch arm64 --apple_deploy_target "$MACOS_DEPLOYMENT_TARGET")
    lib_name=libonnxruntime.a ;;
  windows-x64-directml)
    [[ $host == MINGW*-x86_64 || $host == MSYS*-x86_64 ]] || { echo "build-ort: $variant builds on Windows x64 only (this is $host)" >&2; exit 2; }
    dml_arch=x64
    extra=(--use_dml --enable_msvc_static_runtime)
    lib_name=onnxruntime.lib ;;
  windows-x64-cpu)
    [[ $host == MINGW*-x86_64 || $host == MSYS*-x86_64 ]] || { echo "build-ort: $variant builds on Windows x64 only (this is $host)" >&2; exit 2; }
    extra=(--enable_msvc_static_runtime)
    lib_name=onnxruntime.lib ;;
  linux-x64-cpu)
    [[ $host == Linux-x86_64 ]] || { echo "build-ort: $variant builds on Linux x64 only (this is $host)" >&2; exit 2; }
    lib_name=libonnxruntime.a ;;
  *) echo "build-ort: unknown variant '$variant'" >&2; usage ;;
esac

[ "$jobs" = auto ] && jobs=$("$here/detect-jobs.sh")

# Repository root (the shared checks live in conformance/release).
root=$(cd "$here/../../../.." && pwd)

# ONNX Runtime's build.py needs Python 3.10+. On Windows `python3` may be the Store's placeholder that only opens a
# window, so each candidate must answer --version with a 3.10+ version.
python=()
for cand in python3 python "py -3"; do
  # shellcheck disable=SC2086
  if v=$($cand -c 'import sys; print("%d.%d" % sys.version_info[:2])' 2>/dev/null) && [ "${v%%.*}" -eq 3 ] && [ "${v#*.}" -ge 10 ]; then
    read -r -a python <<<"$cand"
    break
  fi
done
[ ${#python[@]} -gt 0 ] || { echo "build-ort: no Python 3.10 or later found (python3, python, py -3)" >&2; exit 1; }

case "$host" in
  MINGW* | MSYS*)
    if [ ${#work} -gt 40 ]; then
      echo "build-ort: the work folder '$work' is ${#work} characters; use one of at most 40 (deep build paths fail on Windows)" >&2
      exit 2
    fi ;;
esac

ccache_args=()
if [ "${ORT_USE_CCACHE:-0}" = 1 ]; then
  case "$host" in
    MINGW* | MSYS*) echo "build-ort: ccache is not used on Windows (Visual Studio generator)" ;;
    *) if command -v ccache >/dev/null 2>&1; then ccache_args=(--use_cache); else echo "build-ort: ORT_USE_CCACHE=1 but no ccache; building without it"; fi ;;
  esac
fi

pkg_version="$ORT_VERSION-p$PIPELINE_REV"
name="onnxruntime-$pkg_version-$variant"
src="$work/src/onnxruntime-$ORT_VERSION"
build="$work/build/$variant"
stage="$work/stage/$name"
out="$work/out"
cmake_defines=(
  onnxruntime_BUILD_UNIT_TESTS=OFF
  # Dependencies come from cmake/deps.txt (URLs pinned by SHA-1), never from whatever the runner has installed.
  FETCHCONTENT_TRY_FIND_PACKAGE_MODE=NEVER
)
build_cmd=("${python[@]}" tools/ci_build/build.py --build_dir "$build" --config Release --update --build --parallel "$jobs"
  --skip_tests --compile_no_warning_as_error --no_telemetry ${extra[@]+"${extra[@]}"} ${ccache_args[@]+"${ccache_args[@]}"})

echo "build-ort: $name, $jobs compile jobs, work folder $work"
if [ "$dry" = 1 ]; then
  [ -n "$dml_arch" ] && echo "  fetch-directml.sh $dml_arch $work/cache  (then --dml_path <its folder>)"
  echo "  git clone --branch v$ORT_VERSION (expect $ORT_COMMIT) -> $src"
  echo "  (cd $src && ${build_cmd[*]}${dml_arch:+ --dml_path <DML_PATH>} --cmake_extra_defines ${cmake_defines[*]})"
  echo "  check CMakeCache.txt (telemetry off); merge the static libraries -> $stage/lib/$lib_name; telemetry-bytes.mjs; pack -> $out/$name.tar.gz"
  exit 0
fi

sha256() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }

# ---- source: the pinned tag, checked against its commit; submodules at the tag's commits (fuzzing and emsdk skipped).
# A source tree restored from a cache is checked the same way on every run: the pinned commit, nothing changed.
src_ok() {
  [ -d "$src/.git" ] && [ "$(git -C "$src" rev-parse HEAD 2>/dev/null)" = "$ORT_COMMIT" ] \
    && git -C "$src" diff --quiet HEAD 2>/dev/null \
    && [ "$(git -C "$src" submodule status -- cmake/external/onnx 2>/dev/null | cut -c1)" = " " ]
}
if ! src_ok; then
  rm -rf "$src"
  mkdir -p "$(dirname "$src")"
  git -c core.longpaths=true clone --quiet --depth 1 --branch "v$ORT_VERSION" https://github.com/microsoft/onnxruntime.git "$src"
  git -C "$src" -c core.longpaths=true submodule update --quiet --init --depth 1 -- cmake/external/onnx
  src_ok || { echo "build-ort: v$ORT_VERSION is $(git -C "$src" rev-parse HEAD), pinned $ORT_COMMIT (or its onnx submodule differs)" >&2; exit 1; }
fi

# ---- DirectML for the DirectML variant: our pinned copy, so ORT's CMake does not restore it with an unpinned nuget.exe.
dml_args=()
if [ -n "$dml_arch" ]; then
  dml_path=$("$here/fetch-directml.sh" "$dml_arch" "$work/cache" | sed -n 's/^DML_PATH=//p')
  [ -n "$dml_path" ] || { echo "build-ort: fetch-directml.sh gave no DML_PATH" >&2; exit 1; }
  dml_args=(--dml_path "$dml_path")
fi

# ---- build, at background priority so a shared runner stays responsive.
started=$(date +%s)
(cd "$src" && nice -n 10 "${build_cmd[@]}" ${dml_args[@]+"${dml_args[@]}"} --cmake_extra_defines "${cmake_defines[@]}")
build_secs=$(( $(date +%s) - started ))

# ---- the telemetry switches as CMake recorded them (check-cmake-telemetry.sh), kept for BUILD-INFO.txt.
telemetry_record=$("$here/check-cmake-telemetry.sh" "$build/Release")

# ---- bundle: every static library of the build except tests, mocks, protoc and import libraries, merged into one.
rel="$build/Release"
rm -rf "$stage" "$out/$name".*
mkdir -p "$stage/lib" "$stage/include" "$out"
list="$work/build/$variant.libs"
find "$rel" \( -name '*.a' -o -name '*.lib' \) -not -path '*/CMakeFiles/*' \
  | grep -v -E '/(lib)?(gtest|gmock|gtest_main|gmock_main|onnx_test_[a-z_]*|onnxruntime_test_[a-z_]*|onnxruntime_mocked_allocator|onnxruntime_providers_shared|protoc|libprotoc|DirectML)[^/]*\.(a|lib)$' \
  | grep -v -E '/(test|tests|testdata)/' | LC_ALL=C sort >"$list"
[ -s "$list" ] || { echo "build-ort: no static libraries found under $rel" >&2; exit 1; }
grep -q -E '/(lib)?onnxruntime_session\.(a|lib)$' "$list" || { echo "build-ort: onnxruntime_session is missing from the build" >&2; exit 1; }
case "$variant" in
  macos-*)
    libs=(); while IFS= read -r l; do libs+=("$l"); done <"$list"
    xcrun libtool -static -no_warning_for_no_symbols -o "$stage/lib/$lib_name" "${libs[@]}" ;;
  linux-*)
    { echo "CREATE $stage/lib/$lib_name"; sed 's/^/ADDLIB /' "$list"; echo SAVE; echo END; } | ar -M
    ranlib "$stage/lib/$lib_name" ;;
  windows-*)
    vswhere="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
    libexe=$("$vswhere" -latest -products '*' -find 'VC/Tools/MSVC/**/bin/Hostx64/x64/lib.exe' | tr -d '\r' | head -n 1)
    [ -n "$libexe" ] || { echo "build-ort: lib.exe not found (Visual Studio C++ tools)" >&2; exit 1; }
    rsp="$work/build/$variant.rsp"
    { echo "/NOLOGO"; echo "/OUT:\"$(native "$stage/lib/$lib_name")\""; while read -r l; do echo "\"$(native "$l")\""; done <"$list"; } >"$rsp"
    "$libexe" "@$(native "$rsp")" ;;
esac
cp "$src"/include/onnxruntime/core/session/*.h "$stage/include/"
cp "$src/LICENSE" "$stage/LICENSE"
cp "$src/ThirdPartyNotices.txt" "$stage/ThirdPartyNotices.txt"

# What the consumer links besides the archive (ort-sys adds the C++ runtime, Foundation and CoreML on Apple, shell32 on
# Windows). The DirectML imports are delay-loaded by the consumer: a static library cannot carry /DELAYLOAD (ONNX Runtime
# cmake/onnxruntime.cmake applies its delay-load list only to onnxruntime.dll), so sighter-infer's build.rs passes them,
# and a missing or broken DirectML.dll then degrades to the CPU instead of stopping the start (ADR 0001 decision 4).
case "$variant" in
  macos-arm64-coreml) printf '%s\n' "framework=CoreML" "framework=Foundation" "dylib=c++" >"$stage/link.txt" ;;
  linux-x64-cpu) printf '%s\n' "dylib=stdc++" "dylib=pthread" "dylib=dl" >"$stage/link.txt" ;;
  windows-x64-cpu) printf '%s\n' "dylib=shell32" >"$stage/link.txt" ;;
  windows-x64-directml)
    printf '%s\n' "dylib=shell32" "dylib=dxguid" "dylib=dxcore" "dylib=dxgi" "dylib=d3d12" "dylib=DirectML" "dylib=delayimp" \
      "delayload=DirectML.dll" "delayload=d3d12.dll" "delayload=dxgi.dll" "delayload=dxcore.dll" >"$stage/link.txt" ;;
esac

# ---- check: no telemetry backend in what we ship.
node "$root/conformance/release/telemetry-bytes.mjs" "$stage/lib/$lib_name"

# ---- record and pack.
case "$variant" in
  windows-*)
    vs=$("$vswhere" -latest -products '*' -property installationPath | tr -d '\r')
    compiler="MSVC $(tr -d '\r\n' <"$vs/VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt")" ;;
  *) compiler=$(c++ --version | head -n 1) ;;
esac
{
  echo "package: $ORT_PACKAGE $pkg_version"
  echo "variant: $variant"
  echo "onnxruntime: $ORT_VERSION ($ORT_COMMIT)"
  echo "build.py flags: --config Release --skip_tests --compile_no_warning_as_error --no_telemetry ${extra[*]-}${dml_arch:+ --dml_path (DirectML $DML_VERSION)}"
  echo "cmake defines: ${cmake_defines[*]}"
  echo "telemetry (CMakeCache.txt): $telemetry_record"
  echo "ccache: ${ccache_args[*]:-no}"
  echo "host: $host"
  echo "compiler: $compiler"
  echo "cmake: $(cmake --version | head -n 1)"
  echo "python: $("${python[@]}" --version 2>&1)"
  echo "build seconds: $build_secs ($jobs jobs)"
  echo "library sha256: $(sha256 "$stage/lib/$lib_name")"
} >"$stage/BUILD-INFO.txt"

archive="$out/$name.tar.gz"
(cd "$work/stage" && find "$name" -type f | LC_ALL=C sort >"$work/stage/$name.files" && tar -czf "$archive" -T "$work/stage/$name.files")
sum=$(sha256 "$archive")
size=$(wc -c <"$archive" | tr -d ' ')
echo "$sum  $name.tar.gz" >"$archive.sha256"
key=$(case "$variant" in macos-*) echo macos-aarch64 ;; windows-*) echo windows-x86_64 ;; linux-*) echo linux-x86_64 ;; esac)
cat >"$out/$name.catalogue.txt" <<EOF
[[platform]]
key = "$key"
url = "$GITEA_PACKAGES/$ORT_PACKAGE/$pkg_version/$name.tar.gz"
size = $size
sha256 = "$sum"
EOF
echo "build-ort: $archive ($size bytes, SHA-256 $sum) in ${build_secs}s"

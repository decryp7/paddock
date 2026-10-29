#!/usr/bin/env bash
# Fetches Microsoft's DirectML redistributable from NuGet, verifies it against the pins and lays out one architecture as
# ONNX Runtime's build expects a custom DirectML (`--dml_path`: bin/DirectML.dll, lib/DirectML.lib, include/*.h).
#
#   apps/sighter/scripts/ort/fetch-directml.sh <x64|arm64> <cache-dir>
#
# prints `DML_PATH=<dir>` on success. The package is never mirrored by this script: it comes from nuget.org, pinned by
# NuGet's SHA-512 and its size, and the DLL by our SHA-256 (pins.env). Using --dml_path also keeps ONNX Runtime's CMake
# from downloading nuget.exe 5.3.0 and restoring the package itself (cmake/external/dml.cmake), which is unpinned.
#
# Licence: DirectML.dll is under the Microsoft Software License Terms for DirectML (proprietary; distributable only inside
# an application, never stand-alone). It is a named licence exception of ADR 0001, conditional on the P6 gate. The
# licence and notice files are copied next to the DLL so the bundle can carry them.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=pins.env
. "$here/pins.env"

arch=${1:?usage: fetch-directml.sh <x64|arm64> <cache-dir>}
cache=${2:?usage: fetch-directml.sh <x64|arm64> <cache-dir>}
case "$arch" in
  x64) want=$DML_X64_DLL_SHA256 ;;
  arm64) want=$DML_ARM64_DLL_SHA256 ;;
  *) echo "fetch-directml: unknown architecture '$arch' (x64 or arm64)" >&2; exit 2 ;;
esac

sha256() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
filesize() { wc -c <"$1" | tr -d ' '; }

pkg_dir="$cache/directml/$DML_VERSION"
pkg="$pkg_dir/microsoft.ai.directml.$DML_VERSION.nupkg"
mkdir -p "$pkg_dir"

if [ ! -f "$pkg_dir/.verified" ]; then
  command -v openssl >/dev/null 2>&1 || { echo "fetch-directml: openssl is needed to check NuGet's SHA-512" >&2; exit 1; }
  tmp="$pkg.part"
  curl -fsSL --retry 3 -o "$tmp" "$DML_NUPKG_URL"
  got_size=$(filesize "$tmp")
  got_sha=$(openssl dgst -sha512 -binary "$tmp" | openssl base64 -A)
  if [ "$got_size" != "$DML_NUPKG_SIZE" ] || [ "$got_sha" != "$DML_NUPKG_SHA512_B64" ]; then
    rm -f "$tmp"
    echo "fetch-directml: the package does not match its pin (size $got_size, SHA-512 $got_sha)" >&2
    exit 1
  fi
  mv "$tmp" "$pkg"
  touch "$pkg_dir/.verified"
fi

# A fresh, empty folder per architecture; only the named files are taken from the package.
out="$pkg_dir/$arch"
rm -rf "$out"
mkdir -p "$out/bin" "$out/lib" "$out/include" "$out/x"
unzip -q -o "$pkg" "bin/$arch-win/DirectML.dll" "bin/$arch-win/DirectML.lib" "include/DirectML.h" \
  "include/DirectMLConfig.h" LICENSE.txt LICENSE-CODE.txt ThirdPartyNotices.txt -d "$out/x"
mv "$out/x/bin/$arch-win/DirectML.dll" "$out/bin/"
mv "$out/x/bin/$arch-win/DirectML.lib" "$out/lib/"
mv "$out/x/include/"*.h "$out/include/"
mv "$out/x/LICENSE.txt" "$out/LICENSE-DirectML.txt"
mv "$out/x/LICENSE-CODE.txt" "$out/LICENSE-DirectML-headers.txt"
mv "$out/x/ThirdPartyNotices.txt" "$out/ThirdPartyNotices-DirectML.txt"
rm -rf "$out/x"

got=$(sha256 "$out/bin/DirectML.dll")
if [ "$got" != "$want" ]; then
  rm -rf "$out"
  echo "fetch-directml: DirectML.dll ($arch) has SHA-256 $got, pinned $want" >&2
  exit 1
fi

abs=$(cd "$out" && pwd)
command -v cygpath >/dev/null 2>&1 && abs=$(cygpath -m "$abs")
echo "DML_PATH=$abs"

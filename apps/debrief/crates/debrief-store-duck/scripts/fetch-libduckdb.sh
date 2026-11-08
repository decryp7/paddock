#!/usr/bin/env sh
# Downloads the official prebuilt libduckdb for this platform, verifies its pinned SHA-256 and
# unpacks it into a cache directory, so debrief-store-duck links DuckDB without compiling its C++ sources
# (the `bundled` feature needs several GB of RAM and many minutes).
#
#   eval "$(crates/debrief-store-duck/scripts/fetch-libduckdb.sh [cache-dir])"
#
# prints `export DUCKDB_LIB_DIR=... DUCKDB_INCLUDE_DIR=...` (and PATH on Windows, where the
# executable needs duckdb.dll next to it or on PATH). The version must match the `duckdb` crate:
# duckdb-rs 1.20000.x = DuckDB 2.0.0. Checksums are the GitHub release API digests of v2.0.0. macOS is Apple silicon
# only (osx-arm64): Intel Macs are not supported (CHANGELOG; DuckDB 2.1 drops them too).
set -eu

VERSION=2.0.0
CACHE=${1:-${CARGO_TARGET_DIR:-target}/libduckdb}

case "$(uname -s)-$(uname -m)" in
  MINGW*-x86_64 | MSYS*-x86_64 | CYGWIN*-x86_64)
    ASSET=duckdb-shared-libs-windows-amd64.tar.gz; SHA=2b83169128239de5f45fe21de80f99bf436e53aa4bb24020dd951a1ba8d4b95f ;;
  MINGW*-aarch64 | MSYS*-aarch64)
    ASSET=duckdb-shared-libs-windows-arm64.tar.gz; SHA=159ce60eb587112b8c5f290b8beea56af5f13be4d13679de127ad9bfaafb7447 ;;
  Linux-x86_64)
    ASSET=duckdb-shared-libs-linux-amd64.tar.gz; SHA=993ad89e7dfca66cd6da994c2cad090c2fc160ddf9f550498a098ab4bb13cfb1 ;;
  Linux-aarch64)
    ASSET=duckdb-shared-libs-linux-arm64.tar.gz; SHA=c60eecf91143ca6bbac55c4f4d40dc22520488b0ac257c86e0fe9a2b451cf2b7 ;;
  Darwin-arm64)
    ASSET=duckdb-shared-libs-osx-arm64.tar.gz; SHA=c35d60080b31ba5d11a3ce505a6831be784f003b4c575d32a4c2badb3b41cdf0 ;;
  Darwin-x86_64)
    echo "Intel Macs are not supported: Debrief supports Apple silicon Macs only (DuckDB ships an osx-amd64 library, but Debrief does not build or test for it)" >&2; exit 1 ;;
  *) echo "no pinned libduckdb for $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

DIR="$CACHE/$VERSION/${ASSET%.tar.gz}"
if [ ! -f "$DIR/.verified" ]; then
  mkdir -p "$CACHE/$VERSION"
  ARCHIVE="$CACHE/$VERSION/$ASSET"
  curl -fsSL --retry 3 -o "$ARCHIVE" "https://github.com/duckdb/duckdb/releases/download/v$VERSION/$ASSET"
  GOT=$(sha256sum "$ARCHIVE" 2>/dev/null | cut -d' ' -f1 || shasum -a 256 "$ARCHIVE" | cut -d' ' -f1)
  if [ "$GOT" != "$SHA" ]; then
    rm -f "$ARCHIVE"
    echo "SHA-256 mismatch for $ASSET: expected $SHA, got $GOT" >&2
    exit 1
  fi
  rm -rf "$DIR"
  mkdir -p "$DIR"
  # A fresh, empty directory; only the library, import library and headers are used.
  # Git for Windows' GNU tar takes `C:` in a path for a remote host: --force-local (bsdtar has no such option).
  if tar --version 2>/dev/null | grep -q 'GNU tar'; then tar --force-local -xzf "$ARCHIVE" -C "$DIR"; else tar -xzf "$ARCHIVE" -C "$DIR"; fi
  touch "$DIR/.verified"
fi

ABS=$(cd "$DIR" && pwd)
# Cargo build scripts on Windows need a native path (C:/...), the shell's PATH a POSIX one.
NATIVE=$ABS
command -v cygpath >/dev/null 2>&1 && NATIVE=$(cygpath -m "$ABS")
echo "export DUCKDB_LIB_DIR='$NATIVE' DUCKDB_INCLUDE_DIR='$NATIVE'"
case "$ASSET" in
  duckdb-shared-libs-windows-*) echo "export PATH='$ABS':\"\$PATH\"" ;;
  duckdb-shared-libs-linux-*) echo "export LD_LIBRARY_PATH='$ABS':\"\${LD_LIBRARY_PATH:-}\"" ;;
  duckdb-shared-libs-osx-*) echo "export DYLD_LIBRARY_PATH='$ABS':\"\${DYLD_LIBRARY_PATH:-}\"" ;;
esac

#!/usr/bin/env bash
# Downloads one file and refuses it unless its SHA-256 matches the pin.
#
#   fetch-verified.sh <url> <sha256> <destination>
#
# Used by the image build for every binary it adds. Exits 2 on a bad call, 1 on a download or checksum failure; a file that fails
# the check is deleted, never left behind.
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: fetch-verified.sh <url> <sha256> <destination>" >&2
  exit 2
fi
url=$1 want=$2 dest=$3

case "$want" in
  *[!0-9a-f]* | "") echo "fetch-verified: the checksum for $url must be 64 lowercase hex digits (got '$want')" >&2; exit 2 ;;
esac
if [ "${#want}" -ne 64 ]; then
  echo "fetch-verified: the checksum for $url must be 64 lowercase hex digits (got ${#want})" >&2
  exit 2
fi

mkdir -p "$(dirname "$dest")"
curl -fsSL --retry 3 --retry-delay 2 -o "$dest" "$url" || { rm -f "$dest"; echo "fetch-verified: download failed: $url" >&2; exit 1; }

if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$dest" | cut -d' ' -f1); else got=$(shasum -a 256 "$dest" | cut -d' ' -f1); fi
if [ "$got" != "$want" ]; then
  rm -f "$dest"
  echo "fetch-verified: checksum mismatch for $url" >&2
  echo "  expected $want" >&2
  echo "  got      $got" >&2
  exit 1
fi
echo "verified $(basename "$dest") ($want)"

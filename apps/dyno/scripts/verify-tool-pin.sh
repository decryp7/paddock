#!/usr/bin/env bash
# Run once per pin bump (never by the app, never in a test): checks a downloaded tool file against the publisher's OpenPGP signature
# with the system's gpg, and prints the pack lines (size, sha256, date, fingerprint) for crates/dyno-agent/tools/tool-pins.toml.
#
#   apps/dyno/scripts/verify-tool-pin.sh <file> <file.asc> [key.asc]
#
# The expected fingerprint is the one the user confirmed against smartmontools.org on 2026-11-03 (key id FF3AEFF5). The key file
# defaults to scripts/tool-pins/smartmontools-signing-key.asc. The next smartmontools release will be signed with a new key: then
# confirm the new fingerprint with a person, change EXPECTED here and in the pack, and record where it was read.
set -euo pipefail
EXPECTED="0C9577FD2C4CFCB4B9A599640A30812EFF3AEFF5"
file="${1:?usage: verify-tool-pin.sh <file> <file.asc> [key.asc]}"
sig="${2:?usage: verify-tool-pin.sh <file> <file.asc> [key.asc]}"
here="$(cd "$(dirname "$0")" && pwd)"
key="${3:-$here/tool-pins/smartmontools-signing-key.asc}"
command -v gpg >/dev/null || { echo "::error::gpg is not installed; install GnuPG and run this again" >&2; exit 2; }
home="$(mktemp -d)"; trap 'rm -rf "$home"' EXIT; chmod 700 "$home"
export GNUPGHOME="$home"
gpg --batch --quiet --import "$key"
got="$(gpg --batch --with-colons --fingerprint | awk -F: '/^fpr:/ {print $10; exit}')"
if [ "$got" != "$EXPECTED" ]; then
  echo "::error::the key file has fingerprint $got, not the confirmed $EXPECTED; do not pin anything from it" >&2; exit 1
fi
# The signature may be made by a key that has expired since (smartmontools' was valid when it signed); gpg says so in its output.
gpg --batch --verify "$sig" "$file" 2>&1 | tee "$home/verify.txt"
grep -q "Good signature" "$home/verify.txt" || { echo "::error::the signature of $file does not verify" >&2; exit 1; }
size="$(wc -c < "$file" | tr -d ' ')"
sha="$( (shasum -a 256 "$file" 2>/dev/null || sha256sum "$file") | awk '{print $1}')"
echo
echo "size = $size"
echo "sha256 = \"$sha\""
echo "key_fingerprint = \"0C95 77FD 2C4C FCB4 B9A5 9964 0A30 812E FF3A EFF5\""
echo "verified = \"$(date -u +%Y-%m-%d)\""

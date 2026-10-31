#!/usr/bin/env bash
# Fails while a Sighter ONNX Runtime catalogue still holds placeholder pins (an all-zero SHA-256), so no app build or
# release links an archive nobody has pinned. Run by the release checklist (stage-2 plan SW44) and by sighter-infer's
# build before it fetches an archive.
#
#   apps/sighter/scripts/ort/check-catalogue.sh [catalogue.toml]...   (default: every file in catalogue/)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
[ $# -gt 0 ] || set -- "$here"/catalogue/*.toml
status=0
for f in "$@"; do
  n=$(grep -c -E '^sha256 = "0{64}"' "$f" || true)
  if [ "$n" != 0 ]; then
    echo "check-catalogue: $f: $n placeholder pin(s); run sighter-ort.yml and paste the lines it prints" >&2
    status=1
  else
    echo "check-catalogue: $f: pinned"
  fi
done
exit $status

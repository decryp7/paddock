#!/usr/bin/env bash
# Publishes built ONNX Runtime archives (build-ort.sh) as files of the Gitea generic package sighter-onnxruntime, version
# "<ORT version>-p<pipeline revision>", and prints the catalogue lines to paste into catalogue/*.toml. Run by
# .gitea/workflows/sighter-ort.yml only (rule: publishing from CI, never from a workstation).
#
#   PACKAGE_TOKEN=... apps/sighter/scripts/ort/publish-ort.sh <out-dir> [variant]
#
# A file that already exists is refused by Gitea (409): a published archive is never replaced. Rebuilding the same ORT
# version with other flags or tools needs PIPELINE_REV + 1 in pins.env, which gives a new package version.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=pins.env
. "$here/pins.env"

out=${1:?usage: publish-ort.sh <out-dir> [variant]}
variant=${2:-}
: "${PACKAGE_TOKEN:?PACKAGE_TOKEN (write:package) is not set}"
version="$ORT_VERSION-p$PIPELINE_REV"

shopt -s nullglob
archives=("$out"/onnxruntime-"$version"-"${variant:-}"*.tar.gz)
[ ${#archives[@]} -gt 0 ] || { echo "publish-ort: no onnxruntime-$version-${variant}*.tar.gz in $out" >&2; exit 1; }

for a in "${archives[@]}"; do
  sha256sum_file="$a.sha256"
  [ -f "$sha256sum_file" ] || { echo "publish-ort: $sha256sum_file is missing" >&2; exit 1; }
  for f in "$a" "$sha256sum_file"; do
    url="$GITEA_PACKAGES/$ORT_PACKAGE/$version/$(basename "$f")"
    code=$(curl -sS -o /dev/stderr -w '%{http_code}' -H "Authorization: token $PACKAGE_TOKEN" --upload-file "$f" "$url")
    case "$code" in
      201) echo "publish-ort: published $url" ;;
      409) echo "publish-ort: $url exists already; bump PIPELINE_REV in pins.env to publish a rebuild" >&2; exit 1 ;;
      *) echo "publish-ort: $url: HTTP $code" >&2; exit 1 ;;
    esac
  done
  echo "publish-ort: catalogue lines for $(basename "$a"):"
  cat "${a%.tar.gz}.catalogue.txt"
done

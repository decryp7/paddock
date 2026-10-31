#!/usr/bin/env bash
# Publishes built ONNX Runtime archives (build-ort.sh) as files of the Gitea generic package sighter-onnxruntime, version
# "<ORT version>-p<pipeline revision>", and prints the catalogue lines to paste into catalogue/*.toml. Run only by the
# publish job of .gitea/workflows/sighter-ort.yml, which downloads the build jobs' artifacts and runs no build code.
#
#   PACKAGE_TOKEN=... apps/sighter/scripts/ort/publish-ort.sh <folder>
#
# For every onnxruntime-<version>-*.tar.gz in <folder> (searched recursively: one artifact folder per variant):
#   - its .sha256 must match the archive, or nothing is published;
#   - the archive and the .sha256 are uploaded. A published file is never replaced: Gitea answers 409 when it exists,
#     and then the published file is downloaded and compared; identical is success (a re-run), different fails (a rebuild
#     with the same version needs PIPELINE_REV + 1 in pins.env).
# The token never appears on a command line: curl reads the header from a file only this user can read.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=pins.env
. "$here/pins.env"

folder=${1:?usage: publish-ort.sh <folder>}
: "${PACKAGE_TOKEN:?PACKAGE_TOKEN (write:package) is not set}"
version="$ORT_VERSION-p$PIPELINE_REV"
# Tests point this at a local server (tests/run.sh); CI never sets it.
base=${PUBLISH_ORT_BASE_URL:-$GITEA_PACKAGES}
sha256() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
(umask 077 && printf 'Authorization: token %s\n' "$PACKAGE_TOKEN" >"$scratch/auth")

archives=()
while IFS= read -r a; do archives+=("$a"); done < <(find "$folder" -type f -name "onnxruntime-$version-*.tar.gz" | LC_ALL=C sort)
[ ${#archives[@]} -gt 0 ] || { echo "publish-ort: no onnxruntime-$version-*.tar.gz under $folder" >&2; exit 1; }

# Every archive is checked before anything is uploaded.
for a in "${archives[@]}"; do
  [ -f "$a.sha256" ] || { echo "publish-ort: $a.sha256 is missing" >&2; exit 1; }
  want=$(cut -d' ' -f1 <"$a.sha256")
  got=$(sha256 "$a")
  [ "$got" = "$want" ] || { echo "publish-ort: $(basename "$a") has SHA-256 $got, its .sha256 says $want" >&2; exit 1; }
done

upload() {
  local f=$1 url code
  url="$base/$ORT_PACKAGE/$version/$(basename "$f")"
  code=$(curl -sS -o "$scratch/answer" -w '%{http_code}' -H "@$scratch/auth" --upload-file "$f" "$url")
  case "$code" in
    201) echo "publish-ort: published $url" ;;
    409)
      curl -fsSL -H "@$scratch/auth" -o "$scratch/published" "$url"
      if [ "$(sha256 "$scratch/published")" = "$(sha256 "$f")" ]; then
        echo "publish-ort: $url is already published with the same content"
      else
        echo "publish-ort: $url exists with other content; bump PIPELINE_REV in pins.env to publish a rebuild" >&2
        return 1
      fi ;;
    *) echo "publish-ort: $url: HTTP $code: $(head -c 300 "$scratch/answer")" >&2; return 1 ;;
  esac
}

for a in "${archives[@]}"; do
  upload "$a"
  upload "$a.sha256"
  echo "publish-ort: catalogue lines for $(basename "$a"):"
  cat "${a%.tar.gz}.catalogue.txt"
done

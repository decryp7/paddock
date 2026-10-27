#!/usr/bin/env bash
# Verifies the Linux side of Pitwall in a throw-away container (OrbStack or Docker) without touching the host's
# toolchain: builds and tests pitwall-core and pitwall-server, then exercises the systemd user unit generation
# (`pitwall-server print-unit`, and `install` / `uninstall` against a stand-in `systemctl` that records its calls).
# With --app it also installs the WebKitGTK development packages and builds and tests the Tauri shell (pitwall-app).
#
#   bash scripts/linux-container-verify.sh [--app] [--cpus N]
#
# The source is mounted read-only and copied inside; the cargo target lives inside the container; the container is
# removed when the script ends (--rm), and no image is built, only pulled (remove it with: docker rmi rust:1-bookworm).
set -euo pipefail

cd "$(dirname "$0")/.."
app=0
cpus=4
while [ $# -gt 0 ]; do
  case "$1" in
    --app) app=1 ;;
    --cpus) shift; cpus="$1" ;;
    *) echo "unknown option $1"; exit 2 ;;
  esac
  shift
done
runtime=docker
command -v docker >/dev/null || { echo "docker (or OrbStack's docker) is needed"; exit 1; }

docker run --rm --cpus "$cpus" -e WITH_APP="$app" -e CARGO_BUILD_JOBS="$cpus" \
  -v "$(pwd)":/src:ro rust:1-bookworm bash /src/scripts/linux-container-inner.sh

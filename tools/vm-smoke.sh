#!/usr/bin/env bash
# The local entry point for tests that need a throwaway OS (login tests, services, installs, updates, system settings: rule 7).
# First case: the service-after-update smoke (conformance C21, catalogue R03) in ONE local command: builds the shell demo as N and N+1
# and runs the update smoke in a throw-away VM on this Mac (Tart: macOS and Ubuntu guests). Local only: CI has no VMs
# (user decision, 2026-10-28). Run it before tagging a release (docs/blocks/releasing.md).
#
#   bash tools/vm-smoke.sh macos|linux
#
# Environment: TART_HOME (VM images; default /Volumes/d7sst7/ci-tart), PADDOCK_VM_OUT (where the builds go; default a new
#   folder under TART_HOME), PADDOCK_VM_LOG_DIR, and everything tools/macos-vm-smoke.sh reads.
# Needs: an Apple-silicon Mac, Tart (brew install cirruslabs/cli/tart), the TART_HOME disk mounted, 60 GB free there; the
#   linux guest also needs Docker (the linux-arm64 builds are made in one throw-away container).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
die() { echo "vm-smoke: $*" >&2; exit 1; }

guest="${1:-}"
case "$guest" in
  macos | linux) ;;
  *) echo "usage: vm-smoke.sh macos|linux" >&2; exit 2 ;;
esac
[ $# -eq 1 ] || { echo "usage: vm-smoke.sh macos|linux" >&2; exit 2; }

[ "$(uname -s)" = Darwin ] || die "this runs on a Mac (Tart needs Apple silicon)"
tart="${TART:-tart}"
command -v "$tart" >/dev/null 2>&1 || die "tart is not installed: brew install cirruslabs/cli/tart"
if [ "$guest" = linux ]; then
  command -v docker >/dev/null 2>&1 || die "docker is needed for the linux-arm64 builds (start Docker or OrbStack)"
fi

export TART_HOME="${TART_HOME:-/Volumes/d7sst7/ci-tart}"
# The default lives on the external disk: do not create it under a mount point that is not there.
volume_root="$(printf '%s' "$TART_HOME" | awk -F/ '$2=="Volumes" {print "/Volumes/" $3}')"
if [ -n "$volume_root" ] && [ ! -d "$volume_root" ]; then
  die "$volume_root is not mounted, so TART_HOME=$TART_HOME has no disk (mount it, or set TART_HOME to another folder with 60 GB free)"
fi
mkdir -p "$TART_HOME" || die "cannot create TART_HOME=$TART_HOME"

# Fail before the long builds when the disk cannot take the VM image (macos-vm-smoke.sh checks again, with the same limit).
min_free="${PADDOCK_VM_MIN_FREE_GB:-60}"
free_gb=$(df -k "$TART_HOME" | awk 'NR==2 {print int($4/1048576)}')
[ "${free_gb:-0}" -ge "$min_free" ] || die "only ${free_gb:-0} GB free under $TART_HOME; at least $min_free GB are needed. Free space or set TART_HOME elsewhere."

if [ -n "${PADDOCK_VM_OUT:-}" ]; then
  out="$PADDOCK_VM_OUT"
else
  out="$(mktemp -d "$TART_HOME/smoke-builds-XXXXXX")"
  trap 'rm -rf "$out"' EXIT # the builds are scratch; a PADDOCK_VM_OUT the caller named is kept
fi
mkdir -p "$out"
cd "$repo"

if [ "$guest" = macos ]; then
  bash scripts/shell-demo/build-macos-n-n1.sh "$out"
  read -r PADDOCK_SMOKE_OLD_VERSION PADDOCK_SMOKE_NEW_VERSION < "$out/versions"
  export PADDOCK_SMOKE_OLD_VERSION PADDOCK_SMOKE_NEW_VERSION
  app="$(basename "$(ls -d "$out"/n/*.app | head -n 1)")"
  bash tools/macos-vm-smoke.sh --old "$out/n/$app" --new "$out/n1/$app"
else
  npm --prefix ui run shell-demo:build # the container build embeds this UI (needs `npm ci` at the root once)
  bash scripts/shell-demo/build-linux-arm64.sh "$out"
  read -r PADDOCK_SMOKE_OLD_VERSION PADDOCK_SMOKE_NEW_VERSION < "$out/versions"
  export PADDOCK_SMOKE_OLD_VERSION PADDOCK_SMOKE_NEW_VERSION
  bash tools/macos-vm-smoke.sh --guest linux --old "$out/n/paddock-shell-demo" --new "$out/n1/paddock-shell-demo"
fi
echo "vm-smoke: the $guest update smoke passed"

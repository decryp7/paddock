#!/usr/bin/env bash
# Runs cargo for Dyno inside a native Linux container (OrbStack/Docker), so the Linux adapters are built and tested
# against a real /proc and /sys. The repository is mounted read-only; the cargo registry and the build output live in
# named volumes (dyno-cargo-registry, dyno-linux-target), never in the repository. Containers are --rm, so none is left.
#
# Usage:  scripts/linux-container.sh                 # cargo test -p dyno-platform + clippy -D warnings
#         scripts/linux-container.sh cargo test -p dyno-platform linux::
#         scripts/linux-container.sh sh -c 'ls /proc | head'
#         scripts/linux-container.sh --clean-volumes  # delete the two named volumes
# The image dyno-linux-dev is the official Rust image plus clippy and rustfmt, built once on first use.
# Environment: DYNO_LINUX_BASE_IMAGE (default rust:1-bookworm), DYNO_LINUX_IMAGE, DYNO_LINUX_CPUS (default 4), DYNO_LINUX_JOBS (default 2).
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
base_image="${DYNO_LINUX_BASE_IMAGE:-rust:1-bookworm}"
image="${DYNO_LINUX_IMAGE:-dyno-linux-dev:1}"
cpus="${DYNO_LINUX_CPUS:-4}"
jobs="${DYNO_LINUX_JOBS:-2}"
registry_volume=dyno-cargo-registry
target_volume=dyno-linux-target

if [[ "${1:-}" == "--clean-volumes" ]]; then
  docker volume rm -f "$registry_volume" "$target_volume"
  exit 0
fi

if ! docker image inspect "$image" >/dev/null 2>&1; then
  printf 'FROM %s\nRUN rustup component add clippy rustfmt\n' "$base_image" | docker build -t "$image" -
fi
# Idempotent: creating an existing volume is a no-op.
docker volume create "$registry_volume" >/dev/null
docker volume create "$target_volume" >/dev/null

if [[ $# -eq 0 ]]; then
  set -- sh -c 'cargo test -p dyno-platform && cargo clippy -p dyno-platform --all-targets -- -D warnings'
fi

# Not --privileged: the tests must pass as an ordinary user would run them. The container runs as root (the image's
# default) unless DYNO_LINUX_USER is set, e.g. DYNO_LINUX_USER=1000:1000 to exercise the "other users' processes" paths.
user_args=()
if [[ -n "${DYNO_LINUX_USER:-}" ]]; then
  # An ordinary user cannot write the root-owned volumes, so it builds into the container's own /tmp (a cold build).
  user_args=(--user "$DYNO_LINUX_USER" -e HOME=/tmp/home -e CARGO_HOME=/tmp/cargo-home -e CARGO_TARGET_DIR=/tmp/target)
fi

exec docker run --rm --cpus "$cpus" \
  --mount "type=bind,source=$repo,target=/work,readonly" \
  --mount "type=volume,source=$registry_volume,target=/usr/local/cargo/registry" \
  --mount "type=volume,source=$target_volume,target=/target" \
  -e CARGO_TARGET_DIR=/target -e CARGO_BUILD_JOBS="$jobs" -e CARGO_TERM_COLOR=never \
  ${user_args[@]+"${user_args[@]}"} -w /work "$image" "$@"

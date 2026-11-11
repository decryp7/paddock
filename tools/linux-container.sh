#!/usr/bin/env bash
# Runs cargo (or any command) for an app inside a native Linux container (OrbStack/Docker), so the Linux adapters build and
# test against a real /proc and /sys (plan X47; Dyno's linux-container.sh and Pitwall's linux-container-verify.sh as one).
# The repository is mounted read-only; the cargo registry and the build output live in named volumes, never in the
# repository. Every container is created with --rm and the label paddock.tools=<id>; only containers and volumes this script
# created (that label, those names) are ever removed (rule 7).
#
#   bash paddock/tools/linux-container.sh --id dyno [--root <repo>] [--packages dyno-platform] -- [command...]
#   bash paddock/tools/linux-container.sh --id paddock --image <ref> [--platform linux/amd64] -- command...   # a ready image, run as it is
#   bash paddock/tools/linux-container.sh --id dyno --clean              # stop labelled containers, delete the two volumes
#
# With no command: `cargo test -p <packages> && cargo clippy -p <packages> --all-targets -- -D warnings`.
# Environment: DOCKER (default docker), PADDOCK_LINUX_BASE_IMAGE (rust:1-bookworm), PADDOCK_LINUX_CPUS (4), PADDOCK_LINUX_JOBS (2),
# PADDOCK_LINUX_MEMORY (12g; ER-14 wants every test container capped), PADDOCK_LINUX_USER (e.g. 1000:1000 to exercise the "other users' processes" paths; builds into the container's /tmp).
set -euo pipefail

id=""; root="$PWD"; packages=""; clean=0; ready_image=""; platform=""
while [ $# -gt 0 ]; do
  case "$1" in
    --id) id="${2:?--id needs a value}"; shift ;;
    --root) root="${2:?--root needs a value}"; shift ;;
    --packages) packages="${2:?--packages needs a value}"; shift ;;
    --image) ready_image="${2:?--image needs a value}"; shift ;;
    --platform) platform="${2:?--platform needs a value}"; shift ;;
    --clean) clean=1 ;;
    --) shift; break ;;
    *) echo "linux-container: unknown option $1 (a command goes after --)" >&2; exit 2 ;;
  esac
  shift
done
[ -n "$id" ] || { echo "linux-container: --id <app id> is needed" >&2; exit 2; }
[[ "$id" =~ ^[a-z][a-z0-9-]*$ ]] || { echo "linux-container: --id must be lower-case letters, digits and dashes" >&2; exit 2; }
docker="${DOCKER:-docker}"
command -v "$docker" >/dev/null 2>&1 || { echo "linux-container: $docker (OrbStack or Docker) is needed" >&2; exit 1; }

label="paddock.tools=$id"
base_image="${PADDOCK_LINUX_BASE_IMAGE:-rust:1-bookworm}"
image="$id-linux-dev:1"
cpus="${PADDOCK_LINUX_CPUS:-4}"
jobs="${PADDOCK_LINUX_JOBS:-2}"
memory="${PADDOCK_LINUX_MEMORY:-12g}"
registry_volume="$id-cargo-registry"
target_volume="$id-linux-target"

if [ "$clean" = 1 ]; then
  leftovers=$("$docker" ps -aq --filter "label=$label")
  # shellcheck disable=SC2086
  [ -z "$leftovers" ] || "$docker" rm -f $leftovers >/dev/null
  "$docker" volume rm -f "$registry_volume" "$target_volume" >/dev/null
  echo "removed the volumes $registry_volume and $target_volume"
  exit 0
fi

if [ $# -eq 0 ] && [ -z "$packages" ]; then
  echo "linux-container: no command and no --packages: nothing to run" >&2
  exit 2
fi

if [ -n "$ready_image" ]; then
  image="$ready_image" # a ready image (for example the CI image) is run as it is: no layer of ours, no rustup
elif ! "$docker" image inspect "$image" >/dev/null 2>&1; then
  printf 'FROM %s\nRUN rustup component add clippy rustfmt\n' "$base_image" | "$docker" build --label "$label" -t "$image" -
fi
"$docker" volume create --label "$label" "$registry_volume" >/dev/null
"$docker" volume create --label "$label" "$target_volume" >/dev/null

if [ $# -eq 0 ]; then
  set -- sh -c "cargo test -p $packages && cargo clippy -p $packages --all-targets -- -D warnings"
fi

# Not --privileged: the tests must pass as an ordinary user would run them.
user_args=()
if [ -n "${PADDOCK_LINUX_USER:-}" ]; then
  user_args=(--user "$PADDOCK_LINUX_USER" -e HOME=/tmp/home -e CARGO_HOME=/tmp/cargo-home -e CARGO_TARGET_DIR=/tmp/target)
fi

exec "$docker" run --rm --label "$label" --cpus "$cpus" --memory "$memory" \
  ${platform:+--platform "$platform"} \
  --mount "type=bind,source=$root,target=/work,readonly" \
  --mount "type=volume,source=$registry_volume,target=/usr/local/cargo/registry" \
  --mount "type=volume,source=$target_volume,target=/target" \
  -e CARGO_TARGET_DIR=/target -e CARGO_BUILD_JOBS="$jobs" -e CARGO_TERM_COLOR=never \
  ${user_args[@]+"${user_args[@]}"} -w /work "$image" "$@"

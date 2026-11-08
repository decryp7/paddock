#!/usr/bin/env bash
# A cargo home that survives between Linux container jobs (ADR 0062 WP-1e). A container job starts with an empty CARGO_HOME, so
# every run downloaded every crate (job 31430, line 1066). The runner's host mount /sccache (already writable; node-caches keeps
# npm there) holds a fixed number of cargo homes, /sccache/cargo-home/<n>. A job takes slot n by holding an flock on
# cargo-home/<n>.lock for the whole job: a detached holder process keeps the lock until the container, and with it the holder,
# is gone (the kernel drops the lock then, so there is no stale lock to reclaim; rule 53). The whole CARGO_HOME is per slot, not
# only registry/, because cargo's .package-cache lock sits at its root and would serialise concurrent jobs.
#
#   cargo-home-slot.sh        run from the job's first Rust step; appends CARGO_HOME (and the image's cargo bin to PATH) to
#                             $GITHUB_ENV / $GITHUB_PATH, and prints the slot it took
#
# Environment: PADDOCK_CI_MOUNT (default /sccache), PADDOCK_CARGO_SLOTS (default 8), PADDOCK_IMAGE_CARGO_HOME (default
# the job's CARGO_HOME or ~/.cargo: the image's cargo home with its bin folder and cargo configuration), GITHUB_ENV, GITHUB_PATH.
# A container without the mount, or with every slot taken, keeps the image's cargo home and says so (degrade, don't fail).
set -euo pipefail
mount="${PADDOCK_CI_MOUNT:-/sccache}"
slots="${PADDOCK_CARGO_SLOTS:-8}"
image_home="${PADDOCK_IMAGE_CARGO_HOME:-${CARGO_HOME:-${HOME:-/root}/.cargo}}"

if [ "$(uname -s)" != Linux ]; then exit 0; fi
if [ ! -d "$mount" ] || [ ! -w "$mount" ]; then
  echo "::notice::cargo-home-slot: no writable $mount in this container; the crates are downloaded in this job."
  exit 0
fi
if ! command -v flock >/dev/null; then
  echo "::warning::cargo-home-slot: flock is not installed, so the crates are downloaded in this job. Next: install util-linux in the image."
  exit 0
fi
dir="$mount/cargo-home"
mkdir -p "$dir"
got=""
for n in $(seq 1 "$slots"); do
  ready="$(mktemp -u)"
  # The holder takes the lock without waiting; once it has it, it says so and sleeps until the container dies.
  setsid nohup flock -n "$dir/$n.lock" bash -c ': > "$1"; exec sleep 86400' _ "$ready" >/dev/null 2>&1 &
  holder=$!
  for _ in $(seq 1 30); do
    if [ -e "$ready" ]; then break; fi
    if ! kill -0 "$holder" 2>/dev/null; then break; fi
    sleep 0.1
  done
  if [ -e "$ready" ]; then
    rm -f "$ready"
    got="$n"
    break
  fi
done
if [ -z "$got" ]; then
  echo "::warning::cargo-home-slot: all $slots cargo-home slots under $dir are held by running jobs; the crates are downloaded in this job. Next: raise PADDOCK_CARGO_SLOTS or wait for a job to finish."
  exit 0
fi
home="$dir/$got"
mkdir -p "$home"
# The image's cargo configuration and credentials (the decryp7 registry) come along; its bin folder stays on PATH for cargo-nextest, cargo-deny and the like.
for f in config.toml config credentials.toml credentials; do
  if [ -f "$image_home/$f" ]; then cp -f "$image_home/$f" "$home/$f"; fi
done
if [ -n "${GITHUB_ENV:-}" ]; then printf 'CARGO_HOME=%s\nCARGO_HOME_SLOT_HOLDER=%s\n' "$home" "$holder" >> "$GITHUB_ENV"; fi
if [ -n "${GITHUB_PATH:-}" ] && [ -d "$image_home/bin" ]; then echo "$image_home/bin" >> "$GITHUB_PATH"; fi
echo "cargo-home-slot: slot $got, CARGO_HOME=$home ($(du -sh "$home" 2>/dev/null | cut -f1) kept)"

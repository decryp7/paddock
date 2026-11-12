#!/usr/bin/env bash
# Linux Clippy of the shared workspace before a push (rule 50h, ER-9): a lint that exists only on Linux (for example
# `useless_conversion` on `mode_t`, a different width on Linux and macOS) is found here, not in CI. It runs CI's own command,
# `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` (ci.yml, "Clippy (workspace, every feature)"),
# in crates/, whenever the change touches crates/ (the crates, Cargo.toml, Cargo.lock, clippy.toml, deny.toml) or .cargo/.
# The toolchain is CI's: the CI image dev.decryptology.net/decryp7/paddock-ci:v<ci-image/VERSION>, run as it is (amd64, emulated
# on Apple Silicon), `rustc --version` and `cargo clippy --version` printed first. Container: --rm --cpus 4 --memory 12g.
#
#   bash tools/linux-clippy-touched.sh [--base origin/main] [--root <repo>] [--image <ref>] [--platform <p>] [--dry-run]
#
# Exit codes: 0 Clippy ran and passed (or nothing under crates/ or .cargo/ changed); Clippy's own code when it found something;
# 2 usage; 1 the base could not be diffed; 3 NOT MEASURED (no Docker daemon): the check did not run, so nothing is proven;
# 125 docker could not start the container (log in or pass --image) and 137 the container was killed (raise PADDOCK_LINUX_MEMORY),
# each with an ::error:: that names the next step.
# Environment: DOCKER (default docker); PADDOCK_HEAVY, a command prefix for the one-heavy-step lock (for example
# `heavy.sh`, ER-24), put before the container run; PADDOCK_LINUX_IMAGE and PADDOCK_LINUX_PLATFORM (the --image and --platform).
set -euo pipefail

base="origin/main"; root="$PWD"; dry=0; image="${PADDOCK_LINUX_IMAGE:-}"; platform="${PADDOCK_LINUX_PLATFORM:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --base) base="${2:?--base needs a value}"; shift ;;
    --root) root="${2:?--root needs a value}"; shift ;;
    --image) image="${2:?--image needs a value}"; shift ;;
    --platform) platform="${2:?--platform needs a value}"; shift ;;
    --dry-run) dry=1 ;;
    *) echo "::error::linux-clippy-touched: unknown option $1. Usage: [--base <ref>] [--root <repo>] [--image <ref>] [--platform <p>] [--dry-run]" >&2; exit 2 ;;
  esac
  shift
done

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
docker="${DOCKER:-docker}"

errfile=$(mktemp)
trap 'rm -f "$errfile"' EXIT
fork_point=$(git -C "$root" merge-base "$base" HEAD 2>"$errfile") || {
  echo "::error::linux-clippy-touched: no merge base of $base and HEAD ($(cat "$errfile")). Fetch it (git fetch origin) or pass --base." >&2; exit 1; }
files=$(git -C "$root" diff --name-only "$fork_point" -- crates .cargo 2>"$errfile") || {
  echo "::error::linux-clippy-touched: cannot diff against $fork_point ($(cat "$errfile")). Fetch the base (git fetch origin) or pass --base." >&2; exit 1; }

if [ -z "$files" ]; then
  echo "linux-clippy-touched: nothing under crates/ or .cargo/ changed since $base; nothing to check"
  exit 0
fi

if [ -z "$image" ]; then
  version=$(tr -d '[:space:]' <"$root/ci-image/VERSION" 2>"$errfile") || {
    echo "::error::linux-clippy-touched: cannot read ci-image/VERSION ($(cat "$errfile")). Run from a full checkout or pass --image." >&2; exit 1; }
  image="dev.decryptology.net/decryp7/paddock-ci:v$version"
  [ -n "$platform" ] || platform="linux/amd64" # the image is amd64 only (ci-image.yml)
fi

command_line='rustc --version && cargo clippy --version && cd crates && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings'
echo "linux-clippy-touched: $(printf '%s\n' "$files" | wc -l | tr -d ' ') file(s) changed under crates/ or .cargo/; CI's workspace Clippy in $image"
[ "$dry" = 0 ] || exit 0

if ! command -v "$docker" >/dev/null 2>&1 || ! "$docker" info >/dev/null 2>&1; then
  echo "::warning::linux-clippy-touched: Linux Clippy NOT MEASURED: Docker/OrbStack is not running. Agents: report this to the coordinator; do not start or restart it (ER-14). People: start it, then rerun: bash tools/linux-clippy-touched.sh" >&2
  exit 3
fi

rm -f "$errfile"; trap - EXIT
# No exec: docker's own failures (125, 137) get a cause and a next step here; every other code (Clippy's) passes through.
code=0
# shellcheck disable=SC2086 # PADDOCK_HEAVY is a command prefix, split on purpose
${PADDOCK_HEAVY:-} bash "$here/linux-container.sh" --id paddock --root "$root" --image "$image" ${platform:+--platform "$platform"} -- sh -c "$command_line" || code=$?
case "$code" in
  125) echo "::error::linux-clippy-touched: docker could not start the container (exit 125; usually the image $image could not be pulled). Run docker login dev.decryptology.net or pass --image <ref>." >&2 ;;
  137) echo "::error::linux-clippy-touched: the container was killed (exit 137; usually out of memory at ${PADDOCK_LINUX_MEMORY:-12g}). raise PADDOCK_LINUX_MEMORY (for example PADDOCK_LINUX_MEMORY=16g) and rerun." >&2 ;;
esac
exit "$code"

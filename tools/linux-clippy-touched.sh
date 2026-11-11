#!/usr/bin/env bash
# Pre-push Linux Clippy for the shared crates a change touches (rule 50h, ER-9): a lint that exists only on Linux (for example
# `useless_conversion` on `mode_t`, which is a different width on Linux and macOS) is found here, in the container of
# linux-container.sh (--rm, --cpus 4 --memory 12g), not in CI. Only crates under crates/ whose files differ from the base are
# checked. Without Docker this prints a warning with the next step and succeeds: the check is skipped, never faked.
#
#   bash tools/linux-clippy-touched.sh [--base origin/main] [--root <repo>] [--dry-run]
#
# Environment: DOCKER (default docker), PADDOCK_LINUX_BASE_IMAGE (rust:1-bookworm; set it to the CI image to match CI exactly).
set -euo pipefail

base="origin/main"; root="$PWD"; dry=0
while [ $# -gt 0 ]; do
  case "$1" in
    --base) base="${2:?--base needs a value}"; shift ;;
    --root) root="${2:?--root needs a value}"; shift ;;
    --dry-run) dry=1 ;;
    *) echo "::error::linux-clippy-touched: unknown option $1. Usage: [--base <ref>] [--root <repo>] [--dry-run]" >&2; exit 2 ;;
  esac
  shift
done

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
docker="${DOCKER:-docker}"

files=$(git -C "$root" diff --name-only "$base" -- crates 2>&1) || {
  echo "::error::linux-clippy-touched: cannot diff against $base ($files). Fetch it (git fetch origin) or pass --base." >&2; exit 1; }

packages=()
while IFS= read -r dir; do
  [ -n "$dir" ] || continue
  manifest="$root/crates/$dir/Cargo.toml"
  [ -f "$manifest" ] || continue # a crate deleted by the change has nothing to check
  name=$(sed -n 's/^name *= *"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)
  [ -n "$name" ] && packages+=("$name")
done < <(printf '%s\n' "$files" | sed -n 's#^crates/\([^/]*\)/.*#\1#p' | sort -u)

if [ ${#packages[@]} -eq 0 ]; then
  echo "linux-clippy-touched: no shared crate touched against $base; nothing to check"
  exit 0
fi

pargs=""
for p in "${packages[@]}"; do pargs="$pargs -p $p"; done
echo "linux-clippy-touched: Linux Clippy for${pargs}"
[ "$dry" = 0 ] || exit 0

if ! command -v "$docker" >/dev/null 2>&1 || ! "$docker" info >/dev/null 2>&1; then
  echo "::warning::linux-clippy-touched: Docker is not available, so the Linux Clippy check of${pargs} was SKIPPED. Start Docker or OrbStack and run: bash tools/linux-clippy-touched.sh (CI will run it otherwise)." >&2
  exit 0
fi

exec bash "$here/linux-container.sh" --id paddock --root "$root" -- sh -c "cd crates && cargo clippy --locked${pargs} --all-targets -- -D warnings"

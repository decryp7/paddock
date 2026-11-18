#!/usr/bin/env bash
# Regenerates (or with --check, verifies) an app's THIRD_PARTY_NOTICES.md: Rust crates through cargo-about, npm production
# dependencies through the root package-lock.json, both described by the `notices` block of apps/<app>/paddock.ci.json
# (conformance/src/notices; the one mechanism for every app).
# Usage: scripts/notices.sh <app> [--check]      (needs cargo and node; without --check it installs cargo-about 0.9.2 and runs `npm ci --ignore-scripts` when missing or incomplete;
#  --check installs nothing and exits 6, not measured, when either is missing)
set -euo pipefail
app=${1:?usage: scripts/notices.sh <app> [--check]}
shift
cd "$(dirname "$0")/.."

check=0
for a in "$@"; do [ "$a" = --check ] && check=1; done
# Exit 6 is ADR 0089's "not measured" (`./dev gen check` reads it as "could not look"): a check never installs anything (ER-45) and
# never reports stale from inputs that are not all there.
not_measured() {
  echo "::error::notices $app: not measured: $1. Next step: $2" >&2
  exit 6
}

# Pinned: the committed notices are cargo-about's output and a new release can find more licence texts. Any other installed
# version is replaced when writing, so every machine and CI produce the same file; --check installs nothing.
want=0.9.2
if [ "$(cargo about --version 2>/dev/null | awk '{print $2}')" != "$want" ]; then
  [ "$check" = 1 ] && not_measured "cargo-about $want is not installed" "cargo install cargo-about --version $want --locked --features cli"
  echo "installing cargo-about $want..." >&2
  cargo install cargo-about --version "$want" --locked --force --features cli
fi
# One npm workspace for the monorepo (ADR 0014 §1): the root lock and node_modules hold every app's packages. The generator
# compares the installed packages with the lock (a partial install, such as `npm ci -w conformance`, leaves most of them out).
if ! node conformance/bin/paddock-notices.mjs --config "apps/$app/paddock.ci.json" --verify-npm; then
  [ "$check" = 1 ] && not_measured "the npm install at the repo root does not match package-lock.json" "run \`npm ci --ignore-scripts\` at the repo root, then check again"
  echo "npm install is incomplete: npm ci --ignore-scripts" >&2
  npm ci --ignore-scripts
fi
node conformance/bin/paddock-notices.mjs --config "apps/$app/paddock.ci.json" "$@"

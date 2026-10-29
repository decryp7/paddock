#!/usr/bin/env bash
# Regenerates (or with --check, verifies) an app's THIRD_PARTY_NOTICES.md: Rust crates through cargo-about, npm production
# dependencies through the root package-lock.json, both described by the `notices` block of apps/<app>/paddock.ci.json
# (conformance/src/notices; the one mechanism for every app).
# Usage: scripts/notices.sh <app> [--check]      (needs cargo and node; installs cargo-about 0.9.2 if missing)
set -euo pipefail
app=${1:?usage: scripts/notices.sh <app> [--check]}
shift
cd "$(dirname "$0")/.."

# Pinned: the committed notices are cargo-about's output and a new release can find more licence texts.
if ! command -v cargo-about >/dev/null; then
  echo "installing cargo-about 0.9.2..." >&2
  cargo install cargo-about --version 0.9.2 --locked --features cli
fi
# One npm workspace for the monorepo (ADR 0014 §1): the root lock and node_modules hold every app's packages.
if [ ! -d node_modules ]; then
  npm ci --ignore-scripts
fi
node conformance/bin/paddock-notices.mjs --config "apps/$app/paddock.ci.json" "$@"

#!/usr/bin/env bash
# Regenerates (or with --check, verifies) an app's THIRD_PARTY_NOTICES.md: Rust crates through cargo-about, npm production
# dependencies through the root package-lock.json, both described by the `notices` block of apps/<app>/paddock.ci.json
# (conformance/src/notices; the one mechanism for every app).
# Usage: scripts/notices.sh <app> [--check]      (needs cargo and node)
# --provision (conformance/src/notices/cli.mjs) looks after the tools: without --check it installs cargo-about 0.9.2 and runs
# `npm ci --ignore-scripts` when missing or incomplete; --check installs nothing and exits 6 (not measured, ADR 0089) instead.
set -euo pipefail
app=${1:?usage: scripts/notices.sh <app> [--check]}
shift
cd "$(dirname "$0")/.."
exec node conformance/bin/paddock-notices.mjs --config "apps/$app/paddock.ci.json" --provision "$@"

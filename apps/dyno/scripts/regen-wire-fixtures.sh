#!/bin/sh
# Regenerates every wire fixture of the UI contract from the Rust types (crates/dyno-core/tests/wire), then fails when any file
# changed, so the change is committed together with its *.contract.spec.ts. Run it after a change to protocol.rs or a wire type
# (the pre-push hook runs it for such a push). Run from anywhere.
set -eu
cd "$(dirname "$0")/.."
DYNO_UPDATE_WIRE_FIXTURE=1 cargo test --locked -p dyno-core --test wire --quiet
if ! git diff --quiet -- 'app/src/app/core/*.wire.json'; then
  echo "::error::wire fixtures were stale: $(git diff --name-only -- 'app/src/app/core/*.wire.json' | tr '\n' ' ')" >&2
  echo "Next step: update the matching *.contract.spec.ts (the same values as TypeScript), then commit the regenerated *.wire.json with it." >&2
  exit 1
fi
echo "wire fixtures are current"

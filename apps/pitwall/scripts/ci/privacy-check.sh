#!/usr/bin/env bash
# ADR 0001: Pitwall's only outbound HTTP client lives in crates/pitwall-server/src/egress.rs. Fails when `reqwest` is
# referenced anywhere else in the Rust sources or in a crate manifest other than pitwall-server's (which declares it
# for egress.rs). The Rust test crates/pitwall-server/tests/it/privacy.rs checks the same from inside the crate; this
# grep also covers every other crate and file type.
set -euo pipefail
cd "$(dirname "$0")/../.."

fail=0
hits=$(grep -rIn --include='*.rs' 'reqwest' crates app/src-tauri 2>/dev/null | grep -v -e '^crates/pitwall-server/src/egress\.rs:' -e '^crates/pitwall-server/tests/it/privacy\.rs:' || true)
if [ -n "$hits" ]; then
  echo "::error::reqwest may only be used in crates/pitwall-server/src/egress.rs (ADR 0001):"
  echo "$hits"
  fail=1
fi
manifests=$(grep -rIln --include='Cargo.toml' 'reqwest' crates app/src-tauri 2>/dev/null | grep -v '^crates/pitwall-server/Cargo\.toml$' || true)
if [ -n "$manifests" ]; then
  echo "::error::only pitwall-server may depend on reqwest (ADR 0001):"
  echo "$manifests"
  fail=1
fi
exit "$fail"

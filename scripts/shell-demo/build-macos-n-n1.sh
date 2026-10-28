#!/usr/bin/env bash
# Builds the shell demo's .app twice on the macOS host for the update smoke (tools/vm-smoke.sh, conformance C21): build N
# (the version in Cargo.toml) and build N+1 (its patch number plus one, for the app and its agent, so the agent has a new
# ad-hoc cdhash and its heartbeat reports the new version).
#   scripts/shell-demo/build-macos-n-n1.sh <out>    -> <out>/n/<App>.app, <out>/n1/<App>.app, <out>/versions ("N N+1")
# The version bump is undone on every exit, failure included, so the checkout is left as it was found.
# PADDOCK_BUNDLE (default scripts/demo/bundle.sh) is the bundler; the tests replace it.
set -euo pipefail
cd "$(dirname "$0")/../.."
out="${1:?out folder}"
bundler="${PADDOCK_BUNDLE:-scripts/demo/bundle.sh}"
dir="${CARGO_TARGET_DIR:-apps/shell-demo/src-tauri/target}"
tomls=(apps/shell-demo/src-tauri/Cargo.toml apps/shell-demo/src-tauri/agent/Cargo.toml)
lock=apps/shell-demo/src-tauri/Cargo.lock

# The agent's `--self-test` prints "<name> <version> self-test" and then one line per check: the version is the header's.
agent_version() { "$1" --self-test | awk 'NR == 1 { print $2 }'; }

v_old=$(sed -n 's/^version = "\(.*\)"/\1/p' "${tomls[0]}" | head -n1)
v_new=$(echo "$v_old" | awk -F. '{ print $1 "." $2 "." $3 + 1 }')

# Refuse to start over uncommitted edits: the restore on exit would throw them away.
if [ -n "$(git status --porcelain -- "${tomls[@]}" "$lock")" ]; then
  echo "::error::uncommitted changes in ${tomls[*]} or $lock; commit or revert them first (this script bumps and restores them)"
  exit 1
fi

restore() { git checkout -- "${tomls[@]}" "$lock"; }
trap restore EXIT

rm -rf "$out"; mkdir -p "$out/n" "$out/n1"
bundle() { # <dest> <expected version>
  bash "$bundler" shell-demo app,dmg >/dev/null
  cp -R "$dir"/release/bundle/macos/*.app "$1/"
  local agent got
  agent=$(ls -d "$1"/*.app)/Contents/MacOS/shell-demo-agent
  got=$(agent_version "$agent")
  [ "$got" = "$2" ] || { echo "::error::the agent of the build for $2 reports '$got'"; exit 1; }
}
bundle "$out/n" "$v_old"
for toml in "${tomls[@]}"; do
  sed "s/^version = \"$v_old\"/version = \"$v_new\"/" "$toml" > "$toml.bump" && mv "$toml.bump" "$toml"
done
bundle "$out/n1" "$v_new"
echo "$v_old $v_new" > "$out/versions"
cat "$out/versions"

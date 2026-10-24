#!/usr/bin/env bash
# Builds Paddock Shell Demo's installers with `tauri build` and copies them to apps/shell-demo/dist (plan S18).
#   scripts/shell-demo/bundle.sh <bundles>     e.g. app,dmg | nsis | deb,appimage
# The UI is built by tauri.conf.json's beforeBuildCommand (ui/, `npm run shell-demo:build`). The demo has no updater key
# yet: its builds say "Updates are not available in this build" and no updater artifacts are made. A local build is for
# trying the app only; releases come from a `shell-demo-v*` tag in CI (rule 9).
set -euo pipefail
cd "$(dirname "$0")/../.."
bundles="${1:?bundles, e.g. app,dmg}"

dir="${CARGO_TARGET_DIR:-apps/shell-demo/src-tauri/target}"
if command -v cygpath >/dev/null; then dir="$(cygpath -u "$dir")"; fi
# A build folder kept between runs must not hand an older version's installer to this one.
rm -rf "$dir/release/bundle"

[ -d ui/node_modules ] || (cd ui && npm ci)
(cd apps/shell-demo && npm ci && npx tauri build --bundles "$bundles")

# Tauri seals the .app ad-hoc (signingIdentity "-"), but not the DMG around it: seal that too, so the DMG verifies with
# `codesign --verify --deep --strict` like the app (rule 23).
if [ "$(uname)" = "Darwin" ]; then
  identifier="$(node -p "require('./apps/shell-demo/src-tauri/tauri.conf.json').identifier")"
  find "$dir/release/bundle" -type f -name '*.dmg' -exec codesign --force --sign - --identifier "$identifier.dmg" {} \;
fi

rm -rf apps/shell-demo/dist
mkdir -p apps/shell-demo/dist
find "$dir/release/bundle" -type f \( -name '*-setup.exe' -o -name '*.dmg' -o -name '*.deb' -o -name '*.AppImage' \) \
  -exec cp {} apps/shell-demo/dist/ \;
ls -la apps/shell-demo/dist
version="$(node scripts/shell-demo-release.mjs version)"
if ! ls apps/shell-demo/dist | grep -q .; then
  echo "::error::no installers were built for $bundles"
  exit 1
fi
stale="$(ls apps/shell-demo/dist | grep -v "_${version}_" || true)"
if [ -n "$stale" ]; then
  echo "::error::installers that do not carry version $version: $stale"
  exit 1
fi

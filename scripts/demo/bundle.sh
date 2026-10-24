#!/usr/bin/env bash
# Builds a Paddock demo app's installers with `tauri build` and copies them to apps/<app>/dist (plan S18, X50 W8).
#   scripts/demo/bundle.sh <app> <bundles>     e.g. shell-demo app,dmg | workbench-demo nsis | shell-demo deb,appimage
# The UI is built by tauri.conf.json's beforeBuildCommand (ui/, `npm run <app>:build`). The demos have no updater key
# yet: their builds say "Updates are not available in this build" and no updater artifacts are made. A local build is for
# trying the app only; releases come from an `<app>-v*` tag in CI (rule 9).
set -euo pipefail
cd "$(dirname "$0")/../.."
app="${1:?app, e.g. shell-demo}"
bundles="${2:?bundles, e.g. app,dmg}"
node scripts/demo-release.mjs "$app" version >/dev/null  # an unknown app stops here

dir="${CARGO_TARGET_DIR:-apps/$app/src-tauri/target}"
if command -v cygpath >/dev/null; then dir="$(cygpath -u "$dir")"; fi
# A build folder kept between runs must not hand an older version's installer to this one.
rm -rf "$dir/release/bundle"

[ -d ui/node_modules ] || (cd ui && npm ci)
(cd apps/$app && npm ci && npx tauri build --bundles "$bundles")

# Tauri seals the .app ad-hoc (signingIdentity "-"), but not the DMG around it: seal that too, so the DMG verifies with
# `codesign --verify --deep --strict` like the app (rule 23).
if [ "$(uname)" = "Darwin" ]; then
  identifier="$(node -p "require('./apps/$app/src-tauri/tauri.conf.json').identifier")"
  find "$dir/release/bundle" -type f -name '*.dmg' -exec codesign --force --sign - --identifier "$identifier.dmg" {} \;
fi

rm -rf apps/$app/dist
mkdir -p apps/$app/dist
find "$dir/release/bundle" -type f \( -name '*-setup.exe' -o -name '*.dmg' -o -name '*.deb' -o -name '*.AppImage' \) \
  -exec cp {} apps/$app/dist/ \;
ls -la apps/$app/dist
version="$(node scripts/demo-release.mjs "$app" version)"
if ! ls apps/$app/dist | grep -q .; then
  echo "::error::no installers were built for $bundles"
  exit 1
fi
stale="$(ls apps/$app/dist | grep -v "_${version}_" || true)"
if [ -n "$stale" ]; then
  echo "::error::installers that do not carry version $version: $stale"
  exit 1
fi

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

# One npm workspace for the repo (ADR 0014 §1): the root install brings the UI's packages and the app's Tauri CLI.
[ -d node_modules ] || npm ci
here="$PWD/conformance/ci"
. "$here/tauri-build-lib.sh"
seed_build_inputs  # ADR 0062: Tauri's helpers come from the private store, never from GitHub
tauri_verbose "$bundles"
(cd apps/$app && tauri_build npx tauri build ${verbose[@]+"${verbose[@]}"} --bundles "$bundles")

# ADR 0032: no binary the Windows installer carries imports the VC++ runtime (the list comes from the Tauri configuration).
case ",$bundles," in
  *,nsis,* | *,msi,*)
    bin_dir="$dir/release"
    if command -v cygpath >/dev/null; then bin_dir="$(cygpath -m "$bin_dir")"; fi  # Node reads C:/..., not /c/...
    node conformance/release/windows-crt-imports.mjs --src-tauri "apps/$app/src-tauri" --bin-dir "$bin_dir"
    ;;
esac

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

#!/usr/bin/env bash
# Builds the installers with `tauri build` and copies them (and their updater signatures) to ./dist.
#   scripts/ci/tauri-bundle.sh <bundles>     e.g. nsis | deb,appimage | app,dmg
# With TAURI_SIGNING_PRIVATE_KEY and TAURI_UPDATER_PUBKEY set the updater artifacts are signed; without them (forks,
# pull requests, secrets not set yet) the build still succeeds, unsigned and without update support.
# Skips cleanly (exit 0) while app/src-tauri does not exist.
set -euo pipefail
cd "$(dirname "$0")/../.."
bundles="${1:?bundles, e.g. deb,appimage}"

if [ ! -f app/src-tauri/Cargo.toml ]; then
  echo "::warning::app/src-tauri does not exist yet: no installers to build."
  exit 0
fi

dir="${CARGO_TARGET_DIR:-target}"
if command -v cygpath >/dev/null; then dir="$(cygpath -u "$dir")"; fi
# The Windows runner keeps its target folder between runs: drop old installers so an earlier version's (signed)
# installer can never be collected into this release (it once shipped as latest.json's Windows download).
rm -rf "$dir/dist/bundle"

(
  cd app
  npm ci
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] && [ -n "${TAURI_UPDATER_PUBKEY:-}" ]; then
    mkdir -p ../target
    node -e 'process.stdout.write(JSON.stringify({ bundle: { createUpdaterArtifacts: true }, plugins: { updater: { pubkey: process.env.TAURI_UPDATER_PUBKEY } } }))' > ../target/updater.conf.json
    npx tauri build --bundles "$bundles" --config ../target/updater.conf.json -- --profile dist
  else
    echo '::warning::No updater signing secrets: this build cannot update itself.'
    unset TAURI_UPDATER_PUBKEY TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
    npx tauri build --bundles "$bundles" -- --profile dist
  fi
)

rm -rf dist
mkdir -p dist
find "$dir/dist/bundle" -type f \( -name '*-setup.exe' -o -name '*.dmg' -o -name '*.deb' -o -name '*.AppImage' \
  -o -name '*.app.tar.gz' -o -name '*.sig' \) -exec cp {} dist/ \;
ls -la dist
# Every versioned installer must carry this build's version (the .app.tar.gz has none in its name).
version="$(node -p "require('./app/src-tauri/tauri.conf.json').version")"
stale="$(ls dist | grep -E '_[0-9]+\.[0-9]+\.[0-9]+' | grep -v "_${version}_" || true)"
if [ -n "$stale" ]; then
  echo "::error::Installers from another version than $version: $stale"
  exit 1
fi

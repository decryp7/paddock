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

dir="${PADDOCK_BUILD_FOLDER:-${CARGO_TARGET_DIR:-target}}"
if command -v cygpath >/dev/null; then dir="$(cygpath -u "$dir")"; fi
# The Windows runner keeps its target folder between runs: drop old installers so an earlier version's (signed)
# installer can never be collected into this release (it once shipped as latest.json's Windows download).
rm -rf "$dir/dist/bundle"

# An unset secret arrives as an empty variable, which Tauri lets override signingIdentity "-" (codesign: `Signing with identity ""`).
[ -n "${APPLE_SIGNING_IDENTITY:-}" ] || unset APPLE_SIGNING_IDENTITY

# Ad-hoc builds ship WITHOUT the hardened runtime (tauri.macos.conf.json, Paddock rule 23): ad-hoc code under it cannot
# load a vendor-signed library ("different Team IDs"). A real signing identity (APPLE_SIGNING_IDENTITY, not "-") turns it
# back on, as notarization requires.
extra=()
if [ "$(uname -s)" = Darwin ] && [ -n "${APPLE_SIGNING_IDENTITY:-}" ] && [ "$APPLE_SIGNING_IDENTITY" != "-" ]; then
  mkdir -p target
  echo '{"bundle":{"macOS":{"hardenedRuntime":true}}}' > target/hardened.conf.json
  extra=(--config ../target/hardened.conf.json)
fi

# Secrets pasted into Gitea often carry a trailing newline or a line break; both keys are single-line base64.
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then export TAURI_SIGNING_PRIVATE_KEY="$(printf '%s' "$TAURI_SIGNING_PRIVATE_KEY" | tr -d ' \t\r\n')"; fi
if [ -n "${TAURI_UPDATER_PUBKEY:-}" ]; then export TAURI_UPDATER_PUBKEY="$(printf '%s' "$TAURI_UPDATER_PUBKEY" | tr -d ' \t\r\n')"; fi

(
  cd app
  npm ci
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] && [ -n "${TAURI_UPDATER_PUBKEY:-}" ]; then
    mkdir -p ../target
    node -e 'process.stdout.write(JSON.stringify({ bundle: { createUpdaterArtifacts: true }, plugins: { updater: { pubkey: process.env.TAURI_UPDATER_PUBKEY } } }))' > ../target/updater.conf.json
    npx tauri build --bundles "$bundles" --config ../target/updater.conf.json ${extra[@]+"${extra[@]}"} -- --profile dist
  else
    echo '::warning::No updater signing secrets: this build cannot update itself.'
    unset TAURI_UPDATER_PUBKEY TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
    npx tauri build --bundles "$bundles" ${extra[@]+"${extra[@]}"} -- --profile dist
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

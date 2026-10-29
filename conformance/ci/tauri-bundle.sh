#!/usr/bin/env bash
# Builds the installers with `tauri build` and copies them (and their updater signatures) to ./dist (plan X10; Pitwall's
# scripts/ci/tauri-bundle.sh, shared).
#   bash tauri-bundle.sh <bundles> [--app-dir app] [--root <dir>] [--prepare <command>]       e.g. nsis | deb,appimage | app,dmg
# --root is where `npm ci` runs (default: the app dir; the monorepo's root, whose one lock installs every app); --prepare is a
# command run there right after it (an app whose UI imports the shared library builds it: `npm run build -w ui`). The build
# folder is the build-folder action's ($PADDOCK_BUILD_FOLDER), else cargo's $CARGO_TARGET_DIR, else target/.
# With TAURI_SIGNING_PRIVATE_KEY and TAURI_UPDATER_PUBKEY set the updater artifacts are signed (the keys are stripped of
# whitespace first); without them (forks, pull requests, secrets not set yet) the build still succeeds, unsigned and without
# update support. Skips cleanly (exit 0) while <app-dir>/src-tauri does not exist. Run from the repository root.
set -euo pipefail
bundles="${1:?bundles, e.g. deb,appimage}"
app_dir="app"
root=""
prepare=""
shift
while [ $# -gt 0 ]; do
  case "$1" in
    --app-dir) app_dir="${2:?--app-dir needs a value}"; shift 2 ;;
    --root) root="${2:?--root needs a value}"; shift 2 ;;
    --prepare) prepare="${2:?--prepare needs a command}"; shift 2 ;;
    *) echo "::error::unknown option $1"; exit 2 ;;
  esac
done
root="${root:-$app_dir}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# On a host runner the stable-workspace action moved the checkout: build from its physical path (sccache keys a compile on it).
if [ -n "${PADDOCK_WORKSPACE:-}" ]; then
  ws="$PADDOCK_WORKSPACE"
  if command -v cygpath >/dev/null; then ws="$(cygpath -u "$ws")"; fi
  cd "$ws"
fi

if [ ! -f "$app_dir/src-tauri/Cargo.toml" ]; then
  echo "::warning::$app_dir/src-tauri does not exist yet: no installers to build."
  exit 0
fi

dir="${PADDOCK_BUILD_FOLDER:-${CARGO_TARGET_DIR:-target}}"
if command -v cygpath >/dev/null; then dir="$(cygpath -u "$dir")"; fi
# A host runner keeps its target folder between runs: drop old installers so an earlier version's (signed) installer can
# never be collected into this release (it once shipped as latest.json's Windows download).
rm -rf "$dir/dist/bundle"

# Tauri's DMG step skips its Finder window scripting on CI (no Automation permission prompt on a host runner).
export CI="${CI:-true}"

# Secrets pasted into Gitea often carry a trailing newline or a line break; both keys are single-line base64.
eval "$(node "$here/../release/strip-keys.mjs")"

# An unset secret arrives as an empty variable, which Tauri lets override signingIdentity "-" (codesign: `Signing with identity ""`).
[ -n "${APPLE_SIGNING_IDENTITY:-}" ] || unset APPLE_SIGNING_IDENTITY

(cd "$root" && npm ci)
if [ -n "$prepare" ]; then (cd "$root" && bash -euo pipefail -c "$prepare"); fi
(
  cd "$app_dir"
  # Ad-hoc builds ship WITHOUT the hardened runtime (tauri.macos.conf.json, rule 23): ad-hoc code under it cannot load a
  # vendor-signed library ("different Team IDs"). A real signing identity turns it back on, as notarization requires.
  extra=()
  mkdir -p ../target
  if [ "$(uname -s)" = Darwin ] && [ -n "${APPLE_SIGNING_IDENTITY:-}" ] && [ "$APPLE_SIGNING_IDENTITY" != "-" ]; then
    echo '{"bundle":{"macOS":{"hardenedRuntime":true}}}' > ../target/hardened.conf.json
    extra=(--config ../target/hardened.conf.json)
  fi
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] && [ -n "${TAURI_UPDATER_PUBKEY:-}" ]; then
    node -e 'process.stdout.write(JSON.stringify({ bundle: { createUpdaterArtifacts: true }, plugins: { updater: { pubkey: process.env.TAURI_UPDATER_PUBKEY } } }))' > ../target/updater.conf.json
    npx tauri build --bundles "$bundles" --config ../target/updater.conf.json ${extra[@]+"${extra[@]}"} -- --profile dist
  else
    echo '::warning::No updater signing secrets: this build cannot update itself.'
    unset TAURI_UPDATER_PUBKEY TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
    npx tauri build --bundles "$bundles" ${extra[@]+"${extra[@]}"} -- --profile dist
  fi
)

# ADR 0032: no binary the Windows installer carries imports the VC++ runtime (a PC without the redistributable cannot start
# it; CI runners have it, so only this check shows it). The list comes from the Tauri configuration (the app, its
# externalBin sidecars, the .exe and .dll resources), never from the build folder, which a host runner keeps between runs.
case ",$bundles," in
  *,nsis,* | *,msi,*)
    # Node on Windows reads C:/... paths, not Git Bash's /c/...
    bin_dir="$dir/dist"
    if command -v cygpath >/dev/null; then bin_dir="$(cygpath -m "$bin_dir")"; fi
    node "$here/../release/windows-crt-imports.mjs" --src-tauri "$app_dir/src-tauri" --bin-dir "$bin_dir"
    ;;
esac

rm -rf dist
mkdir -p dist
find "$dir/dist/bundle" -type f \( -name '*-setup.exe' -o -name '*.dmg' -o -name '*.deb' -o -name '*.AppImage' \
  -o -name '*.app.tar.gz' -o -name '*.sig' \) -exec cp {} dist/ \;
ls -la dist
# Every versioned installer must carry this build's version (the .app.tar.gz has none in its name).
version="$(node -p "require('./$app_dir/src-tauri/tauri.conf.json').version")"
stale="$(ls dist | grep -E '_[0-9]+\.[0-9]+\.[0-9]+' | grep -v "_${version}_" || true)"
if [ -n "$stale" ]; then
  echo "::error::Installers from another version than $version: $stale"
  exit 1
fi

#!/usr/bin/env bash
# Builds Pitcrew on a Mac and prints what to do next (docs/macos.md). Installs nothing system-wide: it checks the
# prerequisites, runs `npm ci`, builds the app bundle and the .dmg, and tells you where they are.
#
#   bash scripts/macos-bootstrap.sh            # build
#   bash scripts/macos-bootstrap.sh --check    # only check the prerequisites
set -euo pipefail

cd "$(dirname "$0")/.."
root=$(pwd)
ok() { printf '  \033[32mok\033[0m  %s\n' "$1"; }
fail() { printf '  \033[31mmissing\033[0m  %s\n      %s\n' "$1" "$2"; missing=1; }
missing=0

echo "Pitcrew macOS bootstrap"
[ "$(uname -s)" = Darwin ] || { echo "This script is for macOS."; exit 1; }
sw_vers -productVersion | awk -F. '{ exit !($1 >= 13) }' && ok "macOS $(sw_vers -productVersion) (13 or newer)" ||
  fail "macOS 13 or newer" "SMAppService (the launch agent registration) needs macOS 13."
xcode-select -p >/dev/null 2>&1 && ok "Xcode command line tools ($(xcode-select -p))" ||
  fail "Xcode command line tools" "run: xcode-select --install"
command -v cargo >/dev/null && ok "Rust $(rustc --version | cut -d' ' -f2)" ||
  fail "Rust" "install rustup from https://rustup.rs (user-level), then open a new shell"
want=$(tr -d ' v\r\n' < ../../.nvmrc | cut -d. -f1)
if command -v node >/dev/null; then
  have=$(node -p 'process.versions.node.split(".")[0]')
  [ "$have" = "$want" ] && ok "Node $(node --version)" || fail "Node $want" "found Node $(node --version); install Node $want (e.g. with nvm)"
else
  fail "Node $want" "install Node $want (https://nodejs.org or nvm)"
fi
[ "$missing" = 0 ] || { echo; echo "Install what is missing, then run this again."; exit 1; }
[ "${1:-}" = --check ] && exit 0

echo
echo "Building (a first build takes a few minutes)..."
(cd ../.. && npm ci --no-audit --no-fund)  # the monorepo's one npm workspace
# Unsigned updater artefacts unless the signing key is in the environment (docs/releasing.md).
(cd app && CI=true npx tauri build --bundles app,dmg -- --profile dist)

target="${CARGO_TARGET_DIR:-$root/target}"
app=$(find "$target/dist/bundle/macos" -maxdepth 1 -name 'Pitcrew.app' 2>/dev/null | head -1)
dmg=$(find "$target/dist/bundle/dmg" -maxdepth 1 -name '*.dmg' 2>/dev/null | head -1)
echo
echo "Built:"
echo "  app: ${app:-not found}"
echo "  dmg: ${dmg:-not found}"
cat <<'NEXT'

Next steps (docs/macos.md has the details):
  1. Open the .dmg and drag Pitcrew to Applications. The build is not notarised: the first time, right-click
     Pitcrew in Applications ▸ Open ▸ Open (or System Settings ▸ Privacy & Security ▸ Open Anyway).
  2. Start Pitcrew. In Settings ▸ Service click "Turn on", then allow Pitcrew in
     System Settings ▸ General ▸ Login Items (macOS asks once).
  3. Runners ▸ Add runner: server URL, the registration token from the server's runner settings, labels such as
     macos-latest,macos-arm64.
  4. Run: bash scripts/macos-verify.sh
NEXT

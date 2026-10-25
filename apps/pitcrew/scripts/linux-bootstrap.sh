#!/usr/bin/env bash
# Builds Pitcrew on Linux and prints what to do next (docs/linux.md). Installs nothing system-wide: it checks the
# prerequisites (and tells you the apt command for what is missing), runs `npm ci`, builds the .deb and the AppImage,
# and tells you where they are.
#
#   bash scripts/linux-bootstrap.sh            # build
#   bash scripts/linux-bootstrap.sh --check    # only check the prerequisites
set -euo pipefail

cd "$(dirname "$0")/.."
root=$(pwd)
ok() { printf '  \033[32mok\033[0m  %s\n' "$1"; }
fail() { printf '  \033[31mmissing\033[0m  %s\n      %s\n' "$1" "$2"; missing=1; }
missing=0

echo "Pitcrew Linux bootstrap"
[ "$(uname -s)" = Linux ] || { echo "This script is for Linux."; exit 1; }
case "$(uname -m)" in
  x86_64 | aarch64) ok "architecture $(uname -m)" ;;
  *) fail "architecture" "Pitcrew builds for x86_64 and aarch64 only (found $(uname -m))" ;;
esac
command -v cargo >/dev/null && ok "Rust $(rustc --version | cut -d' ' -f2)" ||
  fail "Rust" "install rustup from https://rustup.rs (user-level), then open a new shell"
want=$(tr -d ' v\r\n' < ../../.nvmrc | cut -d. -f1)
if command -v node >/dev/null; then
  have=$(node -p 'process.versions.node.split(".")[0]')
  [ "$have" = "$want" ] && ok "Node $(node --version)" || fail "Node $want" "found Node $(node --version); install Node $want (e.g. with nvm)"
else
  fail "Node $want" "install Node $want (https://nodejs.org or nvm)"
fi
command -v pkg-config >/dev/null || fail "pkg-config" "sudo apt install pkg-config build-essential"
for pkg in webkit2gtk-4.1 gtk+-3.0 librsvg-2.0 ayatana-appindicator3-0.1; do
  pkg-config --exists "$pkg" 2>/dev/null && ok "$pkg (development files)" ||
    fail "$pkg" "sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf file xdg-utils"
done
command -v systemctl >/dev/null && ok "systemd (the service runs as a user unit)" ||
  echo "  note  no systemd: Pitcrew can still run in the foreground (pitcrew-service run), but Settings > Service needs systemd"
[ "$missing" = 0 ] || { echo; echo "Install what is missing, then run this again."; exit 1; }
[ "${1:-}" = --check ] && exit 0

echo
echo "Building (a first build takes several minutes)..."
(cd ../.. && npm ci --no-audit --no-fund)  # the monorepo's one npm workspace
# Unsigned updater artefacts unless the signing key is in the environment (docs/releasing.md).
(cd app && CI=true npx tauri build --bundles deb,appimage -- --profile dist)

target="${CARGO_TARGET_DIR:-$root/target}"
deb=$(find "$target/dist/bundle/deb" -maxdepth 1 -name '*.deb' 2>/dev/null | head -1)
appimage=$(find "$target/dist/bundle/appimage" -maxdepth 1 -name '*.AppImage' 2>/dev/null | head -1)
echo
echo "Built:"
echo "  deb:      ${deb:-not found}"
echo "  AppImage: ${appimage:-not found}"
cat <<'NEXT'

Next steps (docs/linux.md has the details):
  1. Install: sudo apt install ./Pitcrew_*.deb   (or chmod +x the AppImage and run it)
  2. Start Pitcrew. In Settings > Service click "Turn on": Pitcrew installs a systemd *user* service, no root needed.
  3. To keep the runners going when nobody is logged in: loginctl enable-linger "$USER"   (your decision, once)
  4. Runners > Add runner: server URL, the registration token from the server's runner settings, labels such as
     ubuntu-latest:host
  5. Run: bash scripts/linux-verify.sh
NEXT

#!/usr/bin/env bash
# Builds an app on this machine and prints what to do next (plan X47; the apps' linux-bootstrap.sh and macos-bootstrap.sh in
# one). Installs nothing system-wide: it checks the prerequisites (and prints the command for what is missing), runs
# `npm ci`, builds the installers and says where they are.
#
#   bash paddock/tools/bootstrap.sh --name Pitcrew [--id pitcrew] [--root <app repo>] [--check] [--next <file>]
#
#   --name   product name (bundle names: Pitcrew.app, Pitcrew_*.deb)         required
#   --id     lower-case id (default: the name in lower case)
#   --root   the app repository (default: the current folder)
#   --check  only check the prerequisites
#   --next   a text file printed after the build (the app's own next steps, its docs/macos.md or docs/linux.md summary)
#   --min-macos N   oldest macOS (default 13)
# CARGO_TARGET_DIR is honoured. PADDOCK_TOOLS_OS=Darwin|Linux overrides `uname -s` (tests only).
set -euo pipefail

name=""; id=""; root="$PWD"; check=0; next=""; min_macos=13
while [ $# -gt 0 ]; do
  case "$1" in
    --name) name="${2:?--name needs a value}"; shift ;;
    --id) id="${2:?--id needs a value}"; shift ;;
    --root) root="${2:?--root needs a value}"; shift ;;
    --next) next="${2:?--next needs a value}"; shift ;;
    --min-macos) min_macos="${2:?--min-macos needs a value}"; shift ;;
    --check) check=1 ;;
    *) echo "bootstrap: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
[ -n "$name" ] || { echo "bootstrap: --name <product name> is needed" >&2; exit 2; }
[ -n "$id" ] || id=$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]')
[ -d "$root/app" ] || { echo "bootstrap: $root has no app/ folder; pass --root <app repository>" >&2; exit 2; }
cd "$root"

green=""; red=""; reset=""
if [ -t 1 ]; then green=$'\033[32m'; red=$'\033[31m'; reset=$'\033[0m'; fi
missing=0
ok() { printf '  %sok%s  %s\n' "$green" "$reset" "$1"; }
fail() { printf '  %smissing%s  %s\n      %s\n' "$red" "$reset" "$1" "$2"; missing=1; }

os="${PADDOCK_TOOLS_OS:-$(uname -s)}"
case "$os" in
  Darwin) kind=macos; bundles="app,dmg" ;;
  Linux) kind=linux; bundles="deb,appimage" ;;
  *) echo "bootstrap: $os is not supported by this script (macOS and Linux; on Windows use the NSIS build in CI)" >&2; exit 1 ;;
esac

echo "$name $kind bootstrap"
if [ "$kind" = macos ]; then
  ver=$(sw_vers -productVersion 2>/dev/null || echo 0)
  if [ "${ver%%.*}" -ge "$min_macos" ] 2>/dev/null; then ok "macOS $ver ($min_macos or newer)"
  else fail "macOS $min_macos or newer" "found $ver; the launch agent registration (SMAppService) needs macOS $min_macos"; fi
  if xcode-select -p >/dev/null 2>&1; then ok "Xcode command line tools"
  else fail "Xcode command line tools" "run: xcode-select --install"; fi
else
  arch="${PADDOCK_TOOLS_ARCH:-$(uname -m)}"
  case "$arch" in
    x86_64 | aarch64) ok "architecture $arch" ;;
    *) fail "architecture" "$name builds for x86_64 and aarch64 only (found $arch)" ;;
  esac
fi
if command -v cargo >/dev/null 2>&1; then ok "Rust $(rustc --version | cut -d' ' -f2)"
else fail "Rust" "install rustup from https://rustup.rs (user-level), then open a new shell"; fi

want=$(tr -d ' v\r\n' < app/.nvmrc | cut -d. -f1)
if command -v node >/dev/null 2>&1; then
  have=$(node -p 'process.versions.node.split(".")[0]')
  if [ "$have" = "$want" ]; then ok "Node $(node --version)"
  else fail "Node $want" "found Node $(node --version); install Node $want (app/.nvmrc; e.g. with nvm)"; fi
else
  fail "Node $want" "install Node $want (https://nodejs.org or nvm)"
fi

if [ "$kind" = linux ]; then
  if command -v pkg-config >/dev/null 2>&1; then
    for pkg in webkit2gtk-4.1 gtk+-3.0 librsvg-2.0 ayatana-appindicator3-0.1; do
      if pkg-config --exists "$pkg" 2>/dev/null; then ok "$pkg (development files)"
      else fail "$pkg" "sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf file xdg-utils"; fi
    done
  else
    fail "pkg-config" "sudo apt install pkg-config build-essential"
  fi
  if command -v systemctl >/dev/null 2>&1; then ok "systemd (a background service runs as a user unit)"
  else echo "  note  no systemd: a background service can run in the foreground, but the service switch in Settings needs systemd"; fi
fi

if [ "$missing" != 0 ]; then echo; echo "Install what is missing, then run this again."; exit 1; fi
[ "$check" = 1 ] && exit 0

echo
echo "Building (a first build takes several minutes)..."
(cd app && npm ci --no-audit --no-fund)
# Unsigned updater artefacts unless the signing key is in the environment (docs/releasing.md).
(cd app && CI=true npx tauri build --bundles "$bundles" -- --profile dist)

target="${CARGO_TARGET_DIR:-$root/target}"
echo
echo "Built:"
if [ "$kind" = macos ]; then
  echo "  app: $(find "$target/dist/bundle/macos" -maxdepth 1 -name "$name.app" 2>/dev/null | head -1 | grep . || echo "not found")"
  echo "  dmg: $(find "$target/dist/bundle/dmg" -maxdepth 1 -name '*.dmg' 2>/dev/null | head -1 | grep . || echo "not found")"
else
  echo "  deb:      $(find "$target/dist/bundle/deb" -maxdepth 1 -name '*.deb' 2>/dev/null | head -1 | grep . || echo "not found")"
  echo "  AppImage: $(find "$target/dist/bundle/appimage" -maxdepth 1 -name '*.AppImage' 2>/dev/null | head -1 | grep . || echo "not found")"
fi
echo
if [ -n "$next" ] && [ -f "$next" ]; then
  cat "$next"
else
  echo "Next steps: install the build, start $name, and run the installed-app check:"
  echo "  bash paddock/tools/verify-installed.sh --name $name --id $id"
fi

#!/usr/bin/env bash
# Checks the built Paddock Shell Demo.app, its DMG and the copy inside the DMG (rule 23): sealed ad-hoc (signingIdentity
# "-"), `codesign --verify --deep --strict` passes on the .app and the DMG, sealed resources, the hardened runtime off
# while ad-hoc, every Mach-O inside verified on its own. Never starts the app.
#   scripts/shell-demo/verify-macos.sh [path/to/App.app] [path/to/App.dmg]
set -euo pipefail
cd "$(dirname "$0")/../.."
dir="${CARGO_TARGET_DIR:-apps/shell-demo/src-tauri/target}"
bundle="$dir/release/bundle"
app="${1:-$(find "$bundle/macos" -maxdepth 1 -name '*.app' | head -1)}"
dmg="${2:-$(find "$bundle/dmg" -maxdepth 1 -name '*.dmg' 2>/dev/null | head -1 || true)}"
[ -d "${app:-}" ] || { echo "::error::no .app bundle found (looked in $bundle/macos)"; exit 1; }
[ -f "${dmg:-}" ] || { echo "::error::no .dmg found (looked in $bundle/dmg)"; exit 1; }

check_app() {
  local app="$1" sig bin
  codesign --verify --deep --strict --verbose=2 "$app"
  sig=$(codesign -dvvv "$app" 2>&1)
  echo "$sig"
  if echo "$sig" | grep -q 'flags=.*linker-signed'; then
    echo "::error::$app is only linker-signed: set bundle.macOS.signingIdentity to \"-\" (ad-hoc seal)"
    return 1
  fi
  if echo "$sig" | grep -Eq 'flags=0x[0-9a-f]+\(.*adhoc'; then
    # Ad-hoc code under the hardened runtime cannot load vendor-signed libraries (rule 23): it stays off until the app
    # has a real signing identity.
    if echo "$sig" | grep -Eq 'flags=0x[0-9a-f]+\(.*runtime'; then
      echo "::error::$app is ad-hoc signed with the hardened runtime on: set bundle.macOS.hardenedRuntime to false"
      return 1
    fi
  else
    echo "$sig" | grep -q 'Authority=' || { echo "::error::$app is neither ad-hoc nor identity signed"; return 1; }
    echo "::notice::$app is signed with a real identity"
  fi
  if echo "$sig" | grep -q 'Sealed Resources=none' || ! echo "$sig" | grep -q 'Sealed Resources'; then
    echo "::error::$app has no sealed resources"
    return 1
  fi
  while IFS= read -r bin; do
    file -b "$bin" | grep -q 'Mach-O' || continue
    codesign --verify --strict "$bin" || { echo "::error::$bin does not verify"; return 1; }
    if codesign -dv "$bin" 2>&1 | grep -q 'flags=.*linker-signed'; then
      echo "::error::$bin is only linker-signed"
      return 1
    fi
    echo "verified: ${bin#"$app"/}"
  done < <(find "$app/Contents" -type f \( -perm -u+x -o -name '*.dylib' \))
  echo "bundle OK: $app"
}

check_app "$app"

# The DMG itself (sealed ad-hoc by bundle.sh, and intact), then the app as a user gets it from the DMG (mounted
# read-only, never opened).
codesign --verify --deep --strict --verbose=2 "$dmg"
codesign -dv "$dmg" 2>&1 | grep -Eq 'flags=0x[0-9a-f]+\(.*adhoc|Authority=' || { echo "::error::$dmg is not signed"; exit 1; }
hdiutil verify -quiet "$dmg"
mnt=$(mktemp -d)
trap 'hdiutil detach "$mnt" -quiet || true; rmdir "$mnt" 2>/dev/null || true' EXIT
hdiutil attach "$dmg" -nobrowse -readonly -noautoopen -mountpoint "$mnt" -quiet
inner=$(find "$mnt" -maxdepth 1 -name '*.app' | head -1)
[ -d "$inner" ] || { echo "::error::no .app inside $dmg"; exit 1; }
check_app "$inner"
echo "dmg OK: $dmg"

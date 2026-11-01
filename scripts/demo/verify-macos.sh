#!/usr/bin/env bash
# Checks a built Paddock demo app: the .app, its DMG and the copy inside the DMG (rule 23): sealed ad-hoc (signingIdentity
# "-"), `codesign --verify --deep --strict` passes on the .app and the DMG, sealed resources, the hardened runtime off
# while ad-hoc, every Mach-O inside verified on its own and carrying the app's TeamIdentifier, and every sidecar started
# from inside the bundle with `--self-test` (it loads, answers and touches nothing). Never starts the app itself.
# The sidecars are the app's row of scripts/demo-release.mjs (none for the workbench demo, which has no externalBin).
#   scripts/demo/verify-macos.sh <app> [path/to/App.app] [path/to/App.dmg]
set -euo pipefail
cd "$(dirname "$0")/../.."
demo="${1:?app, e.g. shell-demo}"
sidecars="$(node scripts/demo-release.mjs "$demo" sidecars)"
dir="${CARGO_TARGET_DIR:-apps/$demo/src-tauri/target}"
bundle="$dir/release/bundle"
app="${2:-$(find "$bundle/macos" -maxdepth 1 -name '*.app' | head -1)}"
dmg="${3:-$(find "$bundle/dmg" -maxdepth 1 -name '*.dmg' 2>/dev/null | head -1 || true)}"
[ -d "${app:-}" ] || { echo "::error::no .app bundle found (looked in $bundle/macos)"; exit 1; }
[ -f "${dmg:-}" ] || { echo "::error::no .dmg found (looked in $bundle/dmg)"; exit 1; }

check_app() {
  local app="$1" sig bin team bin_team
  codesign --verify --deep --strict --verbose=2 "$app"
  sig=$(codesign -dvvv "$app" 2>&1)
  echo "$sig"
  if grep -q 'flags=.*linker-signed' <<<"$sig"; then
    echo "::error::$app is only linker-signed: set bundle.macOS.signingIdentity to \"-\" (ad-hoc seal)"
    return 1
  fi
  if grep -Eq 'flags=0x[0-9a-f]+\(.*adhoc' <<<"$sig"; then
    # Ad-hoc code under the hardened runtime cannot load vendor-signed libraries (rule 23): it stays off until the app
    # has a real signing identity.
    if grep -Eq 'flags=0x[0-9a-f]+\(.*runtime' <<<"$sig"; then
      echo "::error::$app is ad-hoc signed with the hardened runtime on: set bundle.macOS.hardenedRuntime to false"
      return 1
    fi
  else
    grep -q 'Authority=' <<<"$sig" || { echo "::error::$app is neither ad-hoc nor identity signed"; return 1; }
    echo "::notice::$app is signed with a real identity"
  fi
  team=$(echo "$sig" | sed -n 's/^TeamIdentifier=//p')
  if grep -q 'Sealed Resources=none' <<<"$sig" || ! grep -q 'Sealed Resources' <<<"$sig"; then
    echo "::error::$app has no sealed resources"
    return 1
  fi
  while IFS= read -r bin; do
    kind=$(file -b "$bin")
    [[ "$kind" == *Mach-O* ]] || continue
    codesign --verify --strict "$bin" || { echo "::error::$bin does not verify"; return 1; }
    bin_sig=$(codesign -dv "$bin" 2>&1)
    if grep -q 'flags=.*linker-signed' <<<"$bin_sig"; then
      echo "::error::$bin is only linker-signed"
      return 1
    fi
    bin_team=$(echo "$bin_sig" | sed -n 's/^TeamIdentifier=//p')
    if [ "$bin_team" != "$team" ]; then
      echo "::error::$bin has TeamIdentifier '$bin_team', the app '$team' (different Team IDs cannot load each other)"
      return 1
    fi
    echo "verified: ${bin#"$app"/}"
  done < <(find "$app/Contents" -type f \( -perm -u+x -o -name '*.dylib' \))
  # Rule 23: every sidecar starts from the built app.
  for sidecar in $sidecars; do
    out=$("$app/Contents/MacOS/$sidecar" --self-test) || { echo "::error::$sidecar --self-test failed"; return 1; }
    echo "$out"
  done
  echo "bundle OK: $app"
}

check_app "$app"

# The DMG itself (sealed ad-hoc by bundle.sh, and intact), then the app as a user gets it from the DMG (mounted
# read-only, never opened).
codesign --verify --deep --strict --verbose=2 "$dmg"
# Read into a variable first: under pipefail, `grep -q` closing the pipe early fails codesign with SIGPIPE.
dmg_sig=$(codesign -dv "$dmg" 2>&1)
grep -Eq 'flags=0x[0-9a-f]+\(.*adhoc|Authority=' <<<"$dmg_sig" || { echo "::error::$dmg is not signed"; exit 1; }
hdiutil verify -quiet "$dmg"
mnt=$(mktemp -d)
trap 'hdiutil detach "$mnt" -quiet || true; rmdir "$mnt" 2>/dev/null || true' EXIT
hdiutil attach "$dmg" -nobrowse -readonly -noautoopen -mountpoint "$mnt" -quiet
inners=$(find "$mnt" -maxdepth 1 -name '*.app'); inner=${inners%%$'\n'*}
[ -d "$inner" ] || { echo "::error::no .app inside $dmg"; exit 1; }
check_app "$inner"
echo "dmg OK: $dmg"

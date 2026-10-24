#!/usr/bin/env bash
# Checks the built Spotter.app and the copy inside the DMG next to it (Paddock rule 23):
#   - sealed: `codesign --verify --deep --strict`, not linker-signed only (SMAppService refuses that, error -67056),
#     resources sealed;
#   - an ad-hoc build has the hardened runtime OFF on every Mach-O (bundle.macOS.hardenedRuntime false in
#     tauri.macos.conf.json): ad-hoc code under the hardened runtime cannot load a vendor-signed library;
#   - every bundled Mach-O verifies on its own and carries the main executable's TeamIdentifier ("different Team IDs");
#   - every sidecar (bundle.externalBin in tauri.conf.json) starts from the built app: `<sidecar> --version` exits 0;
#   - executables that link @rpath libraries have the bundle's run path (docs/suite-lessons.md).
#   scripts/ci/macos-verify-bundle.sh [path/to/Spotter.app]
# SIDECARS="a b" overrides the sidecar names read from tauri.conf.json; SIDECAR_TIMEOUT (seconds, default 30) bounds each run.
set -euo pipefail
app="${1:-}"
[ -z "$app" ] || [ ! -d "$app" ] || app="$(cd "$app" && pwd)"
cd "$(dirname "$0")/../.."
dir="${CARGO_TARGET_DIR:-target}"
[ -n "$app" ] || app="$(find "$dir/dist/bundle/macos" -maxdepth 1 -name '*.app' 2>/dev/null | head -1)"
[ -d "${app:-}" ] || { echo "::error::no .app bundle found (looked in ${1:-$dir/dist/bundle/macos})"; exit 1; }
app="$(cd "$app" && pwd)"
if [ -z "${SIDECARS+x}" ]; then
  SIDECARS="$(node -p "(require('./app/src-tauri/tauri.conf.json').bundle.externalBin || []).map((p) => p.split('/').pop()).join(' ')")"
fi
timeout_s="${SIDECAR_TIMEOUT:-30}"

# The Team ID of a signed Mach-O ("" when it has none).
team_of() { codesign -dv "$1" 2>&1 | sed -n 's/^TeamIdentifier=//p' | sed 's/^not set$//'; }
# The signature's flags, e.g. "adhoc,runtime" ("" when there are none).
flags_of() { codesign -dv "$1" 2>&1 | sed -n 's/^CodeDirectory .*flags=0x[0-9a-f]*(\([^)]*\)).*/\1/p' | head -1; }
# Runs a command with a time limit (macOS has no `timeout`).
bounded() { perl -e 'alarm shift; exec @ARGV or exit 127' "$timeout_s" "$@"; }

check_app() {
  local app="$1" sig main main_team bin bin_team flags name out paths
  codesign --verify --deep --strict --verbose=2 "$app" || { echo "::error::$app does not verify"; return 1; }
  sig=$(codesign -dvvv "$app" 2>&1)
  echo "$sig"
  if echo "$sig" | grep -q 'flags=.*linker-signed'; then
    echo "::error::$app is only linker-signed: set bundle.macOS.signingIdentity to \"-\" (ad-hoc seal)"
    return 1
  fi
  echo "$sig" | grep -q 'Signature=adhoc' || echo "::notice::$app is signed with a real identity"
  if ! echo "$sig" | grep -q '^Sealed Resources' || echo "$sig" | grep -q 'Sealed Resources=none'; then
    echo "::error::$app has no sealed resources"
    return 1
  fi

  main="$app/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")"
  [ -f "$main" ] || { echo "::error::$app has no main executable ($main)"; return 1; }
  main_team=$(team_of "$main")
  while IFS= read -r bin; do
    file -b "$bin" | grep -q 'Mach-O' || continue
    codesign --verify --strict "$bin" || { echo "::error::$bin does not verify"; return 1; }
    flags=$(flags_of "$bin")
    case ",$flags," in *,linker-signed,*) echo "::error::$bin is only linker-signed"; return 1 ;; esac
    if [[ ",$flags," == *,adhoc,* && ",$flags," == *,runtime,* ]]; then
      echo "::error::$bin is ad-hoc signed with the hardened runtime on: set bundle.macOS.hardenedRuntime to false (it comes back with a real signing identity)"
      return 1
    fi
    bin_team=$(team_of "$bin")
    if [ "$bin_team" != "$main_team" ]; then
      echo "::error::$bin has TeamIdentifier '${bin_team:-none}', the app has '${main_team:-none}': library validation refuses it (re-sign it with the app's identity)"
      return 1
    fi
    echo "verified: ${bin#"$app"/} (${flags:-no flags}, team ${bin_team:-none})"
  done < <(find "$app/Contents" -type f \( -perm -u+x -o -name '*.dylib' \))

  # A bundled lib directory needs an rpath on the executables that link it.
  if [ -d "$app/Contents/Resources/lib" ]; then
    for bin in "$app"/Contents/MacOS/*; do
      [ -f "$bin" ] || continue
      paths=$(otool -l "$bin" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }')
      if otool -L "$bin" | grep -q '@rpath/' && ! echo "$paths" | grep -q '@executable_path/../Resources/lib'; then
        echo "::error::$bin links @rpath libraries but has no @executable_path/../Resources/lib run path (has: ${paths:-none})"
        return 1
      fi
    done
  fi

  # Really start every sidecar from the built app: a signature, library-validation or run-path failure aborts it.
  for name in $SIDECARS; do
    bin="$app/Contents/MacOS/$name"
    [ -x "$bin" ] || { echo "::error::the sidecar $name is missing from $app"; return 1; }
    if ! out=$(bounded "$bin" --version 2>&1 </dev/null); then
      echo "$out" | head -20
      echo "::error::the sidecar $name does not start from $app ('$name --version' failed)"
      return 1
    fi
    echo "started: $name --version -> $(echo "$out" | head -1)"
  done
  echo "bundle OK: $app"
}

check_app "$app"

dmg=$(find "$(dirname "$app")/../dmg" -maxdepth 1 -name '*.dmg' 2>/dev/null | head -1 || true)
if [ -n "$dmg" ]; then
  mnt=$(mktemp -d)
  trap 'hdiutil detach "$mnt" -quiet || true; rmdir "$mnt" 2>/dev/null || true' EXIT
  hdiutil attach "$dmg" -nobrowse -readonly -mountpoint "$mnt" -quiet
  inner=$(find "$mnt" -maxdepth 1 -name '*.app' | head -1)
  [ -d "$inner" ] || { echo "::error::no .app inside $dmg"; exit 1; }
  check_app "$inner"
fi

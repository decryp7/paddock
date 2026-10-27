#!/usr/bin/env bash
# Verifies a built macOS bundle (and the copy inside the DMG next to it): sealed ad-hoc or identity-signed (never only
# linker-signed: Gatekeeper calls a quarantined download "damaged", SMAppService refuses it with -67056), every nested
# Mach-O verifies, is not linker-signed and carries the main executable's Team ID (library validation refuses "different
# Team IDs"), an ad-hoc signature never carries the hardened runtime (ad-hoc code under it cannot load a vendor-signed
# library; rule 23, it comes back with a real signing identity), executables resolve bundled libraries by run path, and
# every sidecar starts from the built app (bounded by SIDECAR_TIMEOUT seconds, default 30).
#
#   macos-verify-bundle.sh [--bundle-dir <dir with macos/ and dmg/>] [--sidecar name[:arg ...]]... [path/to/App.app]
#
# --sidecar worker:--self-test   starts Contents/MacOS/worker with the arguments after the colon (default --self-test)
# Default bundle dir: ${PADDOCK_BUILD_FOLDER:-${CARGO_TARGET_DIR:-target}}/dist/bundle (the build-folder action's, else cargo's). Output of every command is captured before it is matched:
# `cmd | grep -q` under pipefail fails when grep exits early and the writer gets SIGPIPE (docs/lessons.md).
set -euo pipefail

# First line of the command's captured output (no `| head -1`: it can SIGPIPE the writer under pipefail).
first() { local out; out=$("$@" 2>/dev/null || true); printf '%s\n' "${out%%$'\n'*}"; }

# True when the captured text $1 matches the extended regex $2 (no pipe, so no SIGPIPE).
has() { grep -Eq -- "$2" <<<"$1"; }

bundle="${PADDOCK_BUILD_FOLDER:-${CARGO_TARGET_DIR:-target}}/dist/bundle"
timeout_s="${SIDECAR_TIMEOUT:-30}"
sidecars=()
app=""
while [ $# -gt 0 ]; do
  case "$1" in
    --bundle-dir) bundle="${2:?--bundle-dir needs a path}"; shift 2 ;;
    --sidecar) sidecars+=("${2:?--sidecar needs name[:args]}"); shift 2 ;;
    -*) echo "::error::unknown option $1"; exit 2 ;;
    *) app="$1"; shift ;;
  esac
done
[ -n "$app" ] || app=$(first find "$bundle/macos" -maxdepth 1 -name '*.app')
[ -d "${app:-}" ] || { echo "::error::no .app bundle found (looked in $bundle/macos)"; exit 1; }

# The Team ID of a signed Mach-O ("" when it has none).
team_of() {
  local out
  out=$(codesign -dv "$1" 2>&1 || true)
  sed -n 's/^TeamIdentifier=//p' <<<"$out" | sed 's/^not set$//'
}

# Runs a command with a time limit (macOS has no `timeout`).
bounded() { perl -e 'alarm shift; exec @ARGV or exit 127' "$timeout_s" "$@"; }

load_check() {
  local app="$1" spec name args out
  # ${arr[@]+...}: an empty array is "unbound" under set -u on the bash 3.2 of macOS.
  for spec in ${sidecars[@]+"${sidecars[@]}"}; do
    name="${spec%%:*}"
    args="--self-test"
    [ "$spec" = "$name" ] || args="${spec#*:}"
    [ -x "$app/Contents/MacOS/$name" ] || { echo "::error::sidecar $name is missing or not executable in $app"; return 1; }
    # shellcheck disable=SC2086
    out=$(bounded "$app/Contents/MacOS/$name" $args 2>&1 </dev/null) || { printf '%s\n' "$out"; echo "::error::sidecar $name of $app does not start ($args)"; return 1; }
    echo "started: $name $args"
  done
}

check_app() {
  local app="$1" sig bin out main_team bin_team info flags main spec is_sidecar
  codesign --verify --deep --strict --verbose=2 "$app"
  sig=$(codesign -dvvv "$app" 2>&1)
  echo "$sig"
  if has "$sig" 'flags=.*linker-signed'; then
    echo "::error::$app is only linker-signed: set bundle.macOS.signingIdentity to \"-\" (ad-hoc seal)"
    return 1
  fi
  if ! has "$sig" 'flags=0x[0-9a-f]+\(.*adhoc'; then
    has "$sig" 'Authority=' || { echo "::error::$app is neither ad-hoc nor identity signed"; return 1; }
    echo "::notice::$app is signed with a real identity"
  fi
  has "$sig" 'Sealed Resources=none' && { echo "::error::$app has no sealed resources"; return 1; }
  has "$sig" 'Sealed Resources' || { echo "::error::$app has no sealed resources"; return 1; }

  # The main executable is the one Info.plist names (not "the first executable": sidecars sit next to it). Without a plist
  # (a fixture, a non-macOS test run) it is the first executable that is not a named sidecar.
  main=""
  if [ -f "$app/Contents/Info.plist" ] && [ -x /usr/libexec/PlistBuddy ]; then
    main=$(first /usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")
  fi
  if [ -z "$main" ]; then
    for bin in "$app"/Contents/MacOS/*; do
      [ -f "$bin" ] && [ -x "$bin" ] || continue
      is_sidecar=0
      for spec in ${sidecars[@]+"${sidecars[@]}"}; do
        [ "${spec%%:*}" = "$(basename "$bin")" ] && is_sidecar=1
      done
      [ "$is_sidecar" = 0 ] || continue
      main=$(basename "$bin")
      break
    done
  fi
  [ -f "$app/Contents/MacOS/${main:-?}" ] || { echo "::error::$app has no main executable ($app/Contents/MacOS/${main:-?})"; return 1; }
  main_team=$(team_of "$app/Contents/MacOS/$main")
  while IFS= read -r bin; do
    info=$(file -b "$bin")
    has "$info" 'Mach-O' || continue
    codesign --verify --strict "$bin" || { echo "::error::$bin does not verify"; return 1; }
    out=$(codesign -dv "$bin" 2>&1 || true)
    if has "$out" 'flags=.*linker-signed'; then
      echo "::error::$bin is only linker-signed"
      return 1
    fi
    flags=$(sed -n 's/^CodeDirectory .*flags=0x[0-9a-f]*(\([^)]*\)).*/\1/p' <<<"$out")
    if [[ ",$flags," == *,adhoc,* && ",$flags," == *,runtime,* ]]; then
      echo "::error::$bin is ad-hoc signed with the hardened runtime on: set bundle.macOS.hardenedRuntime to false (it comes back with a real signing identity)"
      return 1
    fi
    bin_team=$(team_of "$bin")
    if [ "$bin_team" != "$main_team" ]; then
      echo "::error::$bin has Team ID '$bin_team', the app has '$main_team': library validation will refuse it (re-sign it ad hoc)"
      return 1
    fi
    echo "verified: ${bin#"$app"/} (${flags:-no flags}, team ${bin_team:-none})"
  done < <(find "$app/Contents" -type f \( -perm -u+x -o -name '*.dylib' \))

  # A bundled lib directory needs an rpath on the executables that link it.
  if [ -d "$app/Contents/Resources/lib" ]; then
    for bin in "$app"/Contents/MacOS/*; do
      [ -f "$bin" ] || continue
      local paths links
      paths=$(otool -l "$bin" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }')
      links=$(otool -L "$bin")
      if has "$links" '@rpath/' && ! has "$paths" '@executable_path/../Resources/lib'; then
        echo "::error::$bin links @rpath libraries but has no @executable_path/../Resources/lib run path (has: ${paths:-none})"
        return 1
      fi
    done
  fi
  load_check "$app"
  echo "bundle OK: $app"
}

check_app "$app"

dmg=$(first find "$bundle/dmg" -maxdepth 1 -name '*.dmg')
if [ -n "$dmg" ]; then
  mnt=$(mktemp -d)
  trap 'hdiutil detach "$mnt" -quiet || true' EXIT
  hdiutil attach "$dmg" -nobrowse -readonly -mountpoint "$mnt" -quiet
  inner=$(first find "$mnt" -maxdepth 1 -name '*.app')
  [ -d "${inner:-}" ] || { echo "::error::no .app inside $dmg"; exit 1; }
  check_app "$inner"
fi

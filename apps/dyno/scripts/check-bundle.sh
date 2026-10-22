#!/usr/bin/env bash
# Checks what `tauri build` produced, so a change to the bundle layout cannot ship binaries that fail to start.
#   scripts/check-bundle.sh <bundle-dir>          (e.g. target/dist/bundle)
# Linux: unpacks the .deb and checks that every ELF binary's RPATH/RUNPATH holds only $ORIGIN-relative or system
#   folders (never a build directory), and that dyno-app and dyno-agent are present.
# macOS: checks that every Mach-O's LC_RPATH is @executable_path/@loader_path relative or a system folder, that the
#   .app seal verifies (ad-hoc is fine; a linker-signed bundle is refused by SMAppService), and that the agent is bundled.
# Windows is not checked here (no run paths); the Windows runner's tests and the installer smoke cover it.
set -euo pipefail
bundle="${1:?bundle directory}"
fail=0
err() { echo "::error::$*"; fail=1; }

# A run path is acceptable when it is relative to the binary or inside a system library folder.
ok_path() {
  case "$1" in
    '$ORIGIN'*|@executable_path*|@loader_path*|/usr/lib*|/usr/local/lib*|/lib*|/System/*|/opt/homebrew/lib) return 0 ;;
    *) return 1 ;;
  esac
}

case "$(uname -s)" in
  Linux)
    deb=$(find "$bundle/deb" -name '*.deb' 2>/dev/null | head -n 1)
    [[ -n "$deb" ]] || { echo "::error::no .deb under $bundle/deb"; exit 1; }
    tree=$(mktemp -d)
    trap 'rm -rf "$tree"' EXIT
    dpkg-deb -x "$deb" "$tree"
    found_app=0 found_agent=0
    while IFS= read -r f; do
      case "$(basename "$f")" in dyno-app|dyno) found_app=1 ;; dyno-agent) found_agent=1 ;; esac
      file -b "$f" | grep -q '^ELF' || continue
      paths=$(readelf -d "$f" 2>/dev/null | sed -n 's/.*(R\(UN\)\{0,1\}PATH).*\[\(.*\)\]/\2/p')
      echo "run paths of ${f#"$tree"}: ${paths:-none}"
      IFS=: read -ra entries <<< "$paths"
      for p in "${entries[@]:-}"; do
        [[ -z "$p" ]] || ok_path "$p" || err "${f#"$tree"} has the run path $p (a build directory?)"
      done
    done < <(find "$tree" -type f -perm -u+x)
    (( found_app )) || err "the .deb has no dyno-app / dyno executable"
    (( found_agent )) || err "the .deb has no dyno-agent executable"
    ;;
  Darwin)
    app=$(find "$bundle/macos" -maxdepth 1 -name '*.app' 2>/dev/null | head -n 1)
    [[ -n "$app" ]] || { echo "::error::no .app under $bundle/macos"; exit 1; }
    codesign --verify --deep --strict "$app" || err "$app does not verify (broken or missing seal)"
    codesign -dv "$app" 2>&1 | grep -E 'Signature|Identifier|flags' || true
    found_agent=0
    while IFS= read -r f; do
      [[ "$(basename "$f")" == dyno-agent ]] && found_agent=1
      file -b "$f" | grep -q 'Mach-O' || continue
      paths=$(otool -l "$f" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }')
      echo "run paths of ${f#"$app"/}: $(echo "$paths" | paste -sd: -)"
      while IFS= read -r p; do
        [[ -z "$p" ]] || ok_path "$p" || err "${f#"$app"/} has the run path $p (a build directory?)"
      done <<< "$paths"
    done < <(find "$app/Contents" -type f -perm -u+x)
    (( found_agent )) || err "the .app has no dyno-agent executable"
    ;;
  *)
    echo "check-bundle: nothing to check on $(uname -s)"
    ;;
esac
exit "$fail"

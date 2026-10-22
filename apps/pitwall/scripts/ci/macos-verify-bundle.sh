#!/usr/bin/env bash
# Checks the built Pitwall.app: sealed ad-hoc (signingIdentity "-"; a linker-signed bundle is refused by SMAppService,
# error -67056) and every executable resolves its bundled native libraries by run path (docs/suite-lessons.md).
#   scripts/ci/macos-verify-bundle.sh [path/to/Pitwall.app]
set -euo pipefail
cd "$(dirname "$0")/../.."
dir="${CARGO_TARGET_DIR:-target}"
app="${1:-$(find "$dir/dist/bundle/macos" -maxdepth 1 -name '*.app' | head -1)}"
[ -d "${app:-}" ] || { echo "::error::no .app bundle found (looked in $dir/dist/bundle/macos)"; exit 1; }

codesign --verify --deep --strict --verbose=2 "$app"
sig=$(codesign -dv "$app" 2>&1)
echo "$sig"
if echo "$sig" | grep -q 'flags=.*linker-signed'; then
  echo "::error::$app is only linker-signed: set bundle.macOS.signingIdentity to \"-\" (ad-hoc seal)"
  exit 1
fi
echo "$sig" | grep -q 'Signature=adhoc' || echo "::notice::$app is signed with a real identity"

# A bundled lib directory needs an rpath on the executables that link it.
if [ -d "$app/Contents/Resources/lib" ]; then
  for bin in "$app"/Contents/MacOS/*; do
    [ -f "$bin" ] || continue
    paths=$(otool -l "$bin" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }')
    if otool -L "$bin" | grep -q '@rpath/' && ! echo "$paths" | grep -q '@executable_path/../Resources/lib'; then
      echo "::error::$bin links @rpath libraries but has no @executable_path/../Resources/lib run path (has: ${paths:-none})"
      exit 1
    fi
  done
fi
echo "bundle OK: $app"

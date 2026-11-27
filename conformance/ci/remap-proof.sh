#!/usr/bin/env bash
# TEMPORARY (remap measurement, branch tmp/sccache-remap-measure only; never lands). Run in crates/.
set +e
S="$RUNNER_TEMP/remap-proof"
if command -v cygpath >/dev/null 2>&1; then S="$(cygpath -m "$S")"; fi
rm -rf "$S"; mkdir -p "$S"
echo "REMAP: os=$RUNNER_OS slot=${PADDOCK_CI_SLOT:-?} folder=${PADDOCK_BUILD_FOLDER:-?}"
echo "REMAP: ci.toml:"; sed 's/^/REMAP:   /' .cargo/ci.toml
build() { # <name> <yes|no>
  local T="$S/$1"
  local extra=()
  if [ "$2" = yes ]; then extra=(--config "target.'cfg(all())'.rustflags=['--remap-path-prefix=$T=/paddock-target']"); fi
  env -u RUSTC_WRAPPER cargo build --locked -p serde_json --config "build.target-dir='$T'" "${extra[@]}" >"$S/$1.log" 2>&1 || { echo "REMAP: build $1 failed"; tail -8 "$S/$1.log"; }
}
report() { # <A> <B> <label>
  for c in itoa serde_core serde memchr serde_json; do
    fa=$(find "$S/$1" -name "lib${c}-*.rlib" 2>/dev/null | head -1); fb=$(find "$S/$2" -name "lib${c}-*.rlib" 2>/dev/null | head -1)
    if [ ! -f "$fa" ] || [ ! -f "$fb" ]; then echo "REMAP: $3 $c: no rlib"; continue; fi
    if cmp -s "$fa" "$fb"; then r=IDENTICAL; else r=DIFFERENT; fi
    echo "REMAP: $3 $c $(basename "$fa"): $r size=$(wc -c <"$fa")/$(wc -c <"$fb") folder-names-in-A=[$(grep -a -o -E 'remap-(with|without)-[ab]|paddock-target' "$fa" | sort | uniq -c | tr '\n' ' ')]"
  done
}
build with-a yes; build with-b yes; build without-a no; build without-b no
report with-a with-b WITH-REMAP
report without-a without-b WITHOUT-REMAP
echo "REMAP: slot paths in the job's own build folder (remapped by ci.toml):"
for c in itoa serde_core serde memchr; do
  for f in $(find "${PADDOCK_BUILD_FOLDER:-/nonexistent}" "$PWD/target" -name "lib${c}-*.rlib" 2>/dev/null | head -2); do
    echo "REMAP:   $(basename "$f") sha=$(sha256sum "$f" | cut -c1-16) paths=[$(grep -a -o -E 'target-[A-Za-z0-9-]*-s[0-9]+|paddock-target' "$f" | sort | uniq -c | tr '\n' ' ')]"
  done
done

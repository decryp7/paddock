#!/bin/sh
# Builds the stub paddock-setup.exe and the harness installer into $1 (default: ./out), with the include staged beside them
# the way an app's src-tauri/windows/ holds them. Needs makensis. Usage: build.sh [outdir]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-$here/out}
mkdir -p "$out"
cp "$here/../paddock-per-machine.nsh" "$here/harness-hooks.nsh" "$here/harness.nsi" "$here/harness.txt" "$here/stub-setup.nsi" "$here/stub-server.nsi" "$out/"
printf '!define PADDOCK_STOP_IMAGES "harness.exe,harness-agent.exe"\n' > "$out/paddock-per-machine.generated.nsh"
cd "$out"
if [ -n "${SETUP_EXE:-}" ]; then cp "$SETUP_EXE" paddock-setup.exe; else makensis -V2 stub-setup.nsi; fi
makensis -V2 stub-server.nsi
makensis -V2 harness.nsi
echo "built in $out"

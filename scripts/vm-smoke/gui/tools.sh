#!/usr/bin/env bash
# The host-side tools of `scripts/vm-smoke --gui`, built once and cached by the hash of their sources (scripts/vm-smoke/README.md):
#   axdrive  compiled from axdrive.swift with swiftc (for the GUI test image; never run on the host)
#   vncdo    vncdotool in a venv installed from vnc-requirements.txt with --require-hashes (the install drag, VM's VNC only)
# Prints `axdrive=<path>` and `vncdo=<path>`. Cache: $PADDOCK_VM_GUI_CACHE, default $TART_HOME/.paddock-vm-gui (TART_HOME
# default $HOME/.tart, as in run.mjs). Exit 2 with a message when swiftc or python3 is missing.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cache="${PADDOCK_VM_GUI_CACHE:-${TART_HOME:-$HOME/.tart}/.paddock-vm-gui}"
mkdir -p "$cache"

command -v swiftc >/dev/null || { echo "gui/tools.sh: swiftc is needed (xcode-select --install)" >&2; exit 2; }
command -v python3 >/dev/null || { echo "gui/tools.sh: python3 is needed" >&2; exit 2; }

ax_hash=$(shasum -a 256 "$here/axdrive.swift" | cut -c1-16)
ax="$cache/axdrive-$ax_hash"
if [ ! -x "$ax" ]; then
  nice -n 10 swiftc -O -o "$ax.tmp" "$here/axdrive.swift" >&2
  mv "$ax.tmp" "$ax"
fi

req_hash=$( { shasum -a 256 "$here/vnc-requirements.txt"; python3 -c 'import sys; print(sys.version_info[:2])'; } | shasum -a 256 | cut -c1-16)
venv="$cache/vnc-venv-$req_hash"
# A venv records its own path, so it is built in place; the stamp marks a finished install (a half one is rebuilt).
if [ ! -f "$venv/.complete" ]; then
  rm -rf "$venv"
  python3 -m venv "$venv" >&2
  if ! "$venv/bin/pip" install -q --disable-pip-version-check --require-hashes --only-binary :all: -r "$here/vnc-requirements.txt" >&2; then
    rm -rf "$venv"
    echo "gui/tools.sh: installing vncdotool from the pinned hashes failed (the list is for CPython 3.12 on macOS arm64; see its header)" >&2
    exit 2
  fi
  touch "$venv/.complete"
fi
echo "axdrive=$ax"
echo "vncdo=$venv/bin/vncdo"

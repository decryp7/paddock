#!/bin/sh
# DIAG-TEMP
case "$*" in *"--crate-name paddock_workspace_hack "*)
  { echo "=== PWD=$(pwd)"; prev=; for a in "$@"; do case "$prev" in --extern) p=${a#*=}; echo "EXT ${a%%=*} $(basename "$p") $(shasum "$p" 2>&1 | cut -c1-12)";; esac; prev=$a; done
    for f in $(ls paddock-workspace-hack/src/*.rs paddock-workspace-hack/Cargo.toml 2>/dev/null); do echo "SRC $f $(shasum "$f" | cut -c1-12)"; done
    printf 'ARG %s\n' "$@" | grep -v -- '--extern\|^ARG [a-z_0-9]*=' | md5 | sed 's/^/ARGHASH /'; } >> "${HOME}/.paddock-ci/diag-wrapper.log" ;;
esac
unset CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR
exec sccache "$@"

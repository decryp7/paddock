#!/bin/sh
# DIAG-TEMP
case "$*" in *"--crate-name paddock_env "*|*"--crate-name paddock_log "*)
  { echo "=== PWD=$(pwd)"; printf 'ARG %s\n' "$@"; env | grep -E '^(CARGO|OUT_DIR|RUST|SCCACHE)' | sort | sed 's/^/ENV /'; } >> "${HOME}/.paddock-ci/diag-wrapper.log" ;;
esac
unset CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR
exec sccache "$@"

#!/usr/bin/env bash
# Checks a developer machine for the tools in docs/dev-setup.md. With --install (macOS), installs the missing tools
# first. Never edits the shell profile; prints the lines to add instead.
set -euo pipefail

install=false
[ "${1:-}" = "--install" ] && install=true

missing=()
check() { # label, command that succeeds when present
  if eval "$2" >/dev/null 2>&1; then printf '  ok       %s\n' "$1"; else printf '  missing  %s\n' "$1"; missing+=("$1"); fi
}

report() {
  missing=()
  echo "Tools:"
  check "rustc" "rustc --version"
  check "rustfmt" "cargo fmt --version"
  check "clippy" "cargo clippy --version"
  check "windows-msvc target" "rustup target list --installed | grep -q x86_64-pc-windows-msvc"
  check "node" "node --version"
  check "npm" "npm --version"
  check "sccache" "sccache --version"
  check "cargo-nextest" "cargo nextest --version"
  check "cargo-deny" "cargo deny --version"
  check "cargo-llvm-cov" "cargo llvm-cov --version"
  check "docker" "docker version --format '{{.Server.Version}}'"
  if [ "$(uname)" = "Darwin" ]; then check "Xcode Command Line Tools" "xcode-select -p"; fi
  echo "Environment:"
  if [ "${RUSTC_WRAPPER:-}" = "sccache" ]; then echo "  ok       RUSTC_WRAPPER=sccache"; else echo "  not set  RUSTC_WRAPPER=sccache"; fi
  if [ -n "${CARGO_BUILD_JOBS:-}" ]; then echo "  ok       CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS"; else echo "  not set  CARGO_BUILD_JOBS"; fi
}

report

if $install && [ "${#missing[@]}" -gt 0 ]; then
  if [ "$(uname)" != "Darwin" ] || ! command -v brew >/dev/null; then
    echo "--install needs macOS with Homebrew; install the missing tools as in docs/dev-setup.md." >&2
    exit 1
  fi
  for m in "${missing[@]}"; do
    case "$m" in
      sccache | cargo-nextest) brew install "$m" ;;
      cargo-deny | cargo-llvm-cov) cargo install --locked "$m" ;;
      "windows-msvc target") rustup target add x86_64-pc-windows-msvc ;;
      *) echo "  install by hand: $m (see docs/dev-setup.md)" ;;
    esac
  done
  report
fi

if [ "${RUSTC_WRAPPER:-}" != "sccache" ] || [ -z "${CARGO_BUILD_JOBS:-}" ]; then
  cat <<'EOF'

Add to your shell profile (~/.zshenv on macOS), between builds:
  export RUSTC_WRAPPER=sccache
  export SCCACHE_CACHE_SIZE=60G
  export CARGO_BUILD_JOBS=2
EOF
fi

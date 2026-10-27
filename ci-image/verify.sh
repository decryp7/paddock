#!/usr/bin/env bash
# Checks that the Paddock CI image has everything a job needs, at the pinned versions, and writes the release manifest.
# Runs at the end of the image build (the build fails on any problem) and can be run in a container any time.
#
#   paddock-ci-verify [--manifest FILE]
#
# Expected values come from the environment the Dockerfile sets: EXPECT_NODE_MAJOR, EXPECT_SCCACHE, EXPECT_CHROME,
# EXPECT_PLAYWRIGHT, EXPECT_DUCKDB, EXPECT_SHELLCHECK. An expectation that is not set is not checked.
set -uo pipefail

manifest=/etc/paddock-ci-release
if [ "${1:-}" = "--manifest" ]; then manifest=${2:?--manifest needs a file}; fi

problems=()
lines=()
note() { lines+=("$1"); }
bad() { problems+=("$1"); }

# tool <label> <command> <args...>: the command exists and runs; its first output line goes into the manifest (and into $OUT).
# Cargo subcommands are checked the way they are used: by running `cargo <name> --version`.
OUT=
tool() {
  local label=$1; shift
  OUT=
  if ! command -v "$1" >/dev/null 2>&1; then bad "$label is missing"; return 1; fi
  local full
  if ! full=$("$@" 2>&1); then bad "$label is missing or broken ($*)"; return 1; fi
  OUT=$(printf '%s\n' "$full" | head -1)
  note "$label: ${OUT:-present}"
}

tool rustc rustc --version
tool cargo cargo --version
tool rustfmt rustfmt --version
tool cargo-clippy cargo clippy --version
for t in nextest llvm-cov deny hakari machete; do
  tool "cargo-$t" cargo "$t" --version
done
command -v xvfb-run >/dev/null 2>&1 && note "xvfb-run: present" || bad "xvfb-run (xvfb) is missing"
pkg-config --exists webkit2gtk-4.1 2>/dev/null && note "webkit2gtk-4.1: $(pkg-config --modversion webkit2gtk-4.1)" || bad "webkit2gtk-4.1 (Tauri's Linux libraries) is missing"
pkg-config --exists gtk+-3.0 2>/dev/null || bad "gtk+-3.0 (Tauri's Linux libraries) is missing"

if tool sccache sccache --version && [ -n "${EXPECT_SCCACHE:-}" ]; then
  case "$OUT" in *" ${EXPECT_SCCACHE}") ;; *) bad "sccache is '$OUT', the image pins ${EXPECT_SCCACHE}" ;; esac
fi

if tool node node --version && [ -n "${EXPECT_NODE_MAJOR:-}" ]; then
  major=${OUT#v}; major=${major%%.*}
  if [ "$major" != "$EXPECT_NODE_MAJOR" ]; then bad "node is $OUT, .nvmrc wants major $EXPECT_NODE_MAJOR"; fi
fi
tool npm npm --version

# shellcheck prints its version on the second line ("version: 0.11.0").
if command -v shellcheck >/dev/null 2>&1; then
  scv=
  while IFS= read -r l; do case "$l" in "version: "*) scv=${l#version: }; break ;; esac; done < <(shellcheck --version 2>&1)
  note "shellcheck: ${scv:-unknown}"
  if [ -n "${EXPECT_SHELLCHECK:-}" ] && [ "$scv" != "$EXPECT_SHELLCHECK" ]; then bad "shellcheck is '${scv}', the image pins ${EXPECT_SHELLCHECK}"; fi
else
  bad "shellcheck is missing"
fi

pw=${PLAYWRIGHT_BROWSERS_PATH:-/ms-playwright}
for b in chromium webkit; do
  if ls -d "$pw/$b"-* >/dev/null 2>&1; then note "playwright $b: $(ls -d "$pw/$b"-* | head -1)"; else bad "Playwright $b is missing in $pw"; fi
done
[ -z "${EXPECT_PLAYWRIGHT:-}" ] || note "playwright pinned: $EXPECT_PLAYWRIGHT"

chrome=${CHROMIUM_TEST_PATH:-}
if [ -n "${EXPECT_CHROME:-}" ]; then
  if [ -z "$chrome" ] || [ ! -x "$chrome" ]; then
    bad "Chrome for Testing is missing (CHROMIUM_TEST_PATH='$chrome')"
  else
    cv=$("$chrome" --version 2>&1 | head -1)
    case "$cv" in *"$EXPECT_CHROME"*) note "chrome for testing: $cv" ;; *) bad "Chrome for Testing is '$cv', the image pins $EXPECT_CHROME" ;; esac
  fi
fi

duck=${DUCKDB_LIB_DIR:-}
if [ -n "${EXPECT_DUCKDB:-}" ]; then
  if [ -n "$duck" ] && [ -e "$duck/libduckdb.so" ]; then note "libduckdb $EXPECT_DUCKDB: $duck"; else bad "libduckdb.so is missing in '$duck'"; fi
fi

if [ "${#problems[@]}" -gt 0 ]; then
  echo "paddock-ci-verify: the image is not ready:" >&2
  printf '  - %s\n' "${problems[@]}" >&2
  exit 1
fi

{
  echo "paddock-ci image built $(date -u +%Y-%m-%dT%H:%M:%SZ) from $(. /etc/os-release 2>/dev/null && echo "${PRETTY_NAME:-linux}")"
  printf '%s\n' "${lines[@]}"
} > "$manifest"
cat "$manifest"

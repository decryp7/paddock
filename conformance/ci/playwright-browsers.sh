#!/usr/bin/env bash
# The one place a CI job installs Playwright's browsers (docs/dev-setup.md, "Mac CI runner"; ER-22). Run it from the npm project
# whose Playwright the tests use:
#
#   bash <repo>/conformance/ci/playwright-browsers.sh [--with-deps] <browser>...
#
# On macOS the browsers go to one fixed folder on the boot disk, ~/Library/Caches/paddock-ci/ms-playwright (PADDOCK_PLAYWRIGHT_BROWSERS
# overrides it), exported as PLAYWRIGHT_BROWSERS_PATH to the job's later steps. Never the runner's tool cache: that is on the external
# work volume, and Playwright's headless shell is its own responsible process, so the runner's Full Disk Access does not cover it and
# a LaunchDaemon cannot show the Removable Volumes prompt (2026-11-25, a prompt for
# <work>/tool_cache/paddock-playwright/1.64.1/.../chrome-headless-shell). Playwright keeps one build per browser revision there and
# removes builds no installed Playwright uses. Then the browsers go to the Mac CI signer as a CI hook (tools/mac-ci-signing.sh
# sign-playwright --ci-hook: only in a CI job, only with the identity's default keychain). Elsewhere Playwright's own folder is
# used and nothing is signed. PADDOCK_MAC_SIGNING replaces the signer (tests).
set -uo pipefail

[ $# -gt 0 ] || { echo "playwright-browsers: name the browsers to install, for example: chromium" >&2; exit 2; }
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
signer="${PADDOCK_MAC_SIGNING:-$here/../../tools/mac-ci-signing.sh}"
os=$(uname -s)

if [ "$os" = Darwin ]; then
  dir="${PADDOCK_PLAYWRIGHT_BROWSERS:-$HOME/Library/Caches/paddock-ci/ms-playwright}"
  export PLAYWRIGHT_BROWSERS_PATH="$dir"
  [ -z "${GITHUB_ENV:-}" ] || echo "PLAYWRIGHT_BROWSERS_PATH=$dir" >>"$GITHUB_ENV"
  echo "playwright-browsers: $* into $dir (fixed, boot disk)"
fi

npx playwright install "$@"
rc=$?
if [ $rc -ne 0 ]; then
  echo "::error::playwright-browsers: \`npx playwright install $*\` failed (exit $rc); next: read its output above"
  exit 1
fi

if [ "$os" = Darwin ]; then
  if ! bash "$signer" sign-playwright --ci-hook "$dir"; then
    echo "::error::playwright-browsers: signing the browsers in $dir failed; next: bash tools/mac-ci-signing.sh identity, then the lines above"
    exit 1
  fi
fi

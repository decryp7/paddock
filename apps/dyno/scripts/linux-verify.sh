#!/usr/bin/env bash
# Verifies the Linux adapters of dyno-platform: tests and clippy in a native Linux container, once as root and once as
# an ordinary user (uid 1000, which exercises the "other users' processes are out of sight" paths), then the portable
# parsers natively on this machine. One heavy build at a time; see scripts/linux-container.sh for the container.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"

echo "== load: $(uptime | sed 's/.*load average[s]*: //')"
echo "== container, root: tests + clippy"
"$here/linux-container.sh"
echo "== container, uid 1000: tests (cold build in the container's /tmp)"
DYNO_LINUX_USER=1000:1000 "$here/linux-container.sh" sh -c \
  'mkdir -p /tmp/home /tmp/cargo-home && cp -r /usr/local/cargo/registry /tmp/cargo-home/ && cargo test --offline -p dyno-platform'
echo "== host: portable parsers"
(cd "$repo" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.dyno-target-linux}" CARGO_BUILD_JOBS=2 cargo test -p dyno-platform --lib parse::)

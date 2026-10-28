#!/usr/bin/env bash
# The host runner's CI home: the folder that keeps its build folders, sccache, checkouts, caches and test browsers.
#
#   home=$(bash ci-home.sh)
#
# Default: $HOME/.paddock-ci. A runner relocates it (for example to an external disk) with PADDOCK_CI_HOME in its environment.
# A relocated home must already exist: when it does not (the disk is not mounted), the script fails with a message and
# prints nothing, so no job recreates the cache on the internal disk. The default is created on first use.
set -euo pipefail
if [ -n "${PADDOCK_CI_HOME:-}" ]; then
  home="$PADDOCK_CI_HOME"
  if [ ! -d "$home" ]; then
    echo "::error::PADDOCK_CI_HOME ($home) does not exist: is its disk mounted? Mount it (or create the folder once by hand); the job stops here rather than fill the internal disk." >&2
    exit 1
  fi
else
  home="${HOME:?HOME is not set}/.paddock-ci"
  mkdir -p "$home"
fi
printf '%s\n' "$home"

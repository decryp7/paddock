#!/usr/bin/env bash
# One command per app: scripts/vm-smoke/vm-smoke.sh --app dyno --run <CI run id>   (or --installer <file.dmg>)
# See scripts/vm-smoke/README.md. Local only; never run from CI. The VM itself is started, reverted and stopped by the broker (`./dev vm`).
# PADDOCK_DEV points at a built paddock-dev instead of the ./dev wrapper.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# --run needs GITEA_TOKEN in the environment: `source ~/.zshenv` first (the token is never printed).
exec nice -n 10 node "$here/run.mjs" "$@"

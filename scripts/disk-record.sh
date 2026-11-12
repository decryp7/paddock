#!/bin/bash
# disk-record.sh <dir> <kind> [link-worktree]: writes an owner record for a folder a tool made (`./dev disk record`, ER-2 owner
# records), so a later cleanup can tell whose it is. Never fails its caller: where `./dev disk` does not exist yet (it is on
# branch feat/dev-disk until it lands) or the record fails, it prints nothing and exits 0.
# Environment: PADDOCK_DEV the dev tool (default: ./dev in the current folder).
[ $# -ge 2 ] || { echo "usage: disk-record.sh <dir> <kind> [link-worktree]" >&2; exit 0; }
DEV=${PADDOCK_DEV:-./dev}
"$DEV" disk --help >/dev/null 2>&1 || exit 0
"$DEV" disk record "$1" --kind "$2" --link "${3:-$PWD}" >/dev/null 2>&1 || true
exit 0

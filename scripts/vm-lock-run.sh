#!/usr/bin/env bash
# Runs a command while holding the test machines lock for its whole run (interim lock, scripts/vm-lock.sh):
#   scripts/vm-lock-run.sh <label> -- <command...>     acquire, run the command as a child, pass its exit code through
#   scripts/vm-lock-run.sh --check                      exit 0 when the caller already holds a valid lock (inherited), else 1
# Busy: exit 75 with ::error:: before the command starts. The lock is released when this script ends, whatever the exit.
set -u
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$here/vm-lock.sh"
if [ "${1:-}" = "--check" ]; then vm_lock_inherited; exit $?; fi
label="${1:?usage: vm-lock-run.sh <label> -- <command...>}"; shift
[ "${1:-}" = "--" ] && shift
[ $# -gt 0 ] || { echo "::error::vm-lock-run: no command given. Usage: vm-lock-run.sh <label> -- <command...>" >&2; exit 2; }
vm_lock_acquire "$label"
"$@"

#!/usr/bin/env bash
# Interim exclusive lock for the Windows test VM (/Volumes/d7sst7/vms/paddock-win11-test). SOURCE it, then call
# `vm_lock_acquire <label>` before anything that starts, stops, reverts, snapshots or loads the guest (vmrun, SSH jobs).
#
#   . "$(dirname "$0")/vm-lock.sh"; vm_lock_acquire vm-win-start
#
# It is the pattern of tools/macos-vm-smoke.sh: an OS file lock from `./dev ci slot acquire --max 1` (paddock-slots) under ONE
# fixed root, so the kernel frees it when the holder dies; there is no stale lock file to break. The holder is a detached
# process that lives as long as the owner folder made here, which the EXIT trap (and a later acquirer, for a killed script)
# removes. Replaced by the VM broker when that lands.
#
# A second acquirer fails at once with `::error::` naming the holder (label, pid and start time, rule 53) and the next step;
# PADDOCK_VM_LOCK_WAIT=<seconds> waits that long instead. A holder whose pid is gone, or is now another process (start time
# differs), is dead: its owner folder is removed and the lock is taken over. A script run by a holder (chain-release.sh calling
# vm-win.sh) inherits PADDOCK_VM_LOCK_HELD=<holder pid> and does not wait for itself.
#
# Environment: PADDOCK_VM_LOCK_ROOT (default /Volumes/d7sst7/ci/locks/test-machines), PADDOCK_VM_LOCK_WAIT (0),
#   PADDOCK_VM_LOCK_POLL (seconds between tries, 5), PADDOCK_DEV (the paddock-dev program; default the repository's ./dev).

_vm_lock_here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_vm_lock_slot=""; _vm_lock_token=""; _vm_lock_owner=""; _vm_lock_root=""

_vm_lock_dev() { if [ -n "${PADDOCK_DEV:-}" ]; then "$PADDOCK_DEV" "$@"; else (cd "$_vm_lock_here/.." && bash ./dev "$@"); fi; }
_vm_lock_start() { ps -o lstart= -p "$1" 2>/dev/null | sed 's/^ *//; s/ *$//; s/  */ /g'; }

# The holder's owner folder and pid from `slot status`, if the slot is held.
_vm_lock_holder() {
  local json; json="$(_vm_lock_dev ci slot status --max 1 --root "$_vm_lock_root" --json 2>/dev/null)" || return 1
  case "$json" in *'"busy":true'*) ;; *) return 1 ;; esac
  printf '%s\n' "$json" | sed -n 's/.*"owner":"\([^"]*\)".*/\1/p'
}

vm_lock_release() {
  [ -z "$_vm_lock_slot" ] && return 0
  _vm_lock_dev ci slot release --slot "$_vm_lock_slot" --token "$_vm_lock_token" --root "$_vm_lock_root" >/dev/null 2>&1 || true
  [ -z "$_vm_lock_owner" ] || rm -rf "$_vm_lock_owner"
  _vm_lock_slot=""; _vm_lock_owner=""
}

vm_lock_acquire() {
  local label="${1:?vm_lock_acquire needs a label}" wait poll waited=0 said src owner who pid start now
  # Inherited from a holder that is still running: nothing to take.
  if [ -n "${PADDOCK_VM_LOCK_HELD:-}" ] && kill -0 "$PADDOCK_VM_LOCK_HELD" 2>/dev/null; then return 0; fi
  _vm_lock_root="${PADDOCK_VM_LOCK_ROOT:-/Volumes/d7sst7/ci/locks/test-machines}"
  wait="${PADDOCK_VM_LOCK_WAIT:-0}"; poll="${PADDOCK_VM_LOCK_POLL:-5}"
  mkdir -p "$_vm_lock_root" || { echo "::error::vm-lock: cannot create $_vm_lock_root; check that /Volumes/d7sst7 is mounted" >&2; exit 3; }
  owner="$(mktemp -d "${TMPDIR:-/tmp}/paddock-vm-lock-$label-XXXXXX")"
  printf '%s\t%s\t%s\n' "$$" "$(_vm_lock_start $$)" "$label" > "$owner/who"
  while :; do
    src=0; said="$(_vm_lock_dev ci slot acquire --max 1 --owner "$owner" --root "$_vm_lock_root")" || src=$?
    if [ "$src" = 0 ]; then
      read -r _vm_lock_slot _vm_lock_token <<< "$said"; _vm_lock_owner="$owner"
      _vm_lock_install_traps
      export PADDOCK_VM_LOCK_HELD="$$"
      echo "vm-lock: $label holds the test machines lock ($_vm_lock_root, pid $$)" >&2
      return 0
    fi
    [ "$src" = 4 ] || { rm -rf "$owner"; echo "::error::vm-lock: could not take the lock (paddock-dev exit $src); run: ./dev ci slot status --max 1 --root $_vm_lock_root" >&2; exit 3; }
    # Busy: who holds it, and is that process still the one that took it (pid plus start time)?
    other="$(_vm_lock_holder || true)"
    who="$(cat "$other/who" 2>/dev/null || true)"
    if [ -n "$other" ] && [ -n "$who" ]; then
      pid="${who%%$'\t'*}"; rest="${who#*$'\t'}"; start="${rest%%$'\t'*}"; olabel="${rest#*$'\t'}"
      now="$(_vm_lock_start "$pid")"
      if [ -z "$now" ] || [ "$now" != "$start" ]; then
        echo "vm-lock: the holder ($olabel, pid $pid, started $start) is no longer running (dead holder); taking over" >&2
        rm -rf "$other"; sleep 1; continue
      fi
    else
      pid="?"; start="?"; olabel="unknown (no who file)"
    fi
    if [ "$waited" -ge "$wait" ]; then
      rm -rf "$owner"
      echo "::error::vm-lock: the test machine lock is held by $olabel (pid $pid, started $start)$([ "$wait" -gt 0 ] && echo "; still held after ${waited}s")." \
        "Next step: wait for that run to finish, or set PADDOCK_VM_LOCK_WAIT=<seconds> to wait; never stop another agent's VM." >&2
      exit 3
    fi
    echo "vm-lock: held by $olabel (pid $pid); waiting" >&2
    sleep "$poll"; waited=$((waited + poll))
  done
}

# EXIT releases the lock (success, error, signal); an EXIT trap the script already had still runs afterwards.
_vm_lock_install_traps() {
  local prev; prev="$(trap -p EXIT | sed -n "s/^trap -- '\(.*\)' EXIT$/\1/p")"
  # shellcheck disable=SC2064
  trap "vm_lock_release; $prev" EXIT
  [ -n "$(trap -p INT)" ] || trap 'exit 130' INT
  [ -n "$(trap -p TERM)" ] || trap 'exit 143' TERM
  [ -n "$(trap -p HUP)" ] || trap 'exit 129' HUP
}

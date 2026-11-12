#!/usr/bin/env bash
# Interim exclusive lock for the Windows test VM (/Volumes/d8sst7/vm-data/paddock-win11-test). SOURCE it, then call
# `vm_lock_acquire <label>` before anything that starts, stops, reverts, snapshots or loads the guest (vmrun, SSH jobs).
#
#   . "$(dirname "$0")/vm-lock.sh"; vm_lock_acquire vm-win-start
#
# A long Node runner takes the lock for its WHOLE run through `scripts/vm-lock-run.sh <label> -- <command...>`.
#
# It is the pattern of tools/macos-vm-smoke.sh: an OS file lock from `./dev ci slot acquire --max 1` (paddock-slots) under ONE
# fixed root, so the kernel frees it when the holder dies; there is no stale lock file to break. The holder is a detached
# process that lives as long as the owner folder made here, which the EXIT trap (and a later acquirer, for a killed script)
# removes. The holder has NO time cap (--max-hold-secs 0): a chain release runs for hours. Replaced by the VM broker.
#
# A second acquirer fails at once with `::error::` naming the holder (label, pid and start time, rule 53), exit code 75, and the
# next step; PADDOCK_VM_LOCK_WAIT=<seconds> waits that long instead. A holder whose pid is gone, or is now another process
# (start time differs), is dead: its owner folder is removed and the lock is taken over.
#
# Inheritance: a holder exports PADDOCK_VM_LOCK_HELD="<pid>\t<lstart>\t<owner folder>". A child accepts it only when that pid
# still has that start time, `slot status` shows the slot busy with that very owner folder AND the owner folder's who file names that pid; anything else (a stale pid,
# $PPID of an unrelated process, an owner that no longer holds) is ignored and the lock is taken (or refused) as usual.
#
# NEVER set an EXIT trap after vm_lock_acquire (it would replace the release); set it before: the lock's trap runs yours FIRST, with the
# lock still held, then releases the lock (so a cleanup of the VM is never unlocked).
#
# Exit codes: 75 busy (EX_TEMPFAIL), 3 the lock could not be used (root missing, paddock-dev failed).
# Environment: PADDOCK_VM_LOCK_ROOT (default /Volumes/d7sst7/ci/locks/test-machines), PADDOCK_VM_LOCK_WAIT (0),
#   PADDOCK_VM_LOCK_POLL (seconds between tries, 5), PADDOCK_DEV (the paddock-dev program; default the repository's ./dev).

_vm_lock_here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# State of the lock this shell holds (needed by the EXIT trap).
_vm_lock_slot=""; _vm_lock_token=""; _vm_lock_owner=""; _vm_lock_root=""; _vm_lock_label=""; _vm_lock_since=0; _vm_lock_prev_trap=""
VM_LOCK_BUSY=75

_vm_lock_dev() { if [ -n "${PADDOCK_DEV:-}" ]; then "$PADDOCK_DEV" "$@"; else (cd "$_vm_lock_here/.." && bash ./dev "$@"); fi; }
# The start of a process: `ps lstart` (shown to people), plus on Linux the kernel start tick from /proc/<pid>/stat as " [t<ticks>]". Linux
# derives lstart from the wall clock, which can move by a second between two calls for one process; the tick never changes, so a live holder
# is never taken for a dead one (rule 53 identity is pid plus start; compare with _vm_lock_same_start, never with `=`).
_vm_lock_start() {
  local s tick
  s="$(ps -o lstart= -p "$1" 2>/dev/null | sed 's/^ *//; s/ *$//; s/  */ /g')"
  [ -n "$s" ] || return 0
  if [ -r "/proc/$1/stat" ]; then
    tick="$(sed 's/^.*) //' "/proc/$1/stat" 2>/dev/null | awk '{print $20}')"
    [ -z "$tick" ] || s="$s [t$tick]"
  fi
  printf '%s' "$s"
}
# 0 when two starts name the same process: equal kernel ticks when both carry one; when only one does, the lstart parts alone are compared (a
# holder recorded by an older script must not be taken for dead); else equal strings. An empty start never matches.
_vm_lock_same_start() {
  local a="$1" b="$2" ta tb
  [ -n "$a" ] && [ -n "$b" ] || return 1
  ta="${a##*\[t}"; tb="${b##*\[t}"
  if [ "$ta" != "$a" ] && [ "$tb" != "$b" ]; then [ "$ta" = "$tb" ]; else [ "${a%% \[t*}" = "${b%% \[t*}" ]; fi
}
_vm_lock_default_root() { printf '%s' "${PADDOCK_VM_LOCK_ROOT:-/Volumes/d7sst7/ci/locks/test-machines}"; }

# The owner folder of the current holder, if the slot is held; retried a few times (status can race a holder starting or ending).
_vm_lock_holder() {
  local json
  for _ in 1 2 3; do
    json="$(_vm_lock_dev ci slot status --max 1 --root "$1" --json 2>/dev/null)" || json=""
    case "$json" in
      *'"busy":false'*) return 1 ;;
      *'"busy":true'*) printf '%s\n' "$json" | sed -n 's/.*"owner":"\([^"]*\)".*/\1/p'; return 0 ;;
    esac
    sleep 0.3
  done
  return 1
}

# 0 when PADDOCK_VM_LOCK_HELD names a live holder: same pid and start time, and `slot status` shows that owner folder holding.
vm_lock_inherited() {
  local pid start owner now cur root
  [ -n "${PADDOCK_VM_LOCK_HELD:-}" ] || return 1
  IFS=$'\t' read -r pid start owner <<< "$PADDOCK_VM_LOCK_HELD"
  [ -n "$pid" ] && [ -n "$start" ] && [ -n "$owner" ] || return 1
  now="$(_vm_lock_start "$pid")"; _vm_lock_same_start "$now" "$start" || return 1
  root="$(_vm_lock_default_root)"
  cur="$(_vm_lock_holder "$root")" || return 1
  [ "$cur" = "$owner" ] || return 1
  # The pid must be the one that took the lock: the owner folder's who file names it (an unrelated live pid such as $PPID is not).
  local wpid wstart
  IFS=$'\t' read -r wpid wstart _ < "$owner/who" 2>/dev/null || return 1
  [ "$wpid" = "$pid" ] && _vm_lock_same_start "$wstart" "$start"
}

vm_lock_release() {
  [ -z "$_vm_lock_slot" ] && return 0
  _vm_lock_dev ci slot release --slot "$_vm_lock_slot" --token "$_vm_lock_token" --root "$_vm_lock_root" >/dev/null 2>&1 || true
  [ -z "$_vm_lock_owner" ] || rm -rf "$_vm_lock_owner"
  echo "vm-lock: $_vm_lock_label released the test machines lock after $((SECONDS - _vm_lock_since))s" >&2
  _vm_lock_slot=""; _vm_lock_owner=""
}

# The EXIT trap: the trap the script had before runs FIRST, while the lock is still held (a cleanup that stops or reverts the VM must
# not run unlocked, when another acquirer may already own it), then the release (the exit status is kept).
_vm_lock_on_exit() {
  local rc=$?
  [ -z "$_vm_lock_prev_trap" ] || eval "$_vm_lock_prev_trap"
  vm_lock_release
  return "$rc"
}

_vm_lock_install_traps() {
  local current
  current="$(trap -p EXIT)"
  if [ -n "$current" ]; then eval "set -- $current"; _vm_lock_prev_trap="${3:-}"; fi
  trap _vm_lock_on_exit EXIT
  [ -n "$(trap -p INT)" ] || trap 'exit 130' INT
  [ -n "$(trap -p TERM)" ] || trap 'exit 143' TERM
  [ -n "$(trap -p HUP)" ] || trap 'exit 129' HUP
}

vm_lock_acquire() {
  local label="${1:?vm_lock_acquire needs a label}" unknown=0 wait poll waited=0 said src owner other who pid start rest olabel now errf
  vm_lock_inherited && return 0
  _vm_lock_root="$(_vm_lock_default_root)"
  wait="${PADDOCK_VM_LOCK_WAIT:-0}"; poll="${PADDOCK_VM_LOCK_POLL:-5}"
  mkdir -p "$_vm_lock_root" || { echo "::error::vm-lock: cannot create $_vm_lock_root; check that /Volumes/d7sst7 is mounted" >&2; exit 3; }
  owner="$(mktemp -d "${TMPDIR:-/tmp}/paddock-vm-lock-$label-XXXXXX")"
  owner="$(cd "$owner" && pwd -P)" # `slot status` reports the canonical path
  printf '%s\t%s\t%s\n' "$$" "$(_vm_lock_start $$)" "$label" > "$owner/who"
  errf="$owner.err"
  while :; do
    src=0; said="$(_vm_lock_dev ci slot acquire --max 1 --owner "$owner" --root "$_vm_lock_root" --max-hold-secs 0 2>"$errf")" || src=$?
    if [ "$src" = 0 ]; then
      rm -f "$errf"
      read -r _vm_lock_slot _vm_lock_token <<< "$said"; _vm_lock_owner="$owner"; _vm_lock_label="$label"; _vm_lock_since=$SECONDS
      _vm_lock_install_traps
      PADDOCK_VM_LOCK_HELD="$(printf '%s\t%s\t%s' "$$" "$(_vm_lock_start $$)" "$owner")"; export PADDOCK_VM_LOCK_HELD
      echo "vm-lock: $label holds the test machines lock ($_vm_lock_root, pid $$)" >&2
      return 0
    fi
    if [ "$src" != 4 ]; then
      cat "$errf" >&2; rm -rf "$owner" "$errf"
      echo "::error::vm-lock: could not take the lock (paddock-dev exit $src). Next step: ./dev ci slot status --max 1 --root $_vm_lock_root" >&2; exit 3
    fi
    # Busy (exit 4; paddock-dev's own "raise --max" text is not for this lock): who holds it, still the process that took it?
    other="$(_vm_lock_holder "$_vm_lock_root" || true)"
    who="$(cat "$other/who" 2>/dev/null || true)"
    if [ -n "$other" ] && [ -n "$who" ]; then
      pid="${who%%$'\t'*}"; rest="${who#*$'\t'}"; start="${rest%%$'\t'*}"; olabel="${rest#*$'\t'}"
      now="$(_vm_lock_start "$pid")"
      if ! _vm_lock_same_start "$now" "$start"; then
        echo "vm-lock: the holder ($olabel, pid $pid, started $start) is no longer running (dead holder); taking over" >&2
        rm -rf "$other" "$other.err"; sleep 1; continue
      fi
    else
      # Free again or not yet named (the holder ended or is starting between our acquire and the status): try the acquire again a few times.
      if [ "$unknown" -lt 3 ]; then unknown=$((unknown + 1)); sleep 0.5; continue; fi
      pid="?"; start="?"; olabel="unknown (no who file)"
    fi
    if [ "$waited" -ge "$wait" ]; then
      rm -rf "$owner" "$errf"
      echo "::error::vm-lock: the test machine lock is held by $olabel (pid $pid, started $start)$([ "$wait" -gt 0 ] && echo "; still held after ${waited}s")." \
        "Next step: wait for that run to finish, or set PADDOCK_VM_LOCK_WAIT=<seconds> to wait; never stop another agent's VM." >&2
      exit "$VM_LOCK_BUSY"
    fi
    echo "vm-lock: held by $olabel (pid $pid); waiting" >&2
    sleep "$poll"; waited=$((waited + poll))
  done
}

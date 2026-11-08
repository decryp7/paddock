#!/usr/bin/env bash
# Host driver for WP-0 (Dyno ADR 0033; results in the ADR). Refuses any VM but the pinned Paddock Windows test VM.
#   wp0-run.sh --step <name> [--as paddock-test|paddock-admin] [--arg <path in guest>] [--keep]
# The guest address comes from vmrun (or, where VMware Tools is absent on the Arm guest, from the NAT lease of the VM's fixed MAC), never
# from the command line, so only the pinned VM can be reached. Results go outside the repository: WP0_OUT, default $TMPDIR/dyno-wp0-out.
set -euo pipefail
PINNED=/Volumes/d7sst7/vms/paddock-win11-test
VMX="$PINNED/vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx"
VMRUN="/Applications/VMware Fusion.app/Contents/Library/vmrun"
SNAP=gui-session-tools
MAC=00:50:56:2a:d0:c7
LEASES=${PADDOCK_VM_LEASES:-/var/db/vmware/vmnet-dhcpd-vmnet8.leases}
here=$(cd "$(dirname "$0")" && pwd)
trap 'echo "::error::wp0-run: failed at line $LINENO ($BASH_COMMAND). Next: read the output above; the VM is reverted to $SNAP on exit unless --keep; check the snapshot name with: vmrun listSnapshots <vmx>" >&2' ERR

OUT=${WP0_OUT:-${TMPDIR:-/tmp}/dyno-wp0-out}
mkdir -p "$OUT"
out_real=$(cd "$OUT" && pwd -P)
top=$(git -C "$here" rev-parse --show-toplevel 2>/dev/null || true)
if [ -n "$top" ]; then
  top_real=$(cd "$top" && pwd -P)
  case "$out_real/" in "$top_real"/*) echo "::error::WP0_OUT ($out_real) is inside the repository ($top_real): the results name the VM's accounts and must not be committed. Next: set WP0_OUT to a folder outside it." >&2; exit 2;; esac
fi

step=""; as="paddock-test"; arg=""; keep=0
while [ $# -gt 0 ]; do case "$1" in --step) step=$2; shift 2;; --as) as=$2; shift 2;; --arg) arg=$2; shift 2;; --keep) keep=1; shift;; *) echo "::error::unknown option $1" >&2; exit 2;; esac; done
[ -n "$step" ] || { echo "::error::--step is required" >&2; exit 2; }
[ -f "$VMX" ] || { echo "::error::the pinned VM is missing: $VMX" >&2; exit 2; }
# Only the pinned VM may be running under this script's control: any other running VM stops it (never touch the user's own).
running=$("$VMRUN" list | tail -n +2)
if [ -n "$running" ] && echo "$running" | grep -vqxF "$VMX"; then echo "::error::another VM is running ($running); stop it yourself, this script never touches it" >&2; exit 2; fi
key="$PINNED/keys/$as"; [ -f "$key" ] || { echo "::error::no key $key" >&2; exit 2; }
admin_key="$PINNED/keys/paddock-admin"

revert() { "$VMRUN" revertToSnapshot "$VMX" "$SNAP" && "$VMRUN" start "$VMX" nogui; }
revert
trap '[ "$keep" = 1 ] || "$VMRUN" revertToSnapshot "$VMX" "$SNAP" || echo "::error::could not revert $SNAP; revert it by hand" >&2' EXIT

guest_ip() {
  local ip
  ip=$("$VMRUN" getGuestIPAddress "$VMX" 2>/dev/null || true)
  case "$ip" in [0-9]*.[0-9]*.[0-9]*.[0-9]*) echo "$ip"; return;; esac
  [ -r "$LEASES" ] || return 1
  awk -v mac="$MAC" '/^lease /{a=$2} /hardware ethernet/{m=tolower($3); sub(/;$/,"",m); if (m==mac) f=a} END{if (f=="") exit 1; print f}' "$LEASES"
}
host=""
for i in $(seq 1 60); do host=$(guest_ip 2>/dev/null || true); [ -n "$host" ] && break; sleep 5; done
[ -n "$host" ] || { echo "::error::no address for the pinned VM after 5 minutes" >&2; exit 1; }
opts=(-o IdentitiesOnly=yes -o BatchMode=yes -o UserKnownHostsFile="$PINNED/keys/known_hosts" -o StrictHostKeyChecking=yes -o ConnectTimeout=20)
S() { local k=$1 u=$2; shift 2; ssh -i "$k" "${opts[@]}" "$u@$host" "$@"; }
for i in $(seq 1 60); do S "$key" "$as" 'echo up' >/dev/null 2>&1 && break; [ "$i" = 60 ] && { echo "::error::guest did not answer ssh in 5 minutes (host key unknown? add it once by hand with the pinned known_hosts)" >&2; exit 1; }; sleep 5; done

mkdir -p "$OUT/$step"
scp -i "$key" "${opts[@]}" "$here/wp0-collect.ps1" "$as@$host:wp0-collect.ps1"
P="powershell -NoProfile -ExecutionPolicy Bypass -File wp0-collect.ps1"
a=""; [ -n "$arg" ] && a="-Path '$arg'"
# The environment is recorded with every step (ER-33): who ran, the build, the channel.
S "$key" "$as" "$P -Step env" | tee "$OUT/$step/env.txt"
S "$key" "$as" "$P -Step $step $a" | tee "$OUT/$step/$step.txt"
# A step must print a CONTROL line and it must be ok: a missing line is a probe that never reached its control.
if ! grep -q '^CONTROL ' "$OUT/$step/$step.txt"; then echo "::error::check broken: step $step printed no CONTROL line (it stopped early). Next: read $OUT/$step/$step.txt and fix the step in wp0-collect.ps1." >&2; exit 1; fi
if grep -q 'CONTROL BROKEN' "$OUT/$step/$step.txt"; then echo "::error::the probe's control is broken for $step: this is 'check broken', not a result. Next: fix the step in wp0-collect.ps1." >&2; exit 1; fi

if [ "$step" = tasks-register ]; then
  # The step cannot check its own cleanup (a standard user cannot read the task list through Get-ScheduledTask). As administrator:
  # first a known Windows task must be listed (the query can see), then no DynoTest- task may remain (ER-33).
  known='\Microsoft\Windows\Defrag\ScheduledDefrag'
  S "$admin_key" paddock-admin "schtasks /query /tn $known" >"$OUT/$step/cleanup-control.txt" 2>&1 || { echo "::error::check broken: schtasks /query as paddock-admin cannot see the known task $known, so a clean result would mean nothing. Next: pick another known task or fix the admin key." >&2; exit 1; }
  S "$admin_key" paddock-admin 'schtasks /query /fo csv' >"$OUT/$step/cleanup-all.txt" 2>&1
  if grep -q 'DynoTest-' "$OUT/$step/cleanup-all.txt"; then echo "::error::a DynoTest- task is left in the guest: $(grep -o 'DynoTest-[^\",]*' "$OUT/$step/cleanup-all.txt" | head -3 | tr '\n' ' '). Next: delete it with schtasks /delete as paddock-admin, or let the snapshot revert remove it." >&2; exit 1; fi
  echo "CLEANUP ok: the known task was listed and no DynoTest- task remains"
fi
echo "WP-0 $step done: $OUT/$step"

#!/usr/bin/env bash
# Host driver for WP-0 (Dyno ADR 0033; results in the ADR). Refuses any VM but the pinned Paddock Windows test VM.
#   wp0-run.sh --host <guest ip> --step <name> [--as paddock-test|paddock-admin] [--arg <path in guest>] [--keep]
set -euo pipefail
PINNED=/Volumes/d7sst7/vms/paddock-win11-test
VMX="$PINNED/vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx"
VMRUN="/Applications/VMware Fusion.app/Contents/Library/vmrun"
SNAP=gui-session-tools
OUT=${WP0_OUT:-$PWD/wp0-out}   # outside the repository: results can name the VM's accounts
here=$(cd "$(dirname "$0")" && pwd)
host=""; step=""; as="paddock-test"; arg=""; keep=0
while [ $# -gt 0 ]; do case "$1" in --host) host=$2; shift 2;; --step) step=$2; shift 2;; --as) as=$2; shift 2;; --arg) arg=$2; shift 2;; --keep) keep=1; shift;; *) echo "::error::unknown option $1"; exit 2;; esac; done
[ -n "$host" ] && [ -n "$step" ] || { echo "::error::--host and --step are required"; exit 2; }
[ -f "$VMX" ] || { echo "::error::the pinned VM is missing: $VMX"; exit 2; }
# Only the pinned VM may be running under this script's control: any other running VM stops it (never touch the user's own).
running=$("$VMRUN" list | tail -n +2)
echo "$running" | grep -vqF "$VMX" && [ -n "$running" ] && { echo "::error::another VM is running ($running); stop it yourself, this script never touches it"; exit 2; }
key="$PINNED/keys/$as"; [ -f "$key" ] || { echo "::error::no key $key"; exit 2; }
revert() { "$VMRUN" revertToSnapshot "$VMX" "$SNAP" && "$VMRUN" start "$VMX" nogui; }
revert
trap '[ "$keep" = 1 ] || "$VMRUN" revertToSnapshot "$VMX" "$SNAP" || echo "::error::could not revert $SNAP; revert it by hand"' EXIT
ssh_opts=(-i "$key" -o IdentitiesOnly=yes -o UserKnownHostsFile="$PINNED/keys/known_hosts" -o ConnectTimeout=20)
for i in $(seq 1 60); do ssh "${ssh_opts[@]}" "$as@$host" 'echo up' >/dev/null 2>&1 && break; [ "$i" = 60 ] && { echo "::error::guest did not answer ssh in 5 minutes"; exit 1; }; sleep 5; done
mkdir -p "$OUT/$step"
scp "${ssh_opts[@]}" "$here/wp0-collect.ps1" "$as@$host:wp0-collect.ps1"
a=(); [ -n "$arg" ] && a=(-Path "'$arg'")
# The environment is recorded with every step (ER-33): who ran, the build, the channel.
ssh "${ssh_opts[@]}" "$as@$host" "powershell -NoProfile -ExecutionPolicy Bypass -File wp0-collect.ps1 -Step env" | tee "$OUT/$step/env.txt"
ssh "${ssh_opts[@]}" "$as@$host" "powershell -NoProfile -ExecutionPolicy Bypass -File wp0-collect.ps1 -Step $step ${a[*]:-}" | tee "$OUT/$step/$step.txt"
grep -q 'CONTROL BROKEN' "$OUT/$step/$step.txt" && { echo "::error::the probe's control is broken for $step: this is 'check broken', not a result. Fix the step in wp0-collect.ps1."; exit 1; }
echo "WP-0 $step done: $OUT/$step"

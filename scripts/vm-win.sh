#!/usr/bin/env bash
# Drives the local Windows 11 Arm test VM (VMware Fusion; commands inside it over SSH). Starting, stopping, reverting and snapshotting go through
# the VM broker (`./dev vm`, ADR 0054 section 1.5): one lease per VM, a preflight, headless only, the only writer of the .vmx. This script calls
# no VM tool itself (check C97).
# Local only, never CI. Setup and the update test: docs/testing/windows-test-vm.md.
#
#   scripts/vm-win.sh start [--no-wait]       start headless and wait until SSH answers
#   scripts/vm-win.sh stop                    shut Windows down (over SSH), power off if it does not stop in time
#   scripts/vm-win.sh ssh [--admin] [cmd...]  SSH as paddock-test (or paddock-admin); a shell without a command
#   scripts/vm-win.sh snapshot <name>         take a snapshot
#   scripts/vm-win.sh revert [name]           revert to a snapshot (default clean-baseline); the VM is then off
#   scripts/vm-win.sh status | ip             running or stopped / the guest's address
#   scripts/vm-win.sh install                 one-time: install Windows headless, wait for setup to power off
#   scripts/vm-win.sh baseline                one-time: check setup finished, detach the discs, take clean-baseline
#
# Only ever acts on the VM under PADDOCK_WIN_VM_DIR (default /Volumes/d7sst7/vms/paddock-win11-test); any other VM on
# this Mac is never started, stopped or changed. A guest that runs without a lease (another job) is refused, except by `stop`, which
# takes it on your word. Only if the VM is encrypted (option A, virtual TPM) does the broker read its password from the login keychain
# (service paddock-win11-test-vm, account vmrun).
set -euo pipefail

dir=${PADDOCK_WIN_VM_DIR:-/Volumes/d7sst7/vms/paddock-win11-test}
name=paddock-win11-test
vmx="$dir/vm/$name.vmwarevm/$name.vmx"
export PADDOCK_WIN_VMX="$vmx" # the broker edits and starts this .vmx and no other
mac=00:50:56:2a:d0:c7
leases=${PADDOCK_VM_LEASES:-/var/db/vmware/vmnet-dhcpd-vmnet8.leases}
max_load=${PADDOCK_VM_MAX_LOAD:-25}
ssh_deadline=${PADDOCK_VM_SSH_DEADLINE:-300}
install_deadline=${PADDOCK_VM_INSTALL_DEADLINE:-7200}
baseline=clean-baseline
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
dev=${PADDOCK_DEV:-$repo/dev}
lease_id=""

die() { echo "vm-win: $*" >&2; exit 1; }
usage() { sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; }

guard() {
  case "$dir/" in "$HOME"/*) die "PADDOCK_WIN_VM_DIR must not be under \$HOME (ER-14): $dir" ;; esac
  [ -f "$vmx" ] || die "no test VM at $vmx; run scripts/vm-win/prepare.sh first"
}

# The broker, in text mode: `lease` prints the lease id; every other command prints a short result. Its own errors name the next step.
broker() { "$dev" vm "$@"; }

# Takes the lease on the VM for this script (released when the script ends). Extra arguments go to `dev vm lease` (for example --adopt).
take_lease() {
  local purpose=$1; shift
  lease_id=$(broker lease "$name" --for "$purpose" --holder-pid $$ "$@") || exit 1
  trap 'broker release "$lease_id" >/dev/null 2>&1 || true' EXIT
}

running() { [ "$(broker status "$name" 2>/dev/null)" = running ]; }

# The guest's IPv4 address: the newest NAT DHCP lease for the VM's fixed MAC address (vmrun getGuestIPAddress needs
# VMware Tools, which Arm guests may lack).
ip() {
  [ -r "$leases" ] || die "cannot read $leases"
  awk -v mac="$mac" '
    /^lease / { addr = $2 }
    /hardware ethernet/ { m = tolower($3); sub(/;$/, "", m); if (m == mac) found = addr }
    END { if (found == "") exit 1; print found }
  ' "$leases" || die "no DHCP lease for $mac yet (is the VM running?)"
}

ssh_to() { # user, then ssh arguments
  local user=$1; shift
  local host; host=$(ip)
  ssh -i "$dir/keys/$user" -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=5 \
    -o UserKnownHostsFile="$dir/keys/known_hosts" -o StrictHostKeyChecking=accept-new "$user@$host" "$@"
}

wait_ssh() {
  local end=$((SECONDS + ssh_deadline))
  until (ssh_to paddock-test 'exit 0') >/dev/null 2>&1; do # subshell: no lease yet is not fatal
    [ $SECONDS -lt $end ] || die "SSH did not answer within ${ssh_deadline}s"
    running || die "the VM stopped while waiting for SSH"
    sleep 3
  done
}

wait_off() { # deadline in seconds, label
  local end=$((SECONDS + $1)) next=$((SECONDS + 60))
  while running; do
    [ $SECONDS -lt $end ] || return 1
    if [ $SECONDS -ge $next ]; then echo "  $2: still working ($((SECONDS / 60)) min)"; next=$((SECONDS + 60)); fi
    sleep 5
  done
}

has_snapshot() { broker snapshots "$lease_id" | grep -Fxq "$1"; }

# Powers the guest off the way a person would: Windows is asked to shut down, the broker waits and then powers off hard.
shut_down() { # grace seconds
  (ssh_to paddock-admin 'shutdown /s /t 0') >/dev/null 2>&1 || true
  broker stop "$lease_id" --grace "$1"
}

cmd=${1:-}
[ $# -gt 0 ] && shift
case $cmd in
  start)
    guard
    take_lease "vm-win.sh start"
    broker start "$lease_id" --max-load "$max_load"
    [ "${1:-}" = "--no-wait" ] || { wait_ssh; echo "ready: $(ip)"; }
    echo "the VM keeps running without a lease; stop it with: scripts/vm-win.sh stop"
    ;;
  stop)
    guard
    running || { echo "already stopped"; exit 0; }
    take_lease "vm-win.sh stop" --adopt
    shut_down 120
    echo "stopped"
    ;;
  ssh)
    guard
    user=paddock-test
    [ "${1:-}" = "--admin" ] && { user=paddock-admin; shift; }
    ssh_to "$user" "$@"
    ;;
  snapshot)
    guard
    [ -n "${1:-}" ] || die "usage: vm-win.sh snapshot <name>"
    take_lease "vm-win.sh snapshot"
    broker snapshot "$lease_id" --name "$1" >/dev/null
    echo "snapshot $1 taken"
    ;;
  revert)
    guard
    snap=${1:-$baseline}
    take_lease "vm-win.sh revert"
    has_snapshot "$snap" || die "no snapshot named $snap"
    broker revert "$lease_id" --to "$snap" >/dev/null
    echo "reverted to $snap"
    ;;
  status)
    guard
    if running; then echo "running"; else echo "stopped"; fi
    ;;
  ip)
    guard
    ip
    ;;
  install)
    guard
    win_iso=$(sed -n 's/^sata0:0.fileName = "\(.*\)"$/\1/p' "$vmx")
    [ -z "$win_iso" ] || [ -f "$win_iso" ] || die "Windows ISO missing at $win_iso; run scripts/vm-win/prepare.sh (docs/testing/windows-test-vm.md)"
    [ -f "$dir/iso/paddock-setup.iso" ] || die "setup disc missing; run scripts/vm-win/prepare.sh"
    take_lease "vm-win.sh install" # refused while the VM runs: stop it first
    has_snapshot "$baseline" && die "the VM already has $baseline; revert to it instead of reinstalling"
    for dev_key in sata0:0 sata0:1; do broker set "$lease_id" --key "$dev_key.present" --value TRUE >/dev/null; done
    echo "Installing Windows headless from the no-prompt ISO (no window, no key to press); the VM powers off when done."
    started=$SECONDS
    broker start "$lease_id" --max-load "$max_load"
    wait_off "$install_deadline" "installing Windows" || die "setup did not finish within ${install_deadline}s; see $(dirname "$vmx")/vmware.log"
    took=$(((SECONDS - started) / 60))
    # Setup takes far longer than this; an earlier stop is a crash or a boot failure, never a finished install.
    [ $took -ge "${PADDOCK_VM_MIN_INSTALL_MIN:-10}" ] || die "the VM stopped after $took min, too early for a finished setup; see $(dirname "$vmx")/vmware.log"
    echo "the VM powered off after $took min; next: scripts/vm-win.sh baseline (it checks that setup finished)"
    ;;
  baseline)
    guard
    take_lease "vm-win.sh baseline" # refused while the VM runs: baseline is taken from a powered-off VM
    has_snapshot "$baseline" && die "$baseline already exists"
    # Check first, with the discs still attached: Windows Setup puts Windows Boot Manager first in the boot order, so
    # the disk boots; if setup had not finished, nothing is lost and install can be run again.
    broker start "$lease_id" --max-load "$max_load"
    wait_ssh
    ssh_to paddock-admin 'if (-not (Test-Path C:\paddock-setup\done.marker)) { exit 3 }' ||
      die "setup did not finish (no C:\\paddock-setup\\done.marker); read C:\\paddock-setup\\setup.log in the VM"
    shut_down 180
    for dev_key in sata0:0 sata0:1; do broker set "$lease_id" --key "$dev_key.present" --value FALSE >/dev/null; done
    broker snapshot "$lease_id" --name "$baseline" >/dev/null
    echo "snapshot $baseline taken; the install discs are detached and the VM is off"
    ;;
  -h | --help | help) usage ;;
  *) usage >&2; exit 2 ;;
esac

#!/usr/bin/env bash
# Drives the local Windows 11 Arm test VM (VMware Fusion, headless through vmrun; commands inside it over SSH).
# Local only, never CI. Setup and the update test: docs/testing/windows-test-vm.md.
#
#   scripts/vm-win.sh start [--no-wait]       start headless and wait until SSH answers
#   scripts/vm-win.sh stop                    shut Windows down (over SSH), power off if it does not stop in time
#   scripts/vm-win.sh ssh [--admin] [cmd...]  SSH as paddock-test (or paddock-admin); a shell without a command
#   scripts/vm-win.sh snapshot <name>         take a snapshot
#   scripts/vm-win.sh revert [name]           revert to a snapshot (default clean-baseline); the VM is then off
#   scripts/vm-win.sh status | ip             running or stopped / the guest's address
#   scripts/vm-win.sh install                 one-time: boot the installer in Fusion's window, wait for setup to power off
#   scripts/vm-win.sh baseline                one-time: detach the install discs, take the clean-baseline snapshot
#
# Only ever acts on the VM under PADDOCK_WIN_VM_DIR (default /Volumes/d7sst7/vms/paddock-win11-test); any other VM on
# this Mac is never started, stopped or changed. If the VM has a virtual TPM, its encryption password is read from the
# login keychain (service paddock-win11-test-vm, account vmrun) and passed to vmrun as -vp.
set -euo pipefail

dir=${PADDOCK_WIN_VM_DIR:-/Volumes/d7sst7/vms/paddock-win11-test}
fusion=${FUSION_APP:-/Applications/VMware Fusion.app}
lib="$fusion/Contents/Library"
name=paddock-win11-test
vmx="$dir/vm/$name.vmwarevm/$name.vmx"
mac=00:50:56:2a:d0:c7
leases=${PADDOCK_VM_LEASES:-/var/db/vmware/vmnet-dhcpd-vmnet8.leases}
max_load=${PADDOCK_VM_MAX_LOAD:-25}
ssh_deadline=${PADDOCK_VM_SSH_DEADLINE:-300}
install_deadline=${PADDOCK_VM_INSTALL_DEADLINE:-7200}
baseline=clean-baseline

die() { echo "vm-win: $*" >&2; exit 1; }
usage() { sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; }

guard() {
  case "$dir/" in "$HOME"/*) die "PADDOCK_WIN_VM_DIR must not be under \$HOME (ER-14): $dir" ;; esac
  [ -f "$vmx" ] || die "no test VM at $vmx; run scripts/vm-win/prepare.sh first"
  [ -x "$lib/vmrun" ] || die "vmrun not found in $lib (set FUSION_APP)"
}

vmrun() {
  local pw
  pw=$(security find-generic-password -s paddock-win11-test-vm -a vmrun -w 2>/dev/null || true)
  local cmd=$1; shift
  if [ -n "$pw" ]; then "$lib/vmrun" -T fusion -vp "$pw" "$cmd" "$vmx" "$@"; else "$lib/vmrun" -T fusion "$cmd" "$vmx" "$@"; fi
}

running() { "$lib/vmrun" -T fusion list | grep -Fxq "$vmx"; }

load_ok() {
  local load
  load=$(sysctl -n vm.loadavg | awk '{print $2}')
  awk -v l="$load" -v m="$max_load" 'BEGIN{exit !(l<m)}' || die "1-minute load $load is not under $max_load; try again later"
}

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

has_snapshot() { vmrun listSnapshots | tail -n +2 | grep -Fxq "$1"; }

cmd=${1:-}
[ $# -gt 0 ] && shift
case $cmd in
  start)
    guard
    if running; then echo "already running"; else load_ok; vmrun start nogui; fi
    [ "${1:-}" = "--no-wait" ] || { wait_ssh; echo "ready: $(ip)"; }
    ;;
  stop)
    guard
    running || { echo "already stopped"; exit 0; }
    (ssh_to paddock-admin 'shutdown /s /t 0') >/dev/null 2>&1 || true
    wait_off 120 "shutting down" || { echo "  no clean shutdown in 120 s; powering off"; vmrun stop hard; }
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
    vmrun snapshot "$1"
    echo "snapshot $1 taken"
    ;;
  revert)
    guard
    snap=${1:-$baseline}
    has_snapshot "$snap" || die "no snapshot named $snap"
    vmrun revertToSnapshot "$snap"
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
    [ -z "$win_iso" ] || [ -f "$win_iso" ] || die "Windows ISO missing at $win_iso (docs/testing/windows-test-vm.md, step 1)"
    [ -f "$dir/iso/paddock-setup.iso" ] || die "setup disc missing; run scripts/vm-win/prepare.sh"
    has_snapshot "$baseline" && die "the VM already has $baseline; revert to it instead of reinstalling"
    running && die "the VM is running; stop it first"
    load_ok
    echo "Starting the installer in a Fusion window. Click into it and press a key at \"Press any key to boot from CD\"."
    echo "Setup is unattended from there; the VM powers off when it is done (about 20 to 40 minutes)."
    vmrun start gui
    wait_off "$install_deadline" "installing Windows" || die "setup did not finish within ${install_deadline}s; look at the Fusion window"
    echo "setup finished and the VM is off; next: scripts/vm-win.sh baseline"
    ;;
  baseline)
    guard
    running && die "the VM is running; baseline is taken from a powered-off VM"
    has_snapshot "$baseline" && die "$baseline already exists"
    for dev in sata0:0 sata0:1; do "$lib/vmcli" "$vmx" ConfigParams SetEntry "$dev.present" FALSE >/dev/null; done
    load_ok
    vmrun start nogui
    wait_ssh
    ssh_to paddock-admin 'if (-not (Test-Path C:\paddock-setup\done.marker)) { exit 3 }' ||
      die "setup did not finish (no C:\\paddock-setup\\done.marker); read C:\\paddock-setup\\setup.log in the VM"
    (ssh_to paddock-admin 'shutdown /s /t 0') >/dev/null 2>&1 || true
    wait_off 180 "shutting down" || vmrun stop hard
    vmrun snapshot "$baseline"
    echo "snapshot $baseline taken; the VM is off"
    ;;
  -h | --help | help) usage ;;
  *) usage >&2; exit 2 ;;
esac

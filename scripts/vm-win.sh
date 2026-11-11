#!/usr/bin/env bash
# Drives the local Windows 11 Arm test VM (VMware Fusion, headless through vmrun; commands inside it over SSH).
# Local only, never CI. Setup and the update test: docs/testing/windows-test-vm.md.
#
#   scripts/vm-win.sh start [--no-wait]       start headless and wait until SSH answers
#   scripts/vm-win.sh stop                    shut Windows down (over SSH), power off if it does not stop in time
#   scripts/vm-win.sh ssh [--admin] [cmd...]  SSH as paddock-test (or paddock-admin); a shell without a command
#   scripts/vm-win.sh put|get <user> <src> <dst>   scp a file to / from the guest (user paddock-test or paddock-admin; dst/src guest path)
#   scripts/vm-win.sh gui-setup               one-time: auto-logon of paddock-test + the in-session job runner, snapshot gui-baseline
#   scripts/vm-win.sh job <file.ps1> [secs]   run a script ON the desktop of paddock-test (UI Automation works), print its output; exit = its code
#   scripts/vm-win.sh snapshot <name>         take a snapshot
#   scripts/vm-win.sh revert [name]           revert to a snapshot (default clean-baseline); the VM is then off
#   scripts/vm-win.sh status | ip             running or stopped / the guest's address
#   scripts/vm-win.sh install                 one-time: install Windows headless, wait for setup to power off
#   scripts/vm-win.sh baseline                one-time: check setup finished, detach the discs, take clean-baseline
#
# Only ever acts on /Volumes/d8sst7/vm-data/paddock-win11-test: any other folder in PADDOCK_WIN_VM_DIR is refused before vmrun,
# ssh or scp run (vm-win/target.sh), except a fake VM under $TMPDIR with a fake FUSION_APP, for this script's own tests.
# Any other VM on this Mac is never started, stopped or changed. Only if the VM is encrypted (option A, virtual TPM) is its password read
# from the login keychain (service paddock-win11-test-vm, account vmrun) and passed to vmrun as -vp.
set -euo pipefail
. "$(cd "$(dirname "$0")" && pwd)/vm-win/target.sh"

dir=${PADDOCK_WIN_VM_DIR:-$WIN_VM_DIR}
fusion=${FUSION_APP:-/Applications/VMware Fusion.app}
lib="$fusion/Contents/Library"
name=paddock-win11-test
vmx="$dir/vm/$name.vmwarevm/$name.vmx"
mac=00:50:56:2a:d0:c7
leases=/var/db/vmware/vmnet-dhcpd-vmnet8.leases # PADDOCK_VM_LEASES only in fake mode (guard)
max_load=${PADDOCK_VM_MAX_LOAD:-25}
ssh_deadline=${PADDOCK_VM_SSH_DEADLINE:-300}
install_deadline=${PADDOCK_VM_INSTALL_DEADLINE:-7200}
baseline=clean-baseline

die() { echo "vm-win: $*" >&2; exit 1; }
usage() { sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'; }

guard() {
  case "$dir/" in "$HOME"/*) die "PADDOCK_WIN_VM_DIR must not be under \$HOME (ER-14): $dir" ;; esac
  check_target "$dir" "$fusion"
  [ "$FAKE_VM" = 1 ] && leases=${PADDOCK_VM_LEASES:-$leases}
  [ -f "$vmx" ] || die "no test VM at $vmx; run scripts/vm-win/prepare.sh first"
  [ -x "$lib/vmrun" ] || die "vmrun not found in $lib (set FUSION_APP)"
}

# An encrypted VM (option A: TPM files encrypted) has encryption.* keys in its .vmx and needs the password from the login
# keychain; an unencrypted VM (option B, the default set-up) never reads the keychain.
encrypted() { grep -q '^encryption\.' "$vmx"; }

vmrun() {
  local cmd=$1; shift
  if encrypted; then
    local pw
    pw=$(security find-generic-password -s paddock-win11-test-vm -a vmrun -w 2>/dev/null) ||
      die "the VM is encrypted but the keychain has no item paddock-win11-test-vm/vmrun (docs/testing/windows-test-vm.md, TPM)"
    "$lib/vmrun" -T fusion -vp "$pw" "$cmd" "$vmx" "$@"
  else
    "$lib/vmrun" -T fusion "$cmd" "$vmx" "$@"
  fi
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

scp_via() { # user, then scp arguments with @ placeholders for user@host
  local user=$1; shift
  local host; host=$(ip)
  scp -q -i "$dir/keys/$user" -o IdentitiesOnly=yes -o BatchMode=yes -o UserKnownHostsFile="$dir/keys/known_hosts" -o StrictHostKeyChecking=accept-new "${@//@HOST@/$user@$host}"
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
# One user of the test VM at a time (interim lock, scripts/vm-lock.sh): only the commands that change or load the guest.
case $cmd in
  start | stop | ssh | put | get | gui-setup | job | snapshot | revert | install | baseline)
    . "$(cd "$(dirname "$0")" && pwd)/vm-lock.sh"; vm_lock_acquire "vm-win-$cmd" ;;
esac
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
  put)
    guard
    [ $# -eq 3 ] || die "usage: vm-win.sh put <user> <local file> <guest path>"
    scp_via "$1" "$2" "@HOST@:$3"
    ;;
  get)
    guard
    [ $# -eq 3 ] || die "usage: vm-win.sh get <user> <guest path> <local file>"
    scp_via "$1" "@HOST@:$2" "$3"
    ;;
  gui-setup)
    guard
    here=$(cd "$(dirname "$0")" && pwd)
    running || { load_ok; vmrun start nogui; }
    wait_ssh
    pw=$(sed -n 's/^paddock-test=//p' "$dir/keys/setup-passwords")
    [ -n "$pw" ] || die "no paddock-test password in $dir/keys/setup-passwords"
    scp_via paddock-admin "$here/vm-win/logon-runner.ps1" "@HOST@:C:/paddock-test/logon-runner.ps1"
    scp_via paddock-admin "$here/vm-win/gui-setup.ps1" "@HOST@:C:/paddock-test/gui-setup.ps1"
    ssh_to paddock-admin "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\paddock-test\\gui-setup.ps1 -TestPassword '$pw' -RunnerSource C:\\paddock-test\\logon-runner.ps1"
    ssh_to paddock-admin 'Remove-Item C:\paddock-test\gui-setup.ps1 -Force; shutdown /s /t 0' >/dev/null 2>&1 || true
    wait_off 180 "shutting down" || vmrun stop hard
    has_snapshot gui-baseline && vmrun deleteSnapshot gui-baseline
    vmrun snapshot gui-baseline
    echo "snapshot gui-baseline taken (clean-baseline + auto-logon + job runner); use: vm-win.sh revert gui-baseline"
    ;;
  job)
    guard
    [ -f "${1:-}" ] || die "usage: vm-win.sh job <file.ps1> [timeout secs]"
    limit=${2:-300}
    id="job-$(date +%s)-$$"
    # The runner starts at the console logon of paddock-test; wait for it (a heartbeat file newer than 10 s).
    end=$((SECONDS + 300))
    until ssh_to paddock-test '$f=Get-Item C:\paddock-test\runner.alive -ErrorAction SilentlyContinue; if ($f -and ((Get-Date)-$f.LastWriteTime).TotalSeconds -lt 10) { exit 0 } else { exit 1 }' >/dev/null 2>&1; do
      [ $SECONDS -lt $end ] || die "the in-session runner is not alive (is this the gui-baseline snapshot? vm-win.sh gui-setup)"
      sleep 3
    done
    scp_via paddock-test "$1" "@HOST@:C:/paddock-test/jobs/$id.ps1"
    end=$((SECONDS + limit))
    until ssh_to paddock-test "if (Test-Path C:\\paddock-test\\jobs\\$id.done) { exit 0 } else { exit 1 }" >/dev/null 2>&1; do
      [ $SECONDS -lt $end ] || die "job $id did not finish within ${limit}s"
      sleep 2
    done
    ssh_to paddock-test "Get-Content C:\\paddock-test\\jobs\\$id.out" 2>/dev/null | grep -v -e 'post-quantum' -e 'store now' -e 'openssh.com/pq' || true
    code=$(ssh_to paddock-test "(Get-Content C:\\paddock-test\\jobs\\$id.done).Trim()" 2>/dev/null | tail -n 1 | tr -d '\r')
    ssh_to paddock-test "Remove-Item C:\\paddock-test\\jobs\\$id.* -Force" >/dev/null 2>&1 || true
    exit "${code:-1}"
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
    [ -z "$win_iso" ] || [ -f "$win_iso" ] || die "Windows ISO missing at $win_iso; run scripts/vm-win/prepare.sh (docs/testing/windows-test-vm.md)"
    [ -f "$dir/iso/paddock-setup.iso" ] || die "setup disc missing; run scripts/vm-win/prepare.sh"
    has_snapshot "$baseline" && die "the VM already has $baseline; revert to it instead of reinstalling"
    running && die "the VM is running; stop it first"
    for dev in sata0:0 sata0:1; do "$lib/vmcli" "$vmx" ConfigParams SetEntry "$dev.present" TRUE >/dev/null; done
    load_ok
    echo "Installing Windows headless from the no-prompt ISO (no window, no key to press); the VM powers off when done."
    started=$SECONDS
    vmrun start nogui
    wait_off "$install_deadline" "installing Windows" || die "setup did not finish within ${install_deadline}s; see $(dirname "$vmx")/vmware.log"
    took=$(((SECONDS - started) / 60))
    # Setup takes far longer than this; an earlier stop is a crash or a boot failure, never a finished install.
    [ $took -ge "${PADDOCK_VM_MIN_INSTALL_MIN:-10}" ] || die "the VM stopped after $took min, too early for a finished setup; see $(dirname "$vmx")/vmware.log"
    echo "the VM powered off after $took min; next: scripts/vm-win.sh baseline (it checks that setup finished)"
    ;;
  baseline)
    guard
    running && die "the VM is running; baseline is taken from a powered-off VM"
    has_snapshot "$baseline" && die "$baseline already exists"
    load_ok
    # Check first, with the discs still attached: Windows Setup puts Windows Boot Manager first in the boot order, so
    # the disk boots; if setup had not finished, nothing is lost and install can be run again.
    vmrun start nogui
    wait_ssh
    ssh_to paddock-admin 'if (-not (Test-Path C:\paddock-setup\done.marker)) { exit 3 }' ||
      die "setup did not finish (no C:\\paddock-setup\\done.marker); read C:\\paddock-setup\\setup.log in the VM"
    (ssh_to paddock-admin 'shutdown /s /t 0') >/dev/null 2>&1 || true
    wait_off 180 "shutting down" || vmrun stop hard
    for dev in sata0:0 sata0:1; do "$lib/vmcli" "$vmx" ConfigParams SetEntry "$dev.present" FALSE >/dev/null; done
    vmrun snapshot "$baseline"
    echo "snapshot $baseline taken; the install discs are detached and the VM is off"
    ;;
  -h | --help | help) usage ;;
  *) usage >&2; exit 2 ;;
esac

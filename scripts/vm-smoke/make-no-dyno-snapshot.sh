#!/usr/bin/env bash
# One-off: a Windows test VM snapshot `gui-session-tools-no-dyno` = `gui-session-tools` (the session tools: auto-logon, SSH keys, actor) with NO Dyno of any kind
# (a per-user Dyno in the older snapshot makes the per-machine installer refuse, exit 2). Existing snapshots are never deleted or changed. Run it under the VM lock:
#   PLOCK_DIR=/private/tmp/claude-501/vm-lock PLOCK_PAR_LOAD=0 plock run <name> -- scripts/vm-smoke/make-no-dyno-snapshot.sh
# The VM is reverted to gui-session-tools, started, cleaned over SSH, stopped, snapshotted; nothing edits the vmx.
set -euo pipefail
VMWIN="${PADDOCK_VM_WIN:-/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh}"
FROM=gui-session-tools; NEW=gui-session-tools-no-dyno
VMRUN="/Applications/VMware Fusion.app/Contents/Public/vmrun"
VMX="${PADDOCK_WIN_VMX:-/Volumes/d7sst7/vms/paddock-win11-test/vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx}"
if "$VMRUN" listSnapshots "$VMX" | grep -qx "$NEW"; then echo "snapshot $NEW already exists: nothing to do"; exit 0; fi
if bash "$VMWIN" status | grep -q running; then echo "::error::the Windows test VM is running (another job?); not touching it"; exit 2; fi
trap 'bash "$VMWIN" stop >/dev/null 2>&1 || true' ERR
bash "$VMWIN" revert "$FROM"
bash "$VMWIN" start
# Every per-user trace of Dyno for the test user (the guard scans the account hives: file + Uninstall key + Run value), the machine-wide one too.
bash "$VMWIN" ssh 'Get-Process dyno-app,dyno-agent,dyno-bench -ErrorAction SilentlyContinue | Stop-Process -Force; Remove-Item "$env:LOCALAPPDATA\Dyno" -Recurse -Force -ErrorAction SilentlyContinue; Remove-Item "$env:APPDATA\Dyno" -Recurse -Force -ErrorAction SilentlyContinue; Remove-Item "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Dyno" -Recurse -Force -ErrorAction SilentlyContinue; $r = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"; (Get-ItemProperty $r -ErrorAction SilentlyContinue).PSObject.Properties | Where-Object { $_.Value -like "*Dyno*" } | ForEach-Object { Remove-ItemProperty $r $_.Name }; exit 0'
bash "$VMWIN" ssh --admin 'Remove-Item "C:\Program Files\Dyno" -Recurse -Force -ErrorAction SilentlyContinue; Remove-Item "C:\ProgramData\Dyno" -Recurse -Force -ErrorAction SilentlyContinue; Remove-Item "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Dyno" -Recurse -Force -ErrorAction SilentlyContinue; exit 0'
left=$(bash "$VMWIN" ssh '(Test-Path "$env:LOCALAPPDATA\Dyno") -or (Test-Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Dyno")' | tr -d '\r' | tail -1)
[ "$left" = "False" ] || { echo "::error::a per-user Dyno is still there after the clean-up ($left); no snapshot taken"; bash "$VMWIN" stop || true; exit 2; }
bash "$VMWIN" stop
bash "$VMWIN" snapshot "$NEW"
"$VMRUN" listSnapshots "$VMX" | grep -qx "$NEW" && echo "snapshot $NEW taken from $FROM"

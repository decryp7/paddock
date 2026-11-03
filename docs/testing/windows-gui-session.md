# Windows test VM: an interactive session for GUI checks

SSH runs in session 0 and has no desktop, so a window, a tray icon or a console window cannot be seen from it. Used for
Dyno 1.0.0 (2026-11-03) to reproduce the visible `dyno-agent` console window. VM: `paddock-win11-test` (see `scripts/vm-win.sh` on `tools/win-test-vm`).

1. Snapshot first (`vm-win.sh snapshot <name>`); start the VM.
2. Auto-logon for `paddock-test` (as `paddock-admin` over SSH): under `HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon` set
   `AutoAdminLogon=1`, `DefaultUserName=paddock-test`, `DefaultDomainName=<computer name>`, `DefaultPassword=<the VM-local password from keys/setup-passwords>`;
   `shutdown /r /t 5`. `query user` then shows `paddock-test` on `console`. The password is VM-local; never reuse it elsewhere.
3. Copy a script to the VM and run it in that session with a scheduled task: `schtasks /create /tn Probe /sc once /st 00:00 /ru paddock-test /it /f /tr
   "powershell -NoProfile -ExecutionPolicy Bypass -File C:\paddock-test\probe.ps1"`, then `schtasks /run /tn Probe`. The script writes a text log and PNG
   screenshots (`scripts/vm-smoke/win-gui-probe.ps1` lists visible windows by process, starts `dyno-agent run` as the Run entry does, starts
   `dyno-app --hidden`, dumps `HKCU\Control Panel\NotifyIconSettings` and takes screenshots); copy them back with `scp`.
4. Poll the log for its last line instead of sleeping a fixed time. Quote nothing on the command line: copy a `.ps1` file (`powershell -File`) rather than `-Command`.

Wait on conditions, not delays. Revert to `clean-baseline` when done unless the auto-logon state is wanted: the snapshot `gui-session` keeps it.

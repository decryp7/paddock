# Runs Dyno's real NSIS installer (built from the worktree, with the stub paddock-setup.exe) inside Tauri's real template, as an
# administrator, on the test VM only (ER-14), and checks the include's behaviour end to end. -Installer is the setup exe.
param([string]$Installer = 'C:\paddock-test\e2e\Dyno_1.1.0_x64-setup.exe', [string]$Root = 'C:\paddock-test\e2e')
$ErrorActionPreference = 'Stop'
$log = Join-Path $Root 'stub.log'
$pub = 'C:\Users\Public\paddock-stub.log'
$inst = 'C:\Program Files\Dyno'
$key = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Dyno'
$fail = 0
function Check($name, $ok, $detail) { if ($ok) { "PASS $name" } else { $script:fail++; "FAIL $name :: $detail" } }
function Run($exe, $argline, $envs) {
  Remove-Item $log, $pub -ErrorAction SilentlyContinue
  $saved = @{}; $envs['PADDOCK_STUB_LOG'] = $log
  foreach ($k in $envs.Keys) { $saved[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, $envs[$k]) }
  try { $p = Start-Process -FilePath $exe -ArgumentList $argline -Wait -PassThru } finally { foreach ($k in $envs.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) } }
  $lines = @(); if (Test-Path $log) { $lines = @(Get-Content $log) }
  [pscustomobject]@{ Code = $p.ExitCode; Lines = $lines }
}
function Cmds($r) { ($r.Lines | ForEach-Object { ($_ -split ' ')[0] }) -join ',' }
function WaitGone($path) { $end = (Get-Date).AddSeconds(40); while ((Test-Path $path) -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 300 } }
function Cleanup { Get-Process dyno-app, dyno-agent -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$inst\*" } | Stop-Process -Force -ErrorAction SilentlyContinue }
Remove-Item -Recurse -Force $inst -ErrorAction SilentlyContinue; Remove-Item $key -ErrorAction SilentlyContinue

# 2 (first, on a fresh machine): the guard blocks; the real template's SetOutPath made the folder, the include removed it again
$msg = 'Uninstall the older Dyno first (Settings > Apps), then run this installer again.'
$r = Run $Installer '/S' @{ PADDOCK_STUB_EXIT_GUARD = '2'; PADDOCK_STUB_OUT_GUARD = "$msg`r`n" }
Check 'guard blocks: exit code 20' ($r.Code -eq 20) "code $($r.Code)"
Check 'guard blocks: the folder is gone' (-not (Test-Path $inst)) (Get-ChildItem $inst -Name -ErrorAction SilentlyContinue)
Check 'guard blocks: nothing after it' (-not ($r.Lines -match '^stop-all|^clean')) ($r.Lines -join '|')
Check 'guard blocks: no uninstall key' (-not (Test-Path $key)) 'key exists'

# 1: fresh install
$r = Run $Installer '/S' @{}
Check 'fresh: exit 0' ($r.Code -eq 0) "code $($r.Code)"
Check 'fresh: calls in order' ((Cmds $r) -match 'check-dir,guard,stop-all,clean-run,clean-staging') (Cmds $r)
Check 'fresh: stop list from the bundle' ($r.Lines -match 'stop-all .* --images "dyno-app.exe,dyno-agent.exe,dyno.exe,dyno-bench.exe,dyno-bench-gpu.exe"') ($r.Lines -join '|')
$files = 'dyno-app.exe', 'dyno-agent.exe', 'dyno.exe', 'dyno-bench.exe', 'dyno-bench-gpu.exe', 'dyno-helper.exe', 'paddock-setup.exe', 'uninstall.exe'
$missing = $files | Where-Object { -not (Test-Path "$inst\$_") }
Check 'fresh: files in Program Files' ($missing.Count -eq 0) ($missing -join ',')
Check 'fresh: HKLM uninstall key' (Test-Path $key) 'no key'
Check 'fresh: no HKCU uninstall key' (-not (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Dyno')) 'per-user key'

# P1-2: on Arm the x86 emulation cache (XtaCache) maps every x64 program that has run; an update after the app ran and quit must
# not be refused because of it
$ver = & "$inst\dyno.exe" --version 2>&1; & "$inst\dyno-agent.exe" --version 2>&1 | Out-Null
Check 'arm: the programs ran and quit' ($LASTEXITCODE -eq 0 -or $ver) "version: $ver"
$r = Run $Installer '/S /UPDATE' @{}
Check 'arm: update after the programs ran: exit 0 (no foreign process blocks it)' ($r.Code -eq 0) "code $($r.Code)"

# 3: update as the updater runs it (/UPDATE; /R starts the app again, unelevated, through RunAsUser)
$r = Run $Installer '/S /UPDATE /R' @{}
Check 'update: exit 0' ($r.Code -eq 0) "code $($r.Code)"
Check 'update: no guard' (-not ($r.Lines -match '^guard')) ($r.Lines -join '|')
Check 'update: stop-all before the files' ($r.Lines -match '^stop-all') ($r.Lines -join '|')
# (/R starting the app again needs the installer in the user's own session: the UAC variant, e2er.sh, covers it)
Cleanup

# guard with /UPDATE /R: the old per-user copy is started again as the desktop user
Remove-Item -Recurse -Force $inst, $key -ErrorAction SilentlyContinue
$r = Run $Installer '/S /UPDATE /R' @{ PADDOCK_STUB_EXIT_GUARD = '2'; PADDOCK_STUB_OUT_GUARD = "$msg`r`nRESTART:$Root\old-copy.exe`r`n" }
Check 'guard+restart: exit 20' ($r.Code -eq 20) "code $($r.Code)"
Check 'guard+restart: folder removed' (-not (Test-Path $inst)) 'folder left'

# reinstall for the next cases
$r = Run $Installer '/S' @{}
Check 'reinstall after repair: exit 0' ($r.Code -eq 0) "code $($r.Code)"

# 5: in-place uninstall (what a manual upgrade's reinstall page runs): an update, updates\ kept
New-Item -ItemType Directory -Force "$inst\updates" | Out-Null; 'x' | Set-Content "$inst\updates\setup.log"
$r = Run "$inst\uninstall.exe" "/S _?=$inst" @{ PADDOCK_STUB_EXIT_SAME_FOLDER = '0' }
Check 'in place: exit 0' ($r.Code -eq 0) "code $($r.Code)"
Check 'in place: same-folder asked, treated as update (machine scope)' (($r.Lines -match '^same-folder') -and ($r.Lines -match 'clean-run .* --scope machine') -and -not ($r.Lines -match '--scope all')) ($r.Lines -join '|')
Check 'in place: program files gone, setup gone, updates kept' ((-not (Test-Path "$inst\dyno-app.exe")) -and (-not (Test-Path "$inst\paddock-setup.exe")) -and (Test-Path "$inst\updates\setup.log")) (Get-ChildItem $inst -Name -ErrorAction SilentlyContinue)
Check 'in place: the template removed the uninstall key (the new installer writes it again)' (-not (Test-Path $key)) 'key still there'
Remove-Item -Recurse -Force $inst -ErrorAction SilentlyContinue
$r = Run $Installer '/S' @{}
Check 'repair after the half-removed state: fresh install runs the guard' (($r.Code -eq 0) -and ($r.Lines -match '^guard')) "code $($r.Code) $($r.Lines -join '|')"

# 4: a real uninstall
New-Item -ItemType Directory -Force "$inst\updates" | Out-Null; 'x' | Set-Content "$inst\updates\setup.log"
$r = Run "$inst\uninstall.exe" '/S' @{ PADDOCK_STUB_EXIT_SAME_FOLDER = '1' }
WaitGone $inst
$lines = @(Get-Content $log -ErrorAction SilentlyContinue)
Check 'uninstall: the folder is gone (updates removed, then the non-recursive RMDir)' (-not (Test-Path $inst)) (Get-ChildItem $inst -Name -ErrorAction SilentlyContinue)
Check 'uninstall: calls' (($lines -match '^same-folder') -and ($lines -match '^stop-all') -and ($lines -match 'clean-run .* --scope all')) ($lines -join '|')
Check 'uninstall: the HKLM key is gone' (-not (Test-Path $key)) 'key still there'
Cleanup
if ($fail -eq 0) { 'ALL PASS' } else { "FAILURES: $fail"; exit 1 }

# Runs the harness installer (build.sh) against the stub paddock-setup.exe on a Windows machine, as an administrator, and
# checks the order of the calls, the exit codes and what is left behind. ER-14: run it in the test VM only; it writes only
# below -Root. Usage: powershell -File run-harness.ps1 -Root C:\paddock-test\s1s4 (harness-installer.exe is in -Root)
param([string]$Root = 'C:\paddock-test\s1s4')
$ErrorActionPreference = 'Stop'
$installer = Join-Path $Root 'harness-installer.exe'
$log = Join-Path $Root 'stub.log'
$fail = 0
function Check($name, $ok, $detail) {
  if ($ok) { "PASS $name" } else { $script:fail++; "FAIL $name :: $detail" }
}
function Run($exe, $argline, $envs) {
  Remove-Item $log -ErrorAction SilentlyContinue
  $saved = @{}
  $envs['PADDOCK_STUB_LOG'] = $log
  foreach ($k in $envs.Keys) { $saved[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, $envs[$k]) }
  try { $p = Start-Process -FilePath $exe -ArgumentList $argline -Wait -PassThru } finally {
    foreach ($k in $envs.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
  }
  $lines = @(); if (Test-Path $log) { $lines = @(Get-Content $log) }
  [pscustomobject]@{ Code = $p.ExitCode; Lines = $lines }
}
function Cmds($r) { ($r.Lines | ForEach-Object { ($_ -split ' ')[0] }) -join ',' }
function WaitGone($path) { $end = (Get-Date).AddSeconds(30); while ((Test-Path $path) -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 200 } }

Remove-Item -Recurse -Force (Join-Path $Root 'run') -ErrorAction SilentlyContinue
$base = Join-Path $Root 'run'; New-Item -ItemType Directory -Force $base | Out-Null
$key = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PaddockHarness'
Remove-Item $key -ErrorAction SilentlyContinue

# A: fresh install
$a = Join-Path $base 'a'
$r = Run $installer "/S /D=$a" @{}
Check 'A fresh install exit 0' ($r.Code -eq 0) "code $($r.Code)"
Check 'A order of calls' ((Cmds $r) -match '^(hold-installing,)?check-dir,guard,stop-all,clean-run,clean-staging$' -or (Cmds $r) -match 'check-dir,guard,stop-all,clean-run,clean-staging') (Cmds $r)
Check 'A clean-run machine scope' ($r.Lines -match 'clean-run --dir .* --scope machine') ($r.Lines -join '|')
Check 'A stop list' ($r.Lines -match 'stop-all --dir .* --images "harness.exe,harness-agent.exe"') ($r.Lines -join '|')
Check 'A files' ((Test-Path "$a\harness.txt") -and (Test-Path "$a\paddock-setup.exe") -and (Test-Path "$a\uninstall.exe")) (Get-ChildItem $a -Name -ErrorAction SilentlyContinue)

# E: update over the existing install: no guard
$r = Run $installer "/S /UPDATE /D=$a" @{}
Check 'E update exit 0' ($r.Code -eq 0) "code $($r.Code)"
Check 'E update skips the guard' (-not ($r.Lines -match '^guard')) ($r.Lines -join '|')

# G: reinstall in place (same folder): an update, updates\ kept
New-Item -ItemType Directory -Force "$a\updates" | Out-Null; 'x' | Set-Content "$a\updates\setup.log"
$r = Run "$a\uninstall.exe" "/S _?=$a" @{ PADDOCK_STUB_EXIT_SAME_FOLDER = '0' }
Check 'G in-place exit 0' ($r.Code -eq 0) "code $($r.Code)"
Check 'G treated as an update: machine scope' ($r.Lines -match 'clean-run --dir .* --scope machine' -and -not ($r.Lines -match '--scope all')) ($r.Lines -join '|')
Check 'G setup removed, updates kept' ((-not (Test-Path "$a\paddock-setup.exe")) -and (Test-Path "$a\updates\setup.log")) (Get-ChildItem $a -Name -ErrorAction SilentlyContinue)
Check 'G stop-all still runs' ($r.Lines -match '^stop-all') ($r.Lines -join '|')

# Reinstall again so F has a full install.
$r = Run $installer "/S /D=$a" @{}

# F: a real uninstall (the uninstaller copies itself to the temp folder and runs from there)
$r = Run "$a\uninstall.exe" "/S" @{ PADDOCK_STUB_EXIT_SAME_FOLDER = '1' }
WaitGone $a
$lines = @(Get-Content $log -ErrorAction SilentlyContinue)
Check 'F uninstall removed the folder' (-not (Test-Path $a)) (Get-ChildItem $a -Name -ErrorAction SilentlyContinue)
Check 'F uninstall calls' (($lines -match '^same-folder') -and ($lines -match '^stop-all') -and ($lines -match 'clean-run --dir .* --scope all')) ($lines -join '|')
Check 'F uninstall removed the key' (-not (Test-Path $key)) 'key still there'

# B: check-dir refused
$b = Join-Path $base 'b'
$r = Run $installer "/S /D=$b" @{ PADDOCK_STUB_EXIT_CHECK_DIR = '3' }
Check 'B check-dir refusal exit 3' ($r.Code -eq 3) "code $($r.Code)"
Check 'B empty folder removed' (-not (Test-Path $b)) 'folder left'
Check 'B message' ($r.Lines -match 'ABORT level 3: Install PaddockHarness in Program Files') ($r.Lines -join '|')
Check 'B nothing after the refusal' (-not ($r.Lines -match '^stop-all')) ($r.Lines -join '|')

# C: guard blocked, update with /R: starts the old copy as the desktop user
$c = Join-Path $base 'c'
$msg = "Uninstall the older PaddockHarness first (Settings > Apps), then run this installer again."
$r = Run $installer "/S /UPDATE /R /D=$c" @{ PADDOCK_STUB_EXIT_GUARD = '2'; PADDOCK_STUB_OUT_GUARD = "$msg`r`nRESTART:C:\old copy\app.exe`r`n" }
Check 'C guard exit 2' ($r.Code -eq 2) "code $($r.Code)"
Check 'C guard args carry update and restart' ($r.Lines -match '^guard --product "PaddockHarness" --main "harness.exe" --dir ".*" --update --restart$') ($r.Lines -join '|')
Check 'C old copy started again' ($r.Lines -contains 'RunAsUser C:\old copy\app.exe') ($r.Lines -join '|')
Check 'C message without the restart line' ($r.Lines -contains "ABORT level 2: $msg") ($r.Lines -join '|')
Check 'C empty folder removed' (-not (Test-Path $c)) 'folder left'
# C2: the same without /UPDATE: nothing started
$r = Run $installer "/S /D=$c" @{ PADDOCK_STUB_EXIT_GUARD = '2'; PADDOCK_STUB_OUT_GUARD = "$msg`r`nRESTART:C:\old copy\app.exe" }
Check 'C2 no restart without /UPDATE' ((-not ($r.Lines -match '^RunAsUser')) -and $r.Code -eq 2) ($r.Lines -join '|')

# D: a listed program still runs
$d = Join-Path $base 'd'
$r = Run $installer "/S /D=$d" @{ PADDOCK_STUB_EXIT_STOP_ALL = '11' }
Check 'D still running exit 4' ($r.Code -eq 4) "code $($r.Code)"
Check 'D message' ($r.Lines -match 'ABORT level 4: PaddockHarness could not be closed for every signed-in user') ($r.Lines -join '|')
Check 'D folder removed' (-not (Test-Path $d)) 'folder left'
# D2: the tool itself failed
$r = Run $installer "/S /D=$d" @{ PADDOCK_STUB_EXIT_STOP_ALL = '5' }
Check 'D2 tool failure exit 5' ($r.Code -eq 5) "code $($r.Code)"

Remove-Item $key -ErrorAction SilentlyContinue
if ($fail -eq 0) { 'ALL PASS' } else { "FAILURES: $fail"; exit 1 }

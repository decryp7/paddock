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

# H: the uninstaller without paddock-setup.exe next to it (deleted by hand): the uninstall still works, says what it skipped,
# and treats an in-place run as a reinstall by comparing the folders itself
$h = Join-Path $base 'h'
$r = Run $installer "/S /D=$h" @{}
Remove-Item "$h\paddock-setup.exe"
$r = Run "$h\uninstall.exe" "/S _?=$h" @{ PADDOCK_STUB_EXIT_SAME_FOLDER = '1' }
Check 'H no setup: in-place uninstall exit 0' ($r.Code -eq 0) "code $($r.Code)"
Check 'H no setup: says it skipped the steps' ($r.Lines -match '^NOTE paddock-setup.exe is missing') ($r.Lines -join '|')
Check 'H no setup: no call reached the missing program' (-not ($r.Lines -match '^(stop-all|clean-run|same-folder)')) ($r.Lines -join '|')
Check 'H no setup: the in-place run is an update by its own folder compare' ($r.Lines -match '^NOTE reinstall: treated as an update') ($r.Lines -join '|')
$r = Run $installer "/S /D=$h" @{}
Remove-Item "$h\paddock-setup.exe"
$r = Run "$h\uninstall.exe" "/S" @{}
WaitGone $h
Check 'H no setup: a real uninstall removes the folder' (-not (Test-Path $h)) (Get-ChildItem $h -Name -ErrorAction SilentlyContinue)
Remove-Item $key -ErrorAction SilentlyContinue

# B: check-dir refused
$b = Join-Path $base 'b'
$r = Run $installer "/S /D=$b" @{ PADDOCK_STUB_EXIT_CHECK_DIR = '3' }
Check 'B check-dir refusal exit 21 (its own code, not NSIS cancel)' ($r.Code -eq 21) "code $($r.Code)"
Check 'B empty folder removed' (-not (Test-Path $b)) 'folder left'
Check 'B message' ($r.Lines -match 'ABORT level 21: Install PaddockHarness in Program Files') ($r.Lines -join '|')
Check 'B nothing after the refusal' (-not ($r.Lines -match '^stop-all')) ($r.Lines -join '|')

# C: guard blocked, update with /R: starts the old copy as the desktop user
$c = Join-Path $base 'c'
$msg = "Uninstall the older PaddockHarness first (Settings > Apps), then run this installer again."
Remove-Item (Join-Path $env:TEMP 'PaddockHarness-setup-refused.log') -ErrorAction SilentlyContinue
$r = Run $installer "/S /UPDATE /R /D=$c" @{ PADDOCK_STUB_EXIT_GUARD = '2'; PADDOCK_STUB_OUT_GUARD = "$msg`r`nRESTART:C:\old copy\app.exe`r`n" }
Check 'C guard exit 20 (its own code: NSIS uses 2 for a cancelled install)' ($r.Code -eq 20) "code $($r.Code)"
Check 'C guard args carry update and restart' ($r.Lines -match '^guard --product "PaddockHarness" --main "harness.exe" --dir ".*" --update --restart$') ($r.Lines -join '|')
Check 'C old copy started again' ($r.Lines -contains 'RunAsUser C:\old copy\app.exe') ($r.Lines -join '|')
Check 'C message without the restart line' ($r.Lines -contains "ABORT level 20: $msg") ($r.Lines -join '|')
Check 'C empty folder removed' (-not (Test-Path $c)) 'folder left'
# C3: the refusal is written where it survives (the folder is gone), and the details name that file
$refusal = Join-Path $env:TEMP 'PaddockHarness-setup-refused.log'
Check 'C3 refusal log in %TEMP%: the reason and where the details are' ((Test-Path $refusal) -and ((Get-Content $refusal -Raw) -match 'level 20: Uninstall the older PaddockHarness first') -and ((Get-Content $refusal -Raw) -match 'updates.setup.log')) ((Get-Content $refusal -Raw -ErrorAction SilentlyContinue))
# C2: the same without /UPDATE: nothing started
$r = Run $installer "/S /D=$c" @{ PADDOCK_STUB_EXIT_GUARD = '2'; PADDOCK_STUB_OUT_GUARD = "$msg`r`nRESTART:C:\old copy\app.exe" }
Check 'C2 no restart without /UPDATE' ((-not ($r.Lines -match '^RunAsUser')) -and $r.Code -eq 20) ($r.Lines -join '|')

# D: a listed program still runs
$d = Join-Path $base 'd'
$r = Run $installer "/S /D=$d" @{ PADDOCK_STUB_EXIT_STOP_ALL = '11' }
Check 'D still running exit 22' ($r.Code -eq 22) "code $($r.Code)"
Check 'D message' ($r.Lines -match 'ABORT level 22: PaddockHarness could not be closed for every signed-in user') ($r.Lines -join '|')
Check 'D folder removed' (-not (Test-Path $d)) 'folder left'
# D2: the tool itself failed
$r = Run $installer "/S /D=$d" @{ PADDOCK_STUB_EXIT_STOP_ALL = '5' }
Check 'D2 tool failure exit 23' ($r.Code -eq 23) "code $($r.Code)"

Remove-Item $key -ErrorAction SilentlyContinue
if ($fail -eq 0) { 'ALL PASS' } else { "FAILURES: $fail"; exit 1 }

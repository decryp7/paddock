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
# Waits for a real condition (a script block that becomes true); the ceiling only turns a hang into a failure with its reason.
function WaitUntil([scriptblock]$cond, $reason, [int]$ceilingSeconds = 60) {
  $end = (Get-Date).AddSeconds($ceilingSeconds)
  while (-not (& $cond)) { if ((Get-Date) -gt $end) { return "did not happen within ${ceilingSeconds}s: $reason" }; Start-Sleep -Milliseconds 50 }
  return $null
}
function WaitGone($path) { $null = WaitUntil { -not (Test-Path $path) } "the folder $path to be removed" }

Remove-Item (Join-Path $env:TEMP 'PaddockHarness-setup-refused.log') -ErrorAction SilentlyContinue   # a leftover of an older include
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
# The launched uninstall.exe copies itself and returns after about 0.45 s; the copy does the work and ends with DeleteRegKey and
# then RMDir (measured on the VM: 180 runs, the folder is gone 0.12 s after the launcher exits). So the launcher's exit is the
# wrong signal; the real one is the copy's DeleteRegKey, the step before RMDir.
$why = WaitUntil { -not (Test-Path $key) } 'the uninstaller copy to remove the uninstall key (its last step before RMDir)'
Check 'H no setup: the uninstaller ran to its end' ($null -eq $why) $why
$why = WaitUntil { -not (Test-Path $h) -or ((Get-ChildItem $h -Force -Name -ErrorAction SilentlyContinue) -join ',') -eq 'uninstall.exe' } 'the folder to be removed or to hold only the launcher file' 10
$left = (Get-ChildItem $h -Force -Name -ErrorAction SilentlyContinue) -join ','
Check 'H no setup: a real uninstall removes the files it installed (nothing but NSIS launcher file may remain)' ($null -eq $why -and ($left -eq '' -or $left -eq 'uninstall.exe')) "left: $left; $why"
if ($left -eq 'uninstall.exe') { "NOTE H: the folder holds only uninstall.exe: NSIS's own Delete of its launcher failed once (measured: the first uninstall in a freshly started VM, 3 of 3; later runs 0 of 177); our hooks ran before it and left nothing else" }
Remove-Item $key -ErrorAction SilentlyContinue

# B: check-dir refused
$b = Join-Path $base 'b'
$r = Run $installer "/S /D=$b" @{ PADDOCK_STUB_EXIT_CHECK_DIR = '3' }
Check 'B check-dir refusal exit 41 (its own code, not NSIS cancel)' ($r.Code -eq 41) "code $($r.Code)"
Check 'B empty folder removed' (-not (Test-Path $b)) 'folder left'
Check 'B message' ($r.Lines -match 'ABORT level 41: Install PaddockHarness in Program Files') ($r.Lines -join '|')
Check 'B a refused folder gets no note (there is no safe place to write it)' (-not ($r.Lines -match '^note')) ($r.Lines -join '|')
Check 'B nothing after the refusal' (-not ($r.Lines -match '^stop-all')) ($r.Lines -join '|')

# C: guard blocked, update with /R: starts the old copy as the desktop user
$c = Join-Path $base 'c'
$msg = "Uninstall the older PaddockHarness first (Settings > Apps), then run this installer again."
$r = Run $installer "/S /UPDATE /R /D=$c" @{ PADDOCK_STUB_EXIT_GUARD = '2'; PADDOCK_STUB_OUT_GUARD = "$msg`r`nRESTART:C:\old copy\app.exe`r`n" }
Check 'C guard exit 40 (its own code: NSIS uses 2 for an aborted install)' ($r.Code -eq 40) "code $($r.Code)"
Check 'C guard args carry update and restart' ($r.Lines -match '^guard --product "PaddockHarness" --main "harness.exe" --dir ".*" --update --restart$') ($r.Lines -join '|')
Check 'C old copy started again' ($r.Lines -contains 'RunAsUser C:\old copy\app.exe') ($r.Lines -join '|')
Check 'C message without the restart line' ($r.Lines -contains "ABORT level 40: $msg") ($r.Lines -join '|')
Check 'C empty folder removed' (-not (Test-Path $c)) 'folder left'
# C3: the refusal is written through the tool's checked log (a note to $INSTDIR\updates\setup.log), never to %TEMP%
Check 'C3 the include asked the tool to note the refusal' ($r.Lines -match '^note --dir ".*\\run\\c" --text "refused \(level 40\)') ($r.Lines -join '|')
Check 'C3 nothing is written under %TEMP% by the include' (-not (Test-Path (Join-Path $env:TEMP 'PaddockHarness-setup-refused.log'))) 'a temp file exists'
# C2: the same without /UPDATE: nothing started
$r = Run $installer "/S /D=$c" @{ PADDOCK_STUB_EXIT_GUARD = '2'; PADDOCK_STUB_OUT_GUARD = "$msg`r`nRESTART:C:\old copy\app.exe" }
Check 'C2 no restart without /UPDATE' ((-not ($r.Lines -match '^RunAsUser')) -and $r.Code -eq 40) ($r.Lines -join '|')

# D: a listed program still runs
$d = Join-Path $base 'd'
$r = Run $installer "/S /D=$d" @{ PADDOCK_STUB_EXIT_STOP_ALL = '11' }
Check 'D still running exit 42' ($r.Code -eq 42) "code $($r.Code)"
Check 'D message' ($r.Lines -match 'ABORT level 42: PaddockHarness could not be closed for every signed-in user') ($r.Lines -join '|')
Check 'D folder removed' (-not (Test-Path $d)) 'folder left'
# D2: the tool itself failed
$r = Run $installer "/S /D=$d" @{ PADDOCK_STUB_EXIT_STOP_ALL = '5' }
Check 'D2 tool failure exit 43' ($r.Code -eq 43) "code $($r.Code)"

Remove-Item $key -ErrorAction SilentlyContinue
# R: the real GUI paddock-setup.exe (harness-installer-real.exe, built with SETUP_EXE): the guard's stdout is read through nsExec
$real = Join-Path $Root 'harness-installer-real.exe'
if (Test-Path $real) {
  $uk = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PaddockHarness'
  New-Item $uk -Force | Out-Null; Set-ItemProperty $uk UninstallString '"C:\Users\nobody\AppData\Local\PaddockHarness\uninstall.exe"'
  $rr = 'C:\Program Files\PaddockHarnessReal'   # the real tool's check-dir accepts only a direct child of Program Files
  Remove-Item -Recurse -Force $rr -ErrorAction SilentlyContinue
  $r = Run $real "/S /D=$rr" @{}
  Remove-Item $uk -ErrorAction SilentlyContinue
  Check 'R real GUI tool: guard refused with its own level' ($r.Code -eq 40) "code $($r.Code)"
  Check 'R real GUI tool: its stdout message reached the installer through nsExec' ($r.Lines -match 'ABORT level 40: Uninstall the older PaddockHarness first') ($r.Lines -join '|')
  Check 'R real GUI tool: the note went into the tool-written setup.log' ((Test-Path "$rr\updates\setup.log") -and ((Get-Content "$rr\updates\setup.log" -Raw) -match 'refused \(level 40\)')) (Get-ChildItem $rr -Recurse -Name -ErrorAction SilentlyContinue)
  Remove-Item -Recurse -Force $rr -ErrorAction SilentlyContinue
}
if ($fail -eq 0) { 'ALL PASS' } else { "FAILURES: $fail"; exit 1 }

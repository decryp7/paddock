# Runs the harness installer (build.sh) against the stub paddock-setup.exe on a Windows machine, as an administrator, and
# checks the order of the calls, the exit codes and what is left behind. ER-14: run it in the test VM only; it writes only
# below -Root. Usage: powershell -File run-harness.ps1 -Root C:\paddock-test\s1s4 (harness-installer.exe is in -Root)
param([string]$Root = 'C:\paddock-test\s1s4', [switch]$Cold, [int]$ColdRounds = 1, [switch]$NoSetup)
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
# Runs `<dir>\uninstall.exe /S` the way a user does and waits for the uninstaller COPY's last step, the hook's FINISH_UNINSTALL line
# (the launcher returns after about 0.45 s, long before the copy finishes). With $lockMs > 0 a read handle that denies delete is held
# on uninstall.exe from before the start until that many ms after it: a transient lock, deterministic. Returns the outcome fields.
function UninstallWait($dir, [int]$lockMs = 0) {
  Remove-Item $log -ErrorAction SilentlyContinue
  [Environment]::SetEnvironmentVariable('PADDOCK_STUB_LOG', $log)
  [Environment]::SetEnvironmentVariable('PADDOCK_STUB_EXIT_SAME_FOLDER', '1')   # the stub tool says: not a reinstall, a real uninstall
  $script:fs = $null; $script:lockFrom = $null
  if ($lockMs -gt 0) { $script:fs = [IO.File]::Open("$dir\uninstall.exe", 'Open', 'Read', 'Read') }
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $null = Start-Process -FilePath "$dir\uninstall.exe" -ArgumentList '/S' -PassThru
  $why = WaitUntil {
    # The lock runs from the moment the uninstaller reaches its clean-up (the log line of clean-run), not from the process start:
    # on a loaded or cold VM the start-up takes anywhere from 1 s to 10 s, and a lock timed from the start was gone before the hook ran.
    if ($script:fs -and -not $script:lockFrom -and (@(Get-Content $log -ErrorAction SilentlyContinue) -match '^clean-run').Count -gt 0) { $script:lockFrom = $sw.ElapsedMilliseconds }
    if ($script:fs -and $script:lockFrom -and ($sw.ElapsedMilliseconds - $script:lockFrom) -gt $lockMs) { $script:fs.Dispose(); $script:fs = $null }
    (Test-Path $log) -and (@(Get-Content $log -ErrorAction SilentlyContinue) -match '^FINISH_UNINSTALL').Count -gt 0
  } 'the uninstaller copy to reach FINISH_UNINSTALL' 90
  if ($script:fs) { $script:fs.Dispose(); $script:fs = $null }
  [Environment]::SetEnvironmentVariable('PADDOCK_STUB_LOG', $null)
  [Environment]::SetEnvironmentVariable('PADDOCK_STUB_EXIT_SAME_FOLDER', $null)
  $lines = @(Get-Content $log -ErrorAction SilentlyContinue)
  $f = $lines | Where-Object { $_ -match '^FINISH_UNINSTALL' } | Select-Object -First 1
  [pscustomobject]@{ Why = $why; Line = $f; Lines = $lines; Outcome = if ($f -match 'outcome=(\S+)') { $Matches[1] } else { $null }; Tries = if ($f -match 'tries=(\d+)') { [int]$Matches[1] } else { -1 }; Ms = $sw.ElapsedMilliseconds }
}

Remove-Item (Join-Path $env:TEMP 'PaddockHarness-setup-refused.log') -ErrorAction SilentlyContinue   # a leftover of an older include
Remove-Item -Recurse -Force (Join-Path $Root 'run') -ErrorAction SilentlyContinue
$base = Join-Path $Root 'run'; New-Item -ItemType Directory -Force $base | Out-Null
$key = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PaddockHarness'
$key32 = 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\PaddockHarness'   # the 32-bit harness installer writes here
if ($Cold) {
  # One install and one uninstall, nothing else: run this first after a fresh VM start (the cold case) and report the outcome.
  for ($i = 1; $i -le $ColdRounds; $i++) {
    $d = Join-Path $base "cold$i"
    $null = Run $installer "/S /D=$d" @{}
    if ($NoSetup) { Remove-Item "$d\paddock-setup.exe" }   # the scenario of case H: the tool is missing from the install folder
    $u = UninstallWait $d
    "COLD noSetup=$NoSetup round=$i outcome=$($u.Outcome) tries=$($u.Tries) ms=$($u.Ms) folderGone=$(-not (Test-Path $d)) why=[$($u.Why)] line=[$($u.Line)]"
  }
  exit 0
}
Remove-Item $key -ErrorAction SilentlyContinue

# A: fresh install
$a = Join-Path $base 'a'
$r = Run $installer "/S /D=$a" @{}
Check 'A fresh install exit 0' ($r.Code -eq 0) "code $($r.Code)"
$cmdsNoVersion = (Cmds $r) -replace 'check-version,',''
Check 'A order of calls' ($cmdsNoVersion -match '^(hold-installing,)?check-dir,guard,stop-all,clean-run,clean-staging$' -or $cmdsNoVersion -match 'check-dir,guard,stop-all,clean-run,clean-staging') (Cmds $r)
# The include reads the embedded program's own version (the stub's 9.8.7), never the installer's: check-version --expect names it.
Check 'A check-version expects the embedded program version' ($r.Lines -match 'check-version --dir .* --expect 9\.8\.7$') ($r.Lines -join '|')
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
# The launcher returns after about 0.45 s and the copy finishes 0.12 to 0.18 s later (measured on the VM), so the real signal is the
# hook's FINISH_UNINSTALL line, the last thing the copy does. After the retry fix the folder must be gone: a leftover no longer passes.
Remove-Item $key -ErrorAction SilentlyContinue
$r = Run $installer "/S /D=$h" @{}
Remove-Item "$h\paddock-setup.exe"
$u = UninstallWait $h
Check 'H no setup: the uninstaller reached its last step' ($null -eq $u.Why) $u.Why
Check 'H no setup: a real uninstall removes the folder (removed, or removed after retries)' ((-not (Test-Path $h)) -and $u.Outcome -in 'removed', 'removed-after-retries') "outcome $($u.Outcome); left: $((Get-ChildItem $h -Force -Name -ErrorAction SilentlyContinue) -join ',')"
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
# M: Windows Security removed paddock-setup.exe (ADR 0053, the Dyno 1.1.0 false detection): the installer must end with level 44 and never
# report a success. The stub removes itself (a rename, allowed while it runs) or the installed copy after the named command.
$m = Join-Path $base 'm'
$r = Run $installer "/S /D=$m" @{ PADDOCK_STUB_REMOVE_AFTER = 'check-dir'; PADDOCK_STUB_REMOVE_PATH = 'self' }
Check 'M1 extracted tool gone before guard: exit 44' ($r.Code -eq 44) "code $($r.Code)"
Check 'M1 says what to do' ($r.Lines -match 'ABORT level 44: Windows Security removed paddock-setup.exe') ($r.Lines -join '|')
Check 'M1 no later call reached the tool' (-not ($r.Lines -match '^(guard|stop-all|clean-run|clean-staging)')) ($r.Lines -join '|')
Check 'M1 no install was registered' (-not (Test-Path $key)) 'uninstall key exists'
$r = Run $installer "/S /D=$m" @{ PADDOCK_STUB_REMOVE_AFTER = 'stop-all'; PADDOCK_STUB_REMOVE_PATH = 'self' }
Check 'M2 extracted tool gone before the POSTINSTALL calls: exit 44' ($r.Code -eq 44) "code $($r.Code)"
Check 'M2 reports the abort, not a success' (($r.Lines -match 'ABORT level 44') -and -not ($r.Lines -match '^clean-staging')) ($r.Lines -join '|')
Check 'M2 state after the abort: files and uninstall key are there (the abort comes in POSTINSTALL)' ((Test-Path "$m\harness.txt") -and ((Test-Path $key) -or (Test-Path $key32))) (Get-ChildItem $m -Name -ErrorAction SilentlyContinue)
$r = Run $installer "/S /D=$m" @{ PADDOCK_STUB_REMOVE_AFTER = 'clean-run'; PADDOCK_STUB_REMOVE_PATH = "$m\paddock-setup.exe" }
Check 'M3 installed copy gone after it was written: exit 44' ($r.Code -eq 44) "code $($r.Code)"
Check 'M3 reports the abort, after clean-run and clean-staging finished' (($r.Lines -match 'ABORT level 44: .* is installed, but Windows Security removed paddock-setup.exe') -and ($r.Lines -match '^clean-staging')) ($r.Lines -join '|')
Check 'M3 state after the abort: files and uninstall key are there' ((Test-Path "$m\harness.txt") -and ((Test-Path $key) -or (Test-Path $key32))) (Get-ChildItem $m -Name -ErrorAction SilentlyContinue)
Remove-Item $key -ErrorAction SilentlyContinue
# M4: a real uninstall whose copy of the tool vanishes after its first call: it says so, skips the steps that need the tool, uses the
# Restart Manager fallback and still uninstalls (folder and key gone)
$r = Run $installer "/S /D=$m" @{}
Check 'M4 clean install for the uninstaller case' ($r.Code -eq 0) "code $($r.Code)"
$env:PADDOCK_STUB_REMOVE_AFTER = 'same-folder'; $env:PADDOCK_STUB_REMOVE_PATH = 'self'
try { $u = UninstallWait $m } finally { $env:PADDOCK_STUB_REMOVE_AFTER = $null; $env:PADDOCK_STUB_REMOVE_PATH = $null }
Check 'M4 the uninstaller reached its last step' ($null -eq $u.Why) $u.Why
Check 'M4 says Windows Security may have removed it' ($u.Lines -match '^NOTE paddock-setup.exe is missing \((before|after) a call\).*Windows Security') ($u.Lines -join '|')
Check 'M4 uses the Restart Manager fallback and no call reached the missing program' (($u.Lines -match '^NOTE paddock-setup.exe is missing: Restart Manager fallback') -and -not ($u.Lines -match '^(stop-all|clean-run)')) ($u.Lines -join '|')
Check 'M4 the folder and the uninstall key are gone' ((-not (Test-Path $m)) -and -not (Test-Path $key) -and -not (Test-Path $key32)) "folder: $(Test-Path $m)"
Remove-Item $key -ErrorAction SilentlyContinue
# M5: an in-place run (_?=, the reinstall page of a manual upgrade) whose copy of the tool vanished after the copy is still a reinstall:
# update mode, and the helper is not removed (the harness hook logs HELPER_UNINSTALL where Dyno runs dyno-helper uninstall)
$r = Run $installer "/S /D=$m" @{}
$r = Run "$m\uninstall.exe" "/S _?=$m" @{ PADDOCK_TEST_DROP_TOOL = '1'; PADDOCK_STUB_EXIT_SAME_FOLDER = '1' }
Check 'M5 in-place run exit 0' ($r.Code -eq 0) "code $($r.Code)"
Check 'M5 treated as an update by the folder compare' ($r.Lines -match '^NOTE reinstall: treated as an update') ($r.Lines -join '|')
Check 'M5 the helper is not uninstalled' (-not ($r.Lines -match '^HELPER_UNINSTALL')) ($r.Lines -join '|')
Remove-Item $key -ErrorAction SilentlyContinue
# U: the post-uninstall retry (PADDOCK_FINISH_UNINSTALL)
foreach ($c in @(
  @{ Name = 'U0 no lock'; Lock = 0 },
  @{ Name = 'U1 a transient lock of 2.5 s'; Lock = 2500 })) {
  $d = Join-Path $base ('u' + $c.Name.Substring(1, 1))
  $null = Run $installer "/S /D=$d" @{}
  $u = UninstallWait $d $c.Lock
  Check "$($c.Name): the uninstaller reached its last step" ($null -eq $u.Why) $u.Why
  Check "$($c.Name): the folder is gone" (-not (Test-Path $d)) "outcome $($u.Outcome); $($u.Line)"
  if ($c.Lock -eq 0) { Check "$($c.Name): outcome removed without a retry" ($u.Outcome -eq 'removed' -and $u.Tries -eq 0) $u.Line }
  else {
    Check "$($c.Name): outcome removed-after-retries" ($u.Outcome -eq 'removed-after-retries' -and $u.Tries -ge 1) $u.Line
    Check "$($c.Name): the cause was recorded (an exclusive open failed with a sharing violation, 32)" ($u.Line -match 'open-error-32') $u.Line
    Check "$($c.Name): it waited for the lock to go (about its 2.5 s, under the 5 s bound)" ($u.Ms -ge 2500 -and $u.Ms -lt 6000) "$($u.Ms) ms"
  }
}
# U2: a folder with another file keeps that file and the folder (nothing recursive)
$d = Join-Path $base 'u2'
$null = Run $installer "/S /D=$d" @{}
Set-Content "$d\user-notes.txt" 'keep me'
$u = UninstallWait $d
Check 'U2 other file: our files went, the uninstaller reached its end' (($null -eq $u.Why) -and -not (Test-Path "$d\uninstall.exe") -and -not (Test-Path "$d\harness.txt")) "$($u.Why) $($u.Line)"
Check 'U2 other file: the other file and the folder stay, outcome kept-other-files' ((Test-Path "$d\user-notes.txt") -and $u.Outcome -eq 'kept-other-files') $u.Line
Remove-Item -Recurse -Force $d -ErrorAction SilentlyContinue
# U3: a lock longer than the bound: the file stays (distinct outcome, nothing registered for a restart), and a reinstall over it works
$d = Join-Path $base 'u3'
$null = Run $installer "/S /D=$d" @{}
$pendingBefore = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager' -Name PendingFileRenameOperations -ErrorAction SilentlyContinue).PendingFileRenameOperations
$u = UninstallWait $d 12000
Check 'U3 long lock: the uninstaller reached its last step' ($null -eq $u.Why) $u.Why
Check 'U3 long lock: outcome left-in-place after the full bound' ($u.Outcome -eq 'left-in-place' -and $u.Tries -ge 40) $u.Line
Check 'U3 long lock: the tool was asked to note why' ($u.Lines -match '^note --dir .* --text "uninstall.exe could not be removed after') ($u.Lines -join '|')
$pendingAfter = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager' -Name PendingFileRenameOperations -ErrorAction SilentlyContinue).PendingFileRenameOperations
# (NSIS registers its own temporary plug-in DLLs for deletion at restart: only entries naming this folder count)
Check 'U3 long lock: nothing of this folder is registered for the next restart' (-not (@($pendingAfter) -match [regex]::Escape($d))) (@($pendingAfter) -match [regex]::Escape($d) -join '|')
Check 'U3 long lock: the file is left and the folder holds only what is ours' ((Test-Path "$d\uninstall.exe") -and -not (Test-Path "$d\harness.txt")) ((Get-ChildItem $d -Force -Name -ErrorAction SilentlyContinue) -join ',')
$re = Run $installer "/S /D=$d" @{}
Check 'U3 reinstall over the leftover works (exit 0, files back, key back)' (($re.Code -eq 0) -and (Test-Path "$d\harness.txt") -and (Test-Path "$d\uninstall.exe") -and ((Test-Path $key) -or (Test-Path $key32))) "code $($re.Code); harness.txt $(Test-Path "$d\harness.txt"); uninstall.exe $(Test-Path "$d\uninstall.exe"); key $((Test-Path $key) -or (Test-Path $key32)); lines $($re.Lines -join '|')"
$u = UninstallWait $d
Check 'U3 and uninstalling it again removes the folder' (($null -eq $u.Why) -and -not (Test-Path $d)) "$($u.Outcome) $($u.Line)"

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

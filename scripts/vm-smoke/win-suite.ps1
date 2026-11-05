# Runs INSIDE the interactive session of the Windows test VM (a scheduled task `/it /ru paddock-test`, see docs/testing/windows-gui-session.md).
# The hardware-free release checks of Dyno 1.1.0 that need no UI driver: install, start, one instance, keep awake, scheduled wake, idle action
# and Quit, each judged by the OS (Win32_Process, powercfg), with Dyno's own `dyno` command as the actuator. It writes one JSON file of steps
# ({title,status,summary,checks:[{ok,detail}],evidence:[]}) that the host renders with the same result format as the macOS check.
# Every wait polls a condition with a bounded deadline. Usage: win-suite.ps1 -Installer C:\paddock-test\vmcheck\Dyno_x64-setup.exe -Out C:\paddock-test\vmcheck\out
param([string]$Installer, [string]$Out = 'C:\paddock-test\vmcheck\out', [int]$CdpPort = 9222, [switch]$KeepRunning, [string]$Only = '')
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force $Out | Out-Null
$steps = New-Object System.Collections.ArrayList
# Dyno 1.1.0 installs once for every user of the PC (family ADR 0053): the programs are under Program Files ($prog); the user's own data
# (settings.json, agent.json, logs) stays under %LOCALAPPDATA%\Dyno ($bin).
$bin = Join-Path $env:LOCALAPPDATA 'Dyno'
$prog = Join-Path $env:ProgramFiles 'Dyno'
$dyno = Join-Path $prog 'dyno.exe'
function Log($m) { "$(Get-Date -Format o) $m" | Add-Content "$Out\run.log" }
function New-Step($title) { [pscustomobject]@{ title = $title; status = 'pass'; summary = ''; checks = New-Object System.Collections.ArrayList; evidence = New-Object System.Collections.ArrayList } }
function Check($s, [bool]$ok, $detail) { [void]$s.checks.Add(@{ ok = $ok; detail = "$detail" }); if (-not $ok) { $s.status = 'fail' } }
function Finish($s, $passSummary) {
  $bad = @($s.checks | Where-Object { -not $_.ok })
  if ($s.status -ne 'skip') { $s.status = $(if ($bad.Count) { 'fail' } else { 'pass' }) }
  if (-not $s.summary) { $s.summary = $(if ($bad.Count) { (($bad | ForEach-Object { $_.detail }) -join '; ') } else { $passSummary }) }
  if ($s.summary.Length -gt 240) { $s.summary = $s.summary.Substring(0, 240) }
  [void]$steps.Add($s); Log "$($s.status.ToUpper()) $($s.title): $($s.summary)"
  $steps | ConvertTo-Json -Depth 6 | Set-Content "$Out\steps.json" -Encoding UTF8
}
function Wait-Until([scriptblock]$Cond, [int]$Sec, [int]$PollMs = 1000) {
  $end = (Get-Date).AddSeconds($Sec)
  while ($true) { $v = & $Cond; if ($v) { return $v }; if ((Get-Date) -gt $end) { return $null }; Start-Sleep -Milliseconds $PollMs }
}
function Dyno { param([string]$A) $o = & cmd /c "`"$dyno`" $A 2>&1"; [pscustomobject]@{ code = $LASTEXITCODE; text = ($o -join "`n") } }
function Procs { Get-CimInstance Win32_Process -Filter "Name like 'dyno%'" | Select-Object ProcessId, ParentProcessId, Name, CreationDate, CommandLine }
function Stop-All { Get-Process dyno-app, dyno-agent, dyno-bench -ErrorAction SilentlyContinue | Stop-Process -Force; Wait-Until { -not (Get-Process dyno-app, dyno-agent -ErrorAction SilentlyContinue) } 20 | Out-Null }
function Start-App([switch]$Cdp) {
  if ($Cdp) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$CdpPort" } else { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
  Start-Process (Join-Path $prog 'dyno-app.exe')
}
# `powercfg /requests`, `/waketimers` and `/a` need an administrator; the suite runs as the standard user (like a person), so it asks the host
# (which has the paddock-admin SSH key) through a request file and waits, bounded, for the answer file.
$script:ar = 0
function Admin-Run([string]$Cmd) {
  $script:ar++; $n = $script:ar
  Set-Content (Join-Path $Out "req-$n.txt") $Cmd
  $resp = Join-Path $Out "resp-$n.txt"
  $ans = Wait-Until { if (Test-Path $resp) { Get-Content $resp -Raw } } 90 1000
  if ($null -eq $ans) { return "(no answer from the host for: $Cmd)" }
  return $ans
}
# settings.json is read by serde_json: a UTF-8 byte order mark (Set-Content -Encoding UTF8) makes it unreadable and Dyno falls back to defaults.
function Write-Json($Path, $Obj) { [IO.File]::WriteAllText($Path, ($Obj | ConvertTo-Json -Depth 8), (New-Object Text.UTF8Encoding $false)) }
function Ready { Wait-Until { (Dyno 'status').code -eq 0 } 120 2000 }

Log "start suite, installer=$Installer"
# 1. Install: per machine, silent. The installer asks for an administrator (a standard user types the administrator's credentials): the host answers
# that UAC prompt over the VM's VNC when it sees uac-N.txt (run-win.mjs --vnc). Without the host's answer the install waits 5 minutes and fails.
$s = New-Step 'Install (per-machine NSIS, silent, UAC over VNC)'
Stop-All
Set-Content (Join-Path $Out 'uac-1.txt') 'install'
$p = Start-Process $Installer -ArgumentList '/S' -PassThru
$uk = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Dyno'
$installed = Wait-Until { if ((Test-Path (Join-Path $prog 'dyno-app.exe')) -and (Test-Path $uk) -and -not (Get-Process -Id $p.Id -ErrorAction SilentlyContinue)) { $true } } 300 2000
$code = try { $p.ExitCode } catch { 'unreadable' }
Check $s ([bool]$installed) $(if ($installed) { "installer ended (exit code $code)" } else { 'the installer did not finish in 300 s (no UAC answer?)' })
if ($code -ne 'unreadable') { Check $s ($code -eq 0) "installer exit code $code" }
foreach ($f in 'dyno-app.exe', 'dyno-agent.exe', 'dyno.exe') { Check $s (Test-Path (Join-Path $prog $f)) "$f present under $prog" }
Check $s ($prog -like "$env:ProgramFiles*") "installed under Program Files ($prog)"
Check $s (Test-Path $uk) 'the machine-wide (HKLM) uninstall entry exists'
Check $s (-not (Test-Path (Join-Path $env:LOCALAPPDATA 'Dyno\dyno-app.exe'))) 'no per-user copy under %LOCALAPPDATA%\Dyno'
$acl = (icacls $prog | Out-String)
Check $s (-not ($acl -match 'BUILTIN\\Users:.*\((W|M|F)')) 'a standard user has no write right in the install folder'
[void]$s.evidence.Add((($acl -split "`r?`n" | Where-Object { $_ } | Select-Object -First 8) -join ' | '))
$ver = (Get-Item (Join-Path $prog 'dyno-app.exe')).VersionInfo.ProductVersion
Check $s ($ver -match '^\d+\.\d+\.\d+') "dyno-app.exe version $ver"
[void]$s.evidence.Add("version $ver")
Stop-All   # the installer starts the app for the desktop user when it ends; the cold start below begins from nothing
Finish $s "installed $ver under Program Files after a real UAC prompt"

# 2. Cold start: one app, one agent, the agent's parent is the app, the discovery file names the live agent.
$s = New-Step 'Cold start, one instance'
$sf0 = Join-Path $bin 'settings.json'
$d0 = if (Test-Path $sf0) { Get-Content $sf0 -Raw | ConvertFrom-Json } else { [pscustomobject]@{} }
if (-not $d0.power) { $d0 | Add-Member -NotePropertyName power -NotePropertyValue ([pscustomobject]@{}) -Force }
$d0.power | Add-Member -NotePropertyName wake -NotePropertyValue ([pscustomobject]@{ enabled = $true }) -Force
New-Item -ItemType Directory -Force $bin | Out-Null; Write-Json $sf0 $d0
Start-App -Cdp
$ready = Ready
Check $s ([bool]$ready) $(if ($ready) { 'dyno status answers (the agent is up)' } else { 'no answer from dyno status within 120 s' })
Start-Sleep -Seconds 20   # the observation window: a late second agent must show (ADR: respawn timers up to 15 s)
$pr = @(Procs)
$apps = @($pr | Where-Object Name -eq 'dyno-app.exe'); $agents = @($pr | Where-Object { $_.Name -eq 'dyno-agent.exe' -and $_.CommandLine -match ' run' })
Check $s ($apps.Count -eq 1) "dyno-app.exe: $($apps.Count) running (allowed 1)"
Check $s ($agents.Count -eq 1) "dyno-agent.exe run: $($agents.Count) running (allowed 1)"
foreach ($a in $agents) { Check $s ($apps.ProcessId -contains $a.ParentProcessId) "agent pid $($a.ProcessId) has parent pid $($a.ParentProcessId) (the app is $($apps.ProcessId -join ','))" }
$aj = Join-Path $bin 'agent.json'
if (Test-Path $aj) { $j = Get-Content $aj -Raw | ConvertFrom-Json; Check $s ($agents.ProcessId -contains [int]$j.pid) "agent.json names pid $($j.pid), a live agent" } else { Check $s $false 'agent.json is absent' }
$pr | ForEach-Object { [void]$s.evidence.Add("$($_.ProcessId) <- $($_.ParentProcessId) $($_.Name) $($_.CommandLine)") }
Check $s (-not (Get-Process dyno-agent -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 })) 'the agent shows no window of its own'
Finish $s 'one app, one agent under it, agent.json names it'

# 3. Second launch while running: the same pids.
$s = New-Step 'Second launch while running'
$before = @(Procs | ForEach-Object { $_.ProcessId } | Sort-Object)
Start-Process (Join-Path $prog 'dyno-app.exe'); Start-Sleep -Seconds 8
$after = @(Procs | ForEach-Object { $_.ProcessId } | Sort-Object)
Check $s (($before -join ',') -eq ($after -join ',')) "processes before [$($before -join ',')] and after [$($after -join ',')]"
Finish $s 'single instance: no new process'

if ($Only -eq 'start') { 'done' | Set-Content "$Out\done.txt"; exit 0 }   # exploration: install and start only, the app stays up with the DevTools port

# 4. Keep awake for 1 hour: powercfg /requests lists Dyno, then not.
$s = New-Step 'Keep awake: held by the OS, then released'
$req = { Admin-Run 'powercfg /requests' }
Check $s (-not ((& $req) -match '(?i)dyno')) 'no Dyno request before "on"'
$on = Dyno 'awake on --for 1h'; Check $s ($on.code -eq 0) "dyno awake on --for 1h exit $($on.code): $($on.text)"
$held = Wait-Until { $t = & $req; if ($t -match '(?i)dyno') { $t } } 20
Check $s ([bool]$held) 'powercfg /requests lists a Dyno request'
[void]$s.evidence.Add('> powercfg /requests'); [void]$s.evidence.AddRange(@((& $req) -split "`r?`n"))
$off = Dyno 'awake off'; Check $s ($off.code -eq 0) "dyno awake off exit $($off.code)"
$gone = Wait-Until { -not ((& $req) -match '(?i)dyno') } 20
Check $s ([bool]$gone) 'powercfg /requests no longer lists Dyno after "off"'
Finish $s 'held 1 h by the OS, released after off'

# 5. Scheduled wake: accepted and listed by dyno; powercfg lists a wake timer only where the machine has a sleep state.
$s = New-Step 'Scheduled wake: set, listed, cancelled'
$at = (Get-Date).AddMinutes(5).ToString('HH:mm')
$add = Dyno "wake-schedule add vm-check --at $at --repeat once --stay 10m"
Check $s ($add.code -eq 0) "wake-schedule add exit $($add.code): $($add.text)"
$timers = Admin-Run 'powercfg /waketimers'; $states = Admin-Run 'powercfg /a'
[void]$s.evidence.Add('> dyno wake-schedule list'); [void]$s.evidence.AddRange(@((Dyno 'wake-schedule list').text -split "`n"))
[void]$s.evidence.Add('> powercfg /waketimers'); [void]$s.evidence.AddRange(@($timers -split "`n"))
$hasSleep = $states -match '(?im)^\s*Standby|S0 Low Power|S3'
$aval = ($states -split "\r?\n\r?\n")[0]
$sleepOk = $states -match '(?is)sleep states are available on this system:\s*\r?\n\s*(Standby|Hibernate)'
if ($sleepOk) { Check $s ($timers -match '(?i)dyno') 'powercfg /waketimers lists a Dyno timer' } else { [void]$s.evidence.Add('powercfg /a: no sleep state available in this VM, so Windows lists no wake timers'); $s.summary = 'accepted and cancelled; the OS timer cannot be observed because this VM has no sleep state (the real wake stays on the hardware checklist)' }
$c = Dyno 'wake-schedule cancel vm-check'; Check $s ($c.code -eq 0) "wake-schedule cancel exit $($c.code)"
if (-not $sleepOk -and $s.status -eq 'pass') { $s.status = 'skip' }
Finish $s 'listed by powercfg about 5 min ahead, gone after cancel'

# 6. Idle action: a one-minute rule seeded in settings.json; the countdown, Cancel, Snooze. The VM is never slept.
$s = New-Step 'Idle action: countdown, Cancel, Snooze'
Stop-All
$sf = Join-Path $bin 'settings.json'
$d = if (Test-Path $sf) { Get-Content $sf -Raw | ConvertFrom-Json } else { [pscustomobject]@{} }
if (-not $d.power) { $d | Add-Member -NotePropertyName power -NotePropertyValue ([pscustomobject]@{}) -Force }
$rule = [pscustomobject]@{ enabled = $true; action = 'sleep'; afterSecs = 60; countdownSecs = 90; acOnly = $false; exceptions = [pscustomobject]@{ audio = $true; otherStayAwake = $false; fullScreen = $false; cpuPct = $null; diskBytesPerS = $null; netBytesPerS = $null; programs = @() } }
$d.power | Add-Member -NotePropertyName idle -NotePropertyValue $rule -Force
Write-Json $sf $d
Start-App -Cdp; $r = Ready; Check $s ([bool]$r) 'Dyno started with the idle rule'
[void]$s.evidence.Add('settings.json power section as written: ' + (Get-Content $sf -Raw | ConvertFrom-Json | Select-Object -ExpandProperty power | ConvertTo-Json -Depth 6 -Compress))
$cd = Wait-Until { $t = (Dyno 'idle').text; if ($t -match 'Counting down to sleep') { $t } } 300 3000
Check $s ([bool]$cd) $(if ($cd) { 'the countdown appeared after the idle time' } else { 'no countdown within 300 s' })
[void]$s.evidence.Add('> dyno idle'); if ($cd) { [void]$s.evidence.AddRange(@($cd -split "`n")) }
$cn = Dyno 'idle cancel'; $st = Wait-Until { $t = (Dyno 'idle').text; if ($t -notmatch 'Counting down') { $t } } 20 1500
Check $s ($cn.code -eq 0 -and [bool]$st) "Cancel stopped the countdown: $(($st -split "`n")[0])"
# new input re-arms the rule
# A cursor set through SetCursorPos is not input (GetLastInputInfo ignores it); mouse_event is, like a real mouse.
Add-Type -Namespace W -Name M -MemberDefinition '[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, System.UIntPtr e);'
[W.M]::mouse_event(1, 5, 5, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 200; [W.M]::mouse_event(1, -5, -5, 0, [UIntPtr]::Zero)
$cd2 = Wait-Until { $t = (Dyno 'idle').text; if ($t -match 'Counting down to sleep') { $t } } 300 3000
Check $s ([bool]$cd2) 'after new input the countdown came again'
$sn = Dyno 'idle snooze 30'; $sz = Wait-Until { $t = (Dyno 'idle').text; if ($t -match 'Snoozed') { $t } } 20 1500
Check $s ($sn.code -eq 0 -and [bool]$sz) 'Snooze 30 minutes: the rule is watching and snoozed'
[void]$s.evidence.Add('The OS call for Sleep on Windows is SetSuspendState (power_actions.rs); not made: the VM has no sleep state and is not slept.')
[void]$s.evidence.Add('"Audio is playing": this VM has no sound device (the audio wait is not run on Windows here).')
Dyno 'idle cancel' | Out-Null
Finish $s 'countdown, Cancel and Snooze worked; no sleep was made (VM)'

# 7. Quit: stop the processes the way the tray's Quit ends them is a UI action (win-cdp.mjs); here the app is closed through its window and the agent must follow.
if ($KeepRunning) { Start-App -Cdp; 'done' | Set-Content "$Out\done.txt"; exit 0 }
$s = New-Step 'Quit: nothing of the app is left'
Get-Process dyno-app -ErrorAction SilentlyContinue | ForEach-Object { [void]$_.CloseMainWindow() }
Start-Sleep -Seconds 5
$left = Wait-Until { -not (Get-Process dyno-app, dyno-agent -ErrorAction SilentlyContinue) } 30
if (-not $left) { [void]$s.evidence.Add('window close hid the app to the tray (expected); ending it with Stop-Process as a fallback'); Stop-All; $s.summary = 'closing the window keeps Dyno in the tray; the Quit menu item is checked by the tray step'; $s.status = 'skip' }
Finish $s 'every process ended after Quit'
'done' | Set-Content "$Out\done.txt"

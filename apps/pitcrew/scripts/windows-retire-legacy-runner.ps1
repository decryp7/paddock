<#
.SYNOPSIS
  Retires a hand-made Gitea runner set-up on Windows so Pitcrew can adopt the runner folder.

.DESCRIPTION
  Finds everything that starts or maintains the runner outside Pitcrew - scheduled tasks and services whose
  command or arguments point into the runner folder (e.g. GiteaRunnerDesktop running run-desktop.ps1, a daily
  update-runner.ps1 task, an old GiteaRunner service) - and the set-up's own scripts in that folder.

  Without -Apply it only reports what it would do. With -Apply it:
    1. refuses while a job is running (the runner has child processes), unless -Force;
    2. disables and deletes those scheduled tasks and stops and deletes those services;
    3. stops the runner process(es) started from the folder;
    4. moves the set-up's scripts (*.ps1, *.cmd, *.bat in the folder itself) to <folder>\retired-<timestamp>.

  It never touches the runner's registration (.runner), configuration (config.yaml), binary, cache, work or logs:
  Pitcrew adopts the folder as it is (Runners > Add runner > An existing runner folder). Run it elevated when a
  task or service was created elevated (it says so). Windows PowerShell 5.1 or PowerShell 7.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File windows-retire-legacy-runner.ps1
  powershell -ExecutionPolicy Bypass -File windows-retire-legacy-runner.ps1 -Apply
#>
[CmdletBinding()]
param(
    [string]$RunnerDir = 'C:\gitea-runner',
    [switch]$Apply,
    [switch]$Force
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

function Say([string]$Text, [string]$Color = 'Gray') { Write-Host $Text -ForegroundColor $Color }

# A task action's text. Actions differ by type (an exec action has Execute and Arguments, a COM handler ClassId and
# Data), and strict mode throws on a property an object lacks, so read only the ones it has.
function ActionText($Action) {
    $props = $Action.PSObject.Properties
    @('Execute', 'Arguments', 'WorkingDirectory', 'ClassId', 'Data' | ForEach-Object {
            if ($props[$_] -and $props[$_].Value) { [string]$props[$_].Value }
        }) -join ' '
}

if (-not (Test-Path -LiteralPath $RunnerDir -PathType Container)) {
    Say "The runner folder $RunnerDir does not exist; nothing to retire." Yellow
    exit 0
}
$dir = (Resolve-Path -LiteralPath $RunnerDir).Path.TrimEnd('\')
$needle = $dir.ToLowerInvariant()
$elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
$mode = if ($Apply) { 'APPLY' } else { 'DRY RUN (add -Apply to make the changes)' }
Say "Retiring the hand-made runner set-up in $dir - $mode" Cyan
if (-not $elevated) { Say '  (not elevated: tasks or services created elevated cannot be removed; rerun as administrator if asked)' DarkYellow }

# --- What starts or maintains the runner -------------------------------------------------------------------------
$tasks = @(Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object {
        $t = $_
        $t.TaskName -like 'GiteaRunner*' -or @($t.Actions | Where-Object {
                (ActionText $_).ToLowerInvariant().Contains($needle)
            }).Count -gt 0
    })
$services = @(Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | Where-Object {
        $_.Name -like 'GiteaRunner*' -or ("$($_.PathName)").ToLowerInvariant().Contains($needle)
    } | Where-Object { $_.Name -notlike 'Pitcrew*' -and ("$($_.PathName)") -notmatch 'pitcrew' })
$runners = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        ("$($_.ExecutablePath)").ToLowerInvariant().StartsWith($needle + '\') -and $_.Name -match 'runner'
    })
$scripts = @(Get-ChildItem -LiteralPath $dir -File -ErrorAction SilentlyContinue | Where-Object { $_.Extension -in '.ps1', '.cmd', '.bat' })

Say ''
Say "Scheduled tasks ($($tasks.Count)):"
foreach ($t in $tasks) { Say "  $($t.TaskPath)$($t.TaskName)  [$($t.State)]  $(@($t.Actions | ForEach-Object { ActionText $_ }) -join '; ')" }
Say "Services ($($services.Count)):"
foreach ($s in $services) { Say "  $($s.Name)  [$($s.State)]  $($s.PathName)" }
Say "Runner processes ($($runners.Count)):"
foreach ($p in $runners) { Say "  pid $($p.ProcessId)  $($p.ExecutablePath)" }
Say "Set-up scripts to move aside ($($scripts.Count)):"
foreach ($f in $scripts) { Say "  $($f.Name)" }

$kept = @('.runner', 'config.yaml') | Where-Object { Test-Path -LiteralPath (Join-Path $dir $_) }
Say "Kept for Pitcrew: $($kept -join ', '), the runner binary, cache, work and logs." Green

if (-not $Apply) {
    Say ''
    Say 'Nothing changed. Run again with -Apply to retire these.' Cyan
    exit 0
}

# --- Refuse while a job runs -------------------------------------------------------------------------------------
$runnerPids = @($runners | ForEach-Object { $_.ProcessId })
$children = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object { $runnerPids -contains $_.ParentProcessId })
if ($children.Count -gt 0 -and -not $Force) {
    Say ''
    Say "A job is running (child processes: $(@($children | ForEach-Object { "$($_.Name) ($($_.ProcessId))" }) -join ', '))." Yellow
    Say 'Wait until it finishes and run this again, or add -Force to stop it anyway.' Yellow
    exit 2
}

$failed = 0
function Try-Step([string]$What, [scriptblock]$Do) {
    try { & $Do; Say "  ok    $What" Green }
    catch { $script:failed++; Say "  fail  $What - $($_.Exception.Message)" Red }
}

Say ''
foreach ($t in $tasks) {
    Try-Step "disable and delete task $($t.TaskName)" {
        Disable-ScheduledTask -TaskName $t.TaskName -TaskPath $t.TaskPath | Out-Null
        Stop-ScheduledTask -TaskName $t.TaskName -TaskPath $t.TaskPath -ErrorAction SilentlyContinue
        Unregister-ScheduledTask -TaskName $t.TaskName -TaskPath $t.TaskPath -Confirm:$false
    }
}
foreach ($s in $services) {
    Try-Step "stop and delete service $($s.Name)" {
        if ($s.State -ne 'Stopped') { Stop-Service -Name $s.Name -Force -ErrorAction Stop }
        $out = & sc.exe delete $s.Name 2>&1
        if ($LASTEXITCODE -ne 0) { throw "sc.exe delete: $out" }
    }
}
foreach ($p in $runners) {
    Try-Step "stop runner pid $($p.ProcessId)" {
        $out = & taskkill.exe /PID $p.ProcessId /T /F 2>&1
        if ($LASTEXITCODE -ne 0 -and (Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue)) { throw "taskkill: $out" }
    }
}
if ($scripts.Count -gt 0) {
    $retired = Join-Path $dir ("retired-" + (Get-Date -Format 'yyyyMMdd-HHmmss'))
    Try-Step "move $($scripts.Count) script(s) to $retired" {
        New-Item -ItemType Directory -Path $retired | Out-Null
        foreach ($f in $scripts) { Move-Item -LiteralPath $f.FullName -Destination $retired }
    }
}

Say ''
if ($failed -gt 0) {
    Say "$failed step(s) failed; see above (run elevated if access was denied), then run this again." Red
    exit 1
}
Say "Done. In Pitcrew: Runners > Add runner > An existing runner folder > $dir (session: logged-on user's desktop)." Cyan
Say 'The retired scripts can be deleted once Pitcrew runs the runner.' Cyan

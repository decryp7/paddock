# Runs INSIDE the interactive session of the Windows test VM (see docs/testing/windows-gui-session.md): a scheduled task
# `/it /ru paddock-test` starts it. It records what a user would see, as a text log and screenshots under C:\paddock-test\probe:
#   A  `dyno-agent run`, started the way the per-user Run entry starts it (a plain CreateProcess from the session): is a console
#      window opened (the visible windows of every process, the process's subsystem is read separately by check-pe-subsystem.mjs)?
#   B  `dyno-app --hidden` (what autostart passes): which tray icons were registered (HKCU\Control Panel\NotifyIconSettings), and the
#      app's own log lines about each icon.
# Every wait polls for its condition with a bounded deadline; nothing sleeps a fixed time to "let it settle".
param([string]$Bin = "$env:LOCALAPPDATA\Dyno", [int]$DeadlineSec = 60)
$ErrorActionPreference = 'Stop'
$out = 'C:\paddock-test\probe'
New-Item -ItemType Directory -Force $out | Out-Null
$log = "$out\probe.txt"
"start $(Get-Date -Format o) bin=$Bin" | Out-File $log
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public class W { public delegate bool CB(IntPtr h, IntPtr l);
 [DllImport("user32.dll")] public static extern bool EnumWindows(CB cb, IntPtr l);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid); }
"@

function Wait-Until([scriptblock]$Condition, [string]$What) {
  $deadline = (Get-Date).AddSeconds($DeadlineSec)
  while (-not (& $Condition)) {
    if ((Get-Date) -gt $deadline) { "TIMEOUT waiting for $What" | Out-File $log -Append; return $false }
    Start-Sleep -Milliseconds 250   # a poll interval, not a settle time
  }
  return $true
}
function Shot($n) {
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bm = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  [System.Drawing.Graphics]::FromImage($bm).CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
  $bm.Save("$out\$n.png")
}
function Visible-Windows($tag) {
  $cb = [W+CB]{ param($h, $l)
    if ([W]::IsWindowVisible($h)) {
      $s = New-Object Text.StringBuilder 256; $c = New-Object Text.StringBuilder 256
      [void][W]::GetWindowText($h, $s, 256); [void][W]::GetClassName($h, $c, 256)
      $p = 0; [void][W]::GetWindowThreadProcessId($h, [ref]$p)
      $name = (Get-Process -Id $p -ErrorAction SilentlyContinue).Name
      "$tag visible: $name class=$($c.ToString()) title=$($s.ToString())" | Out-File $log -Append
    }
    return $true }
  [void][W]::EnumWindows($cb, [IntPtr]::Zero)
}
function Agent-Running { [bool](Get-Process dyno-agent -ErrorAction SilentlyContinue) -and (Select-String -Path "$env:LOCALAPPDATA\Dyno\logs\dyno-agent.*.log" -Pattern 'Agent running' -Quiet) }

Get-Process dyno* -ErrorAction SilentlyContinue | Stop-Process -Force
[void](Wait-Until { -not (Get-Process dyno* -ErrorAction SilentlyContinue) } 'earlier Dyno processes to end')

"A: dyno-agent run, as the Run entry starts it" | Out-File $log -Append
Start-Process "$Bin\dyno-agent.exe" -ArgumentList 'run'
[void](Wait-Until { Agent-Running } 'the agent to report running')
Visible-Windows 'A'; Shot 'A-agent'
Get-Process dyno* -ErrorAction SilentlyContinue | Stop-Process -Force
[void](Wait-Until { -not (Get-Process dyno* -ErrorAction SilentlyContinue) } 'the agent to end')

"B: dyno-app --hidden" | Out-File $log -Append
Start-Process "$Bin\dyno-app.exe" -ArgumentList '--hidden'
[void](Wait-Until { Get-ChildItem 'HKCU:\Control Panel\NotifyIconSettings' -ErrorAction SilentlyContinue | Where-Object { (Get-ItemProperty $_.PSPath).ExecutablePath -like '*dyno-app.exe' } } 'the tray icon to register')
Shot 'B-hidden'
Get-Process dyno* | ForEach-Object { "B process $($_.Name) pid=$($_.Id)" } | Out-File $log -Append
"B notify icons:" | Out-File $log -Append
Get-ChildItem 'HKCU:\Control Panel\NotifyIconSettings' -ErrorAction SilentlyContinue | ForEach-Object {
  $p = Get-ItemProperty $_.PSPath; "  $($p.ExecutablePath) promoted=$($p.IsPromoted) tooltip=$($p.InitialTooltip)" } | Out-File $log -Append
Get-ChildItem "$env:LOCALAPPDATA\Dyno\logs\dyno.*.log" -ErrorAction SilentlyContinue | ForEach-Object { Select-String -Path $_.FullName -Pattern 'tray icon|tray click|Popover' | ForEach-Object { "B log: $($_.Line)" } } | Out-File $log -Append
"done" | Out-File $log -Append

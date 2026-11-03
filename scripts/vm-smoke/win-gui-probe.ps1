# Runs in the interactive session of paddock-test: starts Dyno from its Start menu shortcut and records what appears.
$out = 'C:\paddock-test\probe'
New-Item -ItemType Directory -Force $out | Out-Null
$log = "$out\probe.txt"
"start $(Get-Date -Format o)" | Out-File $log
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
function Shot($n) { $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $bm = New-Object System.Drawing.Bitmap $b.Width,$b.Height; $g=[System.Drawing.Graphics]::FromImage($bm); $g.CopyFromScreen($b.Location,[System.Drawing.Point]::Empty,$b.Size); $bm.Save("$out\$n.png"); }
function Wins($tag) {
  "--- windows $tag" | Out-File $log -Append
  $cb = [W+CB]{ param($h,$l) if ([W]::IsWindowVisible($h)) { $s=New-Object Text.StringBuilder 256; $c=New-Object Text.StringBuilder 256; [void][W]::GetWindowText($h,$s,256); [void][W]::GetClassName($h,$c,256); $p=0; [void][W]::GetWindowThreadProcessId($h,[ref]$p); $pn=(Get-Process -Id $p -EA SilentlyContinue).Name; if ($pn -like 'dyno*' -or $pn -eq 'conhost' -or $pn -eq 'cmd' -or $pn -like 'Windows*Terminal*') { "$pn pid=$p class=$($c.ToString()) title=$($s.ToString())" | Out-File $log -Append } }; return $true }
  [void][W]::EnumWindows($cb,[IntPtr]::Zero)
  "--- processes" | Out-File $log -Append
  Get-Process | ? { $_.Name -like 'dyno*' -or $_.Name -eq 'conhost' } | % { "$($_.Name) pid=$($_.Id) session=$($_.SessionId)" } | Out-File $log -Append
}
$out = 'C:\paddock-test\probe2'
New-Item -ItemType Directory -Force $out | Out-Null
$log = "$out\probe.txt"; "start $(Get-Date -Format o)" | Out-File $log
Get-Process dyno* -EA SilentlyContinue | Stop-Process -Force
Start-Sleep 3
$bin = "$env:LOCALAPPDATA\Dyno"
# A: what the per-user Run entry does: explorer starts the registered command line (no flags, no console).
"A: dyno-agent run, as the Run entry starts it" | Out-File $log -Append
Start-Process "$bin\dyno-agent.exe" -ArgumentList 'run'
Start-Sleep 8
Shot 'A-agent'
$cb = [W+CB]{ param($h,$l) if ([W]::IsWindowVisible($h)) { $s=New-Object Text.StringBuilder 256; $c=New-Object Text.StringBuilder 256; [void][W]::GetWindowText($h,$s,256); [void][W]::GetClassName($h,$c,256); $p=0; [void][W]::GetWindowThreadProcessId($h,[ref]$p); $pn=(Get-Process -Id $p -EA SilentlyContinue).Name; "A visible: $pn class=$($c.ToString()) title=$($s.ToString())" | Out-File $log -Append }; return $true }
[void][W]::EnumWindows($cb,[IntPtr]::Zero)
Get-Process dyno* -EA SilentlyContinue | Stop-Process -Force
Start-Sleep 3
# B: the app started hidden (the autostart entry): tray icons?
"B: dyno-app --hidden" | Out-File $log -Append
Start-Process "$bin\dyno-app.exe" -ArgumentList '--hidden'
Start-Sleep 20
Shot 'B-hidden'
Get-Process dyno* | % { "B proc $($_.Name) pid=$($_.Id)" } | Out-File $log -Append
"B notify icons:" | Out-File $log -Append
Get-ChildItem 'HKCU:\Control Panel\NotifyIconSettings' -EA SilentlyContinue | % { $p = Get-ItemProperty $_.PSPath; "  $($p.ExecutablePath) promoted=$($p.IsPromoted) tooltip=$($p.InitialTooltip)" } | Out-File $log -Append
Get-ChildItem "$env:APPDATA\net.decryptology.dyno*", "$env:LOCALAPPDATA\net.decryptology.dyno*" -Recurse -Filter *.log -EA SilentlyContinue | % { "logfile $($_.FullName)" } | Out-File $log -Append
"done" | Out-File $log -Append

# The in-session actor of the Windows test VM: a loop (scheduled task `/it /ru paddock-test`) that executes one input or window command per
# request file and writes the answer, so the host can drive native input while it reads the app through the DevTools port. It is the
# `SendInput` side of win-cdp.mjs. Commands (one line in act-<n>.txt in -Dir): key <VK+VK...> | move X Y | click X Y | fg <title regex> |
# rect <title regex> | shot <name> | windows | trayicons <name regex> (button, centre, class, top-level class, top-level process) | quit. Answers go to act-<n>.resp. Every wait is bounded by -MaxMin.
param([string]$Dir = 'C:\paddock-test\vmcheck\act', [int]$MaxMin = 40)
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force $Dir | Out-Null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
Add-Type -Namespace W -Name A -MemberDefinition '
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, System.UIntPtr e);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, System.UIntPtr e);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool SetForegroundWindow(System.IntPtr h);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr h, int n);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool GetWindowRect(System.IntPtr h, out RECT r);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool EnumWindows(EnumCb cb, System.IntPtr l);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr h);
[System.Runtime.InteropServices.DllImport("user32.dll", CharSet=System.Runtime.InteropServices.CharSet.Unicode)] public static extern int GetWindowText(System.IntPtr h, System.Text.StringBuilder s, int n);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr h, out uint pid);
[System.Runtime.InteropServices.DllImport("user32.dll", CharSet=System.Runtime.InteropServices.CharSet.Unicode)] public static extern int GetClassName(System.IntPtr h, System.Text.StringBuilder s, int n);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern System.IntPtr SendMessage(System.IntPtr h, uint m, System.IntPtr w, System.IntPtr l);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern int GetMenuItemCount(System.IntPtr m);
[System.Runtime.InteropServices.DllImport("user32.dll", CharSet=System.Runtime.InteropServices.CharSet.Unicode)] public static extern int GetMenuString(System.IntPtr m, uint pos, System.Text.StringBuilder s, int n, uint flags);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool GetMenuItemRect(System.IntPtr h, System.IntPtr m, uint pos, out RECT r);
[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern System.IntPtr GetSubMenu(System.IntPtr m, int pos);
public delegate bool EnumCb(System.IntPtr h, System.IntPtr l);
[System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }'
[void][W.A]::SetProcessDPIAware()
$vk = @{ LWIN = 0x5B; LEFT = 0x25; RIGHT = 0x27; UP = 0x26; DOWN = 0x28; ALT = 0x12; CTRL = 0x11; SHIFT = 0x10; TAB = 0x09; ENTER = 0x0D; ESC = 0x1B; Y = 0x59; N = 0x4E; SPACE = 0x20; COMMA = 0xBC }
function Windows-List {
  $rows = New-Object System.Collections.ArrayList
  $cb = [W.A+EnumCb]{ param($h, $l)
    if ([W.A]::IsWindowVisible($h)) {
      $sb = New-Object System.Text.StringBuilder 256; [void][W.A]::GetWindowText($h, $sb, 256)
      $p = [uint32]0; [void][W.A]::GetWindowThreadProcessId($h, [ref]$p)
      if ($sb.Length -gt 0) { $r = New-Object W.A+RECT; [void][W.A]::GetWindowRect($h, [ref]$r); [void]$rows.Add("$($h.ToInt64())`t$((Get-Process -Id $p -ErrorAction SilentlyContinue).ProcessName)`t$($sb.ToString())`t$($r.L),$($r.T),$($r.R - $r.L)x$($r.B - $r.T)") }
    }
    return $true }
  [void][W.A]::EnumWindows($cb, [IntPtr]::Zero)
  return $rows
}
# The visible native popup menus (window class #32768, what TrackPopupMenu makes): one row per menu, "handle<TAB>owner process<TAB>L,T,R,B". Titles are not needed, so this sees a menu
# that Windows-List (titled windows only) and the UI Automation MenuItem search do not.
function Menu-Windows {
  $rows = New-Object System.Collections.ArrayList
  $cb = [W.A+EnumCb]{ param($h, $l)
    if ([W.A]::IsWindowVisible($h)) {
      $sb = New-Object System.Text.StringBuilder 64; [void][W.A]::GetClassName($h, $sb, 64)
      if ($sb.ToString() -eq '#32768') { $p = [uint32]0; [void][W.A]::GetWindowThreadProcessId($h, [ref]$p); $r = New-Object W.A+RECT; [void][W.A]::GetWindowRect($h, [ref]$r); [void]$rows.Add("$($h.ToInt64())`t$((Get-Process -Id $p -ErrorAction SilentlyContinue).ProcessName)`t$($r.L),$($r.T),$($r.R),$($r.B)") }
    }
    return $true }
  [void][W.A]::EnumWindows($cb, [IntPtr]::Zero)
  return $rows
}
# The items of the native menus that are open. Read per menu window (AutomationElement.FromHandle of each #32768 window, its MenuItem descendants); when that finds nothing, the
# whole-desktop MenuItem search of the older probe is tried as well. Rows are "name<TAB>cx,cy".
# One menu's items by position; a submenu is read without opening it (GetSubMenu) and its items are named "Parent > Item" (no coordinates: 0,0).
function Read-Menu($h, $hm, $prefix, $depth) {
  $out = @(); $cnt = [W.A]::GetMenuItemCount($hm)
  for ($i = 0; $i -lt $cnt; $i++) {
    $sb = New-Object System.Text.StringBuilder 128; [void][W.A]::GetMenuString($hm, [uint32]$i, $sb, 128, 0x400); $nm = ($sb.ToString() -replace '&', '')
    $r = New-Object W.A+RECT; [void][W.A]::GetMenuItemRect($h, $hm, [uint32]$i, [ref]$r)
    $at = if ($prefix) { '0,0' } else { "$([int](($r.L + $r.R) / 2)),$([int](($r.T + $r.B) / 2))" }
    if ($nm) { $out += "$prefix$nm`t$at" }
    $sub = [W.A]::GetSubMenu($hm, $i)
    if ($sub -ne [IntPtr]::Zero -and $depth -lt 2) { $out += Read-Menu $h $sub "$prefix$nm > " ($depth + 1) }
  }
  return $out
}
function Menu-Items {
  $rows = @(); $seenNames = @{}
  # First reader: the menu's own items through Win32 (MN_GETHMENU on the #32768 window, then GetMenuString and GetMenuItemRect per item); UI Automation read none in 0ad round 4.
  foreach ($w in @(Menu-Windows)) {
    $h = [IntPtr][int64](($w -split "`t")[0])
    try {
      $hm = [W.A]::SendMessage($h, 0x1E1, [IntPtr]::Zero, [IntPtr]::Zero)
      if ($hm -ne [IntPtr]::Zero) { $rows += Read-Menu $h $hm '' 0 }
    } catch { }
  }
  if ($rows.Count -gt 0) { return ($rows -join "`n") }
  foreach ($w in @(Menu-Windows)) {
    $h = ($w -split "`t")[0]
    try {
      $el = [Windows.Automation.AutomationElement]::FromHandle([IntPtr][int64]$h)
      $all = $el.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
      foreach ($e in $all) { $r = $e.Current.BoundingRectangle; if ($e.Current.Name -and $e.Current.ControlType.ProgrammaticName -match 'MenuItem' -and $r.Width -gt 0) { $k = "$($e.Current.Name)@$([int]$r.Y)"; if (-not $seenNames[$k]) { $seenNames[$k] = 1; $rows += "$($e.Current.Name)`t$([int]($r.X + $r.Width / 2)),$([int]($r.Y + $r.Height / 2))" } } }
    } catch { }
  }
  if ($rows.Count -eq 0) {
    $root = [Windows.Automation.AutomationElement]::RootElement; $all = $root.FindAll([Windows.Automation.TreeScope]::Descendants, (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::MenuItem)))
    foreach ($e in $all) { $r = $e.Current.BoundingRectangle; if ($r.Width -gt 0 -and $e.Current.Name -and $e.Current.FrameworkId -ne 'Chrome') { $rows += "$($e.Current.Name)`t$([int]($r.X + $r.Width / 2)),$([int]($r.Y + $r.Height / 2))" } }
  }
  return ($rows -join "`n")
}
function Shot-To($name) { $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $bm = New-Object System.Drawing.Bitmap $b.Width, $b.Height; $g = [System.Drawing.Graphics]::FromImage($bm); $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size); $bm.Save("$Dir\$name.png") }
$end = (Get-Date).AddMinutes($MaxMin); $n = 0
while ((Get-Date) -lt $end) {
  $n++
  $req = "$Dir\act-$n.txt"
  while (-not (Test-Path $req) -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 300 }
  if (-not (Test-Path $req)) { break }
  Start-Sleep -Milliseconds 100
  $line = (Get-Content $req -Raw).Trim(); $a = $line -split '\s+', 2; $out = 'ok'
  try {
    switch ($a[0]) {
      'key' { $keys = $a[1].Split('+') | ForEach-Object { [byte]$vk[$_.ToUpper()] }; foreach ($k in $keys) { [W.A]::keybd_event($k, 0, 0, [UIntPtr]::Zero) }; [array]::Reverse($keys); foreach ($k in $keys) { [W.A]::keybd_event($k, 0, 2, [UIntPtr]::Zero) } }
      'rclick' { $p = $a[1].Split(' '); [void][W.A]::SetCursorPos([int]$p[0], [int]$p[1]); Start-Sleep -Milliseconds 250; [W.A]::mouse_event(8, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 90; [W.A]::mouse_event(16, 0, 0, 0, [UIntPtr]::Zero) }
      'tray' { $root = [Windows.Automation.AutomationElement]::RootElement; $all = $root.FindAll([Windows.Automation.TreeScope]::Descendants, (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Button))); $rows = @(); foreach ($e in $all) { $r = $e.Current.BoundingRectangle; if ($e.Current.Name -match $a[1] -and $r.Width -gt 0 -and $e.Current.Name -notmatch 'running window|Expand ') { $rows += "$($e.Current.Name)`t$([int]($r.X + $r.Width / 2)),$([int]($r.Y + $r.Height / 2))" } }; $out = $rows -join "`n" }
      'menu' { $out = (Menu-Items) }
      'move' { $p = $a[1].Split(' '); [void][W.A]::SetCursorPos([int]$p[0], [int]$p[1]) }
      'click' { $p = $a[1].Split(' '); [void][W.A]::SetCursorPos([int]$p[0], [int]$p[1]); Start-Sleep -Milliseconds 200; [W.A]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [W.A]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero) }
      'fg' { $h = (Windows-List | Where-Object { ($_ -split "`t")[2] -match $a[1] } | Select-Object -First 1); if ($h) { $hh = [IntPtr][int64](($h -split "`t")[0]); [void][W.A]::ShowWindow($hh, 9); [void][W.A]::SetForegroundWindow($hh); $out = "fg $(($h -split "`t")[2])" } else { $out = 'no such window' } }
      'rect' { $h = (Windows-List | Where-Object { ($_ -split "`t")[2] -match $a[1] } | Select-Object -First 1); $out = $(if ($h) { ($h -split "`t")[3] } else { 'no such window' }) }
      'trayicons' { $root = [Windows.Automation.AutomationElement]::RootElement; $walk = [Windows.Automation.TreeWalker]::ControlViewWalker; $all = $root.FindAll([Windows.Automation.TreeScope]::Descendants, (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Button))); $rows = @(); foreach ($e in $all) { $r = $e.Current.BoundingRectangle; if ($e.Current.Name -match $a[1] -and $r.Width -gt 0) { $top = $e; $up = $walk.GetParent($top); while ($up -ne $null -and -not [Windows.Automation.Automation]::Compare($up, $root)) { $top = $up; $up = $walk.GetParent($top) }; $proc = (Get-Process -Id $top.Current.ProcessId -ErrorAction SilentlyContinue).ProcessName; $rows += "$($e.Current.Name)`t$([int]($r.X + $r.Width / 2)),$([int]($r.Y + $r.Height / 2))`t$($e.Current.ClassName)`t$($top.Current.ClassName)`t$proc" } }; $out = $rows -join "`n" }
      'rclickwatch' { # right click at X Y, then look for a native menu every 100 ms for 6 s (guest side: no round trip between the click and the look); screenshot and items at first sight
        $p = $a[1].Split(' '); [void][W.A]::SetCursorPos([int]$p[0], [int]$p[1]); Start-Sleep -Milliseconds 250; [W.A]::mouse_event(8, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 90; [W.A]::mouse_event(16, 0, 0, 0, [UIntPtr]::Zero)
        $sw = [Diagnostics.Stopwatch]::StartNew(); $seen = @(); while ($sw.ElapsedMilliseconds -lt 6000 -and $seen.Count -eq 0) { Start-Sleep -Milliseconds 100; $seen = @(Menu-Windows) }
        if ($seen.Count -gt 0) { $t = $sw.ElapsedMilliseconds; $items = ''; for ($k = 0; $k -lt 8 -and -not $items; $k++) { Start-Sleep -Milliseconds 250; $items = Menu-Items }; Shot-To 'rcw-open'; $out = "SEEN $t`n--windows`n$($seen -join "`n")`n--items`n$items" } else { Shot-To 'rcw-none'; $out = "NONE 6000" } }
      'menuwin' { $out = (Menu-Windows) -join "`n" }
      'windows' { $out = (Windows-List) -join "`n" }
      'shot' { $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $bm = New-Object System.Drawing.Bitmap $b.Width, $b.Height; $g = [System.Drawing.Graphics]::FromImage($bm); $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size); $bm.Save("$Dir\$($a[1]).png"); $out = "shot $($a[1])" }
      'quit' { $out = 'bye'; Set-Content "$Dir\act-$n.resp" $out; exit 0 }
      default { $out = "unknown command: $line" }
    }
  } catch { $out = "error: $($_.Exception.Message)" }
  Set-Content "$Dir\act-$n.resp" $out
}

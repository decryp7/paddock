# WP-0 guest-side probes for Dyno ADR 0033. Run inside paddock-win11-test ONLY (the host driver wp0-run.sh checks). Reads; the only
# things it creates are its own DynoTest-<uuid> objects (removed in finally) and, for -Step tasks-register, a disabled task.
param([Parameter(Mandatory)][ValidateSet('env','approved-dump','appx-keys','physdisk','rm-probe','tasks-walk','tasks-register','power-overlay','background-apps')][string]$Step,
      [string]$Path = '', [int]$Expect = 1)
$ErrorActionPreference = 'Stop'
function Control($ok, $what) { if ($ok) { "CONTROL ok: $what" } else { "CONTROL BROKEN: $what" } }
function Hex($b) { if ($null -eq $b) { '<none>' } else { ($b | ForEach-Object { $_.ToString('x2') }) -join ' ' } }
switch ($Step) {
  'env' {
    "user=$(whoami) admin=$(([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole('Administrators'))"
    $cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    "build=$($cv.CurrentBuildNumber).$($cv.UBR) display=$($cv.DisplayVersion) product=$($cv.ProductName)"
    "branch=$((Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -Name BranchName -ErrorAction SilentlyContinue).BranchName)"
    Control (Test-Path 'C:\Windows') 'C:\Windows exists'
  }
  'approved-dump' {
    $roots = @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved', 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved')
    $n = 0
    foreach ($r in $roots) { foreach ($k in 'Run', 'Run32', 'StartupFolder') {
      $p = Join-Path $r $k
      if (-not (Test-Path $p)) { "$p <absent>"; continue }
      $key = Get-Item $p
      foreach ($name in $key.GetValueNames()) { $n++; "$p | $name | $(Hex $key.GetValue($name))" }
    } }
    "values=$n"
    # Control: the Run values of HKCU (Dyno's own agent entry in a VM that ran it) or any value; absent everywhere means the probe saw nothing.
    Control ($n -gt 0) 'at least one StartupApproved value was read (seed one entry and turn it off in Task Manager first)'
  }
  'appx-keys' {
    $base = 'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData'
    if (Test-Path $base) {
      Get-ChildItem $base | ForEach-Object { $fam = $_.PSChildName
        Get-ChildItem $_.PSPath -ErrorAction SilentlyContinue | ForEach-Object { $t = $_.PSChildName; $st = (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).State
          "SystemAppData | $fam | $t | State=$st" } }
    } else { 'SystemAppData <absent>' }
    $manifests = 0
    Get-AppxPackage | ForEach-Object { $loc = $_.InstallLocation; if (-not $loc) { return }
      $mf = Join-Path $loc 'AppxManifest.xml'; if (-not (Test-Path $mf)) { return }
      $manifests++
      [xml]$x = Get-Content $mf -Raw
      $ns = New-Object Xml.XmlNamespaceManager($x.NameTable)
      foreach ($st in $x.SelectNodes('//*[local-name()="StartupTask"]')) { "manifest | $($_.PackageFamilyName) | $($st.TaskId) | Enabled=$($st.Enabled) | Executable=$($st.Executable)" } }
    Control ($manifests -gt 0) "package manifests read ($manifests)"
  }
  'physdisk' {
    $ms = @(); $disks = $null
    1..10 | ForEach-Object { $sw = [Diagnostics.Stopwatch]::StartNew()
      try { $disks = Get-CimInstance -Namespace 'root\Microsoft\Windows\Storage' -ClassName MSFT_PhysicalDisk } catch { "ERROR $($_.Exception.Message)"; break }
      $sw.Stop(); $ms += $sw.ElapsedMilliseconds }
    foreach ($d in $disks) { "disk | $($d.FriendlyName) | HealthStatus=$($d.HealthStatus) | OperationalStatus=$($d.OperationalStatus)" }
    if ($ms.Count) { $s = $ms | Sort-Object; "MS median=$($s[[int]($s.Count / 2)]) all=$($ms -join ',')" }
    Control (@($disks).Count -gt 0) 'MSFT_PhysicalDisk returned a row (as this user)'
  }
  'rm-probe' {
    if (-not $Path) { throw 'pass -Path <a file held open by some process>' }
    if ($Path -notmatch ':') { $Path = Join-Path 'C:\Users\Public' $Path }
    Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class Rm {
  [StructLayout(LayoutKind.Sequential)] public struct UID { public uint Pid; public System.Runtime.InteropServices.ComTypes.FILETIME Start; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct INFO { public UID P; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string App; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string Svc; public int Type; public uint Status; public uint TsSession; [MarshalAs(UnmanagedType.Bool)] public bool Restartable; }
  [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)] public static extern int RmStartSession(out uint h, int f, StringBuilder key);
  [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)] public static extern int RmRegisterResources(uint h, uint nf, string[] files, uint na, IntPtr a, uint ns, string[] s);
  [DllImport("rstrtmgr.dll")] public static extern int RmGetList(uint h, out uint needed, ref uint have, [In, Out] INFO[] info, out uint reasons);
  [DllImport("rstrtmgr.dll")] public static extern int RmEndSession(uint h);
}
'@
    $h = 0; $key = New-Object Text.StringBuilder 33
    "start=$([Rm]::RmStartSession([ref]$h, 0, $key))"
    try {
      "register=$([Rm]::RmRegisterResources($h, 1, @($Path), 0, [IntPtr]::Zero, 0, $null))"
      $needed = 0; $have = 0; $reasons = 0
      $rc = [Rm]::RmGetList($h, [ref]$needed, [ref]$have, $null, [ref]$reasons)
      "getlist1 rc=$rc needed=$needed"
      if ($rc -eq 234 -and $needed -gt 0) {
        $have = $needed; $info = New-Object 'Rm+INFO[]' $needed
        $rc = [Rm]::RmGetList($h, [ref]$needed, [ref]$have, $info, [ref]$reasons)
        "getlist2 rc=$rc count=$have"
        $info[0..($have - 1)] | ForEach-Object { "holder | pid=$($_.P.Pid) | app='$($_.App)' | type=$($_.Type) | session=$($_.TsSession)" }
      }
    } finally { [void][Rm]::RmEndSession($h) }
    if ($Expect -gt 0) { Control ($have -gt 0) "a file known to be held lists at least one holder (-Expect $Expect)" } else { Control ($true) 'no holder expected' }
  }
  'tasks-walk' {
    $sw = [Diagnostics.Stopwatch]::StartNew(); $svc = New-Object -ComObject 'Schedule.Service'; $svc.Connect()
    $script:count = 0; $script:folders = 0; $script:denied = @(); $script:hidden = 0
    function Walk($f, $depth) { if ($depth -gt 16) { return }
      $script:folders++
      try { $tasks = $f.GetTasks(1) } catch { $script:denied += "$($f.Path) | $($_.Exception.HResult.ToString('x8'))"; $tasks = @() }
      foreach ($t in $tasks) { $script:count++; if ($t.Definition.Settings.Hidden) { $script:hidden++ } }
      try { $subs = $f.GetFolders(0) } catch { $script:denied += "$($f.Path)\* | $($_.Exception.HResult.ToString('x8'))"; $subs = @() }
      foreach ($s in $subs) { Walk $s ($depth + 1) } }
    Walk $svc.GetFolder('\') 0
    $sw.Stop()
    "COUNT=$script:count FOLDERS=$script:folders HIDDEN=$script:hidden DENIED=$($script:denied.Count) MS=$($sw.ElapsedMilliseconds) user=$(whoami)"
    $script:denied | ForEach-Object { "denied | $_" }
    Control ($script:count -gt 0) 'the walk saw tasks (Windows ships dozens)'
  }
  'tasks-register' {
    $id = "DynoTest-$([guid]::NewGuid())"; $svc = New-Object -ComObject 'Schedule.Service'; $svc.Connect()
    try {
      $root = $svc.GetFolder('\')
      try { $f = $root.CreateFolder($id); "create-folder: ok"; $where = $f } catch { "create-folder: DENIED $($_.Exception.HResult.ToString('x8'))"; $where = $root }
      $def = $svc.NewTask(0); $def.Settings.Enabled = $false
      $trig = $def.Triggers.Create(9); $trig.Enabled = $false      # TASK_TRIGGER_LOGON
      $act = $def.Actions.Create(0); $act.Path = "$env:TEMP\$id\does-not-exist.exe"
      try { [void]$where.RegisterTaskDefinition("$id-task", $def, 6, $null, $null, 3); "register-logon-task: ok" } catch { "register-logon-task: DENIED $($_.Exception.HResult.ToString('x8'))" }
    } finally {
      try { $svc.GetFolder('\').DeleteTask("$id-task", 0) } catch {}
      try { $svc.GetFolder("\$id").DeleteTask("$id-task", 0) } catch {}
      try { $svc.GetFolder('\').DeleteFolder($id, 0) } catch {}
    }
    Control ((Get-ScheduledTask -TaskName "$id-task" -ErrorAction SilentlyContinue) -eq $null) 'nothing of ours is left'
  }
  'power-overlay' {
    Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Pw {
  [DllImport("powrprof.dll")] public static extern uint PowerGetEffectiveOverlayScheme(out Guid g);
  [DllImport("powrprof.dll")] public static extern uint PowerGetActualOverlayScheme(out Guid g);
  [DllImport("powrprof.dll")] public static extern uint PowerGetActiveScheme(IntPtr root, out IntPtr g);
}
'@
    $e = [Guid]::Empty; $a = [Guid]::Empty
    "effective rc=$([Pw]::PowerGetEffectiveOverlayScheme([ref]$e)) guid=$e"
    "actual    rc=$([Pw]::PowerGetActualOverlayScheme([ref]$a)) guid=$a"
    $p = [IntPtr]::Zero; "active rc=$([Pw]::PowerGetActiveScheme([IntPtr]::Zero, [ref]$p)) guid=$([Runtime.InteropServices.Marshal]::PtrToStructure($p, [Guid]))"
    Control ($e -ne [Guid]::Empty -or $a -ne [Guid]::Empty) 'an overlay GUID (all zeros = Balanced default) was returned; set each Settings > Power mode and run again'
  }
  'background-apps' {
    $p = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications'
    if (Test-Path $p) {
      (Get-ItemProperty $p).PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' } | ForEach-Object { "value | $($_.Name) | $($_.Value)" }
      Get-ChildItem $p | ForEach-Object { "app | $($_.PSChildName) | Disabled=$((Get-ItemProperty $_.PSPath).Disabled) | DisabledByUser=$((Get-ItemProperty $_.PSPath).DisabledByUser)" }
    } else { "$p <absent>" }
    Control (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion') 'HKCU CurrentVersion readable'
  }
}

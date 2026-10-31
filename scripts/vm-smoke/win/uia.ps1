# UI Automation helpers for the in-session jobs of the Windows update test (dot-source this file at the top of a job). They run on the
# desktop of paddock-test (tools/vm-win.sh job), the Windows counterpart of axdrive on macOS. Every wait is a poll on state with a guard.
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$script:AE = [System.Windows.Automation.AutomationElement]
$script:Scope = [System.Windows.Automation.TreeScope]
$script:CT = [System.Windows.Automation.ControlType]

function Wait-Until([scriptblock]$Test, [int]$Seconds = 60) {
  $end = (Get-Date).AddSeconds($Seconds)
  while ((Get-Date) -lt $end) { $v = & $Test; if ($v) { return $v }; Start-Sleep -Milliseconds 500 }
  return $null
}

# Top-level windows of a process (by process name without .exe), as elements.
function Get-AppWindows([string]$Process) {
  $pids = @(Get-Process -Name $Process -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  if (-not $pids) { return @() }
  $kids = $script:AE::RootElement.FindAll($script:Scope::Children, [System.Windows.Automation.Condition]::TrueCondition)
  @($kids | Where-Object { $pids -contains $_.Current.ProcessId })
}

# All visible text (Name of text-like controls) under an element, in document order.
function Get-Texts($Element) {
  $all = $Element.FindAll($script:Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  @($all | Where-Object { $_.Current.ControlType -in @($script:CT::Text, $script:CT::Document) -or $_.Current.LocalizedControlType -match 'text|heading|paragraph' } |
    ForEach-Object { $_.Current.Name } | Where-Object { $_ })
}

# Finds a descendant by Name regex and control type name (Button, MenuItem, ...); $null if absent.
function Find-Element($Element, [string]$NameRegex, [string]$Type = 'Button') {
  $all = $Element.FindAll($script:Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  foreach ($e in $all) {
    if ($e.Current.Name -notmatch $NameRegex) { continue }
    if ($Type -eq 'Any') { $p = $null; if ($e.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$p)) { return $e } }
    elseif ($e.Current.ControlType.ProgrammaticName -eq "ControlType.$Type") { return $e }
  }
  return $null
}

# Presses a button / menu item through its Invoke (or Toggle / ExpandCollapse) pattern.
function Invoke-Element($E) {
  $p = $null
  if ($E.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$p)) { $p.Invoke(); return $true }
  if ($E.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$p)) { $p.Expand(); return $true }
  $E.SetFocus(); [System.Windows.Forms.SendKeys]::SendWait('{ENTER}'); return $true
}

function Show-Tree($Element, [int]$Depth = 4) {
  function Walk($e, $d) {
    if ($d -gt $Depth) { return }
    ('  ' * $d) + "$($e.Current.ControlType.ProgrammaticName) '$($e.Current.Name)'"
    $c = $e.FindAll($script:Scope::Children, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($k in $c) { Walk $k ($d + 1) }
  }
  Walk $Element 0
}

# A screenshot of the primary screen to a PNG (the evidence of a step).
function Save-Shot([string]$Path) {
  Add-Type -AssemblyName System.Windows.Forms, System.Drawing
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
  $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}

# Closes a window through its WindowPattern (the Updates window keeps its last result while it is open).
function Close-Window($W) {
  $p = $null
  if ($W.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$p)) { $p.Close(); return $true }
  return $false
}

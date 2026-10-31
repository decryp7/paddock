# One case of the Windows update test, run ON the desktop of paddock-test by `vm-win.sh job` (update-win.mjs prepends
# `$CaseId`, `$Old`, `$New` and dot-sources uia.ps1 first). Prints `CHECK ok|FAIL <detail>` lines; exit 1 when any FAIL.
$ErrorActionPreference = 'Stop'
$script:fail = 0
function Check([bool]$ok, [string]$detail) { if ($ok) { "CHECK ok $detail" } else { $script:fail++; "CHECK FAIL $detail" } }
$exe = "$env:LOCALAPPDATA\Spotter\spotter-app.exe"
function Installed-Version { (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*' | ? DisplayName -eq 'Spotter' | select -First 1).DisplayVersion }
function Updates-Windows { @(Get-AppWindows 'spotter-app' | ? { (Get-Texts $_) -contains 'Check for updates automatically' }) }
function Main-Window { Get-AppWindows 'spotter-app' | ? { $_.Current.Name -eq 'Spotter' } | select -First 1 }
function All-Texts { $t = @(); foreach ($w in (Get-AppWindows 'spotter-app')) { $t += Get-Texts $w }; $t }
function Update-State([string]$new) {
  $t = (All-Texts) -join "`n"; $v = [regex]::Escape($new)
  if ($t -match 'The last step did not finish\.') { return 'failed' }
  if ($t -match "Version $v is downloaded and its signature verified") { return 'ready' }
  if ($t -match "Installing version $v") { return 'installing' }
  if ($t -match "Downloading version $v") { return 'downloading' }
  if ($t -match "Version $v is available") { return 'available' }
  if ($t -match 'You have the latest version\.') { return 'upToDate' }
  if ($t -match 'Checking for updates|Asking the updater') { return 'checking' }
  return 'unknown'
}
function Wait-State([string[]]$want, [int]$secs = 180) {
  $last = 'unknown'
  $r = Wait-Until { $script:last = Update-State $New; if ($want -contains $script:last) { $script:last } } $secs
  if ($r) { $r } else { "timeout($script:last)" }
}
function Press-Any([string]$re) {
  $ok = Wait-Until { foreach ($w in (Get-AppWindows 'spotter-app')) { $b = Find-Element $w $re 'Any'; if ($b) { Invoke-Element $b | Out-Null; return $true } } } 60
  [bool]$ok
}
$before = Installed-Version
$pidBefore = (Get-Process spotter-app -ErrorAction SilentlyContinue | select -First 1).Id
if (-not $pidBefore) {
  # The webview exposes its page to UI Automation only when asked: a test-only browser argument, never set for users.
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--force-renderer-accessibility'
  Start-Process $exe
  $pidBefore = (Wait-Until { (Get-Process spotter-app -ErrorAction SilentlyContinue | select -First 1).Id } 30)
}
$main = Wait-Until { $w = Main-Window; if ($w -and (Find-Element $w '^Help$' 'MenuItem')) { $w } } 90
Check ([bool]$main) 'the main window is up with its Application menu'
if (-not $main) { Save-Shot "C:\paddock-test\$CaseId-nowindow.png"; exit 1 }
# Help > Check for updates
$help = Find-Element $main '^Help$' 'MenuItem'; Invoke-Element $help | Out-Null
$item = Wait-Until { Find-Element (Main-Window) '^Check for updates' 'MenuItem' } 20
if (-not $item) { $item = Wait-Until { foreach ($w in (Get-AppWindows 'spotter-app')) { $e = Find-Element $w '^Check for updates' 'MenuItem'; if ($e) { $e } } } 10 }
Check ([bool]$item) 'Help > Check for updates found'
if (-not $item) { Save-Shot "C:\paddock-test\$CaseId-nomenu.png"; exit 1 }
Invoke-Element $item | Out-Null
$opened = Wait-Until { (All-Texts) -match 'Not checked yet|Checking for updates|You have|is available|did not finish' } 30
Check ([bool]$opened) 'the Updates window shows'
[void](Press-Any '^(Check (now|again)|Try again)')
if ($CaseId -eq 'down') {
  $s = Wait-State @('failed', 'upToDate', 'available') 60
  Save-Shot "C:\paddock-test\$CaseId-result.png"
  Check ($s -eq 'failed') "window: $s (wanted a failed check)"
  Check ((Get-Process spotter-app -ErrorAction SilentlyContinue | select -First 1).Id -eq $pidBefore) 'the app is still running, the same process'
  Check ((Installed-Version) -eq $Old) "installed version still $Old"
  "TEXTS " + ((All-Texts | select -Unique | select -First 14) -join ' | ')
  exit $script:fail
}
$s = Wait-State @('available') 120
Save-Shot "C:\paddock-test\$CaseId-available.png"
Check ($s -eq 'available') "window: $s (wanted `"Version $New is available`")"
if ($s -ne 'available') { foreach ($w in (Updates-Windows)) { "TREE window '$($w.Current.Name)' class=$($w.Current.ClassName)"; Show-Tree $w 16 | ? { $_ -match "'.+'" } | select -First 45 | % { "TREE $_" } }; "TEXTS " + ((All-Texts | select -Unique | select -First 14) -join ' | '); exit 1 }
Check (Press-Any '^Download') 'pressed Download'
if ($CaseId -eq 'ok') {
  $s = Wait-State @('ready') 240
  Save-Shot "C:\paddock-test\$CaseId-after-download.png"
  Check ($s -eq 'ready') "window: $s (wanted downloaded and its signature verified)"
  if ($s -ne 'ready') { "TEXTS " + ((All-Texts | select -Unique | select -First 14) -join ' | '); exit 1 }
  Check (Press-Any '^Restart to update') 'pressed Restart to update'
  $back = Wait-Until { $v = Installed-Version; $p = (Get-Process spotter-app -ErrorAction SilentlyContinue | select -First 1).Id; if ($v -eq $New -and $p -and $p -ne $pidBefore -and (Main-Window)) { "$v/$p" } } 240
  Save-Shot "C:\paddock-test\$CaseId-relaunched.png"
  Check ([bool]$back) "relaunched on $New (installed: $(Installed-Version), $back)"
  Check (@(Get-Process spotter-app -ErrorAction SilentlyContinue).Count -eq 1) 'one app process (single instance)'
  $sig = Get-AuthenticodeSignature $exe
  "NOTE authenticode: $($sig.Status) (test builds are unsigned)"
} else {
  $s = Wait-State @('failed') 120
  Save-Shot "C:\paddock-test\$CaseId-after-download.png"
  Check ($s -eq 'failed') "window: $s (wanted a refusal)"
  "TEXTS " + ((All-Texts | select -Unique | select -First 14) -join ' | ')
  Check ((Installed-Version) -eq $Old) "installed version still $Old"
  Check ((Get-Process spotter-app -ErrorAction SilentlyContinue | select -First 1).Id -eq $pidBefore) 'the app was not restarted'
}
exit $script:fail

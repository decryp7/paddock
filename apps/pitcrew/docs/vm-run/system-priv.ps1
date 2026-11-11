# Records `whoami /priv` as SYSTEM (a one-shot scheduled task) and checks that SeBackupPrivilege and SeRestorePrivilege are
# present but Disabled: FILE_FLAG_BACKUP_SEMANTICS in the log open then bypasses nothing it should not. At most 60 s.
$out = 'C:\paddock-test\system-priv.txt'
Remove-Item $out -ErrorAction SilentlyContinue
schtasks /Create /TN PitcrewPriv /SC ONCE /ST 00:00 /RU SYSTEM /RL HIGHEST /F /TR "cmd /c whoami /priv > $out" | Out-Null
schtasks /Run /TN PitcrewPriv | Out-Null
$deadline = (Get-Date).AddSeconds(60)
while (-not (Test-Path $out) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
schtasks /Delete /TN PitcrewPriv /F | Out-Null
if (-not (Test-Path $out)) { '::error::whoami /priv as SYSTEM did not run within 60 s'; exit 2 }
Start-Sleep -Milliseconds 500
$text = Get-Content $out -Raw
'--- whoami /priv as SYSTEM'
$text
$bad = 0
foreach ($p in 'SeBackupPrivilege', 'SeRestorePrivilege') {
  $line = ($text -split "`n" | Where-Object { $_ -match $p } | Select-Object -First 1)
  if (-not $line) { "::error::$p is not listed for SYSTEM (cannot confirm it is disabled)"; $bad = 1 }
  elseif ($line -match 'Enabled') { "::error::$p is ENABLED for SYSTEM: BACKUP_SEMANTICS would bypass access checks"; $bad = 1 }
  else { "$p : disabled (as expected)" }
}
exit $bad

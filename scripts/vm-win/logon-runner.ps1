# The in-session job runner of the Windows test VM (docs/testing/windows-test-vm.md, "GUI jobs"). A scheduled task starts it at the
# logon of paddock-test (interactive: it runs ON the desktop, so UI Automation sees the windows); it executes the .ps1 files that
# `vm-win.sh job` drops into C:\paddock-test\jobs, one at a time, and writes <job>.out (all output) and <job>.done (the exit code) next
# to them. SSH sessions have no desktop; this is the macOS `tart exec` equivalent. Local only, never CI. Logs no user content.
$ErrorActionPreference = 'Continue'
$root = 'C:\paddock-test'
$jobs = Join-Path $root 'jobs'
New-Item -ItemType Directory -Force -Path $jobs | Out-Null
while ($true) {
  Set-Content -Path (Join-Path $root 'runner.alive') -Value (Get-Date -Format o)
  if (Test-Path (Join-Path $root 'runner.stop')) { Remove-Item (Join-Path $root 'runner.stop'); break }
  $next = Get-ChildItem $jobs -Filter '*.ps1' -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -First 1
  if ($null -eq $next) { Start-Sleep -Milliseconds 500; continue }
  $out = [IO.Path]::ChangeExtension($next.FullName, 'out')
  $done = [IO.Path]::ChangeExtension($next.FullName, 'done')
  $code = 0
  try {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $next.FullName 2>&1 | Out-File -FilePath $out -Encoding utf8
    $code = $LASTEXITCODE
  } catch { $_ | Out-String | Add-Content $out; $code = 99 }
  Remove-Item $next.FullName -Force
  Set-Content -Path $done -Value $code
}

$ErrorActionPreference = 'Stop'
$p = Start-Process 'C:\paddock-test\old-setup.exe' -ArgumentList '/S' -Wait -PassThru
$v = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*' | ? DisplayName -eq 'Spotter' | select -First 1).DisplayVersion
if ($p.ExitCode -eq 0) { 'CHECK ok installer exit 0' } else { "CHECK FAIL installer exit $($p.ExitCode)" }
if ($v -eq '__OLD__') { "CHECK ok installed version $v" } else { "CHECK FAIL installed version is '$v', wanted __OLD__" }
if (Test-Path "$env:LOCALAPPDATA\Spotter\spotter-app.exe") { 'CHECK ok per-user install under %LOCALAPPDATA%\Spotter' } else { 'CHECK FAIL no spotter-app.exe under %LOCALAPPDATA%\Spotter' }

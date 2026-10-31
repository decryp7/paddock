# One-time, as paddock-admin: auto-logon of paddock-test and the task that starts the in-session job runner at its logon.
# The password is the VM's own throw-away one (setup-passwords on the host); it is stored in Winlogon's registry values, which is
# acceptable for a NAT-only test VM that is reverted after every run, and is never written anywhere else.
param([Parameter(Mandatory = $true)][string]$TestPassword, [Parameter(Mandatory = $true)][string]$RunnerSource)
$ErrorActionPreference = 'Stop'
Copy-Item $RunnerSource 'C:\paddock-test\runner.ps1' -Force
icacls 'C:\paddock-test\runner.ps1' /inheritance:r /grant:r 'SYSTEM:F' 'Administrators:F' 'paddock-test:RX' | Out-Null
$wl = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
Set-ItemProperty $wl -Name AutoAdminLogon -Value '1'
Set-ItemProperty $wl -Name DefaultUserName -Value 'paddock-test'
Set-ItemProperty $wl -Name DefaultDomainName -Value $env:COMPUTERNAME
Set-ItemProperty $wl -Name DefaultPassword -Value $TestPassword
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument '-NoProfile -WindowStyle Minimized -ExecutionPolicy Bypass -File C:\paddock-test\runner.ps1'
$trigger = New-ScheduledTaskTrigger -AtLogOn -User 'paddock-test'
$principal = New-ScheduledTaskPrincipal -UserId 'paddock-test' -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero)
Register-ScheduledTask -TaskName 'PaddockRunner' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
New-Item -ItemType Directory -Force -Path 'C:\paddock-test\jobs' | Out-Null
icacls 'C:\paddock-test\jobs' /grant 'paddock-test:(OI)(CI)M' | Out-Null
Write-Output 'gui-setup: auto-logon and PaddockRunner task registered'

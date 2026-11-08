# First-logon setup of the local Windows 11 Arm test VM (docs/testing/windows-test-vm.md). Runs once, as paddock-admin,
# from the setup disc that prepare.sh builds. Local only, never CI. Logs to C:\paddock-setup\setup.log (no user content).
# Ends by shutting the VM down; the host then takes the clean-baseline snapshot (scripts/vm-win.sh baseline).
param([Parameter(Mandatory = $true)][string]$SetupRoot)

$ErrorActionPreference = 'Stop'
$logDir = 'C:\paddock-setup'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
Start-Transcript -Path (Join-Path $logDir 'setup.log') -Append | Out-Null

function Step([string]$name) { Write-Output "== $(Get-Date -Format o) $name" }

# Reads payload.lock (name, url, sha256) and returns the verified local path of one installer.
function Get-Payload([string]$name) {
  $line = Get-Content (Join-Path $SetupRoot 'payload\payload.lock') | Where-Object { $_ -match "^$([regex]::Escape($name)) " }
  if (-not $line) { throw "payload.lock has no entry for $name" }
  $want = ($line -split '\s+')[2]
  $path = Join-Path $SetupRoot "payload\$name"
  $got = (Get-FileHash -Algorithm SHA256 $path).Hash.ToLowerInvariant()
  if ($got -ne $want) { throw "$name hash mismatch: expected $want, got $got" }
  return $path
}

try {
  Step 'Drivers from the setup disc (vmxnet3, display, mouse), in case Setup did not pick them up'
  $drivers = Join-Path $SetupRoot '$WinPEDriver$'
  if (Test-Path $drivers) { pnputil /add-driver "$drivers\*.inf" /subdirs /install | Out-Null }

  Step 'Power: never sleep, hibernate or turn off the disk'
  foreach ($s in 'standby-timeout-ac', 'hibernate-timeout-ac', 'disk-timeout-ac', 'monitor-timeout-ac') { powercfg /change $s 0 }
  powercfg /hibernate off

  Step 'Test folder C:\paddock-test, owned by paddock-test'
  New-Item -ItemType Directory -Force -Path 'C:\paddock-test' | Out-Null
  icacls 'C:\paddock-test' /inheritance:r /grant:r 'SYSTEM:(OI)(CI)F' 'Administrators:(OI)(CI)F' 'paddock-test:(OI)(CI)M' | Out-Null

  Step 'Defender stays on (checked, not changed); no Dev Drive'
  $mp = Get-MpComputerStatus
  Write-Output "Defender real-time protection: $($mp.RealTimeProtectionEnabled)"
  if (-not $mp.RealTimeProtectionEnabled) { throw 'Defender real-time protection is off; the test VM must match the CI runner' }

  Step 'OpenSSH Server: Windows capability first, the pinned Win32-OpenSSH MSI as fallback'
  $cap = Get-WindowsCapability -Online -Name 'OpenSSH.Server*' | Select-Object -First 1
  if ($cap -and $cap.State -ne 'Installed') {
    try { Add-WindowsCapability -Online -Name $cap.Name | Out-Null } catch { Write-Output "Capability install failed: $($_.Exception.Message)" }
  }
  if (-not (Get-Service sshd -ErrorAction SilentlyContinue)) {
    $msi = Get-Payload 'OpenSSH-ARM64.msi'
    $p = Start-Process msiexec.exe -ArgumentList '/i', "`"$msi`"", '/qn', '/norestart' -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "OpenSSH MSI failed with exit code $($p.ExitCode)" }
  }

  Step 'OpenSSH: key-only, keys in C:\ProgramData\ssh\authorized_keys\<user>'
  $sshDir = 'C:\ProgramData\ssh'
  New-Item -ItemType Directory -Force -Path $sshDir | Out-Null
  $keyDir = Join-Path $sshDir 'authorized_keys'
  New-Item -ItemType Directory -Force -Path $keyDir | Out-Null
  icacls $keyDir /inheritance:r /grant:r 'SYSTEM:(OI)(CI)F' 'Administrators:(OI)(CI)F' | Out-Null
  foreach ($user in 'paddock-test', 'paddock-admin') {
    $dest = Join-Path $keyDir $user
    Copy-Item (Join-Path $SetupRoot "keys\$user.pub") $dest -Force
    # sshd refuses a key file that anyone but SYSTEM, Administrators or the user can write.
    icacls $dest /inheritance:r /grant:r 'SYSTEM:F' 'Administrators:F' "${user}:R" | Out-Null
  }
  @'
# Paddock test VM (docs/testing/windows-test-vm.md): key-only logins for the two Paddock accounts.
AuthorizedKeysFile __PROGRAMDATA__/ssh/authorized_keys/%u
PubkeyAuthentication yes
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitEmptyPasswords no
AllowUsers paddock-test paddock-admin
Subsystem sftp sftp-server.exe
'@ | Set-Content -Encoding ascii (Join-Path $sshDir 'sshd_config')
  New-Item -Path 'HKLM:\SOFTWARE\OpenSSH' -Force | Out-Null
  New-ItemProperty -Path 'HKLM:\SOFTWARE\OpenSSH' -Name DefaultShell -PropertyType String -Force `
    -Value "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" | Out-Null
  Set-Service sshd -StartupType Automatic
  Restart-Service sshd -Force
  Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue | Remove-NetFirewallRule
  New-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -DisplayName 'OpenSSH Server (Paddock test VM, local subnet only)' `
    -Direction Inbound -Protocol TCP -LocalPort 22 -RemoteAddress LocalSubnet -Action Allow | Out-Null

  Step 'Git for Windows (pinned arm64 installer), CRLF checkouts like the CI runner'
  $git = Get-Payload 'Git-arm64.exe'
  $p = Start-Process $git -ArgumentList '/VERYSILENT', '/NORESTART', '/NOCANCEL', '/SP-', '/SUPPRESSMSGBOXES', '/o:CRLFOption=CRLFAlways' -Wait -PassThru
  if ($p.ExitCode -ne 0) { throw "Git for Windows installer failed with exit code $($p.ExitCode)" }
  & "$env:ProgramFiles\Git\cmd\git.exe" config --system core.autocrlf true
  Write-Output "git core.autocrlf (system): $(& "$env:ProgramFiles\Git\cmd\git.exe" config --system core.autocrlf)"

  Step 'Remove the answer file copies (they hold the setup passwords) and turn off auto-logon'
  Remove-Item -Force -ErrorAction SilentlyContinue 'C:\Windows\Panther\unattend.xml', 'C:\Windows\Panther\Unattend\unattend.xml'
  $winlogon = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
  Set-ItemProperty $winlogon -Name AutoAdminLogon -Value '0'
  Remove-ItemProperty $winlogon -Name DefaultPassword -ErrorAction SilentlyContinue

  Set-Content -Path (Join-Path $logDir 'done.marker') -Value (Get-Date -Format o)
  Step 'Done; shutting down for the clean-baseline snapshot'
  Stop-Transcript | Out-Null
  shutdown /s /t 15 /c 'Paddock test VM setup finished'
} catch {
  Write-Output "SETUP FAILED: $($_.Exception.Message)"
  Set-Content -Path (Join-Path $logDir 'failed.marker') -Value $_.Exception.Message
  Stop-Transcript | Out-Null
  exit 1
}

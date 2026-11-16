# One-time, as paddock-admin, for the buildtools-baseline snapshot (scripts/vm-win.sh toolchain-setup; ADR 0087 amendment of
# 2026-11-16): installs the Visual Studio 2022 Build Tools, LTSC 17.12, with the MSVC and Windows SDK pieces a Rust build links
# against (the repo's .cargo/config.toml links with rust-lld but "still links against the MSVC and Windows SDK libraries"). The
# bootstrapper is Microsoft's own, fetched on the host, checked against its pinned SHA-256 there AND here, and checked for a
# Microsoft Authenticode signature; the components come from Microsoft's CDN through that bootstrapper. No token, no account.
param(
  [Parameter(Mandatory)] [string] $Installer,
  [Parameter(Mandatory)] [string] $Sha256
)
$ErrorActionPreference = 'Stop'
# Runs detached from the SSH session that started it (vm-win.sh toolchain-setup polls): the installer starves the guest for minutes and
# an SSH session over a stalled guest is dropped by its keep-alive (seen 2026-11-16). It writes a log and, last, a done file with 0 or 1.
$log = 'C:\paddock-test\buildtools.log'
Remove-Item 'C:\paddock-test\buildtools.done' -Force -ErrorAction SilentlyContinue
Start-Transcript -Path $log -Force | Out-Null
try {
function Say($m) { Write-Output "buildtools-setup: $m" }
$got = (Get-FileHash -Algorithm SHA256 $Installer).Hash.ToLower()
if ($got -ne $Sha256.ToLower()) { throw "installer SHA-256 is $got, not the pinned $Sha256" }
$sig = Get-AuthenticodeSignature $Installer
if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch 'Microsoft Corporation') { throw "installer signature: $($sig.Status) $($sig.SignerCertificate.Subject)" }
Say "installer ok: sha256 $got, signed by $($sig.SignerCertificate.Subject)"
$free0 = (Get-PSDrive C).Free
$started = Get-Date
$args = @('--quiet', '--wait', '--norestart', '--nocache', '--noUpdateInstaller', '--installPath', 'C:\BuildTools',
  '--add', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
  '--add', 'Microsoft.VisualStudio.Component.VC.Tools.ARM64',
  '--add', 'Microsoft.VisualStudio.Component.Windows11SDK.22621')
$p = Start-Process -FilePath $Installer -ArgumentList $args -Wait -PassThru
Say "installer exit $($p.ExitCode) after $([int]((Get-Date) - $started).TotalSeconds) s"
if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) { throw "the installer failed (exit $($p.ExitCode)); its logs are in %TEMP%\dd_*" }
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$info = & $vswhere -products '*' -latest -format json | ConvertFrom-Json
Say "installed: $($info.displayName) $($info.installationVersion) at $($info.installationPath)"
$links = Get-ChildItem -Recurse 'C:\BuildTools\VC\Tools\MSVC' -Filter link.exe | ForEach-Object { $_.FullName }
Say "link.exe: $($links -join '; ')"
if (-not ($links | Where-Object { $_ -match 'arm64' })) { throw 'no arm64 link.exe was installed' }
$sdk = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\Lib' -Directory | Select-Object -ExpandProperty Name
Say "Windows SDK libs: $($sdk -join ', ')"
$used = [math]::Round(($free0 - (Get-PSDrive C).Free) / 1MB)
Say "disk used on C: about $used MB"
Say 'done'
Set-Content 'C:\paddock-test\buildtools.done' '0'
} catch {
  Write-Output "buildtools-setup: FAILED: $_"
  Set-Content 'C:\paddock-test\buildtools.done' '1'
} finally { Stop-Transcript | Out-Null }

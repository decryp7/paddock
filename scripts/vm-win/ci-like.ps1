# Runs a bash script in the guest the way the Gitea Windows CI runner starts a step (ADR 0087 amendment "the test VM runs tests the
# way the Windows CI runner does", cheap option: fixed values, no probe of the runner). Called by `scripts/vm-win.sh run --ci-like`;
# runs as paddock-test over SSH. Facts and their sources: docs/adr/amendments/0087-vm-mirrors-runner-2026-11-13.md (S4 to S7).
#
#   ci-like.ps1 -Bundle <git bundle> -Ref <ref in the bundle> -ScriptFile <bash script> [-Slot <clone folder name>]
#
# 1. temp folder C:\gitea-runner\work\<16 hex>\tmp whose ACL grants Authenticated Users (S-1-5-11) write; TEMP, TMP, TMPDIR,
#    RUNNER_TEMP and PADDOCK_JOB_TEMP point at it (the runner's conformance/ci/job-tmpdir.sh)
# 2. a git checkout (fetched from the bundle) with core.autocrlf=true checked out in the guest (never copied LF files)
# 3. Git's bin first on PATH, then the script runs as: bash --noprofile --norc -e -o pipefail <script> (cwd = the clone)
# 4. a self-check printed first; a control that fails stops the run as "not measured" (ER-33), exit 3
param(
  [Parameter(Mandatory)][string]$Bundle,
  [Parameter(Mandatory)][string]$Ref,
  [Parameter(Mandatory)][string]$ScriptFile,
  [string]$Slot = 'paddock-checkout-Windows-s1'
)
$ErrorActionPreference = 'Stop'
$git = 'C:\Program Files\Git'
$gitBin = "$git\bin"
$bash = "$gitBin\bash.exe"

function Fail($why) { Write-Host "::error::ci-like: $why. The run is not measured, it does not pass."; exit 3 }
# A native program: its stderr is not an error record; its exit code decides.
function Native([string]$what, [scriptblock]$cmd) {
  $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  try { & $cmd 2>&1 | ForEach-Object { "$_" } | Write-Host } finally { $ErrorActionPreference = $old }
  if ($LASTEXITCODE -ne 0) { Fail "$what failed (exit $LASTEXITCODE)" }
}
function NativeOut([scriptblock]$cmd) {
  $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  try { return ((& $cmd 2>&1 | ForEach-Object { "$_" }) -join "`n") } finally { $ErrorActionPreference = $old }
}
foreach ($p in @($bash, $Bundle, $ScriptFile)) { if (-not (Test-Path $p)) { Fail "missing $p" } }

# 1. The temp folder (S4).
$hex = -join ((1..8) | ForEach-Object { '{0:x2}' -f (Get-Random -Maximum 256) })
$work = "C:\gitea-runner\work\$hex"
$tmp = "$work\tmp"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$acl = Get-Acl $tmp
$sid = New-Object System.Security.Principal.SecurityIdentifier('S-1-5-11')
$acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($sid, 'Modify', 'ContainerInherit,ObjectInherit', 'None', 'Allow')))
Set-Acl -Path $tmp -AclObject $acl

# 3. The environment (S6, S7): the names and non-secret values the runner sets; 3 threads, not the runner's 5 and 8 (4 vCPU).
foreach ($n in 'TEMP', 'TMP', 'TMPDIR', 'RUNNER_TEMP', 'PADDOCK_JOB_TEMP') { Set-Item -Path "Env:$n" -Value $tmp }
$env:CI = 'true'; $env:GITHUB_ACTIONS = 'true'; $env:RUNNER_OS = 'Windows'; $env:CARGO_TERM_COLOR = 'always'
$env:CARGO_BUILD_JOBS = '3'; $env:NEXTEST_TEST_THREADS = '3'
$head = @($gitBin, "$git\cmd", "$env:USERPROFILE\.cargo\bin") | Where-Object { Test-Path $_ }
$env:PATH = (($head + ($env:PATH -split ';' | Where-Object { $head -notcontains $_ })) -join ';')

# 2. The checkout (S5): a real git checkout in the guest with the runner's line-end setting.
$clone = "$env:USERPROFILE\.paddock-ci\src\$Slot"
if (Test-Path $clone) { Remove-Item -Recurse -Force $clone }
New-Item -ItemType Directory -Force -Path (Split-Path $clone) | Out-Null
Native 'git init' { git init --quiet $clone }
Native 'git config' { git -C $clone config core.autocrlf true }
Native "git fetch of $Ref from the bundle" { git -C $clone fetch --quiet $Bundle "+${Ref}:refs/ci-like/head" }
Native 'git checkout' { git -C $clone checkout --quiet --detach refs/ci-like/head }

# 4. The self-check: what a run really mirrored, then the controls.
Write-Host '--- ci-like self-check ---'
Write-Host "temp:      $tmp"
$icacls = NativeOut { icacls $tmp }
Write-Host ($icacls -split "`n" | ForEach-Object { "           $_" } | Where-Object { $_.Trim() -ne '' }) -Separator "`n"
$crlf = (NativeOut { git -C $clone config --show-origin core.autocrlf }).Trim()
Write-Host "autocrlf:  $crlf"
$shell = (NativeOut { & $bash --noprofile --norc -e -o pipefail -c 'command -v bash; echo "$BASH_VERSION"; pwd' }) -split "`n"
Write-Host "shell:     $($shell[0]) (bash $($shell[1]))  pwd=$($shell[2])"
Write-Host "PATH head: $(($env:PATH -split ';' | Select-Object -First 4) -join ' ; ')"
Write-Host "missing:   $((@('node', 'npm.cmd', 'cargo', 'rustc') | Where-Object { -not (Get-Command $_ -ErrorAction SilentlyContinue) }) -join ', ')"
Write-Host "checkout:  $clone @ $((NativeOut { git -C $clone rev-parse --short HEAD }).Trim())"
$eol = (NativeOut { git -C $clone ls-files --eol -- AGENTS.md }).Trim()
Write-Host "eol:       $eol"
Write-Host '--------------------------'
if ($icacls -notmatch 'Authenticated Users' -and $icacls -notmatch 'S-1-5-11') { Fail 'the temp folder ACL does not grant Authenticated Users' }
if ($crlf -notmatch 'true$') { Fail 'core.autocrlf is not true' }
if ($shell[0].Trim() -ne '/usr/bin/bash') { Fail "bash is not Git's ($($shell[0]))" }
if ($shell[2] -notmatch '^/c/') { Fail "pwd in bash does not start with /c/ ($($shell[2]))" }
if ($eol -notmatch 'w/crlf') { Fail "a text file without an eol attribute is not CRLF in the checkout ($eol)" }

# 3. Run the script like a CI step.
Set-Location $clone
$old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
& $bash --noprofile --norc -e -o pipefail $ScriptFile 2>&1 | ForEach-Object { "$_" }
$code = $LASTEXITCODE
$ErrorActionPreference = $old
Write-Host "ci-like: the script exited $code"
exit $code

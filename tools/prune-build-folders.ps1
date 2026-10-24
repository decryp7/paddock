<#
.SYNOPSIS
  Lists and removes Cargo build folders that no longer belong to a live worktree (plan X47; Debrief's clean-build-dirs.ps1).

.DESCRIPTION
  Parallel agents build into their own folders (`target\agent-*` in the repository and `~\.<app>-target-*`); a finished one is
  never needed again, and on 2026-10-21 they had grown to 2 TB. Lists them with their sizes and, with -Remove, deletes the
  ones not named in -Keep. The repository's own `target\debug`, `target\release`, `target\rust-analyzer`, `target\doc`,
  `target\tmp` and `target\flycheck*` (editor tools) are never touched.

.EXAMPLE
  prune-build-folders.ps1 -App debrief                       # list with sizes
  prune-build-folders.ps1 -App debrief -Remove -Keep 6c,sdk  # delete all except ~\.debrief-target-6c* and ~\.debrief-target-sdk*
#>
param(
  [Parameter(Mandatory = $true)][ValidatePattern('^[a-z][a-z0-9-]*$')][string]$App,
  [string]$Repo = (Get-Location).Path,
  [switch]$Remove,
  # Suffixes of build folders still in use: `6c` keeps `~\.<app>-target-6c*` and `target\agent-6c*`.
  [string[]]$Keep = @()
)

$ErrorActionPreference = 'Stop'
$protected = @('debug', 'release', 'rust-analyzer', 'doc', 'tmp')

function Size-GB([string]$path) {
  $sum = (Get-ChildItem -LiteralPath $path -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
  [math]::Round(($sum ?? 0) / 1GB, 1)
}

function Kept([string]$name) {
  foreach ($k in $Keep) {
    if ($name -like ".$App-target-$k*" -or $name -like "agent-$k*") { return $true }
  }
  return $false
}

$candidates = @()
$candidates += Get-ChildItem -LiteralPath $HOME -Directory -Force -Filter ".$App-target*" -ErrorAction SilentlyContinue
$target = Join-Path $Repo 'target'
if (Test-Path $target) {
  $candidates += Get-ChildItem -LiteralPath $target -Directory -ErrorAction SilentlyContinue |
    Where-Object { $protected -notcontains $_.Name -and $_.Name -notlike 'flycheck*' }
}

foreach ($dir in $candidates) {
  $gb = Size-GB $dir.FullName
  $keep = Kept $dir.Name
  $action = if (-not $Remove) { 'found' } elseif ($keep) { 'kept' } else { 'removing' }
  '{0,8:N1} GB  {1,-9} {2}' -f $gb, $action, $dir.FullName
  if ($Remove -and -not $keep) {
    Remove-Item -LiteralPath $dir.FullName -Recurse -Force -ErrorAction Continue
  }
}

$drive = Get-PSDrive -Name ((Get-Item $Repo).PSDrive.Name)
'Free on {0}: {1:N1} GB' -f $drive.Name, ($drive.Free / 1GB)

New-Item -ItemType Directory -Force 'C:\ProgramData\Pitcrew\runners\old-svc' | Out-Null
'--- before the service ran: the old layout (made in C:\ProgramData)'
icacls 'C:\ProgramData\Pitcrew'
# P1-B as the installed service: a hard link to service.log that exists before the service starts must not be written through.
New-Item -ItemType Directory -Force 'C:\ProgramData\Pitcrew\logs' | Out-Null
Set-Content -Path 'C:\ProgramData\Pitcrew\logs\service.log' -Value 'planted'
New-Item -ItemType HardLink -Path 'C:\paddock-test\theirs-service.log' -Target 'C:\ProgramData\Pitcrew\logs\service.log' -Force | Out-Null
C:\paddock-test\pitcrew-service.exe install
# Wait for the service to be RUNNING, at most 60 s (never a fixed sleep).
$deadline = (Get-Date).AddSeconds(60)
do {
  $state = (sc.exe query Pitcrew | Out-String)
  if ($state -match 'RUNNING') { break }
  Start-Sleep -Milliseconds 500
} while ((Get-Date) -lt $deadline)
if ($state -notmatch 'RUNNING') { '::error::the Pitcrew service did not reach RUNNING within 60 s'; $state; exit 2 }
'--- service state'
$state
'--- data folder after the service started'
icacls 'C:\ProgramData\Pitcrew'
icacls 'C:\ProgramData\Pitcrew\runners'
'--- service log'
Get-Content 'C:\ProgramData\Pitcrew\logs\service.log' -ErrorAction SilentlyContinue | Select-Object -First 12
'--- the other name of the planted log must be unchanged'
$theirs = (Get-Content 'C:\paddock-test\theirs-service.log' -Raw).Trim()
if ($theirs -ne 'planted') { "::error::the service wrote through a hard link to its log: $theirs"; exit 3 }
'unchanged'
'--- the standard user's planted service.log.1 must be moved aside, and the live log must belong to SYSTEM or Administrators'
$asides = @(Get-ChildItem 'C:\ProgramData\Pitcrew\logs' -Filter 'service.log.1.untrusted-*' -ErrorAction SilentlyContinue)
if ($asides.Count -lt 1) { '::error::the standard user planted service.log.1 and it was not moved aside by the service'; exit 4 }
if (Test-Path 'C:\ProgramData\Pitcrew\logs\service.log.1') {
  $o = (Get-Acl 'C:\ProgramData\Pitcrew\logs\service.log.1').Owner
  if ($o -notmatch 'SYSTEM|Administrators') { "::error::service.log.1 is still owned by $o"; exit 4 }
}
$owner = (Get-Acl 'C:\ProgramData\Pitcrew\logs\service.log').Owner
"service.log owner: $owner"
if ($owner -notmatch 'SYSTEM|Administrators') { "::error::the service's log is owned by $owner"; exit 5 }
'planted files refused'

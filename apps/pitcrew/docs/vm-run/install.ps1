New-Item -ItemType Directory -Force 'C:\ProgramData\Pitcrew\runners\old-svc' | Out-Null
'--- before the service ran: the old layout (made in C:\ProgramData)'
icacls 'C:\ProgramData\Pitcrew'
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

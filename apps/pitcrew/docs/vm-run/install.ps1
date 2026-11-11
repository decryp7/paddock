New-Item -ItemType Directory -Force 'C:\ProgramData\Pitcrew\runners\old-svc' | Out-Null
'--- before the service ran: the old layout (made in C:\ProgramData)'
icacls 'C:\ProgramData\Pitcrew'
C:\paddock-test\pitcrew-service.exe install
Start-Sleep -Seconds 15
'--- service state'
sc.exe query Pitcrew
'--- data folder after the service started'
icacls 'C:\ProgramData\Pitcrew'
icacls 'C:\ProgramData\Pitcrew\runners'
'--- service log'
Get-Content 'C:\ProgramData\Pitcrew\logs\service.log' -ErrorAction SilentlyContinue | Select-Object -First 12

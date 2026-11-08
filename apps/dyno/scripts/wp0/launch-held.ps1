param([string]$Name)
Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$HOME\held.ps1",'-Name',$Name
Start-Sleep 4
Get-Process powershell | ForEach-Object { "ps $($_.Id) si=$($_.SI)" }

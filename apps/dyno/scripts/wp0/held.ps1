param([string]$Name)
if ($Name -notmatch ":") { $Name = Join-Path "C:\Users\Public" $Name }
$f = [IO.File]::Open($Name, "OpenOrCreate", "ReadWrite", "None")
Start-Sleep 400

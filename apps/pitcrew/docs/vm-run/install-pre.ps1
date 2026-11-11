# Before the service is installed: the data folder as an older Pitcrew left it. Its logs folder inherits what C:\ProgramData gives
# users, so a standard user can plant a file in it (std-plant.ps1 does, as the standard user).
New-Item -ItemType Directory -Force 'C:\ProgramData\Pitcrew\logs' | Out-Null
icacls 'C:\ProgramData\Pitcrew\logs' /grant 'Users:(OI)(CI)M' | Out-Null
'logs folder ready for the standard user'

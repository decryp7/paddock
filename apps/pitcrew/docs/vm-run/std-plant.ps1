. C:\paddock-test\env.ps1
whoami
# A rotation name the service opens (service.log.1), owned by this standard user: the sweep and the log writer must both refuse it.
Set-Content -Path 'C:\ProgramData\Pitcrew\logs\service.log.1' -Value 'planted-by-the-standard-user'
if (-not (Test-Path 'C:\ProgramData\Pitcrew\logs\service.log.1')) { '::error::the standard user could not plant service.log.1: install-pre.ps1 did not open the logs folder'; exit 2 }
(Get-Acl 'C:\ProgramData\Pitcrew\logs\service.log.1').Owner
exit 0

# One-time, as paddock-admin, for the admin-baseline snapshot (docs/testing/windows-test-vm.md, "admin-baseline"):
# paddock-test becomes a member of Administrators (a split token: medium integrity by default, elevated on consent) and the UAC
# consent for administrators is shown on the normal desktop (PromptOnSecureDesktop=0), still as a consent prompt
# (ConsentPromptBehaviorAdmin=5), so UI automation can answer it. UAC itself stays on (EnableLUA=1): an elevated relaunch is real.
# Takes effect at the next logon of paddock-test (the token is built at logon), so the caller shuts down before the snapshot.
$ErrorActionPreference = 'Stop'
$user = 'paddock-test'
# The group by its well-known SID (S-1-5-32-544), never by its name: the name is localised.
$adminSid = 'S-1-5-32-544'
$adminGroup = (Get-LocalGroup -SID $adminSid -ErrorAction Stop).Name
if (-not (Get-LocalGroupMember -SID $adminSid -ErrorAction Stop | Where-Object { $_.Name -like "*\$user" })) {
  Add-LocalGroupMember -SID $adminSid -Member $user
}
$pol = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
Set-ItemProperty $pol -Name ConsentPromptBehaviorAdmin -Type DWord -Value 5
Set-ItemProperty $pol -Name PromptOnSecureDesktop -Type DWord -Value 0
Set-ItemProperty $pol -Name EnableLUA -Type DWord -Value 1
$p = Get-ItemProperty $pol
$member = [bool](Get-LocalGroupMember -SID $adminSid | Where-Object { $_.Name -like "*\$user" })
Write-Output "admin-setup: $user in $adminGroup=$member EnableLUA=$($p.EnableLUA) ConsentPromptBehaviorAdmin=$($p.ConsentPromptBehaviorAdmin) PromptOnSecureDesktop=$($p.PromptOnSecureDesktop)"
if (-not $member -or $p.EnableLUA -ne 1 -or $p.ConsentPromptBehaviorAdmin -ne 5 -or $p.PromptOnSecureDesktop -ne 0) { throw 'admin-setup: the settings did not stick' }

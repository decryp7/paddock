. C:\paddock-test\uia.ps1
# The test CA goes into paddock-test's own Root store only (CurrentUser); Windows asks "Do you want to install this certificate?" on the
# desktop, and the answer is pressed through UI Automation.
$job = Start-Job { param($p) $c = [Security.Cryptography.X509Certificates.X509Certificate2]::new($p); $s = [Security.Cryptography.X509Certificates.X509Store]::new('Root', 'CurrentUser'); $s.Open('ReadWrite'); $s.Add($c); $s.Close() } -ArgumentList 'C:\paddock-test\ca.cer'
$dlg = Wait-Until { [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition) | ? { $_.Current.Name -match 'Security Warning' } | select -First 1 } 30
if ($dlg) { $b = Find-Element $dlg '^Yes$' 'Button'; if ($b) { Invoke-Element $b | Out-Null } }
Wait-Job $job -Timeout 30 | Out-Null
$n = @(Get-ChildItem Cert:\CurrentUser\Root | ? Subject -match 'Paddock update test CA').Count
$m = @(Get-ChildItem Cert:\LocalMachine\Root | ? Subject -match 'Paddock update test CA').Count
if ($n -eq 1) { 'CHECK ok the test CA is in the CurrentUser Root store' } else { 'CHECK FAIL the test CA is not in the CurrentUser Root store' }
if ($m -eq 0) { 'CHECK ok and not in the machine store' } else { 'CHECK FAIL the machine store has it too' }
if ($n -ne 1) { exit 1 }

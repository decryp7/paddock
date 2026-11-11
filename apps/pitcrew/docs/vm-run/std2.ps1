. C:\paddock-test\env.ps1
$env:PITCREW_TEST_EXPECT = 'standard'
whoami
$bad = 0
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture settled_service_runner_folders 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
if ($bad -ne 0) { exit 1 } else { exit 0 }

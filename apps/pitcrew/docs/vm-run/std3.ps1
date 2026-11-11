. C:\paddock-test\env.ps1
$env:PITCREW_TEST_EXPECT = 'standard'
$env:PITCREW_TEST_INSTALLED = '1'
$env:PITCREW_TEST_SERVICE_EXE = 'C:\paddock-test\pitcrew-service.exe'
whoami
$bad = 0
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture an_interactive_user_can_use 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\app_lib.exe --include-ignored --test-threads=1 --nocapture a_pipe_hosted_by_a_user 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\app_lib.exe --include-ignored --test-threads=1 --nocapture the_user_engine_started_beside 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
if ($bad -ne 0) { exit 1 } else { exit 0 }

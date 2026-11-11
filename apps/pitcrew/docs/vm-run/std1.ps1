. C:\paddock-test\env.ps1
$env:PITCREW_TEST_EXPECT = 'standard'
whoami
$bad = 0
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture the_pipe_refuses 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture a_new_data 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture a_per_user_engine_is_owned 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture a_data_folder_a_user 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture a_second_per_user_engine 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
if ($bad -ne 0) { exit 1 } else { exit 0 }

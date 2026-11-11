. C:\paddock-test\env.ps1
$env:PITCREW_TEST_EXPECT = 'elevated'
whoami
$bad = 0
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture the_pipe_refuses 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture a_new_data 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture a_data_folder_a_user 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture settled_service_runner_folders 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture the_folder_the_service_made 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture the_sweep_of_logs 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture a_held_open_hard_link 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& powershell -NoProfile -ExecutionPolicy Bypass -File C:\paddock-test\system-priv.ps1; if ($LASTEXITCODE -ne 0) { $bad = 1 }
if ($bad -ne 0) { exit 1 } else { exit 0 }

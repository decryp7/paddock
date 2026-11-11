. C:\paddock-test\env.ps1
$env:PITCREW_TEST_EXPECT = 'elevated'
$env:PITCREW_TEST_ENGINE_FILE = 'C:\paddock-test\engine.pipe'
whoami
$bad = 0
$t = (Get-Date).AddSeconds(150)
while (-not (Test-Path C:\paddock-test\engine.ready) -and (Get-Date) -lt $t) { Start-Sleep -Milliseconds 500 }
if (-not (Test-Path C:\paddock-test\engine.ready)) { 'engine.ready never appeared'; exit 2 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture another_account_cannot_reach 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture a_user_engine_refuses_to_start 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
& C:\paddock-test\app_lib.exe --include-ignored --test-threads=1 --nocapture an_elevated_app_leaves 2>&1 | Out-String; if ($LASTEXITCODE -ne 0) { $bad = 1 }
New-Item -ItemType File -Force C:\paddock-test\engine.done | Out-Null
if ($bad -ne 0) { exit 1 } else { exit 0 }

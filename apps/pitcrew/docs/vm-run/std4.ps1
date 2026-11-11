. C:\paddock-test\env.ps1
$env:PITCREW_TEST_EXPECT = 'standard'
$env:PITCREW_TEST_ENGINE_FILE = 'C:\paddock-test\engine.pipe'
Remove-Item C:\paddock-test\engine.* -ErrorAction SilentlyContinue
whoami
& C:\paddock-test\windows_caller.exe --include-ignored --test-threads=1 --nocapture a_per_user_engine_is_held_open 2>&1 | Out-String
exit $LASTEXITCODE

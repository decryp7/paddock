$env:PITCREW_TEST_SQUATTED = 'C:\ProgramData\PitcrewSquatTest'
$env:PITCREW_TEST_SETTLE_ROOT = 'C:\ProgramData\PitcrewSettleTest'
$env:PITCREW_TEST_SCRATCH = 'C:\paddock-test\scratch'
New-Item -ItemType Directory -Force C:\paddock-test\scratch | Out-Null

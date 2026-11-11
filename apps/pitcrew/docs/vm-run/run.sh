#!/usr/bin/env bash
set -u
W=/Volumes/d8sst7/dev-targets/wt/pitcrew-r5/scripts/vm-win.sh
V=/Volumes/d7sst7/dev-targets/pitcrew-vm/vm4
VMRUN="/Applications/VMware Fusion.app/Contents/Library/vmrun"
cleanup() { timeout 300 $W stop; timeout 300 $W revert clean-baseline; echo "VM running after cleanup:"; "$VMRUN" -T fusion list | head -2; echo CLEANED; }
trap cleanup EXIT
echo "VM running before the run:"; "$VMRUN" -T fusion list | head -2
timeout 300 $W revert gui-baseline || exit 1
timeout 400 $W start || exit 1
for f in windows_caller.exe app_lib.exe pitcrew-service.exe env.ps1 std4.ps1; do timeout 120 $W put paddock-test $V/$f C:/paddock-test/$f || exit 1; done
for f in admin1.ps1 admin2.ps1 install.ps1; do timeout 120 $W put paddock-admin $V/$f C:/paddock-test/$f || exit 1; done
echo "=== 1 STANDARD"; timeout 420 $W job $V/std1.ps1 300; echo "std1 exit $?"
echo "=== 2 ELEVATED ADMIN"; timeout 420 $W ssh --admin 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\paddock-test\admin1.ps1'; echo "admin1 exit $?"
echo "=== 2b HELD ENGINE (standard) and the other account (elevated admin)"
timeout 600 $W job $V/std4.ps1 400 > $V/std4.out 2>&1 &
HELD=$!
timeout 420 $W ssh --admin 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\paddock-test\admin2.ps1'; echo "admin2 exit $?"
wait $HELD; echo "std4 exit $?"; cat $V/std4.out
echo "=== 3 STANDARD again"; timeout 420 $W job $V/std2.ps1 300; echo "std2 exit $?"
echo "=== 4 INSTALL the service"; timeout 300 $W ssh --admin 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\paddock-test\install.ps1'; echo "install exit $?"
echo "=== 5 STANDARD against the installed service"; timeout 420 $W job $V/std3.ps1 300; echo "std3 exit $?"
echo VMDONE

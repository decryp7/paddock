#!/usr/bin/env bash
# The cross-account pipe check of ADR 0092 DF-3 (the test plan in docs/adr/0092-*.md), run on the local Windows test VM (ER-14: local only,
# never CI; the VM lock of scripts/vm-lock.sh applies). Account A (paddock-admin) serves a pipe that EVERY account may open, with
# `paddock-ipc-test-child serve-open`; account B (paddock-test) runs the test `a_pipe_served_by_another_account_is_refused_before_the_token_is_sent`.
# Because the pipe's access list lets B in, B reaches `Client::connect_on`'s `check_server_is_me`: with the check the test passes ("not
# served by your account"); with the call deleted (the `ipc_it_mut` build) the client is let in and the test must FAIL (ER-33). A pipe with the
# default owner-only access list would prove nothing: B would be refused at open and the check would never run.
#
# Inputs (built beforehand, paths below): $L/t-crates/... holds the test binary `it-*.exe` and `paddock-ipc-test-child.exe`; $L/t-mut/... the
# same test binary built with the `check_server_is_me` call removed. Output: $O/{server.log,xacct-*.log}.
set -u
ROOT=$(git rev-parse --show-toplevel)
export PADDOCK_AGENT_ID=${PADDOCK_AGENT_ID:-implementer:dyno-vm}
L=${DYNO_VM_DIR:-/Volumes/d8sst7/dev-targets/dyno-vm}
V="bash $ROOT/scripts/vm-win.sh"
A=$L/t-crates/aarch64-pc-windows-msvc/debug
M=$L/t-mut/aarch64-pc-windows-msvc/debug
IT=${IT_EXE:-it-d04254c346498c95.exe}
O=$L/run/p4; mkdir -p "$O"
PIPE='\\.\pipe\paddock-xacct-open'
ph() { echo "[$(date +%H:%M:%S)] $*"; }
g() { $V ssh "$@" 2>&1 | grep -v -E "post-quantum|store now|upgraded"; }
die() { echo "::error::$*"; exit 1; }

ph "revert and start the VM"
$V revert clean-baseline >"$O/rv.log" 2>&1 || die "the VM could not be reverted to clean-baseline; next step: see $O/rv.log and scripts/vm-lock.sh"
$V start >"$O/start.log" 2>&1 </dev/null || die "the VM did not start; next step: see $O/start.log, check the VM lock and the host's load"
[ "$(g 'echo up' | tr -d '\r' | tail -1)" = "up" ] || die "the VM started but does not answer over ssh; next step: see $O/start.log, then stop and revert it by hand"
ph "status $($V status)"
g 'New-Item -ItemType Directory -Force C:\paddock-test\dv | Out-Null; "up"' >/dev/null
# The test program is in deps/ (cargo before 1.100) or build/<package>/<hash>/out/ (1.100): whichever layout built it.
find_test() { find "$1/deps" "$1/build" -type f -name "$2" 2>/dev/null | head -1; }
for p in "$(find_test "$A" "$IT") ipc_it.exe" "$(find_test "$M" "$IT") ipc_it_mut.exe" "$A/paddock-ipc-test-child.exe child.exe"; do
  set -- $p; [ -f "$1" ] || die "missing build output $1; next step: build it (see the header)"
  $V put paddock-test "$1" "C:/paddock-test/dv/$2" 2>&1 | tail -1
done

ph "A = paddock-admin: serve a pipe every account may open"
( $V ssh --admin "& C:\\paddock-test\\dv\\child.exe serve-open $PIPE" > "$O/server.log" 2>&1 </dev/null & echo $! > "$O/server.pid" )
for i in $(seq 1 30); do grep -q "^ready" "$O/server.log" 2>/dev/null && break; sleep 2; done
grep -q "^ready" "$O/server.log" || die "the pipe server did not report ready; next step: see $O/server.log"

result=0
for v in ipc_it ipc_it_mut; do
  ph "=== B = paddock-test, $v"
  g "cd C:\\paddock-test\\dv; \$env:PADDOCK_OTHER_ACCOUNT_PIPE='$PIPE'; whoami; & .\\$v.exe a_pipe_served_by_another_account --nocapture 2>&1 | Select-Object -Last 25; 'EXIT=' + \$LASTEXITCODE" | tee "$O/xacct-$v.log" | cut -c1-330
  code=$(grep -o 'EXIT=[0-9]*' "$O/xacct-$v.log" | tail -1 | cut -d= -f2)
  # With the check the test passes (EXIT=0); without it the test must fail (ER-33).
  case $v in ipc_it) [ "$code" = "0" ] || { echo "::error::with the check the cross-account test did not pass (EXIT=$code); next step: see $O/xacct-$v.log"; result=1; } ;;
             ipc_it_mut) [ "$code" != "0" ] || { echo "::error::the probe cannot see the difference: the build without the check also passed (ER-33)"; result=1; } ;;
  esac
done

ph "stop the pipe server, stop and revert the VM"
kill "$(cat "$O/server.pid")" 2>/dev/null
g 'Get-Process child -ErrorAction SilentlyContinue | Stop-Process -Force; "server stopped"'
$V stop >/dev/null 2>&1; $V revert clean-baseline 2>&1 | tail -1
ph "status $($V status); result=$result"
exit $result

#!/bin/bash
# Launch at login on Linux (Dyno 1.1.0): first-run default of a fresh install, the Settings switch, and the hidden start of the entry's own command.
# Usage: login-probe-linux.sh <path to Dyno_1.1.0_amd64.deb>   (a fresh clone of the Linux test VM, removed at the end; --no-audio; nothing on the host desktop)
set -u
DEB=${1:?deb path}; HERE=$(cd "$(dirname "$0")" && pwd)
export TART_HOME=${TART_HOME:-/Volumes/d8sst7/vm-data/tart-from-home}
VM=paddock-vmsmoke-linux-login-$$
S='export XDG_RUNTIME_DIR=/run/user/1000 DISPLAY=:0 XAUTHORITY=/run/user/1000/gdm/Xauthority DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus'
g() { tart exec $VM bash -c "$1"; }
ses() { g "$S; $1"; }
res() { echo "RESULT $1: $2 -- $3"; }
cleanup() { tart stop $VM >/dev/null 2>&1; tart delete $VM >/dev/null 2>&1; kill $RUNPID 2>/dev/null; }
trap cleanup EXIT
tart clone linux-gnome-base $VM || exit 2
tart run --no-audio --no-graphics --vnc-experimental --rosetta=rosetta $VM >/dev/null 2>&1 & RUNPID=$!
for i in $(seq 75); do g true 2>/dev/null && break; sleep 4; done
for i in $(seq 60); do g 'pgrep -x gnome-shell' >/dev/null 2>&1 && break; sleep 3; done
cat "$HERE/guest/atspi.py" | tart exec -i $VM sudo tee /var/cache/vmsmoke/atspi.py >/dev/null
cat "$DEB" | tart exec -i $VM bash -c 'cat > /tmp/Dyno.deb'
g 'sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /tmp/Dyno.deb 2>&1 | tail -1; grep -c -a . /usr/bin/dyno-app >/dev/null'
echo "== before the first start: autostart folder"; g 'ls -la ~/.config/autostart 2>&1; ls /etc/xdg/autostart 2>&1 | grep -i dyno'
echo "== first start (plain, as the application menu does)"
ses 'systemd-run --user --collect --unit=dyno-first -E DISPLAY=:0 -E XAUTHORITY=$XAUTHORITY /usr/bin/dyno-app >/dev/null 2>&1'
for i in $(seq 30); do ses 'dyno status >/dev/null 2>&1' && break; sleep 4; done; sleep 15
F='$HOME/.config/autostart/Dyno.desktop'
echo "== default entry"; g "ls -l $F 2>&1; cat $F 2>&1"
if g "test -f $F" >/dev/null 2>&1; then
  EX=$(g "grep -E '^Exec=' $F | head -1")
  if echo "$EX" | grep -q -- '--hidden'; then res default PASS "Dyno.desktop exists after the first start and its $EX"; else res default FAIL "Dyno.desktop exists but $EX"; fi
else res default FAIL "no ~/.config/autostart/Dyno.desktop after a fresh install and first start"; fi
echo "== the agent's login service"; ses 'systemctl --user is-enabled dyno-agent 2>&1; systemctl --user is-active dyno-agent 2>&1'
echo "== Settings switch"
ses 'python3 /var/cache/vmsmoke/atspi.py press dyno "^File$" "menu item" >/dev/null 2>&1; sleep 1; python3 /var/cache/vmsmoke/atspi.py press dyno "^Settings…$" "menu item" >/dev/null 2>&1; sleep 5; python3 /var/cache/vmsmoke/atspi.py press dyno "^Dyno$" "push button" >/dev/null 2>&1; sleep 3'
echo "-- tree lines about login:"; ses 'python3 /var/cache/vmsmoke/atspi.py tree dyno 80 2>&1 | grep -i -B2 -A2 "login" | head -30; echo ---settings frame:; python3 /var/cache/vmsmoke/atspi.py tree dyno 80 2>&1 | grep -n -i "frame\|Settings" | head -8'
for ROLE in 'check box|toggle button|switch' 'check box' 'toggle button' 'switch' 'push button' 'radio button'; do
  if ses "python3 /var/cache/vmsmoke/atspi.py state dyno '\"Start Dyno at login\" \"$ROLE\"' 2>&1 | head -2" | grep -qiE 'checked|on|off|true|false'; then echo "switch role works: $ROLE"; break; fi
done
st() { ses "python3 /var/cache/vmsmoke/atspi.py state dyno '\"Start Dyno at login\" \"check box|toggle button|switch\"' 2>&1 | head -2"; }
echo "state before: $(st)"
ses 'python3 /var/cache/vmsmoke/atspi.py press dyno "Start Dyno at login" "check box|toggle button|switch" 2>&1 | head -2'
for i in $(seq 10); do g "test -f $F" >/dev/null 2>&1 || break; sleep 2; done
if g "test -f $F" >/dev/null 2>&1; then res switch-off FAIL "Dyno.desktop still there 20 s after switching off (or the switch was not pressed: state $(st))"; else res switch-off PASS "Dyno.desktop removed"; fi
ses 'python3 /var/cache/vmsmoke/atspi.py press dyno "Start Dyno at login" "check box|toggle button|switch" 2>&1 | head -2'
for i in $(seq 10); do g "test -f $F" >/dev/null 2>&1 && break; sleep 2; done
if g "test -f $F" >/dev/null 2>&1; then res switch-on PASS "Dyno.desktop back: $(g "grep -E '^Exec=' $F | head -1"); files in folder: $(g 'ls ~/.config/autostart | tr "\n" " "')"; else res switch-on FAIL "Dyno.desktop not back 20 s after switching on"; fi
echo "== hidden start of the entry's own command"
CMD=$(g "grep -E '^Exec=' $F | head -1 | sed 's/^Exec=//; s/\"//g'")
echo "entry command: $CMD"
g 'pkill -f "[r]osetta /usr/bin/dyno-app"; sleep 4'
ses "systemd-run --user --collect --unit=dyno-entry -E DISPLAY=:0 -E XAUTHORITY=\$XAUTHORITY $CMD >/dev/null 2>&1"
UP=no; for i in $(seq 30); do ses 'dyno status >/dev/null 2>&1' && { UP=yes; break; }; sleep 4; done; sleep 20
TREE=$(ses 'python3 /var/cache/vmsmoke/atspi.py tree dyno 20 2>&1 | head -8')
echo "$TREE"
if [ "$UP" != yes ]; then res hidden BROKEN "the agent did not answer after the entry's command"; elif echo "$TREE" | grep -q "frame"; then res hidden FAIL "a window (frame) exists after the hidden start"; else res hidden PASS "agent answers, no frame in the app's accessibility tree"; fi
ses 'systemd-run --user --collect --unit=dyno-show -E DISPLAY=:0 -E XAUTHORITY=$XAUTHORITY /usr/bin/dyno-app >/dev/null 2>&1'; sleep 25
if ses 'python3 /var/cache/vmsmoke/atspi.py tree dyno 20 2>&1 | grep -q "frame"'; then res show PASS "a second dyno-app shows the window"; else res show FAIL "no window after the second start"; fi

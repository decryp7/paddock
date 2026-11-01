#!/usr/bin/env bash
# The desktop-session smoke of the shell demo on Linux (conformance C21's desktop half): in a throw-away VM it starts a
# light X session (Xvfb, the stalonetray system tray, the dunst notification server) on the user's systemd session bus,
# installs the agent, starts the app in the background (`--hidden`, as at login) and checks, the way a user sees it:
#   1. the tray icon is embedded in the system tray;
#   2. when the agent stops, the app's OS notification reaches the notification server;
#   3. Quit in the tray menu ends the app with exit code 0 and leaves no process and no tray icon behind.
# It installs packages, a user unit and runs windows on a virtual display, so by rule 7 it runs only in a disposable VM
# (tools/macos-vm-smoke.sh --guest linux runs it after the service smoke): it refuses unless PADDOCK_DISPOSABLE_VM=1.
#
#   PADDOCK_DISPOSABLE_VM=1 scripts/shell-demo/smoke-desktop-linux.sh <folder with paddock-shell-demo and shell-demo-agent>
set -euo pipefail
[ "${PADDOCK_DISPOSABLE_VM:-}" = "1" ] || {
  echo "::error::this smoke installs packages and a systemd user unit; run it only in a disposable VM with PADDOCK_DISPOSABLE_VM=1 (rule 7)"
  exit 2
}
build="${1:?the build folder}"
unit="paddock-shell-demo-agent.service"
dest="$HOME/.local/opt/paddock-shell-demo"
app="$dest/paddock-shell-demo"
agent="$dest/shell-demo-agent"
work="$(mktemp -d)"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}"
export DISPLAY=:99
pids=(); app_pid=""

fail() { echo "::error::$*"; for f in "$work"/*.log; do echo "--- $(basename "$f")"; tail -n 20 "$f"; done; exit 1; }
# until_ok <seconds> <command...>: polls the command every half second (a hang guard, not a measurement).
until_ok() {
  local deadline=$((SECONDS + $1)); shift
  until "$@" >/dev/null 2>&1; do [ $SECONDS -lt $deadline ] || return 1; sleep 0.5; done
}
cleanup() {
  [ -z "$app_pid" ] || kill "$app_pid" 2>/dev/null || true
  [ -x "$agent" ] && "$agent" uninstall >/dev/null 2>&1 || true
  systemctl --user stop "$unit" 2>/dev/null || true
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  rm -rf "$dest" "$work"
}
trap cleanup EXIT

# --- the light desktop session ------------------------------------------------------------------------------------------
sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq >/dev/null
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends xvfb xauth x11-utils xdotool stalonetray \
  dunst libnotify-bin libwebkit2gtk-4.1-0 libayatana-appindicator3-1 >"$work/apt.log" 2>&1 || fail "the desktop packages did not install"
Xvfb :99 -screen 0 1280x800x24 -nolisten tcp >"$work/xvfb.log" 2>&1 & pids+=($!)
until_ok 20 xdpyinfo || fail "Xvfb did not start"
stalonetray --log-level info >"$work/tray.log" 2>&1 & pids+=($!)
dunst >"$work/dunst.log" 2>&1 & pids+=($!)
until_ok 20 dunstctl count history || fail "the notification server did not start"
until_ok 20 xdotool search --onlyvisible --class stalonetray || fail "the system tray did not start"
tray_wids=$(xdotool search --onlyvisible --class stalonetray); tray_wid=${tray_wids%%$'\n'*}

# --- the app, started in the background as at login ----------------------------------------------------------------------
mkdir -p "$(dirname "$dest")"; rm -rf "$dest"; cp -R "$build" "$dest"
"$agent" install >"$work/install.log" 2>&1 || fail "the agent did not install"
"$app" --hidden >"$work/app.log" 2>&1 & app_pid=$!

# 1. The tray icon: the app's icon window embedded (somewhere) inside the tray's window.
tray_icon() { grep -q '"paddock-shell-demo"' <<<"$(xwininfo -tree -id "$tray_wid")"; }
until_ok 60 tray_icon || fail "the app's tray icon did not appear in the system tray"
echo "the tray icon is in the system tray"

# 2. A notification: the agent stops; the app's health check notices and posts an OS notification.
systemctl --user stop "$unit"
notified() { [ "$(( $(dunstctl count displayed) + $(dunstctl count history) ))" -gt 0 ]; }
until_ok 240 notified || fail "no notification reached the notification server after the agent stopped"
dunstctl close-all # what is still on screen moves to the history
from_app() { grep -q '"data" : "paddock-shell-demo"' <<<"$(dunstctl history)"; }
until_ok 10 from_app || fail "the notification did not come from the app: $(dunstctl history)"
echo "a notification from the app reached the notification server: $(dunstctl history | grep -A2 '"summary"' | sed -n 's/.*"data" : "\(.*\)".*/\1/p' | head -n1)"

# 3. Quit from the tray menu (right click, the second item), as a user does.
read -r x y < <(xwininfo -id "$tray_wid" | awk '/Absolute upper-left X/ {x=$NF} /Absolute upper-left Y/ {y=$NF} END {print x+12, y+12}')
xdotool mousemove "$x" "$y" click 3
until_ok 10 xdotool search --onlyvisible --classname "paddock-shell-demo" || true
xdotool key Down Down Return
rc=0; until_ok 30 bash -c "! kill -0 $app_pid" || fail "the app did not quit from the tray menu"
wait "$app_pid" || rc=$?
app_pid=""
[ "$rc" -eq 0 ] || fail "the app quit with exit code $rc"
! pgrep -f "^$app" >/dev/null || fail "app processes are left behind: $(pgrep -af "^$app")"
until_ok 10 bash -c "! xwininfo -tree -id $tray_wid | grep -q '\"paddock-shell-demo\"'" || fail "the tray icon stayed after quitting"
echo "Quit in the tray menu ended the app cleanly (exit 0, no process or tray icon left)"

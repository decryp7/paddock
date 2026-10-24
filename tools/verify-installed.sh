#!/usr/bin/env bash
# Checks an installed app: the bundle, its background agent (launchd on macOS, a systemd user unit on Linux), the service's
# socket and its answer, and the end of the logs (plan X47; the apps' macos-verify.sh and linux-verify.sh in one). Read-only:
# it changes nothing and starts nothing except the service's own `status` command.
#
#   bash paddock/tools/verify-installed.sh --name Pitcrew [--id pitcrew] [--bundle /Applications/Pitcrew.app]
#        [--label net.decryptology.pitcrew.service] [--unit pitcrew.service] [--service pitcrew-service]
#        [--data <app data folder>] [--socket pitcrew.sock] [--log service.log]
#
# Defaults follow paddock-env: macOS data in ~/Library/Application Support/<Name>, Linux in $XDG_DATA_HOME/<Name>.
# --service is the background program (found in the bundle's Contents/MacOS, on PATH, or in <data>/bin); without it only the
# bundle and the agent are checked. HOME may be pointed at a scratch folder (tests do). PADDOCK_TOOLS_OS overrides `uname -s`.
set -uo pipefail

name=""; id=""; bundle=""; label=""; unit=""; service=""; data=""; socket=""; logfile="logs/service.log"
while [ $# -gt 0 ]; do
  case "$1" in
    --name) name="${2:?--name needs a value}"; shift ;;
    --id) id="${2:?}"; shift ;;
    --bundle) bundle="${2:?}"; shift ;;
    --label) label="${2:?}"; shift ;;
    --unit) unit="${2:?}"; shift ;;
    --service) service="${2:?}"; shift ;;
    --data) data="${2:?}"; shift ;;
    --socket) socket="${2:?}"; shift ;;
    --log) logfile="${2:?}"; shift ;;
    *) echo "verify-installed: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
[ -n "$name" ] || { echo "verify-installed: --name <product name> is needed" >&2; exit 2; }
[ -n "$id" ] || id=$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]')
os="${PADDOCK_TOOLS_OS:-$(uname -s)}"

problems=0
green=""; red=""; reset=""
if [ -t 1 ]; then green=$'\033[32m'; red=$'\033[31m'; reset=$'\033[0m'; fi
ok() { printf '  %sok%s    %s\n' "$green" "$reset" "$1"; }
bad() { printf '  %sfail%s  %s\n        %s\n' "$red" "$reset" "$1" "$2"; problems=$((problems + 1)); }
note() { printf '  ..    %s\n' "$1"; }
tmp=$(mktemp -d "${TMPDIR:-/tmp}/paddock-verify.XXXXXX") && trap 'rm -rf "$tmp"' EXIT

echo "$name on this machine"
case "$os" in
  Darwin)
    [ -n "$bundle" ] || bundle="/Applications/$name.app"
    [ -n "$data" ] || data="$HOME/Library/Application Support/$name"
    [ -d "$bundle" ] && ok "app bundle $bundle" || bad "app bundle" "not found at $bundle (install from the .dmg)"
    svc=""
    if [ -n "$service" ]; then
      svc="$bundle/Contents/MacOS/$service"
      [ -x "$svc" ] && ok "service binary in the bundle" || { bad "service binary" "$svc is missing; rebuild"; svc=""; }
    fi
    if [ -n "$label" ]; then
      plist="$bundle/Contents/Library/LaunchAgents/$label.plist"
      [ -f "$plist" ] && ok "launch agent plist in the bundle" || bad "launch agent plist" "Contents/Library/LaunchAgents/$label.plist is missing; rebuild"
      if [ -f "$plist" ]; then plutil -lint "$plist" >/dev/null 2>&1 && ok "plist is valid" || note "plist could not be checked"; fi
      if launchctl print "gui/$(id -u)/$label" >"$tmp/launchctl" 2>&1; then
        state=$(awk -F'= ' '/^\tstate/ {print $2; exit}' "$tmp/launchctl")
        pid=$(awk -F'= ' '/^\tpid/ {print $2; exit}' "$tmp/launchctl")
        if [ "$state" = running ]; then ok "launchd runs the agent (pid ${pid:-?})"
        else bad "launch agent" "launchd knows it but its state is '${state:-?}' (a stale registration: turn the service off and on in Settings)"; fi
      else
        bad "launch agent" "not loaded: open $name > Settings > Service > Turn on, and allow it in System Settings > General > Login Items"
      fi
    fi
    ;;
  Linux)
    [ -n "$data" ] || data="${XDG_DATA_HOME:-$HOME/.local/share}/$name"
    svc=""
    if [ -n "$service" ]; then
      svc=$(command -v "$service" || true)
      [ -z "$svc" ] && [ -x "$data/bin/$service" ] && svc="$data/bin/$service"
      [ -n "$svc" ] && ok "service program $svc" || bad "service program" "$service not found: install the .deb (it is /usr/bin/$service) or turn the service on from the AppImage"
    fi
    if [ -n "$unit" ]; then
      if command -v systemctl >/dev/null 2>&1; then
        if systemctl --user show-environment >/dev/null 2>&1; then
          systemctl --user cat "$unit" >/dev/null 2>&1 && ok "user unit $unit is installed" || bad "user unit" "not installed: open $name > Settings > Service > Turn on"
          state=$(systemctl --user is-active "$unit" 2>/dev/null)
          [ "$state" = active ] && ok "systemd runs the service ($state)" || bad "service state" "'${state:-unknown}': systemctl --user status $unit; journalctl --user -u $unit -n 30"
          systemctl --user is-enabled "$unit" >/dev/null 2>&1 && ok "enabled: it starts at log-in" || note "not enabled (starts only when turned on)"
        else
          bad "systemd user instance" "not reachable from this shell (ssh without a session?). Log in on the machine, or run 'loginctl enable-linger $USER' and use a full session"
        fi
        linger=$(loginctl show-user "$USER" -p Linger --value 2>/dev/null)
        [ "$linger" = yes ] && ok "lingering is on: the service keeps going after log-out" || note "lingering is off: the service stops at log-out. To keep it running: loginctl enable-linger $USER"
      else
        note "no systemd: start the service by hand ($service run)"
      fi
    fi
    ;;
  *) echo "verify-installed: $os is not supported (macOS and Linux)" >&2; exit 2 ;;
esac

if [ -n "$socket" ]; then
  sock_dir="$data"
  if [ "$os" = Linux ] && [ -n "${XDG_RUNTIME_DIR:-}" ] && [ -S "$XDG_RUNTIME_DIR/$id/$socket" ]; then sock_dir="$XDG_RUNTIME_DIR/$id"; fi
  [ -S "$sock_dir/$socket" ] && ok "service socket $sock_dir/$socket" || bad "service socket" "$sock_dir/$socket is missing: the service is not running"
fi
if [ -n "${svc:-}" ]; then
  echo
  "$svc" status || bad "service status" "the service did not answer; see $data/$logfile"
fi
if [ -f "$data/$logfile" ]; then
  echo
  echo "Service log (last lines of $data/$logfile)"
  tail -n 5 "$data/$logfile" | sed 's/^/    /'
fi

echo
if [ "$problems" = 0 ]; then
  echo "Everything looks right."
else
  echo "$problems problem(s) found."
  exit 1
fi

#!/usr/bin/env bash
# Checks an installed Pitwall on Linux: the server program, its systemd user unit and state, lingering, the discovery
# file and the server's answer (docs/linux.md). Read-only. (Adapted from Pitcrew's linux-verify.sh.)
#
#   bash scripts/linux-verify.sh
set -uo pipefail

unit=pitwall.service
data="${PITWALL_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/pitwall}"
problems=0
ok() { printf '  \033[32mok\033[0m    %s\n' "$1"; }
bad() { printf '  \033[31mfail\033[0m  %s\n        %s\n' "$1" "$2"; problems=$((problems + 1)); }
note() { printf '  ..    %s\n' "$1"; }

echo "Pitwall on this machine"
svc=$(command -v pitwall-server || true)
[ -z "$svc" ] && [ -x "$data/bin/pitwall-server" ] && svc="$data/bin/pitwall-server"
[ -n "$svc" ] && ok "server program $svc" || bad "server program" "pitwall-server not found: install the .deb (it is /usr/bin/pitwall-server) or turn the service on from the AppImage"

if command -v systemctl >/dev/null; then
  if systemctl --user show-environment >/dev/null 2>&1; then
    systemctl --user cat "$unit" >/dev/null 2>&1 && ok "user unit $unit is installed" ||
      bad "user unit" "not installed: open Pitwall > Settings > Service > Turn on (or run: pitwall-server install)"
    state=$(systemctl --user is-active "$unit" 2>/dev/null)
    [ "$state" = active ] && ok "systemd runs the server ($state)" ||
      bad "server state" "'${state:-unknown}': systemctl --user status $unit; journalctl --user -u $unit -n 30"
    systemctl --user is-enabled "$unit" >/dev/null 2>&1 && ok "enabled: it starts at log-in" || note "not enabled (starts only when turned on)"
  else
    bad "systemd user instance" "not reachable from this shell (ssh without a session?). Log in on the machine, or use 'loginctl enable-linger $USER' and a full session"
  fi
  linger=$(loginctl show-user "$USER" -p Linger --value 2>/dev/null)
  [ "$linger" = yes ] && ok "lingering is on: the server keeps going after log-out" ||
    note "lingering is off: the server stops at log-out. To keep it running: loginctl enable-linger $USER"
else
  note "no systemd: start the server by hand with 'pitwall-server'"
fi

if [ -f "$data/discovery.json" ]; then
  ok "discovery file $data/discovery.json"
  port=$(sed -n 's/.*"port": *\([0-9]*\).*/\1/p' "$data/discovery.json" | head -1)
  key=$(cat "$data/admin.key" 2>/dev/null)
  if [ -n "$port" ] && command -v curl >/dev/null; then
    code=$(curl -s -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $key" "http://127.0.0.1:$port/mgmt/v1/rpc/overview")
    [ "$code" = 200 ] && ok "the server answers on 127.0.0.1:$port" || bad "server answer" "overview answered $code"
  fi
else
  bad "discovery file" "missing: the server is not running"
fi
[ -d "$data/logs" ] && { echo; echo "Logs: $data/logs"; ls -1 "$data/logs" | head -5 | sed 's/^/    /'; }

echo
if [ "$problems" = 0 ]; then
  echo "Everything looks right."
else
  echo "$problems problem(s) found; docs/linux.md explains each step."
  exit 1
fi

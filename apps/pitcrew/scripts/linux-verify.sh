#!/usr/bin/env bash
# Checks an installed Pitcrew on Linux: the service program, its systemd user unit and state, lingering, the
# service's socket and answer, each runner's status and the end of its log (docs/linux.md). Read-only.
#
#   bash scripts/linux-verify.sh
set -uo pipefail

unit=pitcrew.service
data="${XDG_DATA_HOME:-$HOME/.local/share}/Pitcrew"
sock_dir="${XDG_RUNTIME_DIR:+$XDG_RUNTIME_DIR/pitcrew}"
sock_dir="${sock_dir:-$data}"
problems=0
ok() { printf '  \033[32mok\033[0m    %s\n' "$1"; }
bad() { printf '  \033[31mfail\033[0m  %s\n        %s\n' "$1" "$2"; problems=$((problems + 1)); }
note() { printf '  ..    %s\n' "$1"; }

echo "Pitcrew on this machine"
svc=$(command -v pitcrew-service || true)
[ -z "$svc" ] && [ -x "$data/bin/pitcrew-service" ] && svc="$data/bin/pitcrew-service"
[ -n "$svc" ] && ok "service program $svc" || bad "service program" "pitcrew-service not found: install the .deb (it is /usr/bin/pitcrew-service) or turn the service on from the AppImage"

if command -v systemctl >/dev/null; then
  if systemctl --user show-environment >/dev/null 2>&1; then
    systemctl --user cat "$unit" >/dev/null 2>&1 && ok "user unit $unit is installed" ||
      bad "user unit" "not installed: open Pitcrew > Settings > Service > Turn on"
    state=$(systemctl --user is-active "$unit" 2>/dev/null)
    [ "$state" = active ] && ok "systemd runs the service ($state)" ||
      bad "service state" "'${state:-unknown}': systemctl --user status $unit; journalctl --user -u $unit -n 30"
    systemctl --user is-enabled "$unit" >/dev/null 2>&1 && ok "enabled: it starts at log-in" || note "not enabled (starts only when turned on)"
  else
    bad "systemd user instance" "not reachable from this shell (ssh without a session?). Log in on the machine, or use 'loginctl enable-linger $USER' and a full session"
  fi
  linger=$(loginctl show-user "$USER" -p Linger --value 2>/dev/null)
  [ "$linger" = yes ] && ok "lingering is on: runners keep going after log-out" ||
    note "lingering is off: the service stops at log-out. To keep it running: loginctl enable-linger $USER"
else
  note "no systemd: start the service by hand with 'pitcrew-service run'"
fi

[ -S "$sock_dir/pitcrew.sock" ] && ok "service socket $sock_dir/pitcrew.sock" || bad "service socket" "missing: the service is not running"
if [ -n "$svc" ]; then
  echo
  "$svc" status || bad "service status" "the service did not answer; see $data/logs/service.log"
fi

for log in "$data"/runners/*/logs/runner.log; do
  [ -f "$log" ] || continue
  name=$(basename "$(dirname "$(dirname "$log")")")
  echo
  echo "Runner $name - last lines of $log"
  tail -n 8 "$log" | sed 's/^/    /'
  grep -q "declare successfully" "$log" && ok "$name was accepted by its server" || note "$name has not been accepted by its server yet"
done
[ -f "$data/logs/service.log" ] && { echo; echo "Service log (last lines)"; tail -n 5 "$data/logs/service.log" | sed 's/^/    /'; }

echo
if [ "$problems" = 0 ]; then
  echo "Everything looks right. Push a commit to a repository with a job for your runner's labels to see it run."
else
  echo "$problems problem(s) found; docs/linux.md explains each step."
  exit 1
fi

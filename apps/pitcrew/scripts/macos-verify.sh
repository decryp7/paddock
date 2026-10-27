#!/usr/bin/env bash
# Checks an installed Pitcrew on a Mac: the app bundle and its launch agent, the agent's state in launchd, the
# service's answer, each runner's status and the end of its log (docs/macos.md). Read-only: it changes nothing.
#
#   bash scripts/macos-verify.sh [/Applications/Pitcrew.app]
set -uo pipefail

app="${1:-/Applications/Pitcrew.app}"
label=net.decryptology.pitcrew.service
support="$HOME/Library/Application Support/Pitcrew"
problems=0
ok() { printf '  \033[32mok\033[0m    %s\n' "$1"; }
bad() { printf '  \033[31mfail\033[0m  %s\n        %s\n' "$1" "$2"; problems=$((problems + 1)); }
note() { printf '  ..    %s\n' "$1"; }

echo "Pitcrew on this Mac"
[ -d "$app" ] && ok "app bundle $app" || bad "app bundle" "not found at $app (install from the .dmg)"
[ -x "$app/Contents/MacOS/pitcrew-service" ] && ok "service binary in the bundle" || bad "service binary" "$app/Contents/MacOS/pitcrew-service is missing; rebuild"
[ -f "$app/Contents/Library/LaunchAgents/$label.plist" ] && ok "launch agent plist in the bundle" ||
  bad "launch agent plist" "Contents/Library/LaunchAgents/$label.plist is missing; rebuild"
plutil -lint "$app/Contents/Library/LaunchAgents/$label.plist" >/dev/null 2>&1 && ok "plist is valid" || note "plist could not be checked"

if launchctl print "gui/$(id -u)/$label" >/tmp/pitcrew-launchctl.$$ 2>&1; then
  state=$(awk -F'= ' '/^\tstate/ {print $2; exit}' /tmp/pitcrew-launchctl.$$)
  pid=$(awk -F'= ' '/^\tpid/ {print $2; exit}' /tmp/pitcrew-launchctl.$$)
  [ "$state" = running ] && ok "launchd runs the agent (pid $pid)" || bad "launch agent" "launchd knows it but its state is '${state:-?}'"
else
  bad "launch agent" "not loaded: open Pitcrew ▸ Settings ▸ Service ▸ Turn on, and allow it in System Settings ▸ General ▸ Login Items"
fi
rm -f /tmp/pitcrew-launchctl.$$

[ -S "$support/pitcrew.sock" ] && ok "service socket $support/pitcrew.sock" || bad "service socket" "missing: the service is not running"
if [ -x "$app/Contents/MacOS/pitcrew-service" ]; then
  echo
  if ! "$app/Contents/MacOS/pitcrew-service" status; then
    bad "service status" "the service did not answer; see $support/logs/service.log"
  fi
fi

for log in "$support"/runners/*/logs/runner.log; do
  [ -f "$log" ] || continue
  name=$(basename "$(dirname "$(dirname "$log")")")
  echo
  echo "Runner $name — last lines of $log"
  tail -n 8 "$log" | sed 's/^/    /'
  grep -q "declare successfully" "$log" && ok "$name was accepted by its server" || note "$name has not been accepted by its server yet"
done
[ -f "$support/logs/service.log" ] && { echo; echo "Service log (last lines)"; tail -n 5 "$support/logs/service.log" | sed 's/^/    /'; }

echo
if [ "$problems" = 0 ]; then
  echo "Everything looks right. Push a commit to a repository with a 'runs-on: macos-latest' job to see it run."
else
  echo "$problems problem(s) found; docs/macos.md explains each step."
  exit 1
fi

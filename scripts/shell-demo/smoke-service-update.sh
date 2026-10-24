#!/usr/bin/env bash
# The service-after-update smoke (catalogue R03, conformance C21): installs build N of Paddock Shell Demo, registers its
# sample agent through SMAppService, replaces the app with build N+1 (a different version, so a different ad-hoc cdhash),
# runs the app's start-up check (rule 24: unregister, bootout, wait, register) and asserts that the agent now runs
# build N+1 and that launchd does not hold it in EX_CONFIG. Then it removes everything it registered.
#
# It REGISTERS A REAL LAUNCH AGENT and puts an app in ~/Applications, so by rule 7 it runs only on a disposable macOS
# machine (a throw-away VM or an ephemeral runner): it refuses to start unless PADDOCK_DISPOSABLE_MAC=1. Never run it on a
# developer's or the shared runner's Mac.
#
#   PADDOCK_DISPOSABLE_MAC=1 scripts/shell-demo/smoke-service-update.sh path/to/N/App.app path/to/N+1/App.app
set -euo pipefail
[ "${PADDOCK_DISPOSABLE_MAC:-}" = "1" ] || {
  echo "::error::this smoke registers a real launch agent; run it only on a disposable Mac with PADDOCK_DISPOSABLE_MAC=1 (rule 7)"
  exit 2
}
old="${1:?build N .app}"
new="${2:?build N+1 .app}"
label="net.decryptology.paddock-shell-demo.agent"
dest="$HOME/Applications/Paddock Shell Demo.app"
agent="$dest/Contents/MacOS/shell-demo-agent"
data="$HOME/Library/Application Support/Paddock Shell Demo"
target="gui/$(id -u)/$label"

version_of() { /usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$1/Contents/Info.plist"; }
heartbeat_version() { sed -n 's/.*"version":"\([^"]*\)".*/\1/p' "$data/agent/heartbeat.json" 2>/dev/null || true; }
wait_for_version() {
  local want="$1" deadline=$((SECONDS + 180))
  until [ "$(heartbeat_version)" = "$want" ]; do
    if [ $SECONDS -ge $deadline ]; then
      echo "::error::the agent does not run version $want (heartbeat says '$(heartbeat_version)')"
      launchctl print "$target" || true
      return 1
    fi
    sleep 2
  done
}
cleanup() {
  [ -x "$agent" ] && "$agent" uninstall || true
  launchctl bootout "$target" 2>/dev/null || true
  rm -rf "$dest"
}
trap cleanup EXIT

v_old=$(version_of "$old")
v_new=$(version_of "$new")
[ "$v_old" != "$v_new" ] || { echo "::error::the two builds have the same version ($v_old)"; exit 1; }

mkdir -p "$HOME/Applications"
rm -rf "$dest" && cp -R "$old" "$dest"
"$agent" install
wait_for_version "$v_old"
echo "build N ($v_old) runs"

# The update: the app is replaced while the agent's launchd job (with build N's launch constraint) lives on.
rm -rf "$dest" && cp -R "$new" "$dest"
"$agent" check
wait_for_version "$v_new"
report=$(launchctl print "$target")
if echo "$report" | grep -q 'last exit code = 78'; then
  echo "::error::launchd holds the agent in EX_CONFIG after the update (R03)"
  echo "$report"
  exit 1
fi
echo "$report" | grep -q 'state = running' || { echo "::error::the agent is not running"; echo "$report"; exit 1; }
echo "build N+1 ($v_new) runs after the update; no EX_CONFIG"

#!/usr/bin/env bash
# The service-after-update smoke (catalogue R03, conformance C21): installs build N of Paddock Shell Demo, registers its
# sample agent through SMAppService, replaces the app with build N+1 (a different version, so a different ad-hoc cdhash),
# runs the app's start-up check (rule 24: unregister, bootout, wait, register) and asserts that the agent now runs
# build N+1 and that launchd does not hold it in EX_CONFIG. Then it removes everything it registered.
#
# The app goes to /Applications the way a user installs it (dragged from the DMG; PADDOCK_SMOKE_APPS overrides the folder).
# On a fresh Mac SMAppService reports the never-registered agent as NotFound; Install must register it anyway (the C21
# VM run found that the first install failed exactly there).
#
# It REGISTERS A REAL LAUNCH AGENT and puts an app in /Applications, so by rule 7 it runs only on a disposable macOS
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
apps="${PADDOCK_SMOKE_APPS:-/Applications}"
dest="$apps/Paddock Shell Demo.app"
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
# The app's own view must agree with launchd: "running", never "refused" for a healthy job (macOS 26 prints nested
# blocks with their own `state = active`, which once read as a stopped job).
expect_running() {
  local st; st=$("$agent" status)
  grep -q '"condition": "running"' <<<"$st" || { echo "::error::the app does not see the agent running"; echo "$st"; return 1; }
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
# The agent reports its own version in the heartbeat; it must be the app's, or the wait below can never succeed.
for build in "$old" "$new"; do
  a=$("$build/Contents/MacOS/shell-demo-agent" --self-test | awk '{print $2}')
  [ "$a" = "$(version_of "$build")" ] || { echo "::error::$build: the agent is $a, the app $(version_of "$build")"; exit 1; }
done

mkdir -p "$apps"
rm -rf "$dest" && ditto "$old" "$dest"
"$agent" install
wait_for_version "$v_old"
expect_running
echo "build N ($v_old) runs"

# The update: the app is replaced while the agent's launchd job (with build N's launch constraint) lives on.
rm -rf "$dest" && ditto "$new" "$dest"
"$agent" check
wait_for_version "$v_new"
report=$(launchctl print "$target")
if grep -q 'last exit code = 78' <<<"$report"; then
  echo "::error::launchd holds the agent in EX_CONFIG after the update (R03)"
  echo "$report"
  exit 1
fi
grep -q 'state = running' <<<"$report" || { echo "::error::the agent is not running"; echo "$report"; exit 1; }
expect_running
echo "build N+1 ($v_new) runs after the update; no EX_CONFIG"

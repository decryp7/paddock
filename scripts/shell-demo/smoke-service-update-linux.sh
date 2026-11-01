#!/usr/bin/env bash
# The Linux twin of smoke-service-update.sh (conformance C21): installs build N of the shell demo's agent as a systemd USER
# unit, replaces the binary with build N+1, runs the agent's start-up check (rule 24) and asserts that the unit is active and
# the heartbeat says N+1. It registers a real user unit, so by rule 7 it runs only in a throw-away VM with a real
# `systemd --user` session (tools/macos-vm-smoke.sh --guest linux): it refuses unless PADDOCK_DISPOSABLE_VM=1.
#
#   PADDOCK_DISPOSABLE_VM=1 scripts/shell-demo/smoke-service-update-linux.sh <N folder with shell-demo-agent> <N+1 folder>
set -euo pipefail
[ "${PADDOCK_DISPOSABLE_VM:-}" = "1" ] || {
  echo "::error::this smoke registers a real systemd user unit; run it only in a disposable VM with PADDOCK_DISPOSABLE_VM=1 (rule 7)"
  exit 2
}
old="${1:?build N folder}"; new="${2:?build N+1 folder}"
unit="paddock-shell-demo-agent.service"
dest="$HOME/.local/opt/paddock-shell-demo"
agent="$dest/shell-demo-agent"

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}"
systemctl --user is-system-running >/dev/null 2>&1 || systemctl --user show-environment >/dev/null 2>&1 || {
  echo "::error::no systemd --user session (is lingering/login set up in the VM?)"; exit 1; }

heartbeat_version() {
  local f fs; fs=$(find "$HOME/.local/share" -path '*/agent/heartbeat.json' 2>/dev/null); f=${fs%%$'\n'*}
  [ -n "$f" ] && sed -n 's/.*"version":"\([^"]*\)".*/\1/p' "$f" || true
}
wait_for_version() {
  local want="$1" deadline=$((SECONDS + 180))
  until [ "$(heartbeat_version)" = "$want" ]; do
    if [ $SECONDS -ge $deadline ]; then
      echo "::error::the agent does not run version $want (heartbeat says '$(heartbeat_version)')"
      systemctl --user status "$unit" --no-pager || true
      return 1
    fi
    sleep 2
  done
}
# The app's own view must agree with systemd: "running" (the agent answers), not just an active unit.
expect_running() {
  local st; st=$("$agent" status)
  grep -q '"condition": "running"' <<<"$st" || { echo "::error::the app does not see the agent running"; echo "$st"; return 1; }
}
cleanup() {
  [ -x "$agent" ] && "$agent" uninstall || true
  systemctl --user stop "$unit" 2>/dev/null || true
  rm -rf "$dest"
}
trap cleanup EXIT

# The expected versions are read from the heartbeats themselves: the builds are given as folders, so the versions are the
# ones the agent reports once it runs (PADDOCK_SMOKE_OLD_VERSION / PADDOCK_SMOKE_NEW_VERSION, set by the workflow).
v_old="${PADDOCK_SMOKE_OLD_VERSION:?the version of build N}"; v_new="${PADDOCK_SMOKE_NEW_VERSION:?the version of build N+1}"
[ "$v_old" != "$v_new" ] || { echo "::error::the two builds have the same version ($v_old)"; exit 1; }

mkdir -p "$(dirname "$dest")"
rm -rf "$dest" && cp -R "$old" "$dest"
"$agent" install
wait_for_version "$v_old"
expect_running
echo "build N ($v_old) runs"

rm -rf "$dest" && cp -R "$new" "$dest"
"$agent" check
wait_for_version "$v_new"
grep -qx active <<<"$(systemctl --user is-active "$unit")" || { echo "::error::the unit is not active"; systemctl --user status "$unit" --no-pager || true; exit 1; }
expect_running
echo "build N+1 ($v_new) runs after the update; the unit is active"

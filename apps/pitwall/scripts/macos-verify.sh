#!/usr/bin/env bash
# Checks a Pitwall app bundle on a Mac (docs/macos.md). Read-only on the system: it never installs, registers or
# changes anything outside a throw-away data folder.
#
#   bash scripts/macos-verify.sh [/path/to/Pitwall.app]            # the bundle, its launch agent plist, signing, rpath
#   bash scripts/macos-verify.sh [/path/to/Pitwall.app] --smoke    # also run the bundled server and the app headless
#   bash scripts/macos-verify.sh --installed                       # check an installed app and the running service
#
# The smoke test starts the bundled pitwall-server on a free port with PITWALL_HOME in a temp folder, waits for its
# discovery.json, then starts the app with --hidden (tray only, no window, so nothing opens on screen), keeps it for a
# few seconds, checks it stays alive, and stops both. (Adapted from Pitcrew's macos-verify.sh.)
set -uo pipefail

app=""
smoke=0
installed=0
for a in "$@"; do
  case "$a" in
    --smoke) smoke=1 ;;
    --installed) installed=1 ;;
    *) app="$a" ;;
  esac
done
[ -z "$app" ] && app="/Applications/Pitwall.app"
label=net.decryptology.pitwall.service
problems=0
ok() { printf '  \033[32mok\033[0m    %s\n' "$1"; }
bad() { printf '  \033[31mfail\033[0m  %s\n        %s\n' "$1" "$2"; problems=$((problems + 1)); }
note() { printf '  ..    %s\n' "$1"; }

echo "Pitwall bundle: $app"
[ -d "$app" ] && ok "app bundle" || { bad "app bundle" "not found at $app (build with cargo tauri build, or install from the .dmg)"; exit 1; }
server="$app/Contents/MacOS/pitwall-server"
main="$app/Contents/MacOS/pitwall-app"
plist="$app/Contents/Library/LaunchAgents/$label.plist"
[ -x "$main" ] && ok "app binary" || bad "app binary" "$main is missing"
[ -x "$server" ] && [ -s "$server" ] && ok "server binary in the bundle" || bad "server binary" "$server is missing or empty; rebuild (scripts/sidecar.mjs)"
[ -f "$plist" ] && ok "launch agent plist in the bundle" || bad "launch agent plist" "Contents/Library/LaunchAgents/$label.plist is missing; rebuild"
if [ -f "$plist" ]; then
  plutil -lint "$plist" >/dev/null 2>&1 && ok "plist is valid" || bad "plist" "plutil -lint failed"
  [ "$(plutil -extract BundleProgram raw "$plist" 2>/dev/null)" = "Contents/MacOS/pitwall-server" ] && ok "BundleProgram points at the bundled server" ||
    bad "BundleProgram" "must be Contents/MacOS/pitwall-server"
  [ "$(plutil -extract KeepAlive raw "$plist" 2>/dev/null)" = "true" ] && ok "KeepAlive restarts the server" || bad "KeepAlive" "must be true"
fi

# SMAppService refuses a bundle that is only linker-signed (-67056): it needs a sealed (ad-hoc or Developer ID) signature.
if codesign --verify --deep --strict "$app" >/dev/null 2>&1; then
  sig=$(codesign -dv "$app" 2>&1 | awk -F= '/^Signature/ {print $2}')
  ok "code signature is sealed (${sig:-valid})"
else
  bad "code signature" "codesign --verify failed; build with signingIdentity \"-\" (docs/macos.md)"
fi
if [ -x "$main" ] && command -v otool >/dev/null; then
  otool -l "$main" | grep -q "LC_RPATH" && ok "rpath present on the app binary" || note "no rpath on the app binary (fine unless native libs are bundled)"
fi
ver=$(plutil -extract LSMinimumSystemVersion raw "$app/Contents/Info.plist" 2>/dev/null)
[ -n "$ver" ] && ok "minimum macOS $ver (SMAppService needs 13)" || note "no LSMinimumSystemVersion"

if [ "$installed" = 1 ]; then
  if launchctl print "gui/$(id -u)/$label" >/tmp/pitwall-launchctl.$$ 2>&1; then
    state=$(awk -F'= ' '/^\tstate/ {print $2; exit}' /tmp/pitwall-launchctl.$$)
    [ "$state" = running ] && ok "launchd runs the agent" || bad "launch agent" "launchd knows it but its state is '${state:-?}'"
  else
    bad "launch agent" "not loaded: open Pitwall > Settings > Service > Turn on, and allow it in System Settings > General > Login Items"
  fi
  rm -f /tmp/pitwall-launchctl.$$
  support="$HOME/Library/Application Support/Pitwall"
  [ -f "$support/discovery.json" ] && ok "discovery.json exists ($support)" || bad "discovery.json" "the server is not running"
fi

if [ "$smoke" = 1 ] && [ -x "$server" ] && [ -x "$main" ]; then
  echo
  echo "Smoke test (throw-away data folder, hidden app)"
  home=$(mktemp -d "${TMPDIR:-/tmp}/pitwall-smoke.XXXXXX")
  port=$((20000 + RANDOM % 20000))
  spid=""; apid=""
  cleanup() { [ -n "$apid" ] && kill "$apid" 2>/dev/null; [ -n "$spid" ] && kill "$spid" 2>/dev/null; wait 2>/dev/null; rm -rf "$home"; }
  trap cleanup EXIT
  PITWALL_HOME="$home" "$server" --port "$port" --data-dir "$home" >"$home/server.log" 2>&1 &
  spid=$!
  for _ in $(seq 1 100); do [ -f "$home/discovery.json" ] && break; sleep 0.1; done
  if [ -f "$home/discovery.json" ]; then
    ok "bundled server wrote discovery.json"
    key=$(cat "$home/admin.key" 2>/dev/null)
    code=$(curl -s -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $key" "http://127.0.0.1:$port/mgmt/v1/rpc/overview")
    [ "$code" = 200 ] && ok "management API answers overview (200)" || bad "management API" "overview answered $code"
    code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$port/mgmt/v1/rpc/overview")
    [ "$code" = 401 ] && ok "management API refuses a call without the admin key (401)" || bad "management API auth" "a call without a key answered $code"
    PITWALL_HOME="$home" "$main" --hidden >"$home/app.log" 2>&1 &
    apid=$!
    sleep 6
    if kill -0 "$apid" 2>/dev/null; then ok "the app started hidden and stayed up for 6 s (pid $apid)"; else bad "app" "exited early; see: $(tail -n 3 "$home/app.log" | tr '\n' ' ')"; fi
    kill "$apid" 2>/dev/null; wait "$apid" 2>/dev/null; apid=""
    kill -0 "$spid" 2>/dev/null && ok "quitting the app leaves the server running" || bad "server" "died with the app"
  else
    bad "bundled server" "no discovery.json after 10 s: $(tail -n 3 "$home/server.log" | tr '\n' ' ')"
  fi
fi

echo
if [ "$problems" = 0 ]; then
  echo "Everything checked looks right."
else
  echo "$problems problem(s) found; docs/macos.md explains each step."
  exit 1
fi

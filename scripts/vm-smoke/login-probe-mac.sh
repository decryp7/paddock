#!/bin/bash
# Launch at login on macOS (Dyno 1.1.0): (A) first-run default of a fresh install (SMAppService.mainApp, read from Background Items with sfltool in the guest), and
# (B) the upgrade from 1.0: ~/Library/LaunchAgents/Dyno.plist (the file 1.0's autostart plugin wrote) is removed and the login item registered once.
# Usage: login-probe-mac.sh <new Dyno .dmg> <1.0.0 .dmg>   (fresh clones of the macOS test VM, removed at the end; --no-audio; guest only)
set -u
NEW=${1:?new dmg}; OLD=${2:?1.0.0 dmg}; HERE=$(cd "$(dirname "$0")" && pwd)
export TART_HOME=${TART_HOME:-/Volumes/d7sst7/dev-targets/tart-home}
BASE=${PADDOCK_MAC_BASE:-vmgui-base-tahoe-4aaa751f}
STAGE=$(mktemp -d /private/tmp/claude-501/login-mac-stage.XXXXXX); cp "$NEW" $STAGE/new.dmg; cp "$OLD" $STAGE/old.dmg
res() { echo "RESULT $1: $2 -- $3"; }
btm() { tart exec $VM bash -c 'sudo sfltool dumpbtm 2>&1' > /tmp/btm-$$.txt; node --input-type=module -e "Promise.all([import('$HERE/login-start-rows.mjs'),import('node:fs')]).then(([m,fs])=>{const p=m.parseBtm(fs.readFileSync('/tmp/btm-$$.txt','utf8'));console.log(JSON.stringify({present:p.present,count:p.count,total:p.total,raw:p.raw}))})" 2>&1 | tail -1; }
g() { tart exec $VM env PADDOCK_DISPOSABLE_VM=1 bash -c "$1" 2>&1; }
newvm() { VM=paddock-vmsmoke-mac-login-$1-$$; tart clone $BASE $VM || exit 2; tart run --no-audio --no-graphics --dir=stage:$STAGE:ro $VM >/dev/null 2>&1 & RUNPID=$!; for i in $(seq 75); do tart exec $VM true 2>/dev/null && break; sleep 4; done; }
dropvm() { tart stop $VM >/dev/null 2>&1; tart delete $VM >/dev/null 2>&1; kill $RUNPID 2>/dev/null; }
trap 'dropvm; rm -rf $STAGE /tmp/btm-$$.txt' EXIT
inst() { g "sudo spctl --global-enable; mp=/tmp/m-\$\$; mkdir -p \$mp; hdiutil attach -nobrowse -readonly -mountpoint \$mp \"/Volumes/My Shared Files/stage/$1\" >/dev/null; sudo rm -rf /Applications/Dyno.app; ditto \$mp/Dyno.app /Applications/Dyno.app; hdiutil detach \$mp >/dev/null; defaults read /Applications/Dyno.app/Contents/Info CFBundleShortVersionString"; }
ready() { for i in $(seq 40); do g '/Applications/Dyno.app/Contents/MacOS/dyno-agent status >/dev/null 2>&1' && return 0; sleep 4; done; return 1; }
LA='$HOME/Library/LaunchAgents'

echo "######## A: fresh install of the new build"
newvm a
echo "installed: $(inst new.dmg | tail -1)"
echo "LaunchAgents before the first start: $(g "ls $LA 2>&1")"
g 'open -a /Applications/Dyno.app'; ready; sleep 20
echo "LaunchAgents after the first start: $(g "ls $LA 2>&1")"
B=$(btm); echo "BTM: $B"
if echo "$B" | grep -q '"present":true' && echo "$B" | grep -q '"count":1'; then res default PASS "SMAppService login item registered once after the first start, Settings never opened: $B"; else res default FAIL "no single enabled Dyno login item after a fresh install and first start: $B (raw dump: /tmp/btm-$$.txt tail: $(tail -c 600 /tmp/btm-$$.txt | tr '\n' ' '))"; fi
g 'launchctl print gui/$(id -u)/net.decryptology.dyno.agent 2>&1 | head -4'
g 'ls $HOME/Library/LaunchAgents/Dyno.plist 2>&1' | grep -q "No such" && res no-legacy PASS "no Dyno.plist on a fresh install" || res no-legacy FAIL "Dyno.plist exists on a fresh install"
g "osascript -e 'quit app \"Dyno\"'; sleep 4; open -a /Applications/Dyno.app"; ready; sleep 10
B2=$(btm); echo "BTM after quit and reopen: $B2"
echo "$B2" | grep -q '"count":1' && res reopen PASS "still one login item after Quit and reopen" || res reopen FAIL "login item count changed after Quit and reopen: $B2"
dropvm

echo "######## B: upgrade 1.0 -> new"
newvm b
echo "installed: $(inst old.dmg | tail -1)"
g 'open -a /Applications/Dyno.app'; ready; sleep 20
echo "LaunchAgents after the 1.0 first start: $(g "ls $LA 2>&1")"
if ! g "test -f $LA/Dyno.plist && echo y" | grep -q y; then
  echo "1.0 did not write Dyno.plist on its own: seeding the exact file the autostart plugin wrote"
  g "mkdir -p $LA; cat > $LA/Dyno.plist <<'PL'
<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\">
<dict>
<key>Label</key>
<string>Dyno</string>
<key>AssociatedBundleIdentifiers</key>
<array>
</array>
<key>ProgramArguments</key>
<array>
<string>/Applications/Dyno.app/Contents/MacOS/dyno-app</string>
<string>--hidden</string>
</array>
<key>RunAtLoad</key>
<true/>
</dict>
</plist>
PL
plutil -lint $LA/Dyno.plist"
  SEEDED=yes
else SEEDED=no; echo "1.0 wrote Dyno.plist itself: $(g "cat $LA/Dyno.plist" | tr '\n' ' ' | cut -c1-300)"; fi
echo "BTM with 1.0 installed: $(btm)"
g "osascript -e 'quit app \"Dyno\"'; sleep 3; pkill -x dyno-app; pkill -x dyno-agent; true"
echo "installed: $(inst new.dmg | tail -1)"
g 'open -a /Applications/Dyno.app'; ready
GONE=no; for i in $(seq 30); do g "test -f $LA/Dyno.plist" >/dev/null 2>&1 || { GONE=yes; break; }; sleep 2; done
sleep 10; B3=$(btm); echo "BTM after the upgrade start: $B3"
[ "$GONE" = yes ] && res plist-removed PASS "Dyno.plist removed after the first start of the new build (seeded: $SEEDED)" || res plist-removed FAIL "Dyno.plist still there 60 s after the first start of the new build (seeded: $SEEDED)"
echo "$B3" | grep -q '"present":true' && echo "$B3" | grep -q '"count":1' && res login-item PASS "login item registered once: $B3" || res login-item FAIL "login item not registered exactly once: $B3"
g "osascript -e 'quit app \"Dyno\"'; sleep 4; open -a /Applications/Dyno.app"; ready; sleep 10
B4=$(btm); echo "BTM after quit and reopen: $B4"
if ! g "test -f $LA/Dyno.plist" >/dev/null 2>&1 && echo "$B4" | grep -q '"count":1'; then res replaced-once PASS "after Quit and reopen: no Dyno.plist and still one login item (replaced once, not again)"; else res replaced-once FAIL "after Quit and reopen: plist back or login item count changed: $B4"; fi
echo "app log lines about the login item:"; g 'cat "$HOME/Library/Logs/Dyno/"dyno*.log 2>/dev/null | grep -i -E "login|launch agent|SMAppService|legacy" | tail -n 12 | cut -c1-260'

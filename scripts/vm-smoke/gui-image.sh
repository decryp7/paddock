#!/usr/bin/env bash
# Builds or refreshes the GUI test image of `scripts/vm-smoke --gui` (README: "The GUI test image"). Local only; never in CI.
#
#   scripts/vm-smoke/gui-image.sh            build it if it does not exist
#   scripts/vm-smoke/gui-image.sh --refresh  build a new one beside it, check it, then replace the old one
#   scripts/vm-smoke/gui-image.sh --check    boot the existing image headless and run the image checks only
#
# The image is a local Tart VM named after the pinned base digest (run.mjs: MACOS_IMAGE, guiImageName). What the base already has
# (measured 2026-10-30, macOS 26.6.2): admin/admin auto-logged in, tart-guest-agent --run-agent in that Aqua session (so `tart exec`
# runs there), SIP off, Accessibility + ScreenCapture + PostEvent granted to tart-guest-agent, and Gatekeeper OFF. This adds:
# Gatekeeper on; AppleEvents grants for tart-guest-agent (System Events, Finder, Safari, System Settings) in admin's TCC.db; the
# screen-capture consent answered once; axdrive in /usr/local/bin; an image stamp. No keys, no tokens, nothing of the host.
# Same VM slot as run.mjs (./dev ci slot), at least PADDOCK_VM_MIN_FREE_GB (60) free, 1-minute load at most PADDOCK_VM_MAX_LOAD
# (16), nice 10, headless with CI=1 (Tart then never opens a viewer on the host). The build clone is deleted on failure.
# Guest commands are single-quoted on purpose: they expand in the guest.
# shellcheck disable=SC2016
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
export TART_HOME="${TART_HOME:-$HOME/.tart}"
export CI=1
tart="${TART:-tart}"
mode="${1:-build}"
case "$mode" in build | --refresh | --check) ;; *) echo "usage: gui-image.sh [--refresh|--check]" >&2; exit 2 ;; esac

base=$(cd "$here" && node -e "import('./run.mjs').then((m) => console.log(m.MACOS_IMAGE))")
name=$(cd "$here" && node -e "import('./run.mjs').then((m) => console.log(m.guiImageName(m.MACOS_IMAGE)))")
log() { echo "gui-image: $*" >&2; }
exists() { "$tart" list --quiet | grep -qx "$1"; }

if [ "$mode" = build ] && exists "$name"; then log "$name exists (use --refresh to rebuild it)"; exit 0; fi
if [ "$mode" = --check ] && ! exists "$name"; then log "$name does not exist; build it first"; exit 2; fi

tools=$("$here/gui/tools.sh")
tool_axdrive=$(sed -n 's/^axdrive=//p' <<<"$tools")
vm="$name"; [ "$mode" = --refresh ] && vm="$name-next"

owner=$(mktemp -d "${TMPDIR:-/tmp}/paddock-vm-owner-XXXX"); slot=""; token=""; run_pid=""; built=0
cleanup() {
  set +e
  [ -n "$run_pid" ] && { "$tart" stop "$vm" >/dev/null 2>&1; wait "$run_pid" 2>/dev/null; }
  if [ "$mode" != --check ] && [ $built = 0 ] && exists "$vm"; then log "deleting the unfinished $vm"; "$tart" delete "$vm"; fi
  [ -n "$slot" ] && (cd "$repo" && ./dev ci slot release --slot "$slot" --token "$token" --root "$TART_HOME/.paddock-vm" >/dev/null 2>&1)
  rm -rf "$owner"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

for _ in $(seq 0 6 "${PADDOCK_VM_SLOT_WAIT:-3600}"); do
  if out=$(cd "$repo" && ./dev ci slot acquire --max "${PADDOCK_VM_MAX:-1}" --owner "$owner" --root "$TART_HOME/.paddock-vm"); then
    read -r slot token <<<"$out"; break
  else
    rc=$?; [ $rc = 4 ] || { log "could not take a VM slot (paddock-dev exit $rc)"; exit 2; }
  fi
  log "another VM run holds this Mac's VM slot; waiting"; sleep 6
done
[ -n "$slot" ] || { log "the VM slot stayed held"; exit 2; }
free_gb=$(df -g "$TART_HOME" | awk 'NR==2 {print $4}')
[ "$free_gb" -ge "${PADDOCK_VM_MIN_FREE_GB:-60}" ] || { log "only $free_gb GB free under $TART_HOME"; exit 2; }
for _ in $(seq 0 6 "${PADDOCK_VM_LOAD_WAIT:-1800}"); do
  l=$(sysctl -n vm.loadavg | awk '{print int($2)}'); [ "$l" -le "${PADDOCK_VM_MAX_LOAD:-16}" ] && break
  log "load $l > ${PADDOCK_VM_MAX_LOAD:-16}; waiting"; sleep 6
done

if [ "$mode" != --check ]; then
  exists "$vm" && "$tart" delete "$vm"
  log "pulling and cloning the pinned base as $vm"
  nice -n 10 "$tart" pull "$base" >&2
  nice -n 10 "$tart" clone "$base" "$vm"
fi
nice -n 10 "$tart" run --no-graphics "$vm" >/dev/null 2>&1 &
run_pid=$!
for _ in $(seq 1 75); do "$tart" exec "$vm" true >/dev/null 2>&1 && break; sleep 4; done
X() { perl -e 'alarm shift; exec @ARGV' "${2:-300}" "$tart" exec "$vm" bash -c "$1"; }
X true 30 || { log "the guest agent did not answer"; exit 2; }

if [ "$mode" != --check ]; then
  "$tart" exec -i "$vm" bash -c 'cat > /tmp/axdrive' <"$tool_axdrive"
  X 'sudo install -m 0755 /tmp/axdrive /usr/local/bin/axdrive && rm /tmp/axdrive'
  X 'sudo spctl --global-enable'
  # Same row shape as the template's own osascript rows (client_type 1 = path, auth_value 2 = allowed); writable because SIP is off.
  X 'U="$HOME/Library/Application Support/com.apple.TCC/TCC.db"; A=$(realpath /opt/homebrew/bin/tart-guest-agent)
     for t in com.apple.systemevents com.apple.finder com.apple.Safari com.apple.systempreferences; do
       sqlite3 "$U" "insert or replace into access (service,client,client_type,auth_value,auth_reason,auth_version,indirect_object_identifier_type,indirect_object_identifier) values (\"kTCCServiceAppleEvents\",\"$A\",1,2,0,1,0,\"$t\")"
     done'
  # The macOS 15+ screen-capture consent ("bypass the system private window picker"): trigger it, answer Allow once.
  X 'screencapture -x /tmp/c.png; for i in $(seq 1 20); do axdrive has UserNotificationCenter "^Allow( \||$)" >/dev/null 2>&1 && axdrive press UserNotificationCenter "^Allow( \||$)" >/dev/null; sleep 0.5; done; rm -f /tmp/c.png'
  X "sudo mkdir -p /usr/local/share/paddock-vmgui && printf 'base %s\naxdrive %s\nbuilt %s\nmacos %s\n' '$base' '$(basename "$tool_axdrive")' '$(date -u +%Y-%m-%dT%H:%M:%SZ)' \"\$(sw_vers -productVersion)\" | sudo tee /usr/local/share/paddock-vmgui/image.txt >/dev/null"
fi

# The image checks (also --check): every one must hold, or the image is not used.
fail=0
chk() { if X "$2" 120 >/dev/null 2>&1; then log "ok    $1"; else log "FAIL  $1"; fail=1; fi; }
chk 'tart exec runs in the Aqua session' '[ "$(launchctl managername)" = Aqua ]'
chk 'Gatekeeper assessments enabled' '[ "$(spctl --status)" = "assessments enabled" ]'
chk 'axdrive is AX-trusted' 'axdrive trusted'
chk 'AppleEvents to System Events without a prompt (first launch of System Events is slow)' 'perl -e "alarm 90; exec @ARGV" osascript -e "tell application \"System Events\" to count processes"'
chk 'screencapture without a prompt' 'screencapture -x /tmp/c.png && rm /tmp/c.png'
chk 'no consent alert on screen' '! axdrive windows | grep -q "^UserNotificationCenter"'
chk 'TCC grant names the installed guest agent' 'U="$HOME/Library/Application Support/com.apple.TCC/TCC.db"; sqlite3 "$U" "select count(*) from access where client=\"$(realpath /opt/homebrew/bin/tart-guest-agent)\" and indirect_object_identifier=\"com.apple.systemevents\"" | grep -qx 1'
chk 'image stamp' 'cat /usr/local/share/paddock-vmgui/image.txt'
[ $fail = 0 ] || { log "the image checks failed"; exit 1; }

X 'sudo /sbin/shutdown -h now' 30 >/dev/null 2>&1 || true
wait "$run_pid" 2>/dev/null || true; run_pid=""
built=1
if [ "$mode" = --refresh ]; then
  exists "$name" && "$tart" delete "$name"
  "$tart" rename "$vm" "$name"
fi
log "ready: $name"

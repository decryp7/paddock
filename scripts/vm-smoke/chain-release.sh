#!/usr/bin/env bash
# The full release set of the VM checks for ONE CI run (local only, never CI). Usage:
#   scripts/vm-smoke/chain-release.sh <ci run id> [--expect-commit <12 hex>] [results folder]
# Runs, one after the other (a VM at a time; it takes the host's VM lock, scripts/vm-lock.sh, itself):
#   windows-ui            Windows per-machine install (a real UAC prompt answered over VNC), UI, helper, uninstall
#   windows-restart       Windows per-machine install and the real restart (autostart, helper service, tray)
#   linux-reboot          Linux with --reboot-check
#   macos-start           macOS start suite
#   macos-power           macOS power suite
#   macos-helper          macOS helper suite, without a restart
#   macos-helper-restart  macOS helper suite with --reboot-check
# Results go to <folder>/run-<id>/ (default ./vm-smoke-results/run-<id>/): one .md and one .json per run, UAC and actor screenshots in evidence/.
# A run that cannot finish does not stop the chain; the verdict at the end (release-verdict.mjs) is PASS only when ALL seven runs exist, are not
# partial and passed, for the same CI run and the same installer per OS. Exit: 0 PASS, 1 a product FAIL, 3 INCOMPLETE.
set -u
# Usage: chain-release.sh <ci run id> [--expect-commit <12 hex>] [results folder]
# The build binding is required: the commit comes from the CI run (Gitea API, needs GITEA_TOKEN) or from --expect-commit; without one the chain stops before any VM starts.
RUN="${1:?usage: chain-release.sh <ci run id> [--expect-commit <12 hex>] [results folder]}"; shift
COMMIT=""
if [ "${1:-}" = "--expect-commit" ]; then COMMIT="${2:?--expect-commit needs a value}"; shift 2; fi
OUT="${1:-${PADDOCK_VM_SMOKE_OUT:-./vm-smoke-results}}"
HERE="$(cd "$(dirname "$0")" && pwd)"
[[ "$RUN" =~ ^[0-9]+$ ]] || { echo "::error::the CI run id must be a number, got '$RUN'"; exit 2; }
if [ -z "$COMMIT" ]; then
  COMMIT="$(cd "$HERE" && node -e "import('./report.mjs').then((m) => process.stdout.write(m.commitOfRun(process.argv[1]) ?? ''))" "$RUN")"
fi
[[ "$COMMIT" =~ ^[0-9a-f]{12} ]] || { echo "::error::no build commit for CI run $RUN (the Gitea lookup needs GITEA_TOKEN: source ~/.zshenv); pass --expect-commit <12 hex>. Nothing was started."; exit 2; }
COMMIT="${COMMIT:0:12}"
# Windows: a snapshot with the session tools and NO Dyno (a per-user Dyno makes the per-machine installer refuse). Made once by make-no-dyno-snapshot.sh.
export PADDOCK_WIN_SNAPSHOT="${PADDOCK_WIN_SNAPSHOT:-gui-session-tools-no-dyno}"
VMRUN="/Applications/VMware Fusion.app/Contents/Public/vmrun"
VMX="$(node "$HERE/win-vm-target.mjs" --vmx)" || exit 2 # only paddock-win11-test, checked before vmrun (review P1-1)
"$VMRUN" listSnapshots "$VMX" 2>/dev/null | grep -qx "$PADDOCK_WIN_SNAPSHOT" || { echo "::error::the Windows snapshot '$PADDOCK_WIN_SNAPSHOT' does not exist: run scripts/vm-smoke/make-no-dyno-snapshot.sh under the VM lock first"; exit 2; }
FOLDER="$OUT/run-$RUN"
mkdir -p "$FOLDER"
# One VM at a time through the host's interim lock (scripts/vm-lock.sh), held for the whole chain: the steps and vm-win.sh inherit PADDOCK_VM_LOCK_HELD.
# The plock lane that used to wrap every step is gone: the Linux and macOS steps no longer queue behind plock (they use Tart's own VM slots, tools/macos-vm-smoke.sh),
# so while the chain runs they are covered only by this lock, which also serialises them against other chains.
. "$HERE/../vm-lock.sh"; vm_lock_acquire chain-release
step() { local label="$1"; shift; local t0=$SECONDS; echo "== $label: $*"; "$@"; local code=$?; echo "== $label exit $code after $((SECONDS - t0)) s"; [ "$code" -eq 0 ] || [ "$code" -eq 1 ] || [ "$code" -eq 3 ] || echo "::error::$label ended with exit $code (the tool could not run); read its log above"; }
step windows-ui           node "$HERE/run-win.mjs"   --app dyno --run "$RUN" --expect-commit "$COMMIT" --vnc --ui --out "$OUT"
step windows-restart      node "$HERE/run-win.mjs"   --app dyno --run "$RUN" --expect-commit "$COMMIT" --vnc --reboot-check --out "$OUT"
step linux-reboot         node "$HERE/run-linux.mjs" --app dyno --run "$RUN" --expect-commit "$COMMIT" --reboot-check --out "$OUT"
step macos-start          node "$HERE/run.mjs"       --app dyno --run "$RUN" --expect-commit "$COMMIT" --suite start --out "$OUT"
step macos-power          node "$HERE/run.mjs"       --app dyno --run "$RUN" --expect-commit "$COMMIT" --suite power --out "$OUT"
step macos-helper         node "$HERE/run.mjs"       --app dyno --run "$RUN" --expect-commit "$COMMIT" --suite helper --out "$OUT"
step macos-helper-restart node "$HERE/run.mjs"       --app dyno --run "$RUN" --expect-commit "$COMMIT" --suite helper --reboot-check --out "$OUT"
node "$HERE/release-verdict.mjs" "$FOLDER"

#!/usr/bin/env bash
# The full release set of the VM checks for ONE CI run (local only, never CI). Usage:
#   scripts/vm-smoke/chain-release.sh <ci run id> [--expect-commit <12 hex>] [results folder]
# Runs, one after the other (a VM at a time; hold the host's VM lock around this script):
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
VMX="${PADDOCK_WIN_VMX:-/Volumes/d7sst7/vms/paddock-win11-test/vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx}"
"$VMRUN" listSnapshots "$VMX" 2>/dev/null | grep -qx "$PADDOCK_WIN_SNAPSHOT" || { echo "::error::the Windows snapshot '$PADDOCK_WIN_SNAPSHOT' does not exist: run scripts/vm-smoke/make-no-dyno-snapshot.sh under the VM lock first"; exit 2; }
FOLDER="$OUT/run-$RUN"
mkdir -p "$FOLDER"
# One VM at a time through the host's lock (plock); without plock the steps run directly.
PLOCK="${PADDOCK_PLOCK:-/Volumes/d7sst7/dev-targets/scratch/pitcrew-lock/plock}"
export PLOCK_DIR="${PLOCK_DIR:-/private/tmp/claude-501/vm-lock}" PLOCK_PAR_LOAD="${PLOCK_PAR_LOAD:-0}"
step() { local label="$1"; shift; local t0=$SECONDS; echo "== $label: $*"; if [ -x "$PLOCK" ]; then "$PLOCK" run "vm-chain-$RUN-$label" -- "$@"; else "$@"; fi; local code=$?; echo "== $label exit $code after $((SECONDS - t0)) s"; [ "$code" -eq 0 ] || [ "$code" -eq 1 ] || [ "$code" -eq 3 ] || echo "::error::$label ended with exit $code (the tool could not run); read its log above"; }
step windows-ui           node "$HERE/run-win.mjs"   --app dyno --run "$RUN" --expect-commit "$COMMIT" --vnc --ui --out "$OUT"
step windows-restart      node "$HERE/run-win.mjs"   --app dyno --run "$RUN" --expect-commit "$COMMIT" --vnc --reboot-check --out "$OUT"
step linux-reboot         node "$HERE/run-linux.mjs" --app dyno --run "$RUN" --expect-commit "$COMMIT" --reboot-check --out "$OUT"
step macos-start          node "$HERE/run.mjs"       --app dyno --run "$RUN" --expect-commit "$COMMIT" --suite start --out "$OUT"
step macos-power          node "$HERE/run.mjs"       --app dyno --run "$RUN" --expect-commit "$COMMIT" --suite power --out "$OUT"
step macos-helper         node "$HERE/run.mjs"       --app dyno --run "$RUN" --expect-commit "$COMMIT" --suite helper --out "$OUT"
step macos-helper-restart node "$HERE/run.mjs"       --app dyno --run "$RUN" --expect-commit "$COMMIT" --suite helper --reboot-check --out "$OUT"
node "$HERE/release-verdict.mjs" "$FOLDER"

#!/usr/bin/env bash
# The full release set of the VM checks for ONE CI run (local only, never CI). Usage:
#   scripts/vm-smoke/chain-release.sh <ci run id> [results folder]
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
RUN="${1:?usage: chain-release.sh <ci run id> [results folder]}"
OUT="${2:-${PADDOCK_VM_SMOKE_OUT:-./vm-smoke-results}}"
HERE="$(cd "$(dirname "$0")" && pwd)"
[[ "$RUN" =~ ^[0-9]+$ ]] || { echo "::error::the CI run id must be a number, got '$RUN'"; exit 2; }
FOLDER="$OUT/run-$RUN"
mkdir -p "$FOLDER"
step() { local label="$1"; shift; local t0=$SECONDS; echo "== $label: $*"; "$@"; local code=$?; echo "== $label exit $code after $((SECONDS - t0)) s"; [ "$code" -eq 0 ] || [ "$code" -eq 1 ] || [ "$code" -eq 3 ] || echo "::error::$label ended with exit $code (the tool could not run); read its log above"; }
step windows-ui           node "$HERE/run-win.mjs"   --app dyno --run "$RUN" --vnc --ui --out "$OUT"
step windows-restart      node "$HERE/run-win.mjs"   --app dyno --run "$RUN" --vnc --reboot-check --out "$OUT"
step linux-reboot         node "$HERE/run-linux.mjs" --app dyno --run "$RUN" --reboot-check --out "$OUT"
step macos-start          node "$HERE/run.mjs"       --app dyno --run "$RUN" --suite start --out "$OUT"
step macos-power          node "$HERE/run.mjs"       --app dyno --run "$RUN" --suite power --out "$OUT"
step macos-helper         node "$HERE/run.mjs"       --app dyno --run "$RUN" --suite helper --out "$OUT"
step macos-helper-restart node "$HERE/run.mjs"       --app dyno --run "$RUN" --suite helper --reboot-check --out "$OUT"
node "$HERE/release-verdict.mjs" "$FOLDER"

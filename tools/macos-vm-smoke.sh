#!/usr/bin/env bash
# Runs a smoke that REGISTERS REAL SERVICES inside a throw-away VM on this Mac (Tart, Apple Virtualization.framework), so
# rule 7 holds: the developer's session and the shared runner are never touched (conformance C21, catalogue R03). One fresh
# VM per run, cloned from a pinned base image (copy-on-write, so cheap), booted headless, always stopped and deleted again
# (trap: success, failure, Ctrl-C, timeout). At most one VM at a time (a lock), disk and load are checked first.
#
#   bash paddock/tools/macos-vm-smoke.sh --old <N build> --new <N+1 build> [--guest macos|linux] [--smoke <script>]
#
# --guest macos (default): --old/--new are the two .app bundles; the smoke is scripts/shell-demo/smoke-service-update.sh.
# --guest linux: an arm64 Ubuntu guest with a real systemd --user session; --old/--new are folders holding the two builds'
#   linux-arm64 binaries; the smoke is scripts/shell-demo/smoke-service-update-linux.sh. (Same VM tool, another image.)
# The guest gets the staged files read-only through a virtio-fs share, copies them into its home and runs the smoke there with
# PADDOCK_DISPOSABLE_MAC=1 and PADDOCK_DISPOSABLE_VM=1; the smoke's output is the script's output and its exit code is ours.
#
# Environment: TART (default tart), TART_HOME (where the images live; the free-space check looks there),
#   PADDOCK_VM_IMAGE (override the pinned image), PADDOCK_VM_MIN_FREE_GB (60), PADDOCK_VM_MAX_LOAD (16),
#   PADDOCK_VM_LOAD_WAIT (seconds to wait for the load to fall, 1800), PADDOCK_VM_PULL_TIMEOUT (3600), PADDOCK_VM_BOOT_TIMEOUT (300),
#   PADDOCK_VM_RUN_TIMEOUT (1200), PADDOCK_VM_POLL (seconds between polls, 5), PADDOCK_SMOKE_* (forwarded to the guest), PADDOCK_VM_LOG_DIR (default: a folder of the run).
set -euo pipefail

MACOS_IMAGE="ghcr.io/cirruslabs/macos-tahoe-base@sha256:4aaa751fa7538902d73c767f27a066fc87e83f183069da4e4b5022863abd4c77"
LINUX_IMAGE="ghcr.io/cirruslabs/ubuntu@sha256:3acc11bebedcbcb813fb17dd0a4671a8c2e6baed537f3caa7922e170ed8a04ed" # 24.04
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

old=""; new=""; guest="macos"; smoke=""
while [ $# -gt 0 ]; do
  case "$1" in
    --old) old="${2:?--old needs a value}"; shift ;;
    --new) new="${2:?--new needs a value}"; shift ;;
    --guest) guest="${2:?--guest needs a value}"; shift ;;
    --smoke) smoke="${2:?--smoke needs a value}"; shift ;;
    *) echo "macos-vm-smoke: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
case "$guest" in
  macos) image="${PADDOCK_VM_IMAGE:-$MACOS_IMAGE}"; smoke="${smoke:-$here/../scripts/shell-demo/smoke-service-update.sh}"
         share="/Volumes/My Shared Files/stage" ;;
  linux) image="${PADDOCK_VM_IMAGE:-$LINUX_IMAGE}"; smoke="${smoke:-$here/../scripts/shell-demo/smoke-service-update-linux.sh}"
         share="/mnt/shared/stage" ;;
  *) echo "macos-vm-smoke: --guest must be macos or linux" >&2; exit 2 ;;
esac
[ -n "$old" ] && [ -n "$new" ] || { echo "macos-vm-smoke: --old and --new (the two builds) are needed" >&2; exit 2; }
[ -e "$old" ] || { echo "macos-vm-smoke: $old does not exist" >&2; exit 2; }
[ -e "$new" ] || { echo "macos-vm-smoke: $new does not exist" >&2; exit 2; }
[ -f "$smoke" ] || { echo "macos-vm-smoke: the smoke script $smoke does not exist" >&2; exit 2; }
[ "$(basename "$old")" = "$(basename "$new")" ] || { echo "macos-vm-smoke: --old and --new must have the same name (the same app)" >&2; exit 2; }

tart="${TART:-tart}"
command -v "$tart" >/dev/null 2>&1 || { echo "macos-vm-smoke: tart is needed (https://tart.run; brew install cirruslabs/cli/tart)" >&2; exit 1; }
tart_home="${TART_HOME:-$HOME/.tart}"
min_free="${PADDOCK_VM_MIN_FREE_GB:-60}"; max_load="${PADDOCK_VM_MAX_LOAD:-16}"; poll="${PADDOCK_VM_POLL:-5}"
vm="paddock-smoke-$guest-$(date +%Y%m%d%H%M%S)-$$"
run_pid=""; stage=""; lock=""; started_pull=0

log() { echo "macos-vm-smoke: $*" >&2; }

# run_to <seconds> <command...>: the command, killed (with its children) when it runs longer; 124 on timeout.
run_to() {
  local secs="$1"; shift
  "$@" & local pid=$!
  ( sleep "$secs"; kill "$pid" 2>/dev/null; sleep 2; kill -9 "$pid" 2>/dev/null ) >/dev/null 2>&1 & local watch=$!
  local rc=0; wait "$pid" 2>/dev/null || rc=$?
  if kill -0 "$watch" 2>/dev/null; then pkill -P "$watch" 2>/dev/null || true; kill "$watch" 2>/dev/null || true
  else rc=124; fi
  wait "$watch" 2>/dev/null || true
  return "$rc"
}

# Always: stop the VM, delete it, remove the staging folder and the lock. Never leaves a VM or its disk behind.
cleanup() {
  local rc=$?
  trap - EXIT INT TERM HUP
  pkill -P $$ 2>/dev/null || true # a smoke or poll still running when we were interrupted
  if [ -n "$run_pid" ]; then kill "$run_pid" 2>/dev/null || true; fi
  if "$tart" list --quiet 2>/dev/null | grep -qx "$vm"; then
    run_to 120 "$tart" stop "$vm" >/dev/null 2>&1 || true
    run_to 120 "$tart" delete "$vm" >/dev/null 2>&1 || log "WARNING: could not delete the VM $vm; delete it with: tart delete $vm"
  fi
  [ -z "$run_pid" ] || wait "$run_pid" 2>/dev/null || true
  [ -z "$stage" ] || rm -rf "$stage"
  [ -z "$lock" ] || rm -rf "$lock"
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

# --- one VM at a time -------------------------------------------------------------------------------------------------
mkdir -p "$tart_home"
lock="$tart_home/.paddock-vm-smoke.lock"
if ! mkdir "$lock" 2>/dev/null; then
  other=$(cat "$lock/pid" 2>/dev/null || true)
  if [ -n "$other" ] && kill -0 "$other" 2>/dev/null; then
    lock=""; log "another smoke VM run is active (pid $other); only one VM at a time"; exit 3
  fi
  log "removing the stale lock of a dead run ($other)"; rm -rf "$lock"; mkdir "$lock"
fi
echo $$ >"$lock/pid"

# --- disk and load ----------------------------------------------------------------------------------------------------
free_gb=$(df -k "$tart_home" | awk 'NR==2 {print int($4/1048576)}')
if [ "${free_gb:-0}" -lt "$min_free" ]; then
  log "only ${free_gb:-0} GB free under $tart_home; at least $min_free GB are needed (the base image is large). Free space or set TART_HOME elsewhere."
  exit 4
fi
load1() { uptime | sed 's/.*load averages*: *//' | awk '{gsub(",", ".", $1); print int($1)}'; }
waited=0; load_wait="${PADDOCK_VM_LOAD_WAIT:-1800}"
while [ "$(load1)" -gt "$max_load" ]; do
  [ "$waited" -lt "$load_wait" ] || { log "the 1-minute load stayed above $max_load for ${load_wait}s; not starting a VM"; exit 5; }
  log "load $(load1) > $max_load; waiting"; sleep "$poll"; waited=$((waited + poll + 1))
done

# --- the image (pinned by digest) ------------------------------------------------------------------------------------
log "image $image (pinned digest)"
run_to "${PADDOCK_VM_PULL_TIMEOUT:-3600}" "$tart" pull "$image" || { log "pulling the image failed or timed out"; exit 6; }

# --- stage the inputs (copy-on-write where the volume supports it) ------------------------------------------------------
stage="$(mktemp -d "${TMPDIR:-/tmp}/paddock-vm-stage-XXXXXX")"
mkdir -p "$stage/stage/old" "$stage/stage/new"
cp -R "$old" "$stage/stage/old/"; cp -R "$new" "$stage/stage/new/"
cp "$smoke" "$stage/stage/smoke.sh"
name="$(basename "$old")"

log "cloning $vm"
run_to 600 "$tart" clone "$image" "$vm" || { log "cloning the image failed"; exit 6; }
log "booting $vm headless"
"$tart" run --no-graphics --dir="stage:$stage/stage:ro" "$vm" >"$stage/tart-run.log" 2>&1 &
run_pid=$!

deadline=$((SECONDS + ${PADDOCK_VM_BOOT_TIMEOUT:-300}))
until run_to 30 "$tart" exec "$vm" true >/dev/null 2>&1; do
  kill -0 "$run_pid" 2>/dev/null || { log "the VM stopped while booting"; cat "$stage/tart-run.log" >&2; exit 7; }
  [ $SECONDS -lt $deadline ] || { log "the VM did not become ready in time"; exit 7; }
  sleep "$poll"
done
log "$vm is up"

# --- run the smoke in the guest ----------------------------------------------------------------------------------------
guest_script='set -euo pipefail
share="$1"; name="$2"; guest="$3"; shift 3
for kv in "$@"; do export "$kv"; done
if [ "$guest" = linux ]; then sudo mkdir -p /mnt/shared && (mountpoint -q /mnt/shared || sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared); fi
rm -rf "$HOME/smoke" && mkdir -p "$HOME/smoke" && cp -R "$share"/. "$HOME/smoke/"
export PADDOCK_DISPOSABLE_MAC=1 PADDOCK_DISPOSABLE_VM=1
exec bash "$HOME/smoke/smoke.sh" "$HOME/smoke/old/$name" "$HOME/smoke/new/$name"'
# PADDOCK_SMOKE_* variables of the caller (e.g. the two versions) are handed to the guest.
forward=(); while IFS= read -r kv; do forward+=("$kv"); done < <(env | grep '^PADDOCK_SMOKE_' || true)
rc=0
run_to "${PADDOCK_VM_RUN_TIMEOUT:-1200}" "$tart" exec "$vm" bash -c "$guest_script" _ "$share" "$name" "$guest" ${forward[@]+"${forward[@]}"} || rc=$?
case "$rc" in
  0) log "the smoke passed in $vm" ;;
  124) log "the smoke timed out" ;;
  *) log "the smoke failed (exit $rc)" ;;
esac
log_dir="${PADDOCK_VM_LOG_DIR:-}"
[ -z "$log_dir" ] || { mkdir -p "$log_dir"; cp "$stage/tart-run.log" "$log_dir/tart-run.log" 2>/dev/null || true; }
exit "$rc"

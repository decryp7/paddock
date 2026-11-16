#!/usr/bin/env bash
# Holds the TRAIN CI RESERVATION while a command runs: the integrator wraps a train's whole CI sequence in it, so no member run (a
# trial, the member gate, any dispatch that is not a train's) takes the gap between two workflows of the sequence (run 8166,
# 2026-11-16: it did, and train 12's Pitwall gate waited behind it on the macOS runner). `./dev ci dispatch` of a non-train ref and
# the member gate (`merge check --gates`) wait on it, bounded; a `train/*` dispatch never does. It is the machine's test-machines
# lock mechanism (scripts/vm-lock.sh: one kernel file lock, freed when the holder dies) on its own folder, <lock root>/train-ci
# (PADDOCK_TRAIN_CI_LOCK_ROOT overrides it). The lock has no time cap of its own, so wrap a sequence that ends (`timeout 8h`, as
# below): the hold is bounded by the command it wraps.
#
#   scripts/train-ci-hold.sh <label> -- <command...>          for example:
#   scripts/train-ci-hold.sh train-12 -- timeout 8h bash -c './dev ci dispatch pitwall.yml --ref train/x --max 30m && ./dev wait --ci-run <id> --max 3h'
#   scripts/train-ci-hold.sh --status                          the holder, if any
#
# Exit codes are the lock's: 75 when another train sequence holds it (a second sequence waits or reports), the command's own otherwise.
set -u
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -z "${PADDOCK_TRAIN_CI_LOCK_ROOT:-}" ]; then
  default="$(cd "$here/.." && bash ./dev places --get lock-root 2>/dev/null)" || { echo "::error::train-ci-hold: the lock folder cannot be derived. Next step: set PADDOCK_TRAIN_CI_LOCK_ROOT" >&2; exit 3; }
  PADDOCK_TRAIN_CI_LOCK_ROOT="$(dirname "$default")/train-ci"
fi
mkdir -p "$PADDOCK_TRAIN_CI_LOCK_ROOT" || exit 3
export PADDOCK_TRAIN_CI_LOCK_ROOT
export PADDOCK_VM_LOCK_ROOT="$PADDOCK_TRAIN_CI_LOCK_ROOT"
if [ "${1:-}" = "--status" ]; then
  cd "$here/.." && bash ./dev ci slot status --max 1 --root "$PADDOCK_TRAIN_CI_LOCK_ROOT"
  exit $?
fi
exec bash "$here/vm-lock-run.sh" "${1:?usage: train-ci-hold.sh <label> -- <command...>}" "${@:2}"

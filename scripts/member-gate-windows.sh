#!/usr/bin/env bash
# The Windows half of the other-os member gate (`./dev merge check --gates`, tools/paddock-dev/src/merge_check/other_os.rs): the
# member's affected nextest packages run on the test VM the way the Gitea Windows runner starts a step (`vm-win.sh run --ci-like`:
# runner-style temp folder, a fresh checkout with core.autocrlf=true, Git bash, CI env), under the test machines lock.
#
#   scripts/member-gate-windows.sh <tree> <spec>...      <tree>: the tree of HEAD; spec: `nextest <workspace> <package>`
#
# The guest has no Rust toolchain (docs/adr/amendments/0087-member-gate-other-os-2026-11-16.md compares the ways to give it one), so
# the test programs are built on the Mac: `cargo test --no-run` for aarch64-pc-windows-msvc through `cargo xwin env` (the guest is
# Arm: an x64 program, nextest.exe included, does not start there, exit 0xC0000135, measured 2026-11-16), and the guest runs each
# test in its own process, three at a time, the way nextest does (`<exe> --exact <test>`). Nextest's own features (test groups,
# retries, the per-test timeouts of .config/nextest.toml) are therefore not mirrored. A package whose tests run
# cargo (they read env!("CARGO"), a Mac path in a cross-built program) cannot run there: it is reported, not skipped, and the
# run is then "not measured" unless something failed.
#
# Exit: 0 passed, 100 tests failed, 75 not measured (the VM is busy or broken, a build failed, a package needs the toolchain).
# The control line `other-os-control: os=windows tree=<tree> ...` is printed by the guest command itself, from the guest's checkout.
# Environment: PADDOCK_MEMBER_GATE_SCRATCH (default <dev disk>/dev-targets/scratch/member-gate-windows).
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
tree="${1:?usage: member-gate-windows.sh <tree> <spec>...}"; shift
[ $# -gt 0 ] || { echo "::error::member-gate-windows: no spec given. Usage: member-gate-windows.sh <tree> <spec>..." >&2; exit 2; }
# The whole run holds the lock (busy is exit 75 with the holder named); a holder that started us passes it on.
if ! bash "$here/vm-lock-run.sh" --check; then
  exec bash "$here/vm-lock-run.sh" member-gate-windows -- bash "${BASH_SOURCE[0]}" "$tree" "$@"
fi
fail() { echo "::error::member-gate-windows: $1. Next step: $2" >&2; exit 75; }

[ "$(git -C "$root" rev-parse 'HEAD^{tree}')" = "$tree" ] || fail "HEAD's tree is not $tree" "run it from the worktree that holds the merge"
commit=$(git -C "$root" rev-parse HEAD)
scratch="${PADDOCK_MEMBER_GATE_SCRATCH:-/Volumes/vault-dev/dev-targets/scratch/member-gate-windows}"
mkdir -p "$scratch/archives" || fail "cannot make $scratch" "check the disk"
target=aarch64-pc-windows-msvc # the guest is Arm: an x64 program does not start there (exit 0xC0000135, measured 2026-11-16)

# Build the test programs on the Mac, per spec, with the xwin cross environment of that workspace.
declare -a archives=() names=() needs_toolchain=()
for spec in "$@"; do
  read -r kind ws pkg <<<"$spec"
  [ "$kind" = nextest ] || fail "unknown spec kind '$kind' (the Windows run takes nextest specs only)" "use nextest"
  case "$pkg" in
    paddock-dev) needs_toolchain+=("$pkg"); continue ;; # its tests run cargo: see the header
  esac
  echo "member-gate-windows: building the $pkg test programs for $target" >&2
  json="$scratch/archives/$pkg.json"
  (cd "$root/$ws" && eval "$(cargo xwin env --target $target 2>/dev/null)" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$scratch/target}/$(echo "$ws" | tr / -)" RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static" \
    nice -n 10 cargo test --locked --no-run --target "$target" -p "$pkg" --message-format=json >"$json") \
    || fail "the Windows build of $pkg failed (cargo test --no-run)" "fix the build, or run the same command by hand in $ws"
  while IFS= read -r exe; do archives+=("$exe"); names+=("$pkg"); done < <(python3 -c '
import json,sys
for l in open(sys.argv[1]):
    try: m=json.loads(l)
    except ValueError: continue
    if m.get("reason")=="compiler-artifact" and m.get("executable") and m.get("profile",{}).get("test"): print(m["executable"])' "$json")
done

failed=0; ran=0
if [ ${#archives[@]} -gt 0 ]; then
  "$here/vm-win.sh" revert >&2 || fail "cannot revert the VM to clean-baseline" "see docs/testing/windows-test-vm.md"
  trap '"$here/vm-win.sh" stop >&2 || true; "$here/vm-win.sh" revert >&2 || true' EXIT
  "$here/vm-win.sh" start >&2 || fail "the VM did not start" "see docs/testing/windows-test-vm.md"
  withs=()
  for a in "${archives[@]}"; do withs+=(--with "$a"); done
  # In the guest (Git bash, runner-like): the control line from the guest's own checkout, then each archive through nextest.
  step='set +e
d=$(cygpath -u "$CI_LIKE_DIR")
echo "other-os-control: os=windows tree=$(git rev-parse HEAD^{tree}) autocrlf=$(git config core.autocrlf) shell=bash temp=$TEMP ci-env=${CI:-unset}"
res=$(mktemp)
run_one() { out=$("$1" --exact "$2" --test-threads 1 2>&1); c=$?; if [ $c = 0 ] && printf "%s" "$out" | grep -q " 1 passed"; then echo "PASS $2" >> "$3"; else echo "FAIL $2 (exit $c)" >> "$3"; printf "%s\n" "$out" | sed "s/^/    /"; fi; }
export -f run_one
for exe in "$d"/*.exe; do
  echo "::group::$(basename "$exe")"
  list=$("$exe" --list --format terse 2>&1); lc=$?
  if [ "$lc" != 0 ] || [ -z "$list" ]; then echo "FAIL $(basename "$exe") --list (exit $lc): $list" | tee -a "$res"; continue; fi
  while IFS= read -r t; do
    run_one "$exe" "$t" "$res" &
    while [ "$(jobs -r | wc -l)" -ge 3 ]; do wait -n; done
  done < <(printf "%s\n" "$list" | sed -n "s/: test\$//p")
  wait
  echo "::endgroup::"
done
pass=$(grep -c "^PASS" "$res" || true); fail=$(grep -c "^FAIL" "$res" || true)
grep "^FAIL" "$res" || true
echo "     Summary [   0.0s] $((pass + fail)) tests run: $pass passed, $fail failed"
echo "other-os-result: failed=$fail"'
  # The verdict travels as a line, not as an exit code: the ci-like wrapper turns the guest's exit 100 into 1.
  log="$scratch/guest.log"
  "$here/vm-win.sh" run --ci-like --ref "$commit" "${withs[@]}" -- "$step" 2>&1 | tee "$log"
  rc=${PIPESTATUS[0]}
  ran=${#archives[@]}
  [ "$rc" = 0 ] || { echo "::error::member-gate-windows: the guest run exited $rc, not a verdict" >&2; exit 75; }
  case "$(tr -d "\r" <"$log" | sed -n 's/^other-os-result: failed=\([0-9]*\)$/\1/p' | tail -1)" in
    0) ;; "") echo "::error::member-gate-windows: the guest printed no other-os-result line" >&2; exit 75 ;;
    *) failed=1 ;;
  esac
fi
if [ "$failed" = 1 ]; then exit 100; fi
if [ ${#needs_toolchain[@]} -gt 0 ]; then
  echo "::warning::member-gate-windows: ${needs_toolchain[*]} not run: its tests run cargo and the guest has no pinned toolchain (docs/adr/amendments/0087-member-gate-other-os-2026-11-16.md). $ran other package(s) passed" >&2
  exit 75
fi
exit 0

#!/usr/bin/env bash
# The Windows half of the other-os member gate (`./dev merge check --gates`, tools/paddock-dev/src/merge_check/other_os.rs): the
# member's affected nextest packages run on the test VM the way the Gitea Windows runner starts a step (`vm-win.sh run --ci-like`:
# runner-style temp folder, a fresh checkout with core.autocrlf=true, Git bash, CI env), under the test machines lock.
#
#   scripts/member-gate-windows.sh <tree> <spec>...      <tree>: the tree of HEAD; spec: `nextest <workspace> <package>`
#
# The guest has no Rust toolchain (docs/adr/amendments/0087-member-gate-other-os-2026-11-16.md compares the ways to give it one), so
# the test programs are built on the Mac: `cargo nextest archive` for x86_64-pc-windows-msvc through `cargo xwin env` (the CI
# runner's architecture; the Arm guest runs it under Prism), and the pinned cargo-nextest.exe (the version and SHA-256 of
# tools/paddock-dev/src/ci_host/host_tools.rs, `dev ci host-tools --print-pin`) runs the archive in the guest. A package whose tests run
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
target=x86_64-pc-windows-msvc

# The pinned cargo-nextest.exe: the release the runners keep (`./dev ci host-tools-pin` prints its URL and SHA-256), fetched once into the
# scratch folder and verified every time.
read -r nx_version nx_url nx_sum nx_bin < <(cd "$root" && ./dev ci host-tools-pin) || fail "cannot read the nextest pin" "run ./dev ci host-tools-pin"
nextest_exe="$scratch/nextest-$nx_version/$nx_bin"
if [ ! -f "$nextest_exe" ]; then
  mkdir -p "$scratch/nextest-$nx_version" && curl -fsSL --retry 3 -o "$scratch/nextest-$nx_version/pkg.zip" "$nx_url" \
    || fail "cannot download $nx_url" "check the network, or copy the release to $scratch/nextest-$nx_version/pkg.zip"
  got=$(shasum -a 256 "$scratch/nextest-$nx_version/pkg.zip" | awk '{print $1}')
  [ "$got" = "$nx_sum" ] || { rm -f "$scratch/nextest-$nx_version/pkg.zip"; fail "cargo-nextest $nx_version checksum mismatch (got $got)" "the pin in host_tools.rs is the reference"; }
  (cd "$scratch/nextest-$nx_version" && unzip -q -o pkg.zip "$nx_bin") || fail "cannot unpack the nextest release" "check the archive"
fi

# Build the archives on the Mac, one per spec, with the xwin cross environment of that workspace.
declare -a archives=() names=() needs_toolchain=()
for spec in "$@"; do
  read -r kind ws pkg <<<"$spec"
  [ "$kind" = nextest ] || fail "unknown spec kind '$kind' (the Windows run takes nextest specs only)" "use nextest"
  case "$pkg" in
    paddock-dev) needs_toolchain+=("$pkg"); continue ;; # its tests run cargo: see the header
  esac
  out="$scratch/archives/$pkg.tar.zst"
  echo "member-gate-windows: building the $pkg archive for $target" >&2
  (cd "$root/$ws" && eval "$(cargo xwin env --target $target 2>/dev/null)" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$scratch/target}/$(echo "$ws" | tr / -)" \
    nice -n 10 cargo nextest archive --locked --target "$target" -p "$pkg" --archive-file "$out" >&2) \
    || fail "the Windows build of $pkg failed (cargo nextest archive)" "fix the build, or run the same command by hand in $ws"
  archives+=("$out"); names+=("$pkg")
done

failed=0; ran=0
if [ ${#archives[@]} -gt 0 ]; then
  "$here/vm-win.sh" revert >&2 || fail "cannot revert the VM to clean-baseline" "see docs/testing/windows-test-vm.md"
  trap '"$here/vm-win.sh" stop >&2 || true; "$here/vm-win.sh" revert >&2 || true' EXIT
  "$here/vm-win.sh" start >&2 || fail "the VM did not start" "see docs/testing/windows-test-vm.md"
  withs=(--with "$nextest_exe")
  for a in "${archives[@]}"; do withs+=(--with "$a"); done
  # In the guest (Git bash, runner-like): the control line from the guest's own checkout, then each archive through nextest.
  step='d=$(cygpath -u "$CI_LIKE_DIR")
echo "other-os-control: os=windows tree=$(git rev-parse HEAD^{tree}) autocrlf=$(git config core.autocrlf) shell=bash temp=$TEMP ci-env=${CI:-unset}"
rc=0
for a in "$d"/*.tar.zst; do
  echo "::group::nextest archive $(basename "$a")"
  "$d/cargo-nextest.exe" nextest run --archive-file "$a" --workspace-remap "$(pwd)" --profile ci --no-fail-fast || { c=$?; [ "$c" = 100 ] && rc=100 || { [ "$rc" = 100 ] || rc=$c; }; }
  echo "::endgroup::"
done
exit $rc'
  "$here/vm-win.sh" run --ci-like --ref "$commit" "${withs[@]}" -- "$step"
  rc=$?
  ran=${#archives[@]}
  case "$rc" in 0) ;; 100) failed=1 ;; *) echo "::error::member-gate-windows: the guest run exited $rc (not 0 or 100)" >&2; exit 75 ;; esac
fi
if [ "$failed" = 1 ]; then exit 100; fi
if [ ${#needs_toolchain[@]} -gt 0 ]; then
  echo "::warning::member-gate-windows: ${needs_toolchain[*]} not run: its tests run cargo and the guest has no pinned toolchain (docs/adr/amendments/0087-member-gate-other-os-2026-11-16.md). $ran other package(s) passed" >&2
  exit 75
fi
exit 0

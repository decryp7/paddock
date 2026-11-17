#!/usr/bin/env bash
# MANUAL USE ONLY. The other-os member gate (`./dev merge check --gates`, tools/paddock-dev/src/merge_check/other_os.rs) runs on the
# real CI runners since the user's decision of 2026-11-16; this script no longer belongs to it. It stays to run one package
# natively on the Windows test VM by hand (for a Windows-only investigation, with the guest's own differences: Arm, Prism, a
# runner-style PATH that is not the runner's). Its packages build and run NATIVELY on the test VM the way the Gitea Windows runner starts a step
# (`vm-win.sh run --ci-like`: runner-style temp folder, a fresh checkout with core.autocrlf=true, Git bash, CI env), under the test
# machines lock. Tests that run cargo (paddock-dev's) therefore work: the guest has the pinned Rust toolchain.
#
#   scripts/member-gate-windows.sh <tree> <spec>...      <tree>: the tree of HEAD; spec: `nextest <workspace> <package> [filter]`
#
# What is where (docs/adr/amendments/0087-member-gate-other-os-2026-11-16.md): the MSVC Build Tools are in the VM image, snapshot
# `buildtools-baseline` (`vm-win.sh toolchain-setup`, once). The Rust toolchain of rust-toolchain.toml is copied in per run from a
# host-side cache (scripts/vm-win/member-gate-tools.lock) with the pinned aarch64 cargo-nextest; no rustup runs in the guest, so
# nothing can auto-install a toolchain, and the registries are read anonymously (.cargo/config.toml).
# The guest is Arm, the runner is x64: the native Arm rustc builds x86_64-pc-windows-msvc by default (the runner's architecture; the
# programs run under Prism emulation, only cargo-nextest.exe is the native Arm build), the control line says so, and
# PADDOCK_MEMBER_GATE_WIN_TARGET=aarch64-pc-windows-msvc builds natively.
#
# Exit: 0 passed, 100 tests failed, 75 not measured (the VM is busy or broken, the toolchain could not be fetched or unpacked).
# The control line `other-os-control: os=windows tree=<tree> ...` and the verdict line `other-os-result: failed=<n>` are printed by
# the guest command itself (the ci-like wrapper turns a guest exit 100 into 1, so the verdict travels as a line).
# Environment: PADDOCK_MEMBER_GATE_SCRATCH (default <dev disk>/dev-targets/scratch/member-gate-windows), PADDOCK_MEMBER_GATE_SNAPSHOT
# (default buildtools-baseline).
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
lock() { sed -n "s/^$1=//p" "$here/vm-win/member-gate-tools.lock" | head -1 | tr -d "\""; }

[ "$(git -C "$root" rev-parse 'HEAD^{tree}')" = "$tree" ] || fail "HEAD's tree is not $tree" "run it from the worktree that holds the merge"
commit=$(git -C "$root" rev-parse HEAD)
scratch="${PADDOCK_MEMBER_GATE_SCRATCH:-/Volumes/vault-dev/dev-targets/scratch/member-gate-windows}"
cache="$scratch/cache"; mkdir -p "$cache" || fail "cannot make $cache" "check the disk"
snapshot="${PADDOCK_MEMBER_GATE_SNAPSHOT:-buildtools-baseline}"

# Every spec is native now; the only thing to check is the shape.
for spec in "$@"; do
  read -r kind _ _ <<<"$spec"
  [ "$kind" = nextest ] || fail "unknown spec kind '$kind' (the Windows run takes nextest specs only)" "use nextest"
done

# The host-side cache: the toolchain of rust-toolchain.toml as one archive, and the aarch64 cargo-nextest.exe.
channel=$(python3 -I -c 'import sys,tomllib; print(tomllib.load(open(sys.argv[1],"rb"))["toolchain"]["channel"])' "$root/rust-toolchain.toml") \
  || fail "cannot read the channel of rust-toolchain.toml" "check the file"
case "$channel" in [0-9]*.[0-9]*.[0-9]*) ;; *) fail "rust-toolchain.toml names '$channel', not an exact version" "pin an exact version (ER-26)" ;; esac
host=$(lock rust_host); dist=$(lock rust_dist)
gate_target="${PADDOCK_MEMBER_GATE_WIN_TARGET:-$(lock gate_target)}"
prefix_tgz="$cache/rust-prefix-$channel-$host-std-$(lock rust_extra_std | tr " " "+").tar.gz"
if [ ! -f "$prefix_tgz" ]; then
  echo "member-gate-windows: filling the host cache with Rust $channel for $host" >&2
  dl="$cache/rust-$channel-$host.tar.xz"; work="$cache/unpack-$$"
  curl -fsSL --retry 3 -o "$dl" "$dist/rust-$channel-$host.tar.xz" && want=$(curl -fsSL "$dist/rust-$channel-$host.tar.xz.sha256" | awk '{print $1}') \
    || fail "cannot download Rust $channel from $dist" "check the network"
  [ "$(shasum -a 256 "$dl" | awk '{print $1}')" = "$want" ] || { rm -f "$dl"; fail "the Rust $channel archive does not match $dist's .sha256" "try again; if it repeats, the dist changed"; }
  mkdir -p "$work/prefix" && tar -xf "$dl" -C "$work" || fail "cannot unpack the Rust archive" "check the disk"
  for c in $(lock rust_components); do cp -R "$work/rust-$channel-$host/$c/." "$work/prefix/" || fail "the Rust archive has no component $c" "check rust_components in member-gate-tools.lock"; done
  for x in $(lock rust_extra_std); do
    xdl="$cache/$x-$channel.tar.xz"; xt=${x#rust-std-}
    curl -fsSL --retry 3 -o "$xdl" "$dist/rust-std-$channel-$xt.tar.xz" && xwant=$(curl -fsSL "$dist/rust-std-$channel-$xt.tar.xz.sha256" | awk '{print $1}') || fail "cannot download $x" "check the network"
    [ "$(shasum -a 256 "$xdl" | awk '{print $1}')" = "$xwant" ] || { rm -f "$xdl"; fail "$x does not match its .sha256" "try again"; }
    tar -xf "$xdl" -C "$work" && cp -R "$work/rust-std-$channel-$xt/$x/." "$work/prefix/" || fail "cannot unpack $x" "check the archive"
    rm -f "$xdl"
  done
  find "$work/prefix" \( -name '*.pdb' -o -name 'manifest.in' \) -delete
  tar -czf "$prefix_tgz.part" -C "$work/prefix" . && mv "$prefix_tgz.part" "$prefix_tgz" || fail "cannot pack the toolchain" "check the disk"
  rm -rf "$work" "$dl"
fi
nx_zip="$cache/cargo-nextest-$(lock nextest_version)-aarch64.zip"
if [ ! -f "$nx_zip" ]; then
  curl -fsSL --retry 3 -o "$nx_zip" "$(lock nextest_url)" || fail "cannot download $(lock nextest_url)" "check the network"
fi
[ "$(shasum -a 256 "$nx_zip" | awk '{print $1}')" = "$(lock nextest_sha256)" ] || { rm -f "$nx_zip"; fail "cargo-nextest does not match the pinned SHA-256" "the lock file is the reference"; }
nx_exe="$cache/nextest-aarch64/cargo-nextest.exe"
[ -f "$nx_exe" ] || { mkdir -p "$cache/nextest-aarch64" && unzip -q -o "$nx_zip" cargo-nextest.exe -d "$cache/nextest-aarch64" || fail "cannot unpack cargo-nextest" "check the archive"; }

# Node, from the same kind of cache: the exact release the lock file pins, whose major must be .nvmrc's.
[ "$(lock node_version | cut -d. -f1)" = "$(tr -d '[:space:]v' <"$root/.nvmrc")" ] || fail "the pinned Node $(lock node_version) is not .nvmrc's major" "bump node_version in scripts/vm-win/member-gate-tools.lock"
node_zip="$cache/node-$(lock node_version)-win-arm64.zip"
[ -f "$node_zip" ] || curl -fsSL --retry 3 -o "$node_zip" "$(lock node_url)" || fail "cannot download $(lock node_url)" "check the network"
[ "$(shasum -a 256 "$node_zip" | awk '{print $1}')" = "$(lock node_sha256)" ] || { rm -f "$node_zip"; fail "Node does not match the pinned SHA-256" "the lock file is the reference"; }

"$here/vm-win.sh" revert --snapshot "$snapshot" >&2 || fail "cannot revert the VM to $snapshot (is vm-win.sh toolchain-setup done?)" "see docs/testing/windows-test-vm.md"
trap '"$here/vm-win.sh" stop >&2 || true; "$here/vm-win.sh" revert >&2 || true' EXIT
t0=$SECONDS
"$here/vm-win.sh" start >&2 || fail "the VM did not start" "see docs/testing/windows-test-vm.md"
echo "member-gate-windows: VM started from $snapshot in $((SECONDS - t0)) s" >&2

# In the guest (Git bash, runner-like): control line, the toolchain unpacked from what was copied in, then each spec.
specs=$(printf '%s\n' "$@")
step='set +e
d=$(cygpath -u "$CI_LIKE_DIR")
echo "other-os-control: os=windows tree=$(git rev-parse HEAD^{tree}) autocrlf=$(git config core.autocrlf) shell=bash temp=$TEMP ci-env=${CI:-unset} target=$TARGET vm-arch=aarch64 runner-arch=x86_64"
mkdir -p "$d/rust" && tar -xf "$d"/rust-prefix-*.tar.gz -C "$d/rust" || { echo "other-os-fatal: cannot unpack the toolchain"; echo "other-os-result: failed=-1"; exit 0; }
mkdir -p "$d/node" && /c/Windows/System32/tar.exe -xf "$(cygpath -w "$(ls "$d"/node-*-win-arm64.zip)")" -C "$(cygpath -w "$d/node")" --strip-components 1 || { echo "other-os-fatal: cannot unpack node"; echo "other-os-result: failed=-1"; exit 0; }
export CARGO_BUILD_TARGET="$TARGET" PATH="$d/rust/bin:$d/node:$d:$PATH" CARGO_HOME="$d/cargo-home" CARGO_TARGET_DIR="$d/target" RUSTUP_AUTO_INSTALL=0 PADDOCK_JOB_TEMP="$TEMP"
echo "other-os-guest: $(rustc --version) / $(cargo --version) / $(cargo-nextest.exe nextest --version | head -1) / node $(node --version) / link=$(command -v link.exe || echo none)"
fail=0
while IFS= read -r spec; do
  read -r kind ws pkg filter <<<"$spec"
  echo "::group::nextest $ws $pkg"
  # The other tools whose cargo metadata the paddock-dev tests read offline (ci.yml: "Fetch the other tools dependencies").
  if [ "$pkg" = paddock-dev ]; then for tl in heavy wiki; do (cd "tools/$tl" && cargo fetch --locked >/dev/null 2>&1) || echo "other-os-warn: cargo fetch --locked failed in tools/$tl"; done; fi
  (cd "$ws" && cargo nextest run -p "$pkg" --profile ci --locked --no-fail-fast ${filter:+-E "$filter"} 2>&1)
  c=$?
  echo "::endgroup::"
  case "$c" in 0) ;; 100) fail=1 ;; *) echo "other-os-fatal: $pkg exited $c (a build or setup failure, not failed tests)"; fail=-1 ;; esac
done <<<"$SPECS"
echo "other-os-result: failed=$fail"'
step="SPECS='$specs'; TARGET='$gate_target'; WARM='${PADDOCK_MEMBER_GATE_MEASURE_WARM:-0}'; export CARGO_TERM_COLOR=never NO_COLOR=1
$step"
log="$scratch/guest.log"
t1=$SECONDS
timeout "${PADDOCK_MEMBER_GATE_WIN_MAX:-2700}" "$here/vm-win.sh" run --ci-like --ref "$commit" --tools-provided --with "$prefix_tgz" --with "$nx_exe" --with "$node_zip" -- "$step" 2>&1 | tee "$log"
rc=${PIPESTATUS[0]}
echo "member-gate-windows: guest run took $((SECONDS - t1)) s" >&2
[ "$rc" != 124 ] || { echo "::error::member-gate-windows: the guest run was ended after ${PADDOCK_MEMBER_GATE_WIN_MAX:-2700} s (a hang or a starved guest). Next step: read the output above; set PADDOCK_MEMBER_GATE_WIN_MAX to allow longer" >&2; exit 75; }
[ "$rc" = 0 ] || { echo "::error::member-gate-windows: the guest run exited $rc, not a verdict" >&2; exit 75; }
case "$(tr -d '\r' <"$log" | sed -n 's/^other-os-result: failed=\(-*[0-9]*\)$/\1/p' | tail -1)" in
  0) exit 0 ;;
  "") echo "::error::member-gate-windows: the guest printed no other-os-result line" >&2; exit 75 ;;
  -1) echo "::error::member-gate-windows: the guest could not set up or build (see other-os-fatal above)" >&2; exit 75 ;;
  *) exit 100 ;;
esac

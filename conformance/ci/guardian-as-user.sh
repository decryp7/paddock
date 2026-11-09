#!/usr/bin/env bash
# Runs the orphan guardian's acting tests as a non-root user (ADR 0076 amendment 2026-11-09). The Linux CI containers run as
# root, and the guardian is off for root, so the crates job alone never runs the guardian's Linux code (`exe_of` through
# /proc, `owner_holds_pipe`) in acting mode: as root those tests assert the root behaviour instead.
#
#   conformance/ci/guardian-as-user.sh     run from crates/; builds the paddock-proc and paddock-chromium unit tests (all
#                                          features), then runs the guardian tests and paddock-chromium's owner-killed test
#                                          as PADDOCK_TEST_UID (default 65534, nobody) through setpriv, with a private TMPDIR
#                                          and HOME. Run as root; run by a user, it runs them directly.
#
# The ci.yml step for the integrator (Linux leg of the crates job, after the workspace tests):
#
#   - name: Orphan guardian as a user (Linux)
#     if: ${{ !cancelled() && runner.os == 'Linux' }}
#     working-directory: ${{ env.PADDOCK_WORKSPACE || github.workspace }}/crates
#     run: bash ../conformance/ci/guardian-as-user.sh
#
# Each failure prints one ::error:: line with the command, its exit code and the next step (ER-27); the job summary says
# what ran.
set -euo pipefail

uid="${PADDOCK_TEST_UID:-65534}"
summary="${GITHUB_STEP_SUMMARY:-/dev/null}"

fail() {
  echo "::error::guardian-as-user: $1"
  exit 1
}

if [ "$(id -u)" = 0 ]; then
  command -v setpriv >/dev/null || fail "setpriv (util-linux) is missing, so the tests cannot run as uid $uid; install util-linux in the image"
  as_user=(setpriv --reuid="$uid" --regid="$uid" --clear-groups --)
else
  uid="$(id -u)"
  as_user=()
fi

echo "Building the paddock-proc and paddock-chromium unit tests"
build_log="$(mktemp)"
if ! cargo test --no-run --all-features -p paddock-proc -p paddock-chromium >"$build_log" 2>&1; then
  tail -n 40 "$build_log"
  fail "cargo test --no-run failed (output above); fix the build first"
fi
# "Executable unittests src/lib.rs (target/.../deps/paddock_proc-<hash>)"
exe_of() { sed -n "s|.*Executable unittests src/lib.rs (\(.*/$1-[0-9a-f]*\))\$|\1|p" "$build_log" | tail -n 1; }
proc_bin="$(exe_of paddock_proc)"
chromium_bin="$(exe_of paddock_chromium)"
[ -n "$proc_bin" ] && [ -n "$chromium_bin" ] || fail "the unit test binaries were not found in cargo's output; see $build_log"

private="$(mktemp -d)"
trap 'rm -rf "$private"' EXIT
[ "${#as_user[@]}" -eq 0 ] || chown "$uid:$uid" "$private"
chmod 700 "$private"

run() {
  local bin="$1" filter="$2" started=$SECONDS code=0
  local cmd=("${as_user[@]}" env "TMPDIR=$private" "HOME=$private" "$bin" "$filter")
  echo "Running as uid $uid: ${cmd[*]}"
  "${cmd[@]}" || code=$?
  local took=$((SECONDS - started))
  if [ "$code" != 0 ]; then
    echo "| \`$filter\` | uid $uid | failed (exit $code) | ${took} s |" >>"$summary"
    fail "\`$filter\` failed as uid $uid (exit $code, ${took} s): ${cmd[*]}. Run the same command in the CI image to see it"
  fi
  echo "| \`$filter\` | uid $uid | passed | ${took} s |" >>"$summary"
}

{
  echo "### Orphan guardian as a user"
  echo
  echo "| Tests | As | Result | Time |"
  echo "|---|---|---|---|"
} >>"$summary"
run "$proc_bin" "guardian::"
run "$chromium_bin" "launch::tests::a_browser_whose_owner_is_killed"
echo "The guardian's acting tests passed as uid $uid"

#!/usr/bin/env bash
# Runs the orphan guardian's acting tests as a non-root user (ADR 0076 amendment 2026-11-09). The Linux CI containers run as
# root, and the guardian is off for root, so the crates job alone never runs the guardian's Linux code (`exe_of` through
# /proc, `owner_holds_pipe`) in acting mode: as root those tests assert the root behaviour instead.
#
#   conformance/ci/guardian-as-user.sh     run from crates/; archives the paddock-proc and paddock-chromium unit tests (cargo
#                                          nextest archive, all features), then runs the guardian tests and paddock-chromium's
#                                          owner-killed test from the archive with `cargo nextest run --profile ci` (one process
#                                          per test: the tests share no guardian, CI run 9011) as PADDOCK_TEST_UID (default
#                                          65534, nobody) through setpriv, with a private TMPDIR and HOME. Run as root; run by a
#                                          user, it runs them directly.
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
  as_user=(setpriv --reuid="$uid" --regid="$uid" --clear-groups --no-new-privs --)
else
  uid="$(id -u)"
  as_user=()
fi

echo "Archiving the paddock-proc and paddock-chromium unit tests"
build_log="$(mktemp)"
trap 'rm -f "$build_log"' EXIT
private="$(mktemp -d)"
trap 'rm -rf "$private" "$build_log"' EXIT
archive="$private/guardian.tar.zst"
run_dir="$private/run"
if ! cargo nextest archive --all-features -p paddock-proc -p paddock-chromium --archive-file "$archive" >"$build_log" 2>&1; then
  tail -n 40 "$build_log"
  fail "cargo nextest archive failed (output above); fix the build first"
fi

# The tests' fake programs are the test kit's stand-in. Unnamed, the kit compiles it into the build folder of the test program,
# which the user the tests drop to cannot write (ci.yml 8359). So it is built here, before the drop, and named to the tests; a
# stand-in the job names (PADDOCK_STAND_IN_BIN) is used as it is.
stand_in="${PADDOCK_STAND_IN_BIN:-}"
if [ -z "$stand_in" ]; then
  stand_in_log="$private/stand-in.log"
  code=0
  cargo build -p paddock-testkit --bin paddock-stand-in --message-format=json >"$stand_in_log" 2>&1 || code=$?
  [ "$code" = 0 ] || fail "cargo build of paddock-stand-in failed (exit $code); see $stand_in_log; fix the build first"
  # The executable of cargo's compiler-artifact line for the paddock-stand-in target.
  stand_in="$(grep '"reason":"compiler-artifact"' "$stand_in_log" | grep '"name":"paddock-stand-in"' | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' | tail -n 1)"
  [ -n "$stand_in" ] || fail "cargo built no paddock-stand-in; see $stand_in_log; check that paddock-testkit still has its paddock-stand-in binary"
fi
mkdir "$run_dir"
# The ci profile writes its JUnit report into nextest's store folder, which is the workspace's own target folder unless a config names
# another one (root's, or read-only, so not the user's): a tool config puts it in the user's own folder.
printf "[store]\ndir = '%s'\n" "$run_dir/store" >"$private/store.toml"
[ "${#as_user[@]}" -eq 0 ] || chown "$uid:$uid" "$private" "$run_dir"
chmod 700 "$private"

# What runs: the guardian tests of paddock-proc and paddock-chromium's owner-killed browser test (the Rust step's FILTERSET).
filterset="(package(paddock-proc) & test(guardian::)) | (package(paddock-chromium) & test(launch::tests::a_browser_whose_owner_is_killed))"
label="guardian::"

{
  echo "### Orphan guardian as a user"
  echo
  echo "| Tests | As | Result | Time |"
  echo "|---|---|---|---|"
} >>"$summary"
started=$SECONDS
code=0
cmd=("${as_user[@]}" env "TMPDIR=$private" "HOME=$private" "PADDOCK_PROC_GUARDIAN_LOG=$private/guardian.log" "PADDOCK_STAND_IN_BIN=$stand_in" \
  cargo nextest run --archive-file "$archive" --extract-to "$run_dir" --workspace-remap "$PWD" --tool-config-file "guardian:$private/store.toml" --profile ci --no-tests=fail -E "$filterset")
echo "Running as uid $uid: ${cmd[*]}"
"${cmd[@]}" || code=$?
took=$((SECONDS - started))
if [ "$code" != 0 ]; then
  if [ -f "$private/guardian.log" ]; then
    echo "The guardian's log:"
    cat "$private/guardian.log"
  else
    echo "The guardian wrote no log ($private/guardian.log)"
  fi
  echo "| \`$label\` | uid $uid | failed (exit $code) | ${took} s |" >>"$summary"
  fail "\`$label\` failed as uid $uid (exit $code, ${took} s): ${cmd[*]}. Run the same command in the CI image to see it"
fi
echo "| \`$label\` | uid $uid | passed | ${took} s |" >>"$summary"
echo "The guardian's acting tests passed as uid $uid"

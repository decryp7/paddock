#!/usr/bin/env bash
# Runs INSIDE the CI image as root (scripts/member-gate-linux.sh starts it through tools/linux-container.sh --loop-mount): sets up
# the CI-like environment, clones the merge commit fresh, and runs the specs. See that script for the contract and the exit codes.
set -uo pipefail
tree="$1"; shift
fail() { echo "::error::member-gate-linux: $1. Next step: $2" >&2; exit 75; }

[ "$(id -u)" = 0 ] || fail "not root (CI's containers run as root)" "run it through scripts/member-gate-linux.sh"
head_tree=$(git -C /work rev-parse 'HEAD^{tree}') || fail "/work is not a readable git checkout" "run from the worktree that holds the merge"
[ "$head_tree" = "$tree" ] || fail "the checkout's HEAD tree is $head_tree, not $tree" "run it from the worktree that holds the merge"
commit=$(git -C /work rev-parse HEAD)

# TMPDIR on ext4 (CI's runners have it; the container's /tmp is overlay, whose folder blocks differ: run 8138).
for i in 0 1 2 3 4 5 6 7; do [ -e "/dev/loop$i" ] || mknod "/dev/loop$i" b 7 "$i" 2>/dev/null; done
truncate -s 6G /tmp/member-gate.img && mkfs.ext4 -q /tmp/member-gate.img || fail "cannot make the ext4 image" "check the container's disk"
mkdir -p /mnt/e && mount -o loop /tmp/member-gate.img /mnt/e || fail "cannot loop-mount the ext4 image (is --loop-mount in effect?)" "run it through scripts/member-gate-linux.sh"
trap 'umount /mnt/e 2>/dev/null' EXIT
fstype=$(df -T /mnt/e | awk 'NR==2 {print $2}')
[ "$fstype" = ext4 ] || fail "TMPDIR is on $fstype, not ext4" "the loop mount did not work"
mkdir -p /mnt/e/tmp && chmod 1777 /mnt/e/tmp
export TMPDIR=/mnt/e/tmp TMP=/mnt/e/tmp TEMP=/mnt/e/tmp

# A fresh shallow clone of exactly the merge commit, as actions/checkout leaves it: git's own defaults and /etc/gitconfig only.
src=/mnt/e/src
git init -q "$src" && git -C "$src" fetch -q --depth 1 file:///work "$commit" && git -C "$src" checkout -q --detach FETCH_HEAD \
  || fail "cannot clone $commit freshly from /work" "check that the commit is reachable in the worktree"
[ "$(git -C "$src" rev-parse 'HEAD^{tree}')" = "$tree" ] || fail "the clone's tree is not $tree" "report this"
local_keys=$(git -C "$src" config --local --list | cut -d= -f1 | grep -vE '^(core\.(repositoryformatversion|filemode|bare|logallrefupdates)|extensions\..*)$' || true)
[ -z "$local_keys" ] || fail "the clone has repo-local git config: $local_keys" "report this"
[ -z "$(git config --global --list 2>/dev/null)" ] || fail "there is a global git config in the container" "the CI image has none"

# CI's environment: the variables the runners set (CI, GITHUB_ACTIONS), nextest with the CI profile, the CI file-descriptor limit.
export CI=true GITHUB_ACTIONS=true GITHUB_WORKSPACE="$src" PADDOCK_JOB_TEMP=/mnt/e/tmp
export NEXTEST_TEST_THREADS=${NEXTEST_TEST_THREADS:-4}
ulimit -n 1024
echo "other-os-control: os=linux tree=$tree user=root tmp=$fstype clone=fresh-shallow gitconfig=none arch=$(uname -m) ci-env=1"

failed=0; ran=0
for spec in "$@"; do
  read -r kind a b filter <<<"$spec"
  case "$kind" in
    nextest)
      echo "::group::nextest $a $b"
      (cd "$src/$a" && cargo nextest run -p "$b" --profile ci --locked --no-fail-fast ${filter:+-E "$filter"} 2>&1)
      rc=$?
      echo "::endgroup::"
      ;;
    node)
      dir="$a"; cmd="${spec#node $a }"
      echo "::group::node $dir: $cmd"
      (cd "$src" && [ -d node_modules ] || npm ci --ignore-scripts -w conformance >/dev/null 2>&1; cd "$src/$dir" && sh -c "$cmd" 2>&1)
      rc=$?
      echo "::endgroup::"
      ;;
    *) fail "unknown spec kind '$kind'" "use nextest or node" ;;
  esac
  ran=$((ran + 1))
  # nextest: 100 is failed tests; any other non-zero is a build or setup failure that did not look at the tests.
  if [ "$kind" = nextest ]; then
    case "$rc" in 0) ;; 100) failed=1 ;; *) echo "::error::member-gate-linux: $spec exited $rc (a build or setup failure, not failed tests)" >&2; exit 75 ;; esac
  else
    [ "$rc" = 0 ] || failed=1
  fi
done
echo "member-gate-linux: $ran spec(s), failed=$failed" >&2
[ "$failed" = 0 ] && exit 0 || exit 100

#!/usr/bin/env bash
# Exit 0 when <dir> is a pristine checkout of <commit>: HEAD is that commit; `git fsck --full --strict` finds the object
# store sound (a forged or corrupt object fails it); nothing is changed, untracked or ignored
# (`git status --porcelain --ignored --untracked-files=all` prints nothing); and every named submodule is checked out at
# the commit the tree records and passes the same checks. Exit 1 otherwise, with the reason on stderr. build-ort.sh trusts
# a source tree (a fresh clone or one restored from a cache) only after this.
#
#   apps/sighter/scripts/ort/check-source.sh <dir> <commit> [submodule-path]...
set -uo pipefail

dir=${1:?usage: check-source.sh <dir> <commit> [submodule-path]...}
commit=${2:?usage: check-source.sh <dir> <commit> [submodule-path]...}
shift 2

fail() { echo "check-source: $dir: $*" >&2; exit 1; }
pristine() {
  local d=$1 out
  git -C "$d" fsck --full --strict --no-progress >/dev/null 2>&1 || fail "${d#"$dir"}: git fsck --full --strict fails"
  out=$(git -C "$d" status --porcelain --ignored --untracked-files=all --ignore-submodules=none 2>&1) || fail "git status fails: $out"
  [ -z "$out" ] || fail "${d#"$dir"} is not pristine: $(printf '%s' "$out" | head -n 3 | tr '\n' ';')"
}

[ -d "$dir/.git" ] || [ -f "$dir/.git" ] || fail "not a Git checkout"
head=$(git -C "$dir" rev-parse HEAD 2>/dev/null) || fail "no HEAD"
[ "$head" = "$commit" ] || fail "HEAD is $head, pinned $commit"
pristine "$dir"
for sub in "$@"; do
  want=$(git -C "$dir" ls-tree HEAD -- "$sub" | awk '$2 == "commit" {print $3}')
  [ -n "$want" ] || fail "$sub is not a submodule of $commit"
  got=$(git -C "$dir/$sub" rev-parse HEAD 2>/dev/null) || fail "$sub is not checked out"
  [ "$got" = "$want" ] || fail "$sub is at $got, the tree records $want"
  pristine "$dir/$sub"
done
exit 0

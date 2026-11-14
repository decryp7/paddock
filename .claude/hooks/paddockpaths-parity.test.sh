#!/bin/bash
# Parity of the two copies of the dev-targets rule (docs/known-copies.toml, `dev-places`): tools/paddock-dev/src/places.rs
# (`dev places --get dev-targets`) and the hooks' lib/paddockpaths.py (dev_targets()) get the same inputs and must give the same
# folder, or both refuse. Run: bash .claude/hooks/paddockpaths-parity.test.sh
# The paddock-dev program: PADDOCK_DEV (a built binary), else the repository's ./dev. Folders are invented (/srv/...).
here=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$here/../.." && pwd)
DEV=${PADDOCK_DEV:-$repo_root/dev}
TMP=$(cd "$(mktemp -d)" && pwd -P); trap 'rm -rf "$TMP"' EXIT
command -v python3 >/dev/null || { echo "FAIL: python3 is missing: the hooks cannot run either"; exit 1; }
fail=0
n=0
# case <name> <PADDOCK_DEV_TARGETS or -> <git config paddock.worktreeRoot or -> <PADDOCK_WORKTREE_ROOT or -> <want or REFUSED>
case_() {
  local name=$1 named=$2 cfg=$3 root=$4 want=$5 r env=() rust py
  n=$((n + 1)); r="$TMP/r$n"
  git init -q "$r" || exit 1
  [ "$cfg" = - ] || git -C "$r" config paddock.worktreeRoot "$cfg"
  env=(env -u PADDOCK_DEV_TARGETS -u PADDOCK_WORKTREE_ROOT -u DOCKER_GUARD_OK_DIRS -u GIT_CONFIG_COUNT)
  [ "$named" = - ] || env+=(PADDOCK_DEV_TARGETS="$named")
  [ "$root" = - ] || env+=(PADDOCK_WORKTREE_ROOT="$root")
  rust=$("${env[@]}" "$DEV" places --get dev-targets --repo "$r" 2>/dev/null) || rust=REFUSED
  py=$("${env[@]}" CLAUDE_PROJECT_DIR="$r" python3 -I -c 'import sys; sys.path.insert(0, sys.argv[1]); import paddockpaths; print(paddockpaths.dev_targets() or "REFUSED")' "$here/lib" 2>/dev/null) || py=ERROR
  if [ "$rust" = "$want" ] && [ "$py" = "$want" ]; then echo "ok: $name -> $want"
  else echo "FAIL: $name: want '$want', Rust '$rust', Python '$py'"; fail=1; fi
}
case_ "the parent of the worktree root" - /srv/a/dev-targets/wt - /srv/a/dev-targets
case_ "a trailing slash on the root" - /srv/a/dev-targets/wt/ - /srv/a/dev-targets
case_ "the named folder beats git config" /srv/n /srv/a/dev-targets/wt - /srv/n
case_ "a trailing slash on the named folder" /srv/n/ - - /srv/n
case_ "whitespace around the named folder" "  /srv/n  " - - /srv/n
case_ "a blank name falls through to git config" "   " /srv/a/dt/wt - /srv/a/dt
case_ "a root-level named folder" /dt - - /dt
case_ "a relative named folder" rel/dt /srv/a/dt/wt - REFUSED
case_ "the filesystem root as the named folder" / - - REFUSED
case_ "a named folder with .." /srv/../n - - REFUSED
case_ "a root-level worktree root" - /wt - REFUSED
case_ "a relative worktree root" - wt - REFUSED
case_ "a relative worktree root with .." - ../dt/wt - REFUSED
case_ "an absolute worktree root with .." - /srv/../wt - REFUSED
case_ "a blank worktree root and nothing else" - "  " - REFUSED
case_ "a blank worktree root falls through to the variable" - "  " /srv/b/dt/wt /srv/b/dt
case_ "nothing set" - - - REFUSED
case_ "the variable when git config is unset" - - /srv/b/dt/wt /srv/b/dt
case_ "git config beats the variable" - /srv/a/dt/wt /srv/b/dt/wt /srv/a/dt
case_ "a relative variable" - - wt REFUSED
case_ "a variable with leading whitespace" - - " /srv/b/dt/wt" REFUSED
# `places --pinned` (the Windows VM guards) reads the repository's own config with an empty environment: nothing moves it.
r="$TMP/pinned"; git init -q "$r" && git -C "$r" config paddock.worktreeRoot /srv/p/dt/wt
got=$(GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=paddock.worktreeRoot GIT_CONFIG_VALUE_0=/srv/e/dt/wt PADDOCK_DEV_TARGETS=/srv/y \
  PADDOCK_WORKTREE_ROOT=/srv/z/dt/wt PADDOCK_VM_DATA=/srv/x "$DEV" places --pinned --get vm-data --repo "$r" 2>&1)
[ "$got" = /srv/p/vm-data ] && echo "ok: --pinned ignores the environment" || { echo "FAIL: --pinned gave '$got', want /srv/p/vm-data"; fail=1; }
exit $fail

# Sourced (bash): where this machine keeps its development folders, derived and never named (C125, rule 40). The layout is
# described once, in docs/dev-setup.md ("Where things live"); the Rust original is tools/paddock-dev/src/places.rs and this
# is a registered copy (docs/known-copies.toml, `dev-places`), as are scripts/dev-places.mjs and .claude/hooks/lib/paddockpaths.py.
#
#   paddock_dev_targets    PADDOCK_DEV_TARGETS, else the parent of `git config paddock.worktreeRoot`, else of PADDOCK_WORKTREE_ROOT
#   paddock_dev_disk       the folder that holds the dev-targets folder (the dev disk's root)
#   paddock_vm_data        PADDOCK_VM_DATA, else <dev disk>/vm-data
#   paddock_vm_data_pinned <dev disk>/vm-data from the repository's own git config only, with the environment cleared: for the
#                          guards that must not be moved by any variable (scripts/vm-win/target.sh)
#
# Each prints the folder, or prints `::error::` with the next step on stderr and returns 3 when it cannot be derived (fail
# closed: never a guessed disk).

_paddock_places_here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_paddock_places_hint="run \`git config paddock.worktreeRoot <dev-targets>/wt\` once in the main checkout, or set PADDOCK_DEV_TARGETS (docs/dev-setup.md)"

# _paddock_parent <path> <what>: the parent folder of an absolute path below the filesystem root, or an error.
_paddock_parent() {
  local p="${1%/}" parent
  case "$p" in /?*) ;; *) echo "::error::dev-places: $2 '$1' is not an absolute path; $_paddock_places_hint" >&2; return 3 ;; esac
  parent="${p%/*}"
  [ -n "$parent" ] || { echo "::error::dev-places: $2 '$1' has no parent folder; $_paddock_places_hint" >&2; return 3; }
  printf '%s\n' "$parent"
}

# The worktree root from the repository's git config; with `pinned`, git runs with an empty environment, so no GIT_* or HOME
# variable can change what it reads.
_paddock_worktree_root() {
  if [ "${1:-}" = pinned ]; then
    env -i PATH=/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin git -C "$_paddock_places_here" config --local --get paddock.worktreeRoot 2>/dev/null
  else
    git -C "$_paddock_places_here" config --get paddock.worktreeRoot 2>/dev/null || {
      [ -n "${PADDOCK_WORKTREE_ROOT:-}" ] && printf '%s\n' "$PADDOCK_WORKTREE_ROOT"
    }
  fi
}

paddock_dev_targets() {
  local root
  if [ -n "${PADDOCK_DEV_TARGETS:-}" ]; then
    case "$PADDOCK_DEV_TARGETS" in /?*) printf '%s\n' "${PADDOCK_DEV_TARGETS%/}"; return 0 ;; esac
    echo "::error::dev-places: PADDOCK_DEV_TARGETS '$PADDOCK_DEV_TARGETS' is not an absolute path; $_paddock_places_hint" >&2
    return 3
  fi
  root="$(_paddock_worktree_root)" && [ -n "$root" ] || {
    echo "::error::dev-places: the dev-targets folder cannot be derived: no worktree root is set; $_paddock_places_hint" >&2
    return 3
  }
  _paddock_parent "$root" "the worktree root"
}

paddock_dev_disk() {
  local dt
  dt="$(paddock_dev_targets)" || return 3
  _paddock_parent "$dt" "the dev-targets folder"
}

paddock_vm_data() {
  local disk
  if [ -n "${PADDOCK_VM_DATA:-}" ]; then printf '%s\n' "${PADDOCK_VM_DATA%/}"; return 0; fi
  disk="$(paddock_dev_disk)" || return 3
  printf '%s/vm-data\n' "$disk"
}

paddock_vm_data_pinned() {
  local root dt disk
  root="$(_paddock_worktree_root pinned)" && [ -n "$root" ] || {
    echo "::error::dev-places: the VM folder cannot be derived: the repository's git config has no paddock.worktreeRoot; run \`git config paddock.worktreeRoot <dev-targets>/wt\` once in the main checkout (docs/dev-setup.md)" >&2
    return 3
  }
  dt="$(_paddock_parent "$root" "the worktree root")" || return 3
  disk="$(_paddock_parent "$dt" "the dev-targets folder")" || return 3
  printf '%s/vm-data\n' "$disk"
}

"""Paths the guards need, derived from git instead of hard-coded (a disk move once left stale defaults behind).

The copy of tools/paddock-dev/src/places.rs for the hooks, which run before any binary is built (docs/known-copies.toml,
`dev-places`; .claude/hooks/paddockpaths-parity.test.sh runs both on one table of cases).

main_checkout(): WORKTREE_GUARD_MAIN, else the parent of the git common dir of $CLAUDE_PROJECT_DIR (else of this script's own
folder). dev_targets(): PADDOCK_DEV_TARGETS, else the parent of `git config paddock.worktreeRoot`, else of PADDOCK_WORKTREE_ROOT
(docs/dev-setup.md, "Where things live"). dev_targets_dirs(): DOCKER_GUARD_OK_DIRS (an explicit list), else [dev_targets(),
the session scratchpads]. Each returns None when it cannot be derived (a relative, root-level, blank or `..` value included); the
guards then refuse (fail closed) with a `Blocked: ... Next step: ...` message."""
import os, subprocess


def _git(folder, *args):
    try:
        r = subprocess.run(["git", "-C", folder] + list(args), capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.SubprocessError):
        return None
    out = r.stdout.strip()
    return out if r.returncode == 0 and out else None


def _start():
    return os.environ.get("CLAUDE_PROJECT_DIR") or os.path.dirname(os.path.abspath(__file__))


def main_checkout():
    env = os.environ.get("WORKTREE_GUARD_MAIN")
    if env:
        return env
    common = _git(_start(), "rev-parse", "--path-format=absolute", "--git-common-dir")
    if not common or os.path.basename(common.rstrip("/")) != ".git":
        return None
    return os.path.dirname(common.rstrip("/"))


def worktree_root():
    return _git(_start(), "config", "paddock.worktreeRoot")


def _bad(p):
    return not p.startswith("/") or ".." in p.split("/")


def dev_targets_of(named, root):
    """The pure rule, the same as places.rs `dev_targets_of`: the named folder (absolute, not `/`, no `..`), else the parent of
    the worktree root (absolute, no `..`, and the parent is not `/`); None otherwise."""
    if named is not None and named.strip():
        p = named.strip()
        if _bad(p) or p.rstrip("/") == "":
            return None
        return p.rstrip("/")
    if not root:
        return None
    r = root.rstrip("/")
    if _bad(r):
        return None
    parent = os.path.dirname(r)
    return parent if parent not in ("", "/") else None


def dev_targets():
    named = os.environ.get("PADDOCK_DEV_TARGETS")
    if named is not None and named.strip():
        return dev_targets_of(named, None)
    root = worktree_root()
    if root is None:
        root = os.environ.get("PADDOCK_WORKTREE_ROOT") or None
    return dev_targets_of(None, root)


def dev_targets_dirs():
    env = os.environ.get("DOCKER_GUARD_OK_DIRS")
    if env is not None:
        return [d for d in env.split(":") if d]
    dt = dev_targets()
    if not dt:
        return None
    return [dt, "/private/tmp/claude-"]

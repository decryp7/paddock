"""Paths the guards need, derived from git instead of hard-coded (a disk move once left stale defaults behind).

main_checkout(): WORKTREE_GUARD_MAIN, else the parent of the git common dir of $CLAUDE_PROJECT_DIR (else of this script's own
folder). dev_targets_dirs(): DOCKER_GUARD_OK_DIRS, else the parent of `git config paddock.worktreeRoot`. Both return None when
they cannot be derived; the guards then refuse (fail closed) with a `Blocked: ... Next step: ...` message."""
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


def dev_targets_dirs():
    env = os.environ.get("DOCKER_GUARD_OK_DIRS")
    if env is not None:
        return [d for d in env.split(":") if d]
    wt = worktree_root()
    if not wt:
        return None
    return [os.path.dirname(wt.rstrip("/")), "/private/tmp/claude-"]

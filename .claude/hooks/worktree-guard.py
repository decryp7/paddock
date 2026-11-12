#!/usr/bin/env python3
"""The ER-2 guard (RT-7), blocking, on lib/shellscan.py. See worktree-guard.sh for the rules."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "lib"))
import shellscan


NAME = "worktree-guard"
MAIN = os.environ.get("WORKTREE_GUARD_MAIN", "/Volumes/d8sst7/dev.decryptology.net/paddock")

WRITES = {"checkout", "switch", "commit", "merge", "rebase", "reset", "push", "cherry-pick", "revert", "am", "pull",
          "restore", "clean", "stash", "update-ref"}
BRANCH_W = {"-D", "-d", "-m", "-M", "--delete", "--move", "--force", "-f", "-C", "-c", "--copy"}
TAG_RO = {"-l", "--list", "-n", "-v", "--contains", "--points-at", "--no-contains", "--merged", "--no-merged"}
WT_W = {"remove", "prune", "move", "repair"}


def norm(p):
    return os.path.realpath(p).casefold()


def in_main(d):
    d, m = norm(d), norm(MAIN)
    return d == m or d.startswith(m + os.sep)


def is_write(sub, args):
    if sub in WRITES:
        return True
    if sub == "branch":
        return any(a in BRANCH_W for a in args)
    if sub == "tag":
        return bool(args) and not any(a in TAG_RO for a in args)
    if sub == "worktree":
        return bool(args) and args[0] in WT_W
    return False


def closes_with_exit(cmds, cmd):
    """True when what follows `cd X ||` stops the script: `exit`/`return`, alone or inside a { ...; } group."""
    nxt = cmds[cmd.idx + 1] if cmd.idx + 1 < len(cmds) else None
    if nxt is None:
        return False
    depth = nxt.opens
    if nxt.argv[:1] in (["exit"], ["return"]):
        return True
    if depth == 0:
        return False
    for c in cmds[nxt.idx + 1:]:
        if c.argv[:1] in (["exit"], ["return"]):
            return True
        depth += c.opens
        if c.words[:1] == ["}"]:
            depth -= 1
            if depth <= 0:
                return False
    return False


def decide(data):
    if data.get("tool_name") != "Bash":
        return None
    text = (data.get("tool_input") or {}).get("command") or ""
    is_agent = bool(data.get("agent_id"))
    cmds = shellscan.scan(text, data.get("cwd"))
    for c in cmds:
        if c.is_cd and not c.cond:
            tgt = c.cd_target
            if c.term in (";", "\n") and not c.set_e and shellscan.next_cmd(cmds, c) is not None:
                return ("Blocked: `cd %s` is followed by `%s` and more commands (unguarded cd): if the cd fails, the rest "
                        "runs in the wrong folder (on 2026-11-10 that merged into the main checkout's main). Next step: "
                        "write `cd %s && ...` (or `cd %s || exit 1; ...`), or work in your own worktree with absolute "
                        "paths and `git -C`." % (tgt, "newline" if c.term == "\n" else ";", tgt, tgt))
            if c.term == "||" and not c.set_e and not closes_with_exit(cmds, c):
                return ("Blocked: `cd %s || ...` does not stop when the cd fails (unguarded cd). Next step: `cd %s && ...`,"
                        " or `cd %s || exit 1; ...`." % (tgt, tgt, tgt))
        g = shellscan.git_invocation(c)
        if not g:
            continue
        eff, sub, args = g
        if not is_write(sub, args):
            continue
        if c.after_cd_chain:
            return ("Blocked: `git %s` runs after a `cd X && ...` chain ended by `;` or a newline (unguarded cd): if "
                    "the cd failed, it runs in the wrong folder. Next step: keep it in the chain (`cd X && ... && git %s`)"
                    ", or use `git -C <worktree> %s`." % (sub, sub, sub))
        if is_agent and eff and in_main(eff):
            return ("Blocked: `git %s` in the main checkout (%s) by a subagent. The main checkout stays on `main` for "
                    "the coordinator's fast-forwards (ER-2). Next step: do it in your own worktree (`./dev worktree add` "
                    "and claim it), with `cd <worktree> && git ...` or `git -C <worktree> ...`." % (sub, MAIN))
    return None


def main():
    try:
        data = json.load(sys.stdin)
        reason = decide(data)
    except shellscan.TooBig as e:
        shellscan.warn(NAME, "command too large to scan (%s)" % e)
        return
    except Exception as e:  # never fail closed
        shellscan.warn(NAME, "could not read the input (%s)" % e)
        return
    if reason:
        print(shellscan.deny_json(reason))


if __name__ == "__main__":
    main()

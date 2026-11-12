#!/usr/bin/env python3
# The logic of worktree-guard.sh (see there).
import json, os, shlex, sys

MAIN = os.environ.get("WORKTREE_GUARD_MAIN", "/Volumes/d8sst7/dev.decryptology.net/paddock")

def allow(warn=None):
    if warn:
        print("worktree-guard: " + warn + "; allowing the command", file=sys.stderr)
    sys.exit(0)

def deny(reason):
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
                                             "permissionDecisionReason": reason}}))
    sys.exit(0)

try:
    data = json.load(sys.stdin)
except Exception as e:
    allow("input is not JSON (%s)" % e)
if data.get("tool_name") != "Bash":
    allow()
cmd = (data.get("tool_input") or {}).get("command") or ""
is_agent = bool(data.get("agent_id"))
cwd0 = data.get("cwd") or ""

# Split into segments: (text, terminator, depth). Quotes and backslashes are respected; `(` and `)` change the depth.
def scan(s):
    segs, buf, depth = [], [], 0
    i, n, q = 0, len(s), None
    def flush(term):
        t = "".join(buf).strip()
        buf.clear()
        segs.append((t, term, depth))
    while i < n:
        c = s[i]
        if q:
            buf.append(c)
            if c == "\\" and q == '"' and i + 1 < n:
                i += 1; buf.append(s[i])
            elif c == q:
                q = None
        elif c == "\\" and i + 1 < n:
            buf.append(c); i += 1; buf.append(s[i])
        elif c in "'\"":
            q = c; buf.append(c)
        elif c == "#" and (not buf or buf[-1] in " \t"):
            while i < n and s[i] != "\n":
                i += 1
            continue
        elif c == "\n":
            flush("\n")
        elif c == ";":
            flush(";")
        elif c == "&" and s[i:i+2] == "&&":
            flush("&&"); i += 1
        elif c == "&":
            flush("&")
        elif c == "|" and s[i:i+2] == "||":
            flush("||"); i += 1
        elif c == "|":
            flush("|")
        elif c == "(":
            flush("("); depth += 1
        elif c == ")":
            flush(")"); depth -= 1
        else:
            buf.append(c)
        i += 1
    flush("")
    return segs

try:
    segs = scan(cmd)
except Exception as e:
    allow("could not read the command (%s)" % e)

def words(t):
    try:
        return shlex.split(t, comments=False, posix=True)
    except ValueError:
        return t.split()

def strip_lead(w):
    while w and w[0] in ("{", "!", "time", "command", "builtin"):
        w = w[1:]
    return w

# 1. Unguarded cd.
for i, (t, term, depth) in enumerate(segs):
    w = strip_lead(words(t))
    if not w or w[0] != "cd":
        continue
    rest = []
    for (t2, term2, d2) in segs[i + 1:]:
        if term == ")" or d2 < depth:
            break
        if t2:
            rest.append((t2, d2))
        if term2 == ")" and d2 <= depth:
            break
    if term in (";", "\n"):
        # more commands after it in this level?
        more = False
        for (t2, term2, d2) in segs[i + 1:]:
            if d2 < depth or (t2 == "" and term2 == ")"):
                break
            if t2:
                more = True
                break
        if more:
            deny("Blocked: `cd %s` is followed by `%s` and more commands (unguarded cd): if the cd fails, the rest runs in the "
                 "wrong folder (on 2026-11-10 that merged into the main checkout's main). Next step: write `cd %s && ...` "
                 "(or `cd %s || exit 1; ...`), or work in your own worktree with absolute paths and `git -C`."
                 % (" ".join(w[1:]), "newline" if term == "\n" else ";", " ".join(w[1:]), " ".join(w[1:])))
    elif term == "||":
        nxt = segs[i + 1][0] if i + 1 < len(segs) else ""
        nw = strip_lead(words(nxt))
        if not nw or nw[0] not in ("exit", "return"):
            deny("Blocked: `cd %s || %s` does not stop when the cd fails (unguarded cd). Next step: `cd %s && ...`, or "
                 "`cd %s || exit 1; ...`." % (" ".join(w[1:]), nxt.split(" ")[0] if nxt else "", " ".join(w[1:]), " ".join(w[1:])))

# 2. A subagent's git writes in the main checkout.
def norm(p):
    return os.path.realpath(p).casefold()

MAIN_N = norm(MAIN)

def in_main(d):
    d = norm(d)
    return d == MAIN_N or d.startswith(MAIN_N + os.sep)

WRITES = {"checkout", "switch", "commit", "merge", "rebase", "reset", "push", "cherry-pick", "revert", "am", "pull",
          "restore", "clean", "stash"}
BRANCH_W = {"-D", "-d", "-m", "-M", "--delete", "--move", "--force", "-f"}
TAG_RO = {"-l", "--list", "-n", "-v", "--contains", "--points-at", "--no-contains", "--merged", "--no-merged"}
WT_W = {"remove", "prune", "move", "repair"}
OPT_ARG = {"-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path", "--super-prefix"}

def resolve(cur, d):
    d = os.path.expanduser(d)
    if "$" in d or "`" in d:
        return None
    return d if os.path.isabs(d) else os.path.join(cur, d)

if is_agent and cwd0:
    stack = [cwd0]
    last_depth = 0
    for (t, term, depth) in segs:
        while len(stack) - 1 < depth:
            stack.append(stack[-1])
        while len(stack) - 1 > depth:
            stack.pop()
        cur = stack[-1]
        w = strip_lead(words(t))
        if not w:
            continue
        if w[0] == "cd":
            tgt = resolve(cur, w[1]) if len(w) > 1 else os.path.expanduser("~")
            stack[-1] = tgt if tgt else "?unknown"
            continue
        # skip env assignments and wrappers
        k = 0
        while k < len(w) and ("=" in w[k] and not w[k].startswith("-") and w[k].split("=")[0].isidentifier()):
            k += 1
        if k < len(w) and w[k] in ("env", "sudo", "nohup", "exec"):
            k += 1
        if k >= len(w) or w[k] != "git":
            continue
        k += 1
        eff = cur
        while k < len(w) and w[k].startswith("-"):
            if w[k] in OPT_ARG and k + 1 < len(w):
                if w[k] == "-C":
                    r = resolve(eff, w[k + 1])
                    eff = r if r else "?unknown"
                k += 2
            else:
                k += 1
        if k >= len(w) or eff.startswith("?"):
            continue
        sub, args = w[k], w[k + 1:]
        write = False
        if sub in WRITES:
            write = True
        elif sub == "branch":
            write = any(a in BRANCH_W for a in args)
        elif sub == "tag":
            write = bool(args) and not any(a in TAG_RO for a in args)
        elif sub == "worktree":
            write = bool(args) and args[0] in WT_W
        if write and in_main(eff):
            deny("Blocked: `git %s` in the main checkout (%s) by a subagent. The main checkout stays on `main` for the "
                 "coordinator's fast-forwards (ER-2). Next step: do it in your own worktree (`./dev worktree add` and "
                 "claim it), with `cd <worktree> && git ...` or `git -C <worktree> ...`." % (sub, MAIN))
allow()

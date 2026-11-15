#!/usr/bin/env python3
"""The waiter guard (ER-32), on lib/shellscan.py. See no-pgrep-wait.sh for what it refuses, what it only warns about and why."""
import json, os, re, subprocess, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "lib"))
import shellscan

NAME = "no-pgrep-wait"
DENY = ("Blocked: don't wait with pgrep -f (it matches its own command line). Wait on the job's own completion notification, "
        "plock's '<name> done' line in that job's output, or `./dev wait --pid/--plock/--file/--ci-run ... --max <duration>`.")
# The guard before the scanner, kept for a command too large to scan (shellscan.TooBig).
OLD = re.compile(r"(while|until)[^;]*pgrep[ \t\n]+(-[A-Za-z0-9]*[ \t\n]+)*-[A-Za-z0-9]*f")
GREPS = {"grep", "egrep", "fgrep", "zgrep", "rg"}
TARGETS = ("--file", "--pid", "--ci-run", "--plock")
PS_TIMEOUT = 3


def chain(cmds, first):
    """`first` and the commands joined to it by `&&`, `||` or `|`: a loop's whole condition."""
    out = [first]
    for c in cmds[first.idx + 1:]:
        if out[-1].term not in ("&&", "||", "|") or c.depth != first.depth:
            break
        out.append(c)
    return out


def body_sleeps(cmds, cond_end):
    """Whether the loop body after the condition (up to its `done`) runs `sleep`."""
    depth = cond_end.depth
    for c in cmds[cond_end.idx + 1:]:
        if c.depth < depth:
            return False
        if c.depth == depth and c.words[:1] == ["done"]:
            return False
        if c.argv[:1] == ["sleep"]:
            return True
    return False


def pgrep_f(c):
    a = c.argv
    if not a or os.path.basename(a[0]) != "pgrep":
        return False
    return any(x.startswith("-") and not x.startswith("--") and "f" in x[1:] for x in a[1:]) or "--full" in a[1:]


def grep_file(cond):
    """The file a grep in the condition reads (for the suggested `dev wait`), '' when it reads a pipe, None when no grep."""
    for k, c in enumerate(cond):
        if c.argv and os.path.basename(c.argv[0]) in GREPS:
            if k > 0 and cond[k - 1].term == "|":
                src = [x for x in cond[k - 1].argv[1:] if not x.startswith("-") and not x.isdigit()]
                return src[-1] if src else ""
            ops = [x for x in c.argv[1:] if not x.startswith("-")]
            return ops[1] if len(ops) > 1 else ""
    return None


def wait_target(argv):
    """(flag, value) of a `./dev wait` command line, None for anything else."""
    k = next((i for i, w in enumerate(argv) if w == "wait"), None)
    if k is None or k == 0 or not (argv[k - 1] == "dev" or argv[k - 1].endswith("/dev") or argv[k - 1].endswith("paddock-dev")):
        return None
    rest = argv[k + 1:]
    for i, w in enumerate(rest):
        flag, eq, val = w.partition("=")
        if flag in TARGETS:
            if not eq:
                val = rest[i + 1] if i + 1 < len(rest) else ""
            return (flag, os.path.normpath(val) if flag == "--file" and val else val)
    return None


def running_waits():
    """{(flag, value): pid} of the `dev wait` processes running now; {} when the table cannot be read (never fails closed).
    NO_PGREP_WAIT_PS_FILE names a file with `ps -A -o pid=,command=` output (tests)."""
    fake = os.environ.get("NO_PGREP_WAIT_PS_FILE")
    try:
        if fake:
            with open(fake, encoding="utf-8", errors="replace") as f:
                text = f.read()
        else:
            text = subprocess.run(["ps", "-A", "-o", "pid=,command="], capture_output=True, text=True,
                                  timeout=PS_TIMEOUT).stdout
    except (OSError, subprocess.SubprocessError) as e:
        shellscan.warn(NAME, "the process table could not be read (%s)" % e)
        return {}
    out = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) < 3 or not parts[0].isdigit():
            continue
        t = wait_target(parts[1:])
        if t:
            out.setdefault(t, parts[0])
    return out


def decide(cmd):
    """(deny reason or None, warnings)."""
    try:
        cmds = shellscan.scan(cmd)
    except shellscan.TooBig as e:
        shellscan.warn(NAME, "command too large to scan (%s)" % e)
        return (DENY if OLD.search(cmd) else None), []
    warns = []
    seen = {}
    for c in cmds:
        if c.loop:
            cond = chain(cmds, c)
            if any(pgrep_f(x) for x in cond):
                return DENY, []
            f = grep_file(cond)
            if f is not None and body_sleeps(cmds, cond[-1]):
                target = f or "<the log file>"
                warns.append(
                    "Warning: this command polls with a `%s grep ...; do sleep` loop. Such a loop has no maximum duration and "
                    "stacks up when started twice (ER-32). Next step: wait with `./dev wait --file %s --line '<regex>' --max "
                    "<duration>`, which ends at --max with an ::error:: line." % (c.loop, target))
        t = wait_target(c.argv)
        if t:
            if t in seen:
                warns.append("Warning: this command starts two `./dev wait` for the same target (%s %s). One waiter per event "
                             "(ER-32): drop the second one." % t)
            seen.setdefault(t, c)
    if seen:
        live = running_waits()
        for t in seen:
            if t in live:
                warns.append("Warning: a `./dev wait %s %s` is already running (pid %s). If it is yours, wait on its "
                             "completion notification instead of starting another waiter for the same event (ER-32); if "
                             "another agent's, ignore this." % (t[0], t[1], live[t]))
    return None, warns


def main():
    try:
        data = json.load(sys.stdin)
        cmd = (data.get("tool_input") or {}).get("command") or ""
        deny, warns = decide(cmd)
    except Exception as e:  # never fail closed
        shellscan.warn(NAME, "%s: %s" % (type(e).__name__, e))
        return 0
    if deny:
        print(shellscan.deny_json(deny))
    elif warns:
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": "\n".join(warns)}}))
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""PreToolUse(Bash): refuse force pushes. History on shared branches is never rewritten; push a new branch instead
(for example <branch>-v2) and say so in the report. Uses lib/shellscan.py, so a `git push` and a `-f` of different
commands never combine (the old version joined lines with tr)."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "lib"))
import shellscan


NAME = "no-force-push"


def force_push(args):
    for a in args:
        if a in ("-f", "--force", "--force-if-includes") or a.startswith("--force-with-lease"):
            return True
        if a.startswith("-") and not a.startswith("--") and "f" in a[1:]:
            return True
        if a.startswith("+") and len(a) > 1:
            return True
    return False


def decide(data):
    if data.get("tool_name") != "Bash":
        return None
    text = (data.get("tool_input") or {}).get("command") or ""
    for c in shellscan.scan(text, data.get("cwd")):
        g = shellscan.git_invocation(c)
        if g and g[1] == "push" and force_push(g[2]):
            return ("Blocked: force pushes are never allowed (shared history is not rewritten). "
                    "Next step: push a new branch instead (for example <branch>-v2) and report it.")
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

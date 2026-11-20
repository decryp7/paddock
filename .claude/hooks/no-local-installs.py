#!/usr/bin/env python3
"""PreToolUse(Bash): refuse an agent's commands that install tooling or measure coverage locally (ER-14, ER-45).

Refused: `cargo llvm-cov` (also `cargo +nightly llvm-cov`, `cargo-llvm-cov`), `rustup component add`, `rustup toolchain install`,
`rustup target add` (also `rustup install`, `rustup toolchain add`), wherever the command sits in a line, behind a wrapper
(`heavy run ... --`, `env`, `time`, `bash -c`) or in a `&&` chain. Text in heredocs, comments and quoted arguments is not a
command (lib/shellscan.py). Applies to agents only (agent_id set), like docker-guard and kill-guard. Never fails closed."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "lib"))
import shellscan

NAME = "no-local-installs"
# Programs that start the command they are given (the tool follows them, after options, assignments or `--`).
WRAPPERS = {"env", "time", "nice", "nohup", "timeout", "heavy", "dev", "sudo", "command", "exec", "xargs", "caffeinate", "taskpolicy"}
INSTALL = {("component", "add"), ("toolchain", "install"), ("toolchain", "add"), ("target", "add"), ("install", None)}


def _args(argv, i):
    """The arguments after argv[i] that are not options or a `+toolchain` (cargo and rustup take one)."""
    out = []
    for a in argv[i + 1:]:
        if a.startswith("+") and not out:
            continue
        if a.startswith("-") and not out:
            continue
        out.append(a)
    return out


def refusal(argv):
    for i, a in enumerate(argv):
        # `echo cargo llvm-cov` names a program, it does not start it: only the first word, or one a wrapper starts, counts.
        if i and os.path.basename(argv[0]) not in WRAPPERS and "--" not in argv[:i]:
            break
        prog = os.path.basename(a)
        if prog == "cargo-llvm-cov":
            return "cargo llvm-cov"
        if prog == "cargo" and _args(argv, i)[:1] == ["llvm-cov"]:
            return "cargo llvm-cov"
        if prog == "rustup":
            rest = _args(argv, i)
            if rest[:1] == ["install"]:
                return "rustup install"
            if len(rest) >= 2 and (rest[0], rest[1]) in INSTALL:
                return "rustup %s %s" % (rest[0], rest[1])
    return None


def decide(data):
    if data.get("tool_name") != "Bash" or not data.get("agent_id"):
        return None
    text = (data.get("tool_input") or {}).get("command") or ""
    for c in shellscan.scan(text, data.get("cwd")):
        what = refusal(c.argv)
        if what:
            return ("Blocked: `%s` installs tooling on this machine or waits on an install prompt (ER-14, ER-45). "
                    "Next step: coverage is measured by CI's llvm-cov gate (read its result with the verdict reader or the run log), "
                    "and the user installs toolchains, components and targets; if one is missing, stop and report which." % what)
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

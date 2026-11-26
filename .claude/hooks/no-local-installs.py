#!/usr/bin/env python3
"""PreToolUse(Bash): refuse an agent's commands that install tooling or measure coverage locally (ER-14, ER-45).

Refused: `cargo llvm-cov` (also `cargo +nightly llvm-cov`, `cargo-llvm-cov`), `rustup component add`, `rustup toolchain install`,
`rustup target add` (also `rustup install`, `rustup update`, `rustup self update`, `rustup default <version>`,
`rustup toolchain add`, `cargo install cargo-llvm-cov`), wherever the command sits in a line, behind a wrapper
(`throttle run ... --`, `env`, `timeout`, `bash -c`) or in a `&&` chain. Text in heredocs, comments and quoted arguments is not a
command (lib/shellscan.py). Applies to agents only (agent_id set), like docker-guard and kill-guard. Never fails closed."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "lib"))
import shellscan

NAME = "no-local-installs"
# Programs that start the command they are given: the tool follows them, after options, assignments, numbers or `--`.
WRAPPERS = {"env", "time", "nice", "nohup", "timeout", "heavy", "throttle", "dev", "sudo", "command", "exec", "xargs", "caffeinate", "taskpolicy"}
INSTALL = {("component", "add"), ("toolchain", "install"), ("toolchain", "add"), ("target", "add")}
# cargo options that take a value as the next word (`--opt=value` carries its own).
CARGO_VALUE_OPTS = {"--config", "-C", "-Z", "--manifest-path", "--color", "--target-dir", "--explain"}


# Options of a wrapper that take a value as the next word (`-u X`); `--opt=value` and numbers are skipped anyway.
WRAPPER_VALUE_OPTS = {
    "env": {"-u", "-C", "-S", "--unset", "--chdir", "--split-string"},
    "timeout": {"-s", "-k", "--signal", "--kill-after"},
    "nice": {"-n", "--adjustment"},
    "xargs": {"-I", "-n", "-P", "-L", "-d", "-E", "-s", "-a"},
    "time": {"-o", "-f", "--output", "--format"},
    "sudo": {"-u", "-g", "-C", "-h", "-p", "-r", "-t", "-U"},
}


def program_index(argv):
    """Index of the program a command really starts: past wrappers and their options, or past the first `--` after one."""
    i = 0
    # The scanner drops the keyword `time` and leaves its options in front: `time -o f cargo llvm-cov` reads as `-o f cargo llvm-cov`.
    while i < len(argv) and argv[i].startswith("-"):
        i += 2 if argv[i] in WRAPPER_VALUE_OPTS["time"] else 1
    while i < len(argv) and os.path.basename(argv[i]) in WRAPPERS:
        if "--" in argv[i + 1:]:
            return argv.index("--", i + 1) + 1
        valued = WRAPPER_VALUE_OPTS.get(os.path.basename(argv[i]), set())
        i += 1
        while i < len(argv) and (argv[i].startswith("-") or "=" in argv[i] or argv[i].isdigit()):
            i += 2 if argv[i] in valued else 1
    return i


def cargo_args(rest):
    """The words of a cargo command line after its options (`+toolchain`, `--config k=v`, `-C dir`): subcommand first."""
    out, k = [], 0
    while k < len(rest):
        a = rest[k]
        if not out and a.startswith("+"):
            k += 1
        elif not out and a.startswith("-"):
            k += 2 if a in CARGO_VALUE_OPTS else 1
        else:
            out.append(a)
            k += 1
    return out


def rustup_args(rest):
    """rustup's words without its global options (`-v`, `-q`) and a `+toolchain` override (none of them takes a value)."""
    return [a for a in rest if not a.startswith(("-", "+"))]


def refusal(argv):
    i = program_index(argv)
    if i >= len(argv):
        return None
    prog, rest = os.path.basename(argv[i]), argv[i + 1:]
    if prog == "cargo-llvm-cov":
        return "cargo llvm-cov"
    if prog == "cargo":
        words = cargo_args(rest)
        if words[:1] == ["llvm-cov"]:
            return "cargo llvm-cov"
        if words[:1] == ["install"] and any(os.path.basename(w).startswith("cargo-llvm-cov") for w in words[1:]):
            return "cargo install cargo-llvm-cov"
    if prog == "rustup":
        words = rustup_args(rest)
        if words[:1] in (["install"], ["update"]):
            return "rustup " + words[0]
        if words[:2] == ["self", "update"]:
            return "rustup self update"
        if words[:1] == ["default"] and len(words) >= 2:
            return "rustup default " + words[1]
        if len(words) >= 2 and (words[0], words[1]) in INSTALL:
            return "rustup %s %s" % (words[0], words[1])
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

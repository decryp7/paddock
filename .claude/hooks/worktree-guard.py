#!/usr/bin/env python3
"""The ER-2 guard (RT-7), blocking, on lib/shellscan.py. See worktree-guard.sh for the rules."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "lib"))
import shellscan, paddockpaths


NAME = "worktree-guard"
MAIN = paddockpaths.main_checkout()  # None when it cannot be derived: git writes by agents are then refused

WRITES = {"checkout", "switch", "commit", "merge", "rebase", "reset", "push", "cherry-pick", "revert", "am", "pull",
          "restore", "clean", "stash", "update-ref"}
BRANCH_W = {"-D", "-d", "-m", "-M", "--delete", "--move", "--force", "-f", "-C", "-c", "--copy"}
TAG_RO = {"-l", "--list", "-n", "-v", "--contains", "--points-at", "--no-contains", "--merged", "--no-merged"}
WT_W = {"remove", "prune", "move", "repair"}


def norm(p):
    return os.path.realpath(p).casefold()


def in_main(d):
    if MAIN is None:
        return True
    d, m = norm(d), norm(MAIN)
    return d == m or d.startswith(m + os.sep)


def short_n(args):
    """`-n` among a commit's short flags (`-n`, `-nm`, `-anm`); letters after one that takes a value (-m, -F, -C, -c, -t) are
    that value, not flags."""
    for a in args:
        if a.startswith("-") and not a.startswith("--") and len(a) > 1:
            for ch in a[1:]:
                if ch == "n":
                    return True
                if ch in "mFCctSu":
                    break
    return False


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


# An allow-list: a subagent's `git config` that names `paddock` passes only as a lookup (these flags, the get/list subcommands, or one
# bare key). Every other option (-z, --includes, --show-scope, --fixed-value ...) only modifies a read or a write, so it decides nothing.
CONFIG_LOOKUP_FLAGS = {"--get", "--get-all", "--get-regexp", "--get-urlmatch", "--list", "-l"}
CONFIG_WRITE_FLAGS = {"--unset", "--unset-all", "--add", "--replace-all", "--rename-section", "--remove-section", "--edit", "-e"}
CONFIG_SCOPE = {"--local", "--global", "--system", "--worktree", "--file", "-f", "--blob"}


CONFIG_VALUE_OPTIONS = ("--file", "-f", "--blob", "--type", "-t", "--default", "--comment")


def _config_positionals(args):
    """The arguments of `git config` that are not options or the values of options that take one (`--file x`, `-f x`; the
    joined `--file=x` starts with a dash and is skipped as an option)."""
    out, skip = [], False
    for a in args:
        if skip:
            skip = False
        elif a in CONFIG_VALUE_OPTIONS:
            skip = True
        elif not a.startswith("-"):
            out.append(a)
    return out


def config_write(args, key_pred):
    """True when `git config <args>` writes a key `key_pred` accepts (a lower-cased name; `--edit` counts for every predicate);
    lookups (`--get`, `get`, `--list`, one bare key) are False."""
    if not args:
        return False
    keys = _config_positionals(args)
    sub = None
    if keys and keys[0] in ("get", "list", "set", "unset", "rename-section", "remove-section", "edit"):
        sub, keys = keys[0], keys[1:]
    names_key = any(key_pred(k.lower()) for k in keys[:1])
    if not names_key and not (sub == "edit" or "--edit" in args or "-e" in args):
        return False
    if sub in ("get", "list"):
        return False
    if sub in ("set", "unset", "rename-section", "remove-section", "edit"):
        return True
    if any(a in CONFIG_WRITE_FLAGS for a in args):
        return True
    if any(a in CONFIG_LOOKUP_FLAGS for a in args):
        return False
    # `git config <key> <value>` writes; `git config <key>` reads. The type options (--type, --bool, ...) only canonicalise.
    return len(keys) >= 2


def paddock_config_write(args):
    """True when `git config <args>` writes a `paddock.*` key or the `paddock` section (the shared common config that names the
    machine's dev folders, docs/dev-setup.md)."""
    return config_write(args, lambda k: k == "paddock" or k.startswith("paddock."))


def hooks_path_write(args):
    """True when `git config <args>` writes or unsets `core.hooksPath`, which switches every hook off like `--no-verify`."""
    return config_write(args, lambda k: k == HOOKS_PATH)


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


HOOKS_PATH = "core.hookspath"
# The commands whose hooks run (or whose result is a commit): a config override on one of them can point git at other hooks.
OVERRIDE_SUBS = ("commit", "push", "merge", "cherry-pick", "rebase", "am", "revert")
# git options and environment variables that change which config (and so which hooks, identity or repository) a command reads.
CONFIG_OPTIONS = ("-c", "--config-env", "--exec-path", "--git-dir")
CONFIG_ENV = {"GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM", "GIT_CONFIG_NOSYSTEM", "GIT_CONFIG_COUNT", "GIT_CONFIG_PARAMETERS",
              "HOME", "XDG_CONFIG_HOME", "GIT_DIR", "GIT_EXEC_PATH"}
CONFIG_ENV_PREFIXES = ("GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_")


def config_override(c):
    """What overrides git's configuration for this command (any `-c`, `--config-env`, `--exec-path`, `--git-dir`, or an
    environment variable of CONFIG_ENV), or None. An outcome rule, not a list of spellings: every override of the config of a
    commit, push, merge, cherry-pick, rebase, am or revert can switch the hooks off."""
    a = c.argv
    k = 1
    while k < len(a) and a[k].startswith("-") and a[k] != "--":
        if a[k] == "-c" or a[k].split("=", 1)[0] in CONFIG_OPTIONS:
            return "git %s" % " ".join(a[k:k + 2] if a[k] in shellscan._GIT_OPT_ARG else a[k:k + 1])
        k += 2 if a[k] in shellscan._GIT_OPT_ARG else 1
    for e in c.env:
        name = e.partition("=")[0]
        if name in CONFIG_ENV or name.startswith(CONFIG_ENV_PREFIXES):
            return "the environment variable %s" % name
    return None


def decide(data):
    if data.get("tool_name") != "Bash":
        return None
    text = (data.get("tool_input") or {}).get("command") or ""
    is_agent = bool(data.get("agent_id"))
    cmds = shellscan.scan(text, data.get("cwd"))
    pending_commit, chain_has_commit = False, False   # a commit whose failure would not stop a later push
    for c in cmds:
        g0 = shellscan.git_invocation(c)
        if g0 and not c.cond:
            if g0[1] == "push" and pending_commit and not chain_has_commit:
                return ("Blocked: `git push` follows a `git commit` that is not joined to it by `&&` (the commit-msg hook can "
                        "reject the commit and the push still runs; that happened twice on 2026-11-12). Next step: write "
                        "`git commit ... && git push`, or `git commit ... || exit 1; git push`.")
            if g0[1] == "commit" and not c.set_e:
                pending_commit, chain_has_commit = True, True
                if c.term == "||" and closes_with_exit(cmds, c):
                    pending_commit = False
        if c.term in (";", "\n", "&"):
            chain_has_commit = False
        if c.is_cd and not c.cond and not c.nested:
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
        if is_agent:
            ov = config_override(c) if sub in OVERRIDE_SUBS else None
            if ov:
                return ("Blocked (ER-39, hooks are never bypassed): `git %s` with a config override (%s). Any override of git's "
                        "configuration can point git at other hooks (core.hooksPath, include.path, another HOME or "
                        "GIT_CONFIG_GLOBAL) and skips lefthook like --no-verify. Next step: run it without the override; if a "
                        "hook fails, fix the hook (or cherry-pick its fix) or stop and report." % (sub, ov))
        if is_agent and sub in OVERRIDE_SUBS:
            bypass = [e for e in c.env if e.split("=", 1)[0] in ("LEFTHOOK_EXCLUDE",) or e in ("LEFTHOOK=0", "LEFTHOOK=false")]
            if bypass or (sub in ("commit", "push") and "--no-verify" in args) or (sub == "commit" and short_n(args)):
                return ("Blocked: git hooks are bypassed (%s). Hooks must pass: cherry-pick the hook fix or stop and report "
                        "(no bypass)." % (bypass[0] if bypass else "--no-verify"))
        if is_agent and sub == "config" and hooks_path_write(args) and not paddock_config_write(args):
            return ("Blocked (ER-39, hooks are never bypassed): `git config` writes or unsets core.hooksPath, which points git at "
                    "other hooks and skips lefthook like --no-verify. Next step: leave it; if a hook fails, fix the hook or stop "
                    "and report. Reading it (`git config --get core.hooksPath`) is fine.")
        if is_agent and sub == "config" and paddock_config_write(args):
            return ("Blocked: `git config` writes a `paddock.*` key by a subagent. It lives in the common config every worktree "
                    "shares and moves the dev, VM, lock and disk folders for everyone (docs/dev-setup.md, \"Where things "
                    "live\"). Next step: stop and ask the coordinator; read it with `git config --get` or `./dev places`.")
        if not is_write(sub, args):
            continue
        if c.after_cd_chain:
            return ("Blocked: `git %s` runs after a `cd X && ...` chain ended by `;` or a newline (unguarded cd): if "
                    "the cd failed, it runs in the wrong folder. Next step: keep it in the chain (`cd X && ... && git %s`)"
                    ", or use `git -C <worktree> %s`." % (sub, sub, sub))
        if is_agent and MAIN is None:
            return ("Blocked: `git %s` by a subagent, and the main checkout could not be derived from git (CLAUDE_PROJECT_DIR "
                    "is not in a git checkout), so the ER-2 check cannot run. Next step: run from the repository, or set "
                    "WORKTREE_GUARD_MAIN to the main checkout's path." % sub)
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

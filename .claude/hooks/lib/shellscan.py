"""One shell scanner for every Bash hook (ER-21: each hook had its own regex and its own false positives).

`scan(text, cwd)` splits a command line into simple commands (on `;`, newlines, `&&`, `||`, pipes, `&`, `(`/`)` and
`$( )`), skips heredoc bodies and comments, removes quotes, resolves `NAME=value` variables where it can, and gives each
command its effective directory. Linear time: one pass over the text, no regex over unbounded input. Over MAX_BYTES it
raises TooBig, and the hook allows the command with a warning (never fail closed).
"""
import os
import re
import sys

MAX_BYTES = 200 * 1024
MAX_NEST = 40


class TooBig(Exception):
    pass


class Cmd:
    __slots__ = ("words", "argv", "redirs", "term", "cwd", "depth", "cond", "is_cd", "cd_target", "after_cd_chain",
                 "opens", "set_e", "idx", "env")

    def __init__(self):
        self.words = []
        self.argv = []
        self.redirs = []      # (operator, target word)
        self.term = ""        # the operator that ends it: ; \n && || | & ) ;; or ""
        self.cwd = None       # directory it runs in; None when unknown
        self.depth = 0        # subshell nesting
        self.cond = False     # the condition of if/while/until
        self.is_cd = False
        self.cd_target = ""
        self.after_cd_chain = False   # runs after a `cd X && ...;` chain: in the wrong place if the cd failed
        self.opens = 0        # leading `{` count
        self.set_e = False    # `set -e` was seen before it
        self.idx = 0
        self.env = []         # NAME=value words that set the command's environment (prefix or `env X=1`)


# ---- lexer ----

_FD = re.compile(r"^[0-9]+$")
_SPACE = " \t\r"


def _lex(s, i, closer, level):
    """Tokens of s from i until `closer` (')' or '`') or the end; returns (tokens, next index)."""
    n = len(s)
    toks, buf = [], []
    inword = False
    pend = []     # heredocs waiting for their body at the next newline
    pd = 0

    def flush():
        nonlocal buf, inword
        if inword:
            toks.append(("w", "".join(buf)))
        buf, inword = [], False

    def sub(j, kind):
        """A `$(`, backtick or <( ) starting after its opener at j: lex it, return the index after it."""
        if level >= MAX_NEST:
            return j
        inner, k = _lex(s, j, kind, level + 1)
        toks.append(("sub", inner))
        return k

    while i < n:
        c = s[i]
        if c == "\\":
            if i + 1 < n:
                if s[i + 1] != "\n":
                    buf.append(s[i + 1])
                    inword = True
                i += 2
            else:
                i += 1
            continue
        if c == "'":
            j = s.find("'", i + 1)
            j = n if j < 0 else j
            buf.append(s[i + 1:j])
            inword = True
            i = j + 1
            continue
        if c == '"':
            i += 1
            inword = True
            while i < n and s[i] != '"':
                ch = s[i]
                if ch == "\\" and i + 1 < n:
                    if s[i + 1] in '$`"\\':
                        buf.append(s[i + 1])
                    elif s[i + 1] != "\n":
                        buf.append(ch + s[i + 1])
                    i += 2
                elif ch == "$" and s.startswith("$((", i):
                    i = _skip_arith(s, i + 3)
                    buf.append("0")
                elif ch == "$" and s.startswith("$(", i):
                    i = sub(i + 2, ")")
                    buf.append("$(..)")
                elif ch == "`":
                    i = sub(i + 1, "`")
                    buf.append("`..`")
                else:
                    buf.append(ch)
                    i += 1
            i += 1
            continue
        if c == "$":
            if s.startswith("$((", i):
                i = _skip_arith(s, i + 3)
                buf.append("0")
                inword = True
                continue
            if s.startswith("$(", i):
                i = sub(i + 2, ")")
                buf.append("$(..)")
                inword = True
                continue
            if s.startswith("${", i):
                j, depth = i + 2, 1
                while j < n and depth:
                    depth += (s[j] == "{") - (s[j] == "}")
                    j += 1
                buf.append(s[i:j])
                inword = True
                i = j
                continue
            buf.append(c)
            inword = True
            i += 1
            continue
        if c == "`":
            if closer == "`":
                flush()
                return toks, i + 1
            i = sub(i + 1, "`")
            buf.append("`..`")
            inword = True
            continue
        if c in _SPACE:
            flush()
            i += 1
            continue
        if c == "\n":
            flush()
            toks.append(("op", "\n"))
            i += 1
            for delim, dash in pend:
                while i < n:
                    j = s.find("\n", i)
                    line = s[i:(n if j < 0 else j)]
                    i = n if j < 0 else j + 1
                    line = line.rstrip("\r")
                    if (line.lstrip("\t") if dash else line) == delim:
                        break
            pend = []
            continue
        if c == "#" and not inword:
            j = s.find("\n", i)
            i = n if j < 0 else j
            continue
        if c == ";":
            flush()
            if s.startswith(";;", i) or s.startswith(";&", i):
                toks.append(("op", ";;"))
                i += 2
            else:
                toks.append(("op", ";"))
                i += 1
            continue
        if c == "&" and not s.startswith("&>", i):
            flush()
            if s.startswith("&&", i):
                toks.append(("op", "&&"))
                i += 2
            else:
                toks.append(("op", "&"))
                i += 1
            continue
        if c == "|":
            flush()
            if s.startswith("||", i):
                toks.append(("op", "||"))
                i += 2
            elif s.startswith("|&", i):
                toks.append(("op", "|"))
                i += 2
            else:
                toks.append(("op", "|"))
                i += 1
            continue
        if c == "(":
            if not inword and s.startswith("((", i):
                i = _skip_arith(s, i + 2)
                continue
            flush()
            toks.append(("op", "("))
            pd += 1
            i += 1
            continue
        if c == ")":
            flush()
            if pd > 0:
                pd -= 1
                toks.append(("op", ")"))
            elif closer == ")":
                return toks, i + 1
            i += 1
            continue
        if c in "<>&":
            if c in "<>" and s.startswith("(", i + 1):
                flush()
                i = sub(i + 2, ")")
                buf.append("/dev/fd/0")
                inword = True
                continue
            if inword and _FD.match("".join(buf)):
                buf, inword = [], False
            else:
                flush()
            if s.startswith("&>>", i):
                op = "&>>"
            elif s.startswith("&>", i):
                op = "&>"
            elif s.startswith("<<<", i):
                op = "<<<"
            elif s.startswith("<<-", i):
                op = "<<-"
            elif c == "<" and s.startswith("<<", i):
                op = "<<"
            elif s.startswith(">>", i):
                op = ">>"
            elif s.startswith(">|", i):
                op = ">|"
            elif s.startswith(">&", i) or s.startswith("<&", i):
                op = c + "&"
            elif s.startswith("<>", i):
                op = "<>"
            else:
                op = c
            i += len(op)
            if op in ("<<", "<<-"):
                while i < n and s[i] in _SPACE:
                    i += 1
                d, i = _heredoc_word(s, i)
                pend.append((d, op == "<<-"))
                toks.append(("redir", op, d))
            else:
                toks.append(("redir", op))
            continue
        buf.append(c)
        inword = True
        i += 1
    flush()
    return toks, i


def _skip_arith(s, i):
    """Index after the `))` closing an arithmetic expression whose `((` ended at i."""
    n, depth = len(s), 0
    while i < n:
        if s[i] == "(":
            depth += 1
        elif s[i] == ")":
            if depth == 0:
                return i + 2 if s.startswith("))", i) else i + 1
            depth -= 1
        i += 1
    return n


def _heredoc_word(s, i):
    n, out = len(s), []
    while i < n and s[i] not in " \t\r\n;&|()<>":
        c = s[i]
        if c in "'\"":
            j = s.find(c, i + 1)
            j = n if j < 0 else j
            out.append(s[i + 1:j])
            i = j + 1
        elif c == "\\" and i + 1 < n:
            out.append(s[i + 1])
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out), i


# ---- parser ----

_ASSIGN = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
_VAR = re.compile(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)")
_LEAD = {"if", "while", "until", "elif", "then", "do", "else", "!", "{", "time", "coproc"}
_ENV_ARG = {"-u", "-C", "-S", "--unset", "--chdir"}
_XARGS_ARG = {"-I", "-n", "-P", "-L", "-d", "-E", "-s", "-a", "-J"}


class _State:
    def __init__(self, cwd, home):
        self.cwd = cwd
        self.home = home
        self.vars = {}
        self.exported = []
        self.cmds = []
        self.set_e = False
        self.chain_cd = False
        self.chain_cond = False
        self.maybe_wrong = False
        self.at_chain_start = True


def expand(word, vars_):
    if "$" not in word:
        return word
    return _VAR.sub(lambda m: vars_.get(m.group(1) or m.group(2), m.group(0)), word)


def resolve(cur, target, home):
    """Absolute normalised path of `target` seen from `cur`; None when it depends on something unknown."""
    if target is None or "$" in target or "`" in target or target == "-":
        return None
    if target == "~" or target.startswith("~/"):
        if not home:
            return None
        target = home + target[1:]
    if target.startswith("/"):
        return os.path.normpath(target)
    if cur is None:
        return None
    return os.path.normpath(os.path.join(cur, target))


def _strip(argv, env):
    """Remove wrappers (env, sudo, xargs, ...) from the front of a command; NAME=value words go to `env`."""
    argv = list(argv)
    while argv:
        a = argv[0]
        if a in ("command", "builtin", "nohup", "exec", "time"):
            argv = argv[1:]
        elif a in ("env", "sudo", "nice", "ionice", "caffeinate", "stdbuf"):
            argv = argv[1:]
            while argv and (argv[0].startswith("-") or _ASSIGN.match(argv[0])):
                if _ASSIGN.match(argv[0]):
                    env.append(argv[0])
                skip = 2 if argv[0] in _ENV_ARG or (a == "nice" and argv[0] == "-n") else 1
                argv = argv[skip:]
        elif a == "xargs":
            argv = argv[1:]
            while argv and argv[0].startswith("-"):
                argv = argv[2 if argv[0] in _XARGS_ARG else 1:]
        elif _ASSIGN.match(a):
            env.append(a)
            argv = argv[1:]
        else:
            break
    return argv


def _finish(st, words, redirs, term, depth):
    if not words and not redirs:
        return None
    cmd = Cmd()
    lead = 0
    w = [expand(x, st.vars) for x in words]
    while w and w[0] in _LEAD:
        if w[0] in ("if", "while", "until", "elif"):
            cmd.cond = True
        if w[0] == "{":
            cmd.opens += 1
        w = w[1:]
        lead += 1
    cmd.words = w
    cmd.redirs = [(op, expand(t, st.vars)) for op, t in redirs]
    cmd.term = term
    cmd.depth = depth
    cmd.cwd = st.cwd
    cmd.set_e = st.set_e
    cmd.idx = len(st.cmds)
    if st.at_chain_start:
        st.chain_cond = cmd.cond
    cmd.after_cd_chain = st.maybe_wrong and not st.set_e
    # `NAME=value` alone sets a variable; before a command it only sets that command's environment.
    k = 0
    while k < len(w) and _ASSIGN.match(w[k]):
        k += 1
    if k and k == len(w):
        for a in w:
            name, _, val = a.partition("=")
            st.vars[name] = val
    elif w and w[0] in ("export", "declare", "local", "readonly"):
        for a in w[1:]:
            if _ASSIGN.match(a):
                name, _, val = a.partition("=")
                st.vars[name] = val
                if w[0] == "export":
                    st.exported.append(a)
            elif w[0] == "export" and a in st.vars:
                st.exported.append("%s=%s" % (a, st.vars[a]))
    env = list(w[:k]) if k < len(w) else []
    argv = _strip(w[k:] if k < len(w) else [], env)
    cmd.argv = argv
    cmd.env = env + [e for e in st.exported if not (argv and argv[0] == "export")]
    if argv and argv[0] == "set" and any(a.startswith("-") and not a.startswith("--") and "e" in a for a in argv[1:]):
        st.set_e = True
    if argv and argv[0] == "cd":
        args = [a for a in argv[1:] if a not in ("--", "-P", "-L", "-e", "-@")]
        cmd.is_cd = True
        cmd.cd_target = args[0] if args else "~"
        st.cwd = resolve(st.cwd, cmd.cd_target, st.home)
        st.maybe_wrong = False
        st.chain_cd = term == "&&"
    elif argv and argv[0] in ("pushd", "popd"):
        st.cwd = None
    st.cmds.append(cmd)
    if term in (";", "\n", ";;"):
        if st.chain_cd and not st.set_e and not st.chain_cond:
            st.maybe_wrong = True
        st.chain_cd = False
        st.at_chain_start = True
    elif term == "&":
        st.chain_cd = False
        st.at_chain_start = True
    else:
        st.at_chain_start = False
    return cmd


def _parse(toks, st, depth):
    words, redirs, pending = [], [], None
    stack = []

    def end(term):
        nonlocal words, redirs, pending
        _finish(st, words, redirs, term, depth + len(stack))
        words, redirs, pending = [], [], None

    for t in toks:
        k = t[0]
        if k == "w":
            if pending is not None:
                redirs.append((pending, t[1]))
                pending = None
            else:
                words.append(t[1])
        elif k == "redir":
            if len(t) == 3:
                redirs.append((t[1], t[2]))
            else:
                pending = t[1]
        elif k == "sub":
            saved = (st.cwd, st.maybe_wrong, st.chain_cd, st.chain_cond, st.at_chain_start)
            _parse(t[1], st, depth + len(stack) + 1)
            st.cwd, st.maybe_wrong, st.chain_cd, st.chain_cond, st.at_chain_start = saved
        else:
            op = t[1]
            if op == "(":
                end("(")
                stack.append((st.cwd, st.maybe_wrong, st.chain_cd, st.chain_cond))
                st.at_chain_start = True
            elif op == ")":
                end(")")
                if stack:
                    st.cwd, st.maybe_wrong, st.chain_cd, st.chain_cond = stack.pop()
            else:
                end(op)
    end("")


def scan(text, cwd=None, home=None, max_bytes=MAX_BYTES):
    """The simple commands of `text` in order, each with its effective cwd. Raises TooBig over `max_bytes`."""
    if len(text) > max_bytes:
        raise TooBig("%d bytes" % len(text))
    toks, _ = _lex(text, 0, None, 0)
    st = _State(os.path.normpath(cwd) if cwd else None, home or os.path.expanduser("~"))
    _parse(toks, st, 0)
    return st.cmds


# ---- helpers the hooks share ----

_GIT_OPT_ARG = {"-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path", "--super-prefix", "--config-env"}


def git_invocation(cmd, home=None):
    """(directory or None, subcommand, args) of a git command, with the `-C` directories applied; None if not git."""
    a = cmd.argv
    if not a or a[0] != "git":
        return None
    eff, k = cmd.cwd, 1
    while k < len(a) and a[k].startswith("-"):
        if a[k] in _GIT_OPT_ARG and k + 1 < len(a):
            if a[k] == "-C":
                eff = resolve(eff, a[k + 1], home or os.path.expanduser("~"))
            k += 2
        else:
            k += 1
    if k >= len(a):
        return None
    return eff, a[k], a[k + 1:]


def next_cmd(cmds, cmd):
    """The command that follows `cmd` at its nesting level, or None at the end of the level or at a closing word."""
    for c in cmds[cmd.idx + 1:]:
        if c.depth < cmd.depth:
            return None
        if c.depth > cmd.depth:
            continue
        if c.words and c.words[0] in ("fi", "done", "esac", "}", ";;"):
            return None
        return c
    return None


def deny_json(reason):
    import json
    return json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
                                              "permissionDecisionReason": reason}})


def warn(hook, msg):
    print("%s: %s; allowing" % (hook, msg), file=sys.stderr)

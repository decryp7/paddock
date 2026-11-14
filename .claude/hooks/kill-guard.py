#!/usr/bin/env python3
"""The kill guard (rule 53), blocking, on lib/shellscan.py. See kill-guard.sh for the rules and the known gaps."""
import json, os, re, subprocess, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "lib"))
import shellscan

NAME = "kill-guard"
WHY = ("A pid is a hint, never an owner (rule 53): on 2026-11-14 an integrator killed another agent's pre-commit by pid, "
       "and every agent runs the same tools.")
NEXT = ("Next step: start a long process with the Bash tool's run_in_background so the TaskStop tool can end it, or end it with "
        "`kill %<job>` or `kill $!` in the command that started it, or `cd <your worktree> && kill <pid>` for a process of yours "
        "running there (paddock_proc tools through `./dev` are fine). A process started with nohup, setsid or disown, or one "
        "running in a scratch folder, cannot be proven yours: report it to its owner or the coordinator instead.")
BY_NAME = {"pkill", "killall", "killall5", "pgrep-kill"}
SIG_ARG = {"-s", "-n", "--signal"}
PID = re.compile(r"^[0-9]+$")
KILLISH = re.compile(r"^(-?[0-9]+|%.*|-s|--signal|-(SIG)?(HUP|INT|QUIT|ILL|TRAP|ABRT|KILL|USR1|USR2|SEGV|PIPE|ALRM|TERM|STOP|CONT|TSTP))$")
PROC_TIMEOUT = 3


def refuse(what):
    return "Blocked: %s. %s %s" % (what, WHY, NEXT)


def kill_args(argv):
    """(signal or None, operands, lists) of a `kill` command line."""
    sig, ops, lists, k = None, [], False, 1
    while k < len(argv):
        a = argv[k]
        if ops:
            ops.append(a)
        elif a == "--":
            ops += argv[k + 1:]
            break
        elif a in ("-l", "-L", "--list"):
            lists = True
        elif a in SIG_ARG:
            sig = argv[k + 1] if k + 1 < len(argv) else None
            k += 1
        elif a.startswith("-") and len(a) > 1 and sig is None:
            sig = a[1:]
        else:
            ops.append(a)
        k += 1
    return sig, ops, lists


def own_worktree(d):
    """The linked worktree (a folder whose `.git` is a file) that holds folder d, or None (main checkout, no repo, unknown)."""
    if not d:
        return None
    d = os.path.realpath(d)
    while True:
        g = os.path.join(d, ".git")
        if os.path.isfile(g):
            return d
        if os.path.isdir(g):
            return None
        up = os.path.dirname(d)
        if up == d:
            return None
        d = up


def etime_seconds(t):
    """Seconds of a `ps` elapsed time ([[dd-]hh:]mm:ss)."""
    days, _, rest = t.rpartition("-")
    secs = 0
    for part in rest.split(":"):
        secs = secs * 60 + int(part)
    return secs + (int(days) * 86400 if days else 0)


def process_table():
    """{pid: (ppid, seconds since it started)} of every process, from `ps`."""
    out = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,etime="], capture_output=True, text=True,
                         timeout=PROC_TIMEOUT).stdout
    table = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[0].isdigit() and parts[1].isdigit():
            try:
                table[int(parts[0])] = (int(parts[1]), etime_seconds(parts[2]))
            except ValueError:
                continue
    return table


def session_root(table):
    """The Claude process this hook runs under: KILL_GUARD_ROOT_PID (tests), CLAUDE_PID, or the nearest `claude` ancestor."""
    for var in ("KILL_GUARD_ROOT_PID", "CLAUDE_PID"):
        v = os.environ.get(var, "")
        if v.isdigit():
            return int(v)
    pid = os.getppid()
    seen = set()
    while pid > 1 and pid not in seen:
        seen.add(pid)
        out = subprocess.run(["ps", "-o", "comm=", "-p", str(pid)], capture_output=True, text=True, timeout=PROC_TIMEOUT)
        if os.path.basename(out.stdout.strip()) == "claude":
            return pid
        pid = table.get(pid, (0, 0))[0]
    return None


def descends_from(pid, root, table):
    """True when root is an ancestor of pid by parent pids and by age (rule 53: a child never started before its parent, so a
    reused parent pid does not join the tree)."""
    seen = set()
    while pid in table and pid not in seen:
        if pid == root:
            return True
        seen.add(pid)
        ppid, age = table[pid]
        if ppid in table and table[ppid][1] < age:
            return False
        pid = ppid
    return pid == root


def proc_cwd(pid):
    """The working folder of pid, or None when it cannot be read."""
    link = "/proc/%d/cwd" % pid
    if os.path.exists("/proc/self/cwd"):
        try:
            return os.path.realpath(os.readlink(link))
        except OSError:
            return None
    out = subprocess.run(["lsof", "-a", "-p", str(pid), "-d", "cwd", "-Fn"], capture_output=True, text=True,
                         timeout=PROC_TIMEOUT).stdout
    for line in out.splitlines():
        if line.startswith("n/"):
            return os.path.realpath(line[1:])
    return None


def inside(path, folder):
    return path == folder or path.startswith(folder.rstrip(os.sep) + os.sep)


class Prover:
    """Proves a literal pid is the calling agent's: alive, a descendant of this Claude session, its cwd in the agent's worktree."""

    def __init__(self):
        self._table = None
        self._root = False

    def table(self):
        if self._table is None:
            self._table = process_table()
        return self._table

    def root(self):
        if self._root is False:
            self._root = session_root(self.table())
        return self._root

    def why_not(self, pid, worktree):
        if worktree is None:
            return "the command does not run in your own worktree, so no process can be shown to be yours"
        t = self.table()
        if pid not in t:
            return "there is no process %d" % pid
        root = self.root()
        if root is None or not descends_from(pid, root, t):
            return "process %d was not started from this Claude session" % pid
        cwd = proc_cwd(pid)
        if cwd is None or not inside(cwd, worktree):
            return "process %d runs in %s, not in your worktree %s" % (pid, cwd or "an unknown folder", worktree)
        return None


def check(c, cmds, prover):
    a = c.argv
    if not a:
        return None
    if shellscan.dynamic_argv0(c) and (any(KILLISH.match(x) for x in a[1:]) or any(
            os.path.basename(x) in BY_NAME | {"kill"} for f in c.subs for x in f.argv[:1] + f.argv[1:])):
        return refuse("`%s` is a program named at run time with pid-like arguments, so the hook cannot see whether it is a kill"
                      % a[0])
    n = os.path.basename(a[0])
    if n == "fuser" and any(x == "--kill" or (x.startswith("-") and not x.startswith("--") and "k" in x[1:]) for x in a[1:]):
        return refuse("`fuser -k` ends every process that uses a file, whoever owns it")
    if n in BY_NAME:
        return refuse("`%s` ends processes by name, and other agents run the same programs" % n)
    if n != "kill":
        return None
    sig, ops, lists = kill_args(a)
    if lists or sig == "0" or not ops and not c.xargs:
        return None
    if c.xargs:
        return refuse("`kill` fed by xargs kills pids found by a listing, not processes you hold")
    worktree = own_worktree(c.cwd)
    for op in ops:
        if op.startswith("%"):
            continue
        if op in ("$!", "${!}", "-$!", "-${!}"):
            continue
        if op in c.tainted or "$(..)" in op or "`..`" in op:
            return refuse("`kill %s` takes its pid from a command's output, not from a process you hold" % op)
        if "$" in op:
            return refuse("`kill %s`: the hook cannot see which process the variable names" % op)
        if op.startswith("-") or not PID.match(op) or int(op) <= 1:
            return refuse("`kill %s` signals a process group or every process" % op if op.startswith("-") or op == "0"
                          else "`kill %s` names no single process of yours" % op)
        reason = prover.why_not(int(op), worktree)
        if reason:
            return refuse("`kill %s`: %s" % (op, reason))
    return None


def decide(data):
    if data.get("tool_name") != "Bash" or not data.get("agent_id"):
        return None
    cmds = shellscan.scan((data.get("tool_input") or {}).get("command") or "", data.get("cwd"))
    prover = Prover()
    for c in cmds:
        r = check(c, cmds, prover)
        if r:
            return r
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

#!/usr/bin/env python3
"""Agents stop only their own containers by name (ER-14). See docker-guard.sh."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "lib"))
import shellscan

NAME = "docker-guard"
MSG = "Agents stop only their own containers by name (ER-14); never mass-stop or touch Docker/OrbStack itself."
OK_DIRS = [d for d in os.environ.get("DOCKER_GUARD_OK_DIRS", "/Volumes/d8sst7/dev-targets:/Volumes/d7sst7/dev-targets:/private/tmp/claude-").split(":") if d]
MASS = {"stop", "kill", "rm", "restart"}
VALUE_FLAGS = {"-t", "--time", "-s", "--signal", "-n"}
DOCKER_OPT_ARG = {"-H", "--host", "-c", "--context", "--config", "-l", "--log-level", "--tlscacert", "--tlscert", "--tlskey"}
ORB_BAD = {"stop", "start", "restart", "shutdown", "quit", "reset", "config", "delete", "update"}
PROC_WORDS = ("docker", "orbstack", "com.orbstack")


def refuse(what):
    return "Blocked: %s. %s" % (what, MSG)


def sub_and_args(a):
    k = 1
    while k < len(a) and a[k].startswith("-"):
        k += 2 if a[k] in DOCKER_OPT_ARG else 1
    if k >= len(a):
        return None, []
    sub, rest = a[k], a[k + 1:]
    if sub in ("container", "system", "volume", "network", "image", "builder") and rest:
        return sub + " " + rest[0], rest[1:]
    return sub, rest


def operands(args):
    out, skip = [], False
    for x in args:
        if skip:
            skip = False
        elif x in VALUE_FLAGS:
            skip = True
        elif not (x.startswith("-") and x != "-"):
            out.append(x)
    return out


def substituted(x):
    return "$(..)" in x or "`..`" in x


def under_ok(d):
    return d is None or any(d == o or d.startswith(o.rstrip("/") + "/") or (o.endswith("-") and d.startswith(o)) for o in OK_DIRS)


def check_docker(c):
    a = c.argv
    sub, args = sub_and_args(a)
    if sub is None:
        return None
    flags = [x for x in args if x.startswith("-") and x != "-"]
    if sub.startswith("container "):
        sub = sub.split(" ", 1)[1]
        if sub == "prune":
            return refuse("`docker container prune` removes every stopped container")
    if sub in MASS:
        ops = operands(args)
        if not ops:
            return refuse("`docker %s` with no container name or id" % sub)
        if any(substituted(x) for x in ops) or c.xargs:
            return refuse("`docker %s` fed by command substitution or xargs" % sub)
        if any(f in ("--all", "-a") or (not f.startswith("--") and "a" in f[1:] and sub != "rm") or (sub == "rm" and not f.startswith("--") and f[1:] == "a") for f in flags):
            return refuse("`docker %s --all`" % sub)
        return None
    if sub in ("system prune", "volume prune", "network prune", "builder prune"):
        return refuse("`docker %s` removes things the agent did not create" % sub)
    if sub == "image prune" and any(f in ("-a", "--all") or (not f.startswith("--") and "a" in f[1:]) for f in flags):
        return refuse("`docker image prune -a`")
    if sub == "compose" or sub.startswith("compose"):
        return None
    if sub == "desktop" and args[:1] and args[0] in ("stop", "restart", "start", "reset", "uninstall", "update"):
        return refuse("`docker desktop %s` controls Docker itself" % args[0])
    return None


def compose_down(c):
    a = c.argv
    if a[:1] == ["docker-compose"]:
        rest = a[1:]
    elif a[:1] == ["docker"]:
        sub, rest = sub_and_args(a)
        if sub != "compose":
            return None
    else:
        return None
    if "down" not in rest:
        return None
    d, k = c.cwd, 0
    while k < len(rest):
        if rest[k] in ("--project-directory",) and k + 1 < len(rest):
            d = shellscan.resolve(c.cwd, rest[k + 1], None)
        elif rest[k] in ("-f", "--file") and k + 1 < len(rest):
            f = shellscan.resolve(c.cwd, rest[k + 1], None)
            d = os.path.dirname(f) if f else None
        k += 1
    if not under_ok(d):
        return refuse("`docker compose down` outside your own worktree or scratch folder (%s)" % d)
    return None


def check(c):
    a = c.argv
    if not a:
        return None
    n = os.path.basename(a[0])
    if n == "docker":
        return check_docker(c) or compose_down(c)
    if n == "docker-compose":
        return compose_down(c)
    if n in ("orb", "orbctl"):
        sub = next((x for x in a[1:] if not x.startswith("-")), "")
        if sub in ORB_BAD:
            return refuse("`%s %s` controls OrbStack itself" % (n, sub))
    if n == "osascript" and any(("orbstack" in x.lower() or "docker" in x.lower()) for x in a[1:]):
        return refuse("`osascript` driving OrbStack or Docker")
    if n in ("killall", "pkill", "pgrep-kill", "launchctl") and any(any(w in x.lower() for w in PROC_WORDS) for x in a[1:]):
        if n != "launchctl" or any(x in ("stop", "unload", "remove", "bootout", "kill", "disable") for x in a[1:]):
            return refuse("`%s` of a Docker or OrbStack process" % n)
    return None


def decide(data):
    if data.get("tool_name") != "Bash" or not data.get("agent_id"):
        return None
    for c in shellscan.scan((data.get("tool_input") or {}).get("command") or "", data.get("cwd")):
        r = check(c)
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

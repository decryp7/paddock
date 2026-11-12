#!/usr/bin/env python3
"""Refuse writing a FILE directly into the session scratchpad root. See scratch-root-guard.sh."""
import json, os, re, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "lib"))
import shellscan

NAME = "scratch-root-guard"
ROOT_RE = re.compile(r"^/private/tmp/claude-[^/]+/[^/]+/[^/]+/scratchpad$")
WRITE_TOOLS = ("Write", "Edit", "MultiEdit", "NotebookEdit")


def reason_for(path):
    return ("Blocked: %s is a file directly in the shared session scratchpad root (every agent's files mix there). Next "
            "step: use your own subfolder, <scratchpad>/<your-agent-id>/..., for example the one your brief names "
            "(creating and removing a folder there is fine)." % path)


class Guard:
    def __init__(self, data):
        self.extra = data.get("scratchpad_dir") or ""

    def norm(self, p):
        if p is None or "$" in p:
            return None
        p = os.path.normpath(p)
        if p.startswith("/tmp/"):
            p = "/private" + p
        return p

    def root_entry(self, p):
        """The path when it is directly in a scratchpad root (a file or folder of the root itself), else None."""
        p = self.norm(p)
        if not p:
            return None
        parent = os.path.dirname(p)
        if ROOT_RE.match(parent) or (self.extra and os.path.normpath(self.extra) == parent):
            return p
        return None

    def file_target(self, path, cwd, dir_ok=False):
        """Refusal reason when writing `path` creates or changes a file in the root."""
        if path is None:
            return None
        if not path.startswith("/") and "~" not in path[:1]:
            path = shellscan.resolve(cwd, path, None) if cwd else None
        p = self.root_entry(path) if path else None
        if not p:
            return None
        if dir_ok and os.path.isdir(p):
            return None
        return reason_for(p)


def targets(c):
    """(path, kind) pairs a command writes to; kind: file | dest | dir (a folder the command makes or removes)."""
    a = c.argv
    if not a:
        return []
    name, args = a[0], [x for x in a[1:]]
    flags = [x for x in args if x.startswith("-") and x != "-"]
    pos = [x for x in args if not (x.startswith("-") and x != "-")]
    out = []
    if name in ("tee", "touch", "truncate"):
        out = [(x, "file") for x in pos]
    elif name in ("cp", "mv", "install", "ln", "rsync"):
        tflag = None
        for k, x in enumerate(args):
            if x in ("-t", "--target-directory") and k + 1 < len(args):
                tflag = args[k + 1]
        if tflag is None and pos:
            last = a[-1]
            out = [(last, "dest")] if last in pos else []
    elif name == "rm":
        rec = any(re.match(r"^-[a-zA-Z]*[rR]", f) or f in ("--recursive",) for f in flags)
        out = [(x, "rmdir" if rec else "file") for x in pos]
    elif name in ("mkdir", "rmdir"):
        out = []
    elif name == "dd":
        out = [(x[3:], "file") for x in args if x.startswith("of=")]
    elif name == "curl":
        for k, x in enumerate(args):
            if x in ("-o", "--output") and k + 1 < len(args):
                out.append((args[k + 1], "file"))
    elif name == "wget":
        for k, x in enumerate(args):
            if x in ("-O", "--output-document") and k + 1 < len(args):
                out.append((args[k + 1], "file"))
    return out


def decide(data):
    tool = data.get("tool_name")
    ti = data.get("tool_input") or {}
    g = Guard(data)
    if tool in WRITE_TOOLS:
        p = ti.get("file_path") or ti.get("notebook_path") or ""
        return g.file_target(p, None) if p else None
    if tool != "Bash":
        return None
    for c in shellscan.scan(ti.get("command") or "", data.get("cwd")):
        for op, t in c.redirs:
            if op in (">", ">>", ">|", "&>", "&>>", "<>"):
                r = g.file_target(t, c.cwd)
                if r:
                    return r
        for path, kind in targets(c):
            r = g.file_target(path, c.cwd, dir_ok=kind in ("dest", "rmdir"))
            if r and kind == "rmdir" and not os.path.exists(path):
                r = None
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

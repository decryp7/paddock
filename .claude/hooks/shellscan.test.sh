#!/bin/bash
# Tests for lib/shellscan.py's word expansion. Run: bash .claude/hooks/shellscan.test.sh
here=$(cd "$(dirname "$0")" && pwd)
TMPDIR=/some/tmp K=kill EMPTY='' python3 -I - "$here/lib" <<'PY'
import os, sys
sys.path.insert(0, sys.argv[1])
import shellscan as s
fail = 0
def eq(name, got, want):
    global fail
    print(("ok: " if got == want else "FAIL: ") + name + ("" if got == want else " (got %r, want %r)" % (got, want)))
    fail |= got != want
eq("${TMPDIR:-/tmp} is the real TMPDIR", s.scan("ls ${TMPDIR:-/tmp}/x")[0].argv, ["ls", "/some/tmp/x"])
eq("${K:-echo} with K in the environment", s.scan("${K:-echo} 5")[0].argv, ["kill", "5"])
eq("${UNSET_X:-echo} takes the default", s.scan("${UNSET_X:-echo} 5")[0].argv, ["echo", "5"])
eq("${EMPTY:-d}: empty is the default", s.scan("ls ${EMPTY:-d}")[0].argv, ["ls", "d"])
eq("a variable the command sets wins", s.scan("K=echo; ${K:-x} 5")[1].argv, ["echo", "5"])
eq("$HOME is the home folder", s.scan("ls $HOME/a", home="/h")[0].argv, ["ls", "/h/a"])
eq("HOME set by the command wins", s.scan("HOME=/o; ls $HOME/a", home="/h")[1].argv, ["ls", "/o/a"])
eq("ANSI-C hex", s.scan(r"$'\x6bill' 1")[0].argv, ["kill", "1"])
eq("alias", s.scan("alias k='kill -9'; k 1")[1].argv, ["kill", "-9", "1"])
sys.exit(fail)
PY

#!/usr/bin/env python3
"""The Linux guest's accessibility driver (AT-SPI, the counterpart of axdrive.swift): drives the real Dyno window and the desktop's own dialogs.
  atspi.py tree <app regex> [depth]      the tree of every matching application: "<indent>role  name"
  atspi.py has <app regex> <name regex> [role]    exit 0 if such an element exists
  atspi.py press <app regex> <name regex> [role]  do the first action (click/press/activate) of the first such element
  atspi.py text <app regex> <name regex> <value>  set the text of a text field (read from the argument)
Run in the user's graphical session (DISPLAY, DBUS_SESSION_BUS_ADDRESS, XDG_RUNTIME_DIR); needs toolkit-accessibility on.
"""
import re, sys
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

def apps():
    d = Atspi.get_desktop(0)
    return [d.get_child_at_index(i) for i in range(d.get_child_count())]

def walk(e, depth=0, maxd=40):
    if e is None or depth > maxd:
        return
    yield e, depth
    try:
        n = e.get_child_count()
    except Exception:
        return
    for i in range(n):
        try:
            c = e.get_child_at_index(i)
        except Exception:
            continue
        yield from walk(c, depth + 1, maxd)

def role(e):
    try: return e.get_role_name()
    except Exception: return ""

def name(e):
    try: return e.get_name() or ""
    except Exception: return ""

def matching(app_re):
    return [a for a in apps() if a is not None and re.search(app_re, name(a) or "", re.I)]

def find(app_re, name_re, role_re=None):
    for a in matching(app_re):
        for e, _ in walk(a):
            if re.search(name_re, name(e)) and (role_re is None or re.search(role_re, role(e))):
                return e
    return None

cmd = sys.argv[1]
if cmd == "tree":
    maxd = int(sys.argv[3]) if len(sys.argv) > 3 else 12
    for a in matching(sys.argv[2]):
        for e, d in walk(a, 0, maxd):
            print("  " * d + role(e) + "  " + name(e))
elif cmd == "apps":
    for a in apps(): print(name(a))
elif cmd in ("has", "press"):
    e = find(sys.argv[2], sys.argv[3], sys.argv[4] if len(sys.argv) > 4 else None)
    if e is None:
        print("not found"); sys.exit(1)
    if cmd == "press":
        ai = e.get_action_iface() if hasattr(e, "get_action_iface") else None
        try:
            n = Atspi.Action.get_n_actions(e)
            if n < 1: print("no action"); sys.exit(2)
            Atspi.Action.do_action(e, 0)
        except Exception as x:
            print("action failed: %s" % x); sys.exit(2)
        print("pressed: " + name(e))
    else:
        print("found: " + role(e) + " " + name(e))
elif cmd == "text":
    e = find(sys.argv[2], sys.argv[3], "entry|text|password")
    if e is None: print("not found"); sys.exit(1)
    Atspi.EditableText.set_text_contents(e, sys.argv[4]); print("set")

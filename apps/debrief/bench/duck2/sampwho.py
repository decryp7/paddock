"""usage: python3 -I sampwho.py FILE THREAD_SUBSTR FRAME_SUBSTR [CALLER_SUBSTR] -- for self samples in threads matching THREAD_SUBSTR
whose stack has a frame matching FRAME_SUBSTR, the nearest outer frame matching CALLER_SUBSTR (default 'debrief'), with counts."""
import re, sys, collections
f, th_s, fr_s = sys.argv[1:4]; cl_s = sys.argv[4] if len(sys.argv) > 4 else "debrief"
lines = open(f, errors="replace").read().split("\n")
start = next(i for i, l in enumerate(lines) if l.startswith("Call graph:")) + 1
node_re = re.compile(r"^(\s*[+!:| ]*)(\d+) (.*)$")
nodes = []; thread = ""
for l in lines[start:]:
    if not l.strip(): break
    m = node_re.match(l)
    if not m: continue
    d, c, s = len(m.group(1)), int(m.group(2)), m.group(3)
    if "Thread_" in s and d == 4: thread = s; continue
    nodes.append((thread, d, c, s))
def short(s):
    s = s.split("  (in")[0]
    toks = re.findall(r"\d+([A-Za-z_][A-Za-z0-9_]*)", s)
    return "::".join(t for t in toks if not t.startswith("Cs"))[-110:] or s[:110]
out = collections.Counter(); stack = []
for i, (th, d, c, s) in enumerate(nodes):
    while stack and stack[-1][0] >= d: stack.pop()
    stack.append((d, s))
    if th_s not in th: continue
    j = i + 1; cd = None; child = 0
    while j < len(nodes) and nodes[j][0] == th and nodes[j][1] > d:
        if cd is None: cd = nodes[j][1]
        if nodes[j][1] == cd: child += nodes[j][2]
        j += 1
    sc = c - child
    if sc <= 0: continue
    path = [x[1] for x in stack]
    k = next((idx for idx in range(len(path) - 1, -1, -1) if fr_s in path[idx]), None)
    if k is None: continue
    caller = next((path[idx] for idx in range(k - 1, -1, -1) if cl_s in path[idx] and fr_s not in path[idx]), "?")
    out[short(caller)] += sc
tot = sum(out.values()); print("total", tot)
for k, v in out.most_common(15): print(f"{v:7d} {100*v/max(1,tot):5.1f}%  {k}")

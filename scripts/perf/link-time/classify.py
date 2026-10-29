#!/usr/bin/env python3
# Usage: python3 classify.py <binary A> <binary B>   (unstripped builds, e.g. --config profile.dist.strip=false)
"""Compares two builds of one binary function by function (docs/research/2026-10-29-link-time.md): same size, alignment
padding only, the same instructions calling a folded twin under another name, folded away in one build, or different
instructions. Uses llvm-nm and llvm-objdump from the toolchain's llvm-tools; prints counts, then every differing function."""
import collections
import re
import subprocess
import sys

host = re.search(r"^host: (.+)$", subprocess.run(["rustc", "-vV"], capture_output=True, text=True).stdout, re.M).group(1)
sysroot = subprocess.run(["rustc", "--print", "sysroot"], capture_output=True, text=True).stdout.strip()
BIN = f"{sysroot}/lib/rustlib/{host}/bin"


def table(path):
    """Demangled function name -> list of (start, end) addresses."""
    out = subprocess.run([f"{BIN}/llvm-nm", "-n", "-C", "--defined-only", path], capture_output=True, text=True).stdout
    syms = []
    for line in out.split("\n"):
        q = line.split(" ", 2)
        if len(q) == 3 and q[1] in ("t", "T"):
            syms.append((int(q[0], 16), q[2]))
    r = collections.defaultdict(list)
    for (a, n), (b, _) in zip(syms, syms[1:]):
        r[n].append((a, b))
    return r


def dis(path, a, b, names=True):
    """Instructions of [a, b) with addresses (and, without names, symbol labels) normalised and trailing padding dropped."""
    out = subprocess.run([f"{BIN}/llvm-objdump", "-d", "-C", "--no-show-raw-insn", "--no-leading-addr",
                          f"--start-address={a:#x}", f"--stop-address={b:#x}", path], capture_output=True, text=True).stdout
    lines = [re.sub(r"0x[0-9a-f]+", "ADDR", l).strip() for l in out.split("\n")[4:]]
    if not names:
        lines = [re.sub(r"<[^>]*>", "<sym>", l) for l in lines]
    lines = [l for l in lines if l and l != "..."]
    while lines and (lines[-1] == "nop" or lines[-1].startswith("udf")):
        lines.pop()
    return lines


def main(pa, pb):
    A, B = table(pa), table(pb)
    count = collections.Counter()
    rows = []
    for n in A:
        if n not in B or len(A[n]) != 1 or len(B[n]) != 1:
            count["skipped (name missing or not unique)"] += 1
            continue
        (a0, a1), (b0, b1) = A[n][0], B[n][0]
        if a1 - a0 == b1 - b0:
            count["same size"] += 1
            continue
        da, db = dis(pa, a0, a1), dis(pb, b0, b1)
        if da == db:
            count["same instructions, alignment padding only"] += 1
        elif dis(pa, a0, a1, False) == dis(pb, b0, b1, False):
            count["same instructions, a call target named differently (folded twin)"] += 1
        elif not da or not db:
            count["folded away in one build"] += 1
            rows.append(("folded", len(da), len(db), n))
        else:
            count["instructions differ"] += 1
            rows.append(("differ", len(da), len(db), n))
    print(dict(count))
    for kind, x, y, n in sorted(rows, key=lambda t: (t[0], -abs(t[1] - t[2]))):
        print(kind, x, y, n[:220])


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])

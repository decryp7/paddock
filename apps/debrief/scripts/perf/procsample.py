"""Samples a process and its children on macOS through proc_pid_rusage(RUSAGE_INFO_V4): physical footprint (rule 26),
resident size, CPU time (user+system, ns), wake-ups and the lifetime peak footprint.

usage: python3 -I procsample.py ROOT_PID OUT_CSV [INTERVAL_S]
Runs until ROOT_PID exits. One CSV line per process per tick.
"""
import ctypes
import ctypes.util
import os
import sys
import time

lib = ctypes.CDLL(ctypes.util.find_library("proc") or "/usr/lib/libproc.dylib")
libc = ctypes.CDLL(None)


class Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


tb = Timebase()
libc.mach_timebase_info(ctypes.byref(tb))
TICK_NS = tb.numer / tb.denom

# rusage_info_v4: 16-byte uuid then u64 fields.
F = {"user": 0, "system": 1, "pkg_idle_wkups": 2, "interrupt_wkups": 3, "resident": 6, "footprint": 7,
     "lifetime_max_footprint": 28}


def usage(pid):
    buf = (ctypes.c_uint8 * (16 + 8 * 64))()
    if lib.proc_pid_rusage(pid, 4, ctypes.byref(buf)) != 0:
        return None
    v = (ctypes.c_uint64 * 64).from_buffer(buf, 16)
    return {k: v[i] for k, i in F.items()}


def children(pid):
    arr = (ctypes.c_int * 256)()
    n = lib.proc_listchildpids(pid, arr, ctypes.sizeof(arr))
    return [arr[i] for i in range(max(0, n))]


def name(pid):
    b = ctypes.create_string_buffer(256)
    lib.proc_name(pid, b, 256)
    return b.value.decode(errors="replace") or str(pid)


def main():
    root, out = int(sys.argv[1]), sys.argv[2]
    interval = float(sys.argv[3]) if len(sys.argv) > 3 else 0.25
    with open(out, "w") as f:
        f.write("t_ms,pid,name,cpu_ns,footprint,resident,idle_wkups,intr_wkups,max_footprint\n")
        while True:
            try:
                os.kill(root, 0)
            except OSError:
                break
            now = int(time.time() * 1000)
            for pid in [root] + children(root):
                u = usage(pid)
                if u is None:
                    continue
                cpu = int((u["user"] + u["system"]) * TICK_NS)
                f.write(f"{now},{pid},{name(pid)},{cpu},{u['footprint']},{u['resident']},{u['pkg_idle_wkups']},"
                        f"{u['interrupt_wkups']},{u['lifetime_max_footprint']}\n")
            f.flush()
            time.sleep(interval)


main()

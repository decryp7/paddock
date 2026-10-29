"""Indicative RF-DETR ONNX latency: one (model, EP) per process. Synthetic input only.

Research harness for docs/research/2026-10-29-rf-detr.md (rule 48), not product code. macOS only (reads the physical
footprint through libproc). Skips or aborts when the 1-minute load is above 25.
Usage: `nice -n 19 python -I bench.py <model.onnx> <cpu|coreml:ALL|coreml:CPUAndGPU> <threads> <warm-up> <runs>`.
"""
import ctypes, json, os, sys, time, statistics
import numpy as np, onnxruntime as ort

def footprint():
    lib = ctypes.CDLL("/usr/lib/libproc.dylib")
    buf = (ctypes.c_uint64 * 64)()
    if lib.proc_pid_rusage(os.getpid(), 2, ctypes.byref(buf)) != 0:
        return None
    return buf[9]  # 16-byte uuid = 2 words, then user, system, idle, intr, pageins, wired, resident, phys_footprint

def load1():
    return os.getloadavg()[0]

model, ep, threads, warm, runs = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
if load1() > 25:
    print(json.dumps({"skipped": "load", "load": load1()})); sys.exit(0)
so = ort.SessionOptions()
so.intra_op_num_threads = threads
so.inter_op_num_threads = 1
so.log_severity_level = 1
if ep == "cpu":
    providers = ["CPUExecutionProvider"]
else:
    providers = [("CoreMLExecutionProvider", {"ModelFormat": "MLProgram", "MLComputeUnits": ep.split(":")[1], "RequireStaticInputShapes": "1"}), "CPUExecutionProvider"]
fp0 = footprint()
t0 = time.perf_counter()
sess = ort.InferenceSession(model, so, providers=providers)
create_ms = (time.perf_counter() - t0) * 1000
inp = sess.get_inputs()[0]
shape = [d if isinstance(d, int) else 1 for d in inp.shape]
x = np.random.default_rng(42).random(shape, dtype=np.float32)
for _ in range(warm):
    sess.run(None, {inp.name: x})
ts = []
for i in range(runs):
    if i % 10 == 0 and load1() > 25:
        print(json.dumps({"aborted": "load", "load": load1(), "done": i})); sys.exit(0)
    t = time.perf_counter(); sess.run(None, {inp.name: x}); ts.append((time.perf_counter() - t) * 1000)
ts.sort()
print(json.dumps({"model": os.path.basename(os.path.dirname(model)), "ep": ep, "threads": threads, "input": shape,
    "outputs": [(o.name, o.shape) for o in sess.get_outputs()], "providers": sess.get_providers(),
    "create_ms": round(create_ms), "p50": round(statistics.median(ts), 1), "p95": round(ts[int(0.95 * (len(ts) - 1))], 1),
    "min": round(ts[0], 1), "runs": runs, "footprint_mb": round((footprint() or 0) / 2**20), "footprint_before_mb": round((fp0 or 0) / 2**20),
    "load_end": round(load1(), 1)}))

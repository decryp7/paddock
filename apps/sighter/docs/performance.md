# Performance record

Sighter's benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in
the family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.
Sighter has no code in the tree yet, so it has no benchmarks of its own; the one result below is an indicative
measurement of an upstream model for [ADR 0001](adr/0001-object-detector.md).

## Budgets (proposed, no test)

From the [design review](review/2026-10-design-review.md) §1.7 (2026-10-24):

| Measure | Budget |
|---|---|
| Frame decode | p95 ≤ 50 ms on loopback, ≤ 100 ms on a LAN |
| Locate | p50 ≤ 80 ms and p95 ≤ 250 ms on a GPU; p95 ≤ 600 ms on a CPU |
| Action | p95 ≤ 400 ms |
| HUD | ≤ 2 ms per frame, ≤ 5% |
| 8 targets | ≤ 2 × the locate p95 |
| Model cold load | ≤ 1.5 s |
| Server memory | ≤ 1.5 GB RSS with 3 models (rule 26: measure footprint, PSS or private working set) |
| Studio | 60 fps with 500 boxes; next item ≤ 50 ms |

The [proposal](proposal.md) §7 adds per-stage targets: cache < 5 ms, template < 30 ms, OCR < 100 ms for a region, and a
detector < 50 ms on a GPU at 640 px.

## Gaps

Every budget above needs a benchmark when the code lands, on synthetic captures. The
[detector research](../../../docs/research/2026-10-sighter-detector.md) quotes only third-party figures. Nothing was
measured on our data.

## Results (newest first)

### 2026-10-29, RF-DETR ONNX latency, indicative (docs/sighter-rfdetr)

- **Commit:** 912ad7c12 (origin/main at branch start; docs only, no Sighter code measured)
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2
- **Load:** 1-minute load 17.9–24.4 during the timed runs (other agents' CI builds and tests running); a guard skipped
  any run starting above 25 (Small 512 px on CoreML CPU+GPU was skipped)
- **Build:** Python 3.12 scratch venv; `rfdetr` 1.11.2 (export), `torch` 2.15.0, `onnx` 1.23.2, `onnxruntime` 1.31.1
  (the official PyPI wheel; `ort` rc.15's archives are 1.31.0; not the static `ort` build; **telemetry not disabled**:
  official builds have it on by default, see the research note section 12); models fp32, ONNX opset 17, batch 1; CPU EP with `intra_op_num_threads=4`,
  `inter_op_num_threads=1`; CoreML EP with `ModelFormat=MLProgram`, `RequireStaticInputShapes=1`, `MLComputeUnits` ALL or
  CPUAndGPU, no model cache
- **Corpus:** synthetic: one random float32 tensor `[1,3,H,W]` from `numpy.random.default_rng(42)`; models are the COCO
  checkpoints `rf-detr-nano.pth` and `rf-detr-small.pth` (MD5 checked by `rfdetr`), exported at native size (384, 512)
  and at 640
- **Command:** `RF_HOME=<scratch>/weights nice -n 19 python -I export.py <scratch>/models`, then per row
  `nice -n 19 python -I bench.py <model.onnx> <cpu|coreml:ALL|coreml:CPUAndGPU> 4 5 30` (one process per row; scripts in
  [research/scripts/](research/scripts/))
- **Runs:** single series per row: 5 warm-up and 30 timed `Session.run` calls; reported p50, p95 (`sorted[int(0.95 × (n − 1))]`, the 28th of 30 values; nearest rank would be the 29th) and
  session creation time
- **Numbers:**

  | Model, input | EP | p50 | p95 | Session creation | Footprint | Budget | Verdict |
  |---|---|---|---|---|---|---|---|
  | Nano 384 | CPU | 72.1 ms | 132.1 ms | 0.29 s | 181 MB | CPU locate p95 ≤ 600 ms | within, one tile |
  | Nano 384 | CoreML ALL | 23.4 ms | 30.5 ms | 43.6 s | 215 MB | GPU detector < 50 ms; cold load ≤ 1.5 s | latency within; cold load over |
  | Nano 384 | CoreML CPU+GPU | 17.2 ms | 19.0 ms | 22.7 s | 236 MB | same | latency within; cold load over |
  | Nano 640 | CPU | 142.4 ms | 170.3 ms | 0.18 s | 440 MB | CPU locate p95 ≤ 600 ms | within, one tile |
  | Nano 640 | CoreML ALL | 35.3 ms | 45.1 ms | 40.5 s | 278 MB | GPU < 50 ms; cold ≤ 1.5 s | latency within; cold load over |
  | Nano 640 | CoreML CPU+GPU | 29.8 ms | 31.6 ms | 25.5 s | 328 MB | same | latency within; cold load over |
  | Small 512 | CPU | 98.6 ms | 131.1 ms | 0.20 s | 305 MB | CPU locate p95 ≤ 600 ms | within, one tile |
  | Small 512 | CoreML ALL | 19.1 ms | 21.7 ms | 41.9 s | 276 MB | GPU < 50 ms; cold ≤ 1.5 s | latency within; cold load over |
  | Small 640 | CPU | 179.4 ms | 276.4 ms | 0.23 s | 413 MB | CPU locate p95 ≤ 600 ms | within for one tile; about six tiles per 1080p frame would not be |
  | Small 640 | CoreML ALL | 35.4 ms | 44.1 ms | 49.1 s | 305 MB | GPU < 50 ms; cold ≤ 1.5 s | latency within; cold load over |
  | Small 640 | CoreML CPU+GPU | 29.7 ms | 32.2 ms | 23.2 s | 366 MB | same | latency within; cold load over |

  CoreML EP partitions (ONNX Runtime log): 2, with 802 of 808 nodes (Nano) and 871 of 877 nodes (Small) on CoreML.
  File sizes: `.pth` 366 MB (Nano), 386 MB (Small); ONNX fp32 108 MB (Nano 384), 109 MB (Nano 640), 114 MB (Small 512),
  115 MB (Small 640).

- **Memory measure:** physical footprint (`proc_pid_rusage` `ri_phys_footprint`) of the benchmark process after the timed
  runs, including the Python interpreter (about 26 MB before the session)
- **Notes:** indicative only (shared machine under load, Python wheel instead of static `ort`, random input, 30 runs);
  not a budget check of Sighter, which has no code. Session creation on CoreML is compilation without
  `ModelCacheDirectory`; a cached second load was not measured. Windows (DirectML) and Medium/Large were not measured.
  CoreML outputs were not compared with the CPU EP's (fp16 risk). Raw JSON lines were kept in the scratchpad only. Quoted by
  [research note 2026-10-29-rf-detr](research/2026-10-29-rf-detr.md) section 5 and [ADR 0001](adr/0001-object-detector.md).

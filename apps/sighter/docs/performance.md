# Performance record

Sighter's benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in
the family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.
The pure core (`crates/sighter-core`, timed by `crates/sighter-bench`) has two benchmark programs in `crates/sighter-bench`, `bench_core` and `bench_hud` (clock and statistics from `paddock-bench`, memory from `paddock-proc`); no model is benchmarked yet. Both refuse to run above a 1-minute load of 25 and take the environment variables named in their header (`SIGHTER_BENCH_LOAD` is required).

## Budgets (proposed; the constants are in `sighter_core::bench::budgets`, read by benchmarks only)

Budgets are **benchmark constants only** ([ADR 0001](adr/0001-object-detector.md), "Locate at run time"; family ADR
0031): the `budgets` module of `sighter_core::bench` holds them, the benchmarks read them, and runtime code never does.
Each row names its source.

| Measure | Budget | Source |
|---|---|---|
| Frame arrival to decoded frame (RDP/VNC, 1080p) | p95 ≤ 50 ms on loopback, ≤ 100 ms on a LAN | design review §1.7 |
| Locate **gate**: one full-frame locate (normalise, tile, infer, merge, IPC; capture excluded), under the conditions of ADR 0001 "Gate conditions per execution provider" | p95 ≤ 500 ms on CoreML or DirectML and ≤ 2 s on the CPU at 1920 × 1200 (or 4K at 200 %); p95 ≤ 1 s GPU and ≤ 4 s CPU at 3840 × 2160 at 100 % (`LOCATE_GATE_*`) | ADR 0001 (user, 2026-10-29) |
| Locate **target** (not a gate): end to end (fingerprint, cache, OCR region, detector), 1080p | p50 ≤ 80 ms and p95 ≤ 250 ms on a GPU; p95 ≤ 600 ms on an 8-core CPU (`LOCATE_TARGET_*`) | design review §1.7, kept as the target by ADR 0001 |
| Action end to end (locate, re-check, inject, first visible effect), excluding the app's own latency | p95 ≤ 400 ms | design review §1.7 |
| HUD overhead on the server | ≤ 2 ms per frame; trace on or off changes step timing by ≤ 5 % | design review §1.7 |
| Parallel targets (14 cores, one 8 GB GPU) | 8 targets at 1080p with batched inference; no target's locate p95 above 2 × its single-target value | design review §1.7 |
| Model cold load | ≤ 1.5 s (with the CoreML cache present); first locate after load ≤ 2 × warm | design review §1.7; ADR 0001 |
| Server memory with 3 models loaded | ≤ 1.5 GB physical footprint (macOS), PSS (Linux), private working set (Windows): rule 26, never RSS | design review §1.7 (it says "RSS"; rule 26 replaces that) |
| Studio | 60 fps pan and zoom with 500 boxes; review-queue keystroke to next item ≤ 50 ms | design review §1.7 |

The [proposal](proposal.md) §7 ("Performance targets") adds per-stage **p50 goals** for a 1080p screenshot in memory, to
confirm in the spike: cache under 5 ms; template under 30 ms; OCR anchor under 100 ms with a region (150 to 500 ms full
screen on CPU); detector under 50 ms on a discrete or Apple GPU at 640 px, **including** 10 to 25 ms of capture and
pre-processing, and under 250 ms at 640 on an 8-core CPU for a small model. These are per-tile and per-stage goals, not
locate budgets.

## Gaps

Every budget above needs a benchmark when the code lands, on synthetic captures. The locate gate is measured by
`sighter-bench locate` under ADR 0001's "Gate conditions per execution provider", which emits its results block for
this file; **DirectML has no measurement at all yet** (ADR 0001 spike item S2). The Only the HUD budgets are measured so far (below). The
[detector research](../../../docs/research/2026-10-sighter-detector.md) quotes only third-party figures. Nothing was
measured on our data.

## Results (newest first)

### 2026-10-29, HUD overhead (trace bus, overlay items, sidecar text) (sighter/core-wave-0)

- **Commit:** 9e2804845
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2
- **Load:** 1-minute load 18.54 at start (above the family guide's quiet threshold of about 12, with other agents' builds running: indication only)
- **Build:** release; no model, no UI rendering
- **Corpus:** synthetic: fixed-seed boxes (50 per frame, 300 per step) and 12 trace events per step
- **Command:** `SIGHTER_BENCH_LOAD=<load> nice -n 10 cargo run --release -p sighter-bench --bin bench_hud`
- **Runs:** frame: 50 warm-up and 400 timed runs, nearest-rank p95; step: 6 interleaved A/B pairs of 100 timed runs after 20 warm-up, median (paddock-bench `summarize`)
- **Numbers:**

  | Measure | Value | Budget | Verdict |
  |---|---|---|---|
  | HUD work per frame, 50 detections, 2 viewers, p95 | 27.29 us | HUD per frame <= 2.00 ms | within (indication) |
  | Step median, trace off | 25.04 us | none | indication |
  | Step median, trace on | 25.62 us (+2.3 %) | change <= 5 % | within (indication) |

- **Memory measure:** physical footprint at the end of the run 4.2 MB (paddock-proc::process_memory)
- **Notes:** Pure-core cost only: drawing, the video sidecar write and the UI are not measured. Indicative unless the load is below about 12. Wall time 59.54 ms.

### 2026-10-29, sighter-core geometry, NMS and tile planning (sighter/core-wave-0)

- **Commit:** 9e2804845
- **Machine:** Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2
- **Load:** 1-minute load 18.54 at start (above the family guide's quiet threshold of about 12, with other agents' builds running: indication only)
- **Build:** release; no model, no allocator override
- **Corpus:** synthetic: 500 boxes from a fixed-seed generator in the program; a 3840 x 2160 grid
- **Command:** `SIGHTER_BENCH_LOAD=<load> nice -n 10 cargo run --release -p sighter-bench --bin bench_core`
- **Runs:** single series, 20 warm-up and 200 timed runs; median and nearest-rank p95 (paddock-bench `summarize`)
- **Numbers:**

  | Measure | Value | Budget | Verdict |
  |---|---|---|---|
  | NMS, 500 boxes, median | 62.54 us | none | indication |
  | NMS, 500 boxes, p95 | 69.75 us | none | indication |
  | Tile grid, 3840 x 2160, median | 84 ns | none | indication |
  | Chain map, 500 boxes, median | 292 ns | none | indication |

- **Memory measure:** physical footprint at the end of the run 3.9 MB (paddock-proc::process_memory)
- **Notes:** wall time 23.58 ms; no budget applies to these rows; indicative unless the load is below about 12

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

  | Model, input | EP | p50 | p95 | Session creation | Footprint | Compared with (per tile) | Verdict |
  |---|---|---|---|---|---|---|---|
  | Nano 384 | CPU | 72.1 ms | 132.1 ms | 0.29 s | 181 MB | proposal §7 CPU detector < 250 ms (p50) | within |
  | Nano 384 | CoreML ALL | 23.4 ms | 30.5 ms | 43.6 s | 215 MB | proposal §7 GPU detector < 50 ms (p50); cold load ≤ 1.5 s | latency within; cold load over |
  | Nano 384 | CoreML CPU+GPU | 17.2 ms | 19.0 ms | 22.7 s | 236 MB | same | latency within; cold load over |
  | Nano 640 | CPU | 142.4 ms | 170.3 ms | 0.18 s | 440 MB | proposal §7 CPU detector < 250 ms (p50) | within |
  | Nano 640 | CoreML ALL | 35.3 ms | 45.1 ms | 40.5 s | 278 MB | proposal §7 GPU < 50 ms (p50); cold load ≤ 1.5 s | latency within; cold load over |
  | Nano 640 | CoreML CPU+GPU | 29.8 ms | 31.6 ms | 25.5 s | 328 MB | same | latency within; cold load over |
  | Small 512 | CPU | 98.6 ms | 131.1 ms | 0.20 s | 305 MB | proposal §7 CPU detector < 250 ms (p50) | within |
  | Small 512 | CoreML ALL | 19.1 ms | 21.7 ms | 41.9 s | 276 MB | proposal §7 GPU < 50 ms (p50); cold load ≤ 1.5 s | latency within; cold load over |
  | Small 640 | CPU | 179.4 ms | 276.4 ms | 0.23 s | 413 MB | proposal §7 CPU detector < 250 ms (p50) | within for one tile; a full frame of 8 tiles is not (see ADR 0001 projections) |
  | Small 640 | CoreML ALL | 35.4 ms | 44.1 ms | 49.1 s | 305 MB | proposal §7 GPU < 50 ms (p50); cold load ≤ 1.5 s | latency within; cold load over |
  | Small 640 | CoreML CPU+GPU | 29.7 ms | 32.2 ms | 23.2 s | 366 MB | same | latency within; cold load over |

  CoreML EP partitions (ONNX Runtime log): 2, with 802 of 808 nodes (Nano) and 871 of 877 nodes (Small) on CoreML.
  File sizes: `.pth` 366 MB (Nano), 386 MB (Small); ONNX fp32 108 MB (Nano 384), 109 MB (Nano 640), 114 MB (Small 512),
  115 MB (Small 640).

- **Memory measure:** physical footprint (`proc_pid_rusage` `ri_phys_footprint`) of the benchmark process after the timed
  runs, including the Python interpreter (about 26 MB before the session)
- **Note on the comparison column (revision of 2026-10-29):** these are one-tile, inference-only times, so they are
  compared with the proposal's per-tile detector goals, not with the locate gate or target (which are full-frame and
  end to end); the earlier labels "CPU locate p95 ≤ 600 ms" mixed the two. The GPU goal includes 10 to 25 ms of capture
  and pre-processing that these runs did not time, so "within" there means inference alone fits.
- **Notes:** indicative only (shared machine under load, Python wheel instead of static `ort`, random input, 30 runs);
  not a budget check of Sighter, which has no code. Session creation on CoreML is compilation without
  `ModelCacheDirectory`; a cached second load was not measured. Windows (DirectML) and Medium/Large were not measured.
  CoreML outputs were not compared with the CPU EP's (fp16 risk). Raw JSON lines were kept in the scratchpad only. Quoted by
  [research note 2026-10-29-rf-detr](research/2026-10-29-rf-detr.md) section 5 and [ADR 0001](adr/0001-object-detector.md).

# 1. Sighter's object detector: RF-DETR (Apache sizes) on ONNX Runtime in Sighter's own sidecar

Status: proposed (2026-10-29). The user decided every question on 2026-10-29 (marked "user, 2026-10-29" below); none
is open. Pending: the RT-DETRv4 licence review (plan item P4) and the DirectML egress gate of the spike (decision 4).
The ADR goes to an independent review before it is accepted.

Related: [proposal](../proposal.md) sections 6, 8, 9.1, 11, 12 and 15; [design review](../review/2026-10-design-review.md);
research notes [2026-10-29-rf-detr](../research/2026-10-29-rf-detr.md) (this ADR's evidence) and the family note
[2026-10-sighter-detector](../../../../docs/research/2026-10-sighter-detector.md); Sighter AGENTS.md ("Licences of
models", "Pitwall is always optional", "Local only: no telemetry"); family rules 10 to 15, 23, 26, 42, 46 and 48;
ADR 0024 (background work); Pacenotes ADR 0001 (static ONNX Runtime and rule 23).

## Context

Sighter locates UI elements by sight through a layered resolver (proposal section 8): cache, template, OCR anchor,
**trained detector**, optional Pitwall VLM grounding, then "needs labelling". The trained detector is a per-app,
closed-set model trained on the user's own labelled frames (1k to 5k per app) and queried per frame. It has to:

- carry permissive licences for code **and** weights (no AGPL, GPL or non-commercial: Sighter AGENTS.md), including
  the data its base weights were pre-trained on, as far as that is stated;
- run from Rust on macOS (Apple Silicon only since 2026-10-27) and Windows, with a GPU where there is one and a usable CPU
  path where there is not (CI runners);
- meet the locate budgets in two tiers ("Latency budgets: gate and target" below: a gate sized for test steps, and the
  older faster numbers as a target); model cold load ≤ 1.5 s; server ≤ 1.5 GB with three models;
- support displays of 1920 × 1200 and larger, up to 4K and 5K, with HiDPI and Retina scaling (user, 2026-10-29);
- keep rule 23 (one Team ID in the bundle, sidecars self-test from the built app);
- work without Pitwall and without any network, send no telemetry, run deterministically on the CPU, and say which
  execution provider answered.

The proposal named RT-DETR, D-FINE/DEIM, DEIMv2 and RF-DETR as candidates; the backlog of 2026-10-27 asked "RF-DETR or
UI-DETR-1?". The research note answers with today's official sources and one indicative measurement.

## Options considered

**Architecture**

| Option | Licence (code / weights) | COCO AP50:95, publishers' T4 latency | For | Against |
|---|---|---|---|---|
| **A. RF-DETR N/S/M/L** | Apache-2.0 / Apache-2.0 (Objects365 pre-training accepted, decision 1) | 48.4–56.5, 2.3–6.8 ms | Best published fine-tuning transfer (RF100-VL 57.7–62.2); one architecture, size is a resolution knob; plain ONNX, no NMS; training package does COCO datasets, EMA, eval and export; measured in ONNX Runtime here | ViT backbone, heavier on CPU than a CNN; five sizes of the family are PML 1.0 and must be fenced off; fast-moving upstream |
| B. D-FINE, COCO-only checkpoints (with the DEIM recipe) | Apache-2.0 / Apache-2.0 | 42.8–54.0 (N–L), 2.1–8.1 ms | CNN backbone, likely cheaper on CPU; same ONNX adapter; no Objects365 lineage | Weaker transfer evidence on non-COCO domains; training code is research-grade scripts; the stronger `obj365`/`obj2coco` checkpoints carry the Objects365 caveat |
| C. RT-DETRv4 | Apache-2.0 code / **no stated licence** for the checkpoints | 49.8–57.0, 3.7–12.9 ms | Strong numbers, CNN at inference | Trained by distillation from DINOv3 (custom, non-OSI licence); checkpoints on Google Drive with no licence: pending review |
| D. DEIMv2 | non-commercial | 50.9–57.8 | | **Excluded** by licence |
| E. Ultralytics YOLO, YOLOv9/v10, YOLO-NAS, OmniParser `icon_detect` | AGPL / GPL / restricted | | | **Excluded** by licence |
| F. Open-vocabulary detector or VLM at run time | various | | No training | Seconds per query, non-deterministic, no stable taxonomy; belongs in Pitwall for bounded requests |

**Runtime**

| Option | Licence | For | Against |
|---|---|---|---|
| `ort` 2.0 with pyke's prebuilt ONNX Runtime 1.31.0, static | MIT OR Apache-2.0 / MIT | Prebuilt static libraries with CoreML (macOS arm64) and DirectML (Windows x64/arm64), hash-pinned; no build in CI | Telemetry code inside (TraceLogging on Windows; 1DS is ONNX Runtime 1.31's default), switched off only at run time; CDN download at build time |
| **`ort` 2.0 with ONNX Runtime 1.31 self-built with `--no_telemetry`, static** (chosen) | MIT OR Apache-2.0 / MIT | No telemetry code at all; same static linking (rule 23); version and flags pinned by us; no third-party CDN | A long C++ build per target in CI, cached (plan item P5); we own the build's upkeep |
| `tract` | MIT OR Apache-2.0 | Pure Rust, no C++, no telemetry | CPU only; full RF-DETR graph not yet loaded |
| `candle` / `burn` | MIT OR Apache-2.0 | Pure Rust with Metal/CUDA | No RF-DETR decoder; a port is real work |

**Where it runs**

| Option | For | Against |
|---|---|---|
| **Sighter's own `sighter-infer` sidecar** behind the `Detector` port | Works without Pitwall; no HTTP hop per tile; Sighter owns model, EP and determinism record; a runtime crash ends one request | Sighter carries ONNX Runtime (it needs it for OCR anyway) |
| Pitwall | One AI host for the family | Makes locating depend on Pitwall (against AGENTS.md); adds ONNX Runtime to Pitwall for one client; a C++ fault takes down the family's AI host; Pitwall is the LLM/VLM layer (rule 46 keeps it to bounded requests) |

## Decision

1. **Architecture: RF-DETR, Apache-2.0 sizes only** (user, 2026-10-29). Presets: **Balanced = Small (default)**,
   **Fast = Nano** (CPU-only runners), **Accurate = Medium or Large**. Input sides are multiples of 32 (patch 16 × 2
   windows); frames map to tiles as in "Screen resolutions and HiDPI" below. Atto, Femto, Pico, XL and 2XL are refused:
   the model catalogue lists only the four Apache checkpoints, and model import refuses an ONNX whose metadata or card
   names a PML size. **Objects365 lineage** (user, 2026-10-29): RF-DETR's detection heads were pre-trained on
   Objects365, pseudo-labelled with SAM2, before COCO (paper, appendix A and section 3); Objects365 states no licence;
   Sighter **accepts the N/S/M/L checkpoints on Roboflow's Apache-2.0 label**, because Sighter is for personal,
   non-commercial use. Every model card records the lineage: "DINOv2 (Apache-2.0) → Objects365 pre-training (no
   licence stated) → COCO (Roboflow checkpoint, Apache-2.0) → the user's frames". If Sighter is ever used commercially,
   this acceptance must be revisited first.
2. **Fallback: D-FINE-S/M, COCO-only checkpoints** (user, 2026-10-29; COCO-only per the review), with the DEIM recipe,
   through the same "ONNX DETR, fixed input, no NMS" adapter, used when the spike shows RF-DETR missing the CPU budget on
   the CI runner class or failing on an EP. The `*_obj365` and `*_obj2coco` checkpoints are not used: D-FINE's README
   says they "may be subject to the Objects365 dataset terms". **RT-DETRv4 is pending** (user, 2026-10-29) until the
   licence review in the research note section 9 is done; until then it is not in the catalogue.
3. **Training: the `rfdetr` package** (Apache-2.0), version-pinned, inside the managed Python training runtime (user
   decision of 2026-10-24, recorded in proposal section 6 and the family note; this ADR closes proposal section 15
   question 1). The runtime is itself a `paddock-runtime` entry (pinned, hashed, self-tested, quarantined on failure).
   The training worker runs with no network: `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, and the sandbox denies
   outbound connections (`rfdetr`'s DINOv2 backbone calls `from_pretrained`, which must find what it needs locally or
   fail with a clear message). Start weights: the Apache COCO checkpoints only. **UI-DETR-1 is not in v1** (user,
   2026-10-29): neither as a labeller nor as start weights, because the licences of its training datasets are not
   stated.
4. **Inference: `ort` with statically linked ONNX Runtime**, in Sighter's `sighter-infer` sidecar (started through
   `paddock-ipc`, supervised by `paddock-proc` with a memory cap measured as footprint per rule 26, `--self-test` per
   rule 23), shared with OCR.
   - **GPU by default** (user, 2026-10-29): CoreML with `MLProgram` on macOS; DirectML on Windows, on Intel integrated
     or NVIDIA discrete GPUs, through our `DirectML.dll` or, if the gate below removes it, the system's. The CPU EP is
     the fallback, the reference and the determinism baseline. `error_on_failure` is set for the GPU EP so a failed
     registration is seen, never a silent CPU run.
   - **Start-up check**: a quick GPU self-test, run when the sidecar first loads a model and again after a model,
     driver or OS change: the EP loads, and **the user's own model**, run on its **stored reference frame**, gives boxes
     that match the **CPU answer saved in `model.json` when the model was created**, within tolerance. (The tiny bundled
     model is only the sidecar's rule-23 `--self-test`.) If either check fails (for example an old driver, the DirectML
     gate failing, or, a minor case, Sighter itself started inside a VM or a Remote Desktop session without a GPU),
     Sighter **falls back automatically to the CPU with the same model** (no per-app Nano model exists unless the user
     trains one) and tells the user why in plain words ("The graphics card could not run the detector (driver too old),
     so Sighter uses the processor. Locating is slower."), with an optional action "Train a Fast model for this
     computer" (a Nano model of the same app). Settings has an "Inference device" choice: Automatic (default), GPU, CPU.
   - **Device on every result**: each locate response, pre-labelling job and evaluation records the device used (EP,
     GPU name, fallback reason if any) with the ONNX Runtime version and model hash, for diagnostics.
   - **Telemetry off, by construction** (user, 2026-10-29; Sighter AGENTS.md: no telemetry). ONNX Runtime 1.31.0 made
     1DS telemetry the default for its native builds, and pyke's Windows library carries the TraceLogging provider
     (research note section 12). So Sighter does **not** use pyke's prebuilt libraries: it **builds ONNX Runtime from
     source per platform with `--no_telemetry`** (macOS arm64 with CoreML; Windows x64 and arm64 with DirectML), static,
     at a pinned release tag, and links it through `ort`'s custom-library mode (`ORT_LIB_PATH` pointing at the built
     static libraries, `download-binaries` off, `load-dynamic` and `copy-dylibs` off). Build pipeline: plan item P5.
   - **Second line of defence**: the sidecar still sets `ORT_DISABLE_TELEMETRY=1` before ONNX Runtime initialises and
     calls `DisableTelemetryEvents` after creating the environment, and a CI **egress test** runs
     `sighter-infer --self-test` with outbound connections recorded and fails on any connection attempt. A symbol check
     on the built library (no 1DS SDK, no curl, no TraceLogging provider) runs in the build job.
   - **CoreML cache**: the compiled-model cache is keyed by model hash and cleaned with the model registry. When it is
     missing (new model, cleared cache, new OS), locate **answers on the CPU EP** and starts the "prepare for this Mac"
     job; it never stalls a locate for the 23–49 s compile (indicative, at load 18–24).
   - **Windows DirectML**: a DirectML build of ONNX Runtime imports `DirectML.dll` (pyke's does, with `D3D12`, `DXGI`
     and `DXCORE`), so `sighter-infer` ships Microsoft's DirectML redistributable (`DirectML.dll` 1.15.4 from the NuGet package
     `Microsoft.AI.DirectML`, 18.5 MB, SHA-256-pinned) next to the sidecar (user, 2026-10-29). Its licence is
     Microsoft's proprietary DirectML terms (distribution allowed only inside an application, never stand-alone; may
     collect data; research note section 11). **The user approved it on 2026-10-29 as a named licence exception for that
     one file**, on one condition, the DirectML gate (spike, plan item P6): (1) the egress test shows no outbound
     connection while the DirectML EP runs, and
     (2) **`DirectML.dll` registers no TraceLogging/ETW provider, or the provider is shown to be inert**: search the DLL
     for TraceLogging provider metadata and provider GUIDs, and run an ETW session (`logman` or `tracelog`) that lists
     the providers registered and the events written while the EP runs. What the egress test can and cannot show:
     it sees connections that `sighter-infer` itself opens; it **cannot** see data a DLL hands to ETW/TraceLogging,
     because that leaves the machine, if at all, through Windows' own diagnostics pipeline (for ONNX Runtime's own
     TraceLogging events its privacy notice says "based on user consent"), not through our process. Hence check (2).
     If the gate fails, Sighter does not ship it and uses only the system's
     `DirectML.dll` (System32, Windows 10 1903 and later) or the CPU EP; that needs the DirectML import to be delay-loaded
     (or a CPU-only build variant), so that a missing or too old system copy degrades to the CPU instead of failing the
     start. Our own DirectML build of ONNX Runtime imports it like pyke's does (verify
     whether the import is delay-loaded): if it is not, a missing or broken `DirectML.dll` would stop even a CPU-only
     start, so the spike checks this and the self-test runs CPU first.
5. **Not in Pitwall.** Pitwall stays optional and is the VLM layer for labelling help, naming and grounding fallback, each
   a bounded, user-started request (rule 46). **Labelling** (user, 2026-10-29): whole recordings are pre-labelled by the
   user's own trained detector for that app. Pitwall's VLM is used only on a small selection the user chooses, or after a
   confirmation that states the frame count (and the expected time). Before the first model of an app exists,
   pre-labelling comes from OCR, templates and the review queue only (decision 9, "100 % computer vision"), not from
   the VLM per frame. This amends proposal 9.1's "label suggestions" request kind; the user approved the
   amendment on 2026-10-29 and it is applied in the proposal.
6. **Model distribution.** The runtime detector is always made locally and stored immutable and content-hashed in the
   project registry with its `model.json` (classes, input size, normalisation, thresholds, dataset hash, metrics, parent,
   base checkpoint file name, its licence and pre-training lineage, and the `rfdetr` version). The only downloaded models
   are the four Apache COCO checkpoints, **mirrored on our Gitea (dev.decryptology.net)** with the Apache-2.0 licence and
   notice, and pinned to **our own SHA-256s** (user, 2026-10-29); upstream's MD5s are checked once, when the mirror is
   made. Share first (rule 42): this is the family's third single-file model download (after Pacenotes' Model2Vec
   `potion-code` and, most likely, Pitwall's model downloads), so the catalogue is a **shared single-file asset kind in `paddock-runtime`** (or
   a shared model catalogue next to it): one TOML entry per file with URL, size, SHA-256, licence and upstream URL, parsed
   once, anything wrong refusing the whole catalogue. Downloads go through **`paddock-net` `Egress`** (the family's one
   outbound door, host allow-list = our Gitea) and `paddock-download` (hash while writing, rule 15), as a job. Nothing
   but a tiny self-test model ships in the installer.
7. **Jobs.** Download, pre-labelling, evaluation, CoreML preparation and training are `paddock-jobs` jobs with
   phase-labelled progress, Cancel and background priority (ER-17 to ER-20, ADR 0024). Locate is a bounded request.
8. **Screens, tiles and coordinates** as in the next section: frames are normalised to logical (100 %) scale per
   display, tiled at 640 with overlap, unchanged tiles reuse cached detections, the active window goes first, and boxes
   come back in physical pixels with their display and are converted to points at the input edge.
9. **100 % computer vision** (user, 2026-10-29: "Sighter will be 100% computer vision"). Sighter uses only pixels, for
   local windows and remote ones alike: no accessibility tree, no UI Automation or AX APIs, no AT-SPI, no DOM, no app
   hooks or injected code. Pre-labelling before an app's first model comes from OCR, templates and the review queue
   only. **Allowed:** window bounds, titles and the window list from the OS to pick and follow the capture target, and
   the pointer position: that is capture plumbing, not UI understanding, and it never names, classifies or locates an
   element. The cost and its mitigation are in Consequences.
10. **Main training path: video → auto-label → review → train** (user, 2026-10-29), detailed in "Training from
    test-run videos" below and in proposal section 6.1. Boxes always come from computer vision; Pitwall's VLM only
    names, describes and suggests a class for regions that already have boxes.
11. **Spike before the first release**, per the research note's checklist; its numbers go into
   [performance.md](../performance.md) as results blocks (rule 48) and can move the default preset or switch the CPU
   preset to the fallback without changing this ADR.

## Screen resolutions and HiDPI

Sighter supports 1920 × 1200 (WUXGA) and larger: 2560 × 1440, 3840 × 2160 (4K) and 5120 × 2880 (5K), with HiDPI and
Retina scaling where the logical size (points, or DIPs on Windows) differs from the physical pixels (user, 2026-10-29).

### How frames map to detector input

Tiles are square, 640 or 960 px, with a **64 px overlap**. With an overlap of `o`, only an element **at most `o` px
wide (and high) is guaranteed to lie whole inside some tile**; a wider element that straddles a seam is cut into parts
in neighbouring tiles and relies on the **seam merge** (a pure function in `sighter-core` that joins boxes of the same
class that touch or overlap across the seam). Should the overlap match the largest class width? No: wide classes
(text boxes, tabs, toolbars, often 200–600 px) would need huge overlaps and many more tiles, and a wide element is
still visible in both parts; the overlap is set to cover the **small classes** whose parts would be too small to
detect (icons, checkboxes, radio buttons, small buttons: about 16–64 logical px), and the model card records the
overlap and the largest small-class size measured in the training set. Recall at tile seams is a spike item. Tiles per
frame = ⌈(W − 64) / (T − 64)⌉ × ⌈(H − 64) / (T − 64)⌉.

| Frame (pixels fed to the detector) | 640 tiles | 960 tiles | Downscale to 640 instead: factor |
|---|---|---|---|
| 1920 × 1200 | 4 × 2 = 8 | 3 × 2 = 6 | 3.0 |
| 1920 × 1220 | 4 × 3 = 12 (8 with an overlap ≤ 40 px) | 3 × 2 = 6 | 3.0 |
| 2560 × 1440 | 5 × 3 = 15 | 3 × 2 = 6 | 4.0 |
| 3840 × 2160 | 7 × 4 = 28 | 5 × 3 = 15 | 6.0 |
| 5120 × 2880 | 9 × 5 = 45 | 6 × 4 = 24 | 8.0 |
| 4K at 200 % (logical 1920 × 1080) | 4 × 2 = 8 | 3 × 2 = 6 | 3.0 |
| 5K Retina at 200 % (logical 2560 × 1440) | 5 × 3 = 15 | 3 × 2 = 6 | 4.0 |

### Smallest detectable element, per approach

Design assumption, to be measured in the spike: RF-DETR N–L cut the input into 16 × 16 px patches, so an element needs
about **16 px at the model's input** to be found reliably (COCO calls objects under 32 × 32 px "small").

| Approach | Smallest element, in physical pixels | Cost | Verdict |
|---|---|---|---|
| Downscale the whole frame to one 640 input | 16 × factor: 48 (1920), 64 (2560), 96 (4K), 128 (5K) | 1 run | **No**: a 16 pt checkbox at 200 % is 32 physical px and disappears at 4K/5K |
| Tile at physical 1:1 | 16 at every size | 8 to 45 tiles (table) | Most detail, most cost; elements look 2× larger at 200 % than in 100 % training data (covered by the 50–300 % scale augmentation, proposal section 6) |
| **Normalise to logical 100 % per display, then tile** (default, design review P1) | 16 logical px (16 pt), which is 32 physical px at 200 % | the logical size's tile count: 8 for 4K at 200 %, 15 for 5K at 200 % | **Default**: model sees the scale it was trained at; a 4K or 5K Retina display costs what its logical size costs |
| Logical tiles plus a **1:1 fine pass** on chosen regions | 16 physical px in those regions | + tiles of the region only | For classes the model card marks as small (icons, checkboxes, grid cells) when recall below 16 pt is too low |

### Latency per resolution (projections)

Projection = tile count × the measured per-tile time (RF-DETR Small at 640, batch 1, run one after another;
[performance.md](../performance.md#2026-10-29-rf-detr-onnx-latency-indicative-docssighter-rfdetr), at load 18–24,
indicative). **These are projections, not measurements.** Batching tiles on a GPU and using cores − 1 CPU threads
(the run used 4) would both lower them; neither was measured.

### Latency budgets: gate and target

User, 2026-10-29: Sighter is for UI test automation. A test step waits for the UI to settle anyway (hundreds of
milliseconds to seconds), so a single locate matters less than **accuracy and reliability**; faster is still better.
The locate budgets therefore come in two tiers. Their numbers are **named constants in one place** (a `budgets` module of
`sighter-core` read by the benchmark harness and the spike, mirrored in [performance.md](../performance.md)), so changing
a number is a one-line change.

| Tier | Constant (proposed name) | 1920 × 1200 (or 4K at 200 %) | 3840 × 2160 at 100 % | Role |
|---|---|---|---|---|
| **Gate** (must pass) | `LOCATE_GATE_GPU_P95`, `LOCATE_GATE_CPU_P95` | full-frame locate p95 ≤ **500 ms** on the GPU (CoreML or DirectML), ≤ **2 s** on the CPU | p95 ≤ **1 s** GPU, ≤ **4 s** CPU | Release gate, sized for test steps that take seconds |
| **Target** (not a gate) | `LOCATE_TARGET_GPU_P50`, `LOCATE_TARGET_GPU_P95`, `LOCATE_TARGET_CPU_P95` | p50 ≤ **80 ms** and p95 ≤ **250 ms** on the GPU; p95 ≤ **600 ms** on the CPU | same | For live and watch modes and fast suites; reached through changed tiles, priority order with early stop, or batching |

| Frame (pixels fed to the detector) | 640 tiles | CPU EP, p50 (179 ms/tile) | CPU EP, p95 sum (276 ms/tile) | CoreML CPU+GPU, p50 / p95 sum (30 / 32 ms per tile) | **Gate** (full frame, p95) | **Target** (p50 ≤ 80 ms, p95 ≤ 250 ms GPU; p95 ≤ 600 ms CPU) |
|---|---|---|---|---|---|---|
| 1920 × 1200, or 4K at 200 % | 8 | ~1.4 s | ~2.2 s | ~0.24 s / ~0.26 s | GPU ≤ 500 ms: **projected to pass**; CPU ≤ 2 s: **projected over** (~2.2 s at 4 threads; cores − 1 threads may fit) | **over** on both as a whole frame |
| 2560 × 1440, or 5K at 200 % | 15 | ~2.7 s | ~4.1 s | ~0.45 s / ~0.48 s | no gate set (between the two sizes) | **over** |
| 3840 × 2160 at 100 % | 28 | ~5.0 s | ~7.7 s | ~0.83 s / ~0.90 s | GPU ≤ 1 s: **projected to pass** (tight); CPU ≤ 4 s: **projected over** | **over** |
| 5120 × 2880 at 100 % | 45 | ~8.1 s | ~12.4 s | ~1.3 s / ~1.4 s | no gate set | **over** |

All verdicts are **projections, to be measured** (spike item 8). The gate is projected to pass on the GPU at both gate
sizes and to be missed on the CPU at 4 threads; the CPU gate then depends on cores − 1 threads, the window scope or the
Fast model. The target is missed by every whole frame and needs the measures below; none of them is measured yet.
(Proposal section 7's "detector < 50 ms on a GPU at 640 px" is a per-tile figure; measured: ~30 ms.)

Nano at 640 is ~142 ms per tile on the CPU (about 20 % less), so it does not change the picture for whole frames.

What the design does to reach the target, or the gate on a slow CPU, in this order:

1. **Scope**: a locate is scoped to the target window by default (tests target an app), so the usual cost is the
   window's tiles, not the screen's: a 1200 × 800 logical window is 2 × 2 = 4 tiles (~0.7 s CPU, ~0.12 s GPU, projected;
   inside the gate, still over the target on its own).
2. **Changed tiles only**: tiles whose pixels did not change since the last frame reuse their cached detections.
3. **Priority order**: the focused element's and the pointer's tiles run first; a unique hit above the class threshold
   in the requested scope ends the locate early.
4. **Batching** tiles in one run on a GPU EP (fixed input, no NMS: DETR batches well); on the CPU, cores − 1 threads.
5. **A Fast (Nano) model**, if the user trained one for that app, on CPU-only runners, and **"Still working"** rather than a timeout for a full-screen pre-labelling job
   (rule 12); an unattended locate that cannot finish inside its budget reports the scope it covered and fails as "not
   found in time", never a guess (AGENTS.md: never click blindly).

### Would segmenting the screen make it faster?

The user's question: "if we segment the screen into parts and analyse them, would it be faster?" Only if fewer pixels
reach the detector. Splitting the same pixels into parts does not save work. Costs are relative to the whole frame at
logical 100 % scale (1920 × 1200: 8 tiles, ~1.4 s CPU p50, ~0.24 s CoreML, projected from the measured per-tile times).

| Approach | Cost vs whole frame | What it can miss | Platform API |
|---|---|---|---|
| 1. Tile the whole frame | **1.0×** of compute (the tiles cover more pixels than the frame, from the overlap and the edge tiles: +42 % at 1920 × 1200, +67 % at 2560 × 1440, +38 % at 3840 × 2160, +25 % at 5120 × 2880 with 640 tiles); faster wall-clock only by running tiles in parallel on spare cores or batching them on a GPU (not measured) | Nothing, at the tile scale | none |
| 2. Only segments that changed since the last frame | **≈ changed tiles / all tiles**: 0 for a static screen; 1–3 of 8 (0.13–0.38×, ~0.2–0.5 s CPU) for a typical click that opens a menu or changes a few controls (projection) | Nothing if the change detection is right; misses a change the OS does not report (some GPU-drawn or remote-session surfaces) unless a per-tile pixel hash is the fallback; cache must be cleared on model change, theme or DPI change | macOS: `SCStreamFrameInfo.dirtyRects` (ScreenCaptureKit, macOS 12.3+): "a union of the rectangles redrawn and moved". Windows: Desktop Duplication `IDXGIOutputDuplication::GetFrameDirtyRects` and `GetFrameMoveRects` (Windows 8+; process move rects before dirty rects); otherwise a per-tile hash of the new frame (cheap, not measured) |
| 3. Active window, or the region around the pointer or focus, first | Window 1200 × 800: 4 tiles, **0.5×**; pointer region: 1 tile, **0.13×** (~0.18 s CPU, ~30 ms GPU) | Elements outside the window: menus and pop-ups that open outside it, toasts, other windows of a multi-window app | Window bounds and focus from the capture or window APIs (`SCWindow` frames; `GetWindowRect`/DWM extended frame bounds on Windows; verify in the spike); pointer position |
| 4. Cheap first pass (OCR) picks segments, detector runs on those only | OCR pass + detector on k tiles; OCR is not cheap on a whole frame (proposal: 150–500 ms CPU for 1080p, unverified) | Icon-only and custom-drawn controls with no text, which is exactly where the detector is needed | OCR (`sighter-ocr`) |
| 5. Segments in priority order, results streamed | **1.0×** total if it runs to the end; **first result after one tile** (~0.18 s CPU, ~30 ms GPU); less if it stops early | With early stop: a better or second match elsewhere, so a query that needs uniqueness must finish its scope | none (scheduling in `sighter-core`) |

**Default pipeline:** normalise each display's frame to logical 100 % → limit to the query's scope (target window by
default) → split into 640 tiles with 64 px overlap → reuse cached detections for tiles the OS reports unchanged (dirty
rects, else a per-tile hash) → run the remaining tiles in priority order (focus and pointer first, then the rest of the
scope), batched on a GPU EP → stop early only when a unique match above the threshold is found and the query allows it →
merge boxes across seams (pure function in `sighter-core`). OCR stays a resolver layer and a label source, not the gate
for the detector. **For a viewer window** (remote desktop or VM console): the scope is the viewer's content
rectangle mapped to remote pixels, changed tiles come from Sighter's own per-tile hash instead of OS dirty rects, the
priority goes to the pointer region and the most recently changed tiles (the remote "focused window" is not known from
the host) ("Remote desktops and VM consoles in a viewer window").

### Coordinates

- The detector returns boxes in **tile pixels**; `sighter-core` maps them, with pure functions, to **frame physical
  pixels** (tile offset, then undo the logical-scale normalisation) and tags each box with its **display id**.
- At the input edge each `InputDriver` converts to what its OS wants: on macOS, points in the global display space (the
  display's origin plus pixels ÷ its scale factor, which ScreenCaptureKit reports per frame as
  `SCStreamFrameInfo.scaleFactor`); on Windows, the sidecar and the input helper run **Per-Monitor V2 DPI aware**, so
  they see each display's raw pixels and every display can have its own scale (Microsoft: unaware or system-aware
  processes get virtualised, scaled coordinates; `GetDpiForMonitor`/`GetDpiForWindow` give the per-monitor DPI).
- Multi-monitor setups with mixed DPI: each display is captured, normalised and mapped on its own; a box never spans two
  displays; a window that spans two is split by display.
- **Test item** (`sighter-core`, pure, synthetic layouts): two and three displays with mixed scale (100 % 1920 × 1200
  left, 200 % 3840 × 2160 right, 150 % 2560 × 1440 above with a negative origin); a box on a tile seam; a box at each
  display edge; round trip tile → physical → logical → physical within ±1 px; property tests over random layouts and
  boxes. Plus one OS-level check per platform in the spike (a synthetic window at a known position on a 200 % display).

## Remote desktops and VM consoles in a viewer window

Sighter runs on the PC that has the GPU (the host). What it looks at is often a **viewer window** on that host: the
Remote Desktop client (mstsc or the Windows App), a Hyper-V, VMware, VirtualBox or UTM console, or a VNC client (user,
2026-10-29; proposal 7.6 `remote_session`). Sighter does not run inside the remote machine. This section applies the
rest of the ADR to that case. (Sighter's own embedded RDP client of proposal 7.15 receives the remote frames directly
with the exact remote resolution, so it needs only the parts on compression and input.)

- **Capture and inference**: the viewer window is captured **by window** on the host (ScreenCaptureKit window capture
  on macOS; window capture on Windows), and inference runs on the **host GPU** (decision 4). The CPU fall-back for "no
  usable GPU" stays but is a minor case: it covers a host without a working GPU (old driver, the DirectML gate failing,
  or Sighter itself started inside a VM or a Remote Desktop session, which is not the normal set-up).
- **Only pixels**, as for every target (decision 9): the detector and OCR carry everything. (The remote UI's own
  accessibility tree would not be reachable from the host anyway.)
- **Compression**: RDP and VNC codecs are lossy (blur, banding, soft text edges, progressive refinement after a change).
  Training uses the proposal's "remote codec" augmentation preset (JPEG/H.264-style artefacts) and, best, frames recorded
  through the same viewer and codec settings; OCR runs on upscaled crops (proposal 7.6). Accuracy on compressed frames is
  a spike item.

### Changed tiles without OS dirty rects

The OS dirty rects (`dirtyRects`, `GetFrameDirtyRects`) describe the host's screen: for a viewer window they say, at
best, which parts of the viewer repainted, and a viewer that composites on the GPU may report its whole window. Remote
protocols repaint in tiles and rectangles of their own, and lossy codecs refine pixels for a while after a change. So
for viewer windows Sighter **ignores OS dirty rects** and uses its own **per-tile hash** on the viewer's content area:

1. Crop the viewer chrome (title bar, toolbars, scroll bars) to the content rectangle (below).
2. Lay the tile grid in **remote-desktop pixels**, so tiles do not shift when the viewer window moves.
3. Hash a slightly quantised, downsampled copy of each tile (codec noise must not count as a change); a tile counts as
   changed when the hash differs, confirmed by a mean-absolute-difference threshold.
4. A changed tile is re-run only after it is stable for N frames (proposal 7.6's "stable frames"), so progressive
   refinement does not trigger repeated runs.

Hashing a frame is cheap compared with one detector tile (not measured; a spike number).

### Resolution and scaling

Three scales stack up: the **remote session's** resolution and scale factor (for example 1920 × 1200 at 100 %, or 4K at
150 %), the **viewer's** own scaling (1:1, fit-to-window, smart sizing, a VM console's zoom), and the **host display's**
scale factor (the viewer window's physical pixels on the host).

- **Mapping a box** (pure functions in `sighter-core`): tile pixels → viewer-capture pixels (host physical) → minus the
  content rectangle's origin → divided by the viewer scale `s = content width / remote width` (and the same for height)
  → **remote-desktop pixels** → divided by the remote scale factor → remote logical points, which is what the model is
  normalised to.
- **How Sighter learns the remote resolution and the viewer scale**, in this order: (1) Sighter's own embedded client
  knows both exactly; (2) the target setup records the remote resolution and scale (entered or confirmed by the user,
  and stored with the target as proposal 7.15 already requires: "fixed session resolution and scaling per target");
  (3) the content rectangle is found from the viewer window (chrome detection on the capture), which gives `s` from the
  recorded remote size; (4) a check at start: a known remote element (or a calibration pattern the user shows once)
  must land where expected, else Sighter says the viewer scale changed and asks the user to confirm. Reading the
  viewer's own settings (for example an `.rdp` file's smart-sizing flag) is a possible shortcut, verify per viewer.
- **Recommendation: 1:1 viewer scaling** (smart sizing and fit-to-window off, VM console at 100 %) and a remote
  resolution no larger than the viewer area. Then `s = 1`, small elements keep their pixels, and the downscale rows of
  the "smallest element" table do not apply. When the viewer scales down (a 4K remote session in a 1920-wide window is
  `s = 0.5`), the smallest element in remote pixels doubles: a 16 px element becomes 8 px on the host and is likely lost.
  Sighter warns in plain words when `s < 1` ("The remote desktop is shown smaller than its real size; small controls may
  be missed. Turn off scaling in the viewer.").
- A viewer that scrolls (1:1 but larger than its window) shows only part of the remote desktop: Sighter treats the rest
  as not visible and never acts outside the visible content rectangle.

### Input into the viewer

If Sighter clicks or types, it acts on the viewer window, so the mapping runs backwards: remote-desktop pixels → × `s`
→ plus the content origin → plus the viewer window's current position on the host → host input coordinates (points on
macOS, Per-Monitor V2 physical pixels on Windows, as in "Coordinates"). The viewer is brought to the front before input,
and Sighter re-checks the content rectangle just before acting. Where a viewer filters injected input, Sighter fails with
a message and a next step (proposal 7.6).

**Test item** (pure, synthetic): remote 1920 × 1200 at 100 % and 3840 × 2160 at 150 %, shown 1:1, fit-to-window at
`s = 0.75` and `s = 0.5`, fullscreen, moved, on a 100 % and a 200 % host display, and spanning two host displays; a click
on a known remote pixel must land within ±1 remote pixel after the round trip remote → host → remote, and a box must map
host → remote → host within ±1 host pixel.

### Viewer window changes

| Change | What Sighter does |
|---|---|
| Moved | Window capture does not depend on position; input uses the window's position read just before acting; the tile cache stays (tiles are in remote pixels) |
| Resized | Re-derive the content rectangle and `s`; if `s` changed, drop the tile cache and warn if `s < 1` |
| Fullscreen (or back) | Chrome disappears or returns: re-derive the content rectangle and `s`; input coordinates follow the new geometry |
| Spans several monitors | One window capture gives one image; the window has one DPI on Windows (the monitor it is mostly on) and the mapping uses the window's geometry, so monitor borders do not matter; mixed-DPI tests cover it |
| Minimised or hidden | Some viewers stop rendering when minimised (proposal 7.15): Sighter pauses with "The remote desktop window is minimised; restore it to continue", never acts on a stale frame |
| Covered by another window | Window capture still returns the viewer's pixels where the OS supports it (verify per OS); before input the viewer is brought to the front |

## Training from test-run videos

The main way an app gets its model (user, 2026-10-29; proposal section 6.1):

1. **Import**: a test-run video of a viewer window (or a local window) is imported as a `paddock-jobs` job with phases
   (decode, sample, de-duplicate, store), progress on each phase's own scale and Cancel (ER-17 to ER-19). Frames come
   through `paddock-video` (the shared FFmpeg runtime). **Near-duplicate frames are dropped**: a perceptual hash per
   frame plus a mean-absolute-difference threshold, both tolerant of codec noise (the same quantised hashing as the
   viewer-window tiles). The frame count after de-duplication is shown.
2. **Auto-label**: the user starts it after a **confirmation that states the frame count and the expected time** (rule
   46; amended proposal 9.1). **Boxes come from computer vision only**: OCR (text and text-bearing controls), templates
   (crops the user marked), and the app's own detector once one exists. **Pitwall's vision model never places boxes**;
   when Pitwall is present and the user agrees, it **names and describes** the boxed regions and **suggests a class**
   from numbered crop sheets (proposal 9.1 "element naming"). That keeps the pipeline pixels-only (decision 9): the VLM
   reads pixels too, and only labels what computer vision found. Without Pitwall, names come from OCR text.
3. **Review queue**: the most uncertain labels first (low detector score, disagreement between sources, a VLM class
   that differs from the detector's), with one-key accept and quick fix (proposal section 5).
4. **Train**: RF-DETR **Small** as the app's own model, on the GPU: MPS on Apple Silicon (CoreML is for inference), the
   opt-in CUDA runtime on NVIDIA (licence exception), else the CPU or a remote host. Augmentation per "Augmentation and
   synthetic data".
5. **After training**: later videos of the same app are pre-labelled by the app's own model; the VLM is used only for
   **new, unknown screens** (a screen whose fingerprint matches no reviewed screen and where the detector is unsure),
   again after a confirmation with the count.

**Spike item**: label quality (precision and recall of auto-labels against a hand-reviewed set, per class) and time per
frame (import, auto-label, review) on a real recorded test video of a test app (synthetic data, never real user
recordings: AGENTS.md).

## Augmentation and synthetic data

What the training runtime applies, with the RF-DETR or Albumentations option where one exists (read in the `rfdetr`
repository at `f381551` and on PyPI, 2026-10-29). RF-DETR's default training stack is "resize/crop scale jitter plus
horizontal flip at 50%" (`docs/learn/train/augmentations.md`); custom stacks are passed as `aug_config` (Albumentations
transform names, `rfdetr[augment]`), with `multi_scale` (`per-batch`, `per-sample`, `off`), `expanded_scales` and
`scale_jitter` for scale. RF-DETR has **no built-in mosaic, copy-paste or class balancing**; those are Sighter's.
Albumentations transform names below are to be checked against the pinned version.

| Group | Technique | How (RF-DETR option or equivalent) | Notes |
|---|---|---|---|
| Photometric | Themes and accents, brightness, contrast, gamma, blur, noise | `aug_config`: `RandomBrightnessContrast`, `RandomGamma`, `HueSaturationValue` (accent shifts), `GaussianBlur`, `GaussNoise` | Real themes (light, dark, high contrast) come from recording, not from colour jitter |
| Remote artefacts | The **remote-codec preset**: RDP/VNC-style JPEG, H.264/AVC, 16-bit colour, chroma blur, smart-sizing resampling | JPEG: `ImageCompression`; resampling: `Downscale`; 16-bit colour (RGB565) and chroma blur: small custom transforms; H.264/AVC: rendered offline through `paddock-video`'s FFmpeg at dataset export | Best: frames recorded through the same viewer and codec |
| Geometric | Display scale 100–200 %, viewer scale 0.5–1×, small shifts and crops, window sizes | `multi_scale`, `expanded_scales`, `scale_jitter`; `Affine` with small translation; window sizes from recording | **No flips or rotations**: UI layout and text are not mirror- or rotation-symmetric (a flipped "Save" button or a right-to-left toolbar would teach the model shapes that never occur). RF-DETR's default flip is turned off by passing Sighter's own `aug_config` without `HorizontalFlip` |
| Composition | Copy-paste of controls onto other screens and backgrounds; mosaic; occlusion by cursor, tooltips, dialogs | Sighter's dataset assembly (offline): paste reviewed crops with their boxes; 2 × 2 mosaics of frames; overlay cursor sprites, tooltip and dialog crops (partly occluded boxes kept or dropped by visible fraction) | Exact labels for free; keep pasted scale within the recorded scale range |
| Text | Re-render label text with other strings, lengths, fonts and languages | Offline: fill the text region with its background colour and draw new text (system fonts on the training machine) | Experiment: only kept if the held-out set shows a gain |
| Label economy | Label propagation by tracking across video frames | Track each reviewed box into neighbouring frames (template match in a small search window); propagated boxes enter the review queue as "proposed" | Pure Rust in `sighter-core` |
| | Active learning | Review queue orders frames by uncertainty (score margin, source disagreement) | `sighter-core` |
| | Hard-negative mining | Regions the user rejected (false positives) are kept as background in their frames; frames with many rejections are sampled more | `sighter-core` dataset assembly |
| | Class balancing | Repeat-factor sampling of frames that contain rare classes | `sighter-core` dataset assembly (no RF-DETR option) |
| Synthetic screens | Run the app under test with varied themes, fonts, window sizes and data, and record it | Sighter's recorder with a variation plan (proposal section 6, item 1: "real variation captured by the recorder") | The most realistic "augmentation"; the studio shows coverage gaps |

**Honest measurement.** A **held-out set of real, unaugmented frames**, split by recording session (never frames of
the same session in train and test), is never augmented and never used for training. Each technique (or group) is
switched **on and off** against it, with everything else fixed, and kept only if it raises per-class mAP@0.5:0.95 or
lowers the wrong-click rate. **Spike item**: record which techniques actually raise accuracy, as results blocks in
[performance.md](../performance.md) (rule 48).

**Where the code goes.** Online augmentation (photometric, JPEG, resampling, scale, small shifts) runs in the **Python
training runtime**, as an `aug_config` built from a Sighter preset and passed to `rfdetr`. Pinned libraries:
Albumentations **2.0.8 (MIT)** through `rfdetr[augment]` (`albumentations>=1.4.24,<3.0.0`) and Kornia 0.9 (Apache-2.0).
**Not AlbumentationsX**: the maintained successor on PyPI is AGPL-3.0 (2.5.1, 2026-10-11) and is excluded; the MIT
Albumentations' last release is from 2025-05-27, so the runtime pins it and its licence is checked on every bump.
Offline steps (de-duplication, copy-paste, mosaic, occlusion, text re-rendering, H.264 rendering, tracking, active
learning, hard negatives, class balancing) are Rust in `sighter-core` and `sighter-frames`, which write a COCO dataset
for the runtime. Shared pieces: `paddock-video` (decode, FFmpeg runtime, codec rendering), `paddock-jobs` (import,
auto-label and training jobs), `paddock-runtime` (the Python runtime), `paddock-pitwall` (naming requests). The
near-duplicate and tile hashing is one implementation in `sighter-core` for both uses.

## Licence exceptions

Sighter's code and every bundled or downloaded component are permissive (Sighter `deny.toml`), with exactly two named
exceptions, one shipped (conditional) and one downloaded on request:

| File | Licence | Scope | Reason | Approved |
|---|---|---|---|---|
| `DirectML.dll` 1.15.4 (NuGet `Microsoft.AI.DirectML`, SHA-256-pinned) | Microsoft Software License Terms, Microsoft DirectX Machine Learning (DirectML): proprietary; distribution allowed only inside an application, for Windows and Xbox, not stand-alone; may collect data | Windows only, shipped next to `sighter-infer.exe`; never another file of that package, never another app. **Condition: the DirectML gate passes** (no outbound connection, and no ETW/TraceLogging provider or an inert one; decision 4, plan item P6); otherwise it is not shipped and Sighter uses the system `DirectML.dll` with a delay-loaded import (or a CPU-only build) or the CPU | DirectML is how the ONNX Runtime build reaches any DirectX 12 GPU (NVIDIA, AMD, Intel) on Windows; the build imports it | user, 2026-10-29 (conditional) |
| NVIDIA CUDA runtime libraries (CUDA runtime, cuBLAS, cuDNN and the other NVIDIA libraries the pinned PyTorch CUDA wheels depend on) | NVIDIA CUDA EULA (v13.4) and the NVIDIA SDK licence with its cuDNN supplement (v. 2022-02-22): proprietary; runtime `.so`/`.dll` files distributable only "as incorporated in object code format into a software application" with "material additional functionality", "only accessed by your application" | **Training only**: the opt-in CUDA variant of the managed Python training runtime, **downloaded on request from the upstream hosts** (`download.pytorch.org` for the PyTorch CUDA wheels; `pypi.org` and `files.pythonhosted.org` for the `nvidia-*` wheels they depend on, Linux), **never mirrored on our Gitea** (a stand-alone copy of NVIDIA's files would fall outside EULA 1.1.1's grant); never in the installer, never used by inference; NVIDIA's terms shown before the download; those hosts are on the `Egress` allow-list for this download job only | PyTorch has no other accelerator for NVIDIA GPUs; without it NVIDIA laptops train on the CPU or a remote host | user, 2026-10-29 |

cargo-deny checks crates, not shipped DLLs or downloaded runtimes, so the exceptions are written in
`apps/sighter/deny.toml` as a commented "non-crate binaries" list with these reasons, and the third-party notices list
them. The CUDA runtime is also an entry in the runtime catalogue with its licence shown before download. A check over the bundle's files
against that list comes with the notices tooling.

## Plan items

- **P1. Mirror the COCO checkpoints** (from decision 6). Download `rf-detr-nano.pth`, `rf-detr-small.pth`,
  `rf-detr-medium.pth` and `rf-detr-large-2026.pth` from the URLs in the pinned `rfdetr` version's `model_weights.py`,
  check upstream's MD5s, compute SHA-256s, and upload them with `LICENSE` (Apache-2.0) and a `NOTICE` naming the
  source, version and upstream URLs to a release or package on dev.decryptology.net; then write the catalogue entries.
  About 1.5 GB (Nano and Small measured at 366 and 386 MB; Medium and Large estimated). **Needs the user's OK when it
  happens**: it is a large, outward-facing upload, so the agent doing it asks first and does not do it on its own.
- **P2. Update the proposal** when this ADR is accepted (sections 6 and 8, see Consequences), and close proposal section
  15 question 1; restate section 7's locate budgets as the gate and target tiers. Section 9.1 was already amended, and
  sections 6.1 and 6.2 added, on 2026-10-29.
- **P3. Shared single-file asset kind** in `paddock-runtime` (or a shared model catalogue), with Pacenotes' Model2Vec
  and Pitwall's model downloads as the other users (rule 42, rule of three); downloads through `paddock-net` `Egress`.
- **P4. RT-DETRv4 licence review** (research note section 9): ask the authors for the checkpoints' licence and whether
  they contain teacher weights; inspect a checkpoint's keys; get a reading on DINOv3-distilled weights. Pending until
  done.
- **P5. ONNX Runtime build pipeline** (decision 4). A CI job per target (macOS arm64 + CoreML; Windows x64 + DirectML;
  Windows arm64 + DirectML, cross-compiled or on an arm64 runner) that builds a pinned ONNX Runtime release tag (1.31.x,
  submodules at the tag's commits) with `--no_telemetry`, static libraries, Release, tests skipped; checks the result
  (symbol check for telemetry, a minimal `ort` program that loads the self-test model on each EP); and publishes the
  libraries as a versioned, SHA-256-pinned artefact on our Gitea, keyed by (ORT version, flags, toolchain). App builds
  consume that artefact through `ORT_LIB_PATH`; they never build ONNX Runtime themselves. Cache: the artefact itself
  (rebuilt only when the key changes) plus the CI cache server for the C++ object files. Renovate watches the ORT tag;
  a bump is one PR that rebuilds the artefact, re-runs the symbol check and the egress test. `DirectML.dll` is fetched
  from NuGet once, pinned by SHA-256, and stored with the artefact. **Estimate:** about 5–8 agent-days (build scripts
  and flags per target 2–3, CI jobs, artefact publishing and caching 1–2, `ort` custom-lib wiring and the checks 1–2,
  Windows arm64 1); CI time per cold build not measured (a full ONNX Runtime build is long, so it must never run in
  an app's normal pipeline).
- **P6. Spike gate for `DirectML.dll`**: on Windows with our `DirectML.dll` on the DirectML EP, (1) the egress test
  (no outbound connection from `sighter-infer`) and (2) the ETW check (search the DLL for TraceLogging provider metadata
  and GUIDs; an ETW session with `logman` or `tracelog` listing providers and events while the EP runs). It passes only
  if there is no connection and no provider is registered, or the provider is shown to be inert. A failure removes the
  file from the bundle and switches Windows to the system copy (delay-loaded import or CPU-only build) or the CPU
  (decision 4). Record the result in the spike notes.

## Consequences

- One adapter covers RF-DETR, D-FINE and RT-DETR alike; swapping the detector is a different ONNX file and a catalogue
  entry, not new code (open/closed).
- Sighter carries ONNX Runtime (~80 MB static in one sidecar binary, size of pyke's equivalent build) built by our own
  pipeline (plan item P5) instead of pyke's CDN download. Intel Macs are not supported (already decided). Linux, if
  ever needed, is one more target of that pipeline (pyke offers only CPU and CUDA builds for Linux arm64).
- `cargo deny` must allow `ort`/`ort-sys` (MIT OR Apache-2.0) and ONNX Runtime's notices; the model catalogue gets its
  own licence allow-list (Apache-2.0, MIT) checked by a test over every entry. `DirectML.dll` and the CUDA training runtime are the two named exceptions
  ("Licence exceptions"), listed in the third-party notices.
- macOS packaging has nothing new beyond Pacenotes' findings (macOS ≥ 13.4 link floor, `libclang_rt.osx.a`). Windows
  packaging adds `DirectML.dll` next to `sighter-infer.exe`.
- CoreML compiles each model on first use: 23 to 49 s per session here, without a cache (at load 18–24, indicative,
  [performance.md](../performance.md#2026-10-29-rf-detr-onnx-latency-indicative-docssighter-rfdetr)), against a 1.5 s
  cold-load budget. "Preparing the model for this Mac" runs as a job right after training or import; until it is done,
  locate answers on the CPU EP. Once compiled, a 640 px tile of Small took about 30 to 35 ms (at load 18–24,
  indicative); on 4 CPU threads about 0.2 s, so CPU locating works on window scopes and changed tiles, not on whole
  tiled frames (see the projections).
- Determinism is promised bit-identical only on the CPU EP with fixed threads (to be shown in the spike); GPU EPs promise
  "same top candidate within tolerance".
- The training runtime pins an upstream that releases often; every bump re-runs the export and self-test checks.
- Apache-2.0 does not force its licence onto a model fine-tuned from the checkpoint, but it keeps notice duties: the
  model card carries the base checkpoint's licence, notice and lineage; the user's own data and model stay the user's.
- Proposal section 6 (presets), section 8 (licence table) and 9.1 are updated when this ADR is accepted: RF-DETR becomes
  primary, DEIMv2 moves to excluded (non-commercial), RT-DETRv4 is pending, UI-DETR-1 and the "Florence-2 bootstraps
  boxes" idea are not v1, and 9.1's label suggestions follow decision 5 (applied on 2026-10-29).
- A new app starts with no detector: until the user has reviewed enough frames to train the first model, labelling is
  OCR, templates and manual review. This is the price of leaving UI-DETR-1 out of v1 and of using pixels only.
- **Cost of 100 % computer vision** (decision 9): no ground-truth element names, roles or states from the OS, so labels,
  names and classes come from the user's review. Mitigation: OCR text inside or next to a box is the default label and
  name (the user confirms or corrects it in the review queue), and the VLM can suggest names for a small user-chosen
  selection (rule 46). Follow-up: Sighter AGENTS.md ("Vision first": accessibility trees as an optional `LabelSource`)
  and proposal sections 5, 8 (`LabelSource` list: `A11yUia`, `A11yAx`, `A11yAtspi`, `Dom`) still describe accessibility
  sources and must be updated to match (plan item P2).
- The mirror makes downloads independent of Roboflow's URLs (the Large checkpoint already moved once), and costs about
  1.5 GB on our Gitea. Updating a checkpoint is a new mirror entry with a new hash, never an overwrite.
- Other execution providers (research note section 11): v1 inference stays CPU, CoreML and DirectML, which already use
  NVIDIA and Intel GPUs on Windows. OpenVINO is a candidate opt-in after the spike; CUDA/TensorRT inference is not
  planned. CUDA training is an opt-in download, approved as a recorded licence exception (user, 2026-10-29).

## Decided questions (user, 2026-10-29)

1. Default detector: **RF-DETR Small**; Nano is the CPU preset, Medium or Large the "Accurate" preset; **D-FINE** is
   the fallback, behind the same ONNX adapter (decisions 1 and 2; COCO-only checkpoints).
2. Start weights: **mirror the Apache COCO checkpoints on our Gitea**, pinned to our own SHA-256s (decision 6, plan
   item P1; the upload needs the user's OK when it happens).
3. UI-DETR-1: **not in v1**, because its datasets' licences are not stated (decision 3).
4. Labelling: **whole recordings are pre-labelled by the user's own detector**; Pitwall's VLM only for small,
   user-chosen selections or after a confirmation that states the frame count (rule 46; decision 5).
5. RT-DETRv4: **do a licence review**; the model stays pending until it is done (decision 2, plan item P4).
6. Windows: **ship Microsoft's DirectML redistributable** next to the sidecar (decision 4), **conditional on the
   DirectML gate** (item 10); if the gate fails, the system `DirectML.dll` or the CPU.
7. DirectML: **approved as a named licence exception for `DirectML.dll` only** ("Licence exceptions").
8. Telemetry: **ONNX Runtime is self-built with `--no_telemetry`** per platform and linked through `ort`'s custom-library
   mode, not pyke's prebuilt libraries; the runtime opt-out and the egress test stay as a second line of defence
   (decision 4, plan item P5).
9. Objects365 lineage: **accept RF-DETR's N/S/M/L checkpoints on Roboflow's Apache-2.0 label**, because Sighter is for
   personal, non-commercial use; the lineage is recorded in every model card (decision 1).
10. DirectML: the redistributable exception **holds only if the DirectML gate passes** (no outbound connection, and no
    ETW/TraceLogging provider or an inert one); otherwise the system `DirectML.dll` or the CPU only (decision 4,
    "Licence exceptions", plan item P6).
11. CUDA: **an opt-in CUDA training runtime is approved as a recorded licence exception**, for training only and
    downloaded on request ("Licence exceptions").
12. Proposal 9.1: **the amendment is approved** and applied to the proposal (decision 5).
13. Device: **GPU by default** (CoreML; DirectML on Intel integrated or NVIDIA discrete GPUs, including the system
    DirectML), with a start-up self-test of the user's model against its stored CPU answer and an automatic fall-back to
    the CPU **with the same model** (an optional "Train a Fast model for this computer" action), explained to the user
    and selectable in Settings; the device is recorded on every result and job (decision 4).
14. Remote targets: **Sighter runs on the host that has the GPU** and looks at a viewer window (Remote Desktop, VM
    console, VNC client); only pixels are available, per-tile hashes replace OS dirty rects, boxes and input map
    through the viewer's content rectangle and scale, and 1:1 viewer scaling is recommended ("Remote desktops and VM
    consoles in a viewer window").
15. Pixels only: **Sighter is 100 % computer vision** for local and remote targets; OS window bounds are allowed only
    to pick and follow the capture target; pre-labelling before the first model is OCR, templates and review
    (decision 9).
16. Training path: **video → auto-label → review → train**; boxes from OCR, templates and the app's own detector;
    Pitwall's VLM only names, describes and suggests classes, after a confirmation with the count; RF-DETR Small on the
    GPU; later videos pre-labelled by the app's model, the VLM only for new screens (decision 10).
17. Augmentation and synthetic data as in that section, each technique kept only if the held-out real frames show a
    gain.
18. Latency budgets in two tiers: a **gate** (full frame p95 ≤ 500 ms GPU / 2 s CPU at 1920 × 1200, ≤ 1 s / 4 s at
    4K) and the old numbers as a **target**, as named constants ("Latency budgets: gate and target").

## Open questions for the user

None. The only pending item is the RT-DETRv4 licence review (plan item P4); RT-DETRv4 stays out of the catalogue until
it is done.

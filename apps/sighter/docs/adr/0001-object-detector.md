# 1. Sighter's object detector: RF-DETR (Apache sizes) on ONNX Runtime in Sighter's own sidecar

Status: Accepted (2026-10-30, by the user) after five independent review rounds. The user decided every question on
2026-10-29 (marked "user, 2026-10-29" below); none is open. Pending: the RT-DETRv4 licence review (plan item P4) and the
DirectML egress gate of the spike (decision 4). Revision 4 (2026-10-29) answers the third independent
review: gate conditions per execution provider, "Parallelism and cost" (rule 51), priority classes (ADR 0024), Patience
instead of runtime budgets (ADR 0031), training geometry, password redaction, no remote training in v1, and the VLM's
role; it reopens none of the user's decisions.

Related: [proposal](../proposal.md) sections 6, 8, 9.1, 11, 12 and 15; [design review](../review/2026-10-design-review.md);
research notes [2026-10-29-rf-detr](../research/2026-10-29-rf-detr.md) (this ADR's evidence) and the family note
[2026-10-sighter-detector](../../../../docs/research/2026-10-sighter-detector.md); Sighter AGENTS.md ("Licences of
models", "Pitwall is always optional", "Local only: no telemetry"); family rules 10 to 15, 23, 26, 42, 46 and 48;
ADR 0024 (background work: priority classes, `paddock_proc::threads`, C40), ADR 0031 (patience, not wall clocks:
stall judgements, caps on host ticks); rule 51 (parallelism and cost); Pacenotes ADR 0001 (static ONNX Runtime and rule
23).

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

**What the VLM may do.** Boxes come from computer vision: OCR, templates, the app's own detector and the user's review.
Pitwall's VLM (proposal 9.1) **never places a box that becomes a label, a training box or a stored detection**: it names
boxes that exist, suggests their class, and describes steps and screens. **Runtime grounding (proposal 9.1 kind 3) is
the one exception**, and a narrow one: when every cheaper layer failed on one query, an attended run (or a step whose
author enabled it) may ask Pitwall "where is X" and act on the returned box **for that one action only**, after the
usual re-check before acting. The result is marked `source=vlm-grounding` in the run's trace, it is never written into
the dataset, the cache or the review queue as a box (the frame is queued as "needs labelling", without the box), and
unattended runs never use it by default.

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
   outbound connections. **Where the DINOv2 files come from offline** (read in the `rfdetr` 1.11.2 wheel,
   `rfdetr/models/backbone/dinov2.py` and `rfdetr/config.py`, 2026-10-29): N, S, M and L use the windowed backbone with
   `patch_size` 16; for a patch size other than 14 the backbone sets `load_dinov2_weights = False` and builds
   `WindowedDinov2WithRegistersBackbone(config)` from a JSON config **shipped inside the package**
   (`rfdetr/models/backbone/dinov2_configs/dinov2_small.json` and siblings), so `from_pretrained("facebook/dinov2-…")`
   is not called and nothing is fetched from the Hugging Face Hub; the backbone weights come from the COCO checkpoint
   itself. The checkpoint is passed as a full path to the mirrored file (decision 6): `rfdetr` downloads only when the
   file is missing (`download_pretrain_weights` skips an existing file), so the runtime always hands it a present,
   SHA-256-checked file. The non-windowed path (`AutoBackbone.from_pretrained`) is used only by sizes Sighter refuses.
   The runtime's self-test trains one step on a generated mini dataset with the network denied, which proves this for
   each pinned `rfdetr` version. Start weights: the Apache COCO checkpoints only. **UI-DETR-1 is not in v1** (user,
   2026-10-29): neither as a labeller nor as start weights, because the licences of its training datasets are not
   stated.
4. **Inference: `ort` with statically linked ONNX Runtime**, in Sighter's `sighter-infer` sidecar (started through
   `paddock-ipc`, supervised by `paddock-proc` with a memory cap measured as footprint per rule 26, `--self-test` per
   rule 23), shared with OCR.
   - **Process class** (ADR 0024): `sighter-infer` serves interactive locates, so it is launched at **`User`**, with
     intra-op threads from `paddock_proc::threads(Cpu, User)` (performance cores − 1; 9 on the M4 Pro) and never
     below `User`. Background work in it (pre-labelling, evaluation) runs on threads of class `Background`: **the class
     travels with each IPC call** (ADR 0020 WP-9, tiered workers), and the sidecar's one GPU queue orders work by it
     ("Parallelism and cost"). CoreML preparation runs in a **separate short-lived `sighter-infer --prepare` process
     launched at `Background`**, so a 23–49 s compile never competes with locates at `User`.
   - **GPU by default** (user, 2026-10-29): CoreML on macOS with `ModelFormat=MLProgram`, **`MLComputeUnits=CPUAndGPU`**
     (the measured faster and quicker-to-compile setting: Small 640 p50 29.7 ms against 35.4 ms for `ALL`, session
     creation 23.2 s against 49.1 s, [performance.md](../performance.md#2026-10-29-rf-detr-onnx-latency-indicative-docssighter-rfdetr);
     `ALL` can place work on the Neural Engine, which the spike compares but does not default to),
     `RequireStaticInputShapes=1` and `ModelCacheDirectory` set to the model's cache folder; DirectML on Windows, on Intel integrated
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
5. **Not in Pitwall.** Pitwall stays optional and is the VLM layer for **names and classes of boxes that already
   exist**, step and screen descriptions, and the runtime grounding fallback (the one exception, see Context), each a
   bounded, user-started request (rule 46). **The VLM never places boxes** in labels, datasets or stored detections.
   **Labelling** (user, 2026-10-29): whole recordings are pre-labelled by the user's own trained detector for that app.
   Pitwall's VLM names and classifies boxes only on a small selection the user chooses, or after a confirmation that
   states the frame count (and the expected time). Before the first model of an app exists, boxes come from OCR,
   templates and the review queue only (decision 9, "100 % computer vision"). This amends proposal 9.1's first request
   kind, now "**names and classes for existing boxes**" (it was "label suggestions", which returned boxes); the user
   approved the 2026-10-29 amendment, and revision 4 aligns its wording with decisions 10 and 16 (boxes from computer
   vision only).
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
7. **Jobs.** Download, video import, pre-labelling, evaluation, CoreML preparation, dataset assembly and training are
   `paddock-jobs` jobs with phase-labelled progress and Cancel (ER-17 to ER-20, ADR 0024), each with a priority class:
   download, import, pre-labelling, evaluation and CoreML preparation run at **`Background`**; **training (with its
   dataset assembly) runs at `User`**, because in v1 training is only ever started by the user and the user waits for
   it (ADR 0024: the class is fixed at launch; a training run Sighter started on its own would be `Background`, and v1
   has none). Locate is a bounded request: its liveness is a Patience stall judgement and its only limit is a cap from
   the caller's step timeout ("Locate at run time: no budgets, a stall judgement and one cap"). Thread counts and the
   other tunables are in "Parallelism and cost".
8. **Screens, tiles and coordinates** as in the next section: frames are normalised to logical (100 %) scale per
   display, tiled at 640 with overlap, unchanged tiles reuse cached detections, the active window goes first, and boxes
   come back in physical pixels with their display and are converted to points at the input edge.
9. **100 % computer vision** (user, 2026-10-29: "Sighter will be 100% computer vision"). Sighter uses only pixels, for
   local windows and remote ones alike: no accessibility tree, no UI Automation or AX APIs, no AT-SPI, no DOM, no app
   hooks or injected code. Pre-labelling before an app's first model comes from OCR, templates and the review queue
   only. **Allowed:** window bounds, titles and the window list from the OS to pick and follow the capture target, and
   the pointer position: that is capture plumbing, not UI understanding, and it never names, classifies or locates an
   element. **Adapter hints** (user, 2026-10-29): Sighter itself never calls UIA, AX, MSAA or AT-SPI, but a user's own
   test-framework adapter (`Sighter.FlaUI`, `sighter-pywinauto`, the 7.13 helpers) may pass rectangles it already has
   (an element's bounding rectangle, a window handle's bounds) as **optional hints**: they narrow where Sighter looks
   and rank candidates, they **never replace detection** (a hint with no detected element inside it is "not found"),
   and every result records them as **hint-sourced**. A hint states its coordinate space and enters through the one
   coordinate chain ("Adapter hints: coordinate space"). The cost and its mitigation are in Consequences.
   **Secrets without accessibility**: see "Password fields and secrets, from pixels".
10. **Main training path: video → auto-label → review → train** (user, 2026-10-29), detailed in "Training from
    test-run videos" below and in proposal section 6.1. Boxes always come from computer vision; Pitwall's VLM only
    names, describes and suggests a class for regions that already have boxes.
11. **Spike before the first release**, per the research note's checklist and the spike items of this ADR (listed in
   "Spike items"); its numbers go into [performance.md](../performance.md) as results blocks (rule 48), under the
   conditions of "Gate conditions per execution provider", and can move the default preset or switch the CPU preset to
   the fallback without changing this ADR.

## Screen resolutions and HiDPI

Sighter supports 1920 × 1200 (WUXGA) and larger: 2560 × 1440, 3840 × 2160 (4K) and 5120 × 2880 (5K), with HiDPI and
Retina scaling where the logical size (points, or DIPs on Windows) differs from the physical pixels (user, 2026-10-29).

### How frames map to detector input

Tiles are square, 640 or 960 px, with a **64 px overlap**. With an overlap of `o`, only an element **at most `o` px
wide (and high) is guaranteed to lie whole inside some tile**; a wider element that straddles a seam is cut into parts
in neighbouring tiles and relies on the **seam merge** (a pure function in `sighter-core` that joins boxes of the same
class from neighbouring tiles whose parts in the shared overlap agree; the exact rule is in "Decisions for the pure
core"). Should the overlap match the largest class width? No: wide classes
(text boxes, tabs, toolbars, often 200–600 px) would need huge overlaps and many more tiles, and a wide element is
still visible in both parts; the overlap is set to cover the **small classes** whose parts would be too small to
detect (icons, checkboxes, radio buttons, small buttons: about 16–64 logical px), and the model card records the
overlap and the largest small-class size measured in the training set. Recall at tile seams is a spike item. Tiles per
frame = ⌈(W − 64) / (T − 64)⌉ × ⌈(H − 64) / (T − 64)⌉.

**The grid, exactly** (one pure function in `sighter-core`, `tile_grid`, used by inference **and** by dataset assembly,
so training tiles are the tiles the detector sees): the grid lies on the target's logical (100 %) space with its origin
at the target's logical (0, 0); along each axis, an extent no larger than a tile gets one tile at 0 (padded to the tile
size); a larger extent gets ⌈(extent − overlap) / (T − overlap)⌉ tiles at the stride T − overlap, the last one pulled
back so that it ends on the edge. Tiles are numbered in raster order.

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
| Tile at physical 1:1 | 16 at every size | 8 to 45 tiles (table) | Most detail, most cost; elements look 2× larger at 200 % than in the 100 % training data, which training does not cover (scale jitter stays near 1.0, "Training geometry matches inference"), so this is not used |
| **Normalise to logical 100 % per display, then tile** (default, design review P1) | 16 logical px (16 pt), which is 32 physical px at 200 % | the logical size's tile count: 8 for 4K at 200 %, 15 for 5K at 200 % | **Default**: model sees the scale it was trained at; a 4K or 5K Retina display costs what its logical size costs |
| Logical tiles plus a **1:1 fine pass** on chosen regions | 16 physical px in those regions | + tiles of the region only | For classes the model card marks as small (icons, checkboxes, grid cells) when recall below 16 pt is too low |

### Latency per resolution (projections)

Projection = tile count × the measured per-tile time (RF-DETR Small at 640, batch 1, run one after another;
[performance.md](../performance.md#2026-10-29-rf-detr-onnx-latency-indicative-docssighter-rfdetr), at load 18–24,
indicative). **These are projections, not measurements.** Batching tiles on a GPU and using `threads(Cpu, User)` CPU
threads (9 on the M4 Pro; the run used 4) would both lower them; neither was measured.

### Latency budgets: gate and target

User, 2026-10-29: Sighter is for UI test automation. A test step waits for the UI to settle anyway (hundreds of
milliseconds to seconds), so a single locate matters less than **accuracy and reliability**; faster is still better.
The locate budgets therefore come in two tiers. Their numbers are **named constants in one place** (the `budgets` module
of `sighter_core::bench`, mirrored in [performance.md](../performance.md)), so changing a number is a one-line change.
**They are constants for benchmarks only** (ADR 0031): the benchmark harness and the spike read them; **runtime code
never reads them**, and no locate, job or sidecar call at run time is stopped because one of these numbers passed
("Locate at run time: no budgets, a stall judgement and one cap").

| Tier | Constant (proposed name) | 1920 × 1200 (or 4K at 200 %) | 3840 × 2160 at 100 % | Role |
|---|---|---|---|---|
| **Gate** (must pass) | `LOCATE_GATE_GPU_P95`, `LOCATE_GATE_CPU_P95` | full-frame locate p95 ≤ **500 ms** on the GPU (CoreML or DirectML), ≤ **2 s** on the CPU | p95 ≤ **1 s** GPU, ≤ **4 s** CPU | Release gate, sized for test steps that take seconds |
| **Target** (not a gate) | `LOCATE_TARGET_GPU_P50`, `LOCATE_TARGET_GPU_P95`, `LOCATE_TARGET_CPU_P95` | p50 ≤ **80 ms** and p95 ≤ **250 ms** on the GPU; p95 ≤ **600 ms** on the CPU | same | For live and watch modes and fast suites; reached through changed tiles, priority order with early stop, or batching |

The 4K gate constants are `LOCATE_GATE_GPU_P95_4K` and `LOCATE_GATE_CPU_P95_4K`.

#### Gate conditions per execution provider

A gate number means nothing without its conditions. The gate is measured by **one benchmark, `sighter-bench locate`**
(a binary of a `sighter-bench` crate, to be added with the sidecar; it times through the harness in `sighter_core::bench`
and **emits the rule-48 results block** for [performance.md](../performance.md) itself, with machine, load, build,
corpus, command, runs, numbers, budget, verdict and footprint). It runs nightly in `.gitea/workflows/sighter-bench.yml`
once Sighter has CI, and by hand in the spike until then. One process per row; rows of an A/B comparison are
interleaved (benchmarks guide, rule 4).

| | **CoreML** (macOS) | **CPU** (reference and fallback) | **DirectML** (Windows) |
|---|---|---|---|
| Reference machine | Mac mini M4 Pro, 14 cores (10 performance, 4 efficiency), 64 GB, macOS 27.x | (a) the same Mac mini; (b) the family's Windows host runner (`windows-latest`, a VM without a GPU: the CPU-only runner class, its vCPU count recorded in the block) | **None in the family yet.** A physical Windows 11 PC with a DirectX 12 GPU (Intel integrated Xe and one NVIDIA discrete card); the `windows-latest` runner is a VM and is not expected to expose a GPU (spike item S2 checks) |
| Runner class | macOS host runner (`macos-latest`, on the Mac mini) | (a) `macos-latest`; (b) `windows-latest` | a local machine named by the user, never a CI runner; CI runs DirectML only as a functional check where a runner has a GPU |
| Threads | intra-op `paddock_proc::threads(Cpu, User)` (9) for the CPU partitions; inter-op 1 | intra-op `threads(Cpu, User)`: 9 on the Mac mini; physical cores − 1 on a uniform runner; inter-op 1, sequential execution | intra-op `threads(Cpu, User)`; inter-op 1 |
| EP options | `ModelFormat=MLProgram`, `MLComputeUnits=CPUAndGPU`, `RequireStaticInputShapes=1`, `ModelCacheDirectory` | none | device 0 (the adapter Windows lists first; its name recorded), `error_on_failure` |
| Cache and model state | **warm**: the CoreML compiled-model cache present (prepared), session created and model loaded before timing; 5 warm-up locates discarded | **warm**: session created, 5 warm-up locates discarded | **warm**: session created, 5 warm-up locates discarded |
| Cold rows (separate, not the locate gate) | model cold load with the cache present, against `MODEL_COLD_LOAD` (1.5 s); "prepare for this Mac" without a cache, recorded, no gate | model cold load against `MODEL_COLD_LOAD` | model cold load against `MODEL_COLD_LOAD` |
| Load guard | the harness reads the 1-minute load before each row: **below 12** (benchmarks guide) for a gate verdict; **12 to 25**: the row runs and is marked indicative, never a gate verdict; **above 25** (`RUN_LOAD_LIMIT`): the row is skipped and the block says so. A nightly gate failure measured at a load above 8 is re-run once when the load is below 8 before it counts; both runs go into the block | same | Windows has no load average: the harness records the processor queue length and CPU use for 10 s before the row and runs only when CPU use is below 20 %; nothing else may run on the machine |
| Runs and percentile | **200 timed locates** per row, frames cycled from the corpus; p50 and p95 as `sorted[⌊p · (n − 1)⌋]` (the harness's rule; for n = 200 this is the 190th value for p95, the same as nearest rank) | same | same |
| Corpus | 20 synthetic UI frames per size (1920 × 1200 and 3840 × 2160 at 100 %; 4K at 200 % normalised), rendered by the spike's generator (named in the block), never random tensors and never user captures | same | same |
| What is timed (gate) | **one full-frame locate in `sighter-infer`**, from the frame already in memory (shared with the sidecar) to the merged result: **normalise** (resample to logical) → **tile** → **infer** (every tile, no early stop, the tile cache empty) → **merge** across seams, plus the IPC round trip from the server. **Capture is not in the gate** (proposal 7: "1080p screenshot in memory") and is a separate row; cache, template and OCR layers are off (the detector path, the worst case) | same | same |
| Diagnostic rows (no gate) | inference only, per tile (`Session.run` of one 640 tile), the number the projections use; capture of one frame | same | same |
| Status | indicative numbers only ([performance.md](../performance.md#2026-10-29-rf-detr-onnx-latency-indicative-docssighter-rfdetr): Python wheel, random tensor, 4 threads, load 18–24) | indicative, Mac only | **unmeasured**: no DirectML number exists; every DirectML verdict below is open until spike item S2 |

| Frame (pixels fed to the detector) | 640 tiles | CPU EP, p50 (179 ms/tile) | CPU EP, p95 sum (276 ms/tile) | CoreML CPU+GPU, p50 / p95 sum (30 / 32 ms per tile) | **Gate** (full frame, p95) | **Target** (p50 ≤ 80 ms, p95 ≤ 250 ms GPU; p95 ≤ 600 ms CPU) |
|---|---|---|---|---|---|---|
| 1920 × 1200, or 4K at 200 % | 8 | ~1.4 s | ~2.2 s | ~0.24 s / ~0.26 s | CoreML ≤ 500 ms: **projected to pass**; CPU ≤ 2 s: **projected over** (~2.2 s at 4 threads; 9 threads may fit) | **over** on both as a whole frame |
| 2560 × 1440, or 5K at 200 % | 15 | ~2.7 s | ~4.1 s | ~0.45 s / ~0.48 s | no gate set (between the two sizes) | **over** |
| 3840 × 2160 at 100 % | 28 | ~5.0 s | ~7.7 s | ~0.83 s / ~0.90 s | CoreML ≤ 1 s: **projected to pass** (tight); CPU ≤ 4 s: **projected over** | **over** |
| 5120 × 2880 at 100 % | 45 | ~8.1 s | ~12.4 s | ~1.3 s / ~1.4 s | no gate set | **over** |

All verdicts are **projections, to be measured** (spike item 8). The GPU columns are **CoreML only**: DirectML has no
measurement, so its gate verdict is open (spike item S2). The gate is projected to pass on CoreML at both gate
sizes and to be missed on the CPU at 4 threads (~2.2 s at 1920 × 1200). **Plan to close the CPU gate**, part of the spike
gate, tried in this order: (1) `threads(Cpu, User)` intra-op threads (9 on the M4 Pro) instead of 4; (2) the window scope (a typical app window is
4–6 tiles, not 8); (3) the Fast (Nano) model, if the user trains one. The spike records which of these is needed. The target is missed by every whole frame and needs the measures below; none of them is measured yet.
(Proposal section 7's "detector < 50 ms on a GPU at 640 px" is a per-tile figure; measured: ~30 ms.)

Nano at 640 is ~142 ms per tile on the CPU (about 20 % less), so it does not change the picture for whole frames.

What the design does to reach the target, or the gate on a slow CPU, in this order:

1. **Scope**: a locate is scoped to the target window by default (tests target an app), so the usual cost is the
   window's tiles, not the screen's: a 1200 × 800 logical window is 2 × 2 = 4 tiles (~0.7 s CPU, ~0.12 s GPU, projected;
   inside the gate, still over the target on its own).
2. **Changed tiles only**: tiles whose pixels did not change since the last frame reuse their cached detections.
3. **Priority order**: the focused window's and the pointer's tiles run first; a unique hit above the class threshold
   in the requested scope ends the locate early.
4. **Batching** tiles in one run on a GPU EP (fixed input, no NMS: DETR batches well); on the CPU, `threads(Cpu, User)`
   intra-op threads.
5. **A Fast (Nano) model**, if the user trained one for that app, on CPU-only runners, and **"Still working"** rather
   than a timeout for a full-screen pre-labelling job (rule 12). A locate is never stopped by a budget; it ends when it
   is done, when the judge finds the sidecar stuck, or when the **caller's step timeout** (the one cap) runs out, in
   which case it reports the scope it covered and fails as "not found in time", never a guess (AGENTS.md: never click
   blindly).

### Locate at run time: no budgets, a stall judgement and one cap

ADR 0031 (accepted) applies to every wait on `sighter-infer`:

- **Budgets are benchmark constants only.** Runtime code never reads `LOCATE_GATE_*`, `LOCATE_TARGET_*` or
  `MODEL_COLD_LOAD`.
- **Liveness is a Patience stall judgement** (`paddock-jobs::patience`, ADR 0031 sections 2 and 5), keyed per call kind
  (`CallKey("sighter.locate")`, `"sighter.load"`, `"sighter.selftest"`, `"sighter.prepare"`, `"sighter.train"`). **Tile
  progress is the call progress signal**: the sidecar sends a `Progress` frame when a tile (or a batch of tiles) has
  been inferred, from that work and never from a timer; the training worker sends one per optimiser step. While the
  sidecar is starved, waiting for I/O or paused, no tick counts; a stuck sidecar (blocked for N ticks, or C CPU-seconds
  without a finished tile) fails the call as a sidecar failure, and `paddock-proc` restarts it with backoff. C per key
  is measured in the spike (the largest CPU time between two progress frames on the synthetic corpus, × 4; ADR 0031
  section 5) and is a Setting with an "Auto" default (rule 51). Model load and the CoreML compile are one opaque ONNX
  Runtime call each, with no progress frame inside: they are judged on CPU time and run state only (as ADR 0031
  section 9 does for a peer without progress frames), so their C covers the longest measured compile (49 s, indicative)
  with the margin.
- **The only limit is a cap: the caller's step timeout**, which the client passes with the locate (the test step's
  `timeout`, a product rule the user sets). It is **counted in host ticks** (`HostTickClock`), so a paused host does not
  spend it, and when it fires it explains itself (ADR 0019): "Not found within the step's 10 s; searched the target
  window, 6 of 8 tiles." A locate without a caller timeout (the studio, an attended "Find") has no cap and shows "Still
  working" (rule 12) with Cancel.
- Jobs (import, pre-labelling, dataset assembly, training, preparation) have **no cap** at all: they are judged for
  stalls, show progress, and the user can cancel them (rule 12).


### Would segmenting the screen make it faster?

The user's question: "if we segment the screen into parts and analyse them, would it be faster?" Only if fewer pixels
reach the detector. Splitting the same pixels into parts does not save work. Costs are relative to the whole frame at
logical 100 % scale (1920 × 1200: 8 tiles, ~1.4 s CPU p50, ~0.24 s CoreML, projected from the measured per-tile times).

| Approach | Cost vs whole frame | What it can miss | Platform API |
|---|---|---|---|
| 1. Tile the whole frame | **1.0×** of compute (the tiles cover more pixels than the frame, from the overlap and the edge tiles: +42 % at 1920 × 1200, +67 % at 2560 × 1440, +38 % at 3840 × 2160, +25 % at 5120 × 2880 with 640 tiles); faster wall-clock only by running tiles in parallel on spare cores or batching them on a GPU (not measured) | Nothing, at the tile scale | none |
| 2. Only segments that changed since the last frame | **≈ changed tiles / all tiles**: 0 for a static screen; 1–3 of 8 (0.13–0.38×, ~0.2–0.5 s CPU) for a typical click that opens a menu or changes a few controls (projection) | Nothing if the change detection is right; misses a change the OS does not report (some GPU-drawn or remote-session surfaces) unless a per-tile pixel hash is the fallback; cache must be cleared on model change, theme or DPI change | macOS: `SCStreamFrameInfo.dirtyRects` (ScreenCaptureKit, macOS 12.3+): "a union of the rectangles redrawn and moved". Windows: Desktop Duplication `IDXGIOutputDuplication::GetFrameDirtyRects` and `GetFrameMoveRects` (Windows 8+; process move rects before dirty rects); otherwise a per-tile hash of the new frame (cheap, not measured) |
| 3. Active window, or the region around the focused window and pointer, first | Window 1200 × 800: 4 tiles, **0.5×**; pointer region: 1 tile, **0.13×** (~0.18 s CPU, ~30 ms GPU) | Elements outside the window: menus and pop-ups that open outside it, toasts, other windows of a multi-window app | Window bounds and focus from the capture or window APIs (`SCWindow` frames; `GetWindowRect`/DWM extended frame bounds on Windows; verify in the spike); pointer position |
| 4. Cheap first pass (OCR) picks segments, detector runs on those only | OCR pass + detector on k tiles; OCR is not cheap on a whole frame (proposal: 150–500 ms CPU for 1080p, unverified) | Icon-only and custom-drawn controls with no text, which is exactly where the detector is needed | OCR (`sighter-ocr`) |
| 5. Segments in priority order, results streamed | **1.0×** total if it runs to the end; **first result after one tile** (~0.18 s CPU, ~30 ms GPU); less if it stops early | With early stop: a better or second match elsewhere, so a query that needs uniqueness must finish its scope | none (scheduling in `sighter-core`) |

**Default pipeline:** normalise each display's frame to logical 100 % → limit to the query's scope (target window by
default) → split into 640 tiles with 64 px overlap → reuse cached detections for tiles the OS reports unchanged (dirty
rects, else a per-tile hash) → run the remaining tiles in priority order (focused window and pointer first, then the rest of the
scope), batched on a GPU EP → stop early only when a unique match above the threshold is found and the query allows it →
merge boxes across seams (pure function in `sighter-core`). OCR stays a resolver layer and a label source, not the gate
for the detector. **For a viewer window** (remote desktop or VM console): the scope is the viewer's content
rectangle mapped to remote pixels, changed tiles come from Sighter's own per-tile hash instead of OS dirty rects, the
priority goes to the pointer region and the most recently changed tiles (the remote "focused window" is not known from
the host) ("Remote desktops and VM consoles in a viewer window").

### Coordinates: one chain and one grid

Every target, a local window, a whole display or a viewer window of a remote desktop, uses **one coordinate chain** and
**one tile grid**. Symbols: `c` = the content rectangle's origin in the capture; `s` = viewer scale (capture pixels per
remote pixel; 1 for a local window or display); `r` = the target's scale factor (remote pixels per remote logical
point; for a local target, the display's scale `h`); `h` = the host display's scale factor; `o` = the captured window's
origin on the host.

- **Forward (frames)**: capture (host physical pixels) → crop the content rectangle at `c` → resample by `1 / (s · r)`
  → **remote logical points**. Tiling (640, 64 px overlap) and the change hashes use this **single grid**, so tiles do not
  move when a window moves and the model always sees the logical 100 % scale it was trained at.
- **Boxes**: tile coordinate + tile offset = remote logical; × `r` = remote pixels; × `s` + `c` = capture pixels. Each
  box carries its target and, for whole-display captures, its display id; a box never spans two displays.
- **Input**: capture pixels → on macOS ÷ `h` + `o` (window origin in global points) = the CGEvent point; on Windows,
  with the sidecar and the input helper **Per-Monitor V2 DPI aware** (they see raw pixels and per-monitor DPI;
  unaware or system-aware processes get virtualised coordinates; `GetDpiForMonitor`/`GetDpiForWindow`), capture pixels
  + `o` (window origin in physical virtual-screen pixels) = the `SendInput` point. The window origin is read just
  before acting.
- **Capture size on macOS**: ScreenCaptureKit writes into the size set in `SCStreamConfiguration` (`width`: "The width
  of the output") and scales the window into it ("always performs hardware scaling on the captured window so it never
  exceeds the frame output", WWDC22 session 10155); `SCStreamFrameInfo.scaleFactor` gives the window's scale per frame.
  Sighter sets the output size to the window size × `scaleFactor`, so a capture pixel is a host physical pixel; any
  other output size is an extra factor folded into `s`.
- **Adapter hints** enter the same chain backwards; see "Adapter hints: coordinate space" below.
- All of it is pure functions in `sighter-core`, with time and geometry passed in.
- **Test item** (`sighter-core`, pure, synthetic layouts): two and three displays with mixed scale (100 % 1920 × 1200
  left, 200 % 3840 × 2160 right, 150 % 2560 × 1440 above with a negative origin); a box on a tile seam; a box at each
  display edge; viewer cases with `r` = 1 and **`r` = 1.5 together with `s` < 1 (0.75 and 0.5) on a 200 % host**; round
  trips tile → capture → tile and remote → host input → remote within ±1 pixel of the finer grid; property tests over
  random layouts, scales and boxes. Plus one OS-level check per platform in the spike (a synthetic window at a known
  position on a 200 % display).

### Decisions for the pure core

Points the first `sighter-core` implementation needed and earlier revisions left open (coordinator, 2026-10-29):

1. **The tile grid's edges** (confirmed, part of "one grid for training and inference"): the last tile of a row or column
   is **pulled back so that it ends on the frame edge** (it overlaps its neighbour by more than 64); a frame side no larger
   than one tile gets **a single tile at 0, padded** to the tile size (padding colour: the model card's normalisation
   mean, so it reads as background). Dataset assembly cuts exactly these tiles, so training sees the same pulled-back
   edge tiles and padded small frames as inference.
2. **Seam merge**, precisely. Every detection carries the **tile it came from**. Two detections A and B of the same class
   are merged only when (a) they come from **different, neighbouring tiles**, (b) **both intersect the overlap strip**
   those two tiles share, and (c) **their parts inside that strip agree**: IoU(A ∩ strip, B ∩ strip) ≥ **0.5**
   (`SEAM_IOU`, a validated tunable in (0, 1]). Both tiles see the same pixels in the strip, so the two parts of one cut
   element cover the same strip region; two separate neighbours in the strip are each seen whole by both tiles and
   pair up with their own copies, not with each other (the duplicates are then removed by class-aware NMS). Two
   detections from the **same** tile are never merged. Merging is transitive (a box cut at a four-tile corner); the
   merged box is the union with the best score. Test: two separate same-class buttons 4 points apart inside a strip
   stay two; one wide text box cut by a seam becomes one; a box cut at a corner becomes one.
3. **HUD sidecar format** (the run's overlay record): **JSON Lines**, file suffix **`.hud.jsonl`**, first line a header
   `{"format":"sighter-hud","version":1,…}`. Limits: a line is at most **64 KiB**, a file at most **256 MiB** and
   **1,000,000** lines (named constants; the reader stops with a clear message at a limit). Versions: a **higher major
   version** is refused with "This HUD file was written by a newer Sighter"; unknown fields in a known version are
   ignored (new fields are optional and additive); a malformed line is skipped and counted, never a panic. The HUD
   holds boxes, classes, scores and labels, never pixels; labels of redacted regions are masked before they are written
   (redaction before every store).
4. **Confidence bands** (confirmed as defaults): **high ≥ 0.8** (the resolver's acting threshold τ) and **medium ≥ 0.5**,
   below that low; both are validated Settings (0 < medium < high ≤ 1). They are set from measurements once a model
   exists: spike item S8 compares them with each class's F1-optimal threshold from evaluation (proposal section 6) and
   the wrong-click rate, and may move the defaults or make them per class. They are **not the labelling
   thresholds**: the bands judge a calibrated detector score when a locate acts at run time, while the review queue's
   fusion score (proposal section 5) flags a label below 0.6 and auto-accepts it only at 0.85 or more with no
   disagreeing source; auto-accept is stricter than "high" because a wrong label is learned by every later model.
5. **Hint spaces and containment**: as in "Adapter hints: coordinate space" below (virtualised refused with its message;
   containment 50 %).
6. **`o`** (confirmed): the captured window's origin on the host, in the host input space (macOS global points; Windows
   physical virtual-screen pixels), read just before acting; for a whole-display capture, the display's origin.
   **Overlay items are generic over capture pixels and host input**: the HUD draws in host input space on Sighter's own
   overlay window, and burned-in run recordings draw in capture pixels; both come from the same items through the chain.

### Adapter hints: coordinate space

A hint (decision 9) is a rectangle from the user's own framework adapter. It is parsed at the server's edge into a typed
value that **names its space**; Sighter never guesses a space and never rescales a hint by a guessed factor.

| Hint space (wire tag) | Where it comes from | How it enters the chain |
|---|---|---|
| `host-input` on **Windows**: physical virtual-screen pixels | an adapter on the host whose reading thread is **Per-Monitor DPI aware** (v1 or V2). UI Automation returns physical coordinates (Microsoft, "Understanding Screen Scaling Issues": "The UI Automation API does not use logical coordinates", `CurrentBoundingRectangle` among the listed properties; clients that are not DPI aware "will not obtain correct results"). pywinauto 0.6.9 makes its process Per-Monitor aware at import (`win32functions.py`: `SetProcessDpiAwareness(Process_Per_Monitor_DPI_Aware)`, read in the wheel 2026-10-29). `Sighter.FlaUI` runs its reads under `SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)` (whether UI Automation honours the thread context rather than the process's is spike item S6) | it is a `HostInput` rectangle: the chain's inverse input step (− `o`, the window origin read for this query) gives capture pixels, then − `c` and ÷ (`s` · `r`) give logical points: `Chain::input_to_logical`, the same inverse that input uses |
| `host-input` on **macOS**: global points (top-left origin of the main display, the space of `CGEvent` and of the window list) | an adapter on the host | (point − `o`) × `h` = capture pixels, then as above: `Chain::input_to_logical` |
| `remote-pixels`: pixels of the remote desktop (origin at its top-left) | an adapter running **inside** the remote machine or VM (the remote desktop's own virtual screen, Per-Monitor aware as above) | ÷ `r` gives remote logical points (the inverse of `logical_to_remote`); only for viewer-window targets with a known remote resolution |
| `capture-pixels`, `logical` | rectangles taken from Sighter's **own earlier results** (a client narrowing a follow-up query to a box Sighter returned) | the chain's inverses (`capture_to_logical`), or as is |
| `virtualised` (a `system`-aware or unaware reader) | an adapter that is not Per-Monitor aware | **refused** (below) |

- The client states its DPI awareness with each hint (`per-monitor-v2`, `per-monitor`, `system`, `unaware`; `Sighter.FlaUI`
  reads it with `GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext())`); `system` and `unaware` make the
  hint `virtualised`. **A virtualised hint is refused** with a message that says what to fix ("The hint from <adapter>
  is in DPI-virtualised coordinates; make the adapter Per-Monitor V2 DPI aware"), because Windows virtualises such
  coordinates per monitor and no chain can undo that on mixed-DPI layouts; the locate itself goes on without the hint.
  A hint without a space tag is refused at the edge.
- After mapping, a hint is clipped to the query's scope and grown by a small margin (default 8 logical points, a
  tunable) because it can be stale; a hint that falls outside the target is dropped and recorded. Hints then only
  narrow and rank (decision 9): a detection **counts as inside a hint when at least 50 % of its area lies inside the
  grown hint** (`HINT_CONTAINMENT`, default 0.5, a validated Setting in (0, 1]), **or when at least that fraction of
  the hint's area lies inside the detection** (a hint smaller than its element, such as the inner text area of a
  field); detections that meet neither are not candidates for the hinted query. A hinted query with no detection inside is "not found", never the hint itself.
- **Test item** (pure, synthetic): a hint on each display of the mixed-scale layout above, in both host spaces and in
  `remote-pixels` with `r` = 1.5 and `s` = 0.5, maps to the logical rectangle of the element it names within ±1 logical
  point; an unaware or untagged hint is refused with its reason.

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
2. Lay the tile grid on the **remote logical grid** of "Coordinates: one chain and one grid", so tiles do not shift
   when the viewer window moves.
3. Hash a slightly quantised, downsampled copy of each tile (codec noise must not count as a change). A tile counts as
   changed when its hash differs, confirmed when the largest absolute difference of any single cell of the downsampled
   tile reaches a threshold; a mean over the whole tile must not be used, because it averages a small local change (a
   16-point checkbox in a 640 tile moves it by about 0.14 luma units) away to nothing. The cell size (the downsample
   grid) is chosen so that one cell is about the size of the smallest element to notice.
4. A changed tile is re-run only after it is stable for N frames (proposal 7.6's "stable frames"), so progressive
   refinement does not trigger repeated runs.

Hashing a frame is cheap compared with one detector tile (not measured; a spike number).

### Resolution and scaling

Three scales stack up: the **remote session's** resolution and scale factor (for example 1920 × 1200 at 100 %, or 4K at
150 %), the **viewer's** own scaling (1:1, fit-to-window, smart sizing, a VM console's zoom), and the **host display's**
scale factor (the viewer window's physical pixels on the host).

- **Mapping** uses the one chain of "Coordinates: one chain and one grid", with `s = content width / remote width`
  (and the same for height) and `r` = the remote session's scale factor.
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

If Sighter clicks or types, it acts on the viewer window, so the mapping runs backwards along the same chain: remote
logical × `r` × `s` + `c` = capture pixels, then the input step of "Coordinates" with the viewer window's current
origin. The viewer is brought to the front before input,
and Sighter re-checks the content rectangle just before acting. Where a viewer filters injected input, Sighter fails with
a message and a next step (proposal 7.6).

**Test item** (pure, synthetic): remote 1920 × 1200 at `r` = 1 and 3840 × 2160 at `r` = 1.5, shown 1:1, fit-to-window
at `s = 0.75` and `s = 0.5`, fullscreen, moved, on a 100 % and a 200 % host display, and spanning two host displays; a
click on a known remote pixel must land within ±1 remote pixel after the round trip remote → host → remote, and a box
must map host → remote → host within ±1 host pixel (part of the test item of "Coordinates").

### Viewer window changes

| Change | What Sighter does |
|---|---|
| Moved | Window capture does not depend on position; input uses the window's position read just before acting; the tile cache stays (tiles are in remote pixels) |
| Resized | Re-derive the content rectangle and `s`; if `s` changed, drop the tile cache and warn if `s < 1` |
| Fullscreen (or back) | Chrome disappears or returns: re-derive the content rectangle and `s`; input coordinates follow the new geometry |
| Spans several monitors | One window capture gives one image; the window has one DPI on Windows (the monitor it is mostly on) and the mapping uses the window's geometry, so monitor borders do not matter; mixed-DPI tests cover it |
| Minimised or hidden | macOS: "when the source window is minimized, the stream output is paused" (WWDC22 session 10155). Windows: "Minimized windows are enumerated but not captured" (Microsoft's Windows.Graphics.Capture WPF and Win32 samples). Some viewers also stop rendering (proposal 7.15). Sighter pauses with "The remote desktop window is minimised; restore it to continue" and never acts on a stale frame |
| Covered by another window | macOS: "When the source window is occluded or partially occluded, the stream output always includes the window's full content", also off-screen (WWDC22 10155). Windows: not documented; spike item. Before input the viewer is brought to the front either way |

## Training from test-run videos

The main way an app gets its model (user, 2026-10-29; proposal section 6.1):

1. **Import**: a test-run video of a viewer window (or a local window) is imported as a `paddock-jobs` job at
   `Background`, built as the rule-14 pipeline of "Parallelism and cost": **decode → de-duplicate → redact → infer →
   store**, each a stage with its own progress on its own scale and Cancel (ER-17 to ER-19). Frames come through
   `paddock-video` (the shared FFmpeg runtime). **Near-duplicate frames are dropped**: a perceptual hash per frame plus
   a largest-cell-difference threshold (never a whole-frame mean), both tolerant of codec noise (the same quantised hashing as the viewer-window
   tiles). **Redaction runs before the store stage** ("Password fields and secrets, from pixels"): only redacted frames
   are written, and **the user's source video is never copied into the project**; the project keeps its path, size and
   SHA-256 (to find it again and to detect a changed file), and the frames. The infer stage runs only when the user
   chose "Label with computer vision while importing" (it adds OCR, templates and the app's own detector, no VLM);
   otherwise it passes frames through. The frame count after de-duplication is shown.
2. **Auto-label**: boxes come from **computer vision only**: OCR (text and text-bearing controls), templates (crops
   the user marked), and the app's own detector once one exists, either in the import pipeline or later over stored
   frames (load → infer → store labels). **Pitwall's vision model never places boxes**; when Pitwall is present and the
   user agrees, it **names** the boxed regions and **suggests a class** for each from numbered crop sheets (proposal 9.1
   kind 1, "names and classes for existing boxes"), after a **confirmation that states the frame count and the expected
   time** (rule 46). That keeps the pipeline pixels-only (decision 9): the VLM reads pixels too, and only labels what
   computer vision found. Without Pitwall, names come from OCR text.
3. **Review queue**: the most uncertain labels first (low detector score, disagreement between sources, a VLM class
   that differs from the detector's), with one-key accept and quick fix (proposal section 5).
4. **Train**: RF-DETR **Small** as the app's own model, **on this computer only**: MPS on Apple Silicon (CoreML is for
   inference), the opt-in CUDA runtime on NVIDIA (licence exception), else the CPU. Dataset assembly per "Training
   geometry matches inference", augmentation per "Augmentation and synthetic data". **No remote training in v1**
   (below).
5. **After training**: later videos of the same app are pre-labelled by the app's own model; the VLM is used only to
   name and classify boxes on **new, unknown screens** (a screen whose fingerprint matches no reviewed screen and where
   the detector is unsure), again after a confirmation with the count.

**Remote training: not in v1** (coordinator's decision of 2026-10-29, following "local first, opt-in online": the user
approved local training with an opt-in CUDA runtime only). Proposal section 6's "user-chosen remote host over SSH" is
removed from v1. The export-only path stays: the user may take an exported (redacted) dataset to another machine
themselves; Sighter sends nothing. **Possible future work**, only through its own ADR: training on a host the user names, opt-in through `paddock-net` `Egress`, off
by default and blocked by Offline mode, sending **redacted frames only**, with a per-job host allow-list and a
confirmation that shows the host and the frame count before anything is sent.

**Spike item**: label quality (precision and recall of auto-labels against a hand-reviewed set, per class) and time per
frame (import, auto-label, review) on a recorded test video of a synthetic test app (synthetic data, never real user
recordings: AGENTS.md).

## Training geometry matches inference

The detector only ever sees 640 tiles of frames normalised to logical 100 % ("Screens, tiles and coordinates"), so the
training data is cut the same way. Dataset assembly (Rust, `sighter-core` and `sighter-frames`) does, per reviewed
frame:

1. **Normalise to logical 100 %** with the frame's recorded chain (the display scale `h` from the recorder's session
   metadata; for imported videos the scale entered or confirmed at import, and `s` and `r` for viewer windows), with
   the same resampling function inference uses. Boxes map through the same chain.
2. **Cut the inference grid**: every tile of `tile_grid(width, height, 640, 64)`, exactly as at run time.
3. **Add random-offset crops**: k extra 640 crops per frame (default 2, a dataset preset value) at uniformly random
   offsets inside the frame, so elements are also seen away from the grid's fixed seams and edges. These replace the
   2 × 2 mosaic of earlier revisions: a mosaic puts four frames' edges in the middle of a tile, a layout the detector
   never sees at run time.
4. **Clip boxes** to the tile or crop. **Visible-fraction rule**: a clipped box is kept when at least **40 %** of its
   area is visible, or when its visible part is at least **64 logical points** along the cut side (wide elements cut by
   a seam, which the seam merge joins at run time); otherwise it is dropped and the area it covered in the tile is
   left as it is (DETR has no ignore regions; the spike checks whether filling it with background helps). Both numbers
   are dataset preset values recorded in the model card.
5. **Scale jitter stays narrow around 1.0**: 0.9 to 1.1 (per sample, applied to the 640 tile with its boxes, then
   padded or cropped back to 640), to cover rounding and font-rendering differences. `rfdetr`'s `multi_scale` is
   `off` and `expanded_scales` unused, because inference never sees another scale.
6. **HiDPI is simulated, not scaled**: frames recorded or rendered at 125 %, 150 %, 175 % or 200 % (the recorder's
   variation plan, proposal section 6 item 1) are resampled by 1/`h` to logical 100 % in step 1, exactly as at run
   time, so the model learns the resampling artefacts it will meet (fractional factors such as 1/1.5 blur edges
   differently from 1/2). Where only 100 % recordings exist, dataset assembly can render a 150 % or 200 % copy by
   upsampling and resampling back, which is an approximation (real HiDPI rendering has more detail) and is marked as
   such in the dataset.
7. **Viewer scale** (`s` < 1) is simulated the same way: downsample by `s`, then resample by 1/`s`, as the chain does.

This settles the old contradiction between "display scale 100–200 %" (this ADR) and "scale 50 to 300 percent"
(proposal section 6): **there is no geometric scale augmentation beyond 0.9–1.1**; display scales from 100 % to 300 %
are supported by normalising (at run time and in training), and HiDPI artefacts come from simulation. Proposal section
6 is amended to match.

## Augmentation and synthetic data

What the training runtime applies, with the RF-DETR or Albumentations option where one exists (read in the `rfdetr`
repository at `f381551` and on PyPI, 2026-10-29). RF-DETR's default training stack is "resize/crop scale jitter plus
horizontal flip at 50%" (`docs/learn/train/augmentations.md`); custom stacks are passed as `aug_config` (Albumentations
transform names), with `multi_scale` (`per-batch`, `per-sample`, `off`), `expanded_scales` and `scale_jitter` for scale.
RF-DETR has **no built-in copy-paste or class balancing**; those are Sighter's. Albumentations transform names below are
to be checked against the pinned version.

| Group | Technique | How (RF-DETR option or equivalent) | Notes |
|---|---|---|---|
| Photometric | Themes and accents, brightness, contrast, gamma, blur, noise | `aug_config`: `RandomBrightnessContrast`, `RandomGamma`, `HueSaturationValue` (accent shifts), `GaussianBlur`, `GaussNoise` | Real themes (light, dark, high contrast) come from recording, not from colour jitter |
| Remote artefacts | The **remote-codec preset**: RDP/VNC-style JPEG, H.264/AVC, 16-bit colour, chroma blur, smart-sizing resampling | JPEG: `ImageCompression`; resampling: `Downscale`; 16-bit colour (RGB565) and chroma blur: small custom transforms; H.264/AVC: rendered offline through `paddock-video`'s FFmpeg at dataset export | Best: frames recorded through the same viewer and codec |
| Geometric | Scale jitter 0.9–1.1, small shifts, random-offset crops, HiDPI and viewer-scale simulation, window sizes | `scale_jitter` narrow, `multi_scale=off`; `Affine` with small translation; crops and simulation in dataset assembly ("Training geometry matches inference"); window sizes from recording | **No flips or rotations**: UI layout and text are not mirror- or rotation-symmetric (a flipped "Save" button or a right-to-left toolbar would teach the model shapes that never occur). RF-DETR's default flip is turned off by passing Sighter's own `aug_config` without `HorizontalFlip` |
| Composition | Copy-paste of controls onto other screens and backgrounds; occlusion by cursor, tooltips, dialogs | Sighter's dataset assembly (offline), **on the tile grid and random-offset crops** above: paste reviewed crops (already at logical 100 %) with their boxes; overlay cursor sprites, tooltip and dialog crops (occluded boxes kept or dropped by the visible-fraction rule) | Exact labels for free; pasted elements keep scale 1.0 (± the jitter) |
| Redaction | The password fill applied at random to ordinary text inputs | Dataset assembly ("Password fields and secrets, from pixels") | So that a filled field does not mean "password" to the model |
| Text | Re-render label text with other strings, lengths, fonts and languages | Offline: fill the text region with its background colour and draw new text (system fonts on the training machine) | Experiment: only kept if the held-out set shows a gain |
| Label economy | Label propagation by tracking across video frames | Track each reviewed box into neighbouring frames (template match in a small search window); propagated boxes enter the review queue as "proposed" | Pure Rust in `sighter-core` |
| | Active learning | Review queue orders frames by uncertainty (score margin, source disagreement) | `sighter-core` |
| | Hard-negative mining | Regions the user rejected (false positives) are kept as background in their frames; frames with many rejections are sampled more | `sighter-core` dataset assembly |
| | Class balancing | Repeat-factor sampling of frames that contain rare classes | `sighter-core` dataset assembly (no RF-DETR option) |
| Synthetic screens | Run the app under test with varied themes, fonts, window sizes, scales and data, and record it | Sighter's recorder with a variation plan (proposal section 6, item 1: "real variation captured by the recorder") | The most realistic "augmentation"; the studio shows coverage gaps |

**Honest measurement.** A **held-out set of real, unaugmented frames** is never augmented and never used for training.
It is split **by recording session and by screen fingerprint**: no frame of a held-out session is in training, and no
screen whose fingerprint (proposal section 5, the screen-state fingerprint) appears in the held-out set appears in
training from any session, so the score measures new screens and not memorised ones. Each technique (or group) is
switched **on and off** against it, with everything else fixed, and kept only if it raises per-class mAP@0.5:0.95 or
lowers the wrong-click rate. **Spike item**: record which techniques actually raise accuracy, as results blocks in
[performance.md](../performance.md) (rule 48).

**Where the code goes.** Online augmentation (photometric, JPEG, resampling, the narrow scale jitter, small shifts) runs
in the **Python training runtime**, as an `aug_config` built from a Sighter preset and passed to `rfdetr` with
`augmentation_backend="albumentations"`. Pinned library: Albumentations **2.0.8 (MIT)**, installed directly at that
version rather than through the `rfdetr[augment]` extra, because the extra also pulls in Kornia (`kornia<1,>=0.7`, in
the 1.11.2 wheel's metadata), which Sighter does not use: **Kornia is not in the runtime**. Kornia is `rfdetr`'s GPU
augmentation backend; it becomes a candidate only if the spike shows training bound by CPU-side augmentation (then it
is added with its own entry, licence Apache-2.0, and its behaviour pinned: the research note records a behaviour change
in an unreleased version). **Not AlbumentationsX**: the maintained successor on PyPI is AGPL-3.0 (2.5.1, 2026-10-11)
and is excluded; the MIT Albumentations' last release is from 2025-05-27, so the runtime pins it and its licence is
checked on every bump. Offline steps (de-duplication, normalising, grid tiles and crops, copy-paste, occlusion, the
redaction fill, text re-rendering, H.264 rendering, tracking, active learning, hard negatives, class balancing) are Rust
in `sighter-core` and `sighter-frames`, which write a COCO dataset of 640 tiles for the runtime. Shared pieces:
`paddock-video` (decode, FFmpeg runtime, codec rendering), `paddock-jobs` (import, auto-label and training jobs),
`paddock-runtime` (the Python runtime), `paddock-pitwall` (naming requests). The near-duplicate and tile hashing is one
implementation in `sighter-core` for both uses.

## Password fields and secrets, from pixels

Without accessibility (decision 9) there is no `IsPassword` flag, so the recorder's "pause around password fields"
(AGENTS.md, proposal section 4) needs a pixels-only mechanism.

**When and how.** Every frame is **classified in memory at capture time** (the recorder) or at decode time (video
import), **before anything is written or sent**. A region classified as a password field, or as a text input that is
not yet known to be safe, gets a **solid fill**: one opaque colour (the field's own background colour, sampled at its
inner border, so the field's outline stays visible and the class stays learnable), drawn over the field's inner text
area. **Never a blur**: blurred text can be partly recovered, and short secrets such as PINs fall to guessing. **The
fill is permanent**: the stored frame has no unfilled copy, and review cannot undo it. Each stored frame carries a
**`redacted` flag** and the list of filled rectangles, so the studio shows them, evaluation knows them, and dataset
assembly can treat them.

**Classifiers**, combined (any one marks a field):

1. **Masked-glyph check** (works before any model exists): inside text-box-like regions (a rectangle with a text
   baseline, found by the OCR text detector), a run of three or more identical, evenly spaced dot or asterisk glyphs
   (•, ●, ∗, *) marks a password field.
2. **OCR label cue**: OCR text in or next to the field (left of it, or just above it) that matches a password label
   from a localised knowledge list ("Password", "Passcode", "PIN", "Passwort", "Kennwort", "Mot de passe",
   "Contraseña", "Senha", "Пароль", "パスワード", "密码", "비밀번호" and others), shipped as a TOML pack and extendable by
   the user (rule 21; a pack of the "secrets" kind). It catches an **empty** password field and fields with custom masks,
   which the glyph check misses.
3. **Detector class** `password_field` in every app's taxonomy, trained from reviewed frames like any other class.
4. **Text inputs filled by default until classified**: every region the detector or the OCR heuristic sees as a text
   input, and that the detector does not classify with high confidence as not a password field, is filled.
5. **User hotkey to pause** recording at once (and to resume), shown in the recorder's overlay.
6. Typed key text is not stored by default (AGENTS.md), so keystrokes, the main leak, are covered independently.

**Training with filled fields.** Dataset assembly applies **the same fill to ordinary text inputs** (boxes keep their
class), so a filled field does not by itself mean `password_field`. It aims at a **target filled fraction**, not a
fixed probability: counting the fills already made at capture (fill by default leaves many ordinary inputs filled), it
fills further ordinary text inputs, chosen at random, only until ordinary inputs make up at least the target share of
all filled fields in the dataset (default 0.5, a dataset preset value recorded in the model card), and never fills
more than needed. For a dataset with no capture-time fills it fills that share of a random sample of inputs. For `password_field` boxes it draws **synthetic mask dots** (a random count of • glyphs in the field's
text colour) on the fill, because at run time the detector sees live password fields with their dots, never the fill.
Neither step can leak a secret: the fill was applied at capture time, and the dots are synthetic.

**Redaction before every store or send.** One function in `sighter-core` (pure: frame, classifier outputs → filled
frame plus rectangles) is called on every path where pixels leave memory:

| Path | Where redaction runs |
|---|---|
| Recorder | at capture, before the frame is encoded or written; **on overload** (the classifier queue full) the recorder drops new frames, counts them and shows "Recording at N fps, frames skipped"; it **never writes an unclassified frame** |
| Video import | in the pipeline's redact stage, before the store stage; the user's source video is never copied into the project unredacted (only its path, size and hash are kept) |
| Test runs | `run.mp4` (the run recording) and the step and failure screenshots of run reports, before encoding |
| Exports | Debrief bundles and any report export (they are built from already redacted run artefacts, and the export re-checks the `redacted` flag) |
| Diagnostics | diagnostics bundles and logs never include unredacted frames; screenshots in them are taken from redacted artefacts |
| Pitwall | crop sheets and frames sent for naming, descriptions and grounding are cut from redacted frames; a crop that overlaps a filled rectangle is sent filled |

A test per path (synthetic login screen, a known secret string rendered into a password field) asserts the bytes
written or sent contain no unfilled pixel of that field.

**Limits:** a field that shows plain text ("show password" toggles), custom masks without a label cue, secrets in
ordinary fields (API keys, one-time codes), a mask that briefly shows the last typed character, and very small or
heavily compressed fields (remote sessions) can be missed; the fill-by-default rule, the label cue and the hotkey cover
part of that, not all. **Spike item**: recall of the masked-glyph check, the label cue and the `password_field` class
on synthetic login screens, local and RDP-compressed, at 100 % and 200 %.

## Parallelism and cost (rule 51)

Every bulk operation, with (a) its threads and workers and (b) how it is made cheaper. **Thread counts come from
`paddock_proc::threads(kind, class)`** (ADR 0024: cores by kind, never logical CPUs; `User` CPU work gets performance
cores − 1, at least 1; `Background` gets the efficiency-core count on Apple silicon, physical cores / 2 on uniform
CPUs), capped by the app's Performance setting, and each is a **Setting with an "Auto" default**, validated in the
engine (rule 51): `recorder.classify_workers`, `recorder.queue_memory`, `import.decode_threads`, `import.workers`,
`import.store_writers`, `infer.cpu_threads`, `dataset.workers`, `dataset.store_writers`, `training.data_workers`,
`pipeline.queue_memory`. Results are deterministic: work is numbered at the source and
committed in that order (rule 14).

| Operation | Class | (a) Parallelism | (b) Cheaper | Cost estimate |
|---|---|---|---|---|
| **Recorder** (capture-time classification and redaction) | `User` (the user is recording) | Capture → **bounded in-memory queue, sized in bytes** (`recorder.queue_memory`, Auto: 256 MB, with the 2-frame floor of the import pipeline below) → **classifier workers** (`recorder.classify_workers`, Auto `threads(Cpu, User)`; the masked-glyph check, the OCR label cue and the detector's `password_field` class, the last two through `sighter-infer` calls of class `User`) → fill → encode and write | **Reuse the per-tile hash**: a tile whose hash did not change since the last classified frame keeps its earlier classification and fill rectangles, and only changed tiles are classified again (a fill rectangle is reused only when every tile it touches is unchanged); the recorder's own change trigger (proposal section 4) already skips identical frames | **Spike item S3**: classification cost per frame and per changed tile, and the sustained frames per second at 1920 × 1200 and 4K on CoreML and CPU |
| **Video import** | `Background` | The rule-14 pipeline below: FFmpeg decode with `threads(Cpu, Background)` threads; de-duplication hashing and redaction on `threads(Cpu, Background)` workers; store on `import.store_writers` writers (Auto 4: SSD I/O, rule 13) | Drop near-duplicates **inside FFmpeg** with `mpdecimate` ("Drop frames that do not differ greatly from the previous frame", FFmpeg filter docs) or a `select` on the scene score, so most frames never cross into Sighter, are never converted or hashed; `mpdecimate` still decodes every frame, so the only step before decode is `-skip_frame nokey` (keyframes only), which loses frames and is a spike comparison, not a default; decode at the size the chain needs, not larger | **Spike item S3**: frames per second per stage on a synthetic 10-minute 1920 × 1200 test-run video |
| **Auto-label** (CV) | `Background` | Tiles of several frames batched on the GPU in `sighter-infer`'s queue (behind locates); OCR in the same sidecar; one frame per worker for the CPU stages | **Reuse OCR detections**: one OCR pass per frame hash serves the password classifiers, the OCR label source, names and the OCR-anchor layer; unchanged tiles of consecutive frames reuse the cached detections (the per-tile hash); fp16 and IO binding below | Detector only, projected from the indicative per-tile times: 8 tiles per 1920 × 1200 frame × ~30 ms (CoreML) ≈ 0.24 s per frame, about **8 minutes for 2,000 frames**; on 4 CPU threads ~1.4 s per frame, about **47 minutes**; OCR is extra and unmeasured. Projections, to be measured (S3) |
| **Dataset assembly** | `User` (a phase of the user-started training job) | `threads(Cpu, User)` workers, one frame each (normalise, grid tiles, crops, clipping, fill, composition); encode and write on `dataset.store_writers` writers (Auto 4) | Tiles cut once per dataset snapshot and reused across training runs with the same preset (content-hashed); lossless WebP or PNG chosen by the spike's encode cost | ~10 tiles and crops per frame, so 2,000 frames give ~20,000 training images; time **spike item S3** |
| **Training** | `User` | One GPU process; data-loader workers `threads(Cpu, User)`, capped by RAM (each worker holds decoded batches); batch size from free GPU memory | Train at 640 only (no multi-scale), fixed tile size; early stopping; fine-tune from the COCO checkpoint; fp16/AMP where the backend supports it (CUDA; MPS to be checked) | **Spike item S4**: minutes per epoch for RF-DETR Small on 20,000 tiles, on the M4 Pro (MPS), on an NVIDIA laptop (CUDA) and on the CPU; nothing is quoted until measured |
| **CoreML preparation** | `Background` (separate `sighter-infer --prepare` process) | One compile per model; never two at once (a compile is memory-heavy) | Run once per model hash and OS version; the cache is kept with the model; prepare right after training so the first locate is warm | 23–49 s per model (indicative, at load 18–24, [performance.md](../performance.md#2026-10-29-rf-detr-onnx-latency-indicative-docssighter-rfdetr)) |

**The import pipeline (rule 14).** Producers → bounded queue → workers, per stage:

```text
decode (FFmpeg, mpdecimate) → [queue] → de-duplicate → [queue] → redact → [queue] → infer (optional) → [queue] → store
```

- **Bounded queues**, sized in bytes, not frames (a 1920 × 1200 RGBA frame is about 9.2 MB, a 3840 × 2160 one about
  33 MB, a 5120 × 2880 one about 59 MB): together at most `pipeline.queue_memory` (Auto: 512 MB, or less on machines
  with little RAM), split over the stages. **Floor**: every queue holds at least **2 frames at the largest frame size
  of the job**; when Auto (or a user value) is below that, it is **raised automatically** to the floor and the job's
  details say so. A full queue blocks its producer (back-pressure); nothing is dropped.
- **Stage-specific workers**: decode is FFmpeg's own threads; de-duplicate hashes on workers and **compares in order**
  at a single commit point (a frame is compared with the last kept frame, so this step is sequential but cheap);
  redact and infer on workers; store on `import.store_writers` writers (Auto 4).
- **Cancel** reaches every stage; frames already stored stay consistent (the job writes into a new import folder and
  swaps it in at the end, ER-18). **Progress** per stage on its own scale (decoded frames count up, kept frames,
  redacted, labelled, stored).
- **Determinism**: frames are numbered by their decode timestamp; the store commits in that order, so the same video
  gives the same frame ids.

**Sharing the GPU between locate and pre-labelling.** `sighter-infer` has **one GPU queue** per execution provider,
ordered by the class each call carries: **locates first**, background work (pre-labelling, evaluation) after. The unit of
work is **a tile, or a small batch of tiles**; a waiting locate goes next **between tiles**, so it waits at most for one
batch already running (one tile is ~30 ms on CoreML; the background batch size is capped so a batch stays within a few
tiles). Training runs in its own process on the same GPU and cannot be pre-empted per tile; while it runs, Sighter says
"Training is using the graphics card; locating may be slower", and the spike measures locate p95 during training
(S4).

**Concurrent locates** (several targets or clients at once; the design review's "8 targets"): each locate is split into
tiles in its own priority order; the queue serves locates **round-robin by tile**, so one full-screen locate cannot
starve a small one, and on a GPU it **batches tiles of different locates together**. On the CPU there is one inference
run at a time with `threads(Cpu, User)` intra-op threads, never a thread pool per locate (that would oversubscribe the
cores). Two locates on the same frame hash and scope share one run (single-flight). The number of locates in flight is
bounded (Auto: 8, a Setting); further locates wait for admission, which counts against their caller's step timeout like
any other wait. Each locate's cap counts host ticks from its own start.

**Cheaper options to try** (rule 51 b; each measured in the spike and kept only with a recorded gain):

- **FFmpeg `mpdecimate` or a scene `select`** before frames leave FFmpeg (above), against Sighter's own hash alone.
- **fp16**: an fp16 ONNX export for DirectML and the GPU; CoreML `MLProgram` can compute in fp16 on the GPU. Kept only
  if the GPU self-test (decision 4) still matches the CPU answer within tolerance; the CPU stays fp32 (the determinism
  reference).
- **IO binding** (ONNX Runtime `IoBinding`): preallocated input and output buffers for a batch of tiles, filled in place
  by normalise and tile, so no tensor is allocated or copied per call.
- **Reusing OCR detections** across the password check, the OCR label source and the locate's OCR-anchor layer (one
  pass per frame hash), and reusing tile detections across frames whose tiles did not change.

## Priority classes and the C40 instance list (ADR 0024)

Sighter's child-process launch sites, each through `paddock_proc::command(...).class(...)` (C40; Sighter is born
compliant: the checks are errors for it from its first code, ADR 0024 rollout step 3):

| Launch site (planned) | What it runs | Class | Threads |
|---|---|---|---|
| `sighter-infer` (server start) | inference sidecar: locate, OCR, pre-labelling, evaluation, the GPU self-test | **`User`**; the class travels per call (locate `User`, pre-labelling and evaluation `Background` threads inside) | intra-op `threads(Cpu, User)` |
| `sighter-infer --prepare` | CoreML compile of one model into its cache | `Background` | `threads(Cpu, Background)` |
| Capture helper (proposal section 4, out of process) | screen and window capture for locate and recording | `User` (serves interactive locates) | 1–2 |
| Input helper (Windows, Per-Monitor V2) | `SendInput` for `act` | `User` | 1 |
| FFmpeg through `paddock-video` (`ffprobe`, decode) | video import | `Background` | `threads(Cpu, Background)` (FFmpeg `-threads`) |
| FFmpeg through `paddock-video` (encode) | H.264 rendering for the remote-codec preset (dataset assembly) | `User` (a phase of training) | `threads(Cpu, User)` |
| Training worker (managed Python runtime, `paddock-runtime`) | `rfdetr` training | `User` while the user started it (all of v1) | data-loader workers `threads(Cpu, User)` |
| Training runtime self-test (after download or update) | one step on a generated mini dataset | `Background` | `threads(Cpu, Background)` |

**Amendment note for the integrator (ADR 0024, section 1, "Instances today").** The row "Pitcrew, Spotter, Sighter,
Pacenotes – no priority set on children" gets Sighter's sites from the table above (Sighter has no code yet; the sites
are planned and are checked by C40 from the first commit that adds them).

## Amendment note for ADR 0031, section 7 (for the integrator)

Rows to add to ADR 0031's instance table (Sighter has no code yet; these are the planned bounds, so C51 finds them
classified from the first commit):

| Where | Bound | Class | Note |
|---|---|---|---|
| Sighter server → `sighter-infer` locate call | a locate | L; C | judged on tile progress (`Progress` per finished tile or batch); the caller's step timeout is the only cap, on host ticks |
| Sighter `sighter-infer` model load (session creation) | model load | L | one opaque ONNX Runtime call: CPU time and run state only; C from the measured load |
| Sighter `sighter-infer` GPU self-test | start-up check of the user's model | L | progress per reference tile; a stuck self-test means "use the CPU" with the reason shown |
| Sighter `sighter-infer --prepare` (CoreML preparation job) | CoreML compile | L | opaque compile: CPU time and run state; the job itself has no cap (rule 12) |
| Sighter training worker (Python runtime) | a training job | L | progress per optimiser step from the worker's callback; no cap (rule 12), Cancel |
| Sighter → Pitwall naming, descriptions, grounding (`paddock-pitwall`, loopback HTTP) | a VLM request | X | Pitwall is another app's service: its own timeouts, Retry, never counts toward a quarantine; long batches are a job with progress per item |
| Sighter capture and input helpers | start and handshake | L | through `paddock-ipc`'s handshake (already a row) |

## Spike items

Collected from this ADR; each result is a rule-48 block in [performance.md](../performance.md).

- **S1. Gate on CoreML and CPU** under "Gate conditions per execution provider": the locate gate at both sizes, cold
  load, prepare time, the CPU-gate plan's steps, recall at tile seams.
- **S2. DirectML, not yet measured at all.** On a physical Windows 11 PC with an Intel integrated GPU and on one with an
  NVIDIA discrete GPU: the locate gate and cold load under the same conditions; the GPU self-test against the CPU answer;
  whether the `windows-latest` VM runner exposes any DirectX 12 adapter; together with the DirectML licence gate (P6)
  and the delay-load check of decision 4. Until S2 runs, DirectML is "supported, unmeasured", and **a Windows release
  of Sighter waits for S2**, unless the user waives the gate for that release; a waiver is recorded (date, release,
  reason) in this ADR's notes and the release notes. Which PC to measure on is the user's to name.
- **S3. Bulk cost**: import stage rates (decode, `mpdecimate` and scene `select` against Sighter's hash alone,
  `-skip_frame nokey`), auto-label per frame on CoreML and CPU, dataset assembly time and encode format, and the
  **recorder's capture-time classification**: cost per frame and per changed tile, sustained fps before frames are
  skipped, at 1920 × 1200 and 4K, on CoreML and CPU.
- **S4. Training cost**: minutes per epoch for Small on 20,000 tiles (MPS, CUDA, CPU), memory, and locate p95 while
  training runs.
- **S5. Cheaper inference**: fp16 (self-test still matching), IO binding, batch size per EP, against the fp32 baseline.
- **S6. Adapter hints**: whether UI Automation honours a Per-Monitor V2 **thread** context in a process that is not
  DPI aware (the `Sighter.FlaUI` case); a synthetic window on a 150 % monitor beside a 100 % one.
- **S7.** The items named in the text: auto-label quality, augmentation gains, password recall, compressed-frame
  accuracy, occlusion on Windows, hashing cost, seam recall with `SEAM_IOU`; and a **synthetic evaluation set with
  live (unfilled) text inputs and password fields with their dots**, because training frames carry fills the
  detector never sees at run time: `text_input` and `password_field` recall on it, for each target filled fraction.
- **S8. Confidence bands**: per-class F1-optimal thresholds and wrong-click rate against the 0.8 and 0.5 defaults.

## Licence exceptions

Sighter's code and every bundled or downloaded component are permissive (Sighter `deny.toml`), with exactly two named
exceptions, one shipped (conditional) and one downloaded on request:

| File | Licence | Scope | Reason | Approved |
|---|---|---|---|---|
| `DirectML.dll` 1.15.4 (NuGet `Microsoft.AI.DirectML`, SHA-256-pinned) | Microsoft Software License Terms, Microsoft DirectX Machine Learning (DirectML): proprietary; distribution allowed only inside an application, for Windows and Xbox, not stand-alone; may collect data | Windows only, shipped next to `sighter-infer.exe`; never another file of that package, never another app. **Condition: the DirectML gate passes** (no outbound connection, and no ETW/TraceLogging provider or an inert one; decision 4, plan item P6); otherwise it is not shipped and Sighter uses the system `DirectML.dll` with a delay-loaded import (or a CPU-only build) or the CPU | DirectML is how the ONNX Runtime build reaches any DirectX 12 GPU (NVIDIA, AMD, Intel) on Windows; the build imports it | user, 2026-10-29 (conditional) |
| NVIDIA CUDA runtime libraries (CUDA runtime, cuBLAS, cuDNN and the other NVIDIA libraries the pinned PyTorch CUDA wheels depend on) | NVIDIA CUDA EULA (v13.4) and the NVIDIA SDK licence with its cuDNN supplement (v. 2022-02-22): proprietary; runtime `.so`/`.dll` files distributable only "as incorporated in object code format into a software application" with "material additional functionality", "only accessed by your application" | **Training only**: the opt-in CUDA variant of the managed Python training runtime, **downloaded on request from the upstream hosts** (`download.pytorch.org` and `download-r2.pytorch.org` for the PyTorch CUDA wheels (about **1.9 GB** for the Windows wheel, `torch-2.14.1+cu130` cp312 checked at 1,990,608,063 bytes; about **0.55 GB** for the Linux x86-64 wheel plus the `nvidia-*` wheels, cuDNN alone about 0.5 GB); `pypi.org` and `files.pythonhosted.org` for the `nvidia-*` wheels they depend on, Linux), **never mirrored on our Gitea** (a stand-alone copy of NVIDIA's files would fall outside EULA 1.1.1's grant); never in the installer, never used by inference; NVIDIA's terms shown before the download; those hosts are on the `Egress` allow-list for this download job only | PyTorch has no other accelerator for NVIDIA GPUs; without it NVIDIA laptops train on the CPU (no remote training in v1) | user, 2026-10-29 |

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
  sections 6.1 and 6.2 added, on 2026-10-29. Revision 4 applied now: section 4 (redaction by solid fill, imports
  without VLM boxes), section 6 (scale augmentation 0.9–1.1 with HiDPI simulation; no remote SSH training in v1),
  section 9.1 kind 1 ("names and classes for existing boxes"; no Florence-2 boxes) and section 10 (no training host).
- **P7. Integrator's amendments** to family ADRs: Sighter's launch sites into ADR 0024's instance list ("Priority
  classes and the C40 instance list") and Sighter's rows into ADR 0031 section 7 ("Amendment note for ADR 0031").
- **P8. `sighter-bench`**: the `locate` benchmark of "Gate conditions per execution provider", emitting the rule-48
  block, and the nightly workflow once Sighter has CI.
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
  boxes" idea are not v1, and 9.1's first request kind is "names and classes for existing boxes" (decision 5; applied
  on 2026-10-29 and aligned in revision 4).
- No remote training in v1: a computer without a usable GPU trains on the CPU (slowly) or the user takes an exported
  dataset elsewhere; the possible future path is described in "Training from test-run videos".
- Redaction is a solid, permanent fill: stored frames can never be un-redacted, so a field filled by mistake stays
  filled in that recording (the price of filling by default).
- A new app starts with no detector: until the user has reviewed enough frames to train the first model, labelling is
  OCR, templates and manual review. This is the price of leaving UI-DETR-1 out of v1 and of using pixels only.
- **Cost of 100 % computer vision** (decision 9): no ground-truth element names, roles or states from the OS, so labels,
  names and classes come from the user's review. Mitigation: OCR text inside or next to a box is the default label and
  name (the user confirms or corrects it in the review queue), and the VLM can suggest names for a small user-chosen
  selection (rule 46). Sighter AGENTS.md ("100 % computer vision") and proposal sections 5 and 8 (the `LabelSource`
  list has no accessibility or DOM sources) were updated to match in revision 3.
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
    (decision 9). Adapter hints: a user's own framework adapter may pass rectangles it already has as optional,
    recorded hints that never replace detection; Sighter itself never calls UIA, AX, MSAA or AT-SPI.
16. Training path: **video → auto-label → review → train**; boxes from OCR, templates and the app's own detector;
    Pitwall's VLM only names, describes and suggests classes, after a confirmation with the count; RF-DETR Small on the
    GPU; later videos pre-labelled by the app's model, the VLM only for new screens (decision 10).
17. Augmentation and synthetic data as in that section, each technique kept only if the held-out real frames show a
    gain.
18. Latency budgets in two tiers: a **gate** (full frame p95 ≤ 500 ms GPU / 2 s CPU at 1920 × 1200, ≤ 1 s / 4 s at
    4K) and the old numbers as a **target**, as named constants ("Latency budgets: gate and target").

**Amendment (user, 2026-10-30), the DirectML gate (decision 4, P6):** `DirectML.dll` registers a TraceLogging provider,
`Microsoft.Windows.AI.MachineLearning.Dml`, in the Microsoft telemetry group
([ORT builds note](../research/2026-10-ort-builds.md) section 5). It is bundled only if both checks pass:
(1) the egress test shows **no outbound connection** while the DirectML EP runs; (2) the S2 ETW session shows that
provider writes **no events** meanwhile. Windows' diagnostic settings then govern it like any OS component. It is fetched from nuget.org at build time, pinned by hash, and never mirrored. The P6 method
(reading the DLL's bytes and recording its ETW events, no disassembly) is accepted under the licence's clause 3(b).

## Open questions for the user

None. The only pending item is the RT-DETRv4 licence review (plan item P4); RT-DETRv4 stays out of the catalogue until
it is done.

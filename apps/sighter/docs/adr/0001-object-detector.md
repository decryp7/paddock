# 1. Sighter's object detector: RF-DETR (Apache sizes) on ONNX Runtime in Sighter's own sidecar

Status: proposed (2026-10-29). Four of its six questions were decided by the user on 2026-10-29 (marked "user,
2026-10-29" below); RT-DETRv4 and the Windows DirectML question are still open.

Related: [proposal](../proposal.md) sections 6, 8, 9.1, 11 and 12; [design review](../review/2026-10-design-review.md);
research notes [2026-10-29-rf-detr](../research/2026-10-29-rf-detr.md) (this ADR's evidence) and the family note
[2026-10-sighter-detector](../../../../docs/research/2026-10-sighter-detector.md); Sighter AGENTS.md ("Licences of
models", "Pitwall is always optional"); family rules 10 to 15, 23, 26, 42, 46 and 48; ADR 0024 (background work);
Pacenotes ADR 0001 (static ONNX Runtime and rule 23).

## Context

Sighter locates UI elements by sight through a layered resolver (proposal section 8): cache, template, OCR anchor,
**trained detector**, optional Pitwall VLM grounding, then "needs labelling". The trained detector is a per-app,
closed-set model trained on the user's own labelled frames (1k to 5k per app) and queried per frame. It has to:

- carry permissive licences for code **and** weights (no AGPL, GPL or non-commercial: Sighter AGENTS.md);
- run from Rust on macOS (Apple Silicon only since 2026-10-27) and Windows, with a GPU where there is one and a usable CPU
  path where there is not (CI runners);
- meet the proposed budgets: locate p50 ≤ 80 ms and p95 ≤ 250 ms on a GPU, p95 ≤ 600 ms on a CPU; detector < 50 ms on a
  GPU at 640 px; model cold load ≤ 1.5 s; server ≤ 1.5 GB with three models;
- keep rule 23 (one Team ID in the bundle, sidecars self-test from the built app);
- work without Pitwall, deterministically on the CPU, and say which execution provider answered.

The proposal named RT-DETR, D-FINE/DEIM, DEIMv2 and RF-DETR as candidates; the backlog of 2026-10-27 asked "RF-DETR or
UI-DETR-1?". The research note answers with today's official sources and one indicative measurement.

## Options considered

**Architecture**

| Option | Licence (code / weights) | COCO AP50:95, publishers' T4 latency | For | Against |
|---|---|---|---|---|
| **A. RF-DETR N/S/M/L** | Apache-2.0 / Apache-2.0 | 48.4–56.5, 2.3–6.8 ms | Best published fine-tuning transfer (RF100-VL 57.7–62.2); one architecture, size is a resolution knob; plain ONNX, no NMS; training package does COCO datasets, EMA, eval and export; measured in ONNX Runtime here | ViT backbone, heavier on CPU than a CNN; five sizes of the family are PML 1.0 and must be fenced off; fast-moving upstream |
| B. D-FINE (with the DEIM recipe) | Apache-2.0 / Apache-2.0 | 42.8–55.8, 2.1–12.9 ms | CNN backbone, likely cheaper on CPU; same ONNX adapter | Weaker transfer evidence on non-COCO domains; training code is research-grade scripts |
| C. RT-DETRv4 | Apache-2.0 / Apache-2.0 per repo | 49.8–57.0, 3.7–12.9 ms | Strong numbers, CNN at inference | Trained by distillation from DINOv3 (custom, non-OSI licence): unclear whether trained weights inherit its terms |
| D. DEIMv2 | non-commercial | 50.9–57.8 | | **Excluded** by licence |
| E. Ultralytics YOLO, YOLOv9/v10, YOLO-NAS, OmniParser `icon_detect` | AGPL / GPL / restricted | | | **Excluded** by licence |
| F. Open-vocabulary detector or VLM at run time | various | | No training | Seconds per query, non-deterministic, no stable taxonomy; belongs in Pitwall for bounded requests |

**Runtime**

| Option | Licence | For | Against |
|---|---|---|---|
| **`ort` 2.0 (ONNX Runtime 1.31), static** | MIT OR Apache-2.0 / MIT | Prebuilt static libraries with CoreML (macOS arm64) and DirectML (Windows x64/arm64), hash-pinned; no vendor dylib (rule 23); shared with PaddleOCR | ~80 MB static runtime; CDN download at build time; DirectML in sustained engineering; `DirectML.dll` redistribution unverified |
| `tract` | MIT OR Apache-2.0 | Pure Rust, no C++ | CPU only; full RF-DETR graph not yet loaded |
| `candle` / `burn` | MIT OR Apache-2.0 | Pure Rust with Metal/CUDA | No RF-DETR decoder; a port is real work |

**Where it runs**

| Option | For | Against |
|---|---|---|
| **Sighter's own `sighter-infer` sidecar** behind the `Detector` port | Works without Pitwall; no HTTP hop per tile; Sighter owns model, EP and determinism record; a runtime crash ends one request | Sighter carries ONNX Runtime (it needs it for OCR anyway) |
| Pitwall | One AI host for the family | Makes locating depend on Pitwall (against AGENTS.md); adds ONNX Runtime to Pitwall for one client; a C++ fault takes down the family's AI host; Pitwall is the LLM/VLM layer (rule 46 keeps it to bounded requests) |

## Decision

1. **Architecture: RF-DETR, Apache-2.0 sizes only** (user, 2026-10-29). Presets: **Balanced = Small (default)**, **Fast = Nano** (CPU-only
   runners), **Accurate = Medium or Large**. Input sides are multiples of 32 (patch 16 × 2 windows); tiles stay 640 and
   960 at 1:1 scale (design review). Atto, Femto, Pico, XL and 2XL are refused: the model catalogue lists only the four
   Apache checkpoints, and model import refuses an ONNX whose metadata or card names a PML size.
2. **Fallback: D-FINE-S/M** (user, 2026-10-29) with the DEIM recipe, through the same "ONNX DETR, fixed input, no NMS" adapter, used when
   the spike shows RF-DETR missing the CPU budget on the CI runner class or failing on an EP. RT-DETRv4 is reconsidered
   only after the open question on DINOv3-distilled weights is answered.
3. **Training: the `rfdetr` package** (Apache-2.0), version-pinned, inside the managed Python training runtime the user
   accepted on 2026-10-24. Start weights: the Apache COCO checkpoints only. **UI-DETR-1 is not in v1** (user,
   2026-10-29): neither as a labeller nor as start weights, because the licences of its training datasets are not
   stated.
4. **Inference: `ort` with statically linked ONNX Runtime**, in Sighter's `sighter-infer` sidecar (started through
   `paddock-ipc`, supervised by `paddock-proc` with a memory cap measured as footprint per rule 26, `--self-test` per
   rule 23), shared with OCR. EPs: CPU everywhere (the reference and determinism baseline), CoreML with `MLProgram` on
   macOS, DirectML on Windows; `error_on_failure` for the chosen EP, and every response records EP, ONNX Runtime version
   and model hash. The CoreML compiled-model cache is keyed by model hash and cleaned with the model registry.
5. **Not in Pitwall.** Pitwall stays optional and is the VLM layer for labelling help, naming and grounding fallback, each
   a bounded, user-started request (rule 46). **Labelling** (user, 2026-10-29): whole recordings are pre-labelled by the
   user's own trained detector for that app. Pitwall's VLM is used only on a small selection the user chooses, or after a
   confirmation that states the frame count (and the expected time). Before the first model of an app exists,
   pre-labelling comes from the deterministic sources (OCR, template, accessibility `LabelSource`s) and the review queue,
   not from the VLM per frame. Proposal 9.1's "label suggestions" request kind follows this rule.
6. **Model distribution.** The runtime detector is always made locally and stored immutable and content-hashed in the
   project registry with its `model.json` (classes, input size, normalisation, thresholds, dataset hash, metrics, parent,
   base checkpoint file name, its licence and the `rfdetr` version). The only downloaded models are the four Apache
   COCO checkpoints, **mirrored on our Gitea (dev.decryptology.net)** with the Apache-2.0 licence and notice, and pinned
   to **our own SHA-256s** (user, 2026-10-29); upstream's MD5s are checked once, when the mirror is made. They come only
   from a catalogue built into Sighter, one TOML entry each with the mirror URL, size, SHA-256, licence and the upstream
   URL it was taken from, parsed once (the `paddock-runtime` catalogue pattern), downloaded through `paddock-download`
   (hash while writing, rule 15), as a job. Nothing but a tiny self-test model ships in the installer.
7. **Jobs.** Download, pre-labelling, evaluation, CoreML preparation and training are `paddock-jobs` jobs with
   phase-labelled progress, Cancel and background priority (ER-17 to ER-20, ADR 0024). Locate is a bounded request.
8. **Spike before the first release**, per the research note's checklist; its numbers go into
   [performance.md](../performance.md) as results blocks (rule 48) and can move the default preset or switch the CPU
   preset to the fallback without changing this ADR.

## Plan items

- **P1. Mirror the COCO checkpoints** (from decision 6). Download `rf-detr-nano.pth`, `rf-detr-small.pth`,
  `rf-detr-medium.pth` and `rf-detr-large-2026.pth` from the URLs in the pinned `rfdetr` version's `model_weights.py`,
  check upstream's MD5s, compute SHA-256s, and upload them with `LICENSE` (Apache-2.0) and a `NOTICE` naming the
  source, version and upstream URLs to a release or package on dev.decryptology.net; then write the catalogue entries.
  About 1.5 GB (Nano and Small measured at 366 and 386 MB; Medium and Large estimated). **Needs the user's OK when it
  happens**: it is a large, outward-facing upload, so the agent doing it asks first and does not do it on its own.
- **P2. Update the proposal** when this ADR is accepted (see Consequences).

## Consequences

- One adapter covers RF-DETR, D-FINE and RT-DETR alike; swapping the detector is a different ONNX file and a catalogue
  entry, not new code (open/closed).
- Sighter carries ONNX Runtime (~80 MB static in one sidecar binary) and the `cdn.pyke.io` build-time download, cached in
  CI and pinned by Renovate. Intel Macs are not supported (already decided).
- `cargo deny` must allow `ort`/`ort-sys` (MIT OR Apache-2.0) and ONNX Runtime's notices; the model catalogue gets its
  own licence allow-list (Apache-2.0, MIT) checked by a test over every entry.
- Windows packaging has one open item (the `DirectML.dll` question); macOS packaging has none beyond Pacenotes'
  findings (macOS ≥ 13.4 link floor, `libclang_rt.osx.a`).
- CoreML compiles each model on first use: 23 to 49 s per session here, without a cache (indicative,
  [performance.md](../performance.md#2026-10-29-rf-detr-onnx-latency-indicative-docssighter-rfdetr)), against a 1.5 s
  cold-load budget. The compiled-model cache is required, and "preparing the model for this Mac" runs as a job phase
  right after training or import, never inside a locate request. Once compiled, a 640 px tile of Small took about 30 to
  35 ms; on 4 CPU threads about 0.2 s, so CPU locating works on window crops or regions, not on whole tiled frames.
- Determinism is promised bit-identical only on the CPU EP with fixed threads; GPU EPs promise "same top candidate within
  tolerance".
- The training runtime pins an upstream that releases often; every bump re-runs the export and self-test checks.
- Fine-tuned models inherit Apache-2.0 from the base checkpoint; the user's own data stays the user's. The model card
  records both.
- Proposal section 6 (presets) and section 8 (licence table) are updated when this ADR is accepted: RF-DETR becomes
  primary, DEIMv2 moves to excluded (non-commercial), RT-DETRv4 is added as pending, UI-DETR-1 and the "Florence-2
  bootstraps boxes" idea are not v1, and 9.1's label suggestions follow decision 5.
- A new app starts with no detector: until the user has reviewed enough frames to train the first model, labelling is
  OCR, template, accessibility sources and manual review. This is the price of leaving UI-DETR-1 out of v1.
- The mirror makes downloads independent of Roboflow's URLs (the Large checkpoint already moved once), and costs about
  1.5 GB on our Gitea. Updating a checkpoint is a new mirror entry with a new hash, never an overwrite.

## Decided questions (user, 2026-10-29)

1. Default detector: **RF-DETR Small**; Nano is the CPU preset, Medium or Large the "Accurate" preset; **D-FINE** is
   the fallback, behind the same ONNX adapter (decisions 1 and 2).
2. Start weights: **mirror the Apache COCO checkpoints on our Gitea**, pinned to our own SHA-256s (decision 6, plan
   item P1; the upload needs the user's OK when it happens).
3. UI-DETR-1: **not in v1**, because its datasets' licences are not stated (decision 3).
4. Labelling: **whole recordings are pre-labelled by the user's own detector**; Pitwall's VLM only for small,
   user-chosen selections or after a confirmation that states the frame count (rule 46; decision 5).

## Open questions for the user

1. **RT-DETRv4**: worth a licence review of DINOv3-distilled weights, or exclude it like DEIMv2?
2. **Windows**: if the system `DirectML.dll` is too old, may Sighter ship Microsoft's DirectML redistributable next to
   the sidecar, or should Windows GPU wait for Windows ML?

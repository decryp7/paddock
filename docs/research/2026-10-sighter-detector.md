# Sighter's detector: RF-DETR, UI-DETR-1 or something else? (2026-10-27)

Question (user): "Use RF-DETR or UI-DETR-1 for Sighter?" Compared with the close alternatives (YOLO variants, Florence-2,
OmniParser, GroundingDINO, and the D-FINE/DEIM/RT-DETR family the proposal already names), for licences, accuracy on UI
elements, size and latency, running from Rust, rule 23, fine-tuning, and fit with `apps/sighter/docs/proposal.md`
(sections 6, 8, 11, 12) and `apps/sighter/docs/review/2026-10-design-review.md`. Research note from published sources;
nothing was measured on our data. Figures marked (verify) come from secondary sources.

## Answer in short

- **They are not two choices at the same level.** UI-DETR-1 *is* RF-DETR: an RF-DETR-Medium fine-tuned by racine.ai on
  about 2,600 web screenshots, with **one class** ("clickable element"). So the real question is "RF-DETR as the
  architecture, and is UI-DETR-1 a useful starting point?"
- **Recommendation: RF-DETR (Apache-2.0 sizes only: Nano, Small, Medium, Large) as the architecture of Sighter's per-app
  detector**, trained by Sighter's record–label–train loop, exported to ONNX and run through `ort` (statically linked) in an
  out-of-process inference worker. Default preset RF-DETR-S; Nano for the CPU-only runner; Medium/Large as "Accurate".
- **UI-DETR-1: an optional bootstrap labeller, not the product model.** Being class-agnostic it cannot answer
  `GetByKind(Kind.Button)`, it was trained on web pages only, and its training data's licences are unclear. Use it, as an
  opt-in download, as a `LabelSource` that proposes boxes on day one, and measure in the spike whether starting fine-tuning
  from its weights beats COCO weights.
- **Fallback: D-FINE (with the DEIM recipe)**, Apache-2.0 code and weights, CNN backbone (cheaper on CPU, no ViT), same
  ONNX path. YOLOX-Nano stays the tiny CPU tier. Ultralytics YOLO (and OmniParser's detector, which is a YOLOv8) stay
  excluded (AGPL).
- **Never use** RF-DETR XL, 2XL, Atto, Femto or Pico weights: they are under Roboflow's Platform Model License 1.0 and
  need a Roboflow account.

## 1. The candidates

| Model | What it is | Code licence | Weights licence | Verdict for Sighter |
|---|---|---|---|---|
| **RF-DETR** N/S/M/L (Roboflow) | Real-time DETR with a DINOv2 ViT backbone, found by weight-sharing architecture search; ICLR 2026 (arXiv 2511.09554) | Apache-2.0 | **Apache-2.0** for N, S, M, L and all segmentation sizes | **Primary** |
| RF-DETR XL/2XL, Atto/Femto/Pico | Same family; XL/2XL larger, Atto–Pico on a PE-Core-T backbone | `rfdetr[plus]`: PML 1.0 | **PML 1.0**, Roboflow account required | **Excluded** |
| **UI-DETR-1** (racine.ai) | RF-DETR-M fine-tuned on 6 Roboflow Universe web-UI datasets (2,656 train images), one class, run at 1600 px | uses `rfdetr` (Apache-2.0) | MIT per model card (training datasets' licences not stated) | **Optional bootstrap labeller** |
| D-FINE / DEIM | DETR with fine-grained box refinement (D-FINE) and a faster-converging training recipe (DEIM), HGNetv2 CNN backbone | Apache-2.0 | Apache-2.0 | **Fallback** |
| DEIMv2 | DEIM on a DINOv3 backbone | Apache-2.0 | DINOv3 licence (Meta, not OSI) (verify) | Not without a licence check |
| RT-DETR / RT-DETRv2 | The original real-time DETR | Apache-2.0 | Apache-2.0 | Superseded by D-FINE/RF-DETR on the same budget |
| YOLOX | Anchor-free YOLO (2021), needs NMS | Apache-2.0 | Apache-2.0 | Tiny CPU tier only |
| Ultralytics YOLOv5/8/11/26, YOLOv10 | YOLO family | AGPL-3.0 | AGPL-3.0 | **Excluded** |
| YOLOv9 (WongKinYiu) | | GPL-3.0 (verify) | | **Excluded** |
| YOLO-NAS | | Apache-2.0 | non-commercial weights | **Excluded** |
| **OmniParser v2** (Microsoft) | `icon_detect` (fine-tuned YOLOv8) + `icon_caption` (fine-tuned Florence-2) | MIT (repo) | **icon_detect AGPL**, icon_caption MIT | Detector excluded; the idea (detect then caption crops) is already Sighter's |
| **Florence-2** base/large (0.23B/0.77B) | Small vision-language model: caption, OCR, region proposals, phrase grounding | MIT | MIT | Optional labeller (CPU, no Pitwall), not the runtime detector |
| **GroundingDINO** (IDEA) | Open-vocabulary detector from text prompts | Apache-2.0 | Apache-2.0 | Weak on UI (trained on natural images); not chosen |

Why not an open-vocabulary detector (GroundingDINO, Florence-2) as the runtime layer: they are 3 to 30 times larger than
RF-DETR-S, were trained on natural images, give no stable class taxonomy for `GetByKind`, and their text-conditioned
answers are less deterministic in practice. The proposal's split stays right: a small closed-set detector per app at run
time, open-vocabulary models and VLMs only for labelling and the long tail.

## 2. Accuracy on UI elements

There is **no published ScreenSpot number for RF-DETR, UI-DETR-1, D-FINE or any closed-set detector**, and there cannot be
one directly: ScreenSpot (SeeClick, arXiv 2401.10935: 600+ screenshots, 1,200+ instructions across iOS, Android, macOS,
Windows and web) and ScreenSpot-Pro (arXiv 2504.07981: 23 professional apps, 3 OSes, high resolution) measure **grounding**
(instruction to a point), which needs a language component. A detector enters those benchmarks only inside a pipeline:

| System | Benchmark | Score | Note |
|---|---|---|---|
| OmniParser v2 (YOLOv8 detector + Florence-2 captions) with GPT-4o | ScreenSpot-Pro | 39.6 % | Model card; 0.6 s/frame on A100 |
| UI-DETR-1 (+ Gemini 2.5 Pro choosing the element) | WebClick (1,639 web tasks) | 70.8 % vs OmniParser 58.8 % (same LLM, tuned prompts); 67.5 % vs 40.7 % with default parameters | Self-reported; web only; class-agnostic |
| Holo1.5-7B / -72B (VLM) | ScreenSpot-Pro | 57.9 % / 63.3 % | H Company's comparison |
| UI-TARS-1.5-7B (VLM) | ScreenSpot-Pro | 49.6 % (ByteDance) or 39.0 % (H Company's harness) | Harness matters |
| GTA1-7B / -32B (VLM) | ScreenSpot-Pro | 50.1 % / 63.6 % | arXiv 2507.05791 |
| Qwen3-VL-8B-Thinking (VLM) | ScreenSpot-Pro | 38.5 % | H Company's table |

What this means for Sighter:

- **Grounding VLMs are Pitwall's layer**, not the detector. Their best open scores (about 50–64 % on ScreenSpot-Pro, seconds
  per query) confirm the proposal's rule: the VLM labels and handles the long tail, never the per-frame runtime.
- **For the detector the relevant published signal is fine-tuning transfer**, because Sighter trains per app on 1k to 5k
  frames. RF-DETR's paper introduces exactly this measure, RF100-VL (100 diverse real-world datasets fine-tuned from the
  same pretrained weights): AP50:95 Nano 57.7, Small 60.2, Medium 61.2, Large 62.2, where its COCO numbers are N 48.4,
  S 53.0, M 54.7, L 56.5 (T4, TensorRT FP16, 2.3 / 3.5 / 4.4 / 6.8 ms). The DINOv2 backbone is the reason it adapts well to
  domains far from COCO, and screens are far from COCO.
- UI-DETR-1's WebClick result shows that an RF-DETR-M fine-tuned on only about 2,600 UI screenshots already beats
  OmniParser's YOLOv8 detector end to end, which supports the architecture for UI. It says nothing about desktop or legacy
  apps, which is Sighter's market.
- The decision number stays Sighter's own: **top-1 hit rate and wrong-click rate on our five legacy apps** (proposal
  section 8, decision rule for the spike), plus per-class mAP@0.5:0.95 on held-out sessions.

## 3. Size and latency

| Model | Params | Native input | GPU (published) | CPU (published, indicative) |
|---|---|---|---|---|
| RF-DETR-N | 30.5 M | 384 | 2.3 ms T4 TRT FP16; ~103 inf/s ONNX on L4 | ~21 FPS (≈ 45–48 ms) in the opendetect benchmark, CPU unnamed (verify) |
| RF-DETR-S | 32.1 M | 512 | 3.5 ms | not published |
| RF-DETR-M | 33.7 M | 576 | 4.4 ms | not published; an RF-DETR variant runs ~97 ms on an Intel i9 with ONNX Runtime 1.25.1 (OpenCV 5 DNN benchmarks) |
| RF-DETR-L | 33.9 M | 704 | 6.8 ms | not published |
| UI-DETR-1 | 33.7 M (M) | **1600** (model card) | ~0.82 s/image (hardware not stated) | expect ~(1600/576)² ≈ 8× RF-DETR-M |
| D-FINE-S / M | ~10 M / ~19 M | 640 | ~3–5 ms T4 (verify per release) | CNN backbone, expected cheaper than ViT on CPU (measure) |

Notes that matter for Sighter's budgets (proposal section 7: detector < 50 ms on GPU at 640, < 250 ms on an 8-core CPU;
design review: locate p95 ≤ 600 ms on CPU):

- **Input size.** RF-DETR input sides must be divisible by 14 (patch size), not 32: Sighter's 640/960 tiles become
  **644/966** (or 672/980). The design review's P1 fix (bring the frame to 100 % scale, detect on 1:1 window crops with
  overlapping tiles) applies unchanged; UI-DETR-1's 1600 px is the opposite trade (one big frame) and is too slow for
  the runtime path.
- **Parameters are about the same from N to L** (30–34 M); the cost difference is resolution. This makes "one architecture,
  per-app resolution" a simple preset knob.
- **CPU is RF-DETR's weak side** (a ViT runs global/windowed attention). If the spike shows Small above ~150 ms per 644
  tile on the 8-core runner, that is the trigger for the fallback (D-FINE-S) on CPU-only runners.
- **Apple Silicon.** ONNX Runtime's CoreML execution provider takes the parts of the graph it supports and leaves the rest
  on the CPU. The deformable-attention decoder is exported with a patched `MSDeformAttn` that turns into `GridSample`
  (Luxonis integration guide); whether CoreML takes `GridSample` and the ViT blocks or partitions the graph many times is
  **unknown and must be measured** (CPU EP is the reference path either way).
- **Windows GPU.** The DirectML EP supports opsets up to 20 (DirectML 1.15.2) and is now in **sustained engineering**;
  Microsoft points new work to Windows ML, which is ONNX Runtime with automatic EP selection. DirectML still works and is
  what pyke's prebuilt Windows ONNX Runtime links; plan Windows ML as the later path, not v1.
- **Batching across targets** (design review: 8 targets on one controller) fits DETR well: fixed input size, no NMS, one
  batched run.

## 4. Running it from Rust

| Route | Fit | Notes |
|---|---|---|
| **`ort` (ONNX Runtime) with static linking** | **Chosen** | RF-DETR exports to ONNX itself (`model.export()`, opset 17 default). Pre/post-processing (resize, normalise, box decode, top-k) is pure Rust in `sighter-core`. EPs: CPU everywhere, CoreML (macOS), DirectML (Windows), CUDA optional (NVIDIA licence, user-installed). |
| `tract` (pure Rust) | CPU fallback, verify | No C++; whether its ONNX coverage handles `GridSample` and the RF-DETR graph is unverified; test in the spike. |
| `candle` | Later | Has DINOv2, not RF-DETR's decoder; a port is real work but would give Metal and CUDA with no C++ runtime. Worth it only if ONNX Runtime causes trouble. |
| `burn` (ONNX import) | Later | Same op-coverage question; the proposal keeps burn for a tiny YOLOX-style model. |
| Pitwall / llama.cpp | **No** | llama.cpp runs LLMs and VLMs (GGUF), not detection transformers. Pitwall stays the VLM layer (Qwen3-VL, UI-TARS, Holo) for labelling and the long tail. |

**Rule 23 (ad-hoc sealing, one Team ID).** Pacenotes ADR 0001 already checked `ort-sys` 2.0.0-rc.15 on macOS arm64:

- With `download-binaries` (static), pyke's prebuilt `libonnxruntime.a` is linked into the binary; `otool -L` shows only system
  libraries and frameworks, **no vendor dylib**, so rule 23 is not at risk. `load-dynamic` and `copy-dylibs` must stay off
  (they bring back a dylib with a foreign signature: the Debrief 1.0.1 `libduckdb.dylib` "different Team IDs" failure).
- Costs found there: no `x86_64-apple-darwin` prebuilt (Intel Macs need a self-built ONNX Runtime or are CPU-`tract` only);
  the archive needs macOS ≥ 13.4 and `libclang_rt.osx.a` on the link line; the build downloads from `cdn.pyke.io` (CI cache,
  Renovate pin). On Windows pyke's build always links DirectML/D3D12; whether the system `DirectML.dll` suffices or the
  redistributable must ship is **unverified**.
- **Put inference in its own sidecar** (`sighter-infer` worker over `paddock-ipc`): a C++ abort in ONNX Runtime ends one
  request, not the server; its memory is measured and capped by `paddock-proc`; only that binary carries the 80 MB static
  runtime; and CI starts it from the built app with `--self-test` (rule 23 already requires that for every sidecar). The
  self-test runs a bundled tiny ONNX model on CPU and each available EP and compares boxes to a stored answer.
- **Determinism** (design review P2): the bit-identical promise holds on the CPU EP with fixed thread settings; GPU EPs
  promise "same top candidate within tolerance". Record EP, ONNX Runtime version and model hash in every response.

PaddleOCR (the OCR layer) also runs through `ort`, so the one sidecar serves both. No second Paddock app needs ONNX Runtime
today (Pacenotes rejected it in ADR 0001), so this stays in Sighter (rule 42: shared only when a second app needs it).

## 5. Fine-tuning on our own labelled frames

- The `rfdetr` Python package trains detection from **COCO-format** datasets (Sighter's studio already exports COCO), with
  its own augmentation, EMA, COCO evaluation and early stopping, and exports ONNX. That is most of what the proposal planned
  to vendor as "training code", and it is Apache-2.0. It runs in Sighter's **managed, sandboxed Python runtime** (user
  decision 2026-10-24: Python is acceptable for training; inference stays Rust).
- Start weights: COCO-pretrained RF-DETR (Apache-2.0) by default. **Spike question:** does starting from UI-DETR-1 (head
  replaced for Sighter's classes) reach the same mAP with fewer frames? If yes, it becomes an opt-in "UI start" preset
  once its training-data licences are confirmed.
- Hardware (proposal section 12, 8 GB GPU): RF-DETR-S/M at 644 px with small batches and gradient accumulation fit
  (Roboflow's docs suggest keeping batch × accumulation around 16; verify). MPS training on Apple Silicon: verify per
  release; CPU training works but is slow. Remote host over SSH stays the option for Large.
- Taxonomy: closed classes per app (button, text box, checkbox, radio, combo, tab, menu item, icon, link, grid cell,
  scrollbar…) plus the review rules already in the proposal (unreviewed regions masked as ignore, splits by session).
- Model card (proposal "Versions"): base architecture **RF-DETR-S**, base weights and their licence (Apache-2.0 COCO, or
  UI-DETR-1 MIT + its dataset list), dataset hash, input size (multiple of 14), EP used in evaluation.

## 6. Fit with Sighter's design

- `Detector` trait (proposal section 8): unchanged; the adapter is "ONNX DETR, fixed input, no NMS", which covers RF-DETR,
  D-FINE and RT-DETR alike, so the fallback is a different ONNX file, not new code. YOLOX needs an NMS step in the adapter.
- Presets (section 6) become: **Fast** RF-DETR-N (CPU runners), **Balanced** RF-DETR-S (default), **Accurate** RF-DETR-M/L;
  **CPU fallback** D-FINE-S or YOLOX-Nano when the spike's CPU numbers miss the budget. DEIMv2 drops out until DINOv3's
  licence is cleared.
- `LabelSource` (section 8): add `UiDetr` (optional download, hash-pinned, class-agnostic proposals with low prior weight)
  next to `Ocr`, `Template`, `Vlm`, `A11y*`. It helps exactly where the studio starts from zero.
- Licence table (section 8): update RF-DETR to "Apache-2.0 for N/S/M/L weights; XL/2XL/Atto/Femto/Pico PML 1.0, excluded";
  add UI-DETR-1 as above.
- Model import (section 6): an imported ONNX is checked for input size (divisible by 14 for RF-DETR), output layout and
  classes; a PML-licensed RF-DETR is refused by its metadata where detectable and flagged in the model card otherwise.

## 7. Spike checklist (week 1, before the decision is final)

1. Export RF-DETR-N/S/M (COCO weights) and D-FINE-S to ONNX; load all in `ort` static on macOS arm64, Windows x64, Linux x64.
2. Time a 644 tile and a 1080p frame tiled at 1:1 on CPU (8 threads), CoreML, DirectML; record EP partition counts.
3. Fine-tune S on one legacy app (1k frames) from COCO and from UI-DETR-1; compare mAP@0.5:0.95, wrong-click rate and frames
   needed.
4. Check `tract` on the same ONNX files (works / unsupported op).
5. Confirm on the repos: RF-DETR LICENSE and LICENSE.platform per size; UI-DETR-1 card licence and its six datasets'
   licences; DirectML redistributable need on Windows.

## Sources

- RF-DETR repository and licences: https://github.com/roboflow/rf-detr
- RF-DETR detection models (sizes, COCO AP, latency, licences): https://rfdetr.roboflow.com/latest/learn/run/detection/
- RF-DETR export (ONNX, opset 17, sides divisible by 14): https://rfdetr.roboflow.com/latest/learn/export/
- RF-DETR paper (ICLR 2026): https://arxiv.org/abs/2511.09554
- RF-DETR on Luxonis (MSDeformAttn patch for export): https://docs.luxonis.com/software-v3/ai-inference/integrations/rf-detr
- Roboflow inference benchmarks (RF-DETR-N ONNX on L4): https://inference.roboflow.com/using_inference/benchmarks/
- opendetect benchmarks (RF-DETR-N CPU FPS): https://opendetect.readthedocs.io/en/latest/guides/benchmarks.html
- OpenCV 5 DNN benchmarks (RF-DETR on Intel i9): https://lab.trianta.dev/mirrors/opencv.opencv/wiki/OpenCV-5-DNN-Benchmarks
- UI-DETR-1 blog (architecture, data, WebClick): https://huggingface.co/blog/paulml/ui-detr-1
- UI-DETR-1 model card: https://huggingface.co/racineai/UI-DETR-1
- DINOv2 (Apache-2.0): https://github.com/facebookresearch/dinov2
- D-FINE: https://github.com/Peterande/D-FINE ; DEIM: https://github.com/ShihuaHuang95/DEIM ; DEIMv2: https://github.com/Intellindust-AI-Lab/DEIMv2
- RT-DETR: https://github.com/lyuwenyu/RT-DETR ; YOLOX: https://github.com/Megvii-BaseDetection/YOLOX
- OmniParser v2 model card (licences, ScreenSpot-Pro 39.6): https://huggingface.co/microsoft/OmniParser-v2.0
- Florence-2: https://huggingface.co/microsoft/Florence-2-base
- GroundingDINO: https://github.com/IDEA-Research/GroundingDINO
- ScreenSpot (SeeClick): https://arxiv.org/abs/2401.10935 ; ScreenSpot-Pro: https://arxiv.org/abs/2504.07981
- GTA1: https://arxiv.org/abs/2507.05791 ; Holo model comparison: https://hub.hcompany.ai/models/comparingmodels
- ort linking: https://ort.pyke.io/setup/linking
- DirectML EP (sustained engineering, opset 20): https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html ;
  https://learn.microsoft.com/windows/ai/directml
- Internal: `apps/sighter/docs/proposal.md`, `apps/sighter/docs/review/2026-10-design-review.md`,
  `apps/pacenotes/docs/adr/0001-embeddings.md` (static ONNX Runtime and rule 23), `docs/rules.md` (rules 23, 42).

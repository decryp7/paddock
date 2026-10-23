Status: proposal, open questions pending

# Proposal: Sighter, a local vision locator server for UI automation

Date 2026-10-23, revision 3 (clean rewrite; supersedes revisions 1 and 2). Research proposal, nothing built. Items marked (verify) come
from secondary sources (Roboflow catalog pages, mirrors, blogs, forum posts) and must be checked against the official repo or paper
first. Speeds and accuracy figures are published or order-of-magnitude, not measured on our data; a week-1 spike must measure.

## 1. Pitch

Sighter is "Pitwall for UI vision": a local server that engineers call from their own test scripts to find things on a screen. Send a
screenshot (or ask it to capture a window) and a query ("the Save button", text "Export", the third checkbox under "Options"); it answers
with boxes, confidence and which layer found it. **It works from pixels alone**, so it covers legacy apps with no or broken accessibility:
old desktop tools, custom-drawn UIs, remote sessions, games. Accessibility trees (UIA, AX, AT-SPI, DOM) are an optional label source that
speeds up labelling; nothing at run time depends on them. It is a built-in optional module, not a plugin process.

Three parts:
1. **Recorder**: captures windows/screens plus input events from the studio or headlessly on test machines.
2. **Studio** (Tauri + Angular, Label Studio/CVAT-like): label, review, train, evaluate and version a computer-vision model **per app**.
3. **Server** (`sighter-server`) with a layered resolver. Engineers write unit tests and scripts against `/v1/*`, via generated clients or CLI.

Everything is local: loopback only, no cloud, no telemetry. YOLO is not the headline; the detector is one interchangeable backend and the
product is the resolver.

## 2. Name: Sighter (confirmed)

Confirmed by the user; repo `decryp7/sighter`. Reasoning kept below. Scrutineer could be a later name for the assertion/report module if ever needed.

Collision checks were web searches only (no GitHub/crates.io/npm/trademark lookups); run those on the pick before committing.

| Name | Meaning | Collision notes |
|---|---|---|
| **Sighter** (confirmed) | The sighting lap: the lap before the race where cars and drivers check that everything is where it should be. Also "the thing that sights", fits a service you ask "where is X". Short, a noun for a service (`sighter serve`, `sighter locate`). | No UI-testing product found. Word used in archery/shooting and a few unrelated small projects; verify crate/npm/PyPI name and domain. |
| **Scrutineer** | The official who inspects a car against the rules: assertions and conformance. | No UI-testing tool found; "Scrutinizer" (PHP coverage) is a different word. Long to type. Runner-up; keep as the name of the assertion/report layer or CLI front end. |
| **Marshal** | Track marshal who flags problems (matches "flag for labelling"). | "marshal" is a serialization term (Go, Ruby, Java): bad for search and package names. Avoid. |
| **Onboard** | Onboard camera, the viewpoint of a recording. | Generic word, "onboarding" tools everywhere. Avoid. |
| **Ghost Lap** | Replay your previous lap beside you. | "Ghost" is crowded in automation (Ghost Inspector, a desktop-automation MCP server named Ghost); weak for a locator. |
| **Transponder** | Trackside transponder that reports where a car is. | Perfect semantics, long, generic hardware term. Third choice. |

Rejected: Apex (Salesforce Apex, Apex Legends), Telemetry (the one thing the suite promises not to do), Spotter (taken; it is the release
watcher). Decision: **Sighter**.

## 3. How it works end to end

```
 RECORD            IDENTIFY             REVIEW             TRAIN            SERVE              TEST
 recorder  ->  frames + events  ->  suggestions from  ->  human labels  ->  per-app model  ->  sighter-server  ->  engineer's
 (window/      scene-change,        OCR, template,        (queue, "same     (RT-DETRv2 /      layered resolver     scripts call
 screen/       dedup, click         detector, Pitwall     as...", track)    D-FINE/YOLOX)     /v1/locate ...       /v1/* (CI too)
 video import) boxes                VLM, a11y (optional)       |            ONNX, versioned        |
                                          ^                    |                                  v
                                          +---- flag for labelling <---- low confidence / unknown at run time
```

1. **Record** an app session (or import a video). Result: frames, input events, session metadata.
2. **Identify**: pick distinct frames; every label source proposes boxes, classes, names, with source and confidence.
3. **Review**: a human accepts, fixes or draws; anything not identified automatically lands in the manual review queue.
4. **Train**: reviewed labels only, split by session, with real variation first. Output: an immutable, versioned ONNX model per app.
5. **Serve**: `sighter-server` resolves queries through cache, template, OCR, detector, Pitwall VLM, then "needs labelling".
6. **Test**: engineers write unit tests and scripts against the API. Failures and low-confidence results can be flagged back to step 3.

Day-one value needs no training: OCR plus template matching cover many legacy lookups before a model exists.

## 4. The recorder

A small capture component, usable inside the studio and as a headless CLI/tray recorder (`sighter record`) on test machines and CI runners.

**Targets and capture APIs** (a window, a region or the whole screen; native per-OS APIs):
- Windows: Windows.Graphics.Capture, DXGI desktop duplication as fallback.
- macOS: ScreenCaptureKit.
- Linux: PipeWire via xdg-desktop-portal ScreenCast on Wayland (user consent per session unless a restore token is kept), X11 XShm.
- Capture runs in an out-of-process helper so a capture fault never crashes the studio.

**Quality**: lossless or near-lossless for UI: no chroma subsampling (4:4:4) or keyframe PNGs; text edges must survive. Variable frame
rate that captures on change (frame-diff trigger, plus a 2 fps floor and a frame at every input event), so idle UIs cost nothing.

**Input events**: clicks, keys, scroll and drag with timestamps and coordinates (window-relative and screen). The element under each
click is a strong labelling signal ("box from click": pixel differences before/after a click yield a candidate box and the question "what
did you click?") and a step signal (what the user did next). Key text is not stored by default; see section 10.

**Session metadata**: OS and version, display DPI/scale per monitor, theme (light/dark/high contrast), window title, process name,
resolution, locale and font settings where known. Optional accessibility snapshot (any enabled `LabelSource`); the recorder works with
an empty accessibility track.

**Privacy**: redaction regions drawn before recording (applied before anything is written), and automatic pause around password fields
when detectable (UIA `IsPassword`, AX secure text field). See section 10.

**Importing videos**: existing recordings (mp4/mkv/webm/mov) import with no event track; labelled by OCR + detector + VLM + review only
(marked lower quality in the project).

**Encoding**: our own recordings are stored as PNG/WebP frames with timestamps, plus an optional H.264/H.265 4:4:4 stream through the
OS encoder (Media Foundation, VideoToolbox/AVFoundation, GStreamer/VA-API) or ffmpeg as an LGPL dynamic build. The common path needs no
ffmpeg. Imports use ffmpeg as in Debrief ADR 0033 (`docs/adr/0033-video-sources-ffmpeg-runtime.md`: BtbN LGPL shared builds for
Windows/Linux, user-provided on macOS, sandboxed launcher). Native-Rust decoders exist (openh264-rs wraps Cisco's BSD decoder; `rav1d`)
but the container/codec matrix is too wide. **Proposal**: extract the shared parts of Debrief's video crate (runtime download, sandboxed
launcher, ffprobe, frame extraction) into a Paddock crate `paddock-video`, which also helps Debrief.

**Permissions** (explained and tested by a preflight screen in the UI, per OS): macOS Screen Recording (prompts repeat on unsigned
builds, so signing and notarisation matter) and Accessibility (for event capture and optional AX); Windows needs no prompt but DRM
content captures black and elevated windows need an elevated recorder to receive input events; Wayland portal consent per session.
Unsandboxed with hardened runtime on macOS. Each OS update may re-prompt.

## 5. The labelling studio

Built in, in the spirit of Label Studio (keyboard-heavy review, pre-annotation) and CVAT (video interpolation and tracking), but lighter
and offline. Not embedded (both are Python/Django servers); CVAT/Label Studio files import and export.

**Structure**: project -> sessions -> a frame timeline. Scene-change detection (perceptual hash plus SSIM on downscaled grayscale) and
near-duplicate removal mean only distinct screens are shown; a 10-minute recording typically becomes 150 to 500 distinct frames.

**Editor**: canvas with box and polygon tools, classes and optional names ("Save"), resize, merge, split, copy to similar frames,
undo/redo, keyboard-first, per-class colours validated for colour-vision deficiency. OCR text attached to every box.

**Auto-suggestions**: from the resolver layers (template, OCR, detector pre-labels once a model exists = active learning) and from
Pitwall; each suggestion shows its source and confidence.

**Tracking**: follow an element across frames with interpolation (optical flow / template tracking), so one fix applies to a run of
frames. "Same as..." applies one label to all look-alike crops (preview grid, uncheck exceptions).

**Class taxonomy**: button, text field, checkbox, radio, toggle, dropdown, tab, menu item, icon, link, list row, slider, scrollbar,
dialog, toast, progress, image, label, plus custom classes. UIA ControlType / AX role / ARIA role map to classes by a TOML table (data,
not code).

**Dataset**: splits by session (never by frame, to avoid leakage between near-identical frames), quality checks (duplicates,
overlapping or tiny boxes, class imbalance, label/source disagreements), export to COCO JSON and YOLO txt (CVAT XML for round-tripping),
and import of existing labels (COCO, YOLO, Label Studio JSON) so users can bring prior work.

### The review queue (manual labelling)
Anything not identified automatically is flagged; nothing is silently dropped or guessed. Flag when:
- no source proposes an element the pixels say exists (edge density, VLM or detector box without another source), or a region is only seen by one weak source;
- a source's confidence is below the threshold (default 0.6) or it returned no class;
- sources disagree on class, or on box (IoU below 0.5), or two classes are within 0.1;
- the class is unknown (not in the taxonomy) or OCR is unreadable.

Fusion (pure Rust, unit-testable): boxes from different sources merge at IoU >= 0.6 (class-aware NMS); score is a weighted agreement
(weights in a TOML profile, calibrated per model by temperature scaling on reviewed data); two independent sources agreeing raises to
0.95, disagreement lowers and flags. A label is **auto-accepted** only at score >= 0.85 with no disagreeing source. Reviewed labels carry
`source=human, score=1`. If an accessibility source is enabled, its nodes are verified by pixels (reject off-screen, zero-sized,
occluded or uniform crops) because trees can be stale or DPI-shifted.

Queue UI: items grouped by look-alike (crop embeddings), sorted by **impact** (frames containing the element times scripts/flows that
depend on it). Each row shows the crop, full-frame context, the reason and one-click suggestions with source and score. Keys: number
keys accept a suggestion, `/` types a class (autocomplete), arrows move, `S` skip, `D` draw, "same as...". Counts per session and per
flow: "3 elements need review before this flow can run unattended".

Effects: **unreviewed items are excluded from training** (flagged boxes are not positives; frames with unresolved flags are excluded or
have those regions masked as ignore regions, so an unlabelled button is never trained as background). Flows that depend on unidentified
elements are marked Needs review and **will not run unattended** (CLI and Pitcrew refuse with a clear message and exit code). At run
time, low confidence **pauses the step with a screenshot** (attended) or fails it (unattended), with candidates and a "flag for
labelling" action. Never a blind click. The queue is also fed by active learning (uncertainty ranking: entropy, augmentation
disagreement, disagreement between the last two model versions).

## 6. Training and UI variation

**Variation without image generation.** Diffusion models are too heavy for an 8 GB GPU and bad at UI text and layout. Use, in this order:
1. **Real variation captured by the recorder**: DPI 100/125/150/200 percent, window sizes, light/dark/high-contrast themes, fonts, languages. The recorder's session metadata makes each axis explicit, and the studio shows coverage ("no 150 percent dark sessions yet").
2. **Classic augmentation** designed for UIs: colour/theme jitter, scale 50 to 300 percent, font smoothing, JPEG, crop, blur, noise. No flips or rotations (text and layout are not symmetric).
3. **Synthetic compositing** of labelled elements: paste real labelled crops onto other real backgrounds/screens at varied positions and scales, with exact labels for free.

Diffusion is at most an optional "Later" experiment.

**Presets**: Fast (RT-DETR-R18-class, small GPU/CPU, minutes), Balanced (D-FINE/DEIM small/medium or RF-DETR), Accurate (D-FINE-L/X with
the DEIM recipe; DEIMv2 if its DINOv3 weight licence is clean, verify). YOLOX-Nano for the CPU-only runner. Resume, pause, early stop,
live loss/mAP chart, hardware estimate before start ("about 25 min on this GPU, 4 GB"). Fine-tune from COCO-pretrained Apache weights;
1k to 5k frames per app is practical. Export ONNX (opset 17+, fixed input size per model, 640 default, 960/1280 for dense screens).

**Evaluate**: mAP@0.5 and @0.5:0.95 per class, PR curves, F1-optimal threshold per class, per-class confusion matrix (click a cell for
crops), hard-cases gallery, held-out session test (different theme/scale/OS), side-by-side version compare.

**Versions**: every model is immutable (content hash, dataset snapshot id, config, metrics, parent, base-weight licence); promote to
"current for this app", roll back. Scripts can pin a version (section 7).

**Training runtime decision**:
| Option | Verdict |
|---|---|
| Rust-native (burn, candle) | Not for v1: no maintained RT-DETR/D-FINE recipe with augmentation, EMA, matcher losses, mAP tooling. Revisit for a tiny YOLOX-style detector later. |
| **Managed sandboxed Python/PyTorch runtime, downloaded on request** (recommended) | Like Debrief's FFmpeg/llama.cpp runtimes (ADR 0009/0033): pinned bundle (python-build-standalone + torch wheels per OS/accelerator + vendored training code), SHA-256 pinned, self-tested on a generated mini dataset, out-of-process worker over the one IPC, no network, only the dataset readable and output writable, memory/time caps, cancel. CUDA torch is 2 to 3 GB, so not in the installer. Python is downloaded data, not shipped app code; needs an ADR exception clause for the "Rust + Angular only" rule. |
| Export-only | Always provided: COCO + YOLO txt + generated script + README, for users with their own GPU box or who reject a Python runtime. |
| User-chosen remote host over SSH | Same runtime image by checksum, dataset pushed, weights pulled. No third-party cloud. |

Accelerators: CUDA (Windows/Linux), ROCm (Linux, best effort), Apple MPS (smaller models, op gaps fall back to CPU), CPU everywhere.
Import of an externally trained ONNX model with a compatibility check (input size, output layout, class list).

## 7. The server API and clients

**Process**: `sighter-server`, a background service per OS (Windows service/user task, launchd agent, systemd user unit; same pattern as
Pitwall's `paddock-service`), loopback only (`127.0.0.1`/`::1`, optional unix socket/named pipe), per-app API keys in the OS credential
store (pairing code like Pitwall), request size caps, per-key rate limits, no remote bind in v1. Autostart opt-in; models loaded on first
use, idle release.

**Contract**: OpenAPI 3.1 is the source of truth (generated from Rust DTOs, checked in CI). JSON over HTTP; images as base64
PNG/JPEG/WebP, multipart, or `capture` descriptors.

**Endpoints (v1)**:
- `POST /v1/locate`: `{image | capture:{screen|window|region}, query:{class?, name?, text?, near?, inside?:box|screen_state, nth?}, region?, model?:"app@3", layers?, min_confidence?, timeout_ms?}` -> `{candidates:[{box, center, confidence, layer, class, name, text, evidence_id}], status:"found|ambiguous|not_found|needs_labelling", screen_state?, model_version, trace_id}`.
- `POST /v1/locate_all`: all matches for a class/text/region; `stream:true` (NDJSON) and batch with one shared screenshot.
- `POST /v1/wait_for`: `{query, state:"appears|disappears", timeout_ms, poll_ms}`; server-side polling with frame-diff to skip detector runs on unchanged screens.
- `POST /v1/assert_screen`: `{expect:"login" | fingerprint | baseline_image, mask?, tolerance}` -> match score, diff overlay.
- `POST /v1/ocr`, `POST /v1/capture` (screen/window/region), `POST /v1/flag` (send a result to the labelling queue by `trace_id`, correct box optional).
- `GET /v1/models`, `GET /v1/models/{id}`, `POST /v1/models/{id}/promote` (studio only); `GET /healthz`, `GET /v1/status` (layers, hardware, loaded models, p50/p95 per layer).
- `POST /v1/act` (optional, **off by default**, per-key `act` permission, scope-bound to a window/process, deny-list, dry-run, panic key): click, double_click, right_click, type, press, drag, scroll, or "locate then click". Many teams prefer their own driver, so every other endpoint works without it.

**Determinism and model pinning**: same image bytes + query + model version + server version -> identical result (fixed ONNX runtime
thread settings, FP32 reference path option, stable candidate ordering by score then position). Responses carry `model_version`,
`server_version`, `resolver_version`. `model:"app@3"` pins a version; unpinned requests use the app's `current`; promotion in the studio
never changes a pinned request. VLM answers are cached by key, marked `layer:"vlm"` and flagged for review, so replays become deterministic.

**Screen-state fingerprints** (downscaled perceptual hash + layout hash from detector/OCR output) name screens ("Login", "Save as
dialog"), constrain searches ("inside this dialog"), power `assert_screen`/`wait_for` and let cache/template layers skip work.

**Run artefacts**: optional per-key logging of each call (screenshot, query, candidates, layer, timings) into a bounded run folder
(`runs/<run_id>/<seq>-*.png/json`, retention caps, redaction applied before storing). `X-Sighter-Run: <id>` groups calls. A run folder
opens in the studio and exports a Debrief bundle.

**Clients**: OpenAPI is the contract. Thin generated MIT clients live **outside the app repo** in `sighter-clients`: Python (pytest
fixtures, `httpx`), TypeScript (Playwright/Node), C#/.NET (legacy Windows shops), a Robot Framework keyword library wrapping the Python
client; plus the Rust `sighter` CLI shipped with the app (`sighter serve|locate|wait-for|assert|ocr|record|models`). The app itself
contains no non-Rust code. Strict alternative: OpenAPI + CLI + 20-line examples per language. Recommendation: allow the generated clients.

**Performance targets** (goals to confirm in the spike; p50, 1080p screenshot in memory): cache under 5 ms; template under 30 ms; OCR
anchor under 100 ms with a region, 150 to 500 ms full screen on CPU (cache OCR by fingerprint); detector under 50 ms on a discrete/Apple
GPU at 640 px (5 to 20 ms inference + 10 to 25 ms capture/pre-processing), "under 250 ms at 640" on an 8-core CPU for a small model
(dense 1080p at 960 to 1280 px reaches 300 to 800 ms); VLM 1 to 5 s on GPU, 10+ s on CPU, never in those budgets.

## 8. The resolver and models

Per request, configurable: **cache** (screen fingerprint + query + model version) -> **template** (stored crop, fingerprint matches) ->
**OCR anchor** (query has text, or the element has a name) -> **trained detector** (class + context scored with OCR text) -> **Pitwall
VLM grounding** (opt-in; never the default for unattended runs) -> **needs labelling** (flag, with screenshot). The detector is a
backend behind a `Detector` trait; if a better permissive model appears, swap the ONNX. The product does not promise YOLO and uses no
Ultralytics code or weights.

**Pitwall's role**: identifies and names elements ("Settings (gear)"), resolves ambiguity ("which of these numbered crops is the Save
button of the 'Save as' dialog?", cached per screen hash and flagged), and describes steps for reports. It is the labelling teacher and
the fallback, never the per-frame runtime, because VLMs emit coordinates as text, are non-deterministic across runs and versions, and
cost seconds per query. Cost controls: ask only when other layers are unsure; de-duplicate frames first; downscale/tile (1280 px long
edge); batch crops in a numbered grid sheet; cache by (frame hash, model id, prompt version); low-priority cancellable jobs with an
estimate and a hard cap per project.

### Evaluation of the locating approaches
| Approach | Accuracy on GUIs | Speed CPU / GPU | Training data | Deterministic | Licence | Rust / ONNX |
|---|---|---|---|---|---|---|
| Trained detector (RT-DETRv2, D-FINE/DEIM, RF-DETR, YOLOX) | Per-app: mAP@0.5 of 0.8 to 0.95 on a consistent app is typical (to be measured); poor on unseen apps. COCO: RT-DETRv2-L 53.4 AP, D-FINE/DEIM-L 54.7 AP (not UI numbers) | CPU ~60 to 250 ms small model at 640 to 960 px; GPU ~5 to 20 ms | 300 to 3000 labelled frames per app/theme family | Yes | Apache-2.0 per sources (verify weights/backbones) | ONNX export routine; `ort` or `tract` |
| Open-vocabulary detectors (OWLv2, Grounding DINO, Florence-2) | Natural-image training; weak on dense UIs and icons (no ScreenSpot-Pro figure found, verify) | Seconds CPU, 100 to 400 ms GPU | None to start | Mostly | Grounding DINO Apache-2.0, Florence-2 MIT, SAM 2 Apache-2.0 (per Roboflow, verify); OWLv2 unconfirmed; OmniParser icon_detect AGPL (no) | Florence-2 and OWL have ONNX exports; Grounding DINO awkward |
| GUI-grounding VLMs (UI-TARS, ShowUI, OS-Atlas, SeeClick, Qwen-VL) | ScreenSpot-Pro: GPT-4o ~0.8%, SeeClick 1.1%, ShowUI-2B 7.7%, OS-Atlas-4B 3.7%, OS-Atlas-7B 18.9%, Qwen2.5-VL-7B 29.0% (3B 24.2, 32B 39.4, 72B 43.6), UI-TARS-72B 38.1%, UI-TARS-1.5-7B ~49.6% (blog, verify). ScreenSpot: OS-Atlas-7B 84.1, UI-TARS-7B 91.6. Good on ordinary screens, mediocre on dense pro/legacy apps | Seconds to tens of seconds CPU (3B to 7B); ~1 to 4 s consumer GPU | None | No (sampling, version drift) | UI-TARS Apache-2.0 (verify), Qwen-VL Apache-2.0 for most sizes, ShowUI MIT (verify), OS-Atlas/SeeClick (verify) | llama.cpp GGUF via Pitwall |
| Template / feature matching (SikuliX style) | Excellent on a static UI at fixed scale/theme; fails on theme, DPI, font smoothing, language change | 1 to 30 ms (FFT NCC on a region) | One crop per element | Yes | Own code or `imageproc`/`fast_image_resize` (MIT); OpenCV avoided (heavy C++) | Pure Rust |
| OCR-anchored (text, then relative layout) | Strong on legacy UIs (text survives themes/scale); weak for pure icons and repeated labels | PaddleOCR mobile ~150 to 500 ms CPU full 1080p, ~30 to 80 ms GPU; regions much faster | None | Yes | PaddleOCR Apache-2.0; `ocrs` MIT/Apache (verify); Tesseract Apache-2.0 | `ort`; `ocrs` pure Rust |
| Hybrid layered with fingerprints (chosen) | Best: precise where cheap layers hit, learned where needed, VLM for the long tail | p50 is cache/template, a few ms | Starts at zero | Yes except VLM | Composite, all permissive | Yes (VLM via Pitwall HTTP) |

Decision rule for the spike: on 5 real legacy apps compare layers on our own screenshots by top-1 hit rate and **wrong-click rate**
(the number that matters). The detector earns its place when the same app is tested often, runs must be fast and bit-identical, the CI
runner has no GPU/Pitwall, or icons have no text.

### Detector licence table (confirm in the repos before adoption)
| Model | Licence | Facts | Verdict |
|---|---|---|---|
| **RT-DETR / RT-DETRv2** | Apache-2.0 (repo; verify weights) | v2: L 53.4 AP ~108 FPS, X 54.3 AP ~74 FPS on T4 TensorRT FP16 (arXiv 2407.17140); NMS-free, clean ONNX export | **Primary** (Fast/Balanced) |
| **D-FINE** | Apache-2.0 code and weights | DEIM-D-FINE L 54.7 AP / 124 FPS, X 56.5 / 78 FPS on T4 (Roboflow summary); D-FINE-M 52.3 | **Accuracy tier**; better localisation for click targets |
| **DEIM / DEIMv2** | Apache-2.0 (DEIM repo; DEIMv2 verify) | DEIMv2-S 50.9 AP, 11M params, 26 GFLOPs; X 57.8 AP (arXiv 2509.20787); DEIM is a recipe on D-FINE/RT-DETR | Use the recipe; DEIMv2 (DINOv3 backbone, own licence, verify) as Accurate if clean |
| **RF-DETR** (Roboflow) | Apache-2.0 per Roboflow licensing page (verify backbone weights) | DINOv2 backbone, fast fine-tuning | Candidate for Balanced; no platform dependency |
| **YOLOX** | Apache-2.0 | 2021, Nano/Tiny/S run on CPU, anchor-free, needs NMS | CPU/tiny preset; burn port target |
| PP-YOLOE (PaddleDetection) | Apache-2.0; MMYOLO's port is GPL-3 | Second DL stack | Skip |
| YOLO-NAS | Code Apache-2.0, **weights non-commercial** (Frigate docs; Roboflow page conflicts) | | **Excluded** |
| Ultralytics YOLOv5/8/11/12, YOLOv10 (THU-MIG) | **AGPL-3.0** | | **Excluded**, also OmniParser icon_detect |
| YOLOv9 (WongKinYiu) | GPL-3.0 per one source (verify) | | **Excluded** |

### Pitwall VLM candidates (via llama.cpp GGUF; licences from secondary sources (verify))
| Model | Licence | Notes |
|---|---|---|
| Qwen2.5-VL (3B/7B) / Qwen3-VL (2B to 32B) | Apache-2.0 for most sizes (3B and 72B Qwen2.5-VL used a Qwen licence, verify per size) | Primary recommendation: pixel-coordinate grounding, good OCR. Check Qwen3-VL support in Pitwall's pinned llama.cpp (`capabilities.json`). |
| UI-TARS (1.5-7B and successors) | Apache-2.0 (verify checkpoint) | Qwen2.5-VL fine-tuned for GUI grounding; best "locate" model per size; verify llama.cpp compatibility. Qwen3-VL as "describe" model. |
| Florence-2 (0.23B/0.77B) | MIT | Tiny open-vocabulary detection/OCR; not llama.cpp, run as ONNX in Rust. CPU-only bootstrap labeller (the no-Pitwall path). |
| OmniParser v2 caption model | MIT for icon_caption; **icon_detect AGPL, never use** | Reuse the idea (detect then caption crops), not the detector. |
| MiniCPM-V 2.6 / 4.x | Apache-2.0 code; weights under MiniCPM licence (free commercial after registration, verify) | Optional. |
| Gemma 3 (4B/12B vision) | Gemma Terms of Use (custom, not OSI) | Optional user-installed only, not default or bundled. |
| Newer (2026) | check | Pitwall's capability gating (`auto:vision`): ask for a capability and size class, not a model name. |

### Other building blocks
- **OCR**: PaddleOCR det+rec ONNX through `ort` (accuracy leader, multilingual); `ocrs` as small pure-Rust fallback; no Tesseract in v1.
- **Inference**: `ort` (ONNX Runtime MIT; DirectML/CoreML/CUDA/OpenVINO providers; pin and vendor by checksum, CUDA/TensorRT carry NVIDIA licences), `tract` pure-Rust CPU fallback. Pre/post-processing pure Rust.
- **Prior art and datasets**: OmniParser (same detect + OCR + caption idea, AGPL detector), Apple Screen Recognition (CHI 2021), Ferret-UI/CogAgent/SeeClick (licences, verify), ScreenAI (unreleased). Datasets Rico, VINS, WebUI, ScreenSpot (eval only), OS-Atlas data (verify per subset), GroundUI (verify): v1 trains only on the user's own recordings. Neighbours and their features: section 13.

### `LabelSource` trait
`propose(frame, context) -> Vec<Proposal{box, class?, name?, score, source}>`. Providers, all interchangeable and fused by
`sighter-core`: `Ocr`, `Template`, `Detector` (active learning), `Vlm` (Pitwall), `Human`, and the optional `A11yUia`, `A11yAx`,
`A11yAtspi`, `Dom` (CDP, reusing Paddock's `chromium-driver`). Runs never import them.

## 9. Family integrations
- **Debrief**: a run produces `run.mp4` (from capture frames), a step timeline JSON and a clock-offset record; "Open run in Debrief" imports via the video feature (ADR 0033) with steps as markers so a failed step at 10:42:07 sits beside the app's log lines. Needs a small versioned file contract (Paddock ADR). Shared `paddock-video` crate.
- **Pitwall**: yes, Sighter talks to Pitwall for vision. Contract in 9.1.
- **Pitcrew**: runs engineers' scripts on a runner that has `sighter-server` as a service plus an interactive logged-in desktop session (headless Windows services cannot capture or drive the desktop); Pitcrew verifies this, warns, and shows Sighter health. JUnit and artifacts (videos, overlays). Unreviewed-dependent flows are refused.
- **Dyno**: CPU/GPU/memory samples during a run attached to the report; "step slow because the machine was throttling" via Dyno's throttling detector. Optional.
- **Paddock**: `paddock-ui`, `paddock-notify`, `paddock-service`, `paddock-cli`, `chromium-driver`, proposed `paddock-video`.

### 9.1 Sighter to Pitwall contract (vision)
**What Sighter sends**: Pitwall's OpenAI-compatible `/v1/chat/completions` with images inline (base64), under a Pitwall app key for Sighter
(pairing code, key in the OS credential store, local only), at low job priority with cancel. Five request kinds, temperature 0:
1. **Label suggestions**: "list the interactive elements" on a distinct frame -> boxes + classes (feeds the review queue).
2. **Element naming**: a numbered grid sheet of crops -> a name per crop ("Settings (gear)").
3. **Grounding fallback**: "where is X" -> box, only when cheaper layers fail (never the default for unattended runs).
4. **Step descriptions**: before/after frames + the click -> "Click the gear icon in the toolbar to open Settings" for reports and search.
5. **Screen-state descriptions**: a short name and summary for a screen ("Save as dialog"), used for fingerprints and `assert_screen`.

**Recommended Pitwall presets/aliases**: `sighter-label` (describe/list, e.g. Qwen3-VL-4B/7B class) and `sighter-ground` (pixel grounding, e.g.
UI-TARS-1.5-7B or Qwen2.5-VL-7B). Sighter asks by capability (`vision`, `auto:vision`) and alias, never by model name.

**Pitwall work needed**:
1. A structured grounding helper under `/x/v1` (e.g. `/x/v1/ground`) taking `{image, query}` and returning boxes in **pixel coordinates of the
   sent image**, normalising the models' conventions (Qwen-VL absolute or 0-1000 relative, UI-TARS own scheme; verify each model card).
2. Image limits and downscaling rules (max long edge, tile and overlap helper, size caps with a clear error), reported in `capabilities.json`.
3. Batch image requests (several crops/frames per call, per-item results).
4. A response cache for repeated frames (key: image hash + prompt + model + prompt version, temperature 0), which suits Pitwall well and makes replays deterministic.
5. Job priority, cancellation and queue visibility for long label runs; unload/reload of the VLM around Sighter training (section 12).

**Model fit on 8 GB**: at most a 7B 4-bit model (about 5 to 6 GB); Pitwall's memory management unloads it while Sighter trains.
**Without Pitwall**: everything works (record, OCR, template, detector, training, serving); you lose automatic naming, step descriptions and the
VLM fallback, and the review queue picks up the slack (Florence-2 ONNX optionally bootstraps boxes on CPU).

## 10. Safety and privacy
- **Never a blind click**: low confidence pauses (attended) or fails (unattended) with a screenshot and candidates; confidence gates per class (section 5).
- **`act` guards**: off by default; per-key permission; bound to a window/process; deny-list of titles ("Delete account", UAC, Keychain prompts); no keystrokes into password fields; refuses secure-desktop surfaces; dry-run and step-by-step modes; max-actions cap; global panic hotkey (default Ctrl+Alt+Shift+Esc) and a visible "Sighter is driving" border; physical input pauses the run.
- **Recorder privacy**: redaction regions applied before storage; auto-pause on password fields; typed key text not stored by default (keys recorded as events without characters unless the user opts in per project); consent screen before input capture; permission preflight per OS.
- **Data**: recordings and datasets stay in the project folder; optional encrypted at rest; "delete recording but keep labels"; no network except user-requested runtime/model downloads and the user-chosen training host; VLM calls only to local Pitwall. Server: loopback only, hashed per-app keys, size caps, rate limits, logs hold no screenshots unless run logging is enabled per key.
- Per AGENTS.md: validate every IPC/HTTP argument in Rust, parsers never panic, bounded inputs (image size, frame count, regex), plugin-free module guarded by `guard::catch`, every failure mode tested.

## 11. Architecture

**Crates** (Rust workspace, Paddock conventions: pure core, thin edges, ports and adapters):
- `sighter-core`: domain types, taxonomy, fusion/confidence, dataset assembly, resolver, fingerprints, metrics (mAP, confusion), active-learning scoring, version registry. Pure. Ports: `ScreenSource`, `LabelSource`, `Detector`, `Trainer`, `Vlm`, `InputDriver`, `Store`.
- `sighter-record`: per-OS `ScreenSource` and event-capture adapters (WGC/DXGI, ScreenCaptureKit, PipeWire/XShm), out-of-process helper, optional a11y snapshot adapters.
- `paddock-video` (shared, extracted from Debrief): ffmpeg runtime, encoders, frame extraction. `sighter-frames`: sampling, scene-change, dedup (`image`, `fast_image_resize`, perceptual hash, SSIM).
- `sighter-ocr`, `sighter-infer` (`ort`/`tract`), `sighter-train` (`ManagedPython` default, `ExportOnly`, later `Burn`), `sighter-pitwall` (client + job queue).
- `sighter-server` (axum-style HTTP, OpenAPI generation), `sighter-input` (optional `act`: SendInput, CGEvent, XTEST, RemoteDesktop portal/libei, scoped), `sighter` CLI (reuse `paddock-cli`).
- `app/src-tauri` + `app/src` (Angular 22, `paddock-ui`): Home dashboard, Recordings, Label (canvas + queue), Models, Playground (drop a screenshot or capture live, type a query, see every layer's candidates and timing), Runs, Keys and permissions, Settings. Backend service by DI token with a mock for browser dev.

**Processes**: studio (Tauri); `sighter-server` service; recorder helper (out of process); training worker (sandboxed, downloaded runtime);
Pitwall (separate app, HTTP).

**Formats**: model = ONNX + `model.json` (classes, input size, normalisation, per-class thresholds, dataset hash, metrics, parent, base
weight licence; optional FP16/INT8). Project = SQLite (sessions, frames, events, boxes, sources, scores, states) + frame store
(PNG/WebP); schema-versioned, corrupt-file fallbacks per AGENTS.md. Exports COCO, YOLO txt, CVAT XML. OpenAPI 3.1 for the server.

## 12. VRAM and hardware (8 GB GPU target)
| Workload | VRAM | Notes |
|---|---|---|
| Training RT-DETR-R18 / D-FINE-S / YOLOX-S at 640 px | ~3 to 7 GB, small batches (4 to 8), AMP | Fits 8 GB; 2k frames feasible on 6 GB or CPU slowly; Accurate presets (L/X, DEIMv2) want 12+ GB, so fall back to Fast or a remote host |
| Inference detector (ONNX, 640 px) | under 1 GB | Also runs on CPU |
| OCR (PaddleOCR mobile) | a few hundred MB | CPU is fine |
| 4-bit 7B VLM (Qwen2.5-VL / UI-TARS GGUF) | ~5 to 6 GB | **Not concurrent with training**; Pitwall's memory management unloads/loads and queues jobs |
| Diffusion | not planned | Too heavy and poor at UI text/layout |

Estimates, to be confirmed in the spike. Show a hardware estimate before training and refuse to start a job that would not fit. Apple
Silicon uses MPS for training smaller models and CoreML for inference.

## 13. Feature catalogue (v1 vs Later)

Survey of neighbours (vendor pages are marketing; claims marked (verify) are unconfirmed). Eggplant/Keysight: computer vision + OCR + self-healing,
adds image variants as the app changes ([Keysight brief](https://www.keysight.com/sg/en/assets/3125-1455/solution-briefs/Complex-UI-Testing-Deliver-Seamless-Digital-Journeys-Across-Every-Platform.pdf), [press release](https://www.businesswire.com/news/home/20210211005635/en/Keysight-Technologies-Enhances-Eggplant-Digital-Automation-Intelligence-Platform-with-High-Speed-Computer-Vision)).
TestComplete: hybrid property + AI visual recognition; screenshots go to a vendor AI service (cloud, which we avoid) ([SmartBear](https://smartbear.com/news/news-releases/smartbear-advances-testcomplete-with-vision-ai/), [DZone](https://dzone.com/events/video-library/092226SmartBearTestComplete10MT)).
Tosca Vision AI: CNN + heuristics, self-healing, tests from a mockup, no trees/tab bars, VisionScript escape hatch ([docs](https://docs.tricentis.com/tosca-2026.1/en-us/content/vision_ai/vision_ai_introduction.htm)).
UI.Vision RPA: visual assertions, OCR, desktop automation via XModules, CLI for CI; core is AGPL, so reference only ([site](https://ui.vision/rpa)).
Airtest/Poco (NetEase, Apache-2.0): image locating for games, HTML reports with screen recording, device farms ([overview](https://dev.co/testing/open-source/airtest)).
Applitools Eyes: baselines, ignore/floating regions, match levels strict/layout ([docs](https://applitools.com/tutorials/guides/advanced-use-cases/match-levels-&-regions)).
Roboflow: dataset health check (class balance, missing labels), versions with augmentation, active-learning upload ([Ultralytics tutorial](https://docs.ultralytics.com/yolov5/tutorials/roboflow_datasets_integration), [Workflows block](https://inference.roboflow.com/workflows/blocks/roboflow_dataset_upload/)).
CVAT: keyframes with linear interpolation, SAM 2 tracker ([CVAT](https://www.cvat.ai/academy/track-mode), [blog](https://www.cvat.ai/blog/video-annotation-sam-2)).
Playwright codegen: records actions into scripts, role/text/test-id locators, assertion recording ([docs](https://playwright.dev/docs/codegen)).
Anthropic computer use: screenshots downscaled to about XGA and coordinates mapped back (third-party descriptions, verify against official docs).
Not confirmed by search, from general knowledge (verify): SikuliX, Ranorex, Katalon, Appium image plugin, Label Studio, OBS, Windows Steps Recorder (deprecated), UI-TARS Desktop, OmniTool.

Realistic v1 for about 3 to 6 months: everything marked v1 below, on two OSes, Windows + macOS first.

| Area | Feature | v1 / Later | Seen in |
|---|---|---|---|
| Record | Window/region/screen capture, native per-OS APIs, change-triggered VFR, lossless | v1 | OBS (ref), Eggplant |
| Record | Input event track (click/key/scroll/drag), session metadata, redaction, password auto-pause | v1 | Playwright codegen, Steps Recorder |
| Record | Video import (mp4/mkv/webm) | v1 | CVAT, Label Studio |
| Record | Headless CLI/tray recorder for CI runners and VMs | v1 | Airtest (screen recording in reports) |
| Record | Multi-monitor and per-monitor DPI handling | v1 | Eggplant, Applitools |
| Record | Remote/VM targets (RDP/VNC pixel streams) | Later | Eggplant |
| Record | Android/iOS capture (scrcpy/adb) | Later | Airtest, Appium |
| Label | Frame timeline, scene-change + near-duplicate removal | v1 | CVAT, Roboflow |
| Label | Box + polygon editor, taxonomy, "same as...", keyboard review queue | v1 | Label Studio, CVAT |
| Label | Auto-suggestions with source/confidence (OCR, template, detector, Pitwall) | v1 | Label Studio pre-annotation, OmniParser |
| Label | Box-from-click suggestions | v1 | (own idea) |
| Label | Tracking with interpolation | v1 | CVAT |
| Label | SAM 2 mask tracking | Later | CVAT |
| Label | Optional a11y label sources (UIA/AX/AT-SPI/DOM) | Later (Windows UIA first) | Tosca/TestComplete hybrid |
| Dataset | Splits by session, COCO/YOLO export, COCO/YOLO/Label Studio import | v1 | Roboflow, Label Studio |
| Dataset | Dataset health: class balance, near-duplicates, tiny/overlapping boxes, missing labels | v1 | Roboflow |
| Dataset | Dataset versions/snapshots, augmentation, synthetic compositing | v1 | Roboflow |
| Dataset | Variation coverage view (DPI/theme/language gaps) | v1 | (own idea) |
| Dataset | CVAT XML import/export | Later | CVAT |
| Train & evaluate | Managed training, presets, export-only, remote SSH host | v1 (SSH Later) | Roboflow |
| Train & evaluate | mAP, confusion matrix, hard cases, held-out session test | v1 | Roboflow |
| Train & evaluate | Active learning (uncertainty to review queue) | v1 | Roboflow |
| Train & evaluate | Per-app model registry, versions, promote/rollback, compare | v1 | Roboflow, Tosca |
| Serve & locate | Layered resolver, `/v1/locate`, `locate_all`, `wait_for`, `ocr`, `capture`, `flag` | v1 | Eggplant, Tosca, SikuliX |
| Serve & locate | Determinism, model pinning, screen-state fingerprints | v1 | (own idea) |
| Serve & locate | Relative queries (near, inside, nth), OCR text anchors | v1 | Eggplant, UI.Vision |
| Serve & locate | Mockup-based tests (locate from a design image) | Later | Tosca |
| Serve & locate | `act` (guarded) | Open question 3 | UI.Vision, Airtest |
| Script & test | Generated clients (Python, TS) and CLI | v1 | Appium, Playwright |
| Script & test | Script skeletons from a recording (Python/TS/C#/Robot from the click sequence) | v1 Python/TS, Later C#/Robot | Playwright codegen, Airtest IDE |
| Script & test | Visual assertions, baselines with ignore regions and match levels (`assert_screen`) | v1 | Applitools, UI.Vision |
| Script & test | Self-healing: re-resolve through later layers, propose a label/template update when an element moved (explicit, never silent) | v1 proposal only | Eggplant, Tosca, TestComplete |
| Script & test | Flow recorder/replayer, natural-language steps | Later | Eggplant, Airtest |
| Report & debug | Playground (every layer's candidates, timings), run folders with overlays | v1 | Airtest reports |
| Report & debug | Flaky-locator report (hit rate per layer/element, healed counts) | v1 | Tosca, TestComplete (verify) |
| Report & debug | JUnit/JSON reports, step descriptions via Pitwall | v1 | Airtest |
| Report & debug | Debrief bundle (video + step timeline) | v1 | (family) |
| Integrations | Pitwall (9.1), Pitcrew runner template, Dyno samples | v1 | (family) |
| Integrations | Spotter hook (run smoke on new release), Robot library | Later | (family) |
| Admin & safety | Loopback keys, rate limits, run retention, redaction, encrypted-at-rest option | v1 | (own) |
| Admin & safety | Team model packs on a shared folder/NAS, starter model | Later | Roboflow |

## 14. Risks
- **Licences**: exclusions in section 8 (Ultralytics, YOLOv10, YOLOv9, icon_detect, YOLO-NAS weights, MMYOLO). Check DINOv2/DINOv3, Gemma, MiniCPM, CogAgent licences, COCO-weight provenance, PyTorch (BSD), python-build-standalone (MPL/PSF), CUDA redistribution (download on request, never redistribute). Add `cargo deny` and a runtime-manifest licence report. UI.Vision core is AGPL: reference only.
- **OS permissions**: macOS Screen Recording/Accessibility prompts, signing/notarisation, no sandbox; Windows UIPI blocks injection into elevated windows, DRM content captures black; Wayland forbids silent capture and input (portals, libei), X11 works; AT-SPI needs enabling. Preflight per OS.
- **Detector limits**: text-only elements and near-identical controls need OCR/context; theme shifts, animations, UI version drift (active learning, versions); COCO numbers do not transfer, benchmark our own.
- **Label effort**: pure vision means more human review than the accessibility-first design; mitigations are click-derived boxes, tracking, "same as...", template auto-apply.
- **Accessibility-source quality** (when enabled): stale boxes, DPI, virtualised lists, Electron exposing nothing; pixel-verified.
- **Safety and privacy**: section 10; recordings contain secrets, `act` is dangerous.
- **Scope**: recorder, studio and server are three products; v1 is desktop and browser, one OS pair per phase.
- **8 GB ceiling**: Accurate presets and concurrent VLM + training do not fit; handled by gating and queueing.

## 15. Open questions for the user
1. Is a downloaded sandboxed Python/PyTorch **training runtime** an acceptable exception to "Rust + Angular only" (data, not shipped code), Burn-native later?
2. Generated clients in a separate `sighter-clients` repo (Python, TS, C#, Robot), or OpenAPI + CLI + examples only?
3. Is `/v1/act` in v1 or Later (default off, guarded either way)?
4. First OS pair and targets: Windows + macOS, then Linux X11, then Wayland; desktop + browsers only, mobile Later?
5. How important is unattended CI (interactive logged-in session on runners; implications for Pitcrew)? Is recording on CI runners wanted, or studio-only recording?
6. The no-Pitwall path: must Florence-2 ONNX + OCR + review be good enough for v1, or may Pitwall be a hard dependency for imported videos?
7. Thresholds: auto-accept 0.85, flag below 0.6, impact sorting; or stricter (never auto-accept without two agreeing sources)?
8. Extract Debrief's video crate into `paddock-video` now (affects Debrief mid-rename), or copy and consolidate later?
9. May we ever ship a starter model trained on third-party datasets, or strictly the user's own recordings?
10. Safety acceptance: panic key, scope binding, overlay, deny-list; and "no unattended run of flows with unreviewed dependencies" as a hard rule?
11. Recorder consent: is input capture (clicks/keys) opt-in per project, with key text off by default? Hard-require redaction regions on first run? Auto-pause on password fields acceptable as best effort only?
12. Recorder storage: PNG/WebP frames by default vs 4:4:4 video; retention and encryption defaults for recordings that show real data?

## 16. Size (one capable agent team, rough, excluding polish and design pass)
- Spike (2 to 3 weeks): recorder on one OS, 500 labelled frames, train RT-DETRv2 + D-FINE, ONNX in `ort`, measure mAP, wrong-click rate, latency on CPU/GPU, resolver layers on 5 legacy apps.
- Recorder (capture, events, encoding, preflight, video import): large, 4 to 6 weeks (three OS, Wayland hardest).
- Studio labelling (timeline, editor, tracking, review queue, import/export): large, 6 to 8 weeks; the canvas and queue are the biggest UI.
- Managed training runtime + evaluate + versioning: 4 to 6 weeks, mostly cross-platform packaging.
- Server, resolver, OpenAPI, CLI, clients, integrations, docs: 5 to 7 weeks.
- Total about 6 to 8 months of one team, roughly 60 to 90k lines Rust plus 25 to 40k lines Angular/TS. A thin slice (record, label, train, locate on one OS) is about 3 months.

## 17. Sources
- RT-DETRv2: https://arxiv.org/pdf/2407.17140 ; DEIMv2 (DINOv3): https://arxiv.org/pdf/2509.20787 ; DEIM repo: https://github.com/Intellindust-AI-Lab/DEIM
- Roboflow compare pages: https://playground.roboflow.com/models/compare/d-fine-vs-deim ; https://playground.roboflow.com/models/compare/rf-detr-vs-yolo-nas ; https://playground.roboflow.com/models/megvii/yolox
- Roboflow licensing: https://roboflow.com/licensing ; YOLO guide: https://blog.roboflow.com/guide-to-yolo-models/amp
- Frigate commit on YOLO-NAS weights: https://git.laurivan.com/Mirrors/blakeblackshear.frigate/commit/0f0b2687af2341f368a948b8501865259b2f082e
- PP-YOLOE/MMYOLO licence thread: https://github.com/open-mmlab/mmyolo/discussions/977
- OmniParser v2 licences: https://huggingface.co/yuxin111222/OmniParser-v2.0 ; https://replicate.com/microsoft/omniparser-v2
- Datasets: https://huggingface.co/datasets/OS-Copilot/OS-Atlas-data ; https://huggingface.co/datasets/yyyang/UI-Grounding-Benchmarks/blob/main/ScreenSpot/README.md ; https://huggingface.co/datasets/rootsautomation/RICO-ScreenAnnotation
- UI grounding: https://arxiv.org/pdf/2410.11871 (TinyClick) ; https://arxiv.org/pdf/2503.04730 (WinClick) ; ScreenSpot-Pro https://arxiv.org/pdf/2504.07981 ; GUI-Actor https://arxiv.org/pdf/2506.03143 ; UIPro https://arxiv.org/pdf/2509.17328
- UI-TARS cards: https://huggingface.co/ByteDance-Seed/UI-TARS-2B-SFT ; https://huggingface.co/ByteDance-Seed/UI-TARS-72B-SFT ; 1.5-7B figure (blog, verify): https://lilting.ch/en/articles/ui-tars-gui-agent
- Qwen2.5-VL by size (routing paper): https://arxiv.org/pdf/2603.12823 ; Qwen3-VL GGUF: https://huggingface.co/Qwen/Qwen3-VL-32B-Thinking-GGUF/discussions
- Grounding DINO / SAM 2 / Florence-2: https://playground.roboflow.com/models/idea-research/grounding-dino ; https://playground.roboflow.com/models/meta/sam-2 ; https://playground.roboflow.com/models/compare/florence-2-vs-grounding-dino
- `ort`: https://docs.rs/crate/ort/latest/source/ ; ocrs: https://src.opensuse.org/pool/ocrs/src/branch/factory/ocrs.spec ; ocr-rs: https://docs.rs/crate/ocr-rs/2.3.4/source/README.md
- CVAT docs (MIT, LGPL ffmpeg): https://docs.cvat.ai/docs/getting_started/overview/
- Ghost name collision: https://glama.ai/mcp/servers/hcu78gk1x6
- SendInput/UIPI: https://learn.microsoft.com/tr-tr/windows/win32/api/winuser/nf-winuser-sendinput
- libei: https://gitlab.freedesktop.org/libinput/libei/-/issues/34 ; https://software.opensuse.org/package/libei
- macOS permissions: https://developer.apple.com/forums/thread/794253 ; https://developer.apple.com/forums/thread/820594
- Family context: debrief/docs/adr/0033-video-sources-ffmpeg-runtime.md ; pitwall/docs/features.md (vision capability gating, `auto:vision`)
- Feature survey (section 13): Keysight, Tricentis, SmartBear, UI.Vision, Airtest, Applitools, Roboflow, CVAT, Playwright links inline there.
- Capture APIs named in section 4 (Windows.Graphics.Capture, ScreenCaptureKit, xdg-desktop-portal ScreenCast) are from general platform knowledge; add doc links when verifying.

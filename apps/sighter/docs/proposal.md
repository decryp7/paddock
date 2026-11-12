Status: proposal, open questions pending

# Proposal: Sighter, a local vision locator server for UI automation

Date 2026-10-23, revision 4 plus user decisions of the same day (see Decisions): core idea, one locator-style client API, own tests in v1 (feature research pass 2: test layer, plans, remote sessions, family rules; supersedes revisions 1 to 3). Research proposal, nothing built. Items marked (verify) come
from secondary sources (Roboflow catalog pages, mirrors, blogs, forum posts) and must be checked against the official repo or paper
first. Speeds and accuracy figures are published or order-of-magnitude, not measured on our data; a week-1 spike must measure.

## Core idea (user decision, 2026-10-23)

UI automation testing needs computer vision, so a fast model like YOLO; an LLM is too slow but helps classify data to train the model; then Sighter runs the model and exposes an API for engineers to create test cases in their own way (C# unit tests, Java or C++) by calling the API.

Refinements:
- **Identity comes from detector + OCR + layout anchors**, not from the detector alone: the detector gives class and box, OCR gives text, layout anchors (near, inside, nth) disambiguate repeats.
- **A permissive YOLO-style detector**: RT-DETR, D-FINE or YOLOX (Apache-2.0/MIT). Ultralytics YOLO is AGPL and is excluded (section 8).
- **Active-learning loop on UI change**: when the UI changes, low-confidence or unknown results are flagged, the LLM/VLM (Pitwall, optional) proposes labels, a human reviews them, and the model is retrained.
- **Training for variation**: DPI, theme and size variation (section 6) so one model survives scaling, dark/light and resized windows.
- **Two ways to test, one engine**: engineers' own code tests through the API, and tests Sighter authors and runs itself (section 7.8), both on the same models and resolver.

## Decisions (user, 2026-10-23)
- Core idea: fast permissive detector + OCR/layout identity; the LLM only helps label; the API is the product.
- **One client API style: Playwright-style locators**, generated in C#, Java, Python, TypeScript and C++ over OpenAPI (7.9).
- Sighter authors and runs its own tests in v1; `act` is required for those (7.8).
- AI-assisted recovery through Pitwall: diagnose-only by default, recover opt-in, reported as "passed with recovery" (7.10).
- Works alongside any UI test framework (FlaUI, pywinauto, Appium, Robot) as an add-on (7.11); multi-application tests (7.12); pure-vision legacy apps from C# (7.13).
- Desktop automation through an MCP server and an agent mode over Pitwall's chat API (7.14); a `Target` interface with Local and RDP in v1, controller PC model, IP-KVM Later (7.15).
- **Amendment 2026-11-12 (user, cut review of 2026-11-12): ONNX Runtime is Microsoft's official build with telemetry switched off, plus an egress test; Sighter does not build its own ORT.** This replaces the earlier decision (2026-10-29, `sighter/sw5-ort`) to build ONNX Runtime ourselves with `--no_telemetry` and to use no Microsoft or pyke prebuilts, and the rule "DirectML is bundled only if an ETW session shows no events". Sighter links the pinned, checksummed official `ort` binaries, calls `DisableTelemetryEvents()` at start-up (Microsoft's documented switch for the Windows TraceLogging provider, ORT `docs/Privacy.md`), and a release **egress test** (the backup the user had already approved) fails the release if the process opens a connection to a non-loopback address during inference. Saves the ORT build pipeline and a rebuild at every ORT update (review: about 3 to 5 days). Risk accepted: trust in Microsoft's documented switch, checked by the egress test. Earlier text below is history.
- **Agent mode is in v1** (user, 2026-10-23): `sighter do "<goal>"` with Pitwall as the local planner ships in v1, not Later (7.14). Pitwall stays optional: without it agent mode shows a clear "needs Pitwall (or an MCP host that brings its own model)" state, and scripted runs and the MCP server work with no LLM.

## 1. Pitch

Sighter is "Pitwall for UI vision": a local server that engineers call from their own test scripts to find things on a screen. Send a
screenshot (or ask it to capture a window) and a query ("the Save button", text "Export", the third checkbox under "Options"); it answers
with boxes, confidence and which layer found it. **It works from pixels alone**, so it covers legacy apps with no or broken accessibility:
old desktop tools, custom-drawn UIs, remote sessions, games. Accessibility trees (UIA, AX, AT-SPI, DOM) are an optional label source that
speeds up labelling; nothing at run time depends on them. It is a built-in optional module, not a plugin process.

Five parts:
1. **Recorder**: captures windows/screens plus input events from the studio or headlessly on test machines.
2. **Studio** (Tauri + Angular, Label Studio/CVAT-like): label, review, train, evaluate and version a computer-vision model **per app**.
3. **Server** (`sighter-server`) with a layered resolver. Engineers write unit tests and scripts against `/v1/*`, via generated clients or CLI.
4. **Test layer**: imports test plans (`.sighterplan`) from the codebase tool or by hand, builds an app state graph with coverage, runs data-driven matrices and reports with requirement traceability (7.1 to 7.7). Remote sessions (Citrix, RDP, VNC viewer windows) are a first-class target (7.6). Pitwall is optional throughout (section 9).
5. **Sighter's own tests** (v1): author tests in the studio (record, step editor, data tables, sub-flows), store them as versionable TOML files and run them from the studio or headless with `sighter run` (7.8).

Everything is local: loopback only, no cloud, no telemetry. A fast permissive YOLO-style detector is the core; it is an interchangeable backend inside the resolver (detector + OCR + layout anchors), which is what engineers actually call.

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

Above the resolver sit a `Target` layer (Local, RDP, later VNC and IP-KVM; 7.15), an MCP server with its safety guard and an agent runner (7.14); see the component diagram there.

![Sighter overview: record, label, train, serve, test, results, with the retrain and recovery loops](images/sighter-overview.svg?v=51430f13)

The containers behind it (planned):

![Sighter containers](images/diagrams/svg/c4-2-containers.svg?v=1b8e84ed)

The lifecycle of one label, from suggestion to model and back to review:

![Element label lifecycle](images/diagrams/svg/uml-state-element-label.svg?v=41a65dc5)

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
| **Managed sandboxed Python/PyTorch runtime, downloaded on request** (recommended) | Like Debrief's FFmpeg/llama.cpp runtimes (ADR 0009/0033): pinned bundle (python-build-standalone + torch wheels per OS/accelerator + vendored training code), SHA-256 pinned, self-tested on a generated mini dataset, out-of-process worker over the one IPC, no network, only the dataset readable and output writable, memory/time caps, cancel. CUDA torch is 2 to 3 GB, so not in the installer. Python is downloaded data, not shipped app code; needs an ADR exception clause for the "Rust + Angular only" rule. **User decision 2026-10-24: Python is acceptable for training when there is no viable Rust choice** (the ADR records the exception; inference in the app stays Rust). |
| Export-only | Always provided: COCO + YOLO txt + generated script + README, for users with their own GPU box or who reject a Python runtime. |
| User-chosen remote host over SSH | Same runtime image by checksum, dataset pushed, weights pulled. No third-party cloud. |

Accelerators: CUDA (Windows/Linux), ROCm (Linux, best effort), Apple MPS (smaller models, op gaps fall back to CPU), CPU everywhere.
Import of an externally trained ONNX model with a compatibility check (input size, output layout, class list).

## 7. The server API, clients and test layer

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
- `POST /v1/act` (optional, **off by default**, per-key `act` permission, scope-bound to a window/process, deny-list, dry-run, panic key): click, double_click, right_click, type, press, drag, scroll, or "locate then click". Many teams prefer their own driver, so every other endpoint works without it. **Sighter's own tests (7.8) need `act`** (its guarded input driver performs their actions), so `act` is a v1 feature; the guards apply unchanged.

**Determinism and model pinning**: same image bytes + query + model version + server version -> identical result (fixed ONNX runtime
thread settings, FP32 reference path option, stable candidate ordering by score then position). Responses carry `model_version`,
`server_version`, `resolver_version`. `model:"app@3"` pins a version; unpinned requests use the app's `current`; promotion in the studio
never changes a pinned request. VLM answers are cached by key, marked `layer:"vlm"` and flagged for review, so replays become deterministic.

**Screen-state fingerprints** (downscaled perceptual hash + layout hash from detector/OCR output) name screens ("Login", "Save as
dialog"), constrain searches ("inside this dialog"), power `assert_screen`/`wait_for` and let cache/template layers skip work.

**Run artefacts**: optional per-key logging of each call (screenshot, query, candidates, layer, timings) into a bounded run folder
(`runs/<run_id>/<seq>-*.png/json`, retention caps, redaction applied before storing). `X-Sighter-Run: <id>` groups calls. A run folder
opens in the studio and exports a Debrief bundle.

**Clients**: OpenAPI is the contract. Thin generated MIT clients live **outside the app repo** in `sighter-clients`, one API style (locators, 7.9) in the same shape for **C#/.NET** (NuGet), **Java** (Maven Central), **Python** (pytest fixtures, `httpx`), **TypeScript** (Playwright/Node) and **C++** (header-only over a small HTTP/JSON dependency), with a Robot Framework keyword library wrapping the Python client and the Rust `sighter` CLI shipped with the app (`sighter serve|locate|wait-for|assert|ocr|record|run|models`). The app itself contains no non-Rust code. Each client ships a tiny example (7.9).

**Performance targets** (goals to confirm in the spike; p50, 1080p screenshot in memory): cache under 5 ms; template under 30 ms; OCR
anchor under 100 ms with a region, 150 to 500 ms full screen on CPU (cache OCR by fingerprint); detector under 50 ms on a discrete/Apple
GPU at 640 px (5 to 20 ms inference + 10 to 25 ms capture/pre-processing), "under 250 ms at 640" on an 8-core CPU for a small model
(dense 1080p at 960 to 1280 px reaches 300 to 800 ms); VLM 1 to 5 s on GPU, 10+ s on CPU, never in those budgets.

### 7.1 Test model: the app state graph
Screens are nodes, actions are edges, both built from recordings and run history (this is the Eggplant DAI idea, without the cloud: DAI models
states and actions as a graph, with the engine choosing paths and a coverage heatmap showing how often states and actions were hit
([DAI docs](https://docs.eggplantsoftware.com/dai/DAI%2026.1/dai-using-eggplant-dai), [coverage report](https://docs.eggplantsoftware.com/dai/DAI%207.2/dai-coverage-report))).
- **Nodes** are screen fingerprints (section 7, perceptual + layout hash), merged by similarity so "Login" with a typed username is still one node. The studio proposes names (Pitwall `screen-state` description, or a human name; non-AI path: window title + dominant OCR heading); a human confirms and merges.
- **Edges** are recorded actions (click on element X, press key, wait) from node A to node B, with the resolved query (not coordinates), a timing sample and an observed-count. Edges carry guards where seen (data-dependent: "after valid login").
- **Built from** recordings (each session is a walk), runs (observed transitions add edges and counts) and `.sighterplan` expected screens (section 7.2). The graph is a derived, versioned artefact in the project (SQLite tables), pure data in `sighter-model`.
- **Coverage**: screens reached / known, transitions exercised / known, per run set and over time; "unreached screens" (in the graph or plan, not visited by any passing run), "untested transitions", and 2-step and 3-step coverage like DAI's Extended type (counts of action sequences).
- **Path generation** (pure, deterministic given a seed): shortest path to a target screen, all-edges cover (Chinese-postman style) and random walk with weights; each path is emitted as a normal script/flow so it runs through the same resolver. Paths that need unreviewed elements or unrecorded edges are listed as "needs recording", never guessed.
- **Drift detection**: a run that lands on an unknown fingerprint creates a candidate node, flagged for review (also the self-healing signal for navigation).
- v1 ships graph building, coverage and "unreached/untested" reports plus shortest-path and all-edges generation; weighted/learned path selection is Later.

### 7.2 Test plans in: the `.sighterplan` import
The codebase tool (name pending) reads requirements and acceptance criteria and drafts UI test cases; it hands them to Sighter as a **`.sighterplan` file**.
Sighter never trusts it: it is parsed once at the edge into typed values (size caps, unknown keys rejected with a message naming the line), then
resolved against what Sighter already knows. Plain TOML (same family as profiles; reviewable in a pull request). Sketch (v1 of the schema):

```toml
format = "sighterplan"
version = 1
[plan]
id = "checkout-2026-10"            # stable; re-import updates by id
title = "Checkout"
app = "shopfront-win"              # Sighter project/app
generated_by = { tool = "pacenotes", revision = "a1b2c3d", at = "2026-10-23T10:00:00Z" }

[[requirement]]
id = "REQ-142"                     # id from the tracker or the codebase tool; free text, max 64 chars
title = "Guest checkout"
acceptance = ["AC-1: a guest can pay by card", "AC-2: totals include tax"]
source = "docs/requirements/checkout.md#guest"   # a link back, never file contents

[[screen]]                         # screens the plan expects to exist
name = "cart"                      # matched to a known fingerprint by name; else an unknown screen
description = "Cart page with line items and a Checkout button"

[[case]]
id = "TC-142-1"
title = "Guest pays by card"
requirements = ["REQ-142"]         # AC refs allowed: "REQ-142#AC-1"
priority = "high"                  # low|normal|high
tags = ["smoke"]
data = "cards"                     # optional parameter table (7.4)
  [[case.step]]
  action = "click"                 # click|type|press|select|wait_for|assert_screen|assert_text|wait
  target = { text = "Checkout", class = "button", inside = "cart" }
  [[case.step]]
  action = "type"
  target = { name = "Card number", class = "textbox" }
  value = "{{data.card}}"          # secrets only by reference: {{secret:NAME}}, never inline
  [[case.step]]
  action = "assert_screen"
  expect_screen = "order-confirmed"
  timeout_ms = 8000
```

**Import behaviour** (a dry-run report first, then apply):
- Steps resolve to **known** (target found in the model/templates or an existing screen), **ambiguous** (several matches) or **unknown**.
- Every unknown screen or target becomes a **"record this screen" task** in the studio (a to-do list with the plan step, the requirement id, the description and a suggested walk through the graph to reach the screen). Cases with open tasks are **Needs recording** and will not run unattended.
- Recordings made from a task are linked back: recording -> task -> step -> case -> requirement id(s). Linking is stored as ids in the project; re-importing a changed plan shows a diff (new, changed, removed steps) and keeps links where the case id and step text still match.
- Guarded: a plan can ask only for the same actions as scripts; `{{secret:NAME}}` resolves from the OS credential store at run time and is never written into reports; destructive-looking steps follow the `act` deny-list. The importer never executes anything.
- Export back: results per case (pass/fail/blocked/needs-recording, evidence links) as `.sighterresult` plus the standard reports (7.5) so the codebase tool can show requirement coverage. The schema lives in a small shared crate (`paddock-testplan`) used by both tools so they cannot drift (open question 10). Hand-written plans are valid; the codebase tool is just one author.
- Without the codebase tool or Pitwall, plans are written by hand or exported from the studio's flow editor; nothing about plan import needs AI.

### 7.3 Test management and traceability
- **Requirement ids everywhere**: cases, steps, recordings, runs and results carry requirement ids; the studio has a traceability matrix (requirement x case x last result x last run) with filters for "requirement without case", "case without recording", "failing".
- **Test runs**: a run set = cases + environment (OS, DPI, theme, language, resolution, model version) + results; history per case, flakiness (pass/fail flips, retries), run-to-run comparison.
- **Results out (v1)**: JUnit XML (de facto CI format, Pitcrew ingests it), TRX (Visual Studio / Azure DevOps), Allure results directory (`*-result.json` plus attachments; format per secondary sources, verify against allurereport.org), and Sighter's own JSON with evidence.
  Requirement ids go in JUnit `properties` and Allure labels so tools can link them.
- **Push to TestRail / Xray (Later)**: opt-in, user-supplied URL and token in the credential store, user-initiated or per-run. TestRail's CLI maps JUnit `test_id` properties or an automation id to cases; Xray accepts JUnit on `POST /api/v2/import/execution/junit?projectKey=...`, and Xray keeps requirement links natively in Jira ([TestRail mapping](https://support.testrail.com/hc/en-us/articles/12989737200276), [Xray via Qt Test Center](https://www.qt.io/quality-assurance/blog/test-center-4.3-release-xray-cloud-integration-and-traceability-as-code); both secondary, verify against vendor API docs).
  Both are a user-chosen network egress to the user's own server and so are off by default and explained in Settings; screenshots are not uploaded unless the user ticks it. Batch calls to respect rate limits (TestRail is reported at 180 requests/minute, verify).

### 7.4 Data-driven tests and run matrices
- **Parameter tables** (CSV/TOML/inline) feed `{{data.col}}` into steps; one row = one iteration with its own result; secrets columns are references into the credential store. Row selection by tag.
- **Matrices**: a run set expands over axes (resolution, DPI 100/125/150/200 %, theme light/dark/high contrast, language/locale, OS), each with its own model/template variants. The recorder's session metadata (section 4) defines which axes have coverage; the matrix report shows "cell not covered by training data" so a failure there is attributed to the model rather than the app.
- Matrices run on the machines that can produce the cell (a runner with that DPI/theme; Sighter can set theme/DPI only through the OS or a test hook, otherwise the cell is "needs a runner"). Pitcrew runner labels carry the axes.

### 7.5 Visual regression and assertions (`assert_screen` completeness)
Baselines per (app, screen, OS, DPI, theme, language); anything below is configuration, not code.
- **Match levels** like Applitools (strict, content, layout; exact for pixel-identical) ([docs](https://applitools.com/tutorials/guides/advanced-use-cases/match-levels-&-regions); level names for content/exact (verify)): `exact`, `strict` (ignore anti-aliasing and colour noise below a threshold), `layout` (structure and positions via the detector/OCR boxes, ignoring text content), `content` (text via OCR, ignoring styling).
- **Regions**: ignore, floating (may move within a bound), layout-only, and "dynamic" regions (clock, counters, avatars) found automatically by comparing several baseline captures; regions are defined by box, by element query or by named element so they follow the UI.
- **Tolerance** per level (pixel colour distance, fraction of pixels), anti-aliasing and sub-pixel text shift handling, and a size limit on the diff.
- **Workflow**: diff overlay and side-by-side/slider, "accept as new baseline" with a reason (a reviewed, versioned change, never automatic), baseline history, per-theme baselines created from the matrix.
- **Result**: score, status (match, match with ignored, mismatch, new screen), regions that differ, and the OCR text that changed. Used by `assert_screen`, plans (`assert_screen` steps) and the model graph (a node is "stable" when its baseline passes).

### 7.6 Virtual desktop, Citrix, RDP and VNC as first-class targets
UiPath built its Computer Vision product mainly for virtual desktops such as Citrix, where selectors are missing or unreliable: it sends an image of the window to a neural network that labels elements, using anchors and "relative to" offsets ([UiPath docs](https://docs.uipath.com/activities/other/latest/ui-automation/computer-vision-activities)).
Automation Anywhere's AISense targets Citrix, RDP and legacy toolkits (Delphi, Qt, Flex) the same way ([docs](https://docs.automationanywhere.com/bundle/enterprise-v11.3/page/enterprise/topics/aae-client/metabots/md-in-enterprise-client/aisense-overview.html)).
Power Automate Desktop switches its recorder to image mode in Citrix and RDP sessions ([June 2021 update](https://powerautomate.microsoft.com/ar-sa/blog/power-automate-desktop-june-2021-update/)).
Sighter is pixels-only, so this is its natural home, not an extra. The `Target` interface and the embedded RDP client of 7.15 build on this section.
- **Target type `remote_session`**: the thing captured is the local client window (mstsc/RDP, Citrix Workspace, VMware Horizon, VNC viewer, VM console) or a VNC/RDP pixel stream; the query and models treat it as a window with a "remote" flag.
- **What differs**: lossy codecs (blur, banding, text edges: lower template tolerance, OCR on 2x upscaled crops, train with JPEG/H.264 augmentation and record on the same codec settings), variable latency (`wait_for` defaults longer, change detection waits for N stable frames), scaling/high-DPI mismatches between client and session (store both scales), keyboard layout and key mapping (type via the viewer's input path, offer a clipboard-free typing mode), and reconnect/lock screens (recognised as screens in the graph).
- **Input**: `act` targets the client window with the usual guards; coordinates are mapped client-window -> session using the recorded geometry. Where the viewer filters injected input (UIPI, secure-attention sequences), it fails with a message and a next step instead of clicking elsewhere. (Details by viewer: verify per product.)
- **Recording on the host**: the recorder captures the viewer window; nothing is installed in the remote machine (no agent, no extension), which is the point for locked-down VDI.
- **v1**: capture the viewer window of RDP/Citrix/Horizon/VNC and run everything on it, with a "remote codec" augmentation preset and the longer stability defaults. **Later**: a direct RDP/VNC pixel stream capture without a client window.

### 7.7 Accessibility audit, localisation and performance (by-products of what Sighter already sees)
- **Accessibility audit (Later)**: from pixels, per detected element: foreground/background contrast ratio (WCAG 1.4.3 thresholds: 4.5:1 normal text, 3:1 large text (from general knowledge, verify)), pointer target size against WCAG 2.2 SC 2.5.8 (at least 24 by 24 CSS px, with a spacing exception; [W3C](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html)); focus-ring visibility by diffing before/after Tab; text-over-image warnings. Useful for legacy apps that have no accessibility tree to audit. Limits: pixels do not give semantic roles, names or reading order, and DPI-to-CSS-pixel mapping needs the recorded scale. Reported as advisory findings with crops, not as conformance.
- **Localisation testing (Later; per-language runs of the same flow)**: OCR the same screen across languages and flag text that is clipped (text region touches or exceeds its container/box, ends mid-glyph, or a pseudo-localised delimiter is missing, as in pseudo-localisation checks ([OpenHarmony i18n docs](https://gitcode.com/openharmony/docs/blob/master/en/application-dev/internationalization/pseudo-i18n-testing-translation.md), [Applitools](https://applitools.com/blog/localization-testing/))), overflows or overlaps, **untranslated** (OCR language id says the source language, or text equals the baseline language string), mojibake/tofu boxes, and RTL layout mirroring (layout match level). Side-by-side screenshots per language for linguist review. Supports the pseudo-locale idea: run once with a pseudo-localised build to find truncation before translations exist.
- **Performance-ish measures (v1 basic, Later richer)**: **time-to-screen** (action timestamp to the first frame where the target fingerprint matches and stays stable), **response latency** (action to first visible change), step durations, p50/p95 per step across runs, and regression alerts against a baseline. Measured from frame timestamps, so the resolution is the capture frame interval (about 16 to 33 ms at 30 to 60 fps; the recorder switches to a fixed high rate while measuring) and the report states its resolution. Dyno CPU/GPU/memory and throttling samples overlay the timeline so "slow because the machine was throttling" is visible. Not a profiler: it measures what a user sees.

### 7.8 Sighter's own tests (v1)
Besides the API for engineers' code, Sighter can author and run test cases itself (user decision 2026-10-23). It uses the same models, resolver and run artefacts; nothing in it needs AI.

**Authoring (studio)**
- **Record a flow**: the click/key track from the recorder (section 4) becomes steps, each with a resolved query (element name or class plus text anchor), not coordinates.
- **Visual step editor** (a step target mirrors the locator API of 7.9: `class`, `name`, `text`, `right_of`/`near`/`inside`, `cell = {row, column}`; see the README example): per step, a target (element name or class plus a text anchor), an action (click, type, press, drag, scroll), an assertion (element present/absent, text, `assert_screen`) and a wait (appears/disappears, timeout).
- **Data-driven parameter tables**: a step or whole test runs once per table row (7.4).
- **Reusable sub-flows**: a named sequence (for example "log in") called from many tests, with parameters.
- **Import `.sighterplan` from Pacenotes** (7.2): imported cases become editable tests; unknown elements become record tasks.

**Storage**: tests are plain, versionable text files in the TOML test-plan format shared with Pacenotes (crate `paddock-testplan`), so they live in git next to the app, are reviewable in a pull request, and diff cleanly. Secrets are references (`{{secret:NAME}}`), never values.

**Running**
- From the studio: watch mode with an overlay showing the resolved box, layer and confidence per step.
- Headless: `sighter run <plan|folder>` on a machine or a CI runner (Pitcrew), exit code and reports for the pipeline.
- Parallel-safe per screen/session: one run owns a screen or remote session at a time; the scheduler never interleaves two runs on one desktop.
- Actions go through Sighter's own guarded input driver (`sighter-input`), so **`act` is required for Sighter's own tests** and its rules apply: per-key permission, scope binding to a window/process, panic key, deny-list, dry-run.
- Retries and waits come from the resolver (re-resolve, frame-diff stability, per-step timeouts), not from fixed sleeps.

**Results**
- JUnit, TRX and Allure export (7.3).
- Run bundles (video plus steps plus screenshots) that open in Debrief.
- Traceability back to requirement ids (Pacenotes) through the plan's requirement links.

**Two ways to test, one engine**: engineers' own code tests via the API (C#, Java, C++, Python, TypeScript) and Sighter-authored tests both go through `sighter-server`, the same pinned models and the same resolver, so results are comparable and a flagged failure feeds the same labelling queue.

### 7.9 Client API: locators
User decision 2026-10-23: **one client API style only, Playwright-style locators**, to reduce complexity and effort. Playwright's API design ([docs](https://playwright.dev/docs/locators), Apache-2.0) inspires it; no code is reused. The **same shape** is generated in **C#, Java, Python, TypeScript and C++** over the OpenAPI wire contract (section 7), which stays the contract underneath; the locator layer is client-side. The raw verbs (`locate`, `wait_for`, `assert_screen`) remain available in every client.

**Semantics**
- **Lazy locators, re-resolved on every action**: `window.GetByText("Save")` does nothing until used; each action resolves it again, so a UI that redraws between steps is handled.
- **Auto-waiting and actionability**: before an action the locator waits (within a timeout) until the target is found, **visible**, **stable** (the box has stopped moving, frame-diff based) and **enabled** (inferred from a disabled class), and checks it is not occluded.
- **Strict matching by default**: more than one match is an error (`Ambiguous`, with the candidates); choose with `.First()`, `.Last()` or `.Nth(n)`.
- **Retrying assertions**: `Expect(locator).ToHaveTextAsync("Saved")`, `ToBeVisibleAsync()`, `ToBeEnabledAsync()` retry until the timeout, so tests need no sleeps.
- **Finding**: `GetByText`, `GetByKind(Kind.Button, name: "Save")` (the detector class plus OCR name), `.Near("label")`, `.RightOf`, `.Below`, `.Within(region | window)`, `.Filter(...)`, `.ByImage(template)`, `.MinConfidence(0.8)`; `Locator.And/Or` to combine.
- **Grid helpers**: `grid.Cell(row, column)` by row and column header (7.13).
- **Actions**: `ClickAsync`, `FillAsync` (focus click plus typing), `PressAsync`, `DragToAsync`, `CaptureAsync`; session-level `Sighter.Keyboard.PressAsync("Control+C")`, `Sighter.Mouse` and `Sighter.Clipboard.ReadAsync()` for cross-app flows; all go through the guarded input driver (`act`) or the engineer's own.
- **Element info**: box, text, kind, confidence, `resolvedBy` (cache, template, OCR, detector or VLM) and `modelVersion`.
- **Sessions**: `Sighter.Launch(...)` / `Attach(processName | title | pid)` give an app with `MainWindow`; the model pin goes in the session options (`app@3`).

One `Locator.ClickAsync()` from a C# test, through the client, the HTTP API, the resolver layers and the guarded input driver:

![Locate and click an element](images/diagrams/svg/uml-seq-locate.svg?v=7b1c72d7)

```csharp
// C# (NUnit; xUnit is the same with [Fact])
[Test] public async Task SavesDocument() {
    await using var app = await Sighter.LaunchAsync(@"C:\Tools\Editor.exe", new SessionOptions { Model = "editor@3" });
    var win = app.MainWindow;
    var save = win.GetByKind(Kind.Button, name: "Save").MinConfidence(0.8);
    await Expect(save).ToBeEnabledAsync();
    await save.ClickAsync();                                                       // waits, then clicks
    await Expect(win.GetByText("Saved")).ToBeVisibleAsync();                       // retries until shown
    await win.GetByKind(Kind.TextBox).RightOf("File name").FillAsync("report.txt");
    await Expect(win.GetByKind(Kind.Grid).Cell(row: "report.txt", column: "Size")).ToHaveTextAsync("2 KB");
    var box = await save.BoundingBoxAsync();                                       // plus Confidence, ResolvedBy, ModelVersion
    await win.AssertScreenAsync("main");
}
```

```java
// Java (JUnit 5)
@Test void savesDocument() {
    try (var app = Sighter.launch("/opt/editor", SessionOptions.model("editor@3"))) {
        var win = app.mainWindow();
        win.getByKind(Kind.BUTTON, "Save").click();
        expect(win.getByText("Saved")).toBeVisible();
        win.getByKind(Kind.TEXT_BOX).rightOf("File name").fill("report.txt");
    }
}
```

```python
# Python (pytest)
def test_saves_document(sighter):
    app = sighter.launch("/opt/editor", model="editor@3")
    win = app.main_window
    win.get_by_kind(Kind.BUTTON, name="Save").click()
    expect(win.get_by_text("Saved")).to_be_visible()
    win.get_by_kind(Kind.TEXT_BOX).right_of("File name").fill("report.txt")
```

TypeScript and C++ (`win.getByText("Save").click()`, `win.get_by_text("Save").click()`) have the same shape. Method names are illustrative; the generated surface follows the OpenAPI operation names.

**What vision cannot do, stated plainly**
- No element tree: `Parent`, `Children` and tree walks do not exist; use layout conditions (`Near`, `Within`) instead.
- No hidden or off-screen elements: scroll-to-find helpers (`ScrollIntoViewAsync()`) replace them.
- No `AutomationId`: stable names come from Sighter's labels instead.
- `IsEnabled`, checked and toggle state are inferred from detector classes and can be wrong where the app draws states unusually; confidence is exposed so tests can assert on it. Value comes from OCR; expand/collapse is `act` plus a verifying locator.

**Failures** throw typed exceptions (`ElementNotFound`, `Ambiguous`, `NeedsLabelling`, `LowConfidence`, `NotActionable`) carrying the screenshot path (`ScreenshotPath`), the candidates, the review-queue link (`ReviewLink`, `/v1/flag`) and, when Pitwall is available, its diagnosis (`Diagnosis`, 7.10); **never a blind click**: an action on an ambiguous or below-threshold match is refused, not guessed.

Familiar to Playwright users; FlaUI users keep their FlaUI code and add Sighter locators where UIA cannot see (7.11).

### 7.10 AI-assisted recovery (via Pitwall)
User decision 2026-10-23: include Pitwall in the mix; if a test is stuck, go back to the LLM to find out why and try to continue. This applies to Sighter-authored tests (7.8) and, opt-in, to engineers' code tests through the clients.

![Recover a stuck step](images/diagrams/svg/uml-seq-recovery.svg?v=2a42aa9c)

**When it triggers** (failure path only):
- a step cannot resolve its target within its wait;
- the screen does not match the expected screen fingerprint;
- an unexpected window or dialog appears;
- the resolver confidence is under the threshold.

**What happens**
1. Sighter sends Pitwall's vision model (UI-TARS or Qwen-VL via `sighter-ground`, or a new `sighter-recover` preset) the current screenshot, the step's intent ("click Save in the Order dialog"), the expected screen, and the last few steps with their screenshots.
2. Pitwall answers with a **diagnosis** ("a 'Session expired' dialog covers the window", "the Save button is below the fold", "the app is still loading") and a **proposed recovery plan** from a closed action vocabulary: dismiss a dialog via a named button, scroll, wait N s, press Escape, re-locate with a new anchor, navigate back. Anything outside the vocabulary is discarded when the answer is parsed.
3. Sighter executes the plan step by step, then retries the original step.

**Guardrails**
- Only allowed actions, and **no destructive ones**: never "Delete", "Format", "Uninstall", or anything on the deny-list (section 10).
- Every action goes through the same resolver and confidence rules, so it is never a blind click; an ambiguous or low-confidence target is refused.
- Bounded attempts (for example 3) and a time budget per step and per run.
- Scope-bound windows only (the `act` scope binding applies); the panic key still works.
- `act` must be enabled for the run; without it, recovery degrades to diagnosis only.

**Honest results**: a recovered step is reported as **"passed with recovery"**, a warning state and not a clean pass. The report shows the diagnosis, the actions taken and before/after screenshots. CI chooses how to treat it: fail, warn or pass; default **warn**, so flakiness stays visible.

**Learning loop**: every recovery becomes a **proposal**: add a "handle 'Session expired' dialog" sub-flow, update the step's anchor or wait, or label new elements. The user accepts proposals in the studio; nothing changes a test silently.

**API users**: every locator client gets an opt-in `RecoveryPolicy` set on the session or a single call (`new SessionOptions { Recovery = RecoveryPolicy.DiagnoseOnly }`; `Off` / `DiagnoseOnly` / `Recover`). A failure exception always includes Pitwall's diagnosis when available, even with recovery off, which helps debugging.

**Pitwall optional**: without Pitwall, behaviour is exactly as before: fail with a screenshot. With Pitwall, the default is `DiagnoseOnly`; `Recover` is opt-in per suite.

**Debrief**: run bundles include the recovery transcript (prompt metadata, diagnosis, actions, screenshots).

**Performance**: a VLM call takes seconds, so it happens only on the failure path, never on normal steps; the 8 GB one-GPU gating of section 9 applies.

### 7.11 Alongside your UI test framework (hybrid tests)
User decisions 2026-10-23: engineers who code their tests with FlaUI can keep using FlaUI and use the Sighter API for the UI FlaUI cannot handle, and the same holds for **any other UI testing framework** (Java and Python have their own). This is a key selling point: Sighter is an **add-on, not a replacement**, for existing UIA/Appium/pywinauto/Robot suites. No suite has to be rewritten to adopt it. FlaUI is the lead example. Familiar to Playwright users; FlaUI users keep their FlaUI code and add Sighter locators where UIA can't see.

**The shared integration pattern** (works with any framework):
1. **Scope**: nearly every framework exposes an element's **bounding rectangle** and/or a **window handle (hwnd) or process id**. The adapter passes that to Sighter, which searches only inside that region or window (`region` in `locate`, or a window-bound session from 7.12). A custom-drawn grid or canvas inside an otherwise accessible window is the typical case.
2. **Fallback helper** ("framework first, vision when it misses"): `FirstOf(frameworkQuery, sighterLocator)` tries the framework, falls back to vision when it throws or finds nothing, and logs which one answered, so a team sees where its locators fail.
3. **Shared input in one coordinate space**: framework mouse/keyboard and Sighter actions both use the OS input, so they interleave safely when the adapter does all coordinate conversion in one helper with per-monitor DPI awareness (the same helper as 7.12). Sighter returns the centre in screen coordinates; the adapter converts for the framework when it needs window-relative ones.
4. **Shared waits**: Sighter's auto-waiting and `Expect` work on either side; `FirstOf` accepts both in one wait.
5. **Reports**: a Sighter step inside a framework test still produces screenshots and run-bundle entries, through the test runner's context hook (`ITestContext` for NUnit/xUnit, a JUnit extension, a pytest fixture, a Robot listener).

Adapters are thin client-side layers over the HTTP API (section 7); the locator API of 7.9 is reused. Frameworks are third-party peer dependencies, never bundled; the adapters are MIT and share no code with them.

**Frameworks checked** (status and licence from the GitHub/PyPI/npm records on 2026-10-23; (verify) where I could not confirm from an official record):

| Framework | Language | Status and licence | Scope from the framework | Sighter adapter | v1 / Later |
|---|---|---|---|---|---|
| FlaUI (UIA2/UIA3) | C# | Active (pushed 2026-08), MIT ([repo](https://github.com/FlaUI/FlaUI)) | `AutomationElement.BoundingRectangle`; `Window` / process id | `Sighter.FlaUI` | **v1** |
| WinAppDriver | C# / any (WebDriver server) | MIT, but last push 2025-04: effectively unmaintained ([repo](https://github.com/microsoft/WinAppDriver)); Appium recommends its Windows driver | element rect via WebDriver | `sighter-appium` | Later (recipe; do not depend on it) |
| Appium Windows Driver / NovaWindows driver | any | Active (pushed 2026-10), Apache-2.0 ([windows](https://github.com/appium/appium-windows-driver), [nova](https://github.com/AutomateThePlanet/appium-novawindows-driver)) | element rect, window handle | `sighter-appium` | **v1** |
| TestStack.White | C# | Legacy, last push 2023-07, MIT or Apache-2.0 dual ([licence](https://github.com/TestStack/White/blob/master/LICENSE.txt)) | UIA element bounds | `Sighter.FlaUI`-style recipe | Later (recipe) |
| Ranorex | C# / VB.NET | Commercial; vendor docs only (verify) | element `Element.ScreenRectangle` (verify) | recipe | Later |
| Appium Java client | Java | Active (pushed 2026-10), Apache-2.0 ([repo](https://github.com/appium/java-client)) | `WebElement.getRect()` | `sighter-appium` (Java) | **v1** |
| Selenium WebDriver | Java/Python/JS/C# | Active, Apache-2.0 ([repo](https://github.com/SeleniumHQ/selenium)) | `getRect()` of web elements; for web parts of hybrid apps | `sighter-appium` pattern (same rect scope) | Later (recipe) |
| AssertJ Swing (successor of FEST Swing) | Java | Active (pushed 2026-01); licence per the repo, Apache-2.0 for FEST lineage (verify); FEST itself dead since 2015 | component `getBounds()` + `getLocationOnScreen()` | recipe | Later |
| TestFX | Java (JavaFX) | Last push 2024-02, slow; licence MIT/Apache-style (verify) | node `localToScreen(getBoundsInLocal())` | recipe | Later |
| Marathon / Jemmy | Java (Swing/AWT) | Jemmy v2 repo is archived (OpenJDK, GPL-2.0) ([repo](https://github.com/openjdk/jemmy-v2)); Marathon status (verify) | component bounds | recipe | Later |
| SikuliX Java API | Java | MIT, active (repo moved to oculix-org, pushed 2026-07) ([repo](https://github.com/RaiMan/SikuliX1)); image-based | match region | `sighter-appium`-style drop-in (see positioning) | Later |
| Winium | C# / Java (WebDriver) | Legacy, last push 2023-12, MPL-2.0 ([repo](https://github.com/2gis/Winium)) | element rect | none | Later (recipe only) |
| pywinauto (win32 + UIA backends) | Python | Active (release 2026-10), BSD-3-Clause ([repo](https://github.com/pywinauto/pywinauto)) | `element.rectangle()`, `handle`, `process_id()` | `sighter-pywinauto` | **v1** |
| `uiautomation` (yinkaisheng) | Python | Active enough (2.0.29, 2025-08), Apache-2.0 ([repo](https://github.com/yinkaisheng/Python-UIAutomation-for-Windows)) | `control.BoundingRectangle`, `NativeWindowHandle` | `sighter-pywinauto` pattern | Later |
| PyAutoGUI | Python | Stale (0.9.54, 2023-05), BSD-3-Clause ([repo](https://github.com/asweigart/pyautogui)); coordinates and image matching | none (screen coordinates) | `sighter-pyautogui` drop-in | Later |
| Appium Python client | Python | Active (pushed 2026-10), Apache-2.0 ([repo](https://github.com/appium/python-client)) | `element.rect` | `sighter-appium` (Python) | **v1** |
| Selenium / Playwright for Python | Python | Active, Apache-2.0 ([playwright](https://github.com/microsoft/playwright)) | `bounding_box()` | recipe | Later |
| Robot Framework + libraries: FlaUILibrary (MIT, release 2026-10), RPA Framework Windows (Apache-2.0, 2026-05), AppiumLibrary (Apache-2.0, 2025-12), SikuliLibrary (Apache-2.0, 2026-05) | Robot (Python/.NET) | Active ([FlaUILibrary](https://github.com/GDATASoftwareAG/robotframework-flaui), [RPA Framework](https://github.com/robocorp/rpaframework)) | each library's element/region keywords | Robot `SighterLibrary` (keywords over the Python client) | **v1** |
| Playwright / WebdriverIO | JS/TS | Active, Apache-2.0 / MIT ([playwright](https://github.com/microsoft/playwright), [wdio](https://github.com/webdriverio/webdriverio)); web and Electron | `boundingBox()` / `getLocation()`+`getSize()` | `sighter-appium` (JS) | Later (WDIO also speaks Appium) |
| nut.js | JS/TS | Desktop automation with image matching; licence and maintenance changed over time (verify on the official repo) | screen regions | `sighter-nutjs` | Later |
| Squish | C++ (scripts in Python/JS) | Commercial (Qt/Froglogic), vendor docs only (verify) | object `screenRectangle` (verify) | recipe | Later |
| Qt Test | C++ | Part of Qt (LGPL/GPL/commercial, verify) | `QWidget::mapToGlobal(rect)` | raw-region recipe | Later |
| Raw UIA COM | C++ | Microsoft API | `CurrentBoundingRectangle`, `CurrentNativeWindowHandle` | raw-region recipe | Later |
| Appium Mac2 driver (XCUITest) | any | Active (pushed 2026-10), Apache-2.0 ([repo](https://github.com/appium/appium-mac2-driver)) | element rect | `sighter-appium` | Later (macOS phase) |
| AT-SPI / dogtail | Python (Linux) | dogtail active (release 2026-10); licence GPL-style (verify) | `node.position`/`size` | recipe | Later (Linux phase) |
| LDTP | Python | Legacy, last push 2021-03, LGPL-2.1 ([repo](https://github.com/ldtp/ldtp2)) | | none | Later (recipe only) |

**v1 adapters**: `Sighter.FlaUI` (C#), `sighter-pywinauto` (Python), `sighter-appium` (Java and Python; the JS flavour follows), and the Robot Framework `SighterLibrary`. Everything else is **Later**, with a documented recipe (a few lines using the rectangle or hwnd the framework gives you) rather than a package. Appium is the common base for Java, Python and JS users, so one `sighter-appium` design (element rect or session window as scope) covers all three language bindings.

**Examples**

```csharp
// C# + FlaUI (NUnit)
using var app = Application.Launch("Editor.exe");
using var automation = new UIA3Automation();
var window = app.GetMainWindow(automation);
window.FindFirstDescendant(cf => cf.ByAutomationId("Menu")).Click();             // UIA is fine here (FlaUI code unchanged)
var grid = window.FindFirstDescendant(cf => cf.ByAutomationId("CustomGrid"));    // custom-drawn: UIA sees one pane
await grid.Sighter().GetByText("Total").ClickAsync();                            // vision, inside the grid's rectangle
await Expect(window.Sighter().GetByText("Saved")).ToBeVisibleAsync();           // whole FlaUI Window
await FirstOf(window.FindFirstDescendant(cf => cf.ByAutomationId("Save")),       // UIA first, vision when it misses
              window.Sighter().GetByKind(Kind.Button, name: "Save")).ClickAsync();
```

```python
# Python + pywinauto
from pywinauto import Application
from sighter_pywinauto import sighter          # adapter, illustrative names
app = Application(backend="uia").connect(title_re="Editor.*")
dlg = app.window(title_re="Editor.*")
canvas = dlg.child_window(auto_id="Canvas")    # one UIA pane
sighter(canvas).get_by_text("Export").click()  # search inside canvas.rectangle()
dlg.child_window(title="Save", control_type="Button").click()
```

```java
// Java + Appium (JUnit 5)
WebElement pane = driver.findElement(AppiumBy.accessibilityId("CustomGrid"));
Sighter.on(driver).within(pane.getRect()).getByText("Total").click();           // scope from the framework
firstOf(driver.findElement(AppiumBy.accessibilityId("Save")), Sighter.on(driver).getByText("Save")).click();
```

(All adapter names and methods above are illustrative; the generated surface follows the OpenAPI operation names and 7.9.)

**Java** also covers Swing (AssertJ Swing, Marathon/Jemmy), JavaFX (TestFX) and web parts (Selenium) by the rectangle recipe: component bounds to screen coordinates, then `locate` with `region`. **C++** (Qt Test, Squish, raw UIA COM) uses raw coordinates and regions the same way.

**Positioning against image-based tools** (SikuliX, PyAutoGUI, nut.js, SikuliLibrary): they locate by template images or pixel colours, which breaks on DPI, theme, font and minor UI changes. Sighter replaces brittle template matching with trained detectors plus OCR and layout anchors, and the active-learning loop repairs it when the UI changes. Their users do not have to migrate at once: the same script can call Sighter for the lookups that keep breaking and keep SikuliX/PyAutoGUI/nut.js for input or for the lookups that work, and a drop-in `find`/`click` shim (Later) maps their common calls. Sighter does not compete with the UIA/Appium frameworks; it covers what they cannot see.

### 7.12 Multi-application tests
User decision 2026-10-23: a test case may involve multiple applications running and switching.
- **Sessions**: one test can hold several sessions: `Sighter.Launch(...)` / `Attach(processName | title | pid)`, each with its own window tracking and **its own model** (per-app models in the registry, auto-selected by process name and window class), plus an **OS dialogs model** for file pickers, UAC-like prompts and message boxes, shared across apps.
- **Switching**: `app.ActivateAsync()` brings the app to the front and waits until focused, verified through the foreground-window API and the screen fingerprint. Windows from child or helper processes are handled through `app.Windows` and `WaitForWindow(title|class)`.
- **Cross-app flows**: copy in A and paste in B; drag between windows; one app launching another (for example an installer opening the product), with `WaitForProcess` and `WaitForWindow`.
- **Screen-level search**: `Sighter.Desktop.GetByText("Update available")` (a locator over all windows and monitors) searches across all windows and monitors when you do not know which app shows the thing (for example a toast or tray balloon). It is occlusion-aware: only visible pixels count, and z-order is checked before acting.
- **Multiple monitors and DPI**: per-monitor coordinates; regions are always relative to the window and converted centrally.
- **Example**: `var installer = await Sighter.AttachAsync(title: "Setup"); await installer.MainWindow.GetByText("Finish").ClickAsync(); var editor = await Sighter.WaitForAppAsync(process: "Editor.exe"); await Expect(editor.MainWindow.GetByText("Welcome")).ToBeVisibleAsync();` (each app uses its own model).
- **Recording**: the recorder follows focus across apps and tags each step with its app or window, so a recorded multi-app flow replays with the right session per step. The step editor (7.8) shows the app per step.
- **Recovery (7.10)** knows which app was expected to be in front, so "wrong app in front" is a diagnosis it can act on (activate the expected app) within the same guardrails.
- **Debrief run bundles**: steps carry the app identity, and logs from each app can be correlated with them.
- `act` scope binding (section 10) becomes a set of allowed windows/processes per run rather than one.

### 7.13 Pure-vision testing of legacy apps
User point 2026-10-23: very old C++ applications have no UIA, so Sighter may be needed completely, while the test project is still written in C# calling the Sighter API.

**The scenario**: MFC, raw Win32 or owner-drawn C++ apps (also VB6, Delphi and PowerBuilder) with no usable UIA. The test project is plain C# (NUnit/xUnit) using **only** Sighter's locator API (7.9): no FlaUI dependency, everything located by vision.

```csharp
// C# (NUnit), legacy MFC app, no UIA, no FlaUI
[Test] public async Task CreatesOrder() {
    await using var app = await Sighter.LaunchAsync(@"C:\Legacy\Orders.exe", new SessionOptions { Model = "orders@5" });
    var win = app.MainWindow;
    await win.GetByKind(Kind.TextBox).RightOf("Customer").FillAsync("ACME");                  // label anchor
    await win.GetByKind(Kind.Button, name: "Add").ClickAsync();                               // owner-drawn button
    var grid = win.GetByKind(Kind.Grid);
    await Expect(grid.Cell(row: "ACME", column: "Total")).ToHaveTextAsync("120.00");          // row/column header lookup, OCR
    await win.AssertScreenAsync("order-saved");
}
```

**Cheap helpers that are not UIA** (optional, Windows only, never required; the pixel path works without them):
- **Win32 window info**: HWND enumeration, window titles, class names and process ids. Used to identify, activate and scope windows and dialogs reliably (7.12 uses the same data), even without UIA.
- **MSAA (IAccessible)**: some old controls expose it even without UIA. It is an optional extra `LabelSource` (like the other accessibility sources, section 5), never a run-time dependency.

**Legacy-specific considerations**
- **Non-DPI-aware apps** are bitmap-scaled by Windows: record and train at the DPI the tests run at, or let Sighter normalise the scale (detect the scale factor from the window's client size versus its logical size).
- **Classic GDI fonts** make OCR very reliable (crisp, fixed fonts, no anti-aliasing variants), so OCR anchors are the main identity signal here.
- **Owner-drawn grids and lists** need **table/cell lookup** (row and column header lookup, `Cell(row, column)`); promoted from "lookup later" to **v1**, with table extract still Later.
- **Fixed-size dialogs** make layout anchors and screen fingerprints very stable.
- **Old common controls** (themed versus classic look) are two visual variants: include both in training and the theme matrix (7.4).
- **Running under RDP/VDI** is common for such apps; 7.6 applies unchanged.

**Positioning**: for these apps Sighter is the only practical automation path in the sense that UIA-based tools (FlaUI, Appium Windows) cannot see inside them. The competitors there are UiPath Computer Vision, Eggplant and SikuliX (section 13). Sighter's advantages are trained per-app models, OCR and layout identity, the C# locator API in a shape many testers already know from Playwright (7.9), and running locally with no cloud.

### 7.14 Desktop automation: MCP server and agent mode
User decision 2026-10-23: Sighter also drives desktops for people and agents, not only for test code. Three ways in, one set of tools, one safety layer.

![Desktop automation components](images/diagrams/svg/c4-3-desktop-automation.svg?v=0614eabe)

**MCP server** (user, 2026-10-23). Sighter exposes a Model Context Protocol server over stdio or loopback only. Its tools act on the session's selected target (7.15):
- `observe`: a screenshot plus the detected, labelled elements with boxes (`#1 Save button`, `#2 Cancel`, ...), so a model refers to elements by label and never has to guess pixels;
- `observe_image`: detect and label elements on a given image (for example a recorded video frame), with no input and no target needed;
- `locate` (a locator query, 7.9), `click`, `type`, `press`, `drag`, `launch_app`, `wait_for`.

Any MCP host can drive it: Sighter's own runner, Claude Desktop or Claude Code, Pacenotes, or a script. **Any app in the family can be an MCP host** (user, 2026-10-23): an app with an agent loop and a model, Pitwall by default. A host brings its own model; Sighter does not need Pitwall for this.

**Agent mode: `sighter do "<goal>"`** (user, 2026-10-23; in v1, decided the same day). A built-in loop calls Pitwall's OpenAI-compatible chat API with tool calling, using the same tool definitions, and runs the tools in-process (no MCP hop). Each step is observe, model picks one tool call, safety check, act, verify (the screen changed as expected); the loop has a step and time budget and ends when the goal is met, a budget runs out or the user stops it.

![One agent-mode step](images/diagrams/svg/uml-seq-agent-step.svg?v=201c19c3)

- **Pitwall does not speak MCP and does not change.** It stays a model server; the agent runner is the MCP-style client and the tool-calling loop lives in Sighter.
- **Pitwall is optional** (family rule, section 9): without it agent mode is unavailable and says so with a clear "needs Pitwall (or an MCP host that brings its own model)" state, with the next step (install or start Pitwall, or connect an MCP host). Scripted runs and the MCP server still work with no LLM, and an external MCP host may bring its own model.
- **Scripted automation (RPA)** through plans and the locator clients (7.8, 7.9) works with no LLM at all; it is the deterministic path and stays the default for CI.

**Safety is enforced in the MCP server, not in the client** (user, 2026-10-23), so it holds for every host, the agent runner and scripts alike:
- allow-listed apps and windows only (the `act` scope binding of section 10, a set per run);
- confirmation before destructive actions (send, delete, pay, install), by the user, never by the model;
- step and time budget per run;
- a visible "agent in control" overlay with a stop hotkey (the panic key of section 10); physical input pauses the run;
- a full replay log of observations and actions (screenshots, tool calls, verdicts), kept in the run bundle and openable in Debrief;
- local only: no cloud model, no remote MCP.

**Worked flow: reproduce a bug from a Debrief video** (user, 2026-10-23). Debrief already opens a screen recording as a source, picks key frames, aligns them to the logs and has the local model describe them. As an MCP host it can go one step further:
1. Debrief turns the video into **candidate steps**. Sighter's detector can also run on the recorded frames through `observe_image`, which names the elements that were clicked or typed into more precisely than a free-text description.
2. Debrief connects to Sighter's MCP server and selects the **RDP target** of the remote PC under test (7.15).
3. It drives the steps with Pitwall choosing the tools step by step, while **watching the remote PC's logs** (live or watched-folder import) for the same error signature or known issue.
4. It stops when the failure reproduces, or when the step or time budget runs out.
5. It saves the executed steps as a **`.sighterplan` regression test** that later runs with no LLM.

Safety is unchanged: Sighter's server enforces it (allow-list, confirmations, budget, stop key, replay log); the user confirms before anything runs on a remote PC; everything stays local.

![Reproduce a bug from a Debrief video](images/diagrams/svg/uml-seq-repro-from-video.svg?v=16ea23e7)

**Model fit.** The detector does the seeing, so the LLM only chooses among labelled elements; this is a far easier job than pixel grounding. Current local models with tool calling are enough (Qwen3.x and Gemma 4 through llama.cpp in Pitwall; verify tool-calling quality per model in the spike). A step takes seconds, so agent mode suits exploratory and one-off tasks, not the CI regression path.

### 7.15 Targets: where frames come from and input goes
User decision 2026-10-23. This extends 7.6 (remote sessions), which stays the reference for codecs, scaling and keyboard mapping; here the same idea becomes one interface.

A small `Target` interface: `frames()` (screenshots), `input()` (keyboard and mouse), optional `launch()` and file transfer. Detection, locators, plans, the MCP server and agent mode sit above it, so a test written once runs on any target.

| Adapter | How | v1 |
|---|---|---|
| **Local** | Screenshots plus SendInput (platform equivalents elsewhere), the guarded input driver of section 11 | v1 |
| **RDP** | Sighter's own embedded RDP client (FreeRDP library), headless: frames go straight to Sighter | v1 |
| **VNC** | VNC client | alongside RDP if cheap, else Later |
| **IP-KVM** | An existing open IP-KVM device (HDMI capture plus USB keyboard and pointer) | Later, optional |

**The controller PC model** (user, 2026-10-23: "perhaps hardware is too much; maybe I run Sighter on this PC and remote desktop to the PC under test"). The primary way to test what local input cannot reach is a controller PC that runs Sighter and connects to the PC under test over RDP. Because the embedded client has no mstsc window, the classic problem of a minimised RDP window that stops rendering does not apply. Works for VMs too.
- **UAC prompts and elevated apps are reachable inside the RDP session** (they are shown in it), which local SendInput cannot do: UIPI blocks injection into elevated windows and the secure desktop without an elevated helper.
- **Reboots**: Sighter reconnects after a reboot and waits for the session. The logon itself is done by RDP and NLA, so the interactive Windows logon screen and BIOS/UEFI are not tested this way.
- **One interactive session**: Windows client editions allow one interactive session, so connecting takes over the console user's session (the person at that PC is signed out of the screen); server editions and VMs are not affected the same way. Say this in the target setup screen.
- **Stable detection**: fixed session resolution and scaling per target, recorded with the model (7.6).
- **Credentials** come from the OS credential store, by reference; never in plans or logs (section 10).
- **Other routes**: RDP usually shows UAC prompts inside the session; a VNC server shows them only when it runs as a system service.

**Running and stopping a test from the controller PC** (user, 2026-10-24: "Sighter opens a remote desktop to the test PC and starts the test; a hotkey to stop the test anytime"):
- The embedded client keeps the session rendering while Sighter is in the background or minimised, so the person at the controller PC can keep working; a live view window shows the session on demand. (Driving the separate mstsc window instead breaks when it is minimised or loses focus.)
- **Stop hotkey**: a system-wide hotkey on the controller PC (default Ctrl+Alt+Shift+F12, configurable, registered with the OS so it works whichever app has focus), plus a Stop button in the overlay, the tray menu and the CLI (`sighter stop`). Stop: halts the run at once, releases every key and mouse button Sighter holds in the session, cancels queued actions, saves the run bundle (video, steps, logs) marked "stopped by user", and leaves the RDP session connected so the screen can be inspected; Disconnect is a separate action. Stop never waits on the remote PC. Tested with a fake target that hangs.
- **Multiple displays** (user, 2026-10-24): when the controller PC has more than one display, Sighter asks once which display the remote desktop's live view goes on (a quick modal choice listing the displays by name and resolution, with "remember for this target"), and opens it there; the choice is kept per target and can be changed from the live view's menu. If the PC under test has several monitors, the session uses one fixed monitor layout per target (single monitor by default; multi-monitor opt-in), and plans address a monitor by index or name, so detection stays stable.
- **Client library**: prefer **IronRDP** (Rust, MIT/Apache-2.0, Devolutions) over the FreeRDP C library so the app stays Rust-only and avoids bundling a vendor-signed C library (the macOS Team ID class of bug); verify its codec, NLA/CredSSP, clipboard and reconnect support in a spike before committing, with FreeRDP as the fallback.

**Virtual machines** (user, 2026-10-24: "Sighter can even drive virtual machines"). Two levels, both through the same `Target` interface:
- **Inside the guest**: an RDP (or VNC) target pointed at the VM, exactly like a physical test PC. Works today with nothing new.
- **At the VM console** (a `vm` target): the hypervisor's own console protocol, which also shows boot, BIOS/UEFI, the logon screen and crash screens without IP-KVM hardware: VirtualBox VRDE (RDP), Hyper-V (VMConnect console / enhanced session), QEMU/KVM and UTM (built-in VNC, SPICE later), VMware (VNC/WebMKS where enabled).
- **Lifecycle**: start, stop, **revert to a named snapshot before each run** (clean, repeatable state) and take a snapshot on failure for later inspection, through the hypervisor's CLI/API (`VBoxManage`, Hyper-V PowerShell, `virsh`/QMP, `utmctl`, `vmrun`) with timeouts, behind a small `Hypervisor` trait; credentials by reference from the OS credential store.
- **Parallel runs**: several VMs from one template, one session each, capped by the controller's CPU/RAM (Paddock rule 41).
- Phase: after RDP (v1), before IP-KVM; the guest-RDP route needs no extra work in v1. To confirm when Sighter starts.

**IP-KVM (Later, optional).** Only worth it for what RDP cannot show: BIOS/UEFI, the pre-logon screen and crash screens, on any OS with nothing installed. Prefer supporting existing open device APIs (PiKVM, GPL, so API only and no linking; Sipeed NanoKVM; JetKVM; GL.iNet KVM) over building hardware. Design notes when it is built: absolute pointer mapping, keyboard layout mapping for typed text, capture resolution, scaling and colour, about 50 to 150 ms capture latency, HDCP content shows black, no clipboard (use the device's virtual USB mass storage or a network share), multi-monitor.

**Phasing** (user, 2026-10-23): v1 = Local + RDP (controller PC model); VNC alongside if cheap; IP-KVM Later. Agent mode (user, 2026-10-23) is v1 together with the Target interface and MCP server; IP-KVM is Later.

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
- **Inference**: `ort` (ONNX Runtime MIT, Microsoft's official build with telemetry off and an egress test, no own build of ORT: see the Decisions amendment of 2026-11-12; DirectML/CoreML/CUDA/OpenVINO providers; pin and vendor by checksum, CUDA/TensorRT carry NVIDIA licences), `tract` pure-Rust CPU fallback. Pre/post-processing pure Rust.
- **Prior art and datasets**: OmniParser (same detect + OCR + caption idea, AGPL detector), Apple Screen Recognition (CHI 2021), Ferret-UI/CogAgent/SeeClick (licences, verify), ScreenAI (unreleased). Datasets Rico, VINS, WebUI, ScreenSpot (eval only), OS-Atlas data (verify per subset), GroundUI (verify): v1 trains only on the user's own recordings. Neighbours and their features: section 13.

### `LabelSource` trait
`propose(frame, context) -> Vec<Proposal{box, class?, name?, score, source}>`. Providers, all interchangeable and fused by
`sighter-core`: `Ocr`, `Template`, `Detector` (active learning), `Vlm` (Pitwall), `Human`, and the optional `A11yUia`, `A11yAx`,
`A11yAtspi`, `Dom` (CDP, reusing Paddock's `chromium-driver`). Runs never import them.

## 9. Family integrations
- **Debrief**: a run produces `run.mp4` (from capture frames), a step timeline JSON and a clock-offset record; "Open run in Debrief" imports via the video feature (ADR 0033) with steps as markers so a failed step at 10:42:07 sits beside the app's log lines. Needs a small versioned file contract (Paddock ADR). Shared `paddock-video` crate.
- **Pitwall**: optional, never required (rules below). Contract in 9.1, client in the shared crate `paddock-pitwall`.
- **Codebase tool (name pending)**: drafts UI test cases from requirements and acceptance criteria and hands them over as `.sighterplan` (7.2); reads back `.sighterresult` (requirement coverage, failing cases). File-based and CLI-based hand-off first (`sighter plan import`), no live coupling; each tool works alone. Its read-only MCP server may expose "which requirements have recordings" later (Later).
- **Pitcrew**: runs engineers' scripts on a runner that has `sighter-server` as a service plus an interactive logged-in desktop session (headless Windows services cannot capture or drive the desktop); Pitcrew verifies this, warns, and shows Sighter health. JUnit, TRX and artifacts (videos, overlays). Runner labels carry matrix axes (DPI, theme, language, remote-session capable). Unreviewed-dependent flows are refused.
- **Dyno**: CPU/GPU/memory samples during a run attached to the report; "step slow because the machine was throttling" via Dyno's throttling detector; overlay on time-to-screen charts. Optional.
- **Any app as MCP host** (7.14): for example Debrief reproduces a bug from a recording (`observe_image` on key frames, drive a remote PC through Sighter, watch its logs, save a `.sighterplan`); an app brings an agent loop and a model, Pitwall by default.
- **Pitwall as agent planner** (7.14): Pitwall's chat API with tool calling drives `sighter do`; Pitwall does not speak MCP and does not change, and agent mode is unavailable without it.
- **Pacenotes and Claude as MCP hosts** (7.14): Pacenotes, Claude Desktop/Code or any script can drive Sighter's MCP server with their own model.
- **Spotter**: new release detected -> run the smoke plan (Later).
- **Paddock**: `paddock-ui`, `paddock-notify`, `paddock-service`, `paddock-cli`, `chromium-driver`, proposed `paddock-video`, `paddock-pitwall` (shared Pitwall client: discovery, pairing, key in the credential store, priorities, cancel, caps, `ground`/chat helpers, a "needs local AI" status type; Sighter, the codebase tool, Spotter and Debrief all use it instead of four copies), `paddock-testplan` (the plan/result schema shared with the codebase tool).

### Family rules applied to Sighter
- **Pitwall is optional.** Every Pitwall-assisted feature has a non-AI path and a visible state `needs local AI` (a banner with "Install/start Pitwall" and the list of features it would enable). Nothing fails because Pitwall is absent; the review queue absorbs the work.

  | Pitwall-assisted feature | Non-AI path (always works) | Without Pitwall you lose |
  |---|---|---|
  | Label suggestions (list elements on a frame) | OCR + template + detector pre-labels + click-derived boxes + optional Florence-2 ONNX on CPU (MIT) + edge/connected-component heuristics | speed on a new app |
  | Element naming ("Settings (gear)") | OCR text of the element or its neighbours; otherwise `icon-17` with a human name in the queue; "same as..." | automatic icon names |
  | Grounding fallback at run time | none: step fails or pauses as "needs labelling" (the safe default anyway) | recovery from unseen UI changes |
  | Step descriptions in reports | templated text from the recorded step ("Click 'Checkout' (button) on Cart") | prose descriptions |
  | Screen-state names | window title + the largest OCR heading + a human name | auto names |
  | Agent mode (`sighter do`) | scripted plans and clients; the MCP server for an external host with its own model | autonomous goal-driven runs |
  | Plan drafting | the codebase tool is a separate decision; hand-written plans work | AI-drafted cases (the codebase tool's concern) |
  | Localisation language id / "is this translated" | OCR language detection (PaddleOCR lang models) and string comparison | semantic checks |
- **One GPU.** Batch labelling and description jobs go to Pitwall at **batch priority and run only when idle** (no training, no interactive Playground request, optionally only when the machine is not in use); they pause and resume, are cancellable, and show an estimate and a per-project cap. Training reserves the GPU: Sighter asks Pitwall to unload the VLM first and restores it after. Interactive `locate` never waits on a batch job.
- **Licences and privacy**: no AGPL/GPL/non-commercial models or code (section 8 table; UI-TARS Desktop and OmniParser's detector excluded as code or weights as stated there); strictly local (loopback, no cloud model, no telemetry); recordings may contain secrets (section 10: redaction before storage, no key text, secrets only by reference in plans and data tables, nothing leaves the machine except the two opt-in egresses: model/runtime downloads and the user-chosen test-management or training host); never click blindly.

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
- **Agent mode and MCP rules** (7.14, user 2026-10-23): enforced in the MCP server, never in the client, so they hold for every host. Allow-listed apps and windows; confirmation by the user for destructive actions (send, delete, pay, install); step and time budget; a visible "agent in control" overlay with a stop hotkey; a full replay log of observations and actions; stdio or loopback only, local models only. Remote targets (7.15): the user confirms before a run starts on a remote PC; RDP credentials come from the OS credential store, never from plans, prompts or logs; the model sees screenshots only through local Pitwall.
- Per AGENTS.md: validate every IPC/HTTP argument in Rust, parsers never panic, bounded inputs (image size, frame count, regex), plugin-free module guarded by `guard::catch`, every failure mode tested.

## 11. Architecture

**Crates** (Rust workspace, Paddock conventions: pure core, thin edges, ports and adapters):
- `sighter-core`: domain types, taxonomy, fusion/confidence, dataset assembly, resolver, fingerprints, metrics (mAP, confusion), active-learning scoring, version registry. Pure. Ports: `ScreenSource`, `LabelSource`, `Detector`, `Trainer`, `Vlm`, `InputDriver`, `Store`.
- `sighter-record`: per-OS `ScreenSource` and event-capture adapters (WGC/DXGI, ScreenCaptureKit, PipeWire/XShm), out-of-process helper, optional a11y snapshot adapters.
- `paddock-video` (shared, extracted from Debrief): ffmpeg runtime, encoders, frame extraction. `sighter-frames`: sampling, scene-change, dedup (`image`, `fast_image_resize`, perceptual hash, SSIM).
- `sighter-model` (state graph, coverage, path generation; pure), `sighter-plan` (`.sighterplan` parser, resolution, task list, traceability; uses shared `paddock-testplan`), `sighter-report` (JUnit/TRX/Allure/JSON writers, matrices, visual-diff levels).
- `sighter-ocr`, `sighter-infer` (`ort`/`tract`), `sighter-train` (`ManagedPython` default, `ExportOnly`, later `Burn`), `sighter-pitwall` (job queue and prompts on top of the shared `paddock-pitwall` client).
- `sighter-target` (the `Target` interface of 7.15 with the Local adapter; RDP adapter on FreeRDP, VNC and IP-KVM adapters later), `sighter-mcp` (MCP server, tool definitions and the safety guard of 7.14) and `sighter-agent` (the `sighter do` loop over the shared `paddock-pitwall` chat client; v1).
- `sighter-server` (axum-style HTTP, OpenAPI generation), `sighter-input` (optional `act`: SendInput, CGEvent, XTEST, RemoteDesktop portal/libei, scoped), `sighter` CLI (reuse `paddock-cli`).
- `app/src-tauri` + `app/src` (Angular 22, `paddock-ui`): Home dashboard, Recordings, Label (canvas + queue), Models, Playground (drop a screenshot or capture live, type a query, see every layer's candidates and timing), Runs, Keys and permissions, Settings. Backend service by DI token with a mock for browser dev.

**Processes**: studio (Tauri); `sighter-server` service; recorder helper (out of process); training worker (sandboxed, downloaded runtime);
Pitwall (separate app, HTTP); the MCP server (stdio child of a host, or loopback) and the agent runner (v1).

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

### Survey of neighbours (two passes)
Vendor pages are marketing; claims marked (verify) are unconfirmed. First pass:
Eggplant/Keysight: computer vision + OCR + self-healing, adds image variants as the app changes ([Keysight brief](https://www.keysight.com/sg/en/assets/3125-1455/solution-briefs/Complex-UI-Testing-Deliver-Seamless-Digital-Journeys-Across-Every-Platform.pdf), [press release](https://www.businesswire.com/news/home/20210211005635/en/Keysight-Technologies-Enhances-Eggplant-Digital-Automation-Intelligence-Platform-with-High-Speed-Computer-Vision)).
TestComplete: hybrid property + AI visual recognition; screenshots go to a vendor AI service (cloud, which we avoid) ([SmartBear](https://smartbear.com/news/news-releases/smartbear-advances-testcomplete-with-vision-ai/)).
Tosca Vision AI: CNN + heuristics, self-healing, tests from a mockup, no trees/tab bars, VisionScript escape hatch ([docs](https://docs.tricentis.com/tosca-2026.1/en-us/content/vision_ai/vision_ai_introduction.htm)).
UI.Vision RPA: visual assertions, OCR, desktop automation, CLI for CI; core is AGPL, reference only ([site](https://ui.vision/rpa)).
Airtest/Poco (Apache-2.0): image locating for games, HTML reports with screen recording ([overview](https://dev.co/testing/open-source/airtest)).
Applitools Eyes: baselines, ignore/floating regions, match levels ([docs](https://applitools.com/tutorials/guides/advanced-use-cases/match-levels-&-regions)).
Playwright codegen: records actions into scripts, role/text locators, assertion recording ([docs](https://playwright.dev/docs/codegen)).

Second pass, with what was confirmed and what we take from each:

| Product | Confirmed (source) | Take for Sighter | Still (verify) |
|---|---|---|---|
| **UiPath Computer Vision** | Built mainly for virtual desktops such as Citrix; sends the window image to a neural network that labels elements (buttons, text fields, checkboxes, dropdowns, text, tables); smart anchors; "Relative To" offsets; every anchor must be found at runtime; fails on repeated identical labels; variable app scale hurts relative offsets; activities live inside one `CV Screen Scope` that connects to the network server ([docs](https://docs.uipath.com/activities/other/latest/ui-automation/computer-vision-activities)) | Remote-session target (7.6); anchors and relative queries (already `near`/`inside`); table extraction (cell by row/column names, v1 `locate` on tables, Later `extract_table`); a *scope* concept = our screen-state/window binding | Whether the server is local or cloud (docs do not say); accuracy numbers |
| **Automation Anywhere AISense** | Computer vision for Citrix, RDP and legacy toolkits (Delphi, Qt, Flex); creates UI objects from an image; "resolution, scale and position independent" ([docs](https://docs.automationanywhere.com/bundle/enterprise-v11.3/page/enterprise/topics/aae-client/metabots/md-in-enterprise-client/aisense-overview.html)) | Same market: legacy toolkits are the proof the pixels-first angle is wanted | Current product state (11.3 docs are old) |
| **Power Automate Desktop** | Capture image + "Move mouse to image" (occurrences, wait timeout, tolerance, offsets); recorder switches to image mode in Citrix/RDP; image can be a **fallback** to a selector; images brittle at other zoom/DPI ([learn](https://learn.microsoft.com/en-nz/power-automate/desktop-flows/images), [fallback](https://learn.microsoft.com/en-us/power-automate/desktop-flows/ui-elements-fallback-mechanism), [troubleshooting](https://learn.microsoft.com/en-us/troubleshoot/power-platform/power-automate/desktop-flows/ui-automation/element-picker-cant-see-elements)) | Per-step settings: occurrences, timeout, tolerance, click offset; "fallback chain" idea = our resolver layers | OCR action details |
| **Squish (Qt)** | Property, image-based and OCR recognition in one tool, combinable; OCR via Tesseract by default, pluggable ([Qt](https://www.qt.io/blog/why-squish-is-the-best-choice-for-gui-testing), [6.5](https://embeddedcomputing.com/technology/debug-and-test/rf-network-testing/available-now-squish-6-5-release-with-ai-driven-ocr-support)) | Mixed recognition per step; pluggable OCR engine behind a trait | Image-based matching algorithm details |
| **Ranorex** | Image-based testing is a fallback when object recognition fails; image mode toggles inside the recorder, can be mixed ([support](https://support.ranorex.com/hc/en-us/articles/38038420878225-Image-based-Testing-Basics)) | Recorder mode toggle; mixing in one recording | |
| **Katalon Studio** | Web image locators need Enterprise licence, pixel compare against the displayed element; mobile uses Appium image finding with OpenCV ([web](https://docs.katalon.com/katalon-studio/test-objects/web-test-objects/web-image-based-testing), [mobile](https://docs.katalon.com/katalon-studio/manage-projects/set-up-projects/mobile-testing/mobile-image-based-testing-in-katalon-studio)) | Nothing new; image locators are a paid add-on there | |
| **SikuliX** | `Pattern` with similarity and target offset, `find`/`wait`/`exists`, event handlers (appear/vanish/change), OpenCV template matching, scores 0 to 1 ([basics](https://sikulix-2014.readthedocs.io/en/latest/basicinfo.html)) | Similarity + offset per target; appear/vanish/change events as `wait_for`; the classic pitfall that settings chains drop the offset (builder API must be immutable and tested) | OCR support, current maintenance |
| **AutoIt** | Not searched in detail; known for `PixelSearch`/`PixelGetColor` and window/control automation (general knowledge) | Pixel-colour probes as a cheap `assert_pixel` | everything (verify) |
| **FlaUI** | MIT .NET wrapper over UIA2/UIA3 for Win32/WinForms/WPF/Store; 5.0.0 reported Feb 2025 ([listing](https://dev.co/testing/open-source/flaui), third-party) | Reference for the optional `A11yUia` label source; we stay Rust (`windows` crate UIA), no .NET | Release/licence from the repo itself |
| **WinAppDriver** | Unmaintained for years; Appium recommends NovaWindows driver ([appium](https://github.com/appium/appium-windows-driver)) | Do not depend on it; UIA via the OS API | Exact archive status |
| **Eggplant DAI** | Model of states and actions (screens, dialogs, menus as states), transitions, parameters; learning algorithms choose paths each run; coverage heatmap, Extended coverage = sequences of three actions/states; max coverage may be below 100 % ([docs](https://docs.eggplantsoftware.com/dai/DAI%2026.1/dai-using-eggplant-dai), [coverage](https://docs.eggplantsoftware.com/dai/DAI%207.2/dai-coverage-report)) | App state graph, coverage, weighted path generation (7.1) | Learning algorithm details |
| **Testim / mabl / Functionize** | Testim Smart Locators weigh hundreds of attributes; mabl auto-heals across web/API; Functionize Architect is an AI recorder and a Create Agent from plain language (all vendor/secondary: [Momentic](https://momentic.ai/blog/ai-test-automation-tools), [Functionize](https://www.functionize.com/blog/a-beginners-guide-to-creating-tests-with-functionize-architect)). Two healing styles: stored alternative locators tried in order vs intent-based re-resolution ([Shiplight](https://www.shiplight.ai/blog/best-self-healing-test-tools), vendor) | Our resolver is already both: ordered layers and intent (query) re-resolution; healing = explicit proposals | Web-only mostly; no evidence on desktop |
| **Applitools Autonomous** | Not confirmed (search returned only Eyes/visual testing) | Plain-language test creation stays with the codebase tool + Pitwall | all (verify) |
| **TestRail / Xray** | TestRail CLI maps JUnit to cases via `test_id` or automation id; Xray imports JUnit via API and holds requirement links in Jira; batch to avoid rate limits ([TestRail](https://support.testrail.com/hc/en-us/articles/12989737200276), [Qt Test Center](https://www.qt.io/quality-assurance/blog/test-center-4.3-release-xray-cloud-integration-and-traceability-as-code), secondary) | JUnit with ids; opt-in push (7.3, Later) | Endpoint and limits against vendor docs |
| **Label Studio** | Apache-2.0 (only a listing site found), pre-annotation / ML backend | Pre-annotation review UX, import of its JSON | Licence and which active-learning parts are Enterprise-only: official repo |
| **CVAT** | MIT, bundles FFmpeg under LGPL ([docs](https://docs.cvat.ai/docs/_print)); keyframes + interpolation, SAM 2 tracker (first pass) | Tracking, import/export | |
| **Roboflow** | Dataset Upload block with probabilistic sampling for active learning ([docs](https://docs.roboflow.com/workflows/blocks/blocks/data-storage/roboflow-dataset-upload)); Health Check: class counts, object histogram, annotation heatmap ([docs](https://docs.roboflow.com/datasets/dataset-health-check)); a class is "balanced" at 75 % to 200 % of the mean count (forum, [thread](https://discuss.roboflow.com/t/what-is-the-calculation-for-a-class-being-called-over-representation-and-under-representation/564)) | Dataset health incl. annotation position heatmap and balance bands; sampled capture of uncertain run frames into the queue (active learning) | Current UI thresholds |
| **UI-TARS Desktop** | Apache-2.0 TypeScript desktop agent around UI-TARS models, macOS-first (mirrors/reviews, [repo mirror](https://github.com/uebylive/UI-TARS-desktop)) | Not a dependency (it is an agent that acts autonomously); model is a Pitwall candidate | Licence in the official ByteDance repo |
| **OmniParser / OmniTool** | Model cards: icon_detect AGPL (inherited from YOLO), captioning models (BLIP-2, Florence) MIT; OmniTool controls a Windows 11 VM ([card](https://huggingface.co/yuxin111222/OmniParser-v2.0)) | Detect+caption idea only; never the detector; VM-driven agent is out of scope | Official repo licence files |

**Insights that change the plan**: (1) the strongest commercial CV products are aimed at **virtual desktops and legacy toolkits**, so remote sessions move to v1; (2) Eggplant's differentiator is the **model**, not the locator, so a graph with coverage is added; (3) mainstream tools treat images as a **fallback**; ours is the main path, so reporting what layer found it matters; (4) every competitor couples to a cloud or licence; our local, per-app model and plain files are the differentiator; (5) healing is two techniques (ordered fallbacks, intent re-resolution) which the resolver already has.

Realistic v1 for about 3 to 6 months for the core (recorder through serve) on two OSes, Windows + macOS first; the test layer v1 items follow (section 16).

| Area | Feature | v1 / Later | Seen in |
|---|---|---|---|
| Record | Window/region/screen capture, native per-OS APIs, change-triggered VFR, lossless | v1 | OBS (ref), Eggplant |
| Record | Input event track (click/key/scroll/drag), session metadata, redaction, password auto-pause | v1 | Playwright codegen, Steps Recorder |
| Record | Video import (mp4/mkv/webm) | v1 | CVAT, Label Studio |
| Record | Headless CLI/tray recorder for CI runners and VMs | v1 | Airtest (screen recording in reports) |
| Record | Multi-monitor and per-monitor DPI handling | v1 | Eggplant, Applitools |
| Record | **Remote-session client windows (RDP, Citrix, Horizon, VNC viewer) as capture targets**, remote-codec augmentation, stability defaults | v1 | UiPath CV, AISense, Power Automate (recorder image mode in Citrix/RDP) |
| Record | Direct RDP/VNC pixel stream capture (no client window) | Later | Eggplant |
| Record | Recorder mode toggle: pixel-only vs with optional a11y track | v1 | Ranorex |
| Record | Android/iOS capture (scrcpy/adb) | Later | Airtest, Appium |
| Label | Frame timeline, scene-change + near-duplicate removal | v1 | CVAT, Roboflow |
| Label | Box + polygon editor, taxonomy, "same as...", keyboard review queue | v1 | Label Studio, CVAT |
| Label | Auto-suggestions with source/confidence (OCR, template, detector, Pitwall if present) | v1 | Label Studio pre-annotation, OmniParser |
| Label | Non-AI bootstrap (Florence-2 ONNX on CPU, OCR, click boxes) and visible "needs local AI" state | v1 | (family rule) |
| Label | Batch labelling via Pitwall at batch priority, only when idle (one GPU) | v1 | (family rule) |
| Label | Box-from-click suggestions | v1 | (own idea) |
| Label | Tracking with interpolation | v1 | CVAT |
| Label | **"Record this screen" task list** (from plans and graph gaps) | v1 | (own idea) |
| Label | SAM 2 mask tracking | Later | CVAT |
| Label | Optional a11y label sources (UIA/AX/AT-SPI/DOM) | Later (Windows UIA first) | Tosca/TestComplete hybrid, FlaUI (ref) |
| Dataset | Splits by session, COCO/YOLO export, COCO/YOLO/Label Studio import | v1 | Roboflow, Label Studio |
| Dataset | Dataset health: class balance bands, near-duplicates, tiny/overlapping boxes, missing labels, annotation position heatmap | v1 | Roboflow |
| Dataset | Dataset versions/snapshots, augmentation, synthetic compositing | v1 | Roboflow |
| Dataset | Variation coverage view (DPI/theme/language/codec gaps) | v1 | (own idea) |
| Dataset | Sampled capture of uncertain run frames into the queue (active-learning loop) | v1 | Roboflow Dataset Upload |
| Dataset | CVAT XML import/export | Later | CVAT |
| Train & evaluate | Managed training, presets, export-only, remote SSH host | v1 (SSH Later) | Roboflow |
| Train & evaluate | mAP, confusion matrix, hard cases, held-out session test | v1 | Roboflow |
| Train & evaluate | Active learning (uncertainty to review queue) | v1 | Roboflow |
| Train & evaluate | Per-app model registry, versions, promote/rollback, compare | v1 | Roboflow, Tosca |
| Serve & locate | Layered resolver, `/v1/locate`, `locate_all`, `wait_for`, `ocr`, `capture`, `flag` | v1 | Eggplant, Tosca, SikuliX |
| Serve & locate | Per-query similarity, occurrences, click offset, timeout | v1 | SikuliX, Power Automate |
| Serve & locate | Determinism, model pinning, screen-state fingerprints | v1 | (own idea) |
| Serve & locate | Relative queries (near, inside, nth, anchors), OCR text anchors | v1 | Eggplant, UI.Vision, UiPath |
| Serve & locate | Table cell lookup by row/column header (owner-drawn grids in legacy apps, 7.13), table extract | **v1 lookup** (promoted for legacy apps), Later extract | UiPath CV |
| Serve & locate | `assert_pixel` colour probe | v1 | AutoIt (verify) |
| Serve & locate | Mockup-based tests (locate from a design image) | Later | Tosca |
| Serve & locate | `act` (guarded; required for Sighter's own tests, 7.8) | v1 (decided) | UI.Vision, Airtest |
| Script & test | **Generated clients: C#, Java, C++, Python, TypeScript** (from OpenAPI) and CLI, each with a tiny NUnit/xUnit, JUnit, GoogleTest, pytest or Playwright example | v1 | Appium, Playwright |
| Script & test | Script skeletons from a recording (Python/TS/C#/Robot from the click sequence) | v1 Python/TS/C#/Java/C++, Later Robot | Playwright codegen, Airtest IDE |
| Script & test | Visual assertions, baselines (`assert_screen`) | v1 | Applitools, UI.Vision |
| Script & test | Match levels (exact/strict/layout/content), ignore/floating/layout/dynamic regions, per-level tolerance, accept-baseline workflow, per-theme/DPI baselines (7.5) | v1 | Applitools |
| Script & test | Self-healing: re-resolve through later layers, propose a label/template update (explicit, never silent) | v1 proposal only | Eggplant, Tosca, TestComplete, Testim/mabl |
| Script & test | **Sighter-authored tests**: record a flow into steps, visual step editor (target, action, assertion, wait), data tables, sub-flows (7.8) | v1 | Eggplant, Airtest, Playwright codegen |
| Script & test | **Run Sighter's own tests**: studio watch mode with overlay, headless `sighter run`, parallel-safe per screen, retries/waits from the resolver, plans as TOML in git (`paddock-testplan`) (7.8) | v1 | Eggplant, Airtest |
| Script & test | **Locator API (C#, Java, Python, TypeScript, C++)**: one Playwright-style client surface; lazy locators, auto-waiting/actionability, strict matching, retrying `Expect`, vision-only filters (`Near`, `RightOf`, `Within`, `ByImage`, `MinConfidence`), `Cell(row, column)`, typed failures, element info with `Confidence`/`ResolvedBy`/`ModelVersion` (7.9) | v1 | Playwright (API design, Apache-2.0; no code reused) |
| Script & test | **Hybrid adapters v1: `Sighter.FlaUI` (C#), `sighter-pywinauto` (Python), `sighter-appium` (Java, Python), Robot Framework `SighterLibrary`**: scope a vision search to a framework element rect/window, "framework first, vision when it misses" fallback with logging, shared waits, runner reporting hooks (7.11) | v1 | FlaUI, pywinauto, Appium, Robot (peer dependencies only) |
| Script & test | **Pure-vision legacy-app testing from C#** (MFC/Win32/VB6/Delphi/PowerBuilder, no UIA, no FlaUI): 7.9 locator API only, plus optional Win32 window info (HWND, class, pid) (7.13) | v1 | UiPath CV, Eggplant, SikuliX |
| Label | Optional MSAA (IAccessible) `LabelSource` for old controls | Later (after Windows UIA) | (own idea) |
| Script & test | Further hybrid adapters and recipes: `sighter-appium` JS, `sighter-nutjs`, `sighter-pyautogui` shim, Swing/JavaFX/Selenium/Playwright/Qt/Squish/raw-UIA/Mac2/dogtail recipes; drop-in `find`/`click` shim for SikuliX/PyAutoGUI/nut.js users (7.11) | Later | SikuliX, PyAutoGUI, nut.js |
| Script & test | **Multi-application tests**: several sessions per test, per-app models plus OS dialogs model, `Activate`/`WaitForWindow`/`WaitForProcess`, cross-app copy/paste/drag, `Sighter.Desktop` search, per-monitor DPI, focus-following recorder (7.12) | v1 | UiPath, Power Automate Desktop |
| Script & test | **AI-assisted recovery via Pitwall**: diagnose-only (default when Pitwall is present) and recover with guardrails (closed action vocabulary, no destructive actions, bounded attempts, "passed with recovery" state, `RecoveryPolicy`, diagnosis in failure exceptions, recovery transcript in Debrief bundles) (7.10) | v1 | (own idea; Functionize/mabl heal differently) |
| Targets | **`Target` interface** (frames, input, launch) with the **Local** and **RDP** adapters (embedded FreeRDP client, controller PC model: UAC and elevated apps inside the session, reconnect after reboot, fixed resolution, credentials in the OS store) (7.15) | v1 | UiPath CV, Automation Anywhere (verify) |
| Targets | VNC adapter | v1 only if cheap, else Later | (own idea) |
| Targets | IP-KVM adapter via existing open device APIs (BIOS/UEFI, pre-logon, crash screens only) | Later, optional | PiKVM, NanoKVM, JetKVM, GL.iNet (verify) |
| Automation | **MCP server** (`observe`, `observe_image`, `locate`, `click`, `type`, `press`, `drag`, `launch_app`, `wait_for`) with the safety guard (allow-list, confirmations, budget, overlay and stop key, replay log) (7.14) | v1 | (own idea) |
| Automation | **Scripted automation (RPA)** by plans and clients, no LLM | v1 | UiPath, Power Automate Desktop |
| Automation | **Agent mode** `sighter do "<goal>"` over Pitwall's chat API with tool calling (no MCP in Pitwall) (7.14) | **v1** (user, 2026-10-23); Pitwall optional, a clear "needs Pitwall" state without it | (own idea) |
| Automation | **Reproduce a bug from a Debrief video** (7.14): Sighter side (`observe_image`, MCP server, RDP target, agent runner or host-driven steps, saving a `.sighterplan`) depends only on agent mode + the RDP target | Sighter side **v1**; the Debrief side (video to candidate steps, MCP host, log watching) is Debrief's own work and ships on Debrief's schedule | (own idea) |
| Script & test | Learned recovery handlers auto-proposed across suites | Later | (own idea) |
| Script & test | Natural-language steps | Later | Eggplant, Functionize |
| Plans & model | **`.sighterplan` import** (requirements, screens, cases, steps), dry-run report, diff on re-import, unknown elements to record tasks (7.2) | v1 | (family: codebase tool) |
| Plans & model | **App state graph** from recordings/runs/plans, screen merge and naming (7.1) | v1 | Eggplant DAI |
| Plans & model | **Coverage** of screens and transitions, unreached screens, untested transitions | v1 | Eggplant DAI coverage |
| Plans & model | **Path generation** (shortest path, all-edges) emitted as flows | v1 | Eggplant DAI |
| Plans & model | Weighted/learned path selection, 2- and 3-step coverage | Later | Eggplant DAI |
| Plans & model | Drift detection (unknown fingerprint during a run) | v1 | Eggplant, Tosca |
| Test management | **Requirement traceability matrix**, ids on cases/recordings/runs | v1 | TestRail, Xray |
| Test management | **Run sets**, history, flakiness, run-to-run compare | v1 | TestRail, Allure |
| Test management | **JUnit, TRX, Allure results** export with requirement ids | v1 | TestRail CLI, Xray, Allure |
| Test management | `.sighterresult` back to the codebase tool | v1 | (family) |
| Test management | TestRail / Xray push (opt-in egress) | Later | TestRail, Xray |
| Data & matrix | **Parameter tables** (data-driven), secrets by reference | v1 | Katalon, Tosca (verify) |
| Data & matrix | **Cross-resolution / DPI / theme / language run matrices** with coverage-gap attribution | v1 | Applitools, Eggplant |
| Quality by-products | **Accessibility audit** from pixels: contrast, target size, focus ring | Later | (own idea; WCAG 1.4.3 / 2.5.8) |
| Quality by-products | **Localisation checks**: truncated/overflowing text, untranslated strings, RTL, pseudo-locale run | Later | Applitools localisation |
| Quality by-products | **Time-to-screen and response latency** (visual), step p50/p95, regression alerts, Dyno overlay | v1 basic, Later richer | (own idea) |
| Report & debug | Playground (every layer's candidates, timings), run folders with overlays | v1 | Airtest reports |
| Report & debug | Flaky-locator report (hit rate per layer/element, healed counts) | v1 | Tosca, TestComplete (verify) |
| Report & debug | JUnit/JSON reports, step descriptions (templated; Pitwall prose if present) | v1 | Airtest |
| Report & debug | Debrief bundle (video + step timeline) | v1 | (family) |
| Integrations | Pitwall (9.1) via `paddock-pitwall`, Pitcrew runner template, Dyno samples | v1 | (family) |
| Integrations | Codebase tool hand-off (`.sighterplan` / `.sighterresult`) | v1 | (family) |
| Integrations | Spotter hook (run smoke on new release), Robot library | Later | (family) |
| Admin & safety | Loopback keys, rate limits, run retention, redaction, encrypted-at-rest option | v1 | (own) |
| Admin & safety | Team model packs on a shared folder/NAS, starter model | Later | Roboflow |

## 14. Risks
- **Licences**: exclusions in section 8 (Ultralytics, YOLOv10, YOLOv9, icon_detect, YOLO-NAS weights, MMYOLO). Check DINOv2/DINOv3, Gemma, MiniCPM, CogAgent licences, COCO-weight provenance, PyTorch (BSD), python-build-standalone (MPL/PSF), CUDA redistribution (download on request, never redistribute). Add `cargo deny` and a runtime-manifest licence report. UI.Vision core is AGPL: reference only. Tesseract (Squish's OCR default) is not needed.
- **OS permissions**: macOS Screen Recording/Accessibility prompts, signing/notarisation, no sandbox; Windows UIPI blocks injection into elevated windows, DRM content captures black; Wayland forbids silent capture and input (portals, libei), X11 works; AT-SPI needs enabling. Preflight per OS.
- **Detector limits**: text-only elements and near-identical controls need OCR/context; theme shifts, animations, UI version drift (active learning, versions); COCO numbers do not transfer, benchmark our own.
- **Label effort**: pure vision means more human review than the accessibility-first design; mitigations are click-derived boxes, tracking, "same as...", template auto-apply. Plan import adds "record this screen" tasks: a large plan creates a large recording backlog, so the studio ranks tasks by requirement priority and shares walks across cases.
- **Remote sessions**: lossy codecs, latency and scaling reduce accuracy; competitors with a cloud service-side network (UiPath) have more training data than we do per app (their server location is not stated in the docs); expect lower hit rates and test them in the spike on one Citrix/RDP setup.
- **Model graph drift**: fingerprint merging can wrongly merge or split screens, silently skewing coverage. Mitigation: human confirm/merge, similarity thresholds shown, coverage reports list low-confidence merges.
- **Plan trust**: a generated plan (possibly by an LLM) may be wrong or hostile; the importer is a pure parser, plans never execute without review, secrets by reference only, destructive steps follow the deny-list.
- **Test-management push**: third-party API drift (TestRail, Xray), tokens and an extra network egress; Later, opt-in, JUnit/Allure files first.
- **Accessibility-source quality** (when enabled): stale boxes, DPI, virtualised lists, Electron exposing nothing; pixel-verified.
- **Safety and privacy**: section 10; recordings contain secrets, `act` is dangerous; evidence screenshots in reports and pushes may show secrets (redaction applies before storage and before any export).
- **Scope**: recorder, studio, server and now a test layer (plans, graph, management, matrices) are four products; v1 is desktop, remote-session windows and browsers, one OS pair per phase. The test layer is cut to the v1 items in section 13; the rest is Later. Sighter-authored tests (7.8) are now v1 and add scope; the cut line is the Later rows, not this.
- **AI recovery (7.10)**: non-determinism (a recovered run can differ from the last) and masking real bugs (a dialog that should not appear gets dismissed). Mitigated by the "passed with recovery" warning state, CI default "warn", diagnose-only as the default policy, a closed action vocabulary, bounded attempts, and proposals that need human acceptance.
- **Agent reliability (7.14)**: local models with tool calling may loop, pick wrong elements or stop early; mitigated by detector-labelled elements, per-step verification, step and time budgets, and agent mode staying off the CI path; scripted plans stay the deterministic route. Agent mode is v1 (user, 2026-10-23), so the mitigations are part of v1: the detector gives the model labelled elements instead of pixels; every step is verified (the screen changed as expected) and a failed verification stops or retries within bounds; a step and time budget ends loops; destructive actions need confirmation by the user; every run keeps a replay log; a stop hotkey ends the run at once; a per-model tool-calling check in the spike picks supported models, and a small evaluation set of goals tracks reliability.
- **Agent and MCP safety**: an agent or a host can click destructive things or be steered by text on screen (prompt injection); mitigated by enforcing allow-lists, confirmations, budgets and the stop key in the server, confirming remote runs, and never trusting screen text as instructions.
- **Capture fidelity (7.15)**: RDP codecs and scaling blur text; a controller PC takes over the console user's session on Windows client editions; reboots drop the session. Mitigated by fixed resolution and scaling, stability waits, reconnect logic and clear setup warnings. Local SendInput cannot reach elevated windows or the secure desktop.
- **Hardware variety (IP-KVM, Later)**: many devices with different APIs, latency and keyboard mapping; PiKVM is GPL (API only, no linking). Kept Later and optional; one device first.
- **8 GB ceiling**: Accurate presets and concurrent VLM + training do not fit; handled by gating and queueing (one GPU rule, section 9).

## 15. Open questions for the user (19; merged from revision 3; questions 2 and 3 narrowed by the 2026-10-23 decisions)
1. Is a downloaded sandboxed Python/PyTorch **training runtime** an acceptable exception to "Rust + Angular only" (data, not shipped code), Burn-native later?
2. Generated clients (C#, Java, C++, Python, TS are decided as first-class): in a separate `sighter-clients` repo as proposed, and is Robot Framework wanted in v1 or Later? Which C++ HTTP/JSON dependency is acceptable for the C++ client? (Decided 2026-10-23: one API style, locators, 7.9; raw verbs stay underneath.)
3. Decided (2026-10-23): `/v1/act` and the flow recorder/runner are in v1, because Sighter runs its own tests (7.8); `act` stays default off and guarded. Remaining: which guard defaults for unattended CI runs?
4. First OS pair and targets: Windows + macOS, then Linux X11, then Wayland; desktop + browsers + **remote-session client windows (Citrix/RDP/Horizon/VNC) in v1**, mobile Later?
5. How important is unattended CI (interactive logged-in session on runners; implications for Pitcrew)? Is recording on CI runners wanted, or studio-only recording?
6. No-Pitwall and starter models (merged 6+9): confirm the rule "Pitwall optional, non-AI path for everything" (table in section 9) and that Florence-2 ONNX + OCR + review is good enough for imported videos; and may we ever ship a starter model trained on third-party datasets, or strictly the user's own recordings?
7. Confidence and safety rules (merged 7+10): auto-accept 0.85 / flag below 0.6 or stricter (two agreeing sources); panic key, scope binding, overlay, deny-list; "no unattended run of flows with unreviewed dependencies or Needs-recording cases" as a hard rule?
8. Extract Debrief's video crate into `paddock-video` now (affects Debrief mid-rename), or copy and consolidate later? Same question for the new `paddock-pitwall` client crate (extract from Pitwall's or Debrief's existing client, or write fresh).
9. Recorder consent and storage (merged 11+12): input capture opt-in per project, key text off, redaction regions required on first run, auto-pause on password fields as best effort; PNG/WebP vs 4:4:4 video; retention and encryption defaults for recordings showing real data.
10. **Plan format ownership**: is `.sighterplan` (TOML, `format`/`version`, requirements, screens, cases, steps, secrets by reference; section 7.2) agreed, living in a shared `paddock-testplan` crate owned jointly with the codebase tool, with `.sighterresult` for the way back? Or should the codebase tool only emit JUnit-like text and Sighter own the schema?
11. **Test layer scope in v1**: model graph with coverage and unreached screens (7.1), traceability and JUnit/TRX/Allure export (7.3), data tables and run matrices (7.4) are proposed v1; TestRail/Xray push, accessibility audit, localisation checks and richer performance reports Later. Agree, or is any Later item wanted earlier (for example the TestRail/Xray push if your team uses one)?
12. **Remote sessions and research spike**: add one Citrix/RDP/Horizon setup (which products do you actually use?) to the week-1 spike, and which of the five legacy apps for the resolver comparison (no confidential screenshots into samples or docs)?

13. **Recovery default for CI**: with Pitwall present, is `DiagnoseOnly` the right default and `Recover` opt-in per suite, and should CI treat "passed with recovery" as warn (proposed), fail or pass?
14. **Recovery budget**: attempts per step (proposed 3), time budget per step and per run, and which actions of the closed vocabulary (dismiss dialog, scroll, wait, Escape, re-anchor, navigate back) are allowed by default.

15. **Which adapters in v1** (7.11): proposed `Sighter.FlaUI` (UIA2 and UIA3), `sighter-pywinauto`, `sighter-appium` (Java and Python) and the Robot `SighterLibrary`; the rest Later as recipes. Which frameworks do your teams actually use (Swing, JavaFX, Qt/Squish, SikuliX, nut.js), and should any of them move to v1? Adapters live in `sighter-clients` as MIT packages.
16. **Multi-application limits**: how many concurrent sessions and models per test (VRAM, section 12), and should the OS dialogs model ship as a starter model per OS (it would be trained on our own recordings of OS dialogs, no third-party data)?
17. **Legacy apps** (7.13): which legacy stacks do you actually test (MFC, VB6, Delphi, PowerBuilder), at which DPI, and are they non-DPI-aware? Is the optional Win32 window info enough, or is MSAA wanted earlier than Later?

18. **Targets and RDP** (7.15): confirm Local + RDP (controller PC model) in v1 and the one-interactive-session takeover on Windows client editions as acceptable; is VNC wanted in v1 or only if cheap; which RDP setups do you actually use (Windows client, server, VMs)?
19. **Agent mode in v1 or Later** (7.14): resolved (user, 2026-10-23): agent mode is in v1, with Pitwall optional.

## 16. Size (one capable agent team, rough, excluding polish and design pass)
- Spike (2 to 3 weeks): recorder on one OS, 500 labelled frames, train RT-DETRv2 + D-FINE, ONNX in `ort`, measure mAP, wrong-click rate, latency on CPU/GPU, resolver layers on 5 legacy apps, plus one remote-session capture to measure the accuracy loss.
- Recorder (capture, events, encoding, preflight, video import, remote-session windows): large, 4 to 6 weeks (three OS, Wayland hardest).
- Studio labelling (timeline, editor, tracking, review queue, import/export): large, 6 to 8 weeks; the canvas and queue are the biggest UI.
- Managed training runtime + evaluate + versioning: 4 to 6 weeks, mostly cross-platform packaging.
- Server, resolver, OpenAPI, CLI, clients, integrations, docs: 5 to 7 weeks.
- **Test layer (new)**: plan import and task backlog 2 to 3 weeks; app state graph, coverage, path generation 3 to 4 weeks; traceability, run sets, JUnit/TRX/Allure 2 to 3 weeks; data tables and matrices 1 to 2 weeks; visual regression completion (regions, levels, baselines UI) 2 to 3 weeks; `paddock-pitwall` crate and batch-priority jobs 1 to 2 weeks. About 11 to 17 weeks, partly parallel.
- **Sighter-authored tests (new in v1, 7.8)**: step model and TOML (de)serialisation in `paddock-testplan` 1 to 2 weeks; flow-to-steps conversion from the recorder 1 to 2 weeks; visual step editor, data tables and sub-flows in the studio 4 to 6 weeks; runner (`sighter run`, watch mode with overlay, parallel-safe scheduling, retries) 3 to 4 weeks; guarded `act` driver hardening on two OSes 2 to 3 weeks (moved from optional to required); C#/Java/C++ clients, examples and CI 2 to 3 weeks. About 10 to 15 weeks, partly parallel.
- **Locator API (7.9)** (my estimate, not the user's): one locator layer defined once and generated in C#, Java, Python, TypeScript and C++ (auto-waiting, actionability, strict matching, `Expect`, grid helpers, typed failures) 2 to 3 weeks, partly inside the 2 to 3 weeks for clients above; a single surface replaces the earlier two (raw verbs plus a FlaUI-style object model), saving roughly 1 to 2 weeks of design, docs and per-language wrappers.
- **AI-assisted recovery (7.10)** (my estimate, not the user's): `sighter-recover` preset and Pitwall contract extension 1 week; trigger detection, closed-vocabulary plan parser and guarded executor 2 to 3 weeks; "passed with recovery" state, reports, Debrief transcript and `RecoveryPolicy` in the clients 1 to 2 weeks; proposals UI in the studio 1 to 2 weeks. About 5 to 8 weeks, partly parallel; adds roughly 1 month to the total.
- **Hybrid adapters (7.11)** (my estimate, not the user's): shared adapter core (scope, fallback, DPI helper shared with 7.12, reporting hook) 1 to 2 weeks; `Sighter.FlaUI` 1 to 2 weeks; `sighter-pywinauto` 1 week; `sighter-appium` (Java and Python) 2 weeks; Robot `SighterLibrary` 1 week; recipes and docs for the Later frameworks about 1 week. About 7 to 9 weeks, partly parallel (was 3 to 4 weeks for FlaUI alone). The Later adapters would add roughly 1 to 2 weeks each.
- **Multi-application tests (7.12)** (my estimate, not the user's): multi-session model, per-app model auto-selection, OS dialogs model, activation/focus verification 3 to 4 weeks; `Desktop` search with occlusion and z-order, per-monitor DPI helper 2 to 3 weeks; focus-following recorder and per-app steps in the editor 2 to 3 weeks; Debrief identity and log correlation about 1 week. About 8 to 11 weeks, partly parallel.
- Together 7.11 and 7.12 add roughly 2 to 3 months to the total.
- **Pure-vision legacy testing (7.13)** (my estimate, not the user's): table/cell lookup and grid wrapper 2 to 3 weeks; Win32 window info helper about 1 week (shared with 7.12); non-DPI-aware scale normalisation 1 week; legacy sample app set (invented, no confidential screenshots), docs and the C# example suite 1 to 2 weeks. About 4 to 6 weeks, partly parallel; MSAA source Later, about 1 to 2 weeks.
- **Desktop automation (7.14, 7.15)** (my estimate, not the user's): `Target` interface and Local adapter refactor of the input driver 1 to 2 weeks; embedded RDP adapter (FreeRDP bindings, headless frames, input mapping, reconnect, credential store, fixed resolution) 4 to 6 weeks; MCP server, tool definitions, `observe_image` and the safety guard (allow-list, confirmations, overlay and stop key, replay log) 3 to 4 weeks. Agent mode (v1, user decision 2026-10-23): `sighter-agent` runner over the `paddock-pitwall` chat client, step verification, budgets, "needs Pitwall" state, stop-hotkey integration, plan saving, per-model tool-calling checks and a small goal evaluation set 4 to 5 weeks. About 12 to 17 weeks in v1, partly parallel. Later: VNC adapter 1 to 2 weeks; first IP-KVM adapter 3 to 5 weeks plus about 1 to 2 weeks per further device; the Debrief reproduction flow (video to steps, MCP host) is Debrief's own work on top.
- Later items: TestRail/Xray push 1 to 2 weeks, accessibility audit 2 weeks, localisation checks 2 to 3 weeks, richer performance reports 1 to 2 weeks.
- Total about 15 to 19 months of one team (includes the desktop automation v1 items of 7.14 and 7.15, agent mode among them (adds about 1 month); was 8 to 11 before the Sighter-authored tests, the extra clients, AI recovery, the FlaUI hybrid and multi-application tests), roughly 95 to 145k lines Rust plus 45 to 70k lines Angular/TS (revision 3: 6 to 8 months, 60 to 90k plus 25 to 40k). A thin slice (record, label, train, locate on one OS) is still about 3 months; the test layer follows it.

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
- Feature survey, second pass (section 13): UiPath Computer Vision https://docs.uipath.com/activities/other/latest/ui-automation/computer-vision-activities ; Automation Anywhere AISense https://docs.automationanywhere.com/bundle/enterprise-v11.3/page/enterprise/topics/aae-client/metabots/md-in-enterprise-client/aisense-overview.html ; Power Automate images https://learn.microsoft.com/en-nz/power-automate/desktop-flows/images , fallback https://learn.microsoft.com/en-us/power-automate/desktop-flows/ui-elements-fallback-mechanism , picker troubleshooting https://learn.microsoft.com/en-us/troubleshoot/power-platform/power-automate/desktop-flows/ui-automation/element-picker-cant-see-elements , June 2021 https://powerautomate.microsoft.com/ar-sa/blog/power-automate-desktop-june-2021-update/
- Squish (Qt): https://www.qt.io/blog/why-squish-is-the-best-choice-for-gui-testing , 6.5 OCR https://embeddedcomputing.com/technology/debug-and-test/rf-network-testing/available-now-squish-6-5-release-with-ai-driven-ocr-support ; Ranorex https://support.ranorex.com/hc/en-us/articles/38038420878225-Image-based-Testing-Basics ; Katalon https://docs.katalon.com/katalon-studio/test-objects/web-test-objects/web-image-based-testing and mobile https://docs.katalon.com/katalon-studio/manage-projects/set-up-projects/mobile-testing/mobile-image-based-testing-in-katalon-studio ; SikuliX https://sikulix-2014.readthedocs.io/en/latest/basicinfo.html
- Eggplant DAI https://docs.eggplantsoftware.com/dai/DAI%2026.1/dai-using-eggplant-dai , coverage https://docs.eggplantsoftware.com/dai/DAI%207.2/dai-coverage-report
- FlaUI (MIT, UIA2/UIA3; third-party listings) https://dev.co/testing/open-source/flaui ; WinAppDriver (unmaintained, Appium recommends NovaWindows driver) https://github.com/appium/appium-windows-driver and https://learn.microsoft.com/en-au/answers/questions/1455246/is-the-tool-winappdriver-dead-or-not
- AI test tools (vendor/secondary): https://momentic.ai/blog/ai-test-automation-tools , https://www.shiplight.ai/blog/best-self-healing-test-tools , Functionize https://www.functionize.com/blog/a-beginners-guide-to-creating-tests-with-functionize-architect
- Test management: https://support.testrail.com/hc/en-us/articles/12989737200276 ; https://www.qt.io/quality-assurance/blog/test-center-4.3-release-xray-cloud-integration-and-traceability-as-code ; Allure history files https://allurereport.org/docs/how-it-works-history-files/
- Roboflow dataset upload (active learning) https://docs.roboflow.com/workflows/blocks/blocks/data-storage/roboflow-dataset-upload ; health check https://docs.roboflow.com/datasets/dataset-health-check ; forum on the 75% to 200% balance rule https://discuss.roboflow.com/t/what-is-the-calculation-for-a-class-being-called-over-representation-and-under-representation/564
- CVAT licence/docs https://docs.cvat.ai/docs/_print ; UI-TARS Desktop (Apache-2.0 per mirrors/reviews) https://github.com/uebylive/UI-TARS-desktop ; OmniParser v2 licence split https://huggingface.co/yuxin111222/OmniParser-v2.0
- WCAG 2.2 target size https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html ; localisation testing https://applitools.com/blog/localization-testing/ , https://gitcode.com/openharmony/docs/blob/master/en/application-dev/internationalization/pseudo-i18n-testing-translation.md

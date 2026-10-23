Status: proposal, open questions pending

# Proposal: Sighter, a local vision locator server for UI automation

Date 2026-10-23, revision 4 plus user decisions of the same day: core idea, C#/Java/C++ clients, own tests in v1 (feature research pass 2: test layer, plans, remote sessions, family rules; supersedes revisions 1 to 3). Research proposal, nothing built. Items marked (verify) come
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

![Sighter overview: record, label, train, serve, test, results, with the retrain and recovery loops](images/sighter-overview.svg?v=3d5c7994)

The containers behind it (planned):

![Sighter containers](images/diagrams/svg/c4-2-containers.svg?v=bfb1ffff)

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
| **Managed sandboxed Python/PyTorch runtime, downloaded on request** (recommended) | Like Debrief's FFmpeg/llama.cpp runtimes (ADR 0009/0033): pinned bundle (python-build-standalone + torch wheels per OS/accelerator + vendored training code), SHA-256 pinned, self-tested on a generated mini dataset, out-of-process worker over the one IPC, no network, only the dataset readable and output writable, memory/time caps, cancel. CUDA torch is 2 to 3 GB, so not in the installer. Python is downloaded data, not shipped app code; needs an ADR exception clause for the "Rust + Angular only" rule. |
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

**Clients**: OpenAPI is the contract. Thin generated MIT clients live **outside the app repo** in `sighter-clients`. First-class, generated and versioned with the API: **C#/.NET** (NuGet), **Java** (Maven Central), **C++** (header-only over a small HTTP/JSON dependency), **Python** (pytest fixtures, `httpx`) and **TypeScript** (Playwright/Node); plus a Robot Framework keyword library wrapping the Python client and the Rust `sighter` CLI shipped with the app (`sighter serve|locate|wait-for|assert|ocr|record|run|models`). The app itself contains no non-Rust code. Each client ships a tiny example; all of them call the same three verbs:

```csharp
// C# (NUnit; xUnit is the same with [Fact])
[Test] public async Task SavesDocument() {
    var s = new SighterClient("app@3");
    await s.Locate(new Query { Name = "Save", Class = "button" }).Click();   // locate (+ act, if enabled)
    await s.WaitFor(new Query { Text = "Saved" }, timeout: 5000);           // wait_for
    await s.AssertScreen("main");                                           // assert_screen
}
```

```java
// Java (JUnit 5)
@Test void savesDocument() {
    var s = new SighterClient("app@3");
    s.locate(Query.name("Save").clazz("button")).click();
    s.waitFor(Query.text("Saved"), Duration.ofSeconds(5));
    s.assertScreen("main");
}
```

```cpp
// C++ (GoogleTest)
TEST(Editor, SavesDocument) {
  sighter::Client s{"app@3"};
  s.locate({.name = "Save", .cls = "button"}).click();
  s.wait_for({.text = "Saved"}, std::chrono::seconds(5));
  s.assert_screen("main");
}
```

(Method names are illustrative; the generated surface follows the OpenAPI operation names.) Python and TypeScript clients mirror these. C#, Java and C++ also offer a FlaUI-style object model over the same API (7.9).

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
Sighter is pixels-only, so this is its natural home, not an extra.
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
- **Visual step editor**: per step, a target (element name or class plus a text anchor), an action (click, type, press, drag, scroll), an assertion (element present/absent, text, `assert_screen`) and a wait (appears/disappears, timeout).
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

### 7.9 FlaUI-style client API
The C#, Java and C++ clients expose an object model **similar to FlaUI** ([FlaUI](https://github.com/FlaUI/FlaUI), MIT, the .NET UIA library many Windows testers know), backed by vision instead of UIA. The HTTP/OpenAPI API stays the wire contract underneath; these object models are client-side layers over it. The raw verbs (`locate`, `wait_for`, `assert_screen`) remain available in every client.

One `FindFirst(...).AsButton().Click()` from a C# test, through the client, the HTTP API, the resolver layers and the guarded input driver:

![Locate and click an element](images/diagrams/svg/uml-seq-locate.svg?v=fd576cdc)

**Familiarity, not compatibility**: not a drop-in FlaUI replacement (different namespaces and types), but familiar enough that a FlaUI user is productive at once. **Licence**: FlaUI is MIT; this is an API style, not code reuse.

| FlaUI | Sighter client | Notes |
|---|---|---|
| `Application.Launch(path)` / `Attach(process or title)` | `Sighter.Launch(...)` / `Sighter.Attach(...)` | Session options carry the model pin (`app@3`) and the act permission |
| `app.GetMainWindow(automation)` | `app.MainWindow` | The OS window APIs give the window and its capture region |
| `FindFirstDescendant(cf => ...)` / `FindAllDescendants(...)` | `window.FindFirst(cf => ...)` / `FindAll(...)` | Same `ConditionFactory`-style builder |
| `cf.ByControlType(ControlType.Button)` | `cf.ByControlType(...)` | The detector class |
| `cf.ByText("Save")` / `ByName` | `cf.ByText(...)` / `ByName(...)` | OCR text (and the label's stable name) |
| (no equivalent) | `Near("Name")`, `RightOf`, `Below`, `Inside(region)`, `Index(n)`, `ByImage(template)`, `MinConfidence(0.8)` | Vision-only conditions, combined with `.And(...)` |
| `Retry.WhileNull(...)` | `window.WaitFor(cond, timeout)`, `WaitUntilGone(cond, timeout)` | Server-side polling with frame-diff |
| `AsButton().Invoke()` | `AsButton().Click()` | |
| `AsTextBox().Enter(text)` | `AsTextBox().Enter(text)` | Focus click plus typing |
| `AsCheckBox().IsChecked` / `Toggle()` | same | State from the detector's checked/unchecked classes |
| `AsComboBox().Select("Item")` | same | Open, find the item by OCR, click, verify |
| `AsListBox` / `AsGrid` rows and cells | same | Rows and cells by layout |
| `BoundingRectangle`, `Name`, `ControlType`, `IsEnabled` | same, plus `Confidence`, `ResolvedBy`, `ModelVersion` | `Name` is OCR text, `ControlType` the class, `IsEnabled` inferred from a disabled class; `ResolvedBy` is cache, template, OCR, detector or VLM |
| `Mouse` / `Keyboard` statics | `Sighter.Mouse` / `Sighter.Keyboard` | Through the guarded input driver (`act`), or the engineer's own |
| `Capture.Element` / `Capture.Screen` | `element.Capture()` / `window.Capture()` | |

```csharp
// C# (NUnit)
using var app = Sighter.Launch(@"C:\Tools\Editor.exe", new SessionOptions { Model = "editor@3" });
var win = app.MainWindow;
var save = win.FindFirst(cf => cf.ByText("Save").And(cf.ByControlType(ControlType.Button)).MinConfidence(0.8));
Assert.That(save.IsEnabled);
save.AsButton().Click();
win.WaitFor(cf => cf.ByText("Saved"), TimeSpan.FromSeconds(5));
win.FindFirst(cf => cf.ByControlType(ControlType.TextBox).Near("File name")).AsTextBox().Enter("report.txt");
Console.WriteLine($"{save.Confidence:F2} via {save.ResolvedBy} on {save.ModelVersion}");
```

```java
// Java (JUnit 5)
try (var app = Sighter.launch("/opt/editor", SessionOptions.model("editor@3"))) {
    var win = app.mainWindow();
    win.findFirst(cf -> cf.byText("Save").and(cf.byControlType(ControlType.BUTTON))).asButton().click();
    win.waitFor(cf -> cf.byText("Saved"), Duration.ofSeconds(5));
}
```

```cpp
// C++ (GoogleTest)
auto app = sighter::Sighter::launch("/opt/editor", {.model = "editor@3"});
auto win = app.main_window();
win.find_first([](auto& cf) { return cf.by_text("Save").and_(cf.by_control_type(ControlType::Button)); }).as_button().click();
win.wait_for([](auto& cf) { return cf.by_text("Saved"); }, std::chrono::seconds(5));
```

**Patterns, approximated visually**: Value via OCR, Toggle via a state class, ExpandCollapse via `act` plus verify. **What vision cannot do, stated plainly**:
- No element tree: `Parent`, `Children` and tree walks do not exist; use layout conditions instead.
- No hidden or off-screen elements: scroll-to-find helpers (`ScrollIntoView(cond)`) replace them.
- No `AutomationId`: stable names come from Sighter's labels instead.
- `IsEnabled` and checked state are inferred from classes and can be wrong where the app draws states unusually; confidence is exposed so tests can assert on it.

**Failures** throw typed exceptions (`ElementNotFound`, `Ambiguous`, `NeedsLabelling`, `LowConfidence`) carrying the screenshot path, the candidates and the review-queue link (`/v1/flag`); **never a blind click**: an action on an ambiguous or below-threshold match is refused, not guessed.

A thin shim package mapping common FlaUI calls, to migrate existing suites, is a Later idea.

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

**API users**: the C#/Java/C++ clients (and Python/TypeScript) get an opt-in `RecoveryPolicy` (`Off` / `DiagnoseOnly` / `Recover`). A failure exception always includes Pitwall's diagnosis when available, even with recovery off, which helps debugging.

**Pitwall optional**: without Pitwall, behaviour is exactly as before: fail with a screenshot. With Pitwall, the default is `DiagnoseOnly`; `Recover` is opt-in per suite.

**Debrief**: run bundles include the recovery transcript (prompt metadata, diagnosis, actions, screenshots).

**Performance**: a VLM call takes seconds, so it happens only on the failure path, never on normal steps; the 8 GB one-GPU gating of section 9 applies.

### 7.11 Alongside FlaUI (hybrid tests)

![Alongside FlaUI: UIA first, vision where UIA cannot see](images/diagrams/svg/uml-seq-hybrid.svg?v=6c8eef45)

User decision 2026-10-23: engineers who code their tests with FlaUI can keep using FlaUI and use the Sighter API for the UI FlaUI cannot handle. This is a key selling point: Sighter is an **add-on, not a replacement**, for existing UIA/FlaUI suites. No suite has to be rewritten to adopt it.

A small C# package, **`Sighter.FlaUI`**, with FlaUI (MIT) as a **peer dependency, not bundled**:
- **Scope from UIA**: `uiaElement.Sighter().FindFirst(cf => ...)` searches only inside a FlaUI `AutomationElement`'s `BoundingRectangle` (for example a custom-drawn grid or a canvas inside an otherwise UIA-friendly window); `window.Sighter()` does the same for a whole FlaUI `Window`. The result is a Sighter element from 7.9.
- **Fallback helper**: `window.FindFirstOrSight(cf => cf.ByAutomationId("Save"), s => s.ByText("Save").And(s.ByControlType(Button)))` tries UIA first, falls back to vision, and logs which one answered (so a team sees where UIA is failing).
- **Mixed input**: both sides use the OS input. FlaUI `Mouse` and Sighter actions interleave safely because they share the coordinate space and DPI handling: per-monitor DPI awareness, with all conversions in one helper (the same helper as 7.12).
- **Shared waits**: `Retry`-style waits that accept a UIA condition, a Sighter condition or either.
- **Reports**: a Sighter step inside a FlaUI test still produces screenshots and run-bundle entries, through an `ITestContext` hook for NUnit and xUnit.
- **Java**: the equivalent for UIA-on-Java users is a hybrid with WinAppDriver/Appium-Windows sessions: the Appium element's rectangle scopes a Sighter search, and Sighter is the fallback when a locator fails. (WinAppDriver is unmaintained; Appium's Windows driver is the recommended base, section 13.)
- **C++**: hybrid through raw coordinates and regions: pass a rectangle from the UIA code to `locate` as `region`, and use the returned centre with the existing input code.

### 7.12 Multi-application tests
User decision 2026-10-23: a test case may involve multiple applications running and switching.
- **Sessions**: one test can hold several sessions: `Sighter.Launch(...)` / `Attach(processName | title | pid)`, each with its own window tracking and **its own model** (per-app models in the registry, auto-selected by process name and window class), plus an **OS dialogs model** for file pickers, UAC-like prompts and message boxes, shared across apps.
- **Switching**: `app.Activate()` brings the app to the front and waits until focused, verified through the foreground-window API and the screen fingerprint. Windows from child or helper processes are handled through `app.Windows` and `WaitForWindow(title|class)`.
- **Cross-app flows**: copy in A and paste in B; drag between windows; one app launching another (for example an installer opening the product), with `WaitForProcess` and `WaitForWindow`.
- **Screen-level search**: `Sighter.Desktop.FindFirst(...)` searches across all windows and monitors when you do not know which app shows the thing (for example a toast or tray balloon). It is occlusion-aware: only visible pixels count, and z-order is checked before acting.
- **Multiple monitors and DPI**: per-monitor coordinates; regions are always relative to the window and converted centrally.
- **Recording**: the recorder follows focus across apps and tags each step with its app or window, so a recorded multi-app flow replays with the right session per step. The step editor (7.8) shows the app per step.
- **Recovery (7.10)** knows which app was expected to be in front, so "wrong app in front" is a diagnosis it can act on (activate the expected app) within the same guardrails.
- **Debrief run bundles**: steps carry the app identity, and logs from each app can be correlated with them.
- `act` scope binding (section 10) becomes a set of allowed windows/processes per run rather than one.

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
- **Pitwall**: optional, never required (rules below). Contract in 9.1, client in the shared crate `paddock-pitwall`.
- **Codebase tool (name pending)**: drafts UI test cases from requirements and acceptance criteria and hands them over as `.sighterplan` (7.2); reads back `.sighterresult` (requirement coverage, failing cases). File-based and CLI-based hand-off first (`sighter plan import`), no live coupling; each tool works alone. Its read-only MCP server may expose "which requirements have recordings" later (Later).
- **Pitcrew**: runs engineers' scripts on a runner that has `sighter-server` as a service plus an interactive logged-in desktop session (headless Windows services cannot capture or drive the desktop); Pitcrew verifies this, warns, and shows Sighter health. JUnit, TRX and artifacts (videos, overlays). Runner labels carry matrix axes (DPI, theme, language, remote-session capable). Unreviewed-dependent flows are refused.
- **Dyno**: CPU/GPU/memory samples during a run attached to the report; "step slow because the machine was throttling" via Dyno's throttling detector; overlay on time-to-screen charts. Optional.
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
- Per AGENTS.md: validate every IPC/HTTP argument in Rust, parsers never panic, bounded inputs (image size, frame count, regex), plugin-free module guarded by `guard::catch`, every failure mode tested.

## 11. Architecture

**Crates** (Rust workspace, Paddock conventions: pure core, thin edges, ports and adapters):
- `sighter-core`: domain types, taxonomy, fusion/confidence, dataset assembly, resolver, fingerprints, metrics (mAP, confusion), active-learning scoring, version registry. Pure. Ports: `ScreenSource`, `LabelSource`, `Detector`, `Trainer`, `Vlm`, `InputDriver`, `Store`.
- `sighter-record`: per-OS `ScreenSource` and event-capture adapters (WGC/DXGI, ScreenCaptureKit, PipeWire/XShm), out-of-process helper, optional a11y snapshot adapters.
- `paddock-video` (shared, extracted from Debrief): ffmpeg runtime, encoders, frame extraction. `sighter-frames`: sampling, scene-change, dedup (`image`, `fast_image_resize`, perceptual hash, SSIM).
- `sighter-model` (state graph, coverage, path generation; pure), `sighter-plan` (`.sighterplan` parser, resolution, task list, traceability; uses shared `paddock-testplan`), `sighter-report` (JUnit/TRX/Allure/JSON writers, matrices, visual-diff levels).
- `sighter-ocr`, `sighter-infer` (`ort`/`tract`), `sighter-train` (`ManagedPython` default, `ExportOnly`, later `Burn`), `sighter-pitwall` (job queue and prompts on top of the shared `paddock-pitwall` client).
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
| Serve & locate | Table cell lookup by row/column name, table extract | v1 lookup, Later extract | UiPath CV |
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
| Script & test | **FlaUI-style object model** in the C#/Java/C++ clients: sessions/windows, `FindFirst`/`FindAll` condition builder with vision-only conditions, `WaitFor`, typed wrappers (Button, TextBox, CheckBox, ComboBox, ListBox, Grid), element properties with `Confidence`/`ResolvedBy`/`ModelVersion`, typed failures (7.9) | v1 | FlaUI (MIT, style only) |
| Script & test | **`Sighter.FlaUI` hybrid package** (C#, FlaUI as peer dependency): scope a vision search to a UIA element or window, `FindFirstOrSight` UIA-then-vision fallback with logging, shared waits, `ITestContext` reporting hook (7.11) | v1 | FlaUI (MIT, peer dependency only) |
| Script & test | Java hybrid with Appium-Windows sessions; C++ hybrid via regions and coordinates (7.11) | v1 as documented recipes, packaged helper Later | Appium |
| Script & test | **Multi-application tests**: several sessions per test, per-app models plus OS dialogs model, `Activate`/`WaitForWindow`/`WaitForProcess`, cross-app copy/paste/drag, `Sighter.Desktop` search, per-monitor DPI, focus-following recorder (7.12) | v1 | UiPath, Power Automate Desktop |
| Script & test | FlaUI migration shim package (map common FlaUI calls) | Later | FlaUI |
| Script & test | **AI-assisted recovery via Pitwall**: diagnose-only (default when Pitwall is present) and recover with guardrails (closed action vocabulary, no destructive actions, bounded attempts, "passed with recovery" state, `RecoveryPolicy`, diagnosis in failure exceptions, recovery transcript in Debrief bundles) (7.10) | v1 | (own idea; Functionize/mabl heal differently) |
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
- **8 GB ceiling**: Accurate presets and concurrent VLM + training do not fit; handled by gating and queueing (one GPU rule, section 9).

## 15. Open questions for the user (16; merged from revision 3; questions 2 and 3 narrowed by the 2026-10-23 decisions)
1. Is a downloaded sandboxed Python/PyTorch **training runtime** an acceptable exception to "Rust + Angular only" (data, not shipped code), Burn-native later?
2. Generated clients (C#, Java, C++, Python, TS are decided as first-class): in a separate `sighter-clients` repo as proposed, and is Robot Framework wanted in v1 or Later? Which C++ HTTP/JSON dependency is acceptable for the C++ client? Is the FlaUI-style object model (7.9) the right default surface, with raw verbs underneath?
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

15. **Hybrid package scope**: is `Sighter.FlaUI` (C#, FlaUI peer dependency) wanted as a first-party package in `sighter-clients`, and which FlaUI versions (FlaUI.UIA3 and UIA2) must it support? For Java, is the Appium-Windows recipe enough, or is a packaged helper wanted?
16. **Multi-application limits**: how many concurrent sessions and models per test (VRAM, section 12), and should the OS dialogs model ship as a starter model per OS (it would be trained on our own recordings of OS dialogs, no third-party data)?

## 16. Size (one capable agent team, rough, excluding polish and design pass)
- Spike (2 to 3 weeks): recorder on one OS, 500 labelled frames, train RT-DETRv2 + D-FINE, ONNX in `ort`, measure mAP, wrong-click rate, latency on CPU/GPU, resolver layers on 5 legacy apps, plus one remote-session capture to measure the accuracy loss.
- Recorder (capture, events, encoding, preflight, video import, remote-session windows): large, 4 to 6 weeks (three OS, Wayland hardest).
- Studio labelling (timeline, editor, tracking, review queue, import/export): large, 6 to 8 weeks; the canvas and queue are the biggest UI.
- Managed training runtime + evaluate + versioning: 4 to 6 weeks, mostly cross-platform packaging.
- Server, resolver, OpenAPI, CLI, clients, integrations, docs: 5 to 7 weeks.
- **Test layer (new)**: plan import and task backlog 2 to 3 weeks; app state graph, coverage, path generation 3 to 4 weeks; traceability, run sets, JUnit/TRX/Allure 2 to 3 weeks; data tables and matrices 1 to 2 weeks; visual regression completion (regions, levels, baselines UI) 2 to 3 weeks; `paddock-pitwall` crate and batch-priority jobs 1 to 2 weeks. About 11 to 17 weeks, partly parallel.
- **Sighter-authored tests (new in v1, 7.8)**: step model and TOML (de)serialisation in `paddock-testplan` 1 to 2 weeks; flow-to-steps conversion from the recorder 1 to 2 weeks; visual step editor, data tables and sub-flows in the studio 4 to 6 weeks; runner (`sighter run`, watch mode with overlay, parallel-safe scheduling, retries) 3 to 4 weeks; guarded `act` driver hardening on two OSes 2 to 3 weeks (moved from optional to required); C#/Java/C++ clients, examples and CI 2 to 3 weeks. About 10 to 15 weeks, partly parallel.
- FlaUI-style object models for C#/Java/C++ on top of the generated clients (7.9): 2 to 3 weeks, partly inside the 2 to 3 weeks for clients above.
- **AI-assisted recovery (7.10)** (my estimate, not the user's): `sighter-recover` preset and Pitwall contract extension 1 week; trigger detection, closed-vocabulary plan parser and guarded executor 2 to 3 weeks; "passed with recovery" state, reports, Debrief transcript and `RecoveryPolicy` in the clients 1 to 2 weeks; proposals UI in the studio 1 to 2 weeks. About 5 to 8 weeks, partly parallel; adds roughly 1 month to the total.
- **FlaUI hybrid package (7.11)** (my estimate, not the user's): `Sighter.FlaUI` C# package with scoping, fallback, shared waits and the `ITestContext` hook 2 to 3 weeks (shares the DPI helper with 7.12); Java and C++ recipes and examples about 1 week. About 3 to 4 weeks.
- **Multi-application tests (7.12)** (my estimate, not the user's): multi-session model, per-app model auto-selection, OS dialogs model, activation/focus verification 3 to 4 weeks; `Desktop` search with occlusion and z-order, per-monitor DPI helper 2 to 3 weeks; focus-following recorder and per-app steps in the editor 2 to 3 weeks; Debrief identity and log correlation about 1 week. About 8 to 11 weeks, partly parallel.
- Together 7.11 and 7.12 add roughly 1.5 to 2.5 months to the total.
- Later items: TestRail/Xray push 1 to 2 weeks, accessibility audit 2 weeks, localisation checks 2 to 3 weeks, richer performance reports 1 to 2 weeks.
- Total about 13 to 17 months of one team (was 8 to 11 before the Sighter-authored tests, the extra clients, AI recovery, the FlaUI hybrid and multi-application tests), roughly 95 to 145k lines Rust plus 45 to 70k lines Angular/TS (revision 3: 6 to 8 months, 60 to 90k plus 25 to 40k). A thin slice (record, label, train, locate on one OS) is still about 3 months; the test layer follows it.

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

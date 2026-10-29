# Plan: Sighter stage 2, from the accepted core to the first release

- Status: Plan, 2026-10-30, revised the same day for the user's decisions below. Nothing here is built. Docs only.
- **Decided by the user, 2026-10-30:** (1) one Sighter release, with the detector; there is no separate "Lab" release;
  (2) pixels only: the 2026-10-24 lab guest helper (UIA labels) is superseded by ADR 0001 decision 9; (3) the model
  checkpoints (about 1.5 GB) are mirrored on the user's Gitea and pinned by hash (plan item P1 approved).
- Starts from: the pure core on `sighter/core-wave-0` (`6a368882e`: coordinate chain, detection model, tile grid, seam
  merge, adapter hints, per-tile change detection, HUD core, benchmark harness; `sighter-core` and `sighter-bench`) and
  [ADR 0001](../adr/0001-object-detector.md) (the object detector), **accepted by the user** on 2026-10-30. ADR 0001, its research
  note and its performance block were landed by package SW1 (merged from `docs/sighter-rfdetr`, `70751e487`).
- Sources read: [AGENTS.md](../../AGENTS.md), [proposal.md](../proposal.md), [follow-ups.md](../follow-ups.md),
  [performance.md](../performance.md), [design review](../review/2026-10-design-review.md) (and the user's decisions of
  2026-10-24 at its end), [poc-vm-console.md](poc-vm-console.md); ADR 0001 revision 4 on `docs/sighter-rfdetr`; the
  earlier backend branch `sighter/core-backend-r2` (`d601d5547`, packages SG1 to SG5); family
  [rules](../../../../docs/rules.md) (1, 3, 9, 14, 21 to 26, 39, 42, 46 to 51), [design guide](../../../../docs/design-guide.md),
  ADRs 0012 (app shell), 0013 (workbench), 0024 (background work), 0031 (patience); the shared `ui/projects/paddock-ui`
  `shell` and `workbench` entry points and `crates/` on `origin/main` (`dfc9cdc94`); the read-only backend note
  `/Volumes/d7sst7/dev-targets/review/2026-10-28-pacenotes-sighter-backend-plan.md` (outside the repo).
- Rule 1 holds: Sighter is after Debrief 1.2 and beside Pacenotes in the agreed order. The packages in section 2 change
  no shared code and touch no UI, so they run beside the releases with one or two agents, within load and CI limits.
  Shared code waits for the Dyno and Spotter tags ([follow-ups.md](../follow-ups.md)); the UI waits for the shared
  workbench move that Debrief 1.2 completes, as Pacenotes' UI does.

**Marked for the user.** Every item that needs the user carries one of these tags, and section 8 lists them together:

- **[USER: hardware]**: a physical machine the user owns and names.
- **[USER: licence]**: a licence call or an outward-facing action under the user's name.
- **[USER: GUI review]**: the user looks at a mock-up or a build and approves it.
- **[USER: scope]**: a scope or release decision.

## 0. Facts this plan starts from

| Fact | Evidence |
|---|---|
| Only the pure core exists; it has no ports, no resolver and no fusion yet | `crates/sighter-core/src/{chain,change,detect,geometry,tiling,bench,hud/*}.rs`; `grep trait` finds none |
| An earlier backend line has a result enum, calibrated scores, fusion with a margin rule, the ports, perceptual hashes, an NCC template matcher and Sighter's own VNC client (about 5,900 lines with tests), ported from the PoC | `sighter/core-backend-r2` (`sighter-core`, `sighter-frames`, `sighter-match`, `sighter-vnc`, `sighter-infer`); not an ancestor of `core-wave-0` |
| That line's `sighter-infer` tiles at 644 px with sides divisible by 14 (a patch-14 assumption); ADR 0001 uses 640 tiles, patch 16 and `tile_grid` in `sighter-core` | `sighter-infer/Cargo.toml` description on `core-backend-r2`; ADR 0001 "The grid, exactly" |
| The ADR's plan items are P1 to P8 and its spikes S1 to S8 (the brief named S1 to S7; S8, confidence bands, is the eighth) | ADR 0001 "Plan items", "Spike items" |
| The family has no `paddock-video` yet; Debrief has `debrief-video` | `crates/` on `origin/main`; `apps/debrief/crates/debrief-video` |
| `paddock-runtime`'s catalogue knows archives only; the single-file asset kind (P3) does not exist | `crates/paddock-runtime/src/catalogue.rs` (archive kinds) |
| `paddock_proc::threads(kind, class)` and the ADR 0031 primitives (`Patience`, `CallKey`, `HostTickClock`) are not on `main`; Debrief 1.2's BP1 builds the ADR 0024 part; no plan schedules the ADR 0031 part yet | `git grep` on `origin/main`; [debrief-1-2.md](../../../../docs/plans/debrief-1-2.md) rows A1, P0, BP1 |
| The shared shell and workbench exist (`paddock-ui/shell`, `paddock-ui/workbench`, crates `paddock-shell`, `paddock-workbench`, `paddock-panels`), with a reference app | `ui/projects/paddock-ui/{shell,workbench}`, `ui/projects/workbench-demo` on `origin/main` |
| The user decided on 2026-10-24: v1 targets VM console, RDP and Local with a **Windows-only controller**; IronRDP after a spike; one stop hotkey (Ctrl+Alt+Shift+F12); the HUD as a sidecar track (its "Lab release before the training studio" is superseded: one release, 2026-10-30) | design review, "Decisions (user, 2026-10-24)" |
| ADR 0001 measures and defaults on CoreML (macOS) and says a **Windows release waits for spike S2** unless the user waives it; with one release, that gate applies to the only release | ADR 0001 decision 4, spike item S2 |
| Sighter's CI is one Linux leg for the pure core; there is no app, installer, workflow or tag | `paddock.ci.json` |

## 1. The release

One tag in this stage, **`sighter-vX.Y.Z`** (the number is the user's call at the tag), with the detector, decided by the
user on 2026-10-30. There is no "Lab" cut and no `sighter-v0.1.0`. It is a Windows controller release; macOS builds are
local builds for development and the user's tries (rule 9), never published, until a real signing identity exists
(design review 1.10, rule 23).

**Contents:** the app on the shared shell and workbench; Local and VNC / VM-console targets; capture and input helpers;
the guarded `act` driver with the stop hotkey; `sighter-infer` with OCR and the detector (RF-DETR, D-FINE fallback) on
CPU, CoreML and DirectML, with the GPU self-test and fallback; the resolver with cache, template, OCR-anchor and detector
layers; redaction before every store; the recorder; video import, auto-label, the review queue and labelling studio,
dataset assembly, the managed training runtime (MPS, CPU, opt-in CUDA), evaluation and promotion; model catalogue and
mirror; `sighter-server` (loopback, `/v1/locate`, `/v1/ocr`, `/v1/capture`, `/v1/wait_for`, `/v1/act`, `/v1/flag`,
health); a minimal plan runner with JUnit and a self-contained HTML report; the HUD (live view, Explain, run viewer from
the `.hud.jsonl` sidecar); the Playground. Pitwall naming stays an optional add-on (section 7).

**The gate.** ADR 0001 decision 11 ("spike before the first release") now applies to this release as written. **Spike S2
and P6 (DirectML on physical Windows GPU PCs) gate the release**: the Windows build waits for them unless the user
waives S2 (U-L4; date, reason and release recorded in ADR 0001's notes and the release notes). If P6 fails, the build
ships without `DirectML.dll` (CPU-only or the system copy, section 5.2) and still ships.

## 2. Start now (no shared code, no UI, no user)

These packages touch only `apps/sighter/` (and, for SW2 and SW5, one declared line or file each in shared docs or
`.gitea/`, done by the integrating agent). Ordered by lead time on the one release path: the longest chains first.

| ID | Package | Role | Days | Depends on | Acceptance |
|---|---|---|---|---|---|
| **SW1** | **Land the accepted design.** Merge `docs/sighter-rfdetr` (ADR 0001, research note `2026-10-29-rf-detr.md`, the performance block); set ADR 0001's status to Accepted with the user's date; apply plan item P2 to the proposal (sections 6, 7, 8, 15 question 1: presets, licence table, gate and target tiers, "names and classes for existing boxes"); update `follow-ups.md` (the "ADR-dependent" wait has ended) and the AGENTS.md lines that still say "accessibility trees are an optional `LabelSource`" and "RF-DETR after verifying weights" | implementer | 1 | none | No sentence in the proposal or AGENTS.md contradicts ADR 0001 decisions 1 to 11 (a reviewer checks each); ADR README lists 0001 as Accepted; docs-only, so no CI (rule 50 d) |
| **SW5** | **ONNX Runtime build pipeline** (plan item P5). Scripts and a workflow file `sighter-ort.yml` that build a pinned ONNX Runtime 1.31.x with `--no_telemetry`, static, Release, per target: macOS arm64 + CoreML; Windows x64 CPU-only (the fallback if P6 fails, and the build for the self-test); Windows x64 + DirectML; Windows arm64 + DirectML; symbol check (no 1DS, curl or TraceLogging provider); a minimal `ort` program loads the self-test model on each EP; artefact published on our Gitea, SHA-256-pinned, keyed by (version, flags, toolchain); `ORT_LIB_PATH` consumption; the Renovate entry; whether the DirectML import is delay-loaded (decision 4) | implementer 6, researcher 1 | 7 | none (the workflow file and `renovate.json` line through the integrator) | ADR's P5 list; an app build never compiles ONNX Runtime; the CPU-only Windows variant starts with no `DirectML.dll` present |
| **SW9** | **Prepare the checkpoint mirror** (plan item P1): a script that reads the URLs from the pinned `rfdetr` version's `model_weights.py`, downloads N, S, M and L, checks upstream's MD5s, computes SHA-256s, writes `LICENSE` (Apache-2.0) and `NOTICE` (source, version, upstream URLs); a dry run into a scratch folder | implementer | 1 | none | Hashes recorded; the upload to the user’s Gitea is **approved (2026-10-30, P1)**: the agent uploads after the dry run and records the hashes |
| **SW4** | **Synthetic UI corpus and test app.** A deterministic generator (seeded) of UI frames with ground-truth boxes in logical points: the classes of the taxonomy, login screens with password fields (dots, empty, "show password"), filled and live text inputs, small icons at 16 to 64 pt; at 1920 × 1200, 2560 × 1440, 3840 × 2160 and at 125 / 150 / 175 / 200 % rendered and normalised; remote-codec variants (JPEG, RGB565, chroma blur). Plus a small synthetic test app to record and drive (for S3, S7 and the redaction tests). The renderer is the designer's choice (for example pinned Chromium through `paddock-chromium`, headless) | designer 1, implementer 4 | 5 | none | Same seed gives byte-identical frames and annotations on one machine; generator name and version go into every rule-48 block that uses it; never captures a real screen (rule 7, audit SG1); ground truth validates (inside the frame, known class) |
| **SW0** | **Integrate wave 0 on `main`** after its independent review verdict, with the Linux CI leg | integrator | 0.5 | review verdict | `node scripts/affected.mjs origin/main` plans Sighter's leg; coverage gate (99 % lines) passes on `main` |
| **SW3a** | **Ports, results and fusion on the wave-0 core.** Port from `core-backend-r2` SG1: the result enum (`passed`, `passed_with_recovery`, `failed`, `error`), calibrated scores (`RawScore` cannot be fused), fusion with the margin rule (found, ambiguous, not found), thresholds resolved from settings, plan and step layers; the ports `Target`, `FrameSource`, `InputDriver`, `Detector`, `LabelSource`, `Ocr`, `Vlm`, `Store`, re-expressed in wave 0's typed geometry (`Rect<Logical>`, `Detection`, `Candidate`, `Provenance`) | designer 1, implementer 2 | 3 | SW0 | Two candidates within the margin give `Ambiguous`, never an action; an uncalibrated score is a type error; a hint-sourced candidate keeps its provenance; no second geometry type; coverage stays at 99 % |
| **SW3b** | **Template layer and frame utilities.** Port SG3 (NCC on a region, FFT for large regions, 1:1 scale) into `sighter-match`; from SG2 keep only what wave 0 lacks (SSIM for scene change, bounded PNG decode); de-duplication and tile hashing use `sighter_core::change` only (ADR 0001: one hashing implementation); drop SG5 (its 644 px, patch-14 tiling contradicts the ADR) | implementer | 2 | SW3a | A crop shifted by 7 px is found at the right point; a theme-inverted crop is `NotFound`, not a low-score hit; `grep` finds one perceptual-hash implementation |
| **SW3c** | **Sighter's own VNC client** (SG4: RFB 3.8, None and VNC auth, Raw, CopyRect, Hextile, ZRLE, keys including Ctrl+Alt+Del, pointer, stop releases held input) into `sighter-target`, behind `Target`; the in-process fake VNC server for tests; bind to loopback in tests | implementer | 3 | SW3a | Fake server tests: auth, each encoding, a dropped connection is `error` (not `failed`), stop during a long wait ends at once and releases held keys and buttons |
| **SW6** | **`locate` benchmark harness** (plan item P8), first with a fake `Detector`: load guard (below 12 a gate verdict, 12 to 25 indicative, above 25 skipped), Windows' 10 s CPU-use check, 5 warm-ups, 200 timed runs, `sorted[⌊p · (n − 1)⌋]`, gate rows, diagnostic rows, cold rows, footprint memory (rule 26), the rule-48 block | implementer | 2 | SW4 for the corpus | Unit tests for each guard band and the percentile rule; the block matches [benchmarks.md](../../../../docs/benchmarks.md)'s template |
| **SW7** | **OCR engine ADR (Sighter ADR 0002).** PaddleOCR det + rec ONNX through `sighter-infer` (the proposal's choice) against the OS engines (Windows.Media.Ocr as in the PoC, Apple Vision) and `ocrs`: licences of code, weights and dictionaries, languages, whether the text detector sees dot and asterisk runs (needed by the masked-glyph check), CPU cost on a region and a full frame (recorded under rule 48), determinism | researcher | 3 | SW4 | ADR with options, decision and a measured block; **[USER: licence]** acceptance of the engine and its model licences |
| **SW2** | **Integrator's amendments** (plan item P7): Sighter's launch sites into ADR 0024's instance list and Sighter's rows into ADR 0031 section 7 | integrator | 0.5 | SW1 | The rows equal ADR 0001's two tables; C40 and C51 will find the sites classified when code adds them |
| **SW8** | **RT-DETRv4 licence review** (plan item P4): inspect a checkpoint's keys for teacher weights, get a reading on DINOv3-distilled weights, draft the question to the authors | researcher | 1 | none | A note in the research folder; **sending the question and the final call are [USER: licence]**; RT-DETRv4 stays out of the catalogue until then (not on any release path) |
| SW10 | **IronRDP spike** (user decision 4 of 2026-10-24): codecs, NLA/CredSSP, clipboard, reconnect, headless frames; FreeRDP out of process as the fallback | researcher | 5 (time-box; the decision allowed up to two weeks) | none | A verdict note with measured frame latency (rule 48); decides the RDP adapter, which is stage 3 unless U-S3 puts it in the release |

Start-now total: **34 agent-days**. The first three to start are **SW1** (it makes the ADR the source every later brief
cites), **SW5** (the longest lead time: the DirectML and CoreML builds are now on the release path, and P6 needs them) and
**SW9** (the mirror is approved, so the hash-pinned upload can start at once and unblocks SW11, SW32 and SW31). **SW4**
(the corpus for S1, S3, S7, S8, the redaction tests and the harness) starts beside them, and SW3a follows as soon as SW0 is
merged. **Book the S2 hardware (U-H1) now**: the release waits for it.

## 3. Shared code (after the Dyno and Spotter tags; rules 39, 42, 43)

Each package lands with its adoption by every user in the same change.

| ID | Package | Role | Days | Depends on | Acceptance |
|---|---|---|---|---|---|
| **SW11** | **Single-file asset kind** in `paddock-runtime` (plan item P3): one TOML entry per file (URL, size, SHA-256, licence, upstream URL), parsed once, any error refusing the whole catalogue; a licence allow-list (Apache-2.0, MIT) checked by a test over every entry; download as a `paddock-jobs` job through `paddock-net` `Egress` (host allow-list: our Gitea) and `paddock-download` (hash while writing, rule 15); Pacenotes' Model2Vec file adopts it in the same change | designer 1, implementer 2 | 3 | the tags; Pacenotes owner agrees | Corrupt, oversized, wrong-hash and non-allow-listed entries are refused with a message; Offline mode blocks the download with its message; tests once, in the crate (rule 43) |
| **SW12** | **`paddock-video`** extracted from `debrief-video` (FFmpeg runtime, sandboxed launcher, `ffprobe`, frame extraction, encode); Debrief adopts it in the same change | designer 1, implementer 4 | 5 | Debrief 1.2 shipped | Debrief's video tests pass unchanged against the shared crate; Sighter can decode with `mpdecimate` and `-skip_frame nokey` as options |
| SW13 | **Follow-ups 1 to 4** ([follow-ups.md](../follow-ups.md)): results-block renderer and A/B timer to `paddock-bench`; `Secret` from `paddock-secrets`; trace bus on `paddock-live` if it fits; the planner's "rust + ui" label | implementer | 2 | the tags | Sighter keeps only its budget constants and its masking rule; planner tests pass |
| SW14 | **ADR 0031 primitives** (`Patience`, `CallKey`, `HostTickClock`, stall judgement on progress, CPU time and run state) and **ADR 0024's `threads(kind, class)` and class on launch** | (owners outside Sighter) | (0; 5 if Sighter must build the ADR 0031 part: designer 1, implementer 4) | Debrief 1.2 BP1 (ADR 0024); ADR 0031: **unscheduled** | Sighter's sidecar client (SW17) needs both. If no plan owns the ADR 0031 part when SW17 starts, the coordinator assigns it as a shared package; Sighter does not write a private copy (rule 42) |

## 4. The release path

One path to one release: the engine foundation (4.1), then the detector and training engine (4.2), with the UI (4.3)
starting when the workbench move ships; spikes and the release package follow in section 5.

### 4.1 Engine foundation (Rust; can start once SW3a is in, independent of the UI)

| ID | Package | Role | Days | Depends on | Acceptance |
|---|---|---|---|---|---|
| **SW15** | **Capture helper**, out of process: Windows.Graphics.Capture (DXGI duplication as fallback) and ScreenCaptureKit (development builds); window, display and region; Per-Monitor V2 aware; scale metadata per display; the output size rule of "Coordinates" (window size × `scaleFactor`); frames handed to the sidecar without a copy per call where the OS allows; launched through `paddock-proc` at `User` (C40), `--self-test` (rule 23) | designer 1, implementer 6 | 7 | SW3a, SW14 | Tests capture only windows the test created (hermetic, audit SG1; a guard refuses the user's console session); mixed-DPI and moved-window tests on the chain; a capture fault ends the helper, not the server |
| **SW15** | **Capture helper**, out of process: Windows.Graphics.Capture (DXGI duplication as fallback) and ScreenCaptureKit (development builds); window, display and region; Per-Monitor V2 aware; scale metadata per display; the output size rule of "Coordinates" (window size × `scaleFactor`); frames handed to the sidecar without a copy per call where the OS allows; launched through `paddock-proc` at `User` (C40), `--self-test` (rule 23) | designer 1, implementer 6 | 7 | SW3a, SW14 | Tests capture only windows the test created (hermetic, audit SG1; a guard refuses the user's console session); mixed-DPI and moved-window tests on the chain; a capture fault ends the helper, not the server |
| **SW16** | **Input helper and the guarded `act` driver** (`sighter-input`): `SendInput` Per-Monitor V2 (CGEvent for development builds); scope binding to a window or process; deny-list; dry-run; max-actions cap; the stop hotkey Ctrl+Alt+Shift+F12 (releases every held key and button, cancels queued actions, keeps the session); physical input pauses a run; pre-action re-check and post-action verification | designer 1, implementer 5 | 6 | SW3a, SW15 | A fake target that hangs: stop ends at once; an action outside the bound window is refused with its reason; the re-check refuses when the element moved |
| **SW17** | **`sighter-infer`, part A: the sidecar and its isolation** (section 6): `ort` in custom-library mode on the SW5 CPU build; started through `paddock-ipc`, supervised by `paddock-proc` with a footprint memory cap (rule 26) and restart with back-off; `--self-test` with the tiny bundled model; `ORT_DISABLE_TELEMETRY=1` and `DisableTelemetryEvents`; the IPC protocol with the class on every call, a `Progress` frame per finished tile or batch, Cancel; one work queue ordered by class; Patience keys `sighter.load`, `sighter.selftest`; CI egress test | designer 2, implementer 6 | 8 | SW5, SW14 | A crash ends one request and the next succeeds after restart; the egress test fails on any connection attempt; a starved sidecar is never judged stuck (ADR 0031 tests with an injected clock); the self-test runs from the built app in CI |
| **SW18** | **OCR in `sighter-infer`** per SW7's ADR: text detection and recognition, pre- and post-processing pure in `sighter-core`; one OCR pass per frame hash, shared by the OCR-anchor layer, the password classifiers and the label source | implementer | 4 | SW7, SW17 | Word boxes in logical points through the chain; same frame and model give the same result on the CPU EP; limits on image size and text length; recorded cost block (rule 48) |
| **SW19** | **Redaction before every store or send**: the pure fill function (frame, classifier outputs → filled frame and rectangles), sampled field background, never a blur; the masked-glyph check; the OCR label cue from a "secrets" TOML pack (rule 21) the user can extend; text inputs filled by default until classified; the `redacted` flag and rectangle list; one test per path of ADR 0001's table (recorder, test runs, exports, diagnostics, Pitwall crops) with a known secret rendered into a synthetic password field | designer 1, implementer 4 | 5 | SW4, SW18 | No byte written or sent contains an unfilled pixel of the field, on every path; the detector's `password_field` class joins in SW31 |
| **SW20** | **Resolver and the locate pipeline without the detector**: cache, template and OCR-anchor layers with fusion and the margin rule; normalise to logical 100 % → scope (target window by default) → `tile_grid` → per-tile change cache → priority order (focused window, pointer) → early stop only on a unique hit above τ → seam merge; liveness by Patience (`sighter.locate`), the only cap the caller's step timeout in host ticks, explained when it fires ("Not found within the step's 10 s; searched the target window, 6 of 8 tiles", ADR 0019); "needs labelling" with screenshot | designer 1, implementer 5 | 6 | SW3a, SW3b, SW18, SW14 | Never an action on `Ambiguous` or below τ; a locate without a caller timeout shows "Still working" (rule 12); round trip of boxes through the chain within ±1 px (ADR test item) |
| **SW21** | **`sighter-server`**: loopback HTTP, OpenAPI 3.1 generated from Rust DTOs and checked in CI; `/v1/locate`, `/v1/ocr`, `/v1/capture`, `/v1/wait_for` (frame-diff skips unchanged screens), `/v1/act` (off by default, per-key permission), `/v1/flag`, `/healthz`, `/v1/status` (layers, device, loaded models); loopback hardening (Host and Origin checks, header key, named pipe), per-app keys in the OS credential store, size and rate caps; the service through `paddock-service` (rule 24) | designer 1, implementer 7 | 8 | SW20, SW16 | A request from a browser origin is refused; oversized images are refused before decode; the OpenAPI check fails on DTO drift; services survive an update in the VM test (rule 24) |
| **SW22** | **Targets: Local and VNC / VM console** behind `Target`, with the viewer-window chain (content rectangle, `s`, `r`), the "Viewer window changes" table, change detection from Sighter's own per-tile hash, input back through the viewer; the PoC's VM console flow | implementer | 4 | SW3c, SW15, SW16 | ADR test items for remote 1920 × 1200 at `r` = 1 and 3840 × 2160 at `r` = 1.5, fit-to-window 0.75 and 0.5, two host displays; a resize with a new `s` drops the tile cache |
| **SW23** | **Recorder**: capture → byte-sized bounded queue (`recorder.queue_memory`, Auto 256 MB, 2-frame floor) → classifier workers (`recorder.classify_workers`, Auto `threads(Cpu, User)`) → fill → lossless PNG or WebP → write; on overload drops new frames, counts them and shows "Recording at N fps, frames skipped", **never writes an unclassified frame**; unchanged tiles reuse their classification; pause hotkey; input events without key text; session metadata (scale per display, theme, window); retention caps; a `paddock-jobs` job with phases | designer 1, implementer 6 | 7 | SW15, SW19 | Overload test with a slow classifier: frames are dropped and counted, none is written unclassified; a cancelled recording leaves a consistent folder (build new, then swap); settings validated in the engine (rule 51) |
| **SW24** | **Minimal runner and reports**: `sighter run` over TOML steps on `paddock-testplan` (click, type, press, wait for text or element, assert); waits from the resolver, never sleeps; the result taxonomy with `error` apart from `failed`; a run folder with redacted step screenshots, `trace` and the `.hud.jsonl` sidecar (limits and refusal rules of ADR 0001 "Decisions for the pure core" 3); JUnit and one self-contained HTML report | designer 1, implementer 6 | 7 | SW20, SW22 | The PoC's Notepad flow passes against a VM target; an infrastructure fault reports `error`; the HTML report opens offline with no external asset; HUD overhead re-measured against ≤ 2 ms per frame (rule 48) |

Engine foundation total: 62 agent-days.

### 4.2 Detector and training engine

| ID | Package | Role | Days | Depends on | Acceptance |
|---|---|---|---|---|---|
| **SW32** | **Model registry and catalogue**: immutable, content-hashed models with `model.json` and the model card (classes, input size, normalisation, thresholds, dataset hash, metrics, parent, base checkpoint, licence, the lineage line "DINOv2 (Apache-2.0) → Objects365 pre-training (no licence stated) → COCO (Roboflow checkpoint, Apache-2.0) → the user's frames", the `rfdetr` version, tile overlap and the largest small class); import refuses PML sizes by metadata or card; the four catalogue entries on SW11's asset kind | implementer | 4 | SW11, SW9 upload | Catalogue licence test over every entry; an ONNX naming a PML size is refused with its reason; promotion and pinning by version (`app@3`) |
| **SW31** | **`sighter-infer`, part B: the detector** behind the `Detector` port: one "ONNX DETR, fixed input, no NMS" adapter for RF-DETR and D-FINE; `model.json` parsed at the edge; CoreML (`MLProgram`, `CPUAndGPU`, `RequireStaticInputShapes=1`, `ModelCacheDirectory`) and DirectML (device 0, `error_on_failure`); the start-up GPU self-test of **the user's model on its stored reference frame against the CPU answer in `model.json`**, re-run after a model, driver or OS change; automatic CPU fallback with the same model and the plain message and the "Train a Fast model for this computer" action; device, EP, GPU name, fallback reason, ONNX Runtime version and model hash on every result and job; one GPU queue per EP ordered by class, a tile or small batch as the unit, locates round-robin by tile, single-flight on (frame hash, scope), admission bound (Auto 8); `sighter-infer --prepare` as a separate process at `Background` (C40) for the CoreML compile, the cache keyed by model hash, locate answering on the CPU EP until it is ready; the detector's `password_field` class joins SW19's classifiers | designer 2, implementer 8 | 10 | SW17, SW5 (GPU variants), SW32 | A failed GPU registration is seen, never a silent CPU run; a waiting locate starts within one background batch; bit-identical CPU results with fixed threads (to be shown in S1); a stuck `--prepare` never blocks a locate |
| **SW37** | **Video import** as the rule-14 pipeline: decode (FFmpeg through `paddock-video`, `mpdecimate`) → de-duplicate (largest-cell difference, sequential commit point) → redact → infer (only when "Label with computer vision while importing" is chosen) → store; byte-sized queues (`pipeline.queue_memory`, Auto 512 MB) with the 2-frame floor raised automatically and said in the details; Cancel in every stage; a new import folder swapped in at the end; frame ids from decode timestamps; the source video never copied, only its path, size and SHA-256 kept; the display scale entered or confirmed at import | designer 1, implementer 6 | 7 | SW12, SW19 | Same video gives the same frame ids; per-stage progress on its own scale; Cancel leaves the project unchanged |
| **SW38** | **Auto-label, fusion and the review-queue backend**: OCR, templates and the app's detector as label sources (never the VLM placing boxes); fusion with the review flags (below 0.6, disagreement, unknown class) and auto-accept only at ≥ 0.85 with no disagreeing source; label propagation by tracking into neighbouring frames as "proposed"; uncertainty ordering; screen fingerprints | implementer | 5 | SW31, SW37 | Unreviewed boxes never reach a dataset; pure functions in `sighter-core` at 99 % coverage |
| **SW40** | **Dataset assembly (dataset tooling)**: per reviewed frame, normalise with its recorded chain and the inference resampler; cut `tile_grid(w, h, 640, 64)` exactly as at run time; k random-offset crops (default 2); clip with the visible-fraction rule (40 % or 64 logical points along the cut side); HiDPI and viewer-scale simulation marked as such; the redaction fill on ordinary text inputs up to the target filled fraction (default 0.5) and synthetic mask dots on `password_field` boxes; hard negatives; repeat-factor class balancing; the held-out set split by recording session and by screen fingerprint, never augmented; a COCO dataset of 640 tiles, content-hashed and reused across runs with the same preset; export-only (redacted COCO plus README); `dataset.workers` and `dataset.store_writers` Settings with Auto | designer 1, implementer 6 | 7 | SW3a, SW19, SW38 | Training tiles equal inference tiles for the same frame (shared `tile_grid` test); no held-out screen fingerprint appears in training; deterministic for a seed |
| **SW36** | **Managed training runtime and worker**: a `paddock-runtime` entry (python-build-standalone, torch for MPS and CPU, `rfdetr` pinned, Albumentations 2.0.8, no Kornia, never AlbumentationsX); runs with no network (`HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, outbound denied) and the checkpoint passed as a full path to the mirrored, hash-checked file; self-test trains one step on a generated mini dataset with the network denied (at `Background`); the opt-in CUDA variant downloaded from the upstream hosts only, NVIDIA's terms shown first, those hosts on the `Egress` allow-list for that job only; the worker over IPC at `User` with progress per optimiser step and Cancel; Sighter's `aug_config` without flips, `multi_scale=off`, scale jitter 0.9 to 1.1; ONNX export and `model.json` with the CPU reference answer; then "prepare for this Mac" | designer 2, implementer 8 | 10 | SW11, SW32, SW40 | Self-test passes offline on each OS; a training run reports progress from the first moment and cancels cleanly; "Training is using the graphics card; locating may be slower" shown while it runs |
| **SW41** | **Evaluation and promotion**: mAP@0.5 and @0.5:0.95 per class, F1-optimal threshold per class, wrong-click rate, held-out real frames, per-slice gates, side-by-side version compare; a model serves unattended suites only after its gates (user decision 6 of 2026-10-24; shadow mode is Next) | implementer | 4 | SW36, SW40 | A model that fails a slice gate cannot be promoted without a recorded override |

Detector and training engine total: 47 agent-days.

### 4.3 UI on the shared shell and workbench (after Debrief 1.2 and the workbench move)

| ID | Package | Role | Days | Depends on | Acceptance |
|---|---|---|---|---|---|
| **SW25** | **App scaffold and CI**: `apps/sighter/app` (Tauri 2, Angular, zoneless, OnPush) on `paddock-ui/shell` and `paddock-ui/workbench`; splash before IPC; start-up watchdog; service health with a working Repair (rule 19); settings sections each with a setting (rule 44), including "Inference device" (Automatic, GPU, CPU) and every thread Setting with Auto; mock backend by DI token; the plum accent; icons from `icons/sighter`; the workflow `.gitea/workflows/sighter.yml` (Windows and macOS legs), `paddock.ci.json`, the CODEOWNERS line | designer 0.5, implementer 4 | 4.5 | workbench move shipped; SW21 | `scripts/affected.test.mjs` sees the workflow, contract and owner line; rule 23 self-tests of every sidecar from the built app; token tests green |
| **SW26** | **HUD and studio-frame design**: mock-ups of the live-view HUD (boxes, classes, confidence bands with words, layer toggles C/T/O/D/V/H, step bar, opacity), "Explain this locate", the run viewer, the Playground, the recorder bar; light, dark and high contrast; per-class colours checked for contrast and colour-vision distance; the text alternative for the canvas | designer | 3 | SW25 | Independent review (rule 22) with no P1; **[USER: GUI review]** approval before SW27 starts |
| **SW27** | **HUD implementation**: live view as a workbench view and a pop-out; overlay items from `sighter_core::hud` (one set of items for the live view and burned-in exports); Explain panel; run viewer playing stored frames with the sidecar; secrets only as `•••• secret NAME`; the on-screen overlay for the Local target only where it is excluded from capture (Windows `WDA_EXCLUDEFROMCAPTURE`) | implementer | 7 | SW26, SW24 | HUD ≤ 2 ms per frame and trace on/off ≤ 5 % step change, measured and recorded (rule 48); screenshots independently reviewed (rule 22); component coverage per AGENTS.md |
| SW28 | **Recorder UI and preflight**: consent, permissions preflight per OS, pause, the frames-skipped state, redaction regions drawn before recording | implementer | 3 | SW26, SW23 | Every state designed (empty, recording, paused, overloaded, error with Retry) |
| SW29 | **Playground**: drop a screenshot or capture, type a query, see every layer's candidates, scores and timing | implementer | 3 | SW26, SW21 | Uses only engine commands; no resolver logic in TypeScript |
| **SW39** | **Labelling studio**: frame timeline, canvas with box tools (draw, resize, merge, split, copy to similar frames, undo), keyboard-first review queue (number keys accept, `/` class, `S` skip, `D` draw, "same as…"), crop grids, per-class colours, redacted rectangles shown | designer 3, implementer 10 | 13 | SW38, SW25 | Studio budgets measured (60 fps with 500 boxes; next item ≤ 50 ms; rule 48); mock-ups independently reviewed and **[USER: GUI review]** before implementation |

UI total: 33.5 agent-days (20.5 for the HUD, recorder and Playground, 13 for the labelling studio).

## 5. Spikes, gates and the release package

### 5.1 The release package

| ID | Package | Role | Days | Depends on | Acceptance |
|---|---|---|---|---|---|
| **SW44** | **Sighter release** (section 9) | implementer 2, integrator 1 | 3 | all of section 4, S1 to S8 recorded, **S2 and P6 passed or waived** | Checklist complete; **[USER: GUI review]** try on the user's Windows PC with a VM, and of the record → label → train → locate loop on the synthetic app and on one app of the user's choice, before the tag |

Release package: 3 agent-days. Packages total: 34 + 10 + 62 + 47 + 33.5 + 3 = 189.5; with the spikes (21), see section 10.

### 5.2 Spikes and their gates

Every result is a rule-48 block in [performance.md](../performance.md) under ADR 0001's "Gate conditions per execution
provider" (Mac mini M4 Pro for CoreML and CPU; the `windows-latest` VM for the CPU-only runner class; physical PCs for
DirectML).

| Spike | What | Role | Days | Needs | Gate it decides |
|---|---|---|---|---|---|
| **S1** | CoreML and CPU: the locate gate at 1920 × 1200 and 4K, cold load, prepare time, the CPU-gate plan in order (`threads(Cpu, User)` instead of 4; window scope; Nano), seam recall with `SEAM_IOU` | researcher | 3 | SW4, SW6, SW31 (an exported COCO checkpoint is enough for latency) | The release's default preset per EP; whether the CPU preset switches to the D-FINE fallback (ADR decision 11) |
| **S2** | DirectML on a physical Windows 11 PC with an Intel integrated GPU and on one with an NVIDIA discrete GPU: locate gate, cold load, GPU self-test against the CPU answer; whether `windows-latest` exposes any DirectX 12 adapter | researcher | 3 | **[USER: hardware]** two PCs the user names, or the user runs a prepared kit (as the PoC zip) and returns the log | **The release waits for S2 on Windows** (a gate, not a follow-up) unless the user waives it (date, release, reason recorded in ADR 0001's notes and the release notes) |
| **P6** | The DirectML gate, with S2: egress test with our `DirectML.dll` on the DirectML EP, and the ETW check (TraceLogging metadata and GUIDs in the DLL; a `logman` or `tracelog` session listing providers and events while the EP runs) | researcher | (in S2) | as S2 | Pass (user, 2026-10-30): no outbound connection, and the ETW session shows **no events** from `DirectML.dll`'s `Microsoft.Windows.AI.MachineLearning.Dml` provider while the EP runs (the DLL registers that provider in the Microsoft telemetry group, so "no provider" cannot hold; Windows' diagnostic settings govern it like any OS component); then `DirectML.dll` 1.15.4, fetched from nuget.org at build time and hash-pinned, never mirrored, ships next to `sighter-infer.exe`. Fail: it is not shipped; Windows uses the system copy through a delay-loaded import, or the CPU-only build. The method (byte reading plus ETW recording, no disassembly) is accepted |
| S3 | Bulk cost: import stage rates (`mpdecimate`, scene `select`, Sighter's hash alone, `-skip_frame nokey`), auto-label per frame on CoreML and CPU, dataset assembly time and encode format, recorder classification cost and sustained fps at 1920 × 1200 and 4K | researcher | 3 | SW4 test app, SW23, SW37, SW40 | Import defaults, the dataset encode format, the recorder's Auto values |
| S4 | Training cost: minutes per epoch for Small on 20,000 tiles (MPS, CPU; CUDA), memory, locate p95 while training runs | researcher | 2 | SW36; **[USER: hardware]** an NVIDIA laptop or PC for the CUDA row | The hardware estimate shown before training; the CUDA row may stay "unmeasured" in the release with the variant marked so |
| S5 | Cheaper inference: fp16 (the GPU self-test still matching), IO binding, batch size per EP, against the fp32 baseline | researcher | 2 | S1 | Each option kept only with a recorded gain (rule 51 b) |
| S6 | Adapter hints: whether UI Automation honours a Per-Monitor V2 thread context in a process that is not DPI aware (`Sighter.FlaUI`); a synthetic window on a 150 % monitor beside a 100 % one | researcher | 2 | a Windows machine with two displays at different scales (try a VM with two virtual monitors first; else **[USER: hardware]**) | The adapter-hint wire for the adapters, which are stage 3: S6 can run after the release if the user agrees (**[USER: scope]** U-S5) |
| S7 | Quality: auto-label precision and recall per class, augmentation gains on held-out real frames, password recall (masked glyph, label cue, class; local and RDP-compressed; 100 % and 200 %), compressed-frame accuracy, occlusion, hashing cost, seam recall; the synthetic set of live, unfilled inputs and dotted password fields for each target filled fraction | researcher | 5 | SW4, SW38, SW40, SW41 | Which augmentation groups stay on; the default filled fraction; whether copy-paste, occlusion and text re-rendering earn a place (else they stay out of the release) |
| S8 | Confidence bands: per-class F1-optimal thresholds and the wrong-click rate against the 0.8 and 0.5 defaults | researcher | 1 | SW41, a model trained on the synthetic app | The band defaults, or per-class bands |

## 6. The inference worker and its isolation

| Process | Runs | Class (ADR 0024) | Isolation and failure |
|---|---|---|---|
| `sighter-infer` | locate, OCR, pre-labelling, evaluation, the GPU self-test | `User` at launch; the class travels with each call; background calls run on `Background` threads and queue behind locates | Supervised by `paddock-proc` (footprint cap, restart with back-off); a C++ fault ends one request; Patience judges stalls on tile progress, CPU time and run state; never in Pitwall |
| `sighter-infer --prepare` | one CoreML compile | `Background` | Separate short-lived process, one at a time; judged on CPU time and run state; no cap (rule 12) |
| Capture helper | capture for locate and recording | `User` | Out of process; a fault ends the helper and degrades the target to "capture failed" with Retry |
| Input helper (Windows) | `SendInput` | `User` | Per-Monitor V2; refuses outside the bound scope |
| Training worker (managed Python) | `rfdetr` training | `User` (all of v1) | Downloaded runtime, no network, dataset read-only, output folder writable, progress per step, Cancel |
| Runtime self-test | one step on a mini dataset | `Background` | After download or update; failure quarantines the runtime entry |
| FFmpeg via `paddock-video` | decode (import) / encode (remote-codec preset) | `Background` / `User` | Sandboxed launcher from Debrief ADR 0033 |

Telemetry is off by construction (the `--no_telemetry` build), with the runtime opt-out, the CI egress test and the
symbol check as second lines.

## 7. Model packaging and the Gitea mirror

- **In the installer:** only the tiny self-test model (rule 23), the OCR models chosen in SW7, and on Windows the
  `DirectML.dll` if P6 passes. No detector weights.
- **Downloaded:** only the four Apache COCO checkpoints (N, S, M, L), from our Gitea mirror (SW9, then the upload, approved by the user on 2026-10-30), pinned to our SHA-256s, through SW11's asset kind, `paddock-net` `Egress` and `paddock-download`, as a
  job. A checkpoint update is a new entry with a new hash, never an overwrite.
- **Made locally:** every runtime detector, stored immutable and content-hashed with `model.json` and the model card;
  the CoreML cache keyed by model hash and cleaned with the registry.
- **Refused:** PML sizes (Atto, Femto, Pico, XL, 2XL), D-FINE `obj365` and `obj2coco` checkpoints, UI-DETR-1,
  RT-DETRv4 until P4 is decided.
- **Licences:** `ort` and ONNX Runtime notices in `deny.toml`; the "non-crate binaries" list with `DirectML.dll` and the
  CUDA runtime as the two named exceptions; a check of the bundle's files against that list (with the notices tooling).
- **Out of stage 2 (Pitwall, optional):** naming and classes for existing boxes, step descriptions, the grounding
  fallback. Everything works without them; they come in stage 3 with the "needs local AI" state.

## 8. What needs the user

Open items:

| ID | Kind | What | Needed by |
|---|---|---|---|
| U-H1 | **hardware** | Two physical Windows 11 PCs with a DirectX 12 GPU, one Intel integrated and one NVIDIA discrete, for S2 and P6, a **gate for the release**; the user names them (ADR 0001 S2), or runs a prepared kit and returns the log. Book early | the release on Windows (or a waiver, U-L4) |
| U-H2 | **hardware** | An NVIDIA laptop or PC for S4's CUDA row and the CUDA training variant | the release (can ship with the CUDA row marked unmeasured) |
| U-H3 | **hardware** | The Windows PC with VMware Workstation (from the PoC) for the release try; a second display at a different scale if S6 runs before the release | the release |
| U-L2 | **licence** | RT-DETRv4: send the question to the authors and decide after the review (P4) | nothing on the release path |
| U-L3 | **licence** | Accept Sighter ADR 0002 (OCR engine) and its model licences | SW18 |
| U-L4 | **licence** | DirectML: accept the P6 result, or waive S2 for the Windows release (recorded) | the release |
| U-G1 | **GUI review** | HUD, Explain, run viewer, Playground and recorder mock-ups (SW26) | SW27 |
| U-G2 | **GUI review** | Labelling studio and review-queue mock-ups (SW39) | SW39 implementation |
| U-G3 | **GUI review** | Try the release before its tag | SW44 |
| U-S2 | **scope** | Platforms: Windows-only controller release (2026-10-24), with macOS as unreleased development builds although ADR 0001 measures on CoreML; or also publish macOS with ad-hoc sealing (Screen Recording grants reset on every update) | before SW25 |
| U-S3 | **scope** | RDP in the release, or after it (this plan: after, on SW10's verdict); interrupt handlers and the scheduler of the 2026-10-24 "Must for v1" list in the release or stage 3 (this plan: stage 3) | before SW24 |
| U-S5 | **scope** | S6 after the release, since the adapters it serves are stage 3 | before SW44 |

Decided by the user on 2026-10-30 (no longer open):

| ID | Decision |
|---|---|
| U-S1 | **One release, with the detector.** No separate Lab release, no `sighter-v0.1.0`; ADR 0001's "spike before the first release" applies to it, so S2 and P6 gate the release unless the user waives S2 (U-L4) |
| U-S4 | **Pixels only.** The lab-only guest helper for UIA labels (decision 3 of 2026-10-24) is superseded by ADR 0001 decision 9; it is not built and appears in no document |
| U-L1 | **Weights.** The four checkpoints (about 1.5 GB) are mirrored on the user's Gitea and pinned by SHA-256; plan item P1 is approved (SW9 uploads, SW32 and SW11 consume) |

## 9. Release checklist

Rule 9 (tag in CI only), rule 25 and the release lessons in [suite-lessons.md](../suite-lessons.md), plus Sighter's own:

1. Release slot from the coordinator (rule 1: one release at a time; Sighter's order is after Debrief 1.2).
2. Full gates once on the combined result (ER-8): fmt, clippy, nextest, coverage (core ≥ 99 %, components 100 %, Angular
   ≥ 80 %), `cargo deny --workspace check` and `cargo deny check advisories`, `npm audit`, lint, build, unit and e2e on
   the gallery and app; the publish script test.
3. Every sidecar (`sighter-infer`, capture and input helpers) starts from the built app with `--self-test` (rule 23, C34);
   the ONNX Runtime artefact's symbol check and the egress test pass on that build.
4. Bundle contents checked against `deny.toml`'s non-crate list: `DirectML.dll` only if P6 passed (S2 recorded or waived).
5. Model catalogue licence test; no detector weights in the installer; the self-test and OCR models' licences in the
   notices; third-party notices and SBOM (through `paddock-about` / `paddock-deps` when they exist).
6. Icons check (C03); release notes with a First launch section (unsigned SmartScreen steps), the platforms, the device
   behaviour (GPU with CPU fallback), any S2 waiver, and links to the performance blocks quoted.
7. Every number in the release notes, CHANGELOG and README links to its rule-48 block; the release needs S1 to S8 recorded (S6
   per U-S5).
8. Screenshots and diagrams independently reviewed (rule 22), and the family profile updated with the new image.
9. Services: re-registration on update tested in a local VM (rule 24); no test touches the user's machine (rule 7).
10. The user's try (U-G3), then the `sighter-vX.Y.Z` tag on a CI-built commit that is not `[skip ci]`; verify the
    published artefacts.


## 10. Order and parallel lanes

With the five-agent cap and Sighter behind the current releases, the lanes are:

| Lane | Now (1-2 agents beside the releases) | After the tags | After Debrief 1.2 and the workbench move |
|---|---|---|---|
| A, docs and integration | SW1, SW0, SW2 | SW13 | SW44 |
| B, pure core and targets | SW3a → SW3b, SW3c | SW20 → SW22, SW24 | SW38, SW40, SW41 |
| C, native runtime | SW5 | SW17 → SW18, SW32 → SW31 (needs SW14) | SW36 |
| D, corpus and measurement | SW4 → SW6 | S1, S5 | S3, S4, S7, S8 |
| E, research and the mirror | SW9 (upload), SW7, SW8, SW10; **book U-H1** | SW11, SW12 | S2 + P6 (user hardware, a release gate) |
| F, OS adapters |  | SW15 → SW16, SW19 → SW23, SW21 | |
| G, UI |  |  | SW25 → SW26 → SW27, SW28, SW29; SW39 |

Critical path to the one release:

- SW5 (CPU, CoreML and DirectML builds) and SW9 (mirror upload) → SW11 → SW32 → SW31 → S1 → S2 + P6 (user hardware, the
  gate); beside it SW17 → SW18 → SW19 → SW23, SW3a → SW20 → SW21 and SW24, SW12 → SW37 → SW38 → SW40 → SW36 → SW41 → S7,
  S8; then the UI chain SW25 → SW26 (user review) → SW27 and SW39 (user review), and last SW44. The UI cannot start
  before the workbench move ships, so the date is set by whichever is later: that move, or the engine chain (about 110
  agent-days). S2 + P6 can run any time after SW31 and should be booked early: the release waits for them.

Totals: start-now 34, shared 10 (plus 5 if Sighter must build the ADR 0031 primitives), engine foundation 62, detector
and training engine 47, UI 33.5, release package 3, spikes 21: **about 210.5 agent-days** (3 fewer than the two-release
plan, which had two release packages), excluding independent reviews (about half a day per package). At three to five
agents that is three to four months of work, in line with the proposal's "thin slice, about 3 months" (section 16), plus
the waits on Debrief 1.2 and on the user's hardware.


## 11. Out of stage 2 (stage 3 and later)

RDP adapter (IronRDP, after SW10), interrupt handlers, scheduler, the Pitwall additions (naming, descriptions, grounding,
recovery, agent mode, MCP server), generated clients (C#, Java, Python, TypeScript, C++) beyond the OpenAPI document,
the hybrid adapters (`Sighter.FlaUI` and others) and their hints, `.sighterplan` import from Pacenotes, multi-application
tests, the state graph, TRX and Allure, Debrief bundles and `run.mp4`, VM snapshot pools and lab management, shadow mode,
the variation farm, OpenVINO, Linux.
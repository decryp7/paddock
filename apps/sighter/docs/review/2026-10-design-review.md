Status: review (2026-10-24), independent of the proposal's author. Docs only; nothing was built or measured.

# Sighter design review, October 2026

Scope: [README.md](../../README.md), [proposal.md](../proposal.md) (revision 4 plus the decisions of 2026-10-23/24),
[plans/poc-vm-console.md](../plans/poc-vm-console.md), [AGENTS.md](../../AGENTS.md), and the family context (Paddock
`docs/rules.md`, ADR 0007, ADR 0012 app shell, ADR 0013 workbench, `docs/ecosystem.md`, `docs/design-guide.md`).

The review uses the user's two goals as its yardstick:

1. **"UI automation like a human operating the machine."** Pixels in, keyboard and mouse out, no hooks into the app
   under test. Perceive like a human (wait until the screen settles, read text, recognise apps, dialogs and states,
   notice an unexpected popup and deal with it). Act like a human (pointer moves, typing rhythm, timing, focus).
   Understand goals (agent mode) and still stay deterministic for tests.
2. **"End to end like a super app."** The whole lifecycle happens inside Sighter, with no external tool needed: lab,
   capture, labelling, training, registry, authoring, execution, monitoring, triage, CI, agent mode, settings.

Severity: **P1** fix in the design before the affected code is written (safety, security, wrong clicks, data
integrity); **P2** fix before the milestone that needs it; **P3** clean-up.

Answers in short:

- **Is the design sound end to end?** Yes in its core. The layered resolver, "never click blindly", per-app immutable
  models, a pure core, Pitwall kept optional and plain-file plans are the right foundations, and the controller-PC
  model (Sighter drives a PC or VM over RDP or a VM console) fits "like a human" better than local injection does. It
  is not yet sound in six places: the window between locating and clicking, confidence that means different things in
  each layer, detector input resolution, the split between infrastructure and product failures, loopback hardening,
  and the order of phases (the POC is VM console first, but the proposal still says RDP first and VM console "to
  confirm"). Each one has a concrete fix below.
- **(a) Enough variation for training:** a variation farm. Lab VMs are driven through settings combinations from
  snapshots. Labels come from a lab-only UIA helper and from carrying labels across variations of the same scripted
  step. Augmentation is calibrated against measured transport distortion. Test sets are held out by screen, by
  variation cell and by app version. Each slice has a gate, and a candidate model runs in shadow mode before it is
  promoted (section 2).
- **(b) Debug HUD and monitoring:** yes. Everything comes from one trace-event stream, shown in four places: the
  live-view HUD, an optional overlay on the local screen that is hidden from capture, the "Explain this locate" panel,
  and the Lab monitor. Run videos keep the HUD as a sidecar track, never burnt into the frames, so training data stays
  clean (section 3).

---

## 1. Soundness walk-through

### 1.1 The loop: record, label, train, evaluate, deploy, run, report, triage, retrain

| Stage | What the design has | Gap | Sev | Fix |
|---|---|---|---|---|
| Record | Local capture (WGC, SCK, PipeWire), change-triggered frames, input track, redaction | The recorder is built around local capture. With the controller-PC model, most recordings of a remote target happen **through the live view**, where Sighter knows every input event exactly and sees frames after the codec | P2 | Make "record through the live view" (RDP, VNC or VM target) the main recorder for remote targets. Events come from the live view's input forwarding, so the target needs no hooks, and each recording stores the transport's codec settings as session metadata |
| Record | 2 fps floor, PNG/WebP frames | Long runs across N VMs: a 1080p PNG at 2 fps is about 2 to 4 GB per hour per target | P2 | Store frames on change only, using the dirty rectangles RDP and VNC already deliver; keep full frames at steps and failures; cap each run against free disk (rule 41) |
| Label | Review queue, fusion, auto-accept 0.85, "same as..." | The thresholds are absolute, but each layer's scores mean different things (see 1.3) | P1 | Calibrate each source and fuse on calibrated scores only |
| Train | Managed Python runtime, presets, 640 px fixed input | **A 1920 px screen squeezed into 640 px turns a 16 px icon into about 5 px**, below what DETR-family detectors find reliably | P1 | Bring the frame to 100 % scale first (the target's scale factor is known), then detect on window crops at 1:1 with overlapping 640/960 tiles and merge boxes across tiles. This also shrinks the scale axis of the variation matrix (section 2) |
| Evaluate | mAP per class, PR curves, held-out session | mAP@0.5:0.95 is not what matters for clicking. The proposal names wrong-click rate but has no gate built on it | P1 | Gate on click-point accuracy, wrong-click rate, ambiguity rate and per-slice recall at the operating threshold, over a frozen golden set (2.8) |
| Deploy | Promote and roll back, pinned versions | Promotion is a manual click with metrics on screen; nothing checks the candidate on real recent run frames | P2 | Shadow mode: the candidate runs beside `current` on live runs without acting, its disagreements go to review, and it is promoted after they are reviewed (2.9) |
| Run | Lazy locators, auto-wait, strict matching | **Time-of-check versus time-of-use**: the element is located on frame *t* and clicked at *t*+δ. Scrolling, animation, a late layout pass or a popup can move it. This is the main real-world cause of wrong clicks | P1 | Re-check just before every action: grab a fresh frame of the target box (plus a margin), compare it with the crop the locate used (NCC ≥ a threshold and no dirty rectangle over the box in the last N ms). If it moved, locate again; if it still moves, refuse. MCP `click #3` binds to the observation's `frame_id` and gets the same re-check |
| Run | Agent mode verifies each step | Scripted steps have no **post-action check** | P1 | Every action declares an expected effect (default: the screen changes within a timeout, outside ignore regions; better: the next step's target or screen appears). If nothing happens, retry once, then fail as "no effect". This is how a human notices a missed click |
| Report | JUnit, TRX, Allure, Debrief bundle | **No split between infrastructure and product failures** | P1 | One result enum: `passed`, `passed_with_recovery`, `failed` (app behaviour), `error` (infrastructure: target lost, capture black, VM revert failed), `blocked` (needs labelling or recording), `stopped` (user), `skipped`. JUnit maps `error` to `<error>` and `failed` to `<failure>`. Flake statistics count only `failed` |
| Triage | `/v1/flag`, review queue | Flags exist only as single items; nothing groups the same failure across targets or runs | P2 | Group failures by signature (plan step, screen state, resolver status, top candidate), shown as "same failure on 5 VMs" (3.5) |
| Retrain | Uncertainty ranking, flags | No rule for when to retrain, and no drift signal | P2 | Drift signals and retrain triggers (2.10) |

### 1.2 The target layer

- **The phases contradict each other (P1).** The README and proposal 7.15 say v1 = Local + RDP, VNC "if cheap", VM
  console "after RDP, to confirm". The POC that is actually being built is VNC on the VM console first. The lock
  screen, sign-in and reboots can only be tested at the console, so the POC's order is the right one for "like a
  human". Fix: proposal revision 5 makes the v1 targets **VM console (VNC) + RDP + Local (studio recording only)**,
  adds a `Hypervisor` trait and a `sighter-hypervisor` crate to section 11, and moves "VM lifecycle and snapshot
  pools" from "to confirm" to v1.
- **The RDP library is named inconsistently (P2).** The 7.15 table, section 11, section 13 and section 16 say FreeRDP;
  7.15's "Client library" says IronRDP is preferred. Pick IronRDP and spike it: NLA/CredSSP, AVC420/444 or RemoteFX,
  dynamic resize with desktop scale factor (MS-RDPEDISP), clipboard, reconnect, and the Hyper-V console preconnection
  blob (verify). FreeRDP is the fallback only as an out-of-process helper, so that the C library's signing and Team ID
  issues stay outside the app bundle.
- **Viewer-window capture and the embedded client (P3).** 7.6 makes "capture the mstsc/Citrix viewer window" v1;
  7.15 says driving mstsc breaks when it is minimised. Keep viewer-window capture only for Citrix and Horizon (no
  embedded client exists for them) and use the embedded client for RDP and VNC.
- **VNC library licence (P1, licensing).** libvncclient/libvncserver are GPL-2.0. The VNC client has to be Sighter's own
  Rust code, as the POC plans; add libvnc to `deny.toml`'s banned list.
- **The VMware console (P2, verify on day 1).** It is not certain that every current VMware Workstation release still
  has "VNC Connections", and VMware's VNC server may offer only Raw, CopyRect and Hextile (not ZRLE or Tight).
  Implement Hextile in milestone 1. VMware's VNC password is at most 8 characters (VNC DES auth) and the stream is
  **not encrypted**: keep it on loopback (the POC runs on the VM host) and say so in the target setup. Never use plain
  VNC across a network without a tunnel.
- **VirtualBox VRDE needs the Oracle Extension Pack** under the PUEL licence, which is not free for commercial use
  (verify the current terms). Mark it as "your licence" in the target setup and prefer QEMU/KVM, Hyper-V or VMware for
  pools.
- **The RDP host certificate (P2, security):** pin it on first use per target, show the fingerprint, and refuse a
  change without the user's confirmation; never accept any certificate silently.
- **Coordinate mapping (P2):** one pure function per target, from frame pixels to session coordinates, covering client
  scale, session scale, monitor offsets and VNC framebuffer resizes. It is property-tested and its geometry is recorded
  in every run bundle. The proposal mentions this in 7.6 and 7.12 but never as one mechanism.

### 1.3 The resolver

- **Confidence on one scale (P1).** NCC similarity, the detector's class score, OCR character confidence and the VLM's
  "certainty" are not comparable, yet one `min_confidence`, the 0.6/0.85 thresholds and `MinConfidence(0.8)` apply to
  all of them. Fix: each layer emits a raw score plus a **calibrated probability that this candidate is the intended
  target**, fitted per layer (and per model) on reviewed run outcomes. Use isotonic or temperature scaling, and fall
  back to conservative defaults until enough data exists. Thresholds, the auto-accept rule and the API apply only to
  the calibrated value; the raw value stays in the trace.
- **Decide on margin as well as score (P1).** Strictness ("two matches is an error") needs a rule: *found* when
  top-1 ≥ τ and top-1 − top-2 ≥ m; *ambiguous* when the margin is below m; *not found* below τ. Defaults: τ 0.8,
  m 0.2. They are Settings (rule 21) and can be overridden per step.
- **A cheap layer short-circuits only after cross-checking (P2).** A template or cache hit is accepted on its own only
  when the fingerprint is the same screen and a second cheap check agrees (the OCR text inside the box matches the
  query name, or the box matches the last known geometry). Otherwise the layers continue and their results are fused.
  This stops a look-alike template winning on a new screen.
- **Anchor semantics are not defined (P2).** `RightOf("Part no.")` needs fixed geometry: the same row band (vertical
  overlap ≥ 50 % of the smaller box), the nearest box to the right, at most k × the anchor's height away, and the label
  box excluded from the result. Write the relations as pure, documented functions with tests, as SikuliX's offset
  pitfall (proposal section 13) warns.
- **The OCR engine (P3).** The POC uses `Windows.Media.Ocr`; the proposal names PaddleOCR and `ocrs`. Put an
  `OcrEngine` trait in the design with the Windows OCR adapter in it, and record the engine and its version in run
  bundles and model cards (results differ by engine and by Windows build).

### 1.4 Determinism and repeatability

- **Bit-identical results are promised too broadly (P2).** ONNX Runtime GPU execution providers (DirectML, CUDA,
  CoreML) are not bit-identical across GPUs or drivers, and OS OCR changes with Windows updates. Fix: the promise is
  "identical on the same execution provider, driver and OCR engine". The CPU FP32 path is the reference, and results
  carry `{ep, ep_version, driver, ocr_engine}`. Replays in the Playground run on the reference path.
- **Repeatable runs need a fixed environment as well as a fixed model (P1 for unattended suites).** Add a **target
  preflight** at the start of every run that checks what the plan declares and refuses on a mismatch, as "environment
  mismatch", never as a test failure. It checks:
  resolution and scale (from the RDP session parameters or the VNC framebuffer), the keyboard layout (type a probe
  string into a known field when the plan allows), theme, language, screensaver and lock timeout off, notifications in
  Do Not Disturb, animations off (where the plan asks for it), the mouse cursor shown or hidden consistently, Caps and
  Num Lock state, and the clock.
- **Snapshots** are the reset (revert, then power on, then wait for a known screen). Watch out for clock skew after a
  revert (resync time as a step) and for domain-joined guests losing their machine account password after a long
  revert gap. Document lab VM settings: workgroup or disabled password rotation, Windows Update paused.
- Pseudo-randomness (path generation, humanised input, augmentation) always takes a seed that is recorded in the run.

### 1.5 Safety: the input guard and the stop hotkey

- **Two hotkeys (P1).** Proposal section 10 says Ctrl+Alt+Shift+Esc; 7.15 and the POC say Ctrl+Alt+Shift+F12. Fix:
  **one** default, Ctrl+Alt+Shift+F12, in a single Setting, with "panic key" and "stop hotkey" as one concept.
- **Where the hotkey runs (P1).** It must not depend on the webview or the server's work threads (rule 19: the control
  that stops X never depends on X). Run `RegisterHotKey` (and its macOS/Linux equivalents) on a dedicated thread with
  its own message loop in the process that owns the input driver. That thread sets an atomic stop flag which the
  driver checks before every injected event, then releases held keys and buttons. The live view must never forward
  the stop combination to the target, even while it captures Win-key combinations.
- **A dead-man rule for the input driver (P2):** if the stop thread or the run loop stops sending heartbeats
  (deadlock, a hung inference call), the driver releases everything and refuses further input. Cap the input rate
  (events per second) so a bug cannot flood a target.
- **"Physical input pauses the run" needs a definition per target (P2).** Local: Windows input that is not marked
  injected (`LLKHF_INJECTED`/`LLMHF_INJECTED`), and the equivalents elsewhere. RDP and VNC: input inside the live view
  pauses the run; input elsewhere on the controller does not. A person at the target PC is invisible to Sighter, so the
  target setup warns about it (and on client editions the console session is taken over).
- **The "agent in control" border on remote targets (P3):** without an agent on the target, nothing can be drawn on
  its screen. State plainly that the border and the HUD live in the controller's live view and the tray, and on the
  target only for the Local target (3.2).
- **Confirmations are Sighter's own windows (P2).** "Confirmation by the user, never the model" requires a modal OS
  window owned by Sighter (rule 17), never a prompt inside the MCP host. In unattended runs, destructive actions are
  refused rather than confirmed.
- Deny-lists, the destructive-word list and the known-dialogs list are **TOML packs** (rule 21), so a lab can extend
  them without a rebuild.

### 1.6 Security

- **Loopback is not a security boundary against the browser (P1).** Any web page can POST to `127.0.0.1`, and DNS
  rebinding gets past same-origin checks. Fix: require the API key in a header (never a cookie), reject requests whose
  `Host` is not a loopback literal or the configured name, reject any request with an `Origin` header unless the origin
  is allow-listed, use a non-default port, and on Windows offer a named pipe with an ACL for local clients. Add tests
  for each rule.
- **Same-user processes can read the OS credential store** (Windows Credential Manager, the macOS login keychain for an
  unsigned binary after approval). State the threat model: Sighter protects against other users and against secrets
  leaking into files, not against malware running as the same user. Store the RDP/VNC credentials and API keys there,
  and nothing else.
- **Secrets on screen (P1).** A typed `{{secret:...}}` is masked in key logs, but the screen may still show it (a "show
  password" eye, a field that does not mask, an echo in a terminal). Fix: a step that types a secret marks its target
  box as a redaction region for the rest of that screen state, in frames, video and HUD; a secret scanner runs on OCR
  text before a bundle is exported; and secrets are never shown as HUD text (only "•••• secret NAME").
- **Prompt injection from screen text** is already covered for agent mode (7.14). Apply the same rule to recovery
  (7.10): screen text is data inside a delimited field, never instructions.
- **The supply chain of the training runtime:** keep the pinned hashes. Add an **offline bundle** (a zip with the same
  manifest) for air-gapped labs, which a "super app" needs in locked-down environments.
- **MCP:** the per-client key and audit log of ADR 0007. Add a **permissions model** (section 4): scopes per key or
  client (`locate`, `observe`, `act`, `record`, `train`, `admin`) and per target.

### 1.7 Performance budgets

Rule 16 applies, but the proposal lists only per-layer p50 targets. Add these to the nightly benchmark, with p50 and
p95 measured on release builds at low load:

| Budget | Proposed goal (to confirm in the spike) |
|---|---|
| Frame arrival to decoded frame (RDP/VNC, 1080p) | p95 ≤ 50 ms on loopback, ≤ 100 ms on a LAN |
| Locate end to end (fingerprint, cache, OCR region, detector), 1080p | p50 ≤ 80 ms, p95 ≤ 250 ms on GPU; p95 ≤ 600 ms on 8-core CPU |
| Action end to end (locate, re-check, inject, first visible effect) excluding the app's own latency | p95 ≤ 400 ms |
| HUD overhead on the server | ≤ 2 ms per frame; trace on or off must not change step timing by more than 5 % |
| Parallel targets on one controller (14 cores, one 8 GB GPU) | 8 targets at 1080p with batched detector inference, no target's locate p95 above 2× its single-target value |
| Model cold load | ≤ 1.5 s; first locate after load ≤ 2 × warm |
| Server memory with 3 models loaded | ≤ 1.5 GB RSS (PSS on Linux, rule 26) |
| Studio canvas | 60 fps pan and zoom with 500 boxes; review queue keystroke to next item ≤ 50 ms |

Two design points follow. Change detection should use the transport's **dirty rectangles** (RDP surface updates, VNC
rectangles), which costs nothing and is more precise than frame diffs. Detector inference should be **batched across
targets**, with interactive locates ahead of batch work in the queue (rule 14: a capture, decode, detect, decide
pipeline with back-pressure).

### 1.8 Failure modes and recovery

The proposal covers resolver failures well. The infrastructure failures that remote and VM targets bring are missing.
Each needs a behaviour and a test that triggers it (AGENTS.md):

| Failure | Behaviour |
|---|---|
| Target disconnects mid-step | Reconnect with back-off up to a budget; resume only at a step boundary after the screen state checks out; otherwise `error` |
| Frames stop arriving (hung server, frozen guest) | A watchdog fires after N s with no frame update while one is expected; ask for a full frame refresh; then `error`, with "frozen" diagnosed against the hypervisor's VM state |
| Black or secure surface (DRM, secure desktop through the wrong route) | Detect a uniform frame; `error` with the next step ("use the VM console target") |
| Lock screen or screensaver appears | Recognised as a known screen state (pack); a plan-level handler signs in again if allowed, else `error` |
| Unexpected dialog (Windows Update, toast, "Session expired") | Declarative **interrupt handlers** (section 4) before any AI recovery |
| App under test crashes | Recognise WER, "not responding" and crash dialogs as known screens; `failed` with "app crashed" as the cause and a VM snapshot kept for inspection |
| Server or controller crash | Run bundles are append-only and crash-safe (each step is flushed), so evidence survives; next start offers "mark run interrupted" |
| Modifier keys stuck after a crash | On every (re)connect, send key-up for all modifiers and release the buttons |
| Snapshot revert fails | `error` for that target, and the pool takes the VM out of service (status "needs attention") |
| Disk fills up | Free disk is checked before and during a run; frame storage degrades to "steps and failures only" before the run fails |

### 1.9 Licensing

The model exclusions in the proposal are good. Additions:

- **GPL VNC libraries** (libvncclient/libvncserver) are excluded (1.2). **PiKVM** is API only, as stated.
- **The VirtualBox Extension Pack** (PUEL) is the user's licence, never bundled.
- **COCO-pretrained weights:** COCO images carry mixed Flickr licences and the annotations are CC-BY-4.0. Industry
  practice treats weights as usable, but record it in the model card and the runtime manifest report.
- **DINOv3** (DEIMv2, RF-DETR variants): its own licence, not OSI (verify before any Accurate preset uses it).
  **DINOv2** is Apache-2.0 (verify the checkpoint).
- **Windows test VMs:** guest OS licences (or evaluation expiry) are the lab owner's concern. Snapshot pools of client
  editions need licences per VM; say so in the pool setup.
- **RDP to Windows Home** is impossible (there is no RDP server); the target setup detects it and points to the VM
  console target.

### 1.10 Windows, macOS and Linux

| Role | Windows | macOS | Linux |
|---|---|---|---|
| Controller (Sighter, server, live view, HUD) | **v1** (the POC host) | after a real signing identity: with ad-hoc sealing (rule 23) the Screen Recording and Accessibility grants are tied to the code hash and reset on every update, which proposal section 4 already notes | Later (X11 first; Wayland needs portals and libei) |
| Target over RDP | **v1** (Pro/Enterprise/Server) | no (no RDP server) | Next (xrdp, gnome-remote-desktop) |
| Target over VNC or VM console | **v1** (VMware, QEMU, Hyper-V console over RDP/VMBus) | Next: macOS Screen Sharing needs Apple's ARD (Diffie-Hellman) auth, not plain VNC auth | Next |
| Local target (studio recording, local tests) | **v1** | with the controller | with the controller |
| Training runtime | CUDA, CPU | MPS, CPU | CUDA, ROCm best effort, CPU |

**A contradiction (P2):** proposal section 4 says "unsandboxed with hardened runtime on macOS"; Paddock rule 23 says the
hardened runtime is off while sealing is ad-hoc. Follow rule 23 and move the macOS controller behind real signing.

**A gain the proposal misses:** remote and VM targets do **not** need an interactive desktop on the controller. A
Pitcrew runner driving VMs can run `sighter-server` as a plain service; only the Local target needs the logged-in
session that proposal section 9 worries about.

### 1.11 Measured against "like a human operating the machine"

Every place the design relies on access a human does not have, classified:

| Where | Access used | Class | Fix |
|---|---|---|---|
| `Sighter.Launch(exe)`, `Attach(pid)`, `WaitForProcess`, MCP `launch_app` | Process APIs on the target | **Not possible on remote or VM targets without an agent** | Launch the way a human does: Start-menu search, Win+R and type, or a desktop icon, as built-in sub-flows; process APIs only on the Local target, as a shortcut |
| `ActivateAsync`, foreground-window check, z-order and occlusion (7.12) | Win32 window APIs | Optional shortcut | A pixel path is required: recognise title bars and the taskbar, click the window or Alt+Tab, check focus by its visual cues (active title bar, caret, focus ring) |
| Scope binding "window or process", deny-list "of titles" (section 10) | Window titles and pids | Not available remotely | Scope by **recognised app and screen state** (fingerprint plus OCR of the title bar); titles from OCR on remote targets |
| Password-field auto-pause (UIA `IsPassword`) | Accessibility | Optional | Pixel cue: detector class `password_field` (masked dots) plus the secret-typing rule of 1.6 |
| `Sighter.Clipboard.ReadAsync` | OS clipboard | Human-like enough over the RDP clipboard channel; none over VNC | Say so per target; for VNC, a typing-only mode |
| Hybrid adapters (FlaUI, pywinauto, Appium) | UIA | The user's choice, opt-in | Fine; documented as non-human assistance |
| A11y label sources, lab guest helper (2.3) | UIA, guest exec | **Lab only, labels only** | Never loaded by `sighter run`; a test fails the build if the runner links them |
| Dyno metrics, hypervisor VM stats | Host side | Observation, not acting | Fine |
| Local target `SendInput` | Injected input, marked as injected | Weaker than human: some apps (anti-automation, games, secure desktop) reject it | Prefer RDP, VM console or IP-KVM, whose input reaches the guest as device input |

What still separates the design from "like a human", and the fix for each (also in the feature list, section 4):

- **Perceive**
  - **Visual settling**: stability as "no dirty rectangles over the region for N ms, ignoring caret and spinner
    regions". A caret blinks forever, so frame-diff stability alone never settles.
  - **Spinners and progress bars**: classes in the taxonomy, so "wait until the progress bar is gone" is a target.
  - **Reading tables and lists**: `Cell(row, column)` exists. Add `extract_table`, a list `items()` reader and
    reading values out of a grid.
  - **Colour and state**: checked, selected, disabled and error-red are classes or attributes. Add a colour probe on
    an element ("the status light is red") that returns the named colour and its distance, never only RGB.
  - **Focus**: detect the caret and the focus ring, so "type here" checks that focus actually landed (a human looks).
  - **Notifications and toasts**: watch the desktop-level screen-state change while waiting.
  - **Sound cues (Later)**: RDP audio output redirection carries the target's sound. An "error beep heard" or "sound
    played" assertion is possible without an agent; VNC carries no audio.
- **Act**
  - **Pointer moves**: today the pointer jumps to the target. Hover menus, tooltips and drag thresholds need real
    moves. Use a seeded, humanised path (eased curve, configurable speed). The default is "direct", and "humanised" is
    switched on per step or per app.
  - **Typing rhythm**: a seeded per-key delay; Unicode or scancode input per transport (RDP Unicode events, VNC
    keysyms); IME composition Later.
  - **Drag and drop**: press, move in steps past the OS drag threshold, hover, then release, with a post-check.
  - **Scroll to find**: wheel or scrollbar in steps, re-locating after each step, with a scroll budget, then stop at
    the end of the list (the screen no longer changes).
  - **Window handling**: move, resize, maximise and snap by the title bar; multi-monitor by index; virtual desktops
    Later.
  - **Context menus, double-click timing, modifier clicks, long-press**: in the action vocabulary.
- **Understand**: agent mode (v1) chooses among labelled elements. For tests, the deterministic path is plans plus
  interrupt handlers, and AI recovery stays opt-in and reported. An agent run can be saved as a plan, so "understand
  the goal" turns into "repeat deterministically".
- **The hardware path**: IP-KVM (Later) is the purest form of "like a human": HDMI in and USB HID out, nothing on the
  target, BIOS included. Keep it behind the same `Target` trait. The VM console already covers BIOS and logon for
  virtual machines.

### 1.12 Measured against "a super app, end to end"

The whole lifecycle mapped onto the Paddock workbench (ADR 0013). Each **activity-bar area** has an Explorer provider,
document views, and work that runs as `paddock-jobs` in the Jobs pane.

| Area (activity bar) | Explorer provider | Views (dock) | Jobs | v1 / Later | External tool today, and how Sighter wraps it |
|---|---|---|---|---|---|
| **Lab** | Targets (PCs, VMs, pools), credentials (references), hypervisors | Target setup and preflight, live view with HUD, snapshot tree, pool status | Snapshot revert, clone, power, preflight | **v1** (the proposal has no section on lab management: add one) | Hypervisor CLIs (`vmrun`, `VBoxManage`, `virsh`/QMP, Hyper-V PowerShell). Wrapped by the `Hypervisor` trait, with detection, version check and a Repair button (rule 19). They ship with the hypervisor, so this is not an install step for Sighter |
| **Capture** | Recordings by app, session and variation cell | Frame timeline, recording player, coverage matrix | Record, import video, variation farm runs | **v1**; variation farm next | ffmpeg only for importing others' videos (managed LGPL download, Debrief ADR 0033); own recordings need none |
| **Label** | Datasets, review queue by reason | Frame canvas, review queue, "same as..." grid, dataset health | Pre-label batches (OCR, detector, Pitwall), dedup | **v1** | CVAT/Label Studio: import and export only, never needed. Pitwall pre-labels optional |
| **Train** | Training runs, models per app (registry) | Training monitor (loss, mAP), evaluation report per slice, version compare, model card | Train, evaluate, shadow-mode report, export ONNX | **v1** (shadow mode next) | **Python/PyTorch**: the managed, sandboxed, downloaded runtime (user decision 2026-10-24), plus an offline bundle. The user never installs Python |
| **Tests** | Plan library (folders of `.sighterplan`), sub-flows, data tables, interrupt handlers | Step editor, recorder-to-plan, data table editor, state graph and coverage | Plan import dry-run, path generation | **v1** (state graph coverage v1, path generation next) | Pacenotes is optional (plans by hand or from recordings) |
| **Runs** | Run history, schedules, run sets | Run queue, run report (steps, video with HUD sidecar, trace), failure groups, flaky report, run compare | Execute, export bundle and reports | **v1**; a simple scheduler next | Debrief is optional for log correlation; **Sighter's own run viewer is v1**, so Debrief is never needed to see a run. Pitcrew is optional: `sighter run` with JUnit works in any CI |
| **Monitor** | (none) | Lab wall: all active targets (3.5) | none | **v1** | none |
| **Agent** | Conversations, saved goals | Assistant pane (ADR 0013) with tool-call log, confirmations as OS windows | Agent runs (budgets) | **v1** (user decision) | Pitwall (a family app, optional; "needs local AI" state) |
| **Playground** | (none) | Locate on a dropped or live frame, every layer, explain | none | **v1** | none |
| Shell (ADR 0012) | none | Settings (engine-described sections), Updates, About, notifications, service health (server, Pitwall, hypervisors, training runtime) | Downloads (runtime, OCR models) | **v1** | none |

**Contradiction with ADR 0013 (P2):** proposal section 11 lists *pages* (Home dashboard, Recordings, Label, Models,
Playground, Runs, Keys). ADR 0013 makes Sighter a workbench app. Replace the page list with the table above.

**What the super app still lacks:** a **lab management** section (targets, pools, credentials, preflight); a
**scheduler** (simple: cron-like schedules and "on new build in folder X"; Pitcrew stays the CI path); an
**in-app run viewer and triage**, so Debrief is optional; and **settings for every tunable** (rule 21). Generated
clients live in `sighter-clients` (NuGet, Maven, PyPI, npm). They are part of the product but installed by the
engineer's package manager, so the studio should offer "copy as C#/Python/..." from any step or locate.

---

## 2. Training data variation: the plan (question a)

The goal is stated as a measurable test: **every slice the tests will run in has enough held-out instances, and the
model passes the gate in each one.** Variation is planned, generated, checked and then measured; it is not hoped for.

### 2.1 The axes and how much of each

| Tier | Axis | Levels (example for a Windows desktop app) | How it is crossed |
|---|---|---|---|
| A | Theme | light, dark, high contrast (Aquatic/black, Desert/white) | **fully crossed** with scale |
| A | Scale | 100, 125, 150, 175, 200 % | fully crossed with theme |
| B | Resolution and window size | 1366×768, 1920×1080, 2560×1440; maximised, default, small | pairwise |
| B | OS version | Windows 10 22H2, 11 23H2/24H2, Server 2022 | pairwise |
| B | Text rendering | ClearType on, grayscale, off | pairwise |
| B | Language and script | en, de (long words), ja (CJK), ar (RTL), pseudo-locale | pairwise |
| B | Accent colour, classic or themed common controls | 2 to 3 each | pairwise |
| B | Transport | local lossless, RDP AVC444, AVC420, RemoteFX, VNC Raw/Hextile/Tight-JPEG at 2 quality levels | pairwise; a lossless reference is always kept |
| B | App version | each release under test | sliced, never mixed into a test split |
| C | Element state | normal, hover, focus, pressed, disabled, selected or checked, error, empty or filled, scrolled | **inside every screen**, generated by driving |
| C | Content density | empty list, full list, long text, truncated text | inside every screen |
| C | Disturbances | tooltip, toast, partial occlusion, cursor over element | inside every screen and by augmentation |

A full cross of Tier B is impossible: it would be thousands of cells. Use a **pairwise covering array** (every pair of
levels appears at least once), seeded and stored in the project. The cells that most change how elements look (theme ×
scale) are fully crossed. With scale normalisation (1.1), scale moves partly from "appearance" to "residual font hinting
and layout", so 3 levels (100, 150, 200 %) may be enough. The spike measures it.

**Minimums** (in Settings, rule 21; starting values): per class and per Tier-A cell, ≥ 50 training instances on ≥ 10
distinct screens, and ≥ 20 test instances. A cell below the minimum shows as **"not measured"**, never as passing.

### 2.2 The coverage matrix in the studio

A heatmap view: rows are classes (and states), columns are slices (Tier-A cells, then a Tier-B axis picker). Each cell
shows train/test instance counts and the last model's recall, with a word and icon for status (Paddock rule: never
colour alone). Clicking a cell opens its crops. "Fill gaps" turns every empty or thin cell into a capture task for the
variation farm. A run in an environment cell that has no coverage reports "untrained cell" (proposal 7.4).

### 2.3 Generating variation automatically: the variation farm

1. **A template VM per OS version**, with snapshots: `base`, then one per language pack. The app under test is
   installed by a setup plan.
2. **Settings through the hypervisor** (guest exec: VMware Tools `vmrun runProgramInGuest`, Hyper-V PowerShell Direct,
   `VBoxManage guestcontrol`, QEMU guest agent). The commands set the theme (`AppsUseLightTheme`,
   `SystemUsesLightTheme`), high contrast (`SystemParametersInfo SPI_SETHIGHCONTRAST` or a `.theme` file), font
   smoothing (`SPI_SETFONTSMOOTHING`), accent colour and language. They run from a TOML pack of settings recipes, never
   code from a model.
3. **Scale and resolution through the RDP session**: the client's desktop scale factor and monitor layout (MS-RDPBCGR
   core data, MS-RDPEDISP resize) change scaling per session without touching the guest and without a sign-out. For
   VNC or console capture, set them in the guest and sign out and in.
4. **Drive the same recorded flows** (Sighter's own plans) in every cell of the covering array. Every step's frame is
   then a *paired* sample: the same screen state in another appearance.
5. **Labels without a human per frame:**
   - **The lab UIA helper** (Windows only, inside lab VMs only): a small Rust helper started by guest exec that dumps
     UIA/MSAA boxes for the captured frame. It is a `LabelSource` and is verified by pixels (proposal section 5:
     reject off-screen, zero-sized, occluded or uniform boxes). It never runs at test time (1.11).
   - **Label transfer**: the labels reviewed in the base cell are carried to the same step in other cells, through the
     known scale mapping, then refined by local template or OCR re-localisation. A carried box is auto-accepted only
     when two sources agree (IoU ≥ 0.6 with the detector or UIA box, and the OCR text equal). Otherwise it goes to the
     review queue. This is what makes pure-vision legacy apps (no UIA) affordable.
   - **States** come from the plan: a scripted hover, focus (Tab), press (mouse down, capture, Escape), disabled (a data
     row that disables the control) and error (invalid input). The plan step names the state, so the label carries the
     state attribute for free.
6. **A control zoo** for the base and OS-dialog models: a small invented Win32/WinForms/WPF/Qt gallery app that shows
   every class in every state. It is generated, pre-labelled by its own layout, licence-clean, and contains no user
   data.
7. The farm is a pipeline in `paddock-jobs` (rule 14): revert, apply settings, drive, capture, label, dedup, commit.
   Concurrency per host is bounded by RAM and CPU (rule 13, rule 41). Progress is per cell and Cancel is safe (a cell
   commits only when complete).

### 2.4 Augmentation that matches real distortions

Use augmentation only for what cannot be captured cheaply, and **calibrate it**:

- **Measure the transport gap**: for paired frames (guest-side lossless capture through the lab helper versus the frame
  after RDP or VNC), record SSIM and PSNR and the edge-blur and chroma-bleed statistics per transport setting.
- **Augment to match**: H.264/AVC420 encode and decode with 4:2:0 chroma at the measured bitrates, JPEG at the VNC Tight
  qualities, colour-depth reduction (16-bit, 8-bit bgr233), RDP progressive-codec tile artefacts, and resampling with
  the kernels Windows uses for DPI-unaware bitmap scaling (bilinear) at the exact ratios (1.25, 1.5, ...). Keep jitter
  at ±3 % around real ratios, never 50 to 300 % uniform.
- **UI disturbances**: cursor sprite over and near elements, caret, hover tint, tooltip or toast pasted with partial
  occlusion, elements cut at the window edge, copy-paste of labelled crops onto real backgrounds of the same app.
- **Never**: flips, rotations, perspective, or a heavy colour jitter that turns a disabled grey into an enabled one
  (state classes depend on colour; the jitter is checked per state class).
- **Validate**: train once with and once without each augmentation family and measure on the *real* transport slice.
  Keep only the families that help (an ablation report in the model card).

### 2.5 Balance and class imbalance

- Dataset health shows per class and per slice counts, distinct screens and the annotation position heatmap. A class is
  "balanced" at 75 to 200 % of the mean (proposal section 13, Roboflow rule).
- Fix rare classes with **repeat-factor sampling** (image-level oversampling by the rarest class it contains, as in
  LVIS) and copy-paste, not by duplicating near-identical frames. Under-sample dominant screens by capping frames per
  screen cluster.
- **Negatives**: include frames where a class is absent (empty dialogs, other apps), plus **hard negatives** mined from
  wrong clicks and false positives in shadow mode.
- Report class confusions that matter to the resolver (button, link, tab, menu item) per slice. Merge classes the app
  does not distinguish visually rather than train noise.

### 2.6 Near-duplicate removal

- Within a recording: pHash plus SSIM on downscaled grayscale (proposal section 5), capped at N frames per screen
  cluster per variation cell (default 5).
- **Across splits**: after splitting, check that no near-duplicate crosses train and test (pHash distance on the whole
  frame and on each labelled crop). Leakage here is the most common reason a UI detector looks great in evaluation and
  fails in use.

### 2.7 Held-out splits

Never split by random frame. Four test sets, each reported separately:

| Test set | Held out by | Question it answers |
|---|---|---|
| Screens | screen cluster (fingerprint groups), grouped k-fold | Does it work on screens it has not seen? |
| Cells | whole variation cells (for example all 175 % dark) | Does it generalise to an appearance it was not trained on? |
| Version | the newest app version (train on 5.1, test on 5.2) | Will it survive the next release? This is the real deployment case |
| Transport | each real transport slice | Does the codec augmentation close the gap? |

Plus the **golden set**: frozen, versioned and only extended explicitly. Its frames are reviewed twice, and it is the
basis of every promotion gate.

### 2.8 Evaluation gates per slice

The model card holds a table of class × slice with: recall at the operating threshold, precision, **click-point
accuracy** (predicted centre inside the true box, and inside its inner 60 %), localisation error relative to box size,
expected calibration error, and a confusion matrix. Resolver-level metrics on the golden set: **wrong-click rate**,
ambiguity rate and needs-labelling rate.

Promotion gates (Settings; starting values):

- the wrong-click rate on the golden set ≤ 0.1 % and never worse than `current`;
- no class × slice recall falls by more than 2 points against `current`, where the drop is significant for the counts
  (a binomial test), and slices with too few instances show "not measured";
- every Tier-A cell used by an unattended suite is measured;
- the calibration error ≤ 0.05 after recalibration;
- the shadow-mode disagreement set is reviewed (2.9).

A failed gate blocks promotion and says which slice failed. An override needs a reason and is written into the model
card.

### 2.9 Active learning from production misses, and shadow mode

- **Sources**, ranked by impact (the number of plans that depend on the element): user flags (`/v1/flag`, and a new
  **"this click was wrong"** action in the run viewer); every `passed_with_recovery`; locates whose margin is near the
  threshold; disagreements between layers (detector against OCR or template); disagreement between `current` and the
  shadow candidate; unknown fingerprints.
- **A sampling budget** per app per week (Settings), deduplicated against the dataset before it enters the queue.
- **Shadow mode**: the candidate model runs on live runs beside `current`, without ever acting, for N runs or until
  enough locates are covered. Its disagreements go to review. Promote after they are reviewed and the gates pass.
  Rollback is one click and stays pinned in the run record.

### 2.10 Drift detection and when to retrain

Signals, tracked per app, app version and environment cell (each one has a threshold in Settings):

- the share of locates answered by fallback layers (VLM, needs labelling) rises;
- the median top-1 margin falls, or the ambiguity rate rises;
- the rate of unknown screen fingerprints rises (a new app version changed screens);
- OCR confidence falls on a slice (a font or rendering change);
- a run's environment falls outside the coverage matrix.

**Retrain when** a new app version changes more than X % of known screens, *or* the reviewed backlog reaches N labels
(default 200) for an app, *or* a slice falls below its gate in the weekly golden-set and replay check, *or* a new level
appears on an axis (a new Windows build). Never retrain on a timer alone. Each retrain produces a candidate that goes
through gates and shadow mode; nothing promotes itself.

---

## 3. Debug and monitoring: the HUD (question b)

### 3.1 One mechanism: the trace-event stream

Every view below renders the same **typed trace events**, emitted by the server for each run, step and locate. Events
are `run.started`, `step.started`, `locate.layer` (layer, candidates with raw and calibrated scores, ms),
`locate.decided` (status, winner, margin, rule), `wait.tick` (elapsed, timeout, stable for ms), `act.planned` (point,
re-check result), `act.done`, `verify.result` and `step.result`. They carry `frame_id`, `trace_id` and monotonic
timestamps. They are bounded (top-k candidates per layer, default 5) and redacted at the source (1.6). Tracing is
**always on** at low cost, so turning the HUD on or off never changes timing (no Heisenbugs; budget in 1.7). This is
rule 40's "one shared mechanism": the live HUD, the on-screen overlay, the run video, the Explain panel, Debrief
bundles and the monitor cannot disagree, because they read the same data.

### 3.2 Where the HUD appears

| Surface | When | How |
|---|---|---|
| **Live-view HUD** (the main one) | any target (RDP, VNC, VM, Local) | Drawn over the live view in the studio (a workbench view, a pop-out to any display). Costs nothing on the target |
| **On-target overlay** | Local target only, opt-in | A transparent, topmost, click-through, non-activating OS window (Windows `WS_EX_LAYERED \| WS_EX_TRANSPARENT \| WS_EX_NOACTIVATE`; macOS `ignoresMouseEvents`). **It must be excluded from Sighter's own capture** (Windows `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`, Windows 10 2004 and later; macOS an `SCContentFilter` that excludes the window). Where exclusion is not available, the overlay is refused, never flickered on and off around captures. Otherwise the overlay itself would be detected, clicked and trained on |
| **Remote targets** | RDP, VNC, VM | No overlay on the target screen (no agent), as stated in 1.5; the live view and tray show "Sighter is driving" |
| **Run video** | after any run | The HUD is a **sidecar track** (`trace.ndjson` aligned to frame timestamps) rendered at playback in the run viewer and Debrief. "Export video with HUD" burns it in as a job. Burnt-in exports are tagged `hud=true` and **the dataset importer refuses them**, so HUD pixels never become training data |

### 3.3 What the HUD shows

- **Boxes per resolver layer**, each with: a layer tag letter, class, OCR text or name, calibrated confidence (two
  decimals), and ms. Example: `D  button "Save"  0.93  14 ms`.
  - Winner: 3 px solid outline and a filled label chip.
  - Other candidates: 1.5 px dashed.
  - Filtered candidates: 1 px dotted, with the reason in the chip (`disabled`, `occluded`, `outside scope`,
    `margin 0.08`).
  - Anchors: a thin line from the anchor box to the target, with the relation word (`right of "Part no."`).
- **The click point**: a crosshair at the planned point, plus the re-check result (`stable ✓ 120 ms` or
  `moved, re-locating`).
- **The step bar** (top): plan, case and step i/N; the action and target in words; the phase (Locating, Waiting,
  Acting, Verifying) with its own timer. A wait shows elapsed against timeout as a ring plus numbers ("2.3 / 8.0 s"),
  with "stable for 340 ms". Status is an icon plus a word plus a colour: found, ambiguous, not found, refused, needs
  labelling.
- **Screen state**: the recognised state name ("Save as dialog") and match score, and the app and model version.
- **The next expected target**: a ghost box where the next step's target was on the last passing run (from the trace
  history or the state graph), labelled `next: click "Search"`.
- **The input trail**: the last N pointer positions as numbered dots (1, 2, 3); click markers with the button;
  drag paths; key chips ("typed 6 chars", "Ctrl+C", secrets as "•••• secret NAME").
- **The timing strip**: a stacked bar per locate (capture, cache, template, OCR, detector, VLM) and per action
  (re-check, inject, verify).
- **The target health strip**: connection, codec, fps, RTT, dirty-rectangle rate, and the VM's CPU and RAM.
- **Toggles**: per layer (C, T, O, D, V, H), winner only, candidates, anchors, trail, timing, next target. HUD opacity
  and a density limit (top-k per layer; labels collapse into number chips listed in the side panel when they would
  overlap). Each toggle has a key, and they are remembered per view.
- **Debug controls in the same view**: breakpoints on steps, "pause before action" (shows the planned click and waits),
  Step, Continue, "edit this locator and re-run the step on the live screen", "run from here", and Stop. This makes
  the HUD a debugger rather than a decoration.

### 3.4 "Explain this locate"

A modeless tool window (rule 17), opened from a box, a step or a failure. For one `trace_id` it shows:

1. the query as parsed (class, name, text, relations, region, model pin);
2. per layer: ran or skipped, and why ("skipped: cache miss", "skipped: query has no text"), its candidates with raw
   and calibrated scores, and its ms;
3. the constraints each candidate met or failed (anchor found at x,y; relation distance; enabled; visible; inside
   scope);
4. the decision rule as applied ("top-1 0.93, top-2 0.41, margin 0.52 ≥ 0.20, so found");
5. the frame hash, model, OCR engine and execution provider (enough to replay exactly);
6. actions: **replay in Playground** (the same frame and the reference path, deterministic), flag for labelling, save
   as template, label this element, copy as a locator in C#, Python, ... or as a plan step, and open the run in
   Debrief.

Screen readers get the same content as a structured list. The HUD canvas has a text alternative: the step bar is a
polite live region announced on phase changes, never every frame.

### 3.5 The Lab monitor (many targets)

A workbench view in the **Monitor** area, for parallel runs across VMs and PCs:

- **A table by default** (with a "wall" mode of live thumbnails at 1 to 2 fps). Columns: target; status (icon and
  word); plan and case; step i/N with a sparkline of step durations; the current screen state; elapsed; the last
  result; VM CPU and RAM (hypervisor or Dyno); stream fps and RTT.
- **A controller header**: CPU load, GPU use and inference queue depth, free disk, and running/queued/max targets
  (rule 41 scaling).
- **Actions**: open the live view (pop out to any display), pause, stop, stop all (the hotkey also stops all), and
  take a snapshot now.
- **Failure groups**: "the same failure on 5 targets" by signature (1.1 triage), one Paddock notification per cause key
  (ADR 0010), never one per target.
- **History**: per pool, the pass/fail/error trend and the infrastructure error rate, kept apart from product failures.

### 3.6 Colour and accessibility rules (Paddock design guide)

- **Layer colours** come from the Paddock categorical palette in a fixed order by identity: OCR = 1 sky blue,
  human/label = 2 orange, detector = 3 bluish green, VLM = 4 reddish purple, template = 6 periwinkle, cache = 8 grey.
  Each layer also has its **letter tag and line style**, so colour is never the only cue.
- **Status colours** (success, warning, danger, info) are used only in the step bar and the monitor, always with icon
  and word. Danger is reserved for refused or failed. Boxes never use red against green.
- **Contrast on arbitrary screens**: every outline is drawn with a 1 px halo in the opposite lightness (dark inside,
  light outside), so it stays at 3:1 or more against any background (SC 1.4.11). Label chips sit on a solid surface
  token with text at 4.5:1 or more.
- **High contrast mode**: two colours plus line styles and tags only. **Reduced motion**: no animated trail or pulsing;
  trail dots are static and numbered; phase changes are instant.
- Validate the HUD palette with the dataviz palette validator and a CVD simulation, against light, dark and
  high-contrast screenshots of the control zoo (2.3), before freezing it. Every HUD screenshot gets the independent
  review of rule 22.

---

## 4. Recommended additional features

**Must** = v1, **Next** = the release after v1, **Later**. One line of rationale each. The items marked † are in the
proposal already; the note says what to change.

### Must for v1

| Feature | Rationale |
|---|---|
| Pre-action re-check and post-action verification (1.1) | Removes the main cause of wrong clicks; this is what a human does |
| Result taxonomy with `error` apart from `failed` (1.1) | Flaky infrastructure must not read as product bugs |
| Calibrated confidence plus margin rule (1.3) | Thresholds only mean something when the scores do |
| Scale normalisation and tiled detection (1.1) | Small icons on big screens are otherwise invisible |
| Target preflight and environment conformance (1.4) | Repeatable runs start from a checked environment |
| **Lab management**: targets, credentials by reference, VM lifecycle, **snapshot pools** with linked clones and bounded parallelism | The POC and "super app" both need it; it is missing as a section |
| **Interrupt handlers**: declarative "when screen X appears, do Y" in plans and packs (update toasts, session expired, lock screen) | A deterministic answer to popups before AI recovery; like a human who knows the usual popups |
| Flaky-step detection with a retry policy: retries only for `error` and for waits, counted and reported, never silent; quarantine with an owner and an expiry | Keeps CI green for the right reasons |
| HUD, Explain panel, step debugger, Lab monitor (section 3) | The user's question (b); also the fastest way to build trust in vision |
| Own run viewer and triage (video with HUD sidecar, steps, trace, failure groups, "this click was wrong") | Debrief stays optional (super app) |
| Reports: JUnit and a **self-contained HTML report** (one file, no external assets), Debrief bundle†; TRX and Allure can move to Next | HTML is what people open first; JUnit is what CI reads |
| Secrets: references only†, the on-screen secret rule and the bundle secret scanner (1.6) | Recordings are the leak path |
| Permissions model: scopes per key or client (`locate`, `observe`, `act`, `record`, `train`, `admin`), per-target allow-lists, Sighter-owned confirmation windows | `act` is dangerous and now also MCP-reachable |
| Loopback hardening (Host, Origin, header key, named pipe) (1.6) | Closes the browser-to-loopback hole |
| Recorder to plan through the live view† (record a remote target, steps with queries, not coordinates) | Authoring for remote targets; the "Record a flow" of 7.8 extended to targets |
| Human-like actions: scroll-to-find, drag and drop with thresholds, seeded humanised pointer and typing, launch-like-a-human sub-flows (1.11) | Reaches "like a human" without hooks |
| Golden set, per-slice gates, model card with coverage (2.8) | Promotion becomes a check, not a judgement call |
| A simple scheduler (named schedules, run sets, "stop all") | Super app; Pitcrew stays the CI path |

### Next

| Feature | Rationale |
|---|---|
| Variation farm with label transfer and the lab UIA helper (2.3) | The scalable answer to question (a); needs the snapshot pools first |
| Shadow mode for candidate models (2.9) | Catches regressions on real runs before promotion |
| Drift dashboard and retrain triggers (2.10) | Tells the user when to retrain instead of guessing |
| Self-healing proposals with review† (anchor or template updates as diffs to the plan, accepted in the studio) | Keep as the proposal says: explicit, never silent |
| Visual regression match levels and regions† | Valuable but large; `exact` and `layout` in v1 are enough |
| Side-channel assertions without an agent: clipboard over RDP, files through hypervisor guest copy or RDP drive redirection, process and network state from the hypervisor (the VM's virtual NIC) | Lets tests check what a human would check elsewhere, without hooks in the app |
| Test data management: data tables†, `{{unique}}` values, seeding through hypervisor guest exec, data reset through snapshots | Data-driven tests need clean, reproducible data |
| CI integration through Pitcrew: runner labels for matrix axes†, VM pool capacity, sharding a run set across pools | Unattended scale |
| Requirements traceability with Pacenotes† (`.sighterresult`) | Already planned; keep it file-based |
| Performance timing checks† (time-to-screen p50/p95 with baselines and alerts) | Cheap by-product of frame timestamps |
| Pseudo-locale localisation checks† (truncation, untranslated text) | Moved up from Later: the variation farm already runs the languages |
| Linux and macOS targets (xrdp, VNC with ARD auth) | Wider target coverage once the Windows path is solid |
| Starter models: the control zoo and OS dialogs model, trained on own data only | Day-one detection for new apps (no third-party data needed) |
| Extracting tables and lists (`extract_table`, `items()`) | Reading data like a human reads a grid |

### Later

| Feature | Rationale |
|---|---|
| IP-KVM target† | The purest "like a human" path: BIOS and bare metal |
| Sound cues over RDP audio redirection | "An error sound played" as an assertion |
| Accessibility audit from pixels† | Advisory findings for apps that have no tree to audit |
| TestRail/Xray push† | Opt-in egress; JUnit covers most teams first |
| Plugin points for `Target`, `LabelSource` and OCR engines as out-of-process Rust plugins (Paddock ADR 0001/0017 model) | Only when a third party needs one (suite lesson: no plugin systems without a reason) |
| Learned path selection and test-impact selection from the state graph | Run only the tests touching changed screens |
| IME composition, virtual desktops, mobile targets | Long-tail human actions and new platforms |
| Team model packs on a shared folder† | Sharing within a team, still local |

**Candidates to cut from v1 to pay for the musts** (the total is already 15 to 19 months): TRX and Allure (JUnit and
HTML first), the four hybrid adapters reduced to `Sighter.FlaUI` plus a documented recipe, path generation (keep graph
coverage), full visual-regression match levels, and the C++ client (generated later from the same OpenAPI).

---

## 5. Decisions for the user

1. **Re-phase v1 around the controller model.** v1 targets are the VM console (VNC), RDP and Local (studio), with the
   controller on **Windows only**. macOS and Linux controllers come after real signing. VM snapshot pools are v1.
   *Recommendation: yes.* It matches the POC and "like a human", and avoids the macOS ad-hoc TCC re-prompt problem.
2. **Ship a "Lab" release before the training studio.** Targets, OCR, template, plans, HUD, debugger, reports and
   interrupt handlers are useful without any training; the studio and training follow as 1.0. *Recommendation: yes*
   (value within months, not 15 to 19, and real run data to train on later).
3. **Allow a lab-only guest helper** (UIA labels, settings recipes through hypervisor guest exec) inside lab VMs for
   training data, never at run time and never on a system under test. *Recommendation: yes.* It is the cheapest source
   of variation and labels, and it keeps run time pure vision.
4. **The RDP library is IronRDP, after a two-week spike**, with FreeRDP only as an out-of-process fallback; the VNC
   client is Sighter's own (no GPL libvnc). *Recommendation: yes.*
5. **One stop hotkey and a HUD-evidence rule.** Ctrl+Alt+Shift+F12 everywhere. The HUD is a sidecar track by default,
   burnt-in only on export, and never used for training. The on-target overlay is for the Local target only and only
   when it can be excluded from capture. *Recommendation: yes.*
6. **The promotion rule:** a model that serves unattended suites is promoted only after it passes the per-slice gates
   and a shadow-mode review. *Recommendation: yes* (with an explicit, recorded override).

## Corrections to make in the proposal (revision 5)

- Section 10's panic key → Ctrl+Alt+Shift+F12 (1.5).
- Sections 7.15, 11, 13 and 16: FreeRDP → IronRDP primary (1.2).
- Section 1 "a built-in optional module, not a plugin process" and section 10 "plugin-free module guarded by
  `guard::catch`" are leftovers from when Sighter was a Debrief module: rewrite them for a standalone app.
- Section 9: add remote targets as a third kind of network traffic beside downloads and the training host (input,
  frames and credentials go to the user's own lab machines), so the README's "sends nothing off the machine" stays
  true as written: "nothing to any third party".
- Section 4: the hardened runtime → follow Paddock rule 23 (1.10).
- Section 11: workbench areas instead of pages (1.12); add `sighter-hypervisor` and `sighter-trace`.
- Section 7.15: the VM console and snapshot pools move from "to confirm" to v1 (1.2).
- POC plan: implement Hextile; bind the VNC server to loopback; write a per-step annotated screenshot and a
  `trace.ndjson`, the first version of the trace stream of 3.1, so the user's first try already has the debug view.


## Decisions (user, 2026-10-24)

All six recommendations in §5 accepted: v1 targets VM console, RDP and Local with a Windows-only controller and VM snapshot pools; a Lab release before the training studio; a lab-only guest helper for training labels, never at test time; IronRDP after a spike (FreeRDP out-of-process fallback) and Sighter's own VNC client; one stop hotkey Ctrl+Alt+Shift+F12, the HUD as a separate video track, the on-screen overlay only where it can be hidden from capture; models promoted for unattended suites only after per-slice gates and shadow mode.

**Superseded, 2026-10-30 (user):** the "Lab release before the training studio" cut and the lab-only guest helper (UIA
labels) no longer stand. Sighter ships one release with the detector, and it is pixels only
([ADR 0001](../adr/0001-object-detector.md) decision 9). See [stage-2.md](../plans/stage-2.md).

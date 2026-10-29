# Sighter — engineering rules for coding agents and contributors

Sighter finds UI elements by sight: a recorder, a labelling and training studio, and a local locator server that tests
query. The stack follows the Paddock family: a Rust core, a Tauri 2 shell and an Angular UI. **No product code exists
yet**; the design is `docs/proposal.md` (open questions pending). Read it and `docs/suite-lessons.md` before building.

## Paddock rules (binding)

The family's binding rules live once, in the monorepo's root [AGENTS.md](../../AGENTS.md) and
[docs/rules.md](../../docs/rules.md) (ADR 0014 §10, rule 8: no per-app copies). Read them before this file; where a
rule here disagrees, they win. This file holds only what is specific to Sighter.

## Design principles

Apply these deliberately. They are not decoration:

- **Single responsibility, open/closed, Liskov, interface segregation.** One reason to change per module; extend by
  adding (a new label source, resolver layer or detector backend is a new implementation behind a small trait); every
  implementation honours the trait's documented contract, with the same error semantics and no surprising panics.
- **Dependency inversion / ports and adapters.** The core depends on no adapter. Tauri commands depend on traits, never
  on a concrete capture API, model runtime or store. Angular components never call Tauri `invoke` directly: they use the
  backend injected by DI token (real or mock), and state lives in SignalStores.
- **Pure core, thin edges.** Geometry, box matching, scoring, resolver ordering, dataset splitting and formatting are
  pure functions with time and randomness passed in. Capture, files, network and the OS stay thin.
- **Parse, don't validate; make illegal states unrepresentable.** Convert untrusted input (images, IPC JSON, API
  requests, project files) into typed values once, at the edge. Prefer enums and newtypes over flags and `Option`
  combinations that can disagree (e.g. a box's review state is an enum).
- **Command–query separation.** Locate, list and export have no side effects; record, label, train, promote and delete
  mutate.
- Also: KISS, YAGNI, DRY with the rule of three, composition over inheritance, Law of Demeter, least astonishment, fail
  fast with a message that tells the user what to fix, and leave touched code better than you found it.

## Sighter-specific rules

- **Vision first.** Locating works from pixels alone. Accessibility trees (UIA, AX, AT-SPI, DOM) are an optional
  `LabelSource` that speeds up labelling; nothing at run time may depend on them.
- **Licences of models.** No AGPL, GPL or non-commercial models or weights: no Ultralytics YOLO (v5/v8/v11/v12), no
  YOLOv9/v10, no YOLO-NAS weights, no OmniParser `icon_detect`. Prefer Apache-2.0/MIT detectors (RT-DETR, D-FINE, DEIM,
  YOLOX, RF-DETR after verifying weights). Record every model's licence (including base weights) in the model card and
  verify it against the official repo before adopting.
- **Never click blindly.** Anything that acts on the UI acts on a located, scored element. Unreviewed elements block
  unattended runs; low confidence returns "needs labelling" rather than a guess.
- **Recordings may contain secrets.** Redaction is applied before anything is written, password fields pause capture,
  typed key text is not stored by default, run folders have retention caps, and recordings are never logged, uploaded or
  committed.
- **Tests never touch TCC-protected folders or real user data** (Desktop, Documents, Downloads, Photos, and similar),
  and never capture the developer's real screen content into fixtures. Use generated or synthetic fixtures; derive data
  folder, port and socket names from a test-specific folder, never the production names.
- **Pitwall is always optional.** Recording, labelling, training and locating work without it; Pitwall-assisted features
  (label suggestions, naming, grounding fallback, step descriptions) show a clear "Needs local AI" state instead of an
  error. Use the shared `paddock-pitwall` client, and send batch labelling at Pitwall's batch priority.

## Security and robustness

- Never trust the webview. Validate every IPC argument and every API request in Rust. The server binds loopback only,
  with bounded request sizes and image dimensions. Open only paths the user chose; canonicalize them.
- Tauri capabilities stay least-privilege: only the plugins and permissions the UI uses, no `fs`/`shell` wildcards, no
  remote content. Keep the CSP free of `unsafe-eval` and remote origins. Never bind untrusted text with `[innerHTML]`.
- Local only: no telemetry and no cloud. Crash reporting, if any, is opt-in. Logs contain no screenshots, OCR text or
  secrets by default.
- Parsers and decoders never panic on any input and enforce limits (image size, frame counts, project size). No
  `unwrap()`/`expect()` on user data or I/O outside tests. Library errors are typed; the app edge converts them to
  messages for the UI.
- Capture, inference and training run out of process or off the UI thread, with timeouts; a capture or model fault never
  takes down the studio or the server. Large results are paged.
- Contain failures, degrade with a clear next step, version and migrate persistent state, tolerate missing or corrupt
  files, and bound every unbounded input. Every robustness behaviour has a test that triggers the failure.
- Never await IPC before the first paint; put a plain splash in `index.html` (see `docs/suite-lessons.md`).

## Where logic lives

The Rust core is the single source of truth for behaviour. Angular is the presentation layer and must not re-implement
domain rules; if the UI needs a computation, add a core operation and a command. The mock backend is for browser
development and UI tests only; features are not done until they work against the real engine.

## Working in parallel (agents)

- Each task owns an explicit set of files; don't edit outside it. Shared integration points (`lib.rs` files, the
  workspace `Cargo.toml`, `api.types.ts` / DTOs) are changed by the integrating agent only.
- Don't run `cargo fmt --all` / `cargo clippy --fix` across the workspace while others are editing; use
  `rustfmt <files>` and a private `CARGO_TARGET_DIR`. Max 5 parallel agents, `CARGO_BUILD_JOBS=2`.
- Commit only complete, owned work. Windows- and macOS-only code is checked on its own CI runner.

## Testing and quality gates

- Every change ships with tests; every bug fix starts with a failing regression test. Targets: core ≥ 99% lines,
  Angular ≥ 80% overall and near 100% for stores, services and pure utilities. Coverage is necessary, not sufficient.
- Before committing, once the code exists, these must pass:

  ```sh
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo deny --workspace check
  node --test scripts/release/publish.test.mjs
  (cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
  ```

## Dependencies, commits and releases

- Prefer well-maintained, permissively licensed (MIT/Apache-2.0) dependencies; `deny.toml` lists the allowed licences.
  Commit `Cargo.lock` and `package-lock.json`; CI uses `npm ci`. Renovate updates dependencies.
- Use Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`). **Never add a
  Co-Authored-By trailer or any AI attribution** to commits or PRs.
- **Never run `npx prettier --write`**: the TypeScript is not prettier-formatted and it reformats whole files.
- Releases are `sighter-v*` tags following semver, published with `scripts/release/publish.mjs`. Never tag a `[skip ci]` commit;
  read the release lessons in `docs/suite-lessons.md`.
- Record significant architecture decisions as ADRs in `docs/adr/`.

## Style

- Match the surrounding code; Rust uses `rustfmt.toml` (max_width 140). Doc comments on public items.
- UI: standalone components, signals, OnPush, CSS custom properties for theme tokens (dark and light, plus high
  contrast), accessible markup. Use Angular CDK for behaviour and Angular Material first (rule 47); icons through
  `MatIconRegistry` with Sighter's own SVGs, no icon fonts or runtime icon URLs.
- Status and review colours always come with words; validate palettes for contrast and colour-vision deficiency.

## Working practices

- Record every benchmark you quote (rule 48): first as a results block in the app's `docs/performance.md` (shared code:
  `docs/benchmarks.md`), then link it from the ADR, plan, CHANGELOG, commit or PR that quotes the number.
- Spend CI only on what a change needs (rule 50): one run per branch head, none while review fixes are pending or for a
  branch already merged elsewhere, none for docs-only changes; branches run the affected plan, full gates once on main.
- Slow means two questions (rule 51): can more threads or workers run it (bounded by the kind of work), and can it be made
  cheaper (less work per item, no repeats, a cheaper algorithm or format, caching, batching, skipping)? Measure both;
  thread counts are Settings with an "Auto" default; a long or bulk operation's design has a "Parallelism and cost" section.
- Work a person waits for never runs at an OS's starvable background level (rule 52: macOS QoS `BACKGROUND` and
  `PRIO_DARWIN_BG`, Windows background modes and idle/lowest priorities, Linux `SCHED_IDLE` and idle I/O; for a service,
  launchd `ProcessType` `Background` (or `Adaptive` without XPC), `LowPriorityIO` and systemd `IOSchedulingClass=idle`):
  lowered work uses `QOS_CLASS_UTILITY`, nice 10 or `THREAD_PRIORITY_BELOW_NORMAL`; a starvable level only for work
  nobody waits on, with a `conformance: background-qos — <why>` comment (C54).
- Research official sources before choosing models or libraries; record what you found and when in the ADR.
- Navigate code with Serena's symbol tools when available. Delegate mechanical work to cheaper models; keep
  architecture and review on the main model.

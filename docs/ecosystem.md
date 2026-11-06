# The Paddock ecosystem

<picture>
  <img src="images/paddock-ecosystem.svg?v=14fc4133" alt="Paddock: eight apps. Pacenotes, Debrief and Sighter above Pitwall, the local AI hub; Recce, Dyno and Spotter below it; Pitcrew under Dyno. Dash-dot lines marked MCP show what is planned for Pitwall 1.1: it calls the MCP servers of Pacenotes, Debrief, Sighter, Dyno and Recce, the family's retrieval engine. All are built on the shared Paddock foundation: the app shell every app uses and the workbench for the IDE-style apps" width="100%">
</picture>

Eight desktop apps for people who keep systems running, built on one shared foundation. Each is useful alone and better with
the others. All run on Windows, macOS and Linux, are written in Rust with a Tauri 2 shell and an Angular UI, and are local
only: no cloud, no account, no telemetry.

The foundation has two layers (decided 2026-10-24, [ADR 0012](adr/0012-paddock-app-shell.md) and [ADR 0013](adr/0013-paddock-workbench.md)). The **App shell** is what every app gets: startup protection with crash recovery and
safe mode, Report a problem, background services and login items, notifications and service health, dialogs as real windows,
jobs and progress, Settings, About and updates, the Storage page, undo, and the theme (shell 0.3.0); Pitcrew, Pitwall,
Spotter and Dyno are the shell with pages. The **Workbench** sits on top of it for the IDE-style apps, Debrief, Pacenotes and
Sighter: docked panes with the layout planner, the activity bar, the Explorer, the command palette and the AI pane.

Images are generated ([`images/src/ecosystem.mjs`](images/src/ecosystem.mjs),
[`images/src/integrations.mjs`](images/src/integrations.mjs)). PNG exports for places that cannot render SVG:
hero [light](images/paddock-ecosystem-light.png?v=d79c0ef6) and [dark](images/paddock-ecosystem-dark.png?v=46a96a3f), integrations
[light](images/paddock-integrations-light.png?v=a9a7c6fa) and [dark](images/paddock-integrations-dark.png?v=38cb6fd5), hand-offs
[light](images/paddock-handoffs-light.png?v=e754dcb5) and [dark](images/paddock-handoffs-dark.png?v=ce5abde7). PNGs are
rendered with Playwright's pinned Chrome for Testing, never the installed Google Chrome ([`images/src/chrome.mjs`](images/src/chrome.mjs); set
`CHROME` to override).

Each app's marketing art (hero banner, feature showcase, how it works, 1280x640 social card) is generated from one data file per app
in [`images/src/apps/`](images/src/apps/) by [`images/src/app-poster.mjs`](images/src/app-poster.mjs), into the app's own
`docs/images/` (the apps live in `apps/<app>` in the monorepo: run it with `PADDOCK_APPS=apps`):
`node docs/images/src/app-poster.mjs <app|all> --png`. Screenshots come from the app's `docs/screenshots`; planned apps use a
concept mock. 

Image links carry a version, `images/x.svg?v=<first 8 hex of the file's SHA-256>`, so a regenerated image shows immediately through
Cloudflare and browser caches (the server caches for an hour and Cloudflare's cache key includes the query string). The generators
call [`images/src/version-links.mjs`](images/src/version-links.mjs) after writing: it rewrites `src`/`srcset` and Markdown links that
point at an existing file in an `images` directory in Paddock's README and this page, in the app README the poster wrote into, and in
the profile README when `PADDOCK_PROFILE` points at its checkout. It is idempotent; to apply it by hand:
`node docs/images/src/version-links.mjs <README.md ...>`.

## Status

| App | Status |
|---|---|
| Pitcrew | **v1.0.1 released** |
| Debrief | **v1.1.1 released, 1.2.0 in progress** |
| Pitwall | **v1.0.3 released** |
| Dyno | **v1.0.0 released, 1.1.0 in progress** |
| Spotter | In development, no release yet |
| Sighter | Planned |
| Pacenotes (understands code in any language) | Planned |
| Recce (searches the whole PC) | Planned (placeholder page, draft plan) |

## The apps

### Debrief
**Purpose.** An IDE for analysing software logs and diagnostic files.
**For.** Support and development teams who receive a zip of logs from several machines and must answer "what happened, in which process, and when?".
- One merged timeline per instance across processes and machines, with a fast filterable grid.
- Format detection from content, editable parsing profiles, charts and analysis views (anomalies, patterns, exceptions).
- Rust engine with a DuckDB store: flat memory on large folders. Safe on hostile input; saved projects open without the original logs.
- From 1.1 Pitwall is its AI engine once you pair the two explicitly; the built-in local engine stays as the fallback.

Status: **v1.1.1 released, 1.2.0 in progress**. Repo: <https://dev.decryptology.net/decryp7/paddock/src/branch/main/apps/debrief>

### Pitcrew
**Purpose.** Keeps CI runners alive, updated and clean.
**For.** Anyone running self-hosted Gitea or Forgejo runners.
- A watchdog restarts crashed or stalled runners; a running job is never killed.
- Safe updates between jobs, verified, with roll-back.
- Housekeeping, disk guard, pause windows and notifications, with a dashboard and tray.
- Runs the CI for every app in this family.

Status: **v1.0.1 released**. Repo: <https://dev.decryptology.net/decryp7/paddock/src/branch/main/apps/pitcrew>

### Pitwall
**Purpose.** The private, local AI hub every other app talks to.
**For.** Anyone who wants AI in their tools without sending data anywhere.
- OpenAI-, Anthropic- and Ollama-compatible APIs over models on your own machine, plus `/x/v1` extensions built for the family.
- llama.cpp is its only engine; model management with checksums, "will it fit?" guidance, presets and idle unloading.
- One key per app, job priorities (interactive first, batch when idle), prompt-prefix and opt-in response caching, usage and limits per app.
- Planned for 1.1: chat with text, audio, vision and MCP. As MCP client and host it calls the family's MCP servers (Dyno's, on
  Dyno's unreleased 1.1 line, not in 1.0.0; Recce's for documents, so Pitwall builds no index of its own; Pacenotes', Debrief's and Sighter's when they ship).
- Local only by design: no cloud models, no telemetry; the only traffic is downloads you asked for.

Status: **v1.0.3 released**. Repo: <https://dev.decryptology.net/decryp7/paddock/src/branch/main/apps/pitwall>

### Dyno
**Purpose.** Watches your PC: live and history, alarms, a disk map and benchmarks.
**For.** People who want to know what a machine is doing, now and over time.
- A background agent keeps history while the window is closed.
- Live and history views grouped by application, alarms, system facts, a disk map and benchmarks.
- No tuning or cleanup; power actions (keep awake, Wake-on-LAN, scheduled wakes, and sleep, hibernate or shut-down after idle or at a set time) run as the owner sets them up. An optional administrator helper, turned on in Settings, reads what needs administrator rights, sets Dyno's own scheduled wakes (macOS, Linux) and installs the pinned disk-health tool on request, with an audit log.
- Extensible with validated declarative packs, no code.
- An opt-in MCP server in the app (Settings > AI access; Dyno must be open) in 1.1.0: eight reading tools, two power readings and two opt-in actions (wake a saved computer, keep awake for up to 12 hours). Pitwall 1.1 is planned to call it.

Status: v1.0.0 released, 1.1.0 in progress (the administrator helper's readings, per-machine Windows installs, OS lifecycle checks). Repo: <https://dev.decryptology.net/decryp7/paddock/src/branch/main/apps/dyno>

### Spotter
**Purpose.** Watches for new releases anywhere and tells you the moment one appears.
**For.** People who track software across shares, listings, forges, feeds and web pages.
- Many source kinds, including JavaScript-rendered pages through a supervised Chromium.
- Version schemes, filters and soak time as declarative presets; plugins for the rest.
- Digests, quiet hours and OS notifications; optional "What's new" summaries from Pitwall.

Status: in development. Repo: <https://dev.decryptology.net/decryp7/paddock/src/branch/main/apps/spotter>

### Sighter
**Purpose.** Finds any UI element by sight: record, label and train vision models that UI tests can query.
**For.** Engineers who test or automate desktop apps, including legacy and custom-drawn ones with no accessibility tree.
- Works from pixels; accessibility is an optional label source.
- A layered resolver (cache, template, OCR, detector, Pitwall vision model, flag for manual) behind a loopback `/v1/locate` API.
- A recorder and a labelling and training studio; redacts recordings before storing them.
- Run bundles (video plus steps) that open next to the app's logs in Debrief.
- An MCP server (`observe`, `observe_image`, `locate`, `click`, `type`, `launch`, ...) that any family app can drive as an MCP host (Debrief reproduces a bug from a video, Pacenotes drives tests), and an agent mode with Pitwall as the local planner. Targets: this PC, or a remote PC under test over RDP from a controller PC (IP-KVM later). Pitwall stays a model server and optional.

Status: **planned**. Repo: <https://dev.decryptology.net/decryp7/paddock/src/branch/main/apps/sighter>

### Pacenotes
**Purpose.** Understands code in any language (v1 language packs: C, C++, C#, Java, TypeScript/JavaScript, Rust, Python, Go): a dependency graph of every link, code review with your own rules, local semantic search, and a read-only MCP server (also an MCP host) for people and AI tools.
**For.** Developers and reviewers who want local models, Debrief and editors to answer questions about real code instead of guessing.
- **Works with the family:** Pitwall (optional) for answers, explanations and embeddings; Spotter for "newer version available"; Debrief for stack trace to code.
- **Core:** the code index, a headless service, the MCP server, the UI shell and a CLI.
- **Plugins** (out of process, Rust only, each adding its own MCP tools, views declared and rendered by the core):
  language packs; **Understand** (symbols, references, repo map); **Review** (including Markdown review rules from
  `AGENTS.md` and `REVIEW.md`); **Diagrams** (draw.io, preserving manual edits); **Requirements** (Word, Excel, PowerPoint
  and ReqIF traceability); **Tests and coverage** (coverage import, gaps, unit-test drafts, Sighter test plans); and the
  **Debrief bridge** (stack trace to code, log checks).
- Phasing: core, Understand, Review, Debrief bridge and the core language packs first; then Diagrams; then Requirements;
  then Tests and coverage.

Status: **planned**. Repo: <https://dev.decryptology.net/decryp7/paddock/src/branch/main/apps/pacenotes>

### Recce
**Purpose.** One search box for the whole PC, "like Spotlight or X1 Search, but powerful": file names, document content, email and attachments, and SharePoint and OneDrive, by name or by meaning, with answers that cite their sources.
**For.** Anyone who knows they have it somewhere: a document, a mail, a passage, and wants it found as they type.
- **Works with the family:** Pitwall (optional) runs the local model for answers and re-ranking; Pacenotes shares the document readers; Debrief and Pacenotes share the embeddings crate.
- A light launcher: apps, files, a calculator and actions from one hotkey.
- Answers from a local model, each claim linked to its source.
- The family's retrieval engine: a read-only MCP server with citations (stage 4) that Pitwall calls instead of building an
  index of its own (decided 2026-11-04; plan on the branch `docs/pc-search-app-plan`, section 0).
- Stages: launcher and file name search; content and meaning search; email and SharePoint/OneDrive; answers with sources, MCP, plugins and workflows.

Status: **planned**, last in the order of 2026-11-04, after Dyno 1.1.0, Spotter 1.0, Pitwall 1.1, Pitcrew 1.1, Debrief 1.2, Pacenotes and Sighter. Page (a placeholder): [docs/planned/recce](planned/recce/README.md); the draft plan is on the branch `docs/pc-search-app-plan`.

## Integration architecture

<picture>
  <img src="images/paddock-integrations.svg?v=21b8a969" alt="Integration architecture: Pacenotes, Debrief, Sighter, Spotter, Dyno and Recce reach Pitwall's local HTTP API through the shared paddock-pitwall client with one key per app (Spotter, Dyno and Recce later); Pitwall runs llama.cpp as a child process and keeps keys in the OS credential store; its tool router, planned for Pitwall 1.1 as MCP client and host, calls the MCP servers of Pacenotes, Debrief, Sighter, Dyno (opt-in, on Dyno's unreleased 1.1 line) and Recce, which other MCP clients can use too" width="100%">
</picture>

<picture>
  <img src="images/paddock-handoffs.svg?v=63c55171" alt="Hand-offs between apps: Sighter run bundles, Pitcrew job logs, Dyno issues and Pacenotes log checks go to Debrief; Debrief sends Pacenotes stack traces and drives Sighter over MCP to reproduce a bug from a video; Pacenotes sends Sighter test plans and gets results back; Dyno sends Spotter its installed software and Pitcrew machine health; Spotter tells Pacenotes about newer versions and, later, triggers Sighter smoke tests" width="100%">
</picture>

The apps never depend on each other to do their own job. They meet in a few well-defined places:

| Contract | Between | How it works |
|---|---|---|
| **Pitwall local API** | every app to Pitwall | HTTP on loopback; OpenAI-, Anthropic- and Ollama-compatible, plus `/x/v1`. One key per app (held in the OS credential store), job priorities, prompt and response caching, usage and limits per app. |
| **MCP** (planned for Pitwall 1.1) | Pitwall to Pacenotes, Debrief, Sighter, Dyno and Recce; any MCP client | Pitwall becomes MCP client and host (chat with text, audio, vision and MCP): local models call the apps' MCP tools. Dyno's server is on its unreleased 1.1 line, not in 1.0.0 (opt-in, Settings > AI access; eight reading tools and two opt-in actions). Recce is the family's retrieval engine, so Pitwall builds no index of its own: a knowledge collection is a Recce scope read over MCP. The hero draws these as dash-dot lines labelled MCP. |
| **MCP servers** (decided 2026-10-23, [ADR 0007](adr/0007-mcp-servers-across-the-family.md)) | Pacenotes, Sighter, Debrief, then Spotter, Pitcrew, Dyno; Recce | Each serves read-only tools locally (Sighter acts, with allow-list, confirmation, budgets and a stop key); off by default, every call logged, results bounded; Pitwall serves none. Debrief first. |
| **Deep links** | Pitcrew and Dyno to Debrief; Dyno to Spotter and Pitcrew | "Job logs" and "Issues" open in Debrief; Dyno's installed software becomes Spotter's watch list; Dyno's machine health shows next to the runners in Pitcrew. The target app opens at the right place. |
| **Debrief bridge** | Debrief and Pacenotes, both ways | Stack trace to code, and log-line checks against the code. |
| **Run bundles** | Sighter to Debrief | A file with video, steps and results; Debrief opens it next to the app's logs. |
| **Test plans and results** | Pacenotes and Sighter | Pacenotes owns requirements and traceability and drafts test plans (`.sighterplan`); Sighter owns UI test execution and results, which flow back to Pacenotes. |
| **`paddock-testplan`** (planned) | Pacenotes and Sighter | The shared test-plan format behind `.sighterplan`. |
| **Newer version** | Spotter to Pacenotes | Spotter tells Pacenotes when a newer version of a dependency is available. |
| **Smoke tests** (later) | Spotter to Sighter | A new release found by Spotter triggers a Sighter smoke test. |
| **CI** | Pitcrew to every repo | Pitcrew keeps the runners that build and test all of these apps alive, updated and clean. |
| **Plugin platform** (planned) | Debrief, Pacenotes, later Dyno's tier-3 plugins | Debrief's plugin host, IPC and API (the ADR 0017 model) extracted into shared crates: out-of-process, Rust-only plugins. |
| **`paddock-pitwall`** | shared crate in each app | One client for discovery, pairing, keychain keys, retries, priorities, cache headers and capability checks, so no app re-implements Pitwall access. Built (extracted from Debrief 1.1's client); Debrief, Pacenotes and Sighter use it, Spotter and Dyno later. |

What each app uses Pitwall for (the dotted lines in the hero): Debrief, its AI engine once paired (from 1.1; the built-in engine is the fallback); Pacenotes, answers, explanations and
embeddings; Sighter, vision and the agent planner; Recce, answers and re-ranking; Spotter, "What's new" summaries; Dyno,
explanations of what it shows (later). Pitcrew does not need it. The other way round, the dash-dot lines labelled MCP are
planned for Pitwall 1.1: Pitwall calls the MCP servers of Pacenotes, Debrief, Sighter, Dyno and Recce.

## Data and privacy

- Your data stays on your machine. Logs, code, screenshots, prompts and answers are processed locally and never uploaded.
- No telemetry and no crash upload by default. The only network traffic is what you ask for: model and engine downloads in
  Pitwall, release checks you configure in Spotter, updates of the apps themselves (signed, from `latest.json`).
- Pitwall listens on loopback only and requires a key per app; keys live in the OS credential store.
- Apps treat input as hostile: parsers never panic, limits are enforced, UIs never render untrusted text as HTML.
- Dyno's optional administrator helper answers an allow-listed set of questions, so it can see every process and the privileged counters.

## Pitwall is optional

Pitwall is the AI hub, never a dependency. Every app's core works without it:

- Without Pitwall, AI features show a clear state, "Needs local AI, set up Pitwall", never an error or a dead button.
- Debrief keeps its built-in local AI as the fallback when Pitwall is not paired or not running.
- With Pitwall, interactive requests (an Explain click) run first; batch work (labelling, bulk review, extraction) runs at
  batch priority when the machine is idle.
- Install or remove Pitwall at any time; nothing else needs reconfiguring beyond pairing.

## What Paddock provides

- **`@decryp7/paddock-ui`** on the Gitea npm registry: tokens, Angular components, ECharts theme ([`ui/`](../ui/README.md)).
- **Design guide and tokens**: five accents and a colour-vision-safe categorical palette ([design guide](design-guide.md)).
- **Diagram kit**: draw.io shape library, templates and canonical export ([`diagrams/`](../diagrams/README.md)).
- **Icon set**: one icon per app, per platform, and the Paddock mark itself: pit garages on a slab striped in the seven app hues ([`icons/`](../icons/README.md)).
- **Shared Rust crates** on the Gitea cargo registry: the app shell, jobs, logs, Report a problem, services, secrets, processes, folders, knowledge packs, IPC and `paddock-pitwall`, among others; the rest of the plugin platform, `paddock-mcp` and `paddock-testplan` are planned ([`crates/`](../crates/README.md)).
- **Conformance checks**: `@decryp7/paddock-conformance`, the shared checks every app runs in CI ([`conformance/`](../conformance/)).
- **Release practices**: tag-driven CI release, signed updater artefacts, `latest.json`, the release script. Apple notarization and Windows code signing are deferred until the releases settle (2026-10-23); every app's release notes carry a "First launch" note (macOS Open Anyway, Windows More info > Run anyway).
- **Lessons**: what Debrief and Pitcrew taught about build, windows, services, native feel and tests ([lessons](lessons.md)).

## Glossary: why these names

Motorsport vocabulary, because the family is about keeping things running fast and safely.

| Name | In motorsport | Why it fits |
|---|---|---|
| **Paddock** | The area behind the pits where teams work | The shared place under every app |
| **Pitcrew** | The mechanics who service the car mid-race | Keeps CI runners running and serviced |
| **Pitwall** | The gantry where engineers watch data and radio the cars | The hub every app talks to |
| **Debrief** | The review after a session | Analysing what happened afterwards |
| **Dyno** | The rig that measures a car's power | Watches your PC |
| **Spotter** | Watches the track and calls what is coming | Calls out new releases |
| **Sighter** | A sighting lap before the race | Looks at the UI before tests drive it |
| **Pacenotes** | The co-driver's notes describing the road ahead | Describes the shape of a codebase to whoever is navigating it |

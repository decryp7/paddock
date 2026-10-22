# Pitwall design

All of Pitwall's design diagrams on one page, from the big picture down to individual flows. Each image is exported
from its editable draw.io source (linked under the image), which a small generator under
[`diagrams/src`](diagrams/src/README.md) writes; export with
[`scripts/export-diagrams.sh`](../scripts/export-diagrams.sh). The style
rules are in [diagrams/STYLE.md](diagrams/STYLE.md), the reasoning behind the design in the
[architecture decision records](adr/README.md), and the prose overview in [architecture.md](architecture.md).

Elements drawn with a dashed outline and a "planned" tag are not built yet; the [roadmap](roadmap.md) lists them.

Contents: [Overview](#overview) · [C4 model](#c4-model) · [Deployment](#deployment) · [Use cases](#use-cases) ·
[Domain model](#domain-model) · [Flows](#flows) · [States](#states)

## Overview

Apps and tools talk to one local server on `127.0.0.1:11435`. The server queues their requests, loads models into
engine processes when they fit, gives the memory back when it is idle, and answers in the shape each client expects.
Pitwall's own window, tray and `pitwall` command line manage it. The only outbound traffic is downloads the user
asked for. The picture follows the reader's light or dark setting.

![Pitwall at a glance](diagrams/pitwall-overview.svg)

## C4 model

### Level 1 — system context

Who uses Pitwall and which outside systems it touches. The suite apps, third-party tools and the user's own scripts
call the APIs; the user manages Pitwall in its window or with the command line. Pitwall reaches out only to Hugging
Face (models and revision checks) and GitHub (engine releases), and only when told to
([ADR 0001](adr/0001-local-only.md)). Sharing on the LAN is planned.

![C4 system context](diagrams/svg/c4-1-context.svg)

Source: [c4-1-context.drawio](diagrams/c4-1-context.drawio)

### Level 2 — containers

The runnable units and the processes they run in: the Tauri app with its Angular UI, tray and Rust shell, the
`pitwall` command line, the windowless `pitwall-server` that the OS service manager keeps running, and the
`llama-server` child processes (one per loaded model) that the server starts and owns. The server uses
`pitwall-core` for every decision and `pitwall-loopback` to reach engines. See
[ADR 0002](adr/0002-core-crate-ports-adapters.md) and [ADR 0004](adr/0004-engine-abstraction-and-process-model.md).

![C4 containers](diagrams/svg/c4-2-containers.svg)

Source: [c4-2-containers.drawio](diagrams/c4-2-containers.drawio)

### Level 3 — core components

`pitwall-core` has no I/O. It is shown in four parts so each stays readable.

Wire formats: the three API families, the neutral request, the llama protocol and the cache rules (keys, expiry, eviction, slot affinity).

![C4 core wire formats](diagrams/svg/c4-3-core-wire.svg)

Source: [c4-3-core-wire.drawio](diagrams/c4-3-core-wire.drawio)

Serving decisions: presets and aliases, capability selection, fit with slot cost, the scheduler, the memory policy and
pressure.

![C4 core serving decisions](diagrams/svg/c4-3-core-serving.svg)

Source: [c4-3-core-serving.drawio](diagrams/c4-3-core-serving.drawio)

Models and engines: Hugging Face parsing, update notices, GGUF, fit, packs, the pinned runtime catalog with
capabilities per build, hardware parsing, and the planned engine kinds.

![C4 core models and engines](diagrams/svg/c4-3-core-models.svg)

Source: [c4-3-core-models.drawio](diagrams/c4-3-core-models.drawio)

Security and state: privacy rules, keys and limits, usage history, settings, metrics and events, the management DTOs.

![C4 core security and state](diagrams/svg/c4-3-core-state.svg)

Source: [c4-3-core-state.drawio](diagrams/c4-3-core-state.drawio)

### Level 3 — server components

`pitwall-server` in two parts: the request path (gate, pipeline, auth, serving), then library, engines, egress and the
thin OS layer. The request path includes the cache layer (ADR 0019): lookup before the queue, a writer thread after a
completed answer, and `cache.sqlite` or memory behind it.

![C4 server request path](diagrams/svg/c4-3-server-requests.svg)

Source: [c4-3-server-requests.drawio](diagrams/c4-3-server-requests.drawio)

![C4 server engines and outbound](diagrams/svg/c4-3-server-edges.svg)

Source: [c4-3-server-edges.drawio](diagrams/c4-3-server-edges.drawio)

### Level 3 — UI components

Inside the Angular app: the feature screens, then their SignalStores and the `BACKEND` token that selects the Tauri
backend or the mock.

![C4 UI screens](diagrams/svg/c4-3-ui-features.svg)

Source: [c4-3-ui-features.drawio](diagrams/c4-3-ui-features.drawio)

![C4 UI state and backend](diagrams/svg/c4-3-ui-state.svg)

Source: [c4-3-ui-state.drawio](diagrams/c4-3-ui-state.drawio)

## Deployment

Per OS the server is a supervised background process so it keeps serving with no window open: a Windows service, a
macOS launch agent registered through `SMAppService`, a Linux systemd user service (`pitwall.service`, with
`Restart=always`). The app and tray run in the user's session; the server starts engine processes as its children.
The data folder (`PITWALL_HOME` overrides it; it also holds `cache.sqlite` and `slots/`) differs per OS. See the per-OS guides:
[Windows](windows.md), [macOS](macos.md), [Linux](linux.md).

![Deployment on Windows](diagrams/svg/uml-deployment-windows.svg)

Source: [uml-deployment-windows.drawio](diagrams/uml-deployment-windows.drawio)

![Deployment on macOS](diagrams/svg/uml-deployment-macos.svg)

Source: [uml-deployment-macos.drawio](diagrams/uml-deployment-macos.drawio)

![Deployment on Linux](diagrams/svg/uml-deployment-linux.svg)

Source: [uml-deployment-linux.drawio](diagrams/uml-deployment-linux.drawio)

## Use cases

What the people and programs around Pitwall do with it, in three parts.

![Use cases: models and engines](diagrams/svg/uml-use-cases-library.svg)

Source: [uml-use-cases-library.drawio](diagrams/uml-use-cases-library.drawio)

![Use cases: running Pitwall](diagrams/svg/uml-use-cases-operate.svg)

Source: [uml-use-cases-operate.drawio](diagrams/uml-use-cases-operate.drawio)

![Use cases: apps and tools](diagrams/svg/uml-use-cases-clients.svg)

Source: [uml-use-cases-clients.drawio](diagrams/uml-use-cases-clients.drawio)

## Domain model

The main types in three parts: serving (aliases, hot models, scheduler, policy), the library and engines (the `Engine`
trait, update notices, build pins), and apps, keys, limits and usage history.

![Domain classes: serving](diagrams/svg/uml-class-serving.svg)

Source: [uml-class-serving.drawio](diagrams/uml-class-serving.drawio)

![Domain classes: library and engines](diagrams/svg/uml-class-library.svg)

Source: [uml-class-library.drawio](diagrams/uml-class-library.drawio)

![Domain classes: apps, keys and usage](diagrams/svg/uml-class-access.svg)

Source: [uml-class-access.drawio](diagrams/uml-class-access.drawio)

## Flows

### Chat request, including a cold start

In two parts. First the gate (Host, Origin, key), parsing, authorisation, model resolution through aliases and presets,
and the queue; then loading the model when it is not hot, generation over loopback, the answer in the caller's format
with provenance headers, and the usage record.

![Chat request: admit and queue](diagrams/svg/uml-seq-chat-admit.svg)

Source: [uml-seq-chat-admit.drawio](diagrams/uml-seq-chat-admit.drawio)

![Chat request: load and answer](diagrams/svg/uml-seq-chat-serve.svg)

Source: [uml-seq-chat-serve.drawio](diagrams/uml-seq-chat-serve.drawio)

### Response cache and slot affinity

After the gate, a deterministic request from an opted-in app or preset is looked up before it is queued; a hit needs no model
and no queue. A miss runs on the slot that served the conversation before (`id_slot`) and the completed answer is stored off
the request path. See [ADR 0019](adr/0019-caching.md).

![Chat request: response cache and slot affinity](diagrams/svg/uml-seq-chat-cache.svg)

Source: [uml-seq-chat-cache.drawio](diagrams/uml-seq-chat-cache.drawio)

### Pairing an app

An app asks, the user compares the six-digit code and approves with permissions, the app polls and receives its
key exactly once ([ADR 0009](adr/0009-app-integration-contract.md), [ADR 0007](adr/0007-security-model.md)).

![Pairing sequence](diagrams/svg/uml-seq-pairing.svg)

Source: [uml-seq-pairing.drawio](diagrams/uml-seq-pairing.drawio)

### Downloading a model

Search and file list from Hugging Face, the fit check, a resumable download with the SHA-256 published for the file,
the GGUF check and the move into the library.

![Model download sequence](diagrams/svg/uml-seq-model-download.svg)

Source: [uml-seq-model-download.drawio](diagrams/uml-seq-model-download.drawio)

### Model update notices

A scheduled check, only when offline mode allows it, compares installed models with Hugging Face and raises a notice.
It never downloads by itself.

![Model update check sequence](diagrams/svg/uml-seq-update-check.svg)

Source: [uml-seq-update-check.drawio](diagrams/uml-seq-update-check.drawio)

### Installing an engine

Download and verify each asset, unpack defensively into a staging folder, self-test, swap in or discard.

![Engine install sequence](diagrams/svg/uml-seq-engine-install.svg)

Source: [uml-seq-engine-install.drawio](diagrams/uml-seq-engine-install.drawio)

### Pinning and rolling back an engine

Pin a build for every model or one, with features gated by what the build supports, and select the previous build.

![Engine pin sequence](diagrams/svg/uml-seq-engine-pin.svg)

Source: [uml-seq-engine-pin.drawio](diagrams/uml-seq-engine-pin.drawio)

### Idle release

The tick, the memory policy's decisions, why a busy model is never touched, and the reload on the next request.
See [ADR 0006](adr/0006-memory-policy.md).

![Idle release sequence](diagrams/svg/uml-seq-idle-release.svg)

Source: [uml-seq-idle-release.drawio](diagrams/uml-seq-idle-release.drawio)

### The command line

`pitwall` reads the discovery file and the admin key, then uses the local management and standard APIs.

![CLI sequence](diagrams/svg/uml-seq-cli.svg)

Source: [uml-seq-cli.drawio](diagrams/uml-seq-cli.drawio)

## States

### Hot model

A model is loading, ready, busy or released; pinned models are exempt from idle release; an engine that exits is
forgotten and reloaded by the next request.

![Hot model states](diagrams/svg/uml-state-hot-model.svg)

Source: [uml-state-hot-model.drawio](diagrams/uml-state-hot-model.drawio)

### Cache entry

Stored, possibly used, then expired and swept, evicted over the size cap, or dropped when its model, engine build or preset
changes. An expired entry is never served, even before the sweep removes it.

![Cache entry states](diagrams/svg/uml-state-cache-entry.svg)

Source: [uml-state-cache-entry.drawio](diagrams/uml-state-cache-entry.drawio)

### Download

Queued, downloading, verifying, then done, failed or cancelled.

![Download states](diagrams/svg/uml-state-download.svg)

Source: [uml-state-download.drawio](diagrams/uml-state-download.drawio)

### Pairing request

Waiting, then approved (the key is delivered once), denied or expired after five minutes.

![Pairing states](diagrams/svg/uml-state-pairing.svg)

Source: [uml-state-pairing.drawio](diagrams/uml-state-pairing.drawio)

## Screenshots

Every screen and recently added feature has a light and a dark shot in [screenshots](screenshots/), 1440x900 logical at 2x
(taller when the page scrolls; the tray is its real 360x520). They are captured from the browser build with the mock
backend by `npm run screenshots` in `app/` (spec: `app/e2e/screenshots/capture.shot.ts`; `SHOT_ONLY=engines,tray` limits it).
Re-capture after every layout change and commit them with the change.

| Feature | Screenshot |
| --- | --- |
| Memory, loaded models, queue, usage by app, model-update notice, history | `dashboard` |
| Library, update marks | `models-library` |
| Model detail: slots, idle override, engine build pin | `models-detail` |
| Parallel slots and fit table | `models-slots` |
| Recommendations, fit per quantisation | `models-discover` |
| Presets | `models-presets` |
| Downloads | `models-downloads` |
| Engines: pin, features, roll back | `engines` |
| Apps, pairing, permissions | `apps` |
| App usage chart and limits | `apps-detail` |
| Request log | `requests` |
| Request detail with preset alias | `requests-detail` |
| Chat | `playground` |
| Speed measurements | `benchmarks` |
| Settings incl. Model updates | `settings` |
| Tray popover | `tray` |

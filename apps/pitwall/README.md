<div align="center">

# Pitwall

**The private, local AI server your apps talk to.**

Pitwall runs language models on your own computer and gives every app one place to use them, through the standard
OpenAI-, Anthropic- and Ollama-compatible APIs plus a few extras built for the app suite (Debrief, Pitcrew, Spotter,
Dyno). Like the pit wall in a race, it is where every car radios in.

</div>

![Pitwall: One private AI for every app](docs/images/pitwall-hero.svg?v=cbab1055)


> **Local only.** Nothing you ask and nothing the model answers ever leaves your computer. No cloud models, no
> telemetry, no crash upload. The only network traffic is downloading engines and models you asked for, and offline
> mode turns even that off. This is the first architecture decision and is enforced in code and tests:
> [ADR 0001](docs/adr/0001-local-only.md).

**Status: in development.** The server works end to end on macOS (Apple Silicon): a Metal llama.cpp build was
installed through the app flow with checksum and self-test, a Hugging Face model downloaded with its SHA-256 verified,
and OpenAI, Anthropic and Ollama requests (streaming and not, plus JSON-schema output) were answered at about 213
tokens per second with a 0.5B model on an M4 Pro. The Tauri app exists, but no installer has been published and the Windows and Linux real-model
runs have not been done; see the [roadmap](docs/roadmap.md) for exactly what is verified and what is planned.

## Screenshots

| Screen | Light | Dark |
| --- | --- | --- |
| Dashboard: memory, loaded models with idle countdown, throughput, queue, usage by app, model-update notice, history | ![dashboard, light](docs/screenshots/dashboard-light.png) | ![dashboard, dark](docs/screenshots/dashboard-dark.png) |
| Models, Library: disk, status, update marks | ![models-library, light](docs/screenshots/models-library-light.png) | ![models-library, dark](docs/screenshots/models-library-dark.png) |
| Models, a model's detail: slots, idle override, engine pin | ![models-detail, light](docs/screenshots/models-detail-light.png) | ![models-detail, dark](docs/screenshots/models-detail-dark.png) |
| Models, parallel slots: what each slot adds and the fit table | ![models-slots, light](docs/screenshots/models-slots-light.png) | ![models-slots, dark](docs/screenshots/models-slots-dark.png) |
| Models, Discover: recommendations and "will it fit?" per quantisation | ![models-discover, light](docs/screenshots/models-discover-light.png) | ![models-discover, dark](docs/screenshots/models-discover-dark.png) |
| Models, Presets: names apps send and what they resolve to | ![models-presets, light](docs/screenshots/models-presets-light.png) | ![models-presets, dark](docs/screenshots/models-presets-dark.png) |
| Models, Downloads: progress, resume | ![models-downloads, light](docs/screenshots/models-downloads-light.png) | ![models-downloads, dark](docs/screenshots/models-downloads-dark.png) |
| Engines: builds, pin, features, self-test, roll back | ![engines, light](docs/screenshots/engines-light.png) | ![engines, dark](docs/screenshots/engines-dark.png) |
| Apps and keys: pairing, apps, permissions | ![apps, light](docs/screenshots/apps-light.png) | ![apps, dark](docs/screenshots/apps-dark.png) |
| Apps, one app: usage over time, limits with progress | ![apps-detail, light](docs/screenshots/apps-detail-light.png) | ![apps-detail, dark](docs/screenshots/apps-detail-dark.png) |
| Requests: log with timing and the chosen model | ![requests, light](docs/screenshots/requests-light.png) | ![requests, dark](docs/screenshots/requests-dark.png) |
| Requests, one request: timing, routing, preset alias | ![requests-detail, light](docs/screenshots/requests-detail-light.png) | ![requests-detail, dark](docs/screenshots/requests-detail-dark.png) |
| Playground: chat with a local model | ![playground, light](docs/screenshots/playground-light.png) | ![playground, dark](docs/screenshots/playground-dark.png) |
| Benchmarks: speed on this machine | ![benchmarks, light](docs/screenshots/benchmarks-light.png) | ![benchmarks, dark](docs/screenshots/benchmarks-dark.png) |
| Settings, including Model updates | ![settings, light](docs/screenshots/settings-light.png) | ![settings, dark](docs/screenshots/settings-dark.png) |
| Tray popover (360x520) | ![Tray, light](docs/screenshots/tray-light.png) | ![Tray, dark](docs/screenshots/tray-dark.png) |

The screenshots come from the browser build with the mock backend (`npm run screenshots` in `app/`; 1440x900 at 2x, long pages taller); they show the
design, not measurements from a real machine.

## At a glance

![Pitwall at a glance: apps and tools call one local server on 127.0.0.1; the server queues requests, loads models into llama.cpp engine processes when they fit, releases them when idle, and answers in OpenAI, Anthropic or Ollama form; the app and tray manage it; only user-requested downloads leave the machine](docs/diagrams/pitwall-overview.svg)

A Rust core decides everything (what fits, what to release, what runs next, which model to use, which URL may be
fetched); a windowless Rust server carries the decisions out, keeps serving when the window is closed, and runs each
loaded model in its own engine process; a Tauri app with a tray and an Angular UI manages it. Details:
[architecture](docs/architecture.md) and every design diagram in [docs/design.md](docs/design.md).

## Why Pitwall

- **One model in memory for every app.** Debrief, Pitcrew, Spotter, Dyno and any OpenAI, Anthropic or Ollama client
  share it, instead of each loading its own.
- **Private by construction.** One audited outbound client, loopback-only listening, `Host` and `Origin` checks,
  per-app keys stored hashed, engines that run with a scrubbed environment and no network of their own.
- **Memory that looks after itself.** Models load on first use, are released when idle or when the machine needs the
  memory, and are never released while answering. A request that cannot fit gets a clear error, not swap.
- **No lock-in to one engine.** llama.cpp today; others behind the `Engine` trait or a declarative TOML pack.

## Features

![Pitwall features](docs/images/pitwall-features.svg?v=a1d3151b)

![How Pitwall works](docs/images/pitwall-how-it-works.svg?v=d8019830)

Every feature, with its status, is in [docs/features.md](docs/features.md).

- **Engines.** llama.cpp with pinned, checksummed builds chosen for your hardware (Metal, CUDA, Vulkan, CPU, ...), a
  self-test after every install and discard of a build that fails it, version pins (all models or one), one-click rollback to the
  last good build, and capability gating (vision, tools, embeddings, rerank, JSON schema) by what the active build supports.
  Planned: MLX, vLLM, whisper.cpp.
- **Model library.** Search and download GGUF models from Hugging Face, a "will this fit?" check before you download,
  resumable downloads verified against the published SHA-256, quantisation variants, delete and disk usage.
- **Standard APIs** on `127.0.0.1:11435`: OpenAI (`/v1/chat/completions`, `/v1/responses`, `/v1/embeddings`,
  `/v1/models`), Anthropic Messages (`/v1/messages`) and Ollama (`/api/chat`, `/api/generate`, `/api/tags`, `/api/ps`,
  `/api/embed`), with streaming, tool calls, vision and JSON-schema output.
- **Extras for apps** under `/x/v1`: pick a model by capability (`auto:vision,8k,fast`), priorities (interactive before
  background before batch) with cancel, pairing, task endpoints, token helpers, provenance headers on every answer.
- **Model presets and aliases.** Give an app a stable name such as `debrief-explain` and decide which model (or capability
  selection) and defaults serve it; it works as the `model` in the OpenAI, Anthropic and Ollama APIs.
- **Parallel slots per model.** Choose how many requests a model serves at once; the UI shows the memory each slot adds and how
  much context or concurrency fits.
- **Model update notices.** Off until you turn it on: Pitwall asks Hugging Face, for the repositories you downloaded from only, whether
  a file changed or a better quantisation fits, and tells you. It never downloads by itself.
- **Caching.** Prompt prefixes are reused across turns (slot affinity), exact repeats of deterministic requests can be answered
  from an opt-in response cache, and embeddings are cached; all stored on this computer only, with expiry, size caps and
  clear buttons ([ADR 0019](docs/adr/0019-caching.md), [user guide](docs/user-guide.md#caching)).
- **Memory management.** Presets (Low memory, Balanced, Performance), idle and deep-idle release, pressure release,
  pinned models, a countdown on the dashboard.
- **Security.** Per-app keys approved with a six-digit pairing code, per-app permissions and limits (with usage over time per app), an offline mode.
- **Packs.** Add engines and model lists with TOML files, no code ([user guide](docs/user-guide.md#packs-add-an-engine-or-a-model-list)).
- **The app.** Dashboard, models, engines, apps, requests, playground, benchmarks, settings, tray.

Not built yet: LAN sharing with TLS, knowledge collections (RAG), MCP and tool hosting, speech to
text. See the [roadmap](docs/roadmap.md).

## Install

There are no published installers yet: the release workflow builds them for `v*` tags and they are unsigned until
the signing keys exist ([releasing](docs/releasing.md)). Per-OS notes: [macOS](docs/macos.md),
[Linux](docs/linux.md), [Windows](docs/windows.md). Until then, build from source (the server alone, or the app with `cd app && npx tauri build`):

```sh
cargo build -p pitwall-server --release
./target/release/pitwall-server --help
```

Requirements: Rust 1.88 or newer. Engines and models are downloaded from inside Pitwall, never bundled.

## Quick start

With the server running (`cargo run -p pitwall-server`, or the app once it ships):

```sh
# 1. The server answers on 127.0.0.1 only
curl http://127.0.0.1:11435/x/v1/health

# 2. Install an engine and download a model: Pitwall > Engines and Pitwall > Models.
#    From the command line the management API does the same with the admin key in <data folder>/admin.key:
KEY=$(cat "$HOME/Library/Application Support/Pitwall/admin.key")        # macOS path; see docs/user-guide.md
curl -s -X POST http://127.0.0.1:11435/mgmt/v1/rpc/engines -H "Authorization: Bearer $KEY"

# 3. Create an API key in Pitwall > Apps (or pair an app), then use any client:
export OPENAI_BASE_URL=http://127.0.0.1:11435/v1
export OPENAI_API_KEY=pw_your_key_here
```

The step-by-step version, with what to expect at each point, is the [user guide](docs/user-guide.md).

The `pitwall` command-line tool does the same from a terminal or a script (no new server surface; it talks to the local
APIs only):

```sh
pitwall status                     # server, memory, loaded models, queue
pitwall pull Qwen/Qwen2.5-0.5B-Instruct-GGUF/qwen2.5-0.5b-instruct-q4_k_m.gguf
pitwall load qwen2.5-0.5b-instruct-q4_k_m
export PITWALL_API_KEY=$(pitwall keys create my-script --new 2>/dev/null)
pitwall chat qwen2.5-0.5b-instruct-q4_k_m "Say hello in five words."
```

See [the CLI section of the user guide](docs/user-guide.md#the-pitwall-command-line-tool) for every command and the exit codes.

## API examples

```sh
# OpenAI chat (add "stream": true for server-sent events)
curl http://127.0.0.1:11435/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" -H "Content-Type: application/json" \
  -d '{"model":"qwen2.5-0.5b-instruct-q4_k_m","messages":[{"role":"user","content":"Say hello in five words."}]}'

# Anthropic Messages (the key goes in x-api-key)
curl http://127.0.0.1:11435/v1/messages \
  -H "x-api-key: $OPENAI_API_KEY" -H "anthropic-version: 2023-06-01" -H "Content-Type: application/json" \
  -d '{"model":"auto:chat","max_tokens":64,"messages":[{"role":"user","content":"Say hello."}]}'

# Ollama chat
curl http://127.0.0.1:11435/api/chat -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d '{"model":"qwen2.5-0.5b-instruct-q4_k_m","messages":[{"role":"user","content":"Say hello."}],"stream":false}'
```

Every answer carries `X-Pitwall-Model`, `X-Pitwall-Engine` and `X-Pitwall-Chosen-By`, and `X-Pitwall-Cold-Start-Ms`
when the model had to be loaded first. More examples (streaming, JSON-schema output, priorities, pairing) are in the
[user guide](docs/user-guide.md). The model names you can use are the ids shown in the Models screen and by
`GET /v1/models`.

## For app developers

Apps find Pitwall through `discovery.json` in its data folder (or port 11435), pair once with a six-digit code the user
approves, then use the OpenAI-compatible API with a few headers for priority and provenance. Debrief keeps its own
built-in AI as the default and offline fallback. See
[ADR 0009](docs/adr/0009-app-integration-contract.md) and [ADR 0003](docs/adr/0003-api-surface-and-compatibility.md).

## Repository

| Path | What it is |
| --- | --- |
| `crates/pitwall-core` | Pure logic: API formats, scheduler, memory policy, selection, keys, metrics, settings, packs |
| `crates/pitwall-loopback` | HTTP client that can only talk to `127.0.0.1` (engines, the CLI) |
| `crates/pitwall-cli` | The `pitwall` command-line tool: status, models, load, unload, pull, keys, presets, chat, bench |
| `crates/pitwall-server` | The server: engines, library, serving, auth, HTTP APIs, management API |
| `app/` | Angular UI and the Tauri 2 shell (`app/src-tauri`) |
| `docs/` | [Architecture](docs/architecture.md), [design diagrams](docs/design.md), [features](docs/features.md), [user guide](docs/user-guide.md), [development](docs/development.md), [releasing](docs/releasing.md), [roadmap](docs/roadmap.md), [testing manual](docs/testing-manual.md), [ADRs](docs/adr/README.md) |

Engineering rules for contributors and coding agents are in [AGENTS.md](AGENTS.md); lessons from the other suite apps
are in [docs/suite-lessons.md](docs/suite-lessons.md).

## Documentation

- [Features](docs/features.md) and the [user guide](docs/user-guide.md)
- [macOS](docs/macos.md), [Linux](docs/linux.md) and [Windows](docs/windows.md) guides
- [Architecture](docs/architecture.md), [design diagrams](docs/design.md), [decisions](docs/adr/README.md)
- [Development](docs/development.md), [releasing](docs/releasing.md), [testing manual](docs/testing-manual.md)
- [Roadmap](docs/roadmap.md)

## Licence

MIT OR Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).

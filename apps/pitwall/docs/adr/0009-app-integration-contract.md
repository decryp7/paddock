# 9. How apps (Debrief first) find, pair with and use Pitwall

- Status: accepted (2026-10-22); the Debrief side is not built yet
- History: 2026-10-24 Debrief's side decided (setting, explicit pairing, no mixing); `contextPerSlot`, `serving` in the
  discovery file and `X-Pitwall-Job-Id` added to /x/v1 (additive)
- Related: ADR 0001 (local only); code: `pitwall_core::{discovery, keys, select, scheduler, mgmt}`, `API_VERSION`

## Context

Debrief (formerly IMT) already runs its own llama.cpp runtime: pinned, checksummed downloads, a self-test after
updates, `/props` capability detection, one model at a time that does text and vision. The user wants an option for
Debrief to use Pitwall instead, so one model in memory serves every app in the suite, while Debrief keeps working on
its own and offline. Pitcrew, Spotter and Dyno will use the same contract. It must stay stable while both sides
evolve.

## Decision

**Discovery.** A running Pitwall writes `discovery.json` into its app-data folder:
`{ format, version, apiVersion, pid, port, baseUrl, startedAt }`, with `baseUrl` always `http://127.0.0.1:<port>`.
An app reads it (bounded size, parsed strictly; a file pointing anywhere but this machine is rejected), checks that
`pid` is alive and `apiVersion` is one it speaks, then calls `GET /x/v1/health`. Without the file it may try the
well-known port **11435** on `127.0.0.1` (next to Ollama's 11434). No discovery over the network.

**Pairing.** An app without a key calls `POST /x/v1/pair` with its name. Pitwall shows a pairing request with a
**six-digit code**; the app shows the same code; the user approves in Pitwall's UI (Apps & keys). The app polls and
receives its key **once** (`pw_` + 32 random bytes; Pitwall stores only a SHA-256 hash), then keeps it in the OS
credential store (Windows Credential Manager, macOS Keychain). Each key has permissions: which models, limits, and
the priorities it may use. The user can revoke a key at any time; the app then falls back as below.

**Requests.** Apps use the OpenAI-compatible chat API (`/v1/chat/completions`): system and messages, tools, inline
images (vision through the model's projector), `response_format` with a JSON schema (grammar-enforced on
llama.cpp), streaming (SSE). The extensions apps may use:
- **Model by capability:** `model: "auto:vision,8k,fast"` (or `POST /x/v1/select` to ask first). Pitwall picks the
  best installed model that fits now and says which.
- **Priority:** header `X-Pitwall-Priority: interactive | background | batch`. Interactive (e.g. Debrief's chat) goes
  ahead of background work; batch runs when idle.
- **Cancel:** close the connection, or `DELETE /x/v1/jobs/{id}` for queued and batch jobs.
- **Provenance on every response:** `X-Pitwall-Model`, `X-Pitwall-Engine`, `X-Pitwall-Chosen-By`,
  `X-Pitwall-Cold-Start-Ms` (when the model had to be loaded first, e.g. after an idle release).
- Typed errors with a clear message: no model fits, model has no vision, queue full, key revoked, offline.

**Versioning.** `/x/v1` and `apiVersion: 1` change only additively. A breaking change is `/x/v2`, served alongside
v1 for at least one release. The standard APIs (`/v1/...`, `/api/...`) follow their upstream shapes.

**Debrief's side (decided 2026-10-24, Debrief ADR 0059).**
- Settings > AI > Engine: **Automatic** (default) | **Pitwall** | **Built-in**. Automatic uses Pitwall only when the
  user has already paired Debrief with it and it is reachable; otherwise the built-in llama.cpp.
- Debrief never pairs on its own. The user explicitly chooses "Use Pitwall" / "Connect to Pitwall", and only then does
  the six-digit-code pairing above start.
- One session, one engine: no silent per-request fallback or mixing. If Pitwall is lost (stopped, key revoked, no
  suitable model) the request fails with a clear message and the user can switch to the built-in engine.
- Debrief keeps its own leak guard and redaction before anything is sent; local only on both sides.

**Installed but not running.** `discovery.json` carries `serving` (bool). A serving Pitwall writes `"serving": true`;
on a clean shutdown it rewrites the file with `"serving": false` instead of deleting it. An app tells the states apart:
- *not installed*: no Pitwall data folder and no discovery file;
- *installed, not running*: the file exists and `serving` is false, or `pid` is not alive (crash or kill);
- *running*: `serving` is true, `pid` alive, and `GET /x/v1/health` answers.
Files without the field (older Pitwall) are read as `serving: true` and judged by `pid` as before.

**Cancel by id.** Chat, completions and messages answers carry `X-Pitwall-Job-Id`, the id of the queued job.
`DELETE /x/v1/jobs/{id}` cancels it while queued (and while running) without closing the connection; an app may cancel
only its own jobs (others answer 404), the admin may cancel any.

**Context per slot.** `GET /x/v1/models` entries and the `POST /x/v1/select` answer report `contextPerSlot` (the context
each slot gets when the model is loaded, ADR 0016) next to `contextMax`, so apps can size prompts.

## Consequences

- One model in memory can serve every app; memory release and the queue are Pitwall's job, not each app's.
- Apps depend only on a small, versioned HTTP contract on `127.0.0.1`, testable with a fake server.
- Two AI paths in Debrief (built-in and Pitwall) must stay behaviourally equivalent; Debrief's AI tests run against
  both (the Pitwall path through a fake).

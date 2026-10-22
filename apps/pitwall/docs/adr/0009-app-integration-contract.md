# 9. How apps (Debrief first) find, pair with and use Pitwall

- Status: accepted (2026-10-22); the Debrief side is not built yet
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

**Debrief's side (to build when Pitwall is complete).**
- Settings ▸ AI ▸ "Use Pitwall when it's running"; off until the user turns it on.
- Debrief's built-in llama.cpp stays the **default and the offline fallback**: if Pitwall isn't running, isn't paired,
  or refuses (revoked key, no suitable model), Debrief uses its own runtime and says so once, without failing the
  request.
- Debrief keeps its own leak guard and redaction before anything is sent; local only on both sides.

## Consequences

- One model in memory can serve every app; memory release and the queue are Pitwall's job, not each app's.
- Apps depend only on a small, versioned HTTP contract on `127.0.0.1`, testable with a fake server.
- Two AI paths in Debrief (built-in and Pitwall) must stay behaviourally equivalent; Debrief's AI tests run against
  both (the Pitwall path through a fake).

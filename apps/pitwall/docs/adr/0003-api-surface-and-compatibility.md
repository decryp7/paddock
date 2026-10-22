# 3. API surface and compatibility policy

- Status: accepted (2026-10-22)
- Related: ADR 0001, ADR 0002, ADR 0007, ADR 0009; code: [`pitwall_core::api`](../../crates/pitwall-core/src/api/mod.rs),
  [`pitwall-server/src/http.rs`](../../crates/pitwall-server/src/http.rs), [`pitwall_core::API_VERSION`](../../crates/pitwall-core/src/lib.rs)

## Context

Apps should not need a Pitwall SDK. Debrief, Pitcrew, Spotter, Dyno and any third-party tool already speak OpenAI,
Anthropic or Ollama. Pitwall must look like those servers closely enough for their official SDKs, and add the few things
the suite needs (capability selection, priorities, provenance, pairing) without breaking that.

## Decision

**Three standard families plus one extension, all on `127.0.0.1:11435`** (Ollama's port is 11434, so both can run):

| Family | Routes implemented |
|---|---|
| OpenAI | `GET /v1/models`, `GET /v1/models/{id}`, `POST /v1/chat/completions`, `POST /v1/responses`, `POST /v1/embeddings` |
| Anthropic | `POST /v1/messages`, `POST /v1/messages/count_tokens` |
| Ollama | `POST /api/chat`, `/api/generate`, `/api/show`, `/api/embed`, `/api/embeddings`; `GET /api/tags`, `/api/ps`, `/api/version` |
| Extended (`/x/v1`) | `GET /x/v1/health`, `POST /x/v1/pair`, `GET /x/v1/pair/{id}`, `POST /x/v1/select`, `GET /x/v1/models`, `GET /x/v1/jobs`, `DELETE /x/v1/jobs/{id}` |

`POST /v1/audio/transcriptions` exists but answers `501 not_available` until a whisper.cpp engine exists. The
management API for Pitwall's own window (`/mgmt/v1/rpc/{method}`, `/mgmt/v1/live`, `/mgmt/v1/chat`) is not part of the
client contract: it needs the admin key (ADR 0007) and may change with the app.

**Parse once, answer in the family's own shape.** Each family parses its request into the neutral
`pitwall_core::chat::ChatRequest` at the edge and formats responses, stream chunks and errors itself. Streaming is SSE for
OpenAI and Anthropic and NDJSON (`application/x-ndjson`) for Ollama. Supported: tool calls, inline images (vision, when
the model has a projector), `response_format` JSON schema (grammar on llama.cpp), embeddings. Images must be inline; the
server never fetches a URL on a request's behalf (ADR 0001).

**Error shapes** follow the family: OpenAI `{"error":{"message","type","param","code"}}`, Anthropic
`{"type":"error","error":{"type","message"}}`, Ollama `{"error":"..."}`, extended `{"error":{"code","message"}}`. One internal
`ErrorKind` maps to status and Pitwall code:

| Kind | HTTP | `code` |
|---|---|---|
| InvalidRequest | 400 | `invalid_request` |
| Authentication | 401 | `unauthorized` |
| Permission | 403 | `forbidden` |
| NotFound | 404 | `not_found` |
| TooLarge | 413 | `request_too_large` |
| RateLimit | 429 | `rate_limited` |
| Overloaded | 503 | `overloaded` |
| InsufficientMemory | 503 | `insufficient_memory` |
| NotAvailable | 501 | `not_available` |
| Timeout | 504 | `timeout` |
| Engine | 500 | `engine_error` |

429 and overloaded answers carry `Retry-After: 5`. Messages say what to fix.

**Model names.** `model` is a library model id. `auto:<needs>` (for example `auto:vision,tools,8k,fast`) asks Pitwall to
choose (`select::Requirements::parse_auto`; unknown words are refused so a typo never silently picks another model).
`POST /x/v1/select` answers which model would be chosen without running it.

**Request headers.** `X-Pitwall-Priority: interactive | background | batch` (default interactive, capped by the app's
permissions, ADR 0005). Credentials: `Authorization: Bearer <key>` or `x-api-key` (ADR 0007).

**Provenance** on every successful chat response: `X-Pitwall-Model`, `X-Pitwall-Engine` (`llama.cpp`, or the id of the engine
pack that served the model), `X-Pitwall-Chosen-By` (`explicit | capabilities | default`) and, when the model had to be loaded first,
`X-Pitwall-Cold-Start-Ms`.

**Versioning.** `/x/v1` and `apiVersion: 1` (`API_VERSION`) change only additively; a breaking change is `/x/v2` served
next to v1 for at least one release. The standard families follow their upstream shapes; contract tests in
`pitwall_core::api` and `crates/pitwall-server/tests` pin the request and response shapes of the official SDKs. Fields
Pitwall does not support are ignored or refused with a clear `invalid_request`, never silently misinterpreted.

**Limits.** Request bodies are capped (`server.maxRequestBytes`, default 32 MiB), requests time out (`server.requestTimeoutSecs`,
default 600 s) and run under the queue caps (ADR 0005).

## Consequences

- Any OpenAI, Anthropic or Ollama client works by changing the base URL; suite apps add headers when they want more.
- Compatibility is a moving target: the contract tests are the record of what was checked, and unsupported upstream
  fields are a known gap rather than a promise.
- Three families mean three translators to keep correct; the neutral request keeps the serving core free of them.

## Not built yet

- Task endpoints (`/x/summarize`, `/x/classify`, `/x/extract`, `/x/translate`; `pitwall_core::tasks` holds the versioned
  prompts only), token-budget helpers beyond `count_tokens`, knowledge collections (RAG), a response cache
  (designed in ADR 0019, not built yet), MCP tool hosting, batch jobs with resume.
- `POST /v1/audio/transcriptions` (needs the whisper.cpp engine).
- Provenance reports the engine as `llama.cpp` even for pack engines; this should name the real engine.
- LAN access (ADR 0007): the server answers only on loopback.

# 1. Local only: no prompt, response or metric ever leaves the machine

- Status: accepted (2026-10-22)
- Related: AGENTS.md ("Privacy: local only"); code: `pitwall_core::privacy`, `pitwall-loopback`, `clippy.toml`

## Context

Pitwall is the AI server every app in the suite (Debrief, Pitcrew, Spotter, Dyno) and any OpenAI-, Anthropic- or
Ollama-compatible tool talks to. Those apps send it log lines, stack traces, screenshots and file contents. The user
chose Pitwall to be **local only because of privacy**: what they ask and what the model answers must stay on their
computer. This is the product's first promise, so it is recorded before any other decision and is not a setting.

## Decision

- **No cloud engines.** Pitwall runs models on this machine only (llama.cpp first; MLX, vLLM, whisper.cpp and
  embedding/reranker engines later). There is no Anthropic, OpenAI, Gemini or any other remote backend, no "fallback
  to the cloud", and no plugin kind that adds one.
- **No telemetry, no crash upload, no update pings that carry data.** Logs stay in the user's data folder and never
  contain prompts, responses or file contents by default; the request inspector shows prompts only on demand, from
  local memory.
- **The only outbound traffic is downloads the user asked for**: engine releases (GitHub) and models (Hugging Face),
  over HTTPS, from the hosts listed in `pitwall_core::privacy::DOWNLOAD_HOSTS`. Every redirect hop is checked again
  against that list. The UI shows each download (host, size, progress). **Offline mode** turns even that off and is
  honoured by every code path that could reach the network.
- **The request path has no HTTP client.** Requests to the API never cause outbound traffic: images must be inline,
  tools run locally, and nothing is fetched on a request's behalf. This is enforced, not just intended:
  - outbound HTTP lives in one `egress` module of the server; `clippy.toml` disallows HTTP clients and raw process
    spawning everywhere else;
  - engines are reached through `pitwall-loopback`, whose port type can only mean `127.0.0.1`;
  - a server test (`request_handlers_cannot_reach_the_network`) fails if any request handler can open an outbound
    connection.
- **Listening is local by default.** The API binds `127.0.0.1` (port 11435). `Host` and `Origin` headers are checked
  so a web page cannot use Pitwall through DNS rebinding or cross-site requests. Sharing on the user's own network is
  an explicit opt-in (off by default, TLS with a self-signed certificate whose fingerprint clients pin, per-app keys);
  it never reaches beyond that network.
- **Plugins get no network access by default** (out of process, sandboxed; see the plugin ADR). A plugin that needs
  the network must say so in its manifest, and the user approves it at install; request/response filter plugins never
  get it.

## Consequences

- Users can point any tool at Pitwall without wondering where their data goes; the promise is checkable in code and
  tests, and stated in the UI ("Local only — nothing leaves this computer").
- Model quality is bounded by what runs on the user's hardware; the model library's fit check and capability-based
  selection exist to make the best of that, not to escape it.
- Features that would need a remote service (web search, cloud sync, shared team models) are out of scope for
  Pitwall. Apps that do research (Debrief's internal browser) do it themselves, in their own UI, with the user in
  control.
- Changing this rule needs a new ADR that supersedes this one, and the user's explicit decision.

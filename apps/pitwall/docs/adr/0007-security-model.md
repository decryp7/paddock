# 7. Security model: keys, pairing, loopback only

- Status: accepted (2026-10-22); LAN exposure not built
- Related: ADR 0001 (local only), ADR 0003, ADR 0009; code: [`pitwall_core::{keys, limits, privacy}`](../../crates/pitwall-core/src/keys.rs),
  [`auth.rs`](../../crates/pitwall-server/src/auth.rs), [`http.rs`](../../crates/pitwall-server/src/http.rs) (`gate`),
  [`process.rs`](../../crates/pitwall-server/src/process.rs), [`egress.rs`](../../crates/pitwall-server/src/egress.rs)

## Context

Any process on the machine, and any web page in a browser, can send requests to a localhost port. Pitwall holds the
user's models and sees their prompts. It must authenticate apps, limit what each may do, resist DNS rebinding and
cross-site requests, and keep its own management surface away from apps.

## Decision

**Loopback only.** The server binds `127.0.0.1` (default port 11435, `server.port`); there is no other listener. A start
failure (port busy, for example another Ollama) says what to change. Engines also listen on loopback only, behind a random key (ADR 0004).

**Host and Origin checks** (`gate`, using `privacy::host_allowed` and `origin_allowed_without_key`): a `Host` that is not a
loopback name (`localhost`, `*.localhost`, `127.*`, `::1`) is refused (403), which blocks DNS rebinding; an `Origin` that is
not a loopback page or Pitwall's own window (`tauri://localhost`) is refused, `null` included. A request with an `Origin`
header is never treated as keyless.

**Keys.** A key is `pw_` plus 32 random bytes (base64url). Pitwall stores only its SHA-256 (`apps.json`: versioned, atomic
write with backup, at most 1 MiB, at most 64 apps), shows the first 10 characters, compares in constant time, and shows
the secret once. Credentials come from `Authorization: Bearer` or `x-api-key` (at most 256 printable ASCII characters).
Apps keep their key in the OS credential store; Pitwall's UI never shows it again. Keys can be added and revoked per app.

**No key, no service**, except `allowKeylessLocal` (off by default), which admits Origin-less local requests as the `local`
app for Ollama-style tools that cannot send a key.

**Pairing** (ADR 0009). `POST /x/v1/pair {name}` (loopback, no key) creates a request with a six-digit code, valid 5 minutes,
at most 8 waiting at once (name 1 to 60 characters, no control characters). The app shows the code; the user approves in
Pitwall with the permissions to grant (or denies). The app polls `GET /x/v1/pair/{id}` and receives its key exactly once;
later polls say it was delivered. Expired or unknown requests read as denied. Pairing needs a person at the Pitwall window.

**Permissions and limits** (`AppPermissions`, checked before queueing): allowed models (all by default), most urgent
priority, tools, extended APIs (default on), requests per minute (sliding window), tokens per UTC day, and concurrent
requests (ADR 0005). Refusals are 403 (permission) or 429 (rate or budget) with a message saying what to change.

**Admin key.** Pitwall's own window uses a separate key: a random `pw_` secret created on first start in `admin.key` in the
data folder (mode 0600 on Unix; the per-user folder on Windows) and read by the local app. Only the admin principal may use
`/mgmt/v1`; app keys get 403 there. Apps never see it.

**Outbound and processes.** Only `egress.rs` has an HTTP client (HTTPS only, no proxy, no credentials in URLs, every redirect
re-checked, max 6) and only to the allow-listed hosts: `huggingface.co`, `hf.co`, `cdn-lfs.huggingface.co`, `cdn-lfs.hf.co`,
`cdn-lfs-us-1.hf.co`, `cdn-lfs-eu-1.hf.co`, `cas-bridge.xethub.hf.co`, `transfer.xethub.hf.co`, `github.com`,
`objects.githubusercontent.com`, `release-assets.githubusercontent.com`, plus subdomains (whole labels) of `cdn.hf.co` and
`xethub.hf.co`. Child processes get a scrubbed environment (ADR 0004). The request
records hold metadata only (the last 500, in memory). Generated text is recorded only when `privacy.keepRequestContent` is on
(off by default, at most 64 KiB per request, in memory); the `request_content` management call does not return content yet.

## Consequences

- A malicious web page cannot use Pitwall; a local process without a paired key cannot either (unless the user enabled keyless mode).
- A local process running as the same user can read `admin.key` or the data folder; Pitwall does not defend against the
  user's own account being compromised.
- Pairing relies on the user comparing a code in the Pitwall window; a local attacker can still ask to pair but cannot approve.

## Not built yet

- LAN exposure: settings (`network.lanEnabled`, `lanPort` 11443) and the DTO (`LanStatus`) exist, but the server does not
  listen on any non-loopback address, and there is no TLS, self-signed certificate or pinning. `gate` passes an empty list of LAN host names.
- Per-key expiry, key rotation on a schedule, and audit logging of denied requests beyond the events feed.
- MCP and tool hosting (the `tools` permission only gates tool calls in requests).
- Rate limiting of unauthenticated pairing attempts beyond the pending cap.

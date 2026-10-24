# 4. `paddock-pitwall`: the shared Pitwall client, and "Pitwall is optional" as a standard state

- Status: accepted; built (shell 0.3.0) as extracted from Debrief 1.1's client and checked against Pitwall's router. As built: blocking only (no async facade), no `mcp` module (Pitwall has no `/x/v1/tool-providers` yet), `Link` is the stateful side (probe, pairing, outcomes), the key lives in `paddock-secrets` (not a Pitwall-specific `KeyStore`), and a single `Client` carries retries; see `crates/README.md`
- Related: Pitwall ADR 0009 (integration contract), 0003 (API surface), 0005 (queue), 0019 (caching); family rule "Pitwall is always optional"

## Context

Debrief switches to Pitwall next (its own llama.cpp runtime stays the offline fallback), Pacenotes (LLM review, draft tests,
semantic links), Sighter (VLM fallback), Spotter and Dyno will follow. All need the same: find Pitwall, pair, keep a key,
call chat/embeddings/vision/extras, respect priorities, and degrade cleanly. Pitwall already has `pitwall-loopback` (its own
client for the CLI) and the contract in ADR 0009; this crate is the **app-side** client and reuses its wire types where
Pitwall publishes them (see open questions).

## Decision

One crate `paddock-pitwall`, with a feature split: the transport is loopback HTTP only (`127.0.0.1`, enforced; any other host is
a typed error), blocking core plus async facade, no TLS, no cloud.

```
crates/paddock-pitwall/src/
  lib.rs  discovery.rs  pairing.rs  keys.rs  client.rs  request.rs  stream.rs  caps.rs
  retry.rs  status.rs  optional.rs  mcp.rs  error.rs  testing.rs (fake server, behind feature "testing")
```

### Discovery

`discovery::locate() -> Result<Located, Unreachable>`: read `discovery.json` from Pitwall's app-data folder (per-OS path
function, bounded read 16 KiB, strict parse: `{format, version, apiVersion, pid, port, baseUrl, startedAt}`; `baseUrl` must be
`http://127.0.0.1:<port>`), check `pid` alive and `apiVersion` supported, then `GET /x/v1/health`. Without the file, try
`127.0.0.1:11435`. No network discovery. Cached for 5 s, invalidated on a connect error.

### Pairing and keys

```rust
pub struct AppIdentity { pub id: String /* "debrief" */, pub display_name: String }
pub trait KeyStore { fn get(&self, app: &str) -> Result<Option<SecretString>, KeyError>; fn put(..); fn delete(..); } // OS keychain
pub struct Pairing { pub id: String, pub code: String /* six digits, shown by the app */, pub expires_in: Duration }
pub enum PairPoll { Pending, Approved(Key), Denied, Expired }
impl Client { pub fn begin_pairing(&self, who: &AppIdentity) -> Result<Pairing, PitwallError>;
              pub fn poll_pairing(&self, p: &Pairing) -> Result<PairPoll, PitwallError>; }
```
`keys` has `KeychainStore` (Windows Credential Manager, macOS Keychain, Secret Service on Linux; service `paddock.pitwall`, account
= app id) and `MemoryStore` for tests. The key (`pw_...`) is returned once, stored immediately, never logged, never in settings
files, redacted by `Debug`. A 401 with "revoked" deletes the stored key and moves the client to `NeedsPairing`.

### Typed requests

```rust
pub struct ChatRequest { pub model: ModelSel, pub messages: Vec<Message>, pub tools: Vec<ToolDef>, pub json_schema: Option<Schema>,
                         pub max_tokens: Option<u32>, pub temperature: Option<f32>, pub options: CallOptions }
pub enum ModelSel { Alias(String), Exact(String), Auto(Caps /* vision, 8k, fast */) }   // "auto:vision,8k,fast"
pub struct CallOptions { pub priority: Priority, pub cache: CachePolicy, pub timeout: Duration, pub cancel: CancelToken }
pub enum Priority { Interactive, Background, Batch }          // X-Pitwall-Priority; clamped by the app's permission, reported back
pub enum CachePolicy { Default, Bypass /* Cache-Control: no-cache */, NoStore }
impl Client {
    pub fn chat(&self, r: ChatRequest) -> Result<ChatReply, PitwallError>;
    pub fn chat_stream(&self, r: ChatRequest) -> Result<ChatStream, PitwallError>;     // SSE; Iterator<Item = Result<Delta>>; drop = cancel
    pub fn embed(&self, r: EmbedRequest) -> Result<EmbedReply, PitwallError>;          // batched; embeddings cache is on at Pitwall
    pub fn vision(&self, r: VisionRequest) -> Result<ChatReply, PitwallError>;         // sugar over chat with inline images; checks caps.vision first
    pub fn select(&self, caps: Caps) -> Result<Selection, PitwallError>;               // POST /x/v1/select
    pub fn models(&self) -> Result<Vec<ModelInfo>, PitwallError>;                      // GET /x/v1/models (capabilities + unavailable reasons)
    pub fn jobs(&self) -> Result<Vec<JobInfo>, ..>;  pub fn cancel_job(&self, id: &str) -> Result<(), ..>;
    pub fn task(&self, t: Task /* Summarize|Classify|Extract|Translate */, r: TaskRequest) -> ..; // /x/v1/tasks/*, when the server has them
    pub fn count_tokens(..); pub fn truncate(..);                                      // /x/v1/tokens/*
}
pub struct Provenance { pub model: String, pub engine: String, pub chosen_by: String, pub cold_start_ms: Option<u64>,
                        pub cache: Option<CacheStatus /* Hit|Miss|Bypass */>, pub alias: Option<String>, pub cached_tokens: Option<u64> }
```
Every reply carries `Provenance` (from `X-Pitwall-*`), so apps can show "answered by model X (cached)" and tests can assert it.

### Capability checks, retries, timeouts

- `Caps` come from `/x/v1/models` and `/x/v1/health`, cached 30 s. Methods check before sending: a vision request to a model
  without vision returns `PitwallError::Unsupported { cap, hint }` locally, with Pitwall's reason ("needs an engine build with ...").
- Retries (`retry.rs`, pure, table-tested): connect errors and `503 overloaded` honour `Retry-After`, exponential backoff with
  jitter (250 ms to 8 s, 4 attempts) for Interactive, 8 attempts for Batch; never retry 4xx except 429 with `Retry-After`; a streamed
  reply is never retried after the first delta. Cancel and per-call deadline always win. Defaults: connect 1 s, first byte 30 s
  (cold start allowance via `X-Pitwall-Cold-Start-Ms`), idle between deltas 60 s, whole call set by the caller.
- Errors: `enum PitwallError { NotRunning, NeedsPairing, PairingDenied, KeyRevoked, Offline, NoModelFits{needs: Caps}, Unsupported{..},
  RateLimited{retry_after}, Overloaded, Cancelled, Timeout, Protocol(String), Server{status, code, message} }`, each with `user_message()`
  and `next_step() -> NextStep` (`StartPitwall | Pair | InstallModel | Wait | None`).

### "Pitwall is optional" helpers (`status`, `optional`)

```rust
pub enum AiState {
    Disabled,                                   // user turned AI features off in this app
    NotInstalled,                               // no discovery.json, port closed: show "Needs local AI -> Set up Pitwall"
    NotRunning,                                 // installed but stopped: "Start Pitwall"
    NeedsPairing,                               // "Connect <App> to Pitwall" (shows the six-digit code flow)
    NoSuitableModel { needs: Caps },            // "Install a model with vision"
    Ready { caps: Caps, models: Vec<ModelInfo> },
}
pub struct StatusProbe;  // probe() -> AiState, never errors, never blocks the UI (timeout 1.5 s), emits changes via a watcher
pub struct NeedsAiView { pub feature: String, pub reason: AiState, pub actions: Vec<NextStep> } // serialises to the paddock-view NeedsAi node
```
`probe()` is a query with no side effects. Each app renders `AiState` with one shared Angular component in `paddock-ui`
(`<pd-needs-ai>`), whose text and actions come from `NeedsAiView`, so the state looks and reads the same everywhere; no app
shows an error or a dead button for a missing Pitwall. A helper `Ai::run_or_fallback(feature, || pitwall_path, || deterministic_path)`
documents the family rule in code. Batch labelling/review/extraction uses `Priority::Batch` and the `idle_only` helper that
waits until the app is idle. Debrief keeps its built-in runtime as the offline fallback behind the same `AiBackend` trait
(the trait lives in Debrief, not here).

### MCP tool-provider registration (`mcp`)

Pitwall plans to host MCP servers and tools (not built). Pacenotes serves MCP itself. The crate offers, behind feature `mcp`,
`register_tool_provider(&Client, ProviderSpec { name, endpoint: Stdio|LoopbackHttp, tools: Vec<ToolSummary>, key_ref }) ->
Registration` against `/x/v1/tool-providers` (**proposed endpoint, additive to `/x/v1`; to be agreed in a Pitwall ADR**), so Pitwall's
chat can call Pacenotes' read-only tools. Registration is idempotent, re-sent after a Pitwall restart, removed on app exit, and
omitted when Pitwall lacks the capability (`caps.tool_providers`). Until the endpoint exists the feature compiles to a typed
`Unsupported`, so apps need not branch.

## Consequences

- Debrief's Settings > AI "Use Pitwall" (ADR 0009) is built on this crate; its AI tests run against the built-in path and the
  crate's fake server (`testing::FakePitwall`, which also serves the contract-test fixtures Pitwall publishes).
- Contract drift is caught by running the crate's tests against Pitwall's real server in Pitwall's CI (consumer-driven).

## Open questions

Reuse `pitwall-core`/`pitwall-loopback` types vs a small `pitwall-wire` crate owned by Pitwall (preferred: Pitwall publishes wire
types, `paddock-pitwall` depends on them, so there is one definition); who owns `/x/v1/tool-providers`; sync vs async primary API.

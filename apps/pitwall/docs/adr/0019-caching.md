# 19. Caching: prompt-prefix reuse, a response cache and an embeddings cache

Status: accepted. The number is provisional; it is allocated again when this branch merges (see `docs/suite-lessons.md`).
Not built yet: this records the design the engine is being written to; the shipped state is in `docs/roadmap.md`.

## Context

Local models are slow to start an answer and expensive to repeat. Three different kinds of repetition occur and they need
different tools:

- Chat apps send the same long system prompt and history again with every turn. llama-server can skip the work for the shared
  start of a prompt (`cache_prompt`), but only if the request lands on the slot that holds that start. With several parallel
  slots (ADR 0016) a request lands on whichever slot is free, so the hit rate is luck.
- An app that asks the exact same deterministic question twice (temperature 0, same messages) gets the same answer, and
  computing it again costs a model load, a queue wait and seconds of generation.
- Embeddings are a pure function of the model and the text. Indexing and search send the same texts again and again.

ADR 0003 and 0005 listed a response cache as future work; `pitwall_core::cache` held only a policy that nothing used.
Pitwall is local only (ADR 0001): whatever is cached stays on this computer, and a cached answer is a copy of private content,
so the privacy rules need to be as explicit as the speed-up.

## Decision

Three layers, each with its own switch. All of them are keyed so that a changed model, engine or build can never produce a
stale hit.

### 1. Prompt-prefix reuse (always on, llama.cpp only)

- **Slot affinity.** `slots::affinity` (pure) chooses the llama-server slot for a request. The conversation key is the SHA-256
  of the app id, the leading system messages and the first non-system message, so every turn of one conversation by one app has
  the same key. The request is sent with `id_slot` set to the slot that served the key last, when that slot is free, so
  `cache_prompt` finds the shared prefix. With no free affine slot the slot idle longest is used (an empty slot first, so a
  warm one is not evicted needlessly). If every slot is busy no `id_slot` is sent and the engine chooses. Other engines get
  no `id_slot`.
- **`--cache-ram`** is sized at 1/16 of the machine's RAM clamped to 256 MiB..4 GiB, unless the user set a size in Settings.
  `--cache-reuse 256` stays. This replaces the two fixed sizes of ADR 0004.
- **Effect is shown.** llama-server's `timings.cache_n` becomes `cachedTokens` on the request details and `prefixTokens` in
  the usage statistics ("tokens not recomputed").
- **Keep warm (optional, per preset).** With the preset's keep-warm switch on, the slots' state is saved (`--slot-save-path`,
  the `slots/` folder in the data directory) when the model unloads and restored when it loads. The folder is bounded, and its
  files are dropped when the model, engine build or preset changes, or after `keepWarmDays` (30 by default) unused.

### 2. Response cache (exact repeats, opt-in)

- **What is cached.** Only deterministic requests: temperature 0 or a fixed seed, and no tools. Streaming and non-streaming
  requests alike; a hit on a streaming request is replayed in pieces. A hit is not queued and needs no model load. Only
  completed answers are stored (finish reason stop or length, no tool calls); errors, cancellations and tool calls never are.
- **Key.** SHA-256 of a fingerprint (model id, model file size and modification time, engine, engine build) followed by the
  canonical request (every generation parameter and the full messages; `stream` and priority excluded). The API key and the
  app are not in the key, so apps that opted in share answers.
- **Off by default.** Enabled per app (`permissions.cacheResponses`) and per preset (`cacheResponses`); either switch enables
  it for a request. Answers can reveal what one app asked another, which is why the default is off and the opt-in is
  explicit.
- **Header.** Every answer from a cache-eligible path carries `X-Pitwall-Cache: hit|miss|bypass`. A request with
  `Cache-Control: no-cache` or `no-store` bypasses the cache (nothing read, nothing written).
- **Expiry.** 7 days after storing by default; options 1 hour, 1 day, 7 days, 30 days, 90 days, never (the size cap
  still applies). Size cap 256 MiB by default.

### 3. Embeddings cache (on by default)

- One vector per input text, keyed like the response cache (fingerprint plus text). On by default, 512 MiB, expiry 90 days
  since last use (sliding: a hit renews it), the same options. A batch with some cached inputs computes only the missing
  ones and returns all vectors in the original order. Embeddings are not private to an app in the way answers are, but they
  are still derived from content, so they follow the same storage and clearing rules.

### Storage, expiry and cleaning (shared by 2 and 3)

- **Storage per cache:** Memory only or On disk (embeddings default to disk; the response cache defaults to disk once
  enabled). Disk is one SQLite file, `cache.sqlite` in the data directory (`rusqlite`, bundled; WAL; a versioned schema;
  owner-only permissions, 0600 on Unix). A corrupt file, or one from an older or newer schema, is renamed aside and recreated.
  If the file cannot be opened the cache falls back to memory and says why in the Settings page and the event log.
- **Rows** hold the key hash, the value, created and last-used times, size, hit count, model id and app id. The API key is
  never stored.
- **Writes happen off the request path** on a writer thread; a hit costs one read, a miss never waits for the disk.
- **Size caps** apply with least-recently-used eviction on top of expiry.
- **Sweep.** A background pass runs at startup, then hourly, and when the server is idle; each pass is bounded (5000 entries) so
  it never holds the store. **An expired entry is never served**, even before a sweep removes it: the read checks the expiry.
- **Invalidation.** A changed model file, engine or build produces different keys. Removing a model, changing or pinning the
  engine build (ADR 0018) and changing a preset drop the affected entries immediately.
- **Controls and privacy.** Settings > Caching: sizes, storage, expiry, "Oldest entry", "Expires after", "Next clean-up" and
  "Clear now", plus clearing per app, per model or everything. Cached answers and embeddings are stored on this computer only;
  answers only for opted-in apps and presets; disk storage can be turned off.
- **Statistics.** Usage counters `cacheHits`, `cacheMisses`, `prefixTokens` and `savedTokens`; the dashboard shows the hit rate
  and tokens saved; per-app hit counts.
- **Management API** (async Tauri commands): `cache_info` (state of each cache in words and numbers, the storage in use and
  why it fell back, the next clean-up) and `clear_cache` (scope: all, app, model, or one cache). Queries have no side effects.

## Consequences

- Slot affinity needs the server to remember which slot served which key (a small bounded map); a restart forgets it, which
  only costs one cold prefix.
- Slot save/restore files are model-sized state on disk; the bound and the 30-day rule keep them from growing.
- A deterministic request is not always deterministic across engine builds or GPUs; the fingerprint contains the engine build
  so a changed build never reuses an answer, and the user can bypass per request or clear at any time.
- Answers on disk are readable by the owner of the data directory; that is the same trust boundary as the request log.
  Turning disk storage off keeps them in memory only.
- `rusqlite` (bundled SQLite) is a new dependency of the server crate only; the core stays pure and holds the policy
  (keys, expiry, eviction decisions, affinity) as functions of their inputs.
- Reasoning/"thinking" tokens are part of a stored answer as the engine returned it; a hit is byte-identical to the original.

## Related

ADR 0016 (slots, which affinity builds on), ADR 0005 (the queue a hit skips), ADR 0018 (engine build in the fingerprint,
pin changes invalidate), ADR 0006 (memory: `--cache-ram` is counted with the engine's footprint).

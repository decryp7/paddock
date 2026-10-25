# 11. Extend through declarative packs, not code

- Status: accepted (2026-10-22). Engine packs, the generic adapter and the per-model engine choice are superseded by [ADR 0020](0020-llama-cpp-is-the-only-engine.md); model catalog packs stay as described.
- Related: ADR 0001 (local only), ADR 0010 (no plugin system); code: `pitwall_core::packs`, `pitwall_server::pack_engine`

## Context

ADR 0010 removed code plugins. Users still need to add engines Pitwall does not ship (llamafile, KoboldCpp,
`mlx_lm.server`, vLLM) and curated model lists, without Pitwall running third-party code in its own process or
growing a hard-coded special case per engine.

## Decision

Pitwall reads two kinds of **TOML packs**, parsed once at the edge into typed values (`Packs`):

- **Engine packs** (`*.engine.toml`) describe any local engine that serves an OpenAI-compatible API: the launch command
  and argument templates (`{host}`, `{port}`, `{model}`, `{context}`, `{alias}`, `{mmproj}`, `{threads}`,
  `{engine_dir}`, `{binary}`), the health endpoint, capabilities (chat, embeddings, vision, tools), memory hints, and
  a pinned download per platform with a SHA-256. One generic adapter (`PackEngine`) runs every pack. llama.cpp stays a
  first-class built-in adapter.
- **Model catalog packs** (`*.models.toml`) list curated models: Hugging Face repo, quantisation variants with sizes,
  context, capabilities and tags (`code`, `small`). They feed the library's "will this fit?" and the recommendations.

Rules that keep packs safe:

- A pack is data. It cannot add an outbound destination: download URLs go through `DownloadUrl`, so only the allow-listed
  hosts of ADR 0001 are possible, always over HTTPS, always checksummed.
- An engine pack must bind loopback: its arguments must use `{host}` (always `127.0.0.1`), `{port}` and `{model}`, and may
  not name `0.0.0.0`. Engines start with a scrubbed environment, no console window and below-normal priority, and die with
  Pitwall.
- Unknown keys are warnings; an invalid pack is skipped with a message that says what to fix; one bad pack never stops the
  others (it appears in the events feed and in `packs` on the management API).
- A pack with no downloads is an *external* engine: the user installed it, Pitwall only launches the command.
- Built-in packs live in `crates/pitwall-core/src/packs/`; user packs go in `<data folder>/packs/` (at most 64 files of 256 KB).
  Later packs replace earlier ones with the same id.
- A model chooses its engine in its preferences (`engine`, default llama.cpp).

## Consequences

- New engines and model lists need no release and no code; a wrong pack degrades into a message.
- Pack engines are only as capable as the OpenAI-compatible surface they expose (no grammar-constrained output unless
  the engine supports it; no `/props`).
- The llamafile pack is experimental: its download is pinned and checksummed, but it is not the default engine.
- Tests cover parsing, every validation error, and a pack driving a fake OpenAI-compatible server through a real child
  process (`tests/e2e.rs`).

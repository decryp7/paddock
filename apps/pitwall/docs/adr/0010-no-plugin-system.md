# 10. No plugin system

- Status: accepted (2026-10-22); supersedes the earlier plugin design (out-of-process Rust plugins over `pitwall-ipc`)
- Related: ADR 0001 (local only), ADR 0009 (app integration contract); suite decision to keep the design simple

## Context

Pitwall's first design had a plugin system: Rust plugins in their own sandboxed process, a plugin SDK
(`pitwall-plugin-api`), a private IPC protocol (`pitwall-ipc`) and a guarded host (`pitwall-plugin-host`), for engines,
endpoints, tools and request filters. The suite lessons (2026-10-22) say to add a plugin system only when third-party
code, untrusted parsing or privilege separation needs one. None of those applies to Pitwall: engines are binaries it
downloads and checksums, and the rest is first-party code. The system added a protocol, an SDK, manifests, quarantine
rules and a surface to test and secure for no user-visible gain.

## Decision

- There is no plugin system, no plugin SDK, no plugin management API and no plugin UI.
- Engines are adapters behind the `Engine` trait in `pitwall-core`. Each engine runs as a separate process managed by
  the server (start, health, restart, release), so a crash or hang is contained without running anything in-process.
- No third-party code runs inside the Pitwall process.

## Consequences

- `pitwall-ipc`, `pitwall-plugin-api`, `pitwall-plugin-host`, `plugins/` and `templates/plugin` are removed, as are
  `PluginKind`, `PluginState`, `PluginInfo`, the plugin engine kind and the plugin-quarantined event, and the Plugins
  page in the UI.
- A new engine is a new `Engine` implementation in the repository.
- Features once planned as plugins (request redaction, audit) become built-in modules later if wanted, with their own
  ADR.
- The "MCP tool" app permission stays: it governs Pitwall's own hosted tools, not third-party plugins.

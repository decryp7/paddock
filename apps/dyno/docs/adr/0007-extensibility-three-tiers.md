# 7. Extensibility in three tiers: built-in core, declarative TOML packs, out-of-process plugins for native code

- Status: accepted (2026-10-23); supersedes the earlier note "drop Dyno's plugin system"
- Related: ADR 0001, ADR 0005, `dyno-plugin-api`, `dyno-plugin-host`, docs/plugins.md (Debrief's ADR 0017 for the process model)

## Context

Dyno needs to grow without becoming a bag of special cases: developers want cleanup for Rust/Node/.NET/Python caches,
browsers want their own, game launchers, Docker, Xcode; tuning and process-taming knowledge changes monthly. Most of
that is data. Some of it is native code (vendor sensor SDKs, benchmarks). Code plugins are costly (ABI, crashes,
supply chain), so they must be the exception.

## Decision

1. **Tier 1, built-in core.** Monitoring, history, the dashboard, issues and statistics, unusual-process detection,
   why-slow, what-changed, sleep/boot analysis, network and energy per app, rules, watched folders, the tuning
   framework and backup *health* checks are Rust modules behind the core's traits. Per-OS extras (power throttling,
   performance counters, App Nap, governors) are `#[cfg]` modules of `dyno-platform`, not plugins.
2. **Tier 2, declarative TOML packs, no code.** Cleanup packs (developer caches, browsers, Docker, Xcode and Android
   SDK, game launchers), tuning packs (check, apply and undo data), process rule packs (taming rules, known helpers)
   and backup-tool detectors. A pack is parsed once at the edge into typed values (`deny_unknown_fields`, size and
   count limits, path templates expanded by the core), validated, and every path it names still passes the cleanup
   safety policy (ADR 0005); a pack cannot widen what Dyno may delete or change. Invalid packs are skipped with a
   message naming the file and line.
3. **Tier 3, real out-of-process Rust plugins**, through the existing `dyno-plugin-api` / `dyno-plugin-host`, only for
   native or risky code: vendor sensor SDKs, benchmarks, an optional full-backup plugin, third-party extensions
   later. Plugins depend on `dyno-plugin-api` alone, speak the one IPC, run guarded (timeouts, memory cap, crash →
   error attributed to the plugin, quarantine after repeats) and never take the agent down. The host is trimmed to
   what these need: sensors, cleanup *proposals* (the host checks every path), and rule actions.
4. **Rule of thumb for contributors:** data → pack; behaviour that every user gets → core; native code or untrusted
   third-party code → plugin. A feature needing code in tier 2 is a sign the core is missing an operation; add the
   operation to the core and keep the pack declarative.
5. **Safety backups of Dyno's own changes** (settings, registry values, plists snapshotted for Undo; an optional
   Windows restore point; export/import of Dyno's configuration) are built in, not extensible.

## Consequences

- Packs ship in the app and may be dropped into the user's `packs/` folder; a pack format version gates loading.
- The plugin host stays small and well tested; no feature depends on it being present.
- Pitwall and Spotter stay plugin-free and use the same pack pattern; Pitcrew has none.

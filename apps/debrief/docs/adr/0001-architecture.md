# 1. Desktop architecture: Rust engine, Tauri shell, Angular UI

- Status: Accepted (2026-10-30, confirmation review of the amendment list; first accepted 2026-10-17); amended by
  [0012](0012-jobs.md), [0014](0014-database-first-engine.md), [0017](0017-one-ipc-out-of-process-plugins.md) and
  [0042](0042-store-port-table.md)
- Date: 2026-10-17
- What changed since: DuckDB in the store worker is the only store (0014: the in-memory `Workspace` is no longer the
  app's store; 0042: the worker's protocol is one port table); long work runs as jobs with per-kind limits (0012)
  instead of plain commands on Tauri's blocking pool (short commands still use it); and plugins run out of process over
  one IPC (0017), the engine's compiled-in registry serving only its own built-ins.

## Context

Debrief grew from a small regex line filter into an "IDE for analysing logs and diagnostic
files". Users open folders of logs from many machines and processes (Serilog JSON/text,
log4net, CSV and more later: registry exports, msinfo), on Windows and macOS, often offline,
and need a per-instance timeline, a fast filterable grid, metric charts and configurable
parsing. Files are untrusted and can be large.

## Decision

- **Engine in Rust** (`crates/debrief-core`): importers, parsing profiles, query, timeline and
  chart aggregation. Parallel parsing and scanning with rayon; records held in memory.
- **Ports and adapters.** The engine exposes two seams:
  - `Importer`: one implementation per file format, chosen by content sniffing and
    registered in `Importers` (plugins add formats without touching the engine).
  - `LogStore`: everything the shell needs. `Workspace` is the in-memory implementation; a
    DuckDB-backed store can replace it for data larger than RAM or persistent workspaces.
- **Tauri 2 desktop shell** (`app/src-tauri`): thin commands over `LogStore`, running work
  on the blocking pool. Chosen over egui (hand-drawn widgets for every view) and a local web
  server (no native packaging), because web UI makes timeline, grid, tree and chart views
  cheap to build and Tauri ships small native installers (WebView2 / WKWebView).
- **Angular 22 + NgRx SignalStore UI** (`app/src`): DI fits plugin views and backend
  abstraction; the UI reaches the backend only through an injected service, with a mock
  implementation for browser development and tests.
- **Configuration over code**: TOML parsing profiles describe files, formats, instance
  keys, extractors, markers and charts; extracted values become ordinary fields.
- **Charts** with Apache ECharts (Apache-2.0); the timeline is custom.
- **Wire contract** in `app/src/app/core/api.types.ts`, mirrored by `debrief_core::api`.

## Consequences

- Two toolchains (Cargo, npm) and three CI runner OSes are needed to build installers.
- The in-memory store bounds dataset size by RAM until a persistent store is added.
- WebDriver end-to-end tests of the real app are not available on macOS; browser-level
  end-to-end tests use the mock backend instead.

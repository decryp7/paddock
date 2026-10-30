# 3. Plugin dependencies, services, extension points and events

- Status: accepted; **scoped and partly superseded** by [ADR 0017](0017-one-ipc-out-of-process-plugins.md) and family
  [ADR 0001](../../../../docs/adr/0001-plugin-platform-crates.md) (see "Amendment: scope after the plugin platform" at
  the end)
- Date: 2026-10-17
- Applies to: the engine's compiled-in registry (`debrief-core::plugin`: `graph.rs`, `services.rs`, `crate::events`)
  only. Out-of-process plugins get no plugin-to-plugin dependencies or services in 1.2.

## Context

With most features moving into plugins (ADR 0002), plugins need to build on each other: the
AI assistant can use PerfMon counters and profiler snapshots; a profiler can accept symbol
resolvers from other plugins. Without explicit contracts, plugins would reach into each other's
internals, start in the wrong order, or fail when a provider is disabled or quarantined.

## Decision

1. **Manifest.** Each plugin declares `id`, semver `version`, `api_version`, `requires`
   (`Dep::required(id, req)` / `Dep::optional(id, req)`) and `provides` (service ids with
   versions). Existing plugins get a default manifest (no dependencies), so `API_VERSION` stays 1. *(Superseded: there
   is no plugin `API_VERSION`; out-of-process plugins are versioned by the platform protocol and per-namespace minors,
   family ADR 0001 "Versioning and compatibility".)*
2. **Resolution.** The host builds the dependency graph before activation: cycles are rejected,
   missing/incompatible/disabled/quarantined required dependencies leave the dependent
   *inactive with a stated reason*; optional ones only reduce features. Activation runs in
   topological order, deactivation in reverse. Quarantining a provider deactivates its
   dependents (cascade) and the health view shows the chain.
3. **Services.** Inter-plugin APIs are Rust traits in small, separately versioned API crates
   (`debrief-api-*`). Providers register `Arc<dyn Trait>` under a service id and version;
   consumers look them up by id and semver requirement and receive a **guarded proxy**
   (panics → errors, quarantine respected). Each API crate ships contract tests every provider
   must pass. Worker-process plugins provide the same traits through JSON-RPC adapters. *(Superseded: no `debrief-api-*`
   crates were made, and there is no JSON-RPC between plugins; ADR 0017 dropped plugin services and the wire is
   framed postcard. Services stay a compiled-in registry feature.)*
4. **Plugin-defined extension points.** A plugin may declare an extension point (id + trait);
   other plugins contribute implementations, collected by the host and guarded like core
   contributions.
5. **Event bus.** Typed events (`RecordsAppended`, `ProfileChanged`, `LiveAlertRaised`,
   `PluginStateChanged`, …) with per-subscriber bounded queues, delivered off the engine
   thread under the guard; a slow subscriber drops/coalesces, never blocks.
6. **Data over calls.** Plugins should cooperate primarily through workspace records and fields
   (queried via `LogStore`); services are for capabilities that are not records.
7. **UI mirror.** Angular features declare `provideDebriefPlugin({ id, requires })`, expose services
   via injection tokens from a shared API package, and are enabled only while their engine plugin
   is active. *(Superseded in the workbench: the UI side is the shell's typed `define*` contributions
   (`ui/projects/paddock-ui/shell/src/define.ts`, family ADR 0013), each gated by its `when` predicate on the engine
   plugin being active; `provideDebriefPlugin` stays until the M-packages of the 1.2 plan move Debrief onto them.)*

## Consequences

- Plugins compose without knowing each other's internals, and failure of one degrades its
  dependents predictably instead of crashing them.
- More moving parts in the host (graph resolution, proxies, bus), all covered by tests for
  missing/incompatible/cyclic dependencies, cascade on quarantine and crashing subscribers.
- API crates become public contracts and need semver discipline.

## Amendment: scope after the plugin platform (2026-10-31)

ADR 0017 moved plugins out of process and dropped "workspace hooks, events, exporters, services and plugin-declared
extension points, and dependencies between plugins" from them; family ADR 0001 then made the out-of-process host a
shared Paddock platform. This ADR therefore governs only what is still in the engine:

1. **Scope.** Sections 1 to 6 apply to the engine's compiled-in registry (`debrief-core/src/plugin/`: `graph.rs`
   resolves the dependency graph, `services.rs` holds `Service<T>`, `crate::events::EventBus` the bus). They do not
   apply to plugins hosted by `paddock-plugin-host`.
2. **Out-of-process plugins cooperate through data and app-brokered contracts.** In 1.2 they have no `requires` or
   `provides`, cannot call each other, and see no event bus. They cooperate through workspace records and fields
   (section 6, which stands) and through the `debrief.*` extension namespaces that Debrief brokers (family ADR 0003,
   mapping table). A plugin that needs another plugin's capability asks Debrief, which decides whether to route it.
3. **A future inter-plugin dependency is a family decision.** If two apps need plugin-to-plugin services, a family ADR
   adds `requires` / `provides` to the shared manifest (`paddock-plugin-package::Manifest`) and host-brokered calls
   (plugin to host to plugin, grant-checked and guarded on both legs), never direct connections between plugin
   processes. Until then the shared manifest has neither field.
4. **Superseded parts**, marked inline: the plugin `API_VERSION` (section 1), `debrief-api-*` crates and JSON-RPC
   adapters (section 3), and `provideDebriefPlugin` as the UI mirror (section 7, now the workbench's `define*`
   contributions).
5. **Sharing.** `graph.rs` and `EventBus` stay in `debrief-core` while Debrief is their only user. They move to Paddock
   when a second app needs them (ER-22, rule of three; rule 42 then moves them and adopts them in the same change).

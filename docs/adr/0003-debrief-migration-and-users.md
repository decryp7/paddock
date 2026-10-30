# 3. Moving Debrief onto the platform, and who uses what

- Status: proposed (2026-10-23; revised 2026-10-31 after an independent review and the user's decision of 2026-10-31: no
  binary legacy mode). The coordinator accepts it after a confirmation review.
- Related: ADRs 0001 (the platform), 0002 (views), 0014 (monorepo); Debrief ADRs 0003 (scoped by this move), 0017, 0027;
  plan [debrief-1-2.md](../plans/debrief-1-2.md) packages PH1 to PH4, 2.6, 0.5 (D1: move in 1.2)

## Context

Debrief has the mature implementation, 11 first-party plugin crates (`debrief-plugin-formats`, `-dotnet`, `-dumps`,
`-native`, `-otel`, `-profiling`, `-registry`, `-syslog`, `-tools`, `-web`, `-windows`) served by the `debrief-plugins` bundle,
and a published SDK (Debrief ADR 0027). The move must not break those plugins or Debrief's release. Under the "no backward
compatibility before v1" rule there is no binary promise. The user asked that existing plugin *sources* keep compiling
(the shim, phase 3), and decided on 2026-10-31 that already built protocol-1 binaries are refused, not served.

## Decision

**Phases (each ends with its tests green):**

1. **Done: the crates are in Paddock.** Paddock 0.3.0 (plan X51) built `paddock-ipc`, `-guard`, `-view`,
   `-plugin-package`, `-plugin-api`, `-plugin-host` and `-plugin-test`, and Debrief now lives in the same monorepo (ADR
   0014). Under rules 39 and 42 (monorepo form) Debrief adopts them **by path dependency in the same change** that deletes
   its copies; there is no publish-and-consume-by-tag step. PH1 does this for the domain-free half (`debrief-ipc`,
   `debrief-core::guard`, `debrief-plugin-package`) and moves the OS sandbox to `paddock-sandbox` (ADR 0001).
0. **Split along the line, done in PH2a, after phase 1.** The platform half of `debrief-plugin-api` (frames, launch,
   supervisor, sandbox, guard, package, manifest, serve) is deleted in favour of the Paddock crates. The domain half
   (`Record`, `Level`, `WireEvent`, importers, `ConfigTree`, markers) becomes `debrief-plugin-ext`, the `debrief.*`
   namespaces below. Proof: a `cargo tree` check in CI that `paddock-plugin-*` has no Debrief dependency.
2. **Platform protocol 2** (PH2a contract half, PH2b host half). Debrief's protocol-1 `PluginRequest` / `HostRequest` map to
   platform requests and `debrief.*` namespaces as in the table below; Debrief hosts its plugins through
   `paddock-plugin-host`.
3. **Source compatibility shim** (PH3; kept for two Debrief minor releases, removed in 1.4). `debrief-plugin-api` re-exports
   every public name it has in 1.1 (`Plugin`, `Contributions`, `serve`, `prelude`, ...), implemented on
   `paddock-plugin-api` + `debrief-plugin-ext`, so a plugin's source recompiles unchanged. **No binary legacy mode:** a
   binary built for protocol 1 is never started. It is refused with `Refusal::Legacy`, whose text says to rebuild it from
   source with the 1.2 SDK and reinstall it. The shared host has no hook for old protocols; the frozen `fixture_v1` in
   `paddock-plugin-test` keeps the handshake refusal tested.
4. **First-party plugins rebuilt** against protocol 2 in the same release (PH4).
5. **Remove the shim** after two minors (1.4). In the same change, the SDK docs (`apps/debrief/docs/plugin-sdk.md`,
   Debrief ADR 0027) move to Paddock `docs/plugin-sdk.md` and describe the platform SDK plus `debrief-plugin-ext`.

Order: 1 (done), then 0 and 2's contract half (PH2a), 2's host half (PH2b), 3 (PH3) and 4 (PH4); 5 in 1.4.

**Safe fallbacks:** phases 1 and 0 change no wire bytes. Phase 2 is the only wire change, and it ships together with
phase 4's rebuilt first-party plugins, so a release never pairs a protocol-2 host with protocol-1 first-party binaries.

### Protocol 1 to protocol 2 mapping

Protocol 1 is `apps/debrief/crates/debrief-plugin-api/src/protocol.rs` (`PROTOCOL` 1.1). Every namespace starts at minor 0.
"Stream" means `Remote::stream` (ADR 0001); "Bulk" and "Live" are the `Contract::role` of the request (ADR 0001,
parallelism and cost).

| Protocol 1 (`PluginRequest`, host to plugin) | Protocol 2 | Kind | Notes |
|---|---|---|---|
| `Manifest` | `PlatformRequest::Describe` + `debrief.describe.v1` `Contributions` | unary, Live | platform half (tools, settings, views) in `Described`; Debrief's half (importers, parsers, annotators, transforms, profile and document importers, marker kinds, sniff declarations) in the namespace |
| `ApplySettings { values }` | `PlatformRequest::ApplySettings { json }` | unary | |
| `InvokeTool { tool, op, args, data_dir }` | `PlatformRequest::Invoke { tool, op, args }` | unary | `data_dir` dropped: the host starts the process in the plugin's data folder (`supervisor.rs`, `current_dir`) |
| `StopTask { tool, task, data_dir }` | `PlatformRequest::Stop { task }` | unary | task ids are unique per plugin (`ToolTask::id`) |
| `PullOutput { stream }` | removed: the plugin pushes `HostCall::Output` pieces | | see `SaveOutput` |
| `Sniff { importer, file, head }` | `debrief.import.v1` `Sniff` | unary, Live | skipped when the manifest's declarative sniff decides (plan 2.6) |
| `Import { importer, file, content }` | `debrief.import.v1` `Import` | stream of `Vec<WireEvent>`, Bulk; reply `ImportDone` | `content` stays a lent host file |
| `ImportTail { importer, file, head, tail }` | `debrief.import.v1` `Tail` | stream, Live; reply `TailDone` | |
| `SniffConfig`, `ParseConfig` | `debrief.config.v1` `Sniff`, `Parse` | unary; Live, Bulk | `Limits` carried in the body |
| `Annotate { annotator, ticket, since }` | `debrief.annotate.v1` `Annotate` | unary, Bulk | reads events through the host side below |
| `Transform { transform, source_name, rows }` | `debrief.transform.v1` `Transform` | unary, Bulk | |
| `SniffProfile`, `ImportProfile` | `debrief.profile.v1` `Sniff`, `Import` | unary Live; stream of pieces, Bulk | reply `ProfileDone` |
| `SniffDocument`, `ImportDocument`, `ResymboliseDocument` | `debrief.document.v1` `Sniff`, `Import`, `Resymbolise` | unary Live; streams, Bulk | reply `DocumentDone` |

| Protocol 1 (`HostRequest`, plugin to host) | Protocol 2 | Notes |
|---|---|---|
| `ReadFile { id, offset, len }` | `HostCall::ReadFile` | same limits (`READ_CHUNK` 1 MiB) |
| `TaskStarted`, `TaskProgress`, `TaskFinished` | `HostCall::TaskStarted`, `Report`, `TaskFinished` | the `tool` field is kept only on `TaskStarted` |
| `InvokeElevated { tool, op, args, timeout_ms }` | `HostCall::Elevated` | only from the normal process, ops the user granted |
| `SaveOutput` (+ the host's `PullOutput`) | `HostCall::Output { name, bytes, last }` | pieces pushed by the plugin; files land in the host's outputs folder for the plugin. PH2b checks that captures still land where Debrief's capture views look (`<data>/captures/<folder>` today) |
| `Log` | `HostCall::Log` | |
| `ViewSelect`, `ViewInstances`, `ViewTimeRange` | `HostCall::Extension` `debrief.view.v1` `Select`, `Instances`, `TimeRange` | served by Debrief during an annotation, under its `ticket` |

Package 0.5 (shared API design) fixes the exact request and reply enums of each namespace from this table; PH2a builds them
with round-trip tests of every body.

### Migration of user state

What a 1.1 install has on disk, and what 1.2 does with it. Each row has a test for a missing file and a corrupt file (PH3,
with fixtures of invented content, never a user's files).

| State (1.1) | Where | 1.2 | Missing / corrupt |
|---|---|---|---|
| Installed format-1 packages (`.debriefplugin`, protocol-1 binaries) | `<data>/plugins/<id>/install.json` (Debrief `InstallRecord`, `format` 1), `v-<version>-<digest>/`, `data/` | **Kept, not run.** Before `PluginsDir` reads the folder, a one-time step moves each Debrief-format plugin to `<data>/plugins-1.1/<id>/`. Both apps use `install.json` format 1 with different shapes, so the shared reader would see an empty or damaged record. The Plugins view lists them as "Built for Debrief 1.1: rebuild it from source and reinstall it", with Remove. Installing a rebuilt package with the same id moves its `data/` folder over | a missing record: listed by folder name as unreadable, with Remove; a corrupt one: the same, and the file is left as it is |
| Installing a `.debriefplugin` file in 1.2 | the install dialog | **Refused** with the same rebuild text (`Refusal::Legacy`) | not applicable |
| Trusted publisher keys | `settings.json`, "Plugin trust" section, `trustedKeys` lines `Name = debrief-plugin-public-key ed25519 <hex>` | **Migrated:** the same Ed25519 key, rewritten with the `paddock-plugin-public-key ed25519` prefix on first load and saved once | missing: none trusted (the default); a corrupt line is dropped with a notice naming the line, and the others are kept |
| Developer mode | the same section, `developerMode` | **Kept** | missing or corrupt: off |
| Grants of third-party plugins | `install.json` `granted` (Debrief `Permission`) | **Reset:** default deny. The rebuilt package's manifest asks again (grants by `GrantRef`, ADR 0001); the old grants are shown as a hint in the install dialog | not applicable (kept in `plugins-1.1/`) |
| Enabled flags, first-party plugins | `<config>/plugins.json` (`disabled` ids, version 1) | **Kept**: first-party plugin ids do not change in 1.2 | missing or corrupt: all enabled (`VersionedFile` defaults) |
| Enabled flags, third-party plugins | `install.json` `enabled` | **Migrated** to the rebuilt package's install record when its id is installed again | not applicable |
| Quarantine | none: Debrief 1.1 keeps quarantine in memory only (`debrief-core/src/guard.rs`, no state file) | **New** shared `paddock-guard` state file, starting empty | missing or corrupt: defaults (`paddock-guard`'s own tests) |

### Who uses what

| | `paddock-ipc` | `-sandbox` | `-guard` | `-view` | `-plugin-api` | `-plugin-host` | `-plugin-package` | App contract | MCP tools |
|---|---|---|---|---|---|---|---|---|---|
| Debrief 1.2 | yes | yes (PH1) | yes | later (plugin panels) | yes | yes (app and store worker) | yes | `debrief.describe`, `.import`, `.config`, `.annotate`, `.view` (host side), `.transform`, `.profile`, `.document` (all `.v1`) | no |
| Pacenotes | yes | yes | yes | yes (the core UI renders all views) | when its plugins go out of process | when its plugins go out of process | then | `pacenotes.index`, `.review`, `.requirements`, `.coverage` | yes (the core owns transport, auth and budgets; plugins contribute read-only tools) |
| Dyno | 1.0: its own copy (`dyno-ipc`); tier 3 later | tier 3 | tier 3 | tier 3 (benchmark and sensor pages) | tier 3 | tier 3 (in the agent) | tier 3 | `dyno.sensors`, `.cleanup`, `.actions` | no |
| Pitwall, Spotter, Pitcrew, Sighter | no | no | no | no | no | no | no | none (core plus declarative TOML packs) | no |

Pacenotes runs Understand, Review and the Debrief bridge **in its engine first**, behind its plugin port, and packages them
as out-of-process plugins later (user decision, 2026-10-29). Dyno differs: its host is the always-on agent (a service), not
the UI, and its cleanup plugins only *propose*; Dyno's safety policy keeps checking every path. Dyno 1.0 ships on its own
small copy and adopts the platform when tier 3 is built.

## Consequences

- Debrief 1.2 hosts plugins only through the shared platform, with OS enforcement from `paddock-sandbox`.
- Third-party plugin authors must rebuild once; their sources compile unchanged until 1.4.
- The shim is the only code that exists to serve the old contract, and it has a removal release.

## Open questions

- Whether the shim should also cover third-party plugins built from the published ADR 0027 SDK (none known). Proposal: yes
  for sources (it costs nothing extra, it is the same re-export), never for binaries.

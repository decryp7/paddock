# 3. Moving Debrief onto the platform, and who uses what

- Status: Accepted (2026-10-31), after an independent review and a confirmation review. Proposed 2026-10-23; revised
  2026-10-31 with the user's decisions of that day: no legacy support at all (no binary legacy mode, no source shim;
  old plugins are removed), grants reset, trusted keys migrated.
- Related: ADRs 0001 (the platform), 0002 (views), 0014 (monorepo); Debrief ADRs 0003 (scoped by this move), 0017, 0027;
  plan [debrief-1-2.md](../plans/debrief-1-2.md) packages PH1 to PH4, 2.6a to 2.6d, 0.5 (D1: move in 1.2)

## Context

Debrief has the mature implementation, 11 first-party plugin crates (`debrief-plugin-formats`, `-dotnet`, `-dumps`,
`-native`, `-otel`, `-profiling`, `-registry`, `-syslog`, `-tools`, `-web`, `-windows`) served by the `debrief-plugins` bundle,
and a published SDK (Debrief ADR 0027). The move must not break those plugins or Debrief's release. Under the "no backward
compatibility before v1" rule there is no compatibility promise, and no third-party plugins are known. The user decided on
2026-10-31: "no need to keep legacy plugins. Just remove them." So there is neither a binary legacy mode nor a source
shim.

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
3. **Old plugins removed** (PH3). On the first start of 1.2, installed format-1 (protocol-1) plugins are removed (see
   "Migration of user state"). A protocol-1 binary or `.debriefplugin` file is never started or installed: it is refused
   with `Refusal::Legacy`, whose text says the plugin is no longer supported. There is no source shim:
   `debrief-plugin-api`'s 1.1 names are not kept, and a plugin is written against `paddock-plugin-api` +
   `debrief-plugin-ext`. The shared host has no hook for old protocols; the frozen `fixture_v1` in `paddock-plugin-test`
   keeps the handshake refusal tested. **In the same change**, the SDK docs (`apps/debrief/docs/plugin-sdk.md`, Debrief ADR
   0027) move to Paddock `docs/plugin-sdk.md` and describe the platform SDK plus `debrief-plugin-ext`.
4. **First-party plugins rebuilt** against protocol 2 in the same release (PH4).
Order: 1 (done), then 0 and 2's contract half (PH2a), 2's host half (PH2b), 3 (PH3) and 4 (PH4). There is no later phase.

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
| `Sniff { importer, file, head }` | `debrief.import.v1` `Sniff` | unary, Live | skipped when the manifest's declarative sniff decides (plan 2.6a) |
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
with fixtures of invented content, never a user's files). The removal of old plugins has two more: an old install record
and its folder are removed and the notice is shown; a missing or corrupt old record does not stop startup.

| State (1.1) | Where | 1.2 | Missing / corrupt |
|---|---|---|---|
| Installed format-1 packages (`.debriefplugin`, protocol-1 binaries) | `<data>/plugins/<id>/install.json` (Debrief `InstallRecord`, `format` 1), `v-<version>-<digest>/`, `data/` | **Removed** on the first start of 1.2, before `PluginsDir` reads the folder (both apps use `install.json` format 1 with different shapes, so the shared reader must never see a Debrief record): the plugin's install record and files, its `data/` folder included, are deleted; one notice is logged with the removed ids; the user sees one line, "Plugins built for older Debrief versions were removed; they are no longer supported." Debrief's own built-in plugins are installed again as signed platform packages (`Builtin`, the family plugin key, ADR 0001) | a missing record: the folder is recognised by its version folders and removed all the same; a corrupt one: removed too; a folder that cannot be deleted (locked, no rights) is logged and retried at the next start. Startup never stops for any of these |
| Installing a `.debriefplugin` file in 1.2 | the install dialog | **Refused** with "no longer supported" (`Refusal::Legacy`) | not applicable |
| Trusted publisher keys | `settings.json`, "Plugin trust" section, `trustedKeys` lines `Name = debrief-plugin-public-key ed25519 <hex>` | **Migrated:** the same Ed25519 key, rewritten with the `paddock-plugin-public-key ed25519` prefix on first load and saved once | missing: none trusted (the default); a corrupt line is dropped with a notice naming the line, and the others are kept |
| Developer mode | the same section, `developerMode` | **Kept** | missing or corrupt: off |
| Grants of third-party plugins | `install.json` `granted` (Debrief `Permission`) | **Reset:** removed with the plugin; a platform package starts at default deny (grants by `GrantRef`, ADR 0001) | not applicable |
| Enabled flags, first-party plugins | `<config>/plugins.json` (`disabled` ids, version 1) | **Kept**: first-party plugin ids do not change in 1.2 | missing or corrupt: all enabled (`VersionedFile` defaults) |
| Enabled flags, third-party plugins | `install.json` `enabled` | **Removed** with the plugin | not applicable |
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
- No code serves the old contract: no shim, no legacy mode, no removal release to plan.
- A third-party plugin (none known) must be ported to the platform SDK and installed again; its 1.1 data is gone.

## Decided questions

- Is anything of 1.1's plugin contract kept (a source shim for ADR 0027 SDK plugins, a binary legacy mode, old installed
  plugins)? **No** (user decision, 2026-10-31: "no need to keep legacy plugins. Just remove them."). Kept from 1.1's plugin
  state are only the trusted keys, developer mode and the built-in plugins' enabled flags.

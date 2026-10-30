# 1. The `paddock-plugin` family: one out-of-process plugin platform for Debrief, Pacenotes and Dyno

- Status: proposed (2026-10-23; revised 2026-10-31 after an independent review). **Built** in Paddock 0.3.0 (plan X51,
  2026-10-25) with the differences under "Implementation notes". The revision adds what Debrief 1.2 needs before PH1 to
  PH4 start: the OS sandbox in Paddock, streamed and host-side extension calls, liveness, and parallelism and cost. The
  coordinator accepts it after a confirmation review.
- Versioning and consumption follow ADR 0014 §4 and §5 (one repository, path dependencies, tags per app), which
  supersede ADR 0006's versioning and consumption parts; this ADR does not depend on ADR 0006.
- Related: Debrief ADR 0017 (one IPC, out-of-process plugins) and 0027 (plugin SDK); ADRs 0002 (declarative views), 0003
  (Debrief's move), 0013 (workbench), 0014 (monorepo), 0024 (background work), 0031 (patience, not wall clocks);
  Pacenotes `apps/pacenotes/docs/architecture-direction.md`; Dyno's `dyno-plugin-api` / `dyno-plugin-host`

## Context

Three apps want real plugins: Debrief (importers, parsers, tools, profilers; ADR 0017), Pacenotes (understand, review,
diagrams, requirements, tests; contributes MCP tools and views) and Dyno tier 3 (vendor sensors, benchmarks, optional backup).
Each has, or would have, its own copy of the same machinery: frame codec, private endpoint and handshake, process launcher and
sandbox, supervisor with restart and quarantine, guarded adapters, signed packages, manifest and permissions. Debrief's is the
mature one (`debrief-ipc`, `debrief-plugin-api`, `debrief-plugin-host`, `debrief-plugin-package`, `debrief-core::guard`). Dyno's
`dyno-ipc`/`dyno-plugin-*` is a smaller sibling of it. With three users the rule of three is met, so the platform moves to Paddock.
Pitwall, Spotter and Pitcrew stay plugin-free (user decision, 2026-10-22).

## Decision

**Rust only, out of process only, one IPC, no web code from plugins.** The family keeps Debrief ADR 0017 and generalises only
what is app specific. The platform knows *how* to host, isolate, call, sign and describe a plugin; each app owns *what* its
plugins may contribute.

### Crates

| Crate | Role | Depends on | Taken from |
|---|---|---|---|
| `paddock-ipc` | frames (4-byte length, postcard), private endpoint, per-launch token, `Hello`/`Welcome` handshake with protocol version and peer pid; `peer` (calls both ways, streams with credits, cancel, pushes); `launch`/`child` (`ChildSpec`, `Bootstrap`); `link` (an app and its companion). Synchronous, on threads, no async runtime | `paddock-proc`, `paddock-jobs`, `paddock-env` | `debrief-ipc`, `dyno-ipc` |
| `paddock-sandbox` (**new, PH1**) | `SandboxSpec` and its OS enforcement: containment by the launcher before the first instruction, lockdown by the child, typed `Gap` notes (see "OS enforcement") | `paddock-proc`; `windows`, `landlock` per target | `debrief-ipc/src/sandbox.rs`, `contain.rs` |
| `paddock-guard` | `catch` (panic to error), `Guard` (call budget, `Failure::Hang`), `Quarantine` (3 failures in 10 min, versioned state file) | `paddock-env`, `paddock-jobs` | `debrief-core::guard` |
| `paddock-view` | the declarative view model (ADR 0002) as serde types, validation, size limits; `GrantRef` | `paddock-ipc` | new |
| `paddock-plugin-package` | `.paddockplugin` archive, the manifest model (`Manifest`, `Permission`, `Namespace`), Ed25519 signing, trust store, verification | `paddock-ipc`, `paddock-view`, `paddock-env` | `debrief-plugin-package` |
| `paddock-plugin-api` | what a plugin links: `Plugin`, `serve`, the wire (`protocol`), `compat` (`HostInfo`, `Refusal`), contributions (tool, settings, view, MCP tool) and `extension` (`Contract`, the app-specific bodies) | `paddock-ipc`, `paddock-plugin-package`, `paddock-view`, `paddock-settings` | `debrief-plugin-api` (platform half) |
| `paddock-plugin-host` | what an app links: `PluginsDir` (installed packages), `Grants` and the pure `sandbox_for`, **`Supervisor`** (start on first use, hash check before every start, restart with `paddock_jobs::time::Backoff`, quarantine through `paddock-guard`), `Remote<C>`, `HostServices` (the host side of `HostCall`), `Status` | `paddock-ipc`, `paddock-guard`, `paddock-plugin-api`, `paddock-plugin-package`, `paddock-view`; `paddock-sandbox` from PH1 | `debrief-plugin-host` |
| `paddock-plugin-test` + `cargo-paddock-plugin` | fake host, fixtures and conformance for plugin tests (including the frozen protocol-1 fixture `fixture_v1`); `new` / `test` / `package` / `sign` | api, host | `debrief-plugin-test`, `cargo-debrief-plugin` |

There is no `Supervisor` in `paddock-ipc`: supervision is the host's (`crates/paddock-plugin-host/src/supervisor.rs`).

Per-app contract crates stay in the apps and sit on top: `debrief-plugin-ext` (Debrief's `debrief.*` namespaces and the domain
types `Record`, `Level`, `WireEvent`; ADR 0003), `pacenotes-plugin-api` (index queries, findings, requirement links),
`dyno-plugin-api` (sensors, cleanup candidates, actions). Domain types never move into Paddock.

### Wire (extends Debrief ADR 0017 §1; frames unchanged)

As built in `crates/paddock-plugin-api/src/protocol.rs` (abridged; what this revision adds is marked **new**):

```rust
pub const PLATFORM_PROTOCOL: ProtocolVersion = ProtocolVersion::new(2, 0); // 1.x was Debrief's own protocol: refused

pub enum PlatformRequest {                                   // app -> plugin
    Describe,                                                // -> Json(Described)
    ApplySettings { json: Json },                            // -> ()
    Invoke { tool: String, op: String, args: Json },         // -> Result<Json, Fail>; the host grant-checks path args
    RenderView { view: String, ctx: Json },                  // -> Result<Json(ViewDoc), Fail>     (ADR 0002)
    ViewAction { view: String, action: String, args: Json }, // -> Result<Json(ViewPatch), Fail>
    McpTool { tool: String, args: Json, budget: McpBudget }, // -> Result<Json, Fail>               (read-only tools)
    Stop { task: String },                                   // -> Result<(), Fail>; the id from HostCall::TaskStarted
    Extension { ns: String, minor: u16, body: Vec<u8> },     // -> the contract's own postcard reply; may stream (below)
}
pub enum HostCall {                                          // plugin -> app, while a request is answered
    ReadFile { id: u64, offset: u64, len: u32 },             // -> Vec<u8>; len <= READ_CHUNK (1 MiB)
    Report { task: String, progress: Option<f32>, note: String }, // -> ()
    TaskStarted { tool: String, task: ToolTask },            // -> ()
    TaskFinished { task: String },                           // -> ()
    Output { name: String, bytes: Vec<u8>, last: bool },     // -> Option<SavedOutput>; pieces <= OUTPUT_CHUNK (256 KiB)
    Elevated { tool: String, op: String, args: Json, timeout_ms: u64 }, // -> Result<Json, Fail>; granted ops only
    Log { lines: Vec<String>, dropped: u64 },                // -> (); at most 64 lines of at most 4 KiB
    Extension { ns: String, minor: u16, body: Vec<u8> },     // new, at the end (a minor change): an app contract's host side
}
```

| Direction | Variant | Reply | Answered by | Notes |
|---|---|---|---|---|
| app to plugin | `Describe` | `Json` of `Described` | plugin, at start | compared with the verified manifest (`compat::check_described`) |
| app to plugin | `ApplySettings` | `()` | plugin | validated by the app first (`paddock-settings`); sent again after a restart |
| app to plugin | `Invoke` | `Result<Json, Fail>` | the tool | path arguments grant-checked by the host |
| app to plugin | `RenderView`, `ViewAction` | `ViewDoc`, `ViewPatch` | the view | validated by `paddock-view` before the app uses them |
| app to plugin | `McpTool` | `Result<Json, Fail>` | the MCP tool | `McpBudget` enforced by the app's server too |
| app to plugin | `Stop` | `Result<(), Fail>` | the tool | the task id is a `String` |
| app to plugin | `Extension` | contract reply bytes | the contract's handler | unary through `Remote::call`, streamed through `Remote::stream` (new) |
| plugin to app | `ReadFile` | bytes | `HostFiles` | only files the app opened for this plugin |
| plugin to app | `Report`, `TaskStarted`, `TaskFinished` | `()` | `TaskObserver` | `Report` counts as call progress (ADR 0031) |
| plugin to app | `Output` | `Option<SavedOutput>` | `OutputSink` | names checked; at most 8 open, 8 GiB each |
| plugin to app | `Elevated` | `Result<Json, Fail>` | `ElevatedRunner` | only ops the manifest lists and the user granted |
| plugin to app | `Log` | `()` | the app's child log | lines cut, batches capped |
| plugin to app | `Extension` (new) | contract reply bytes | the app's handler for `ns` | an `ns` the app does not serve or the manifest does not list: `RemoteError` `denied` |

**Namespaces.** An app contract names its namespace with its major in the name (`debrief.import.v1`, as
`extension::Contract::NS` documents) and sends the minor with each call. A manifest lists each namespace and its minor as a
contribution (`kind = "extension"`, `ns = "debrief.import.v1/0"`). The platform routes by `ns` and never decodes `body`.

**Streamed extension calls (new, package 2.6).** The platform stays domain free, so it gets no `import_stream`. A contract
may stream instead:

- host: `Remote<C>::stream(&self, request: &C::Request, credit: u32) -> Result<RemoteStream<C>, HostError>`, where
  `C: StreamContract` adds `type Item`; `RemoteStream::next()` yields `Item(C::Item)` or `Done(C::Reply)`, and `cancel()`
  ends it. It is `paddock_ipc::Peer::stream` under the supervisor's guard: a garbled item, more items than credits or a
  stall is a fault attributed to the plugin, as for a unary call;
- plugin: `Contributions::add_stream_extension::<C>(handler)`; the handler gets an `ItemSink<C::Item>`, whose `send(item)`
  blocks while the host holds no credit (back-pressure reaches the parser) and fails once the call is cancelled;
- each item counts as call progress for the stall judgement (ADR 0031 §2);
- Debrief's log, tail, profile and document imports are streamed calls of their `debrief.*` namespaces (ADR 0003).

**Host-side extension calls (new, package 2.6).** `HostCall::Extension { ns, minor, body }` lets a plugin call an app
contract back while it answers (Debrief's `View*` reads during an annotation). The app registers one handler per namespace on
`HostServices`. As with `ReadFile`, while a host-side call is outstanding the host owes the plugin an answer, so the plugin
gets no blocked ticks (ADR 0031 §2).

### Guarded adapters, quarantine

`paddock-plugin-host` exposes `Remote<C>` proxies that only `Supervisor::remote` can make, and every call goes through
`paddock_guard::Guard`: a panic, a hang, or an oversized or garbled frame is an error attributed to the plugin and counted
toward quarantine; a typed `Fail` is not a crash. `Quarantine { max: 3, window: 10 min }` is configurable per app; its state is
versioned and persisted, and a missing or corrupt file gives the defaults. Apps never call plugin code except through these types.

### Liveness (ADR 0031 §2 and §3)

Whether a plugin call or start is still working is a **stall judgement**, not a wall clock. It reads call progress (streamed
items, `Report`, answered host calls, lent-file reads), the child's CPU time and its run state, per call, on host ticks
(ADR 0031 §2); in-process guarded calls count host ticks (§3). The fixed timeouts of `paddock-plugin-host` are an **open ADR
0031 instance**: they are liveness bounds (class L), and ADR 0031 §7's table does not name them:

| Where (`crates/paddock-plugin-host/src/`) | Today | ADR 0031 class | Becomes |
|---|---|---|---|
| `config.rs` `HostConfig::call_timeout` | 30 s for every call without its own budget | L | a stall judgement per call |
| `config.rs` `HostConfig::hang_grace` | 5 s past the timeout and the cancel grace | L | removed (the judgement decides) |
| `config.rs` `HostConfig::start_timeout` | 15 s to start and answer `Describe` | L | the start judged as a call (§2) |
| `config.rs` `HostConfig::stop_grace` | 2 s to exit after a stop | S | stays |
| `supervisor.rs` `NO_BUDGET` | 1 h for a call without a budget | L; C | a cap only where an operation declares one |

Package 2.6 converts them, tested with a fake clock and fake run states (no sleeps). Until then they stay as listed.

### Capabilities, permissions and resources

Three separate things, all in the manifest:

1. **Contributions** (what it adds, shown in the UI): `Tool`, `Settings`, `View`, `McpTool`, plus `Extension` namespaces.
2. **Permissions** (what it may touch, granted by the user, default deny): `OpenedFilePaths`, `ReadFolders(Vec<GrantRef>)`,
   `WriteFolders(Vec<GrantRef>)`, `RunPrograms(Vec<ProgramRef>)`, `Network(Vec<HostPattern>)`, `Elevated(Vec<String>)`
   (named operations only, Debrief ADR 0017 §7), `McpExpose`.
3. **Resource requests** (how much it needs; not a permission): memory and CPU share.

**One memory policy.** Memory appears in three places today: `Permission::Memory { mib }` (a permission the user grants),
`Described::memory_limit` (what the plugin says at start) and the 1.2 plan's `MemoryPolicy` (package 2.2, in the grants).
Decision: the manifest's `[resources]` (`memory_mib`, `cpu_percent`) is a *request*. The app's `MemoryPolicy` (`Cap { bytes }`
or `Unlimited { machine_floor }` from `paddock-proc`, per plugin in the grants) sets the cap. The request is where the policy
starts and what the Plugins view shows. `Permission::Memory`, `Permission::Cpu` and `Described::memory_limit` are removed in
package 2.6 (no compatibility before v1). A cap stop is `LimitExceeded`, not a fault (plan 2.6).

`sandbox_for(&Manifest, &Grants) -> SandboxSpec` stays a pure function (tested table). The default is `SandboxSpec::locked()`.

### OS enforcement (`paddock-sandbox`, PH1)

**Decision: a new crate, `paddock-sandbox`, not `paddock-ipc::contain`.**

- The sandbox is its own concept and serves more than plugins: Debrief's store client and video runner start sandboxed
  children through `debrief_ipc::sandbox` today. Naming the crate by its concept (family naming) lets any child use it.
- It is heavy and differs per OS (the `windows` crate's security and isolation APIs, `landlock`, seccomp BPF, the macOS
  profile). `paddock-ipc` stays a small transport that companions and services can link without that weight (single
  responsibility).
- Dependency inversion: `paddock_ipc::launch` gains one port, `ChildSpec::contain: Option<Arc<dyn Contain>>`, which
  `paddock-sandbox` implements, so `paddock-ipc` does not depend on it. On the child side, `paddock_plugin_api::serve` calls
  `paddock_sandbox::lockdown` before it handles any request.

What moves from `apps/debrief/crates/debrief-ipc/src/{sandbox,contain}.rs`:

- `SandboxSpec`, one type (the host's `grants::SandboxSpec` becomes it), and `FsAccess`;
- `Containment`: `SelfApplied` for the app's own children, and `Launcher` for plugin packages. Under `Launcher`, Windows uses
  an AppContainer plus a job object, assigned while the process is suspended; Linux applies rlimits, `no_new_privs`,
  landlock and the seccomp network filter between `fork` and `exec`; macOS uses `sandbox-exec` with the generated profile;
- `lockdown`;
- the typed `Gap` notes for what a system cannot enforce. Linux without landlock leaves files unconfined, a user decision
  kept from Debrief ADR 0017 §6.

**PH1 owns the move.** On each OS's CI runner, a test checks that a contained plugin cannot read a file outside its grant and
can read one inside it; the existing degradation tests move too. **Debrief 1.2 never ships without OS enforcement.**
`Supervisor` refuses to start a plugin when the app has configured no `Contain` (a typed error, tested); only tests and
developer mode may pass an explicit `Contain::none()`. The release checklist checks this.

**Network.** The OS enforces `Permission::Network` only through `paddock-sandbox`: seccomp on Linux, the profile on macOS,
AppContainer capabilities on Windows under `Launcher` containment. Without containment, or on Windows under `SelfApplied`
(low integrity does not block sockets), the grant limits only what the host brokers. The Plugins view then shows a `Gap`
("network not enforced") and never presents the grant as enforced.

### Signing, trust and packaging

`.paddockplugin` = zip with `manifest.toml`, per-platform binaries (`bin/<target>/name`), `SHA256SUMS`, and a detached Ed25519
`signature` over the canonical manifest plus sums. Trust levels:

- `Builtin`: shipped with the app and **signed by the family plugin key**, one key for every app's built-in plugins (like
  the single updater key from ADR 0014's amendment). The key is a CI secret and never on a workstation; the app passes
  its public half as its built-in key list.
- `Signed`: a publisher key the user trusted.
- `Unsigned`: for development only, refused unless the app's developer mode is on.

The host verifies a package before its first start and on every update; a binary hash that changed after install is a
quarantine-grade fault. `Signed` and `Unsigned` binaries always run under `Launcher` containment.

**Trust on declaration.** The platform cannot check two flags, which are the plugin's own claims: that an MCP tool is
read-only (`McpExpose`), and that a view or tool action does not write (`writes: false`, which is what lets a view onto MCP).
The app honours them only for `Builtin` and `Signed` plugins. An `Unsigned` plugin's MCP tools are not exposed and its views
count as writing. Whatever the flags say, the OS sandbox still bounds what a plugin can touch.

### Versioning and compatibility

- The plugin's `PLATFORM_PROTOCOL` major must equal the host's, and its minor must be one the host knows (the host runs
  older minors). Each `Extension` namespace carries its own major (in the name) and minor under the same rule. The rule
  exists once, in `paddock_ipc::ProtocolVersion` (`accepts`, `skew`, `introduced_in`, `mismatch`).
- **No binary compatibility across a major.** A plugin built for an older major is refused with a message telling the
  user to rebuild it from source with the current SDK: `Refusal::PluginTooOld`, or `Refusal::Legacy` for Debrief's
  protocol 1 (user decision, 2026-10-31). The shared host has no hook for serving an old protocol.
- **Support window for the previous major.** When a new major ships, the SDK keeps a source-level shim for the previous
  major for two minor releases of each app that hosts plugins, then removes it. The shim re-exports the old names over the
  new API, so a plugin recompiles unchanged. The release notes of the major give the removal release.
- The refusal text says who must act ("plugin X needs Debrief 1.4 or newer", or "plugin X was built for an older contract;
  rebuild it from source").
- Wire types never use `untagged`, `flatten`, internal tags or `skip_serializing_if` (postcard). Fuzz targets for the
  decoders live in `paddock-ipc`, `paddock-plugin-package` and `paddock-view`.

### Parallelism and cost (rule 51, ER-20)

- **Processes.** One process per plugin, started on first use and stopped when idle. Package 2.6's lifecycle replaces
  Debrief's `warm_up` of every built-in plugin
  (`debrief-plugin-host/src/builtins.rs`), so a project starts only the plugins it needs. Each process has its own memory
  cap (default 1 GiB, range 64 MiB to 4 GiB, `grants.rs`), measured as physical memory (rule 26).
- **Calls in flight.** The wire allows 64 concurrent calls per connection (`PeerOptions::max_concurrent_calls`), each on
  its own thread in the child. That is a ceiling, not the plan. The app's executor and job limits decide parallelism
  (Debrief: one import per window, ADR 0012; files of one import in parallel up to the ER-20 bound), so the platform adds
  no pool of its own.
- **Stream credits.** A streamed call has at most `credit` items in flight. Debrief uses 4 today (`CREDIT` in
  `debrief-plugin-host/src/process.rs`, `OUTPUT_CREDIT` in the protocol). Each item is at most one frame (16 MiB default
  `FrameLimit`) and by convention about 64 KiB of events, so a stream holds about `credit x item size` in flight. The default
  is a `HostConfig` value behind an "Auto" setting; changing it needs a recorded benchmark.
- **Parse threads.** A plugin that parses in parallel sizes its pool with `paddock_proc::threads(kind, class)` (ADR 0024
  §1). CPU-bound work gets cores minus one **in total**, so the app limits concurrent bulk calls across plugins; each
  plugin does not take cores minus one for itself.
- **Priority class.** A plugin child serves live requests (sniffs, views, tools), so it is launched at `User`, never at
  `Background` or `Idle` (ADR 0024 §1). Today `Supervisor` leaves `ChildSpec::lower_priority` off. Priority is set per call:
  `Contract::role(&Request) -> WorkRole` (default `Live`) marks which requests are **Bulk** (an import) and which are
  **Live** (a sniff, a tail the user is watching). These are rule 52's roles; rule 52 is on its way to main with
  `fix/dyno-bundle-ui`. `serve` runs Bulk calls at the gentle level (`QOS_CLASS_UTILITY` on macOS, nice 10 on Linux,
  `THREAD_PRIORITY_BELOW_NORMAL` on Windows) and Live calls at normal level. Nothing a person waits on runs at a starvable
  level (`PRIO_DARWIN_BG`, QoS `BACKGROUND`, `SCHED_IDLE`).
- **Cheaper operations.** Files are lent by range (`ReadFile`, 1 MiB pieces), not copied whole. Events cross in batches
  per item, not one per frame. Sniffing is declarative where it can be (extensions and magic in the manifest, so no process
  starts; plan 2.6). Contract bodies are postcard, not JSON.
- **Benchmark to record (rule 48).** Per-call overhead on each OS: an empty unary `Extension` round trip, and a streamed call
  of N 64 KiB items at credits 1, 4 and 16, in-process fake against a real child. Package 2.6 records it in
  `docs/benchmarks.md` (shared code) before any number is quoted, and the default credit is chosen from it.

## Consequences

- Debrief's mature code becomes the reference. Dyno 1.0 ships on its own copy, which is deleted after Dyno moves (tier 3
  needs only `Tool`, `Settings`, `View` and its own `dyno.sensors` namespace).
- Domain stays in the apps; the platform has no notion of logs, code or sensors.
- Cost: one more indirection (`Extension` bytes) for app contracts. Accepted, because it keeps the platform closed to change.
- The OS sandbox becomes a family crate, so one tested implementation contains every child (plugins and workers).

## Open questions

- Async runtime in `paddock-ipc`: none (a synchronous core on threads), settled by the build.
- Whether `HostCall::Extension` raises the protocol to 2.1, or keeps 2.0 because no app has shipped protocol 2.0 (pre-v1,
  no compatibility promise). Default: 2.1, the rule as written.

## Implementation notes (plan X51, 2026-10-25)

Built as described, with these deliberate differences:

- `paddock-plugin-package` owns the manifest model (`Manifest`, `Permission`, `Namespace`, ...) so a package can be read and
  verified without the API crate. `paddock-plugin-api` depends on it and checks a plugin's `Describe` against the verified
  manifest (`compat::check_described`). Settings schemas and validation are `paddock-settings`' (`SettingsSection`,
  `Configurable`), not a second copy.
- `paddock-ipc` has no `Supervisor`. Restart with `paddock_jobs::time::Backoff` and the quarantine live in
  `paddock-plugin-host::Supervisor`, on `paddock-guard`; the crates table above now says so.
- The host maps manifest and grants to a `SandboxSpec` (pure data). At X51 the OS-level sandbox stayed in the app; this
  revision moves it to `paddock-sandbox` in PH1. The elevated process (a second process of the plugin's binary with the
  `elevated` role) stays the app's: the platform offers the role in `serve`, the `HostCall::Elevated` call (allowed per
  name) and the `ElevatedRunner` port.
- The version rule exists once: `paddock_ipc::ProtocolVersion` (`accepts`, `skew`, `introduced_in`, `mismatch`), re-exported
  by `paddock-plugin-package` and `paddock-plugin-api`. `HostInfo` in `paddock-plugin-api::compat` is the host's policy and
  words the refusals on top of it. `GrantRef` likewise exists once, in `paddock-view`, re-exported by the package.
- Protocol 1 (Debrief's own) is refused by the version gate: at the handshake (`IpcError::Version`, tested with the frozen
  `fixture_v1`) and from a manifest (`Refusal::Legacy`). There is no binary legacy mode (user decision, 2026-10-31); the only
  compatibility is Debrief's source-level shim (ADR 0003 phase 3).
- Still to build after the 2026-10-31 revision: `paddock-sandbox` (PH1); `Remote::stream`, `StreamContract`,
  `HostCall::Extension`, `Contract::role`, resource requests with one memory policy, and the liveness conversion
  (package 2.6); refusal texts that say "rebuild it from source" (PH3).

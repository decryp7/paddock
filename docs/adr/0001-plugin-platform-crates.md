# 1. The `paddock-plugin` family: one out-of-process plugin platform for Debrief, Pacenotes and Dyno

- Status: proposed (2026-10-23); implementation starts after Debrief's release (see ADR 0006)
- Related: Debrief ADR 0017 (one IPC, out-of-process plugins) and 0027 (plugin SDK); Pacenotes `docs/architecture-direction.md`;
  Dyno's `dyno-plugin-api` / `dyno-plugin-host`; ADRs 0002 to 0004 and 0006 here

## Context

Three apps now want real plugins: Debrief (importers, parsers, tools, profilers; ADR 0017), Pacenotes (understand, review,
diagrams, requirements, tests; contributes MCP tools and views) and Dyno tier 3 (vendor sensors, benchmarks, optional backup).
Each has, or would have, its own copy of the same machinery: frame codec, private endpoint and handshake, process launcher and
sandbox, supervisor with restart and quarantine, guarded adapters, signed packages, manifest and permissions. Debrief's is the
mature one (`debrief-ipc`, `debrief-plugin-api`, `debrief-plugin-host`, `debrief-plugin-package`, `debrief-core::guard`). Dyno's
`dyno-ipc`/`dyno-plugin-*` is a smaller sibling of it. With three users the rule of three is met, so the platform moves to Paddock.
Pitwall, Spotter and Pitcrew stay plugin-free (user decision, 2026-10-22).

## Decision

**Rust only, out of process only, one IPC, no web code from plugins.** The family keeps ADR 0017 as is and generalises only what
is app specific. The platform knows *how* to host, isolate, call, sign and describe a plugin; each app owns *what* its plugins
may contribute.

### Crates

| Crate | Role | Depends on | Taken from |
|---|---|---|---|
| `paddock-ipc` | frames, private endpoint + token + peer-pid handshake, launcher (`SpawnSpec`, `SandboxSpec`, `Elevation`), `Supervisor`, memory watcher, child log | std, tokio-free sync core + thin async | `debrief-ipc` |
| `paddock-guard` | `catch` (panic to error), `Quarantine` policy (3 failures in 10 min), attribution | none | `debrief-core::guard` |
| `paddock-view` | the declarative view model (ADR 0002) as serde types, validation, size limits | serde | new |
| `paddock-plugin-api` | what a plugin links: `Plugin`, `Manifest`, `Permission`, `serve`, platform contributions (tool, settings, view, mcp tool), the `Extension` mechanism for app-specific bodies | `paddock-ipc`, `paddock-view` | `debrief-plugin-api` (platform half) |
| `paddock-plugin-host` | what an app links: discovery of installed plugins, start/stop via the supervisor, guarded adapters, permission grants, proxy objects, status for the UI | `paddock-ipc`, `paddock-guard`, `paddock-plugin-api`, `paddock-plugin-package` | `debrief-plugin-host`, `debrief-plugin-registry` |
| `paddock-plugin-package` | `.paddockplugin` archive, manifest, Ed25519 signing, trust store, verification | none | `debrief-plugin-package` |
| `paddock-plugin-test` + `cargo-paddock-plugin` | fake host for plugin unit tests, `new` / `test` / `package` / `sign` | api, host | `debrief-plugin-test`, `cargo-imt-plugin` |

Per-app contract crates stay in the apps and sit on top: `debrief-plugin-api` (importers, parsers, annotators, transforms, profile
importers, markers; Debrief's domain types `Record`, `Level`, `WireEvent`), `pacenotes-plugin-api` (index queries, findings,
requirement links), `dyno-plugin-api` (sensors, cleanup candidates, actions). Domain types never move into Paddock.

### Wire (extends ADR 0017 §1; frames unchanged)

```rust
pub const PLATFORM_PROTOCOL: ProtocolVersion = ProtocolVersion { major: 2, minor: 0 }; // 1 = Debrief's pre-Paddock protocol

/// Body of a Call, platform level. Adding a variant at the end is minor; anything else is a new major.
pub enum PlatformRequest {
    Describe,                                              // -> Manifest
    ApplySettings { json: Json },
    Invoke { tool: String, op: String, args: Json },       // generic tool, host grant-checks Path args
    RenderView { view: String, ctx: Json },                // -> ViewDoc   (ADR 0002)
    ViewAction { view: String, action: String, args: Json },// -> ViewPatch
    McpTool { tool: String, args: Json, budget: Budget },  // -> McpResult (apps that serve MCP)
    Stop { task: u64 },
    Extension { ns: String, minor: u16, body: Vec<u8> },   // an app contract's own postcard enum, opaque to the platform
}
pub enum HostCall {                                       // plugin -> host (symmetric calls)
    ReadFile { id: u64, offset: u64, len: u32 },           // len <= 1 MiB
    Report { task: u64, progress: f32, note: String },
    Output { name: String, bytes: Vec<u8> },
}
```

An app contract declares a namespace (`debrief.import/3`) and implements `Extension` for its own request/reply enums; the
platform routes by `ns` and never decodes `body`. A manifest lists the namespaces and minor versions the plugin speaks.

### Guarded adapters, quarantine

`paddock-plugin-host` exposes `Remote<T>` proxies that are only constructible from a `Supervisor` and always wrapped in
`paddock_guard::Guarded`: a panic, hang (grace period), oversized or garbled frame is an error attributed to the plugin and counted
toward quarantine; a typed `Fail` is not a crash. `Quarantine { max: 3, window: 10 min }` is configurable per app, state is
versioned, persisted, and tolerates a missing or corrupt file (defaults). Apps never call plugin code except through these types
(AGENTS.md rule carried over).

### Capabilities and permissions

Two separate things, both in the manifest:

1. **Contributions** (what it adds, shown in the UI): `Tool`, `Settings`, `View`, `McpTool`, plus `Extension` namespaces.
2. **Permissions** (what it may touch, granted by the user, default deny):
```rust
pub enum Permission {
    OpenedFilePaths,            // paths of files the user opened (not just host-file ranges)
    ReadFolders(Vec<GrantRef>), // named grants chosen by the user, never raw paths
    WriteFolders(Vec<GrantRef>),
    RunPrograms(Vec<ProgramRef>),
    Network(Vec<HostPattern>),  // allow-listed hosts only; loopback to Pitwall is via the host, not the plugin
    Elevated(Vec<String>),      // named operations only (ADR 0017 section 7)
    McpExpose,                  // may contribute MCP tools (read-only tools only; writes are UI/CLI)
    Memory { mib: u32 }, Cpu { percent: u8 },
}
```
`sandbox_for(&Manifest, &Grants) -> SandboxSpec` is a pure function (tested table), as in Debrief. Defaults are
`SandboxSpec::locked()`. Platform degradation rules (landlock, seccomp, low integrity) stay as in ADR 0017 section 6 and are reported as typed `Gap`s.

### Signing and packaging

`.paddockplugin` = zip with `manifest.toml`, per-platform binaries (`bin/<target>/name`), `SHA256SUMS`, and a detached Ed25519
`signature` over the canonical manifest plus sums. Trust levels: `Builtin` (shipped by the app, signed by the app's key),
`Signed` (publisher key the user trusted), `Unsigned` (dev; refused unless the app's developer mode is on). The host verifies
before first start and on every update; a changed binary hash after install is a quarantine-grade fault.
Third-party binaries stay unsupported until external sandbox enforcement exists (ADR 0017 section 6); the format already allows them.

### Versioning and compatibility

- Crates follow semver per ADR 0006. `PLATFORM_PROTOCOL` major must equal; minor of the plugin must be known to the host
  (the host runs older minors). Each `Extension` ns carries its own `major/minor` with the same rule.
- Refusal text says who must update ("plugin X needs Debrief 1.4 or newer" / "plugin X was built for an older contract; update it").
- Wire types never use `untagged`, `flatten`, internal tags or `skip_serializing_if` (postcard). Fuzz targets for the decoder live
  in `paddock-ipc`.

## Consequences

- Debrief's mature code becomes the reference; Dyno's smaller copy is deleted after Dyno moves (tier 3 only needs `Tool`,
  `Settings`, `View` and its own `dyno.sensors` namespace).
- Domain stays in the apps; the platform has no notion of logs, code or sensors.
- Cost: one more indirection (`Extension` bytes) for app contracts; accepted, it keeps the platform closed to change.

## Open questions

See ADR 0006 (list at the end). Specific here: async runtime in `paddock-ipc` (Debrief's is sync core plus thin async; keep).

## Implementation notes (plan X51, 2026-10-25)

Built as described, with these deliberate differences:

- `paddock-plugin-package` owns the manifest model (`Manifest`, `Permission`, `Namespace`, ...) so a package can be read and
  verified without the API crate; `paddock-plugin-api` depends on it and checks a plugin's `Describe` against the verified manifest
  (`compat::check_described`). Settings schemas and validation are `paddock-settings`' (`SettingsSection`, `Configurable`), not a
  second copy.
- `paddock-ipc` has no `Supervisor` (the table above says it has one): restart with `paddock_jobs::time::Backoff` and the
  quarantine live in `paddock-plugin-host::Supervisor`, on `paddock-guard`.
- The host maps manifest and grants to a `SandboxSpec` (pure data); the OS-level sandbox stays in the app. The elevated process (a
  second process of the plugin's binary with the `elevated` role) is also the app's: the platform offers the role in `serve`, the
  `HostCall::Elevated` call (allowed per name) and the `ElevatedRunner` port.
- The version rule exists once: `paddock_ipc::ProtocolVersion` (`accepts`, `skew`, `introduced_in`, `mismatch`), re-exported by
  `paddock-plugin-package` and `paddock-plugin-api`; `HostInfo` in `paddock-plugin-api::compat` is the host's policy and words the
  refusals on top of it. `GrantRef` likewise exists once, in `paddock-view`, re-exported by the package.
- Protocol 1 (Debrief's own) is refused by the version gate; the compatibility shim of ADR 0003 phase 3 is Debrief's.


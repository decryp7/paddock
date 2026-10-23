# 6. Rollout order, crate versioning and how apps consume the crates

- Status: proposed (2026-10-23)
- Related: ADRs 0001 to 0005; `crates/README.md` (current rules: git tags, one repo tag)

## Decision

### Order (all after the Debrief release; no cut-down releases of the queue)

1. **`paddock-pitwall`** first: Debrief switches to Pitwall right after its release, Pitwall's contract (ADR 0009) is stable, and
   Pacenotes/Sighter/Spotter need it next. Needs from Pitwall: a published wire-types crate or fixtures (ADR 0004 open question).
   Includes `<pd-needs-ai>` in `@decryp7/paddock-ui` and the fake server.
2. **Plugin platform** for Pacenotes, in the ADR 0003 phases: domain-free crates (`paddock-ipc`, `-guard`, `-plugin-package`),
   then `paddock-view` + `<pd-view>` renderer, then `paddock-plugin-api`/`-host` with protocol 2 and the Debrief shim.
   Pacenotes v1 (core, Understand, Review, Debrief bridge) is the first consumer built on protocol 2.
3. **`paddock-testplan`** last, when Pacenotes' tests plugin and Sighter's plan import start (both after the above).
4. **Small crates** (below) are extracted opportunistically when their second user exists; none blocks 1 to 3.

### Other planned crates

| Crate | Purpose | Rule of three today |
|---|---|---|
| `paddock-env` | per-OS paths, data/config/cache dirs, portable mode, environment sanitising | Likely yes: every app has app-data/portable code (Debrief, Pitwall, Spotter, Dyno, Pitcrew) |
| `paddock-service` | install/start/stop a background service per OS (Windows service, launchd, systemd user) | Yes: Pitwall, Spotter, Dyno agent, Pitcrew; Sighter and Pacenotes headless later |
| `paddock-proc` | child process helpers with timeouts, output caps, kill-tree, job objects | Yes (Debrief tools, Dyno, Pitwall engines, Pitcrew); overlaps `paddock-ipc` launcher, which should depend on it |
| `paddock-window` | native feel: window state, tray, single instance, title bar/vibrancy, deep links | Yes: all Tauri apps copy it today |
| `paddock-notify` | OS notifications with actions and quiet hours | Two to three (Spotter, Dyno, Sighter, Pitcrew) |
| `chromium-driver` | CDP driver, already a crate in Debrief (`chromium-driver`) | Two: Debrief, Spotter (JS pages), Sighter (browser targets): extract when Spotter starts using it |
| `paddock-cli` | shared CLI conventions: `--json`, exit codes (0/1/2/3/4), time limits, credential-store key input | Yes: Pitwall, Debrief, Sighter, Pacenotes |
| `paddock-video` (proposed in Sighter) | frame capture/encode helpers | One today (Debrief `imt-video`/Sighter); not yet |

"Yes" means three or more apps have the code today; the others follow the repo rule (two for copied code, three for new
abstractions). These are listed in `crates/README.md` with the same marks.

### Versioning

- Keep the repo's single semver tag for the UI package, but **every crate has its own `version`** in its `Cargo.toml` and
  its own changelog entry; the tag `vX.Y.Z` still releases all together (simple for consumers), crates bump only when they change.
- Pre-1.0 (all of this, until v1 of the apps): minor = breaking, patch = additive or fix. Wire protocols have their own
  numbers (ADR 0001) independent of crate semver. From 1.0 on: strict semver, `cargo semver-checks` in CI on the public API.
- Compatibility matrix in `crates/COMPAT.md` (generated in CI): crate version to `PLATFORM_PROTOCOL`, view `schema`, Pitwall
  `apiVersion` range, testplan `version`.

### Consumption

- **Now: git tag dependencies**, as `crates/README.md` says: `paddock-pitwall = { git = "https://dev.decryptology.net/decryp7/paddock", tag = "v0.2.0" }`.
  Simple, no infrastructure; downside: Cargo clones the whole repo and all workspace crates resolve at that tag, and apps
  cannot depend on a single crate's version independently.
- **Target: the Gitea cargo registry** (`dev.decryptology.net/api/packages/decryp7/cargo/`): a tag-triggered CI job
  (`publish.yml`) runs the full gates for the workspace, then `cargo publish` for crates whose version changed, in dependency order,
  with a token from CI secrets. **Never published from a local machine** (the registry token exists only in CI); `Cargo.lock` stays
  committed in apps; `cargo deny` allows the private registry. Move when a second crate has a consumer, or when git clones become
  slow; the dependency lines change from `git/tag` to `version/registry`, nothing else.
- Apps bump deliberately (Renovate PR per crate); a crate change never lands in an app without that app's gates.
- Dyno, Pitwall and Spotter consume only what they need; none links the plugin family unless it hosts plugins.

### CI for crates

Each crate is added to `crates/Cargo.toml` and to its job in `.gitea/workflows/ci.yml` (fmt, clippy -D warnings, test,
llvm-cov >= 99%, deny, `cargo check --target x86_64-pc-windows-msvc` for Windows code). Fuzz targets and property tests for
every parser/decoder (`paddock-ipc` frame, `paddock-view` validate, `paddock-testplan` parse, `paddock-plugin-package` archive).

## Open questions (all ADRs)

1. Wire types for Pitwall: a Pitwall-owned `pitwall-wire` crate vs copying DTOs (ADR 0004)? Who defines `/x/v1/tool-providers`?
2. Sync-first or async-first public API for `paddock-pitwall` (Tauri commands are async; Debrief's workers are sync)?
3. Does the platform keep the `Extension { ns, body }` indirection, or should protocol 2 be generic over the app's request enum
   (`Host<R, P>`)? Proposed: indirection, for a closed platform.
4. Third-party plugins: stay unsupported until external sandboxing (AppContainer/restricted token launcher) exists; when?
5. Graph layout location and large-graph limits for views (ADR 0002); whether `paddock-view` also covers Debrief's panels.
6. Should `.sighterplan` ids for requirements be namespaced by source (`jira:ABC-1`)? Proposed: free text, optional prefix convention.
7. One shared signing key hierarchy for all apps' built-in plugins, or one per app? Proposed: one per app, rotated by release.
8. Moving to the Gitea cargo registry: before or after Pacenotes' first release? Proposed: after `paddock-pitwall` has its second consumer.
9. Does Dyno adopt the platform at tier 3, or keep its copy if tier 3 never ships? Proposed: adopt on first real plugin.
10. Debrief shim removal date (two minors proposed) and what happens to plugins from the ADR 0027 SDK in the wild.

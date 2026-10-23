# 3. Moving Debrief onto the platform, and who uses what

- Status: proposed (2026-10-23)
- Related: ADRs 0001, 0002, 0006; Debrief ADR 0017, 0027; Debrief's release branch `release/v1` (crates renamed `debrief-*`)

## Context

Debrief has the mature implementation and existing first-party plugins (`debrief-plugin-formats`, `-dotnet`, `-dumps`, `-native`,
`-otel`, `-profiling`, `-syslog`, `-tools`, `-web`, `-windows`) and a published SDK (ADR 0027). The extraction must not
break those plugins or Debrief's release, and per the "redesign until stable / no backward compatibility before v1" rules
there is no compatibility promise for a v1 that has not shipped, but the user asked that existing plugins keep working.

## Decision

**Phases (each independently releasable, tests green at each end):**

0. **Inside Debrief, before any move** (after v1): split `debrief-ipc` and `debrief-plugin-api` along the future line in place:
   the platform half (frames, launch, supervisor, sandbox, guard, package, manifest, serve) must compile without any
   Debrief domain type; domain parts (`Record`, `Level`, `WireEvent`, importers, `ConfigTree`, `Markers`) go behind
   an `Extension` namespace `debrief.*`. Proof: a `cargo tree` check in CI that the platform half has no domain dependency.
1. **Copy to Paddock** with history (`git subtree`/filter-repo), publish `paddock-ipc`, `paddock-guard`, `paddock-plugin-package` first
   (no API change, they are already domain free), Debrief consumes them by tag and deletes its copies.
2. **Platform protocol 2.** `paddock-plugin-api` and `paddock-plugin-host` published. Debrief's old `PluginRequest/PluginReply`
   enum becomes the `debrief.*` `Extension` body (same variants, same postcard), wrapped in `PlatformRequest::Extension`.
3. **Compatibility shim** (kept for two Debrief minor releases, then removed): `debrief-plugin-api` re-exports every public name it
   has today (`Plugin`, `Contributions`, `serve`, `prelude`, ...) implemented on `paddock-plugin-api`, so a plugin recompiles
   unchanged. For binaries already built against protocol 1, the host's `Hello` handler accepts `version = 1` and enters
   **legacy mode**: bodies are decoded as the old enum directly and adapted to `Extension { ns: "debrief.legacy" }` calls;
   nothing else (views, MCP) is offered to a legacy plugin. Status UI shows "built for an older Debrief, update it".
4. **First-party plugins rebuilt** against protocol 2 in the same release; legacy mode is exercised by a frozen test binary
   (`fake_plugin_v1`) so the shim has a test that triggers it.
5. Remove the shim and `fake_plugin_v1` after two minors; SDK docs (ADR 0027) move to Paddock `docs/plugin-sdk.md`.

**Safe fallbacks:** the Debrief release keeps working if any phase stalls: phases 0 and 1 change no wire bytes; phase 2 is the
only wire change and is behind the shim.

### Who uses what

| | `paddock-ipc` | `-guard` | `-view` | `-plugin-api` | `-plugin-host` | `-plugin-package` | App contract | MCP tools |
|---|---|---|---|---|---|---|---|---|
| Debrief | yes | yes | yes (plugin panels, later) | yes | yes (app and store worker) | yes | `debrief.import`, `.config`, `.annotate`, `.transform`, `.profile`, `.markers` | no |
| Pacenotes | yes | yes | yes (core UI shell renders all views) | yes | yes | yes | `pacenotes.index`, `.review`, `.requirements`, `.coverage` | yes (core owns transport, auth, budgets; plugins contribute read-only tools) |
| Dyno tier 3 | yes | yes | yes (benchmark and sensor pages) | yes | yes (in the agent) | yes | `dyno.sensors`, `.cleanup`, `.actions` | no |
| Pitwall, Spotter, Pitcrew, Sighter | no | no | no | no | no | no | none (core plus declarative TOML packs) | no |

Dyno differs: its host is the always-on agent (service), not the UI, and its cleanup plugins only *propose*; Dyno's safety
policy keeps checking every path. Dyno adopts the platform when its tier 3 is built; until then its small copy stays (a
copy with one user does not justify a dependency edge into an unreleased crate).

## Consequences

- Debrief's release is untouched; the work starts after it.
- The shim is the only code that exists to serve the old contract and has a removal date.

## Open questions

Whether the shim should also cover third-party plugins built from the published ADR 0027 SDK (none known); proposal: no, they
recompile.

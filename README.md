<p align="center"><img src="icons/paddock/paddock-macos.svg?v=eecc3288" width="112" alt="Paddock icon: pit garages under a roof striped in the seven app colours"></p>

<h1 align="center">Paddock</h1>

<p align="center">
  <a href="docs/ecosystem.md"><img src="docs/images/paddock-ecosystem.svg?v=14fc4133" alt="Paddock: eight apps. Pacenotes, Debrief and Sighter above Pitwall, the local AI hub; Recce, Dyno and Spotter below it; Pitcrew under Dyno. Dash-dot lines marked MCP show what is planned for Pitwall 1.1: it calls the MCP servers of Pacenotes, Debrief, Sighter, Dyno and Recce, the family's retrieval engine. All are built on the shared Paddock foundation: the app shell every app uses and the workbench for the IDE-style apps" width="100%"></a>
</p>

<p align="center"><a href="docs/ecosystem.md">The ecosystem</a> · <a href="docs/ecosystem.md#integration-architecture">Integrations</a> · <a href="ui/README.md">UI kit</a> · <a href="docs/design-guide.md">Design guide</a> · <a href="icons/README.md">Icons</a></p>

Paddock is a family of eight desktop apps for people who keep software and the machines it runs on healthy, and the
foundation they share. Each app does one job well and is useful alone. Together they cover the loop from an incident in
production, through understanding the code and writing a test, to running it on CI. Everything runs on your computer: no
cloud, no account, no telemetry. Optional local AI, served by one app, makes each of them smarter without sending a byte
anywhere.

## The apps

Seven of the eight apps live in this repository under [`apps/`](apps/); Recce, the eighth, is planned and has only a placeholder page so far (plan: branch `docs/pc-search-app-plan`). The old per-app repositories are archived (read-only); their past releases stay downloadable there.

| App&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp; | What it does | Status |
|---|---|---|
| <img src="icons/debrief/small/debrief-48.svg?v=08378a3a" width="40" height="40" align="absmiddle" alt=""> **[Debrief](apps/debrief/README.md)** | An IDE for analysing logs and diagnostic files: one merged timeline across processes and machines. | **v1.1.1 released, 1.2.0 in progress** |
| <img src="icons/pitcrew/small/pitcrew-48.svg?v=fc4676ab" width="40" height="40" align="absmiddle" alt=""> **[Pitcrew](apps/pitcrew/README.md)** | Keeps self-hosted CI runners alive, updated and clean. | **v1.0.1 released** |
| <img src="icons/pitwall/small/pitwall-48.svg?v=5b597766" width="40" height="40" align="absmiddle" alt=""> **[Pitwall](apps/pitwall/README.md)** | The private, local AI hub: OpenAI-, Anthropic- and Ollama-compatible APIs over models on your machine. | **v1.0.3 released** |
| <img src="icons/dyno/small/dyno-48.svg?v=d7c3c328" width="40" height="40" align="absmiddle" alt=""> **[Dyno](apps/dyno/README.md)** | Watches your PC: live and history, alarms, a disk map and benchmarks. | **v1.0.0 released, 1.1.0 in progress** |
| <img src="icons/spotter/small/spotter-48.svg?v=cc97d9e4" width="40" height="40" align="absmiddle" alt=""> **[Spotter](apps/spotter/README.md)** | Watches for new releases anywhere and tells you the moment one appears. | In development, no release yet |
| <img src="icons/sighter/small/sighter-48.svg?v=232dd5be" width="40" height="40" align="absmiddle" alt=""> **[Sighter](apps/sighter/README.md)** | Finds any UI element by sight, so UI tests work even without an accessibility tree. | Planned |
| <img src="icons/pacenotes/small/pacenotes-48.svg?v=38a41979" width="40" height="40" align="absmiddle" alt=""> **[Pacenotes](apps/pacenotes/README.md)** | Understands code in any language: dependency graph, code review with your own rules, local semantic search, and an MCP server. | Planned |
| <img src="docs/images/src/icons/recce-placeholder.svg?v=927fe524" width="40" height="40" align="absmiddle" alt=""> **[Recce](docs/planned/recce/README.md)** | One search box for the whole PC: files, documents, email and SharePoint, by name or by meaning, with answers that cite their sources. | Planned (placeholder page, draft plan) |

Per-app detail, audience and standout features: [docs/ecosystem.md](docs/ecosystem.md).

## How they work together

**A production incident.** Support gets a zip of logs from three machines. In **Debrief** the merged timeline shows the
crash and the errors it caused elsewhere. Debrief asks **Pitwall** to explain the stack trace; Pitwall calls **Pacenotes**
over MCP, which maps the frames to files, callers and recent commits, so the explanation is grounded
in the real code. A regression test is drafted from it, **Sighter** turns the reproduction into a UI test, and **Pitcrew**
keeps the runner that executes it on CI healthy. Sighter's run bundle opens next to the logs in Debrief.

**A vendor release.** **Spotter** sees a new version of a tool you depend on. **Pitwall** summarises the release notes
locally. A Sighter smoke test checks that your workflow still works on the new build, and **Dyno** watches CPU and memory
for a regression over the following days. Anything odd opens in Debrief with one click.

**A slow build machine.** **Dyno** shows what the runner is doing, live and over time: an alarm fires, and the process list
and the disk map show that a background process is taking the runner's disk and CPU. The runner's health shows next to it
in **Pitcrew**; later Dyno can hand that evidence to **Pitwall** for an explanation.
Pitcrew's job logs open in **Debrief** when a build fails.

**No AI installed.** Nothing above stops working. Every app does its core job alone; AI features show "Needs local AI,
set up Pitwall" instead of an error. Install Pitwall later and they light up.

The wiring, protocols and diagram: [integration architecture](docs/ecosystem.md#integration-architecture).

## Principles

- **Local only and private.** No cloud, no telemetry. Nothing leaves your computer unless you ask for it.
- **Windows, macOS and Linux are all first-class**, with a native feel on each.
- **Rust engines, Angular UI, Tauri 2 shell.** The engine is the source of truth; the UI renders what it returns.
- **Pitwall is always optional.** Every app's core works without it; AI features appear when it is present.
- **Observe before acting.** Apps that change things (Pitcrew's cleanup, Sighter's actions) do so only on a click or an enabled rule, with an undo; Dyno only observes.
- **Calm by default.** Neutral surfaces, colour for action and the one thing that needs attention.

## Repository layout

```
apps/<app>/       one folder per app (Recce gets its folder when work starts), plus the shell and workbench demos
crates/           shared Rust crates (Gitea cargo registry)
ui/               @decryp7/paddock-ui and @decryp7/paddock-e2e
icons/            the isometric icon family, per app and platform
conformance/      the shared conformance checks every app runs
diagrams/         the diagram kit (style spec, draw.io library)
scripts/          repo tooling: release dry run, new-app generator, affected-only CI
docs/             rules, lessons, ADRs, design guide, ecosystem, images
.gitea/workflows/ CI and the per-app release workflows
```

## What is in this repo

| Part | Path | What | Consume it |
|---|---|---|---|
| UI package `@decryp7/paddock-ui` | [`ui/`](ui/README.md) | design tokens (dark and light, five accents, three densities, a colour-vision-safe chart palette), Angular components, ECharts theme, gallery | npm, Gitea registry |
| Design guide | [`docs/design-guide.md`](docs/design-guide.md) | principles, layout, type, colour, charts, components, motion, microcopy, polish checklist | read; `ui/` is its executable form |
| Diagram kit | [`diagrams/`](diagrams/README.md) | style spec, draw.io shape library, templates, canonical export script | draw.io library URL |
| Icon set | [`icons/`](icons/README.md) | the family app icons per platform and the tooling that builds them | copy the app's folder |
| Release practices | [`docs/lessons.md`](docs/lessons.md), [`CHANGELOG.md`](CHANGELOG.md) | tag workflow, signed updates, `latest.json`, the release script | follow the lessons |
| Rust crates | [`crates/`](crates/README.md) | shared Rust code. Planned: `paddock-mcp` (MCP servers across the family, ADR 0007), the plugin platform (host, IPC and API, extracted from Debrief's ADR 0017 model), `paddock-pitwall` (the Pitwall client) and `paddock-testplan` (the `.sighterplan` format) | Gitea cargo registry `decryp7` |
| Paddock Shell Demo | [`apps/shell-demo/`](apps/shell-demo/README.md) | the shell demo as an installable app on the `paddock-shell` plugin, to check the shared UI on every OS; the starter of new apps (`node scripts/new-app.mjs <id> "<Name>"`) | installers from a `shell-demo-v*` tag |
| Paddock Workbench Demo | [`apps/workbench-demo/`](apps/workbench-demo/README.md) | the workbench demo (dock, pop-out panels, assistant on Pitwall) as an installable app on the `paddock-shell` and `paddock-workbench` plugins; the starter of new IDE-style apps | installers from a `workbench-demo-v*` tag |
| Rules | [`docs/rules.md`](docs/rules.md) | the binding rules for every app and agent | follow them; each app's AGENTS.md carries them |
| Lessons | [`docs/lessons.md`](docs/lessons.md) | what building Debrief and Pitcrew taught | read before starting an app |

### Use the UI package

```ini
# .npmrc (public package, no token needed to install)
@decryp7:registry=https://dev.decryptology.net/api/packages/decryp7/npm/
```

```json
"@decryp7/paddock-ui": "^0.1.0"
```

`npm ci` installs it from `package-lock.json`. Tokens, components and migration notes: [`ui/README.md`](ui/README.md).

### Use the diagram kit

In draw.io choose File, Open Library from, URL, and paste:

```
https://dev.decryptology.net/decryp7/paddock/raw/branch/main/diagrams/paddock.xml
```

Then apply an app's accent with `node diagrams/tools/apply-accent.mjs <app> <template> <out>`; see
[`diagrams/README.md`](diagrams/README.md).

### Use a crate

The crates are published to the Gitea cargo registry `decryp7`; see [crates/README.md](crates/README.md) for the
`.cargo/config.toml` and `deny.toml` lines.

```toml
paddock-jobs = { version = "0.1", registry = "decryp7" }
```

## Status and roadmap

Released: Debrief 1.1.1, Dyno 1.0.0, Pitwall 1.0.3 and Pitcrew 1.0.1. Spotter, Pacenotes and Sighter have no release yet.
The order of work (2026-11-04):

1. Dyno 1.1.0, in progress: the administrator helper's readings, per-machine Windows installs and OS lifecycle checks.
2. Spotter 1.0.
3. Pitwall 1.1.
4. Pitcrew 1.1.
5. Debrief 1.2.
6. Pacenotes and Sighter.
7. Recce (planned; a placeholder page and a draft plan on `docs/pc-search-app-plan`).

Paddock itself is pre-1.0: a minor bump may change tokens or component APIs, and removed things are deleted rather than
deprecated.

## Versioning and releases

Every app releases from its own tag in this repository, `<app>-v<version>` (for example `debrief-v1.1.1`,
`pitwall-v1.0.3`); the tag's CI workflow (`.gitea/workflows/<app>.yml`) builds the installers and creates the release.
Releases are made only by CI, never from a local machine.

The shared packages use one semver tag for the whole repo (`vMAJOR.MINOR.PATCH`). Pushing a `v*` tag runs
`.gitea/workflows/publish.yml`, which runs the gates again, publishes the changed crates to the Gitea cargo registry and
the npm packages (`@decryp7/paddock-ui` with its entry points, `@decryp7/paddock-e2e`) to the Gitea npm registry, and
creates the release notes. Steps: update the version in `ui/projects/paddock-ui/package.json` and
`ui/projects/paddock-e2e/package.json` (they move together), bump each changed crate's `version`, update `CHANGELOG.md`,
commit, tag, push the tag. `node scripts/publish.mjs` (or `cd ui && npm run release`) is the local dry run: it lists every
crate, npm package and entry point with what the tag would publish, and uploads nothing.

The demos have their own tags: `shell-demo-v<version>` and `workbench-demo-v<version>` (built by
`.gitea/workflows/shell-demo-release.yml` and `workbench-demo-release.yml`). A `v*` tag never builds them, and their tags
never publish the packages.

## Licence

MIT OR Apache-2.0, at your option: [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE).

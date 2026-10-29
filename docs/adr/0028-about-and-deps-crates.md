# 28. `paddock-about` and `paddock-deps`: About and legal texts out of the updater; one dependency inventory for notices, SBOMs and Pacenotes

- Status: Proposed (2026-10-29). The user approved the direction on 2026-10-29: About becomes its own crate, and one Rust
  dependency inventory replaces the JS notices tool, cargo-about and the two CycloneDX tools. Open questions are at the end.
- Date: 2026-10-29
- Start: after the Dyno 1.0 and Spotter 1.0 tags. Shared code is frozen until then (rule 1).
- Related: rules 1, 9, 21, 39, 40, 42, 43; the licence policy (`templates/repo/deny.toml` `[licenses].allow`, root
  AGENTS.md "Dependencies and releases"); ADR 0012 §3 (the shell, About in plan S11); ADR 0014 §1, §6 (one npm workspace,
  per-app Cargo workspaces, affected CI); ADR 0021 (portable tests, xwin clippy); ADR 0022 §3.1, §3.6, §6, R6 (all tooling
  in Rust, golden files and a shadow week, the `cargo-metadata` feature that Pacenotes may not enable); ADR 0026 (dialog
  contracts declared in Rust); plan X42 (`docs/plans/shared-next.md`); `apps/pacenotes/docs/proposal.md` §14, §22 (the
  "Dependency and licence inventory (SBOM)" row) and line 26 (Pacenotes never executes repository code);
  `apps/pacenotes/docs/plans/2026-10-29-first-release-plan.md` §3.3, §4.1, VW1b
- Evidence: `origin/main` at `b618b5c97`; branch `feat/shared-notices` at `062587f80` (with `dyno/notices-1.0` and
  `spotter/notices-1.0` built on it); crate sources downloaded from crates.io on 2026-10-29 and read (listed under Sources)

## Context

### What exists today (verified on 2026-10-29)

- **About lives in the updater.** `crates/paddock-updater/src/about.rs` holds `About`, `AboutInfo`, `LegalDocInfo`,
  `AboutError`, `.legal(id, title, text)` and What's New (`whats_new`, `whats_new_on_start`). What's New reads the
  updater's `notes` module and writes `whats_new_seen` into the updater's `RecordStore` (`record.rs` line 27: "the
  updater and About share it"). The updater's command table (`lib.rs`) serves both the update commands and the four
  About commands (`about_info`, `about_legal`, `about_whats_new`, `about_whats_new_start`). The adapter
  (`src/tauri.rs`, `install`) builds About from `UpdatesBuilder::legal(...)` entries and sets
  `updates_available`. The crate's description names five jobs: update state machine, release information, signatures,
  install hand-off and service re-registration, and About and What's New.
- **The UI side** is `ui/projects/paddock-ui/shell/about/` (About, Updates and What's New dialogs, one store, the
  generated wire types from `paddock-updater` in `generated/`).
- **Who registers notices.** On `main` only Debrief ships `THIRD_PARTY_NOTICES.md` (15,966 lines: 768 Rust crates, 92 npm
  packages, 385 distinct licence texts), and it serves it through its own `apps/debrief/app/src-tauri/src/legal.rs`, not
  through the shared About. `dyno/notices-1.0` and `spotter/notices-1.0` add notices and register them with three
  `.legal(...)` calls on the shared About. Pitwall and Pitcrew have no notices and no legal texts at all.
- **The notices generator.** `conformance/src/notices/` (257 lines of JS on `main`; the shared version on
  `feat/shared-notices` reads the app's `notices` block from `paddock.ci.json`) plus `scripts/notices.sh` (installs
  cargo-about 0.9.2, runs `npm ci --ignore-scripts`, then the JS). Rust licences come from `cargo about generate`; npm
  licences from the root `package-lock.json` (the production closure of one workspace) and the licence files in
  `node_modules`. ADR 0022 §3.5 measured the CI step "notices are up to date" at 113 s, nearly all of it cargo-about
  resolving licences.
- **One licence list, three copies.** `templates/repo/deny.toml` `[licenses].allow`, `templates/repo/about.toml`
  `accepted` and `licences.mjs` `ALLOWED` are kept equal by a test. `licences.mjs` adds `NPM_EXTRA` (`OFL-1.1`,
  `Python-2.0`, `BlueOak-1.0.0`, `CC-BY-4.0`), with one comment as the only reason.
- **SBOMs.** Only Debrief makes them (`.gitea/workflows/debrief.yml`, step "SBOMs (CycloneDX)"): `cargo cyclonedx` for
  the app's Rust manifest and `npx --yes @cyclonedx/cyclonedx-npm` for the UI, two files per release.
  `npx --yes` downloads an unpinned version at release time. Dyno, Spotter, Pitwall and Pitcrew release through
  `app-release.yml`, which makes no SBOM.
- **What Pacenotes needs.** Its proposal (§22) lists "Dependency and licence inventory (SBOM)": external libraries from
  manifests and lockfiles with versions and licences, inventory in v1, licence resolution and CycloneDX/SPDX export in
  v1.1. Its hard rule (proposal line 26) is that it never executes repository code, so nothing may run `cargo`, `npm`,
  `dotnet`, `pip`, `go` or `mvn` on a repository it analyses. ADR 0022 §6 already keeps `paddock-repo-graph`'s
  `cargo-metadata` adapter behind a feature that Pacenotes may not enable.

### Problems

1. **The updater has two responsibilities.** Changing a legal document or What's New touches the crate that verifies
   signatures and installs updates, and an app cannot show About without pulling in the updater.
2. **The licence check is wrong for `AND`, and the npm exception is not scoped to npm.** `nonPermissive` in
   `generate.mjs` accepts a package when *any* identifier of its expression is allowed. Measured by running
   `nonPermissive` from `062587f80` on synthetic expressions on 2026-10-29 (DP1's first tests repeat these rows):

   | Expression | JS result | Correct |
   |---|---|---|
   | `MIT AND GPL-3.0-only` | accepted | refused |
   | `(MIT OR Apache-2.0) AND AGPL-3.0-only` | accepted | refused |
   | `MIT OR GPL-3.0-only` | accepted | accepted |
   | `GPL-3.0-only` | refused | refused |
   | `CC-BY-4.0` (Rust crate) | accepted | refused (npm only) |

   For Rust crates cargo-about and `cargo deny` catch the first two today. For npm packages nothing else does.
3. **Four tools, three languages, one question.** The JS tool, cargo-about, cargo-cyclonedx and cyclonedx-npm each
   resolve the same dependency graph differently. ADR 0022 decided that tooling moves to Rust (its R6 includes notices).
4. **SBOMs are neither reproducible nor universal.** They exist for one app, come from an unpinned download and have no
   test.
5. **Pacenotes would otherwise write a second inventory** (rule 42) for foreign repositories, under a stricter rule.

## Decision

Two new shared crates. `paddock-about` owns what the About window shows. `paddock-deps` owns the dependency inventory and
everything derived from it. `paddock-dev` (ADR 0022) is the only program that runs the inventory in our own repository.

### 1. `paddock-about`

**Scope.** It takes from `paddock-updater`:

- the About model (`About`, `AboutInfo`, `LegalDocInfo`, `AboutError`, `.build`, `.homepage`, `.releases_page`, `.legal`,
  `.legal_text`);
- What's New (`notes` parsing of the app's `CHANGELOG.md`, `WhatsNew`, `WhatsNewDecision`, `whats_new_on_start`);
- the four About commands and their generated TypeScript (`ui/projects/paddock-ui/shell/about/generated/about.ts` and
  `about-commands.ts`), regenerated from the new crate.

The updater keeps the state machine, the manifest, signatures, the install hand-off and the service hooks, and its
description shrinks to that. Its `RecordStore` loses `whats_new_seen`.

**What's New gets its own small record.** `paddock-about` owns a `SeenVersion` port (read and write one version string)
with a file adapter (`about.json` in the app's config folder, a `paddock-env` versioned file). Losing the old
`whats_new_seen` value has no visible effect: the first start after the upgrade is a new version anyway, so What's New
opens as it would have. No migration code (no compatibility shims before v1).

**Dependencies.**

```
paddock-about        (pure core; feature `tauri`: managed state, commands, the adapter)
   ^          ^
   |          |
paddock-shell  (wires About and the updater; sets `updates_available` from the updater)
   |
paddock-updater (no dependency on paddock-about)
```

`paddock-updater` does not depend on `paddock-about`. The only fact About took from the updater is "this build can
update itself". `paddock-shell`, which already depends on the updater, gains `paddock-about` and sets it:
`About::updates_available(updater.available())`.
`paddock-about`'s dependencies are `paddock-commands`, `paddock-dialogs` (the About dialog contract, ADR 0026),
`paddock-files` (the releases link, as today), `paddock-env`, `serde`, and `tauri` behind its feature.

**The notices macro.** One macro embeds the app's generated notices and nothing else is needed to register them:

```rust
let about = About::new("Dyno", version, seen)
    .legal(paddock_about::notices!())?           // resources/THIRD_PARTY_NOTICES.md of the calling crate
    .legal(paddock_about::licence!(Mit))?        // the family's LICENSE-MIT, embedded once in paddock-about
    .legal(paddock_about::licence!(Apache))?;
```

- `notices!()` expands to a `LegalDoc` with id `notices`, title "Third-party notices" and
  `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/THIRD_PARTY_NOTICES.md"))`. A missing file fails the
  build with rustc's "couldn't read" error, naming the path. `notices!("path")` takes another path relative to the
  manifest.
- The macro also checks the file's first line at compile time (a `const` assertion that it starts with
  `# Third-party notices`), so an empty or hand-written placeholder fails the build too.
- `licence!(Mit | Apache)` embeds the family's licence texts once, from `paddock-about` itself. Every app's
  `LICENSE-MIT` and `LICENSE-APACHE` are identical today (plan X27); conformance C28 (the repo baseline) keeps them
  identical.
- `LegalDoc` is a typed value (id newtype checked at construction, so `.legal` cannot be given a bad id from the macro).
  `.legal(id, title, text)` with strings stays for other documents.

**Checks over every app (rule 40).** A conformance check fails when an app that registers About does not register
`notices!()`, or when its `paddock.ci.json` has no `notices` block. One test covers every app, not one test per app.

**Migration of Debrief, Dyno, Spotter, Pitwall and Pitcrew in one change.** No compatibility shim:

- Debrief deletes `legal.rs` and its `legal_text` command and uses the shared About through the shell.
- Dyno and Spotter replace their three `.legal(...)` calls with the macros.
- Pitwall and Pitcrew get a `notices` block, a generated `THIRD_PARTY_NOTICES.md` and the macro registration. Neither is on
  the shell on `main` yet; see open question 3 for how they show it until they are.
- `paddock-updater`'s `UpdatesBuilder::legal`, `.changelog` and the About half of `install` go; the shell's builder takes
  them.

### 2. `paddock-deps`

A pure core with adapters around it. The core parses, evaluates and writes; it never runs a program, opens a socket or
walks a disk on its own.

**The inventory (typed, parsed once at the edge).**

```rust
pub struct Inventory { pub root: Component, pub packages: Vec<Package>, pub edges: Vec<Edge> }
pub struct Package {
    pub id: PackageId,                 // ecosystem + name + version + source, unique in the inventory
    pub ecosystem: Ecosystem,          // Cargo | Npm | NuGet | PyPI | Go | Maven  (one enum, extended by adding)
    pub name: String, pub version: String,
    pub source: Source,                // Registry(url) | Git { url, rev } | Path | Workspace
    pub licence: Licence,              // Expression(spdx::Expression) | NonSpdx(String) | Unknown
    pub texts: Vec<LicenceText>,       // { id: Option<LicenceId>, text, origin: File(path) | Canonical }
    pub reach: Reach,                  // Direct | Transitive
    pub scope: Scope,                  // Production | Build | Dev   (Optional for npm optional dependencies)
    pub hashes: Vec<Hash>,             // Cargo.lock checksum (SHA-256), npm integrity (SHA-512)
    pub confidence: Confidence,        // Exact (lockfile, resolved metadata) | Declared (manifest range only)
}
```

Workspace members (our own crates and packages) are `Source::Workspace`: notices skip them, as today; the SBOM lists them
as first-party components.

**Ports for ecosystems.** An `EcosystemReader` trait (`fn read(&self, files: &dyn FileSource, limits: &Limits) ->
Result<Partial, DepsError>`) per ecosystem, registered in one table. A new ecosystem is a new implementation (open/closed).
Built in this ADR:

| Reader | Input | Runs a program | Used by |
|---|---|---|---|
| `cargo::Metadata` | `cargo metadata --format-version 1 --locked --offline` JSON, all targets | yes, through an injected `CargoMetadata` port; the adapter (`paddock-proc`, timeout, output cap) is behind the feature `cargo-metadata` | `paddock-dev` only |
| `cargo::Lockfile` | `Cargo.lock` (and `Cargo.toml` for direct dependencies) | no | Pacenotes |
| `npm::Lockfile` | `package-lock.json` v2/v3: a workspace's production closure or the whole lock; `license` from the lock | no | both |
| `nuget`, `pypi`, `go`, `maven` | see §4 | no | Pacenotes (stage 2) |

The npm closure already exists twice (the JS tool's production closure; `paddock-repo-graph`'s `lock.rs`, which follows
dev dependencies too for change detection). The typed lock model moves into `paddock-repo-graph` with a `Scope` argument
(`Production` or `All`); `lock_diff` uses `All`, `paddock-deps` uses `Production`. One lock parser, tested once (rule 43).
`paddock-deps` depends on `paddock-repo-graph` with its features off. Likewise its `CargoMetadata` port is the one
`paddock-repo-graph` already declares, widened to take the arguments (with or without `--no-deps`); there is one cargo
adapter in the family.

**Licence texts** come from a `LicenceTexts` port. Its adapter (feature `licence-files`) reads candidate files from a
package's own folder (crate sources from `cargo metadata`'s `manifest_path`, npm packages from `node_modules`). It
detects each file's licence with the `spdx` crate's `detection` (threshold 0.8, the value cargo-about 0.9.3 uses,
`src/licenses.rs` line 110). When a chosen licence has no matching file, it falls back to the canonical text from
`spdx`'s `text` feature, as cargo-about does (`src/generate.rs` lines 141–154). The feature carries about 6 MB of SPDX texts and a
2 MB detection cache (measured in `spdx` 0.13.6's sources), so only `paddock-dev` enables it; apps embed only their
generated notices.

**Licence policy.** One file, `templates/repo/licences.toml`, replaces `about.toml` and `licences.mjs`:

```toml
allow = ["MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
         "0BSD", "CC0-1.0", "BSL-1.0", "Unicode-3.0", "Unlicense", "MPL-2.0", "CDLA-Permissive-2.0"]

[[exception]]
licence = "CC-BY-4.0"
ecosystems = ["npm"]
reason = "Build-time data (caniuse-lite); its attribution is carried by the notices."
approved = "2026-10-29, the user"
```

- **Evaluation** is SPDX semantics through `spdx::Expression::evaluate`: `OR` needs one allowed branch, `AND` needs all,
  `WITH` needs the exception named in the allow entry. The first two rows of the table above become failing tests.
- **Parsing is lax for spellings found in the wild, strict about meaning.** `spdx::ParseMode::LAX` accepts `/` as `OR`,
  imprecise names and deprecated identifiers. `MPL-2.0+` is "MPL-2.0 or later" and satisfies `MPL-2.0`; a deprecated
  `GPL-2.0` is read as `GPL-2.0-only` and refused. Every normalisation is recorded on the package, so the report says
  what was read and what it was taken to mean.
- **Exceptions are scoped and reasoned.** Each `[[exception]]` names its licence, its ecosystems and a reason; one without
  a reason does not load. A package accepted only through an exception says so in the report. Today's `NPM_EXTRA` entries
  become exceptions; only CC-BY-4.0's approval is on record (open question 4).
- **A test keeps `deny.toml`'s allow list equal to `licences.toml`'s** (cargo deny stays the gate for advisories,
  sources and bans, and stays a second check on licences).

**Outputs (writers, pure functions of the inventory).**

1. **`THIRD_PARTY_NOTICES.md`**, equal byte for byte to today's output apart from the header line that names the
   generator. That means the same order (kind, then name, then version, as plain string comparison), the same text
   normalisation (line endings to LF, trailing blanks per line and outer blank lines removed), texts de-duplicated by
   SHA-256 and labelled `<id>-<n>` in (id, hash) order, the "Used by" wrap at 96 columns, the npm text ids guessed from
   the file name, and cargo-about's choice of licences for an `OR` expression (the first branch the accepted list
   satisfies: `MIT OR Apache-2.0` lists only the MIT text, as Debrief's notices show for `anyhow` and `serde`).
2. **CycloneDX 1.6 JSON**, one file per app per release (`<app>-<version>.cdx.json`, both ecosystems in one BOM):
   `metadata.component` is the app with its licence; components carry `purl` (`pkg:cargo/…`, `pkg:npm/…`), `bom-ref`,
   `scope` (`required`, or `optional` for npm optional dependencies; dev dependencies are left out), `hashes` and
   `licenses` (an `expression`, or a `license.name` for a non-SPDX string); `dependencies` carries the graph. It is
   reproducible: `serialNumber` is a UUID v5 of the app, its version and the lockfiles' hashes, and `timestamp` is the
   release commit's time. A test validates the output against the official 1.6 JSON schema (vendored, Apache-2.0) and a
   golden file. SPDX output is later (an added writer, nothing else changes).
3. **Inventory data** for Pacenotes: the typed `Inventory` (serde, with generated TypeScript for its views) plus the
   policy result per package.

**Bounds and errors.** Every input has a limit, with the value in the error (ADR 0019): lockfile and metadata size,
package count, edges, licence files per package, licence file size, expression length and nesting depth. The values are
set in DP2/DP3 from the largest app's measured inputs with headroom (recorded in `docs/benchmarks.md`, rule 48), not
guessed here. Errors are a typed `DepsError` (`Parse { path, reason }`, `Limit { what, limit }`, `Workspace(name)`,
`Tool { program, reason }`, `Missing { what, next_step }`). Policy failures are findings in the report, not errors.
Parsers never panic on any bytes (a `paddock-testkit` never-panics corpus per reader; fuzz targets on `app-fuzz.yml`).

**No network by default.** The core has none. Fetching a missing licence text is the feature `fetch`, off by default,
used only by an explicit flag (`--fetch-licences`). It goes through `paddock-net`'s `Egress` with a host allow-list and
`set_offline`, never during a release build (the release must be reproducible from the lockfiles and sources).

### 3. Library choices (researched 2026-10-29)

Facts from the crates.io API and the published sources unless marked otherwise. "Recent downloads" is crates.io's 90-day
count.

| Need | Choice | Licence | Maintenance | Why, and the rejected alternatives |
|---|---|---|---|---|
| SPDX expressions, evaluation, deprecated spellings, canonical texts, licence detection | **`spdx` 0.13.x** (EmbarkStudios) | Apache-2.0 (0.13.6's manifest) | 0.13.6 published 2026-10-02; ~7.0 M recent downloads; used by cargo-deny and cargo-about | It is what cargo-about and cargo-deny use, so our evaluation and detection agree with both. It has `ParseMode::LAX` (slash, imprecise names, `+` on GPL, deprecated ids; `src/lexer.rs`), the SPDX list 3.29.0, `text` and `detection` features. **Pin the minor version** to the one cargo-about uses: detection scores decide which files count, and a new store can change the notices. Writing our own: no, the SPDX grammar and the licence list are exactly what a shared crate should not re-implement. |
| `cargo metadata` types | **Own serde structs** for the ~10 fields used | ours | – | `paddock-repo-graph` already parses this JSON with `serde_json`; ids are used as opaque keys, so the PackageId format change does not matter. Rejected: `cargo_metadata` 0.23.1 (MIT, 2025-11-12, ~65 M recent downloads, well maintained) would add `derive_builder` and `cargo-platform` and a 0.x API that breaks every few releases for a dozen fields. Fallback if the subset grows: adopt it. |
| `Cargo.lock` without running cargo (Pacenotes) | **`cargo-lock` 11.x** (RustSec) | Apache-2.0 OR MIT | 11.1.0 published 2026-07-24; ~7.7 M recent downloads; part of the RustSec project | Parses every lockfile version, including dependency lists and checksums, without cargo. Writing our own is possible (TOML) but the format has three versions and RustSec maintains them. |
| CycloneDX 1.6 JSON | **Own writer** (serde structs for the subset we emit) | ours; schema Apache-2.0 | – | `cyclonedx-bom` 0.8.1 (Apache-2.0, published 2026-03-19) supports spec 1.3–1.5 only (`src/models/bom.rs`, `SpecVersion`; read 2026-10-29). Support for 1.6 and 1.7 was merged upstream on 2026-10-08 (PR #872), but no release has it yet. `serde-cyclonedx` 0.10.0 (MIT, 2025-06-17) is generated from schemas and small. We emit about fifteen fields; a writer of our own with a schema test is smaller than either dependency. Revisit `cyclonedx-bom` when a release with 1.6 ships (it would also give us 1.7 and validation). |
| Licence files and detection | **Our scan on `spdx` detection**, matching cargo-about's rules | ours | – | See "Risks": reproducing cargo-about's choices is the hard part. Rejected for production use: the `cargo_about` library (MIT OR Apache-2.0, 0.9.3 published 2026-10-26; it has a `lib` target). It links `ureq`, `rustls` and `ring` unconditionally, its library API is the binary's internals (no stability promise), and it brings `krates`. It stays the **oracle**: the golden and shadow comparisons run today's cargo-about against our output. Fallback if DP3 cannot close the differences in its budget: use `cargo_about::licenses::Gatherer` for Rust crates behind the `licence-files` feature. |
| XML (Maven, NuGet `.csproj`) | **`roxmltree`** | MIT OR Apache-2.0 | 0.21.2 published 2026-10-24; already in the family's lockfiles (0.20.0) | Read-only, no entity expansion of external DTDs; refuse documents with a DTD (to be confirmed in PN1 by a billion-laughs test). |
| Schema test | **`jsonschema`**, dev-dependency only, default features off | MIT | 0.60.2 published 2026-10-26; already in the family's lockfiles (0.33.0) | Validates the CycloneDX output in tests. |
| UUID v5 for `serialNumber` | **`uuid`** (`v5`) | Apache-2.0 OR MIT | 1.29.0 published 2026-10-23; already in the family | – |

Every crate above is on the allow list. Transitive licences are checked by `cargo deny` in DP1 (one unverified item: the
`zstd` crate behind `spdx`'s detection cache reports BSD-3-Clause for its latest version on crates.io; its bundled C
library is dual BSD-3-Clause OR GPL-2.0, which the allow list's BSD entry satisfies; DP1 confirms with `cargo deny`).

### 4. Integration

- **`paddock-dev`** gets `./dev deps notices <app> [--check]`, `./dev deps sbom <app> --out <dir>` and
  `./dev deps inventory <app> --json`, reading the app's `notices` block in `paddock.ci.json` (the fields of
  `feat/shared-notices`: `name`, `manifests`, `npm`, `out`; `about` and `rustJson` go). This is the notices part of ADR
  0022's R6, which shrinks by it. The tool runs `cargo metadata --locked --offline`; when sources are missing it fails with
  "run `cargo fetch --locked`", and when `node_modules` is missing with "run `npm ci --ignore-scripts`".
- **Deleted in the same change** (ADR 0022 §3.7, the baseline shrinks): `scripts/notices.sh`,
  `conformance/src/notices/`, `conformance/bin/paddock-notices.mjs`, their tests, `templates/repo/about.toml` and the
  cargo-about install.
- **The conformance check that notices are current stays,** as the `app-gates` step `./dev deps notices <app> --check`
  on the Linux leg (where `feat/shared-notices` put the JS check), plus the every-app check of §1.
- **SBOMs for every app.** `app-release.yml` (Dyno, Spotter, Pitwall, Pitcrew, the demos) and `debrief.yml` run
  `./dev deps sbom` and attach `<app>-<version>.cdx.json` as a release asset. Debrief's `cargo cyclonedx` and `npx --yes
  @cyclonedx/cyclonedx-npm` steps are removed, and with them the unpinned download.
- **Shadow week** (ADR 0022 §3.6): for one week on `main`, the JS tool and `./dev deps notices` both run for every app and a
  step compares them. A difference is a warning with both outputs and is reviewed, never accepted silently.

### 5. Pacenotes (stage 2, after Debrief 1.2)

Pacenotes adopts `paddock-deps` for its dependency inventory view (plan VW1b; proposal §22). It needs more than releases do:

1. **Foreign repositories.** It reads whatever repository the user opened, in any layout, so readers find manifests and
   lockfiles through `paddock-repo-files`' walk within the workspace allow-list (canonical paths, no symlink escape).
   Releases only read our own declared manifests.
2. **More ecosystems.** Readers on the same port, lockfile first, manifest as a fallback with `Confidence::Declared`:
   - NuGet: `packages.lock.json`; else `PackageReference` in `*.csproj` and `Directory.Packages.props` (central package
     management).
   - PyPI: `uv.lock`, `poetry.lock`, `Pipfile.lock`; else `requirements*.txt` (pinned `==` lines are exact, others
     declared) and `pyproject.toml`.
   - Go: `go.mod` with `go.sum` (`// indirect` marks transitive).
   - Maven: `pom.xml` dependencies with properties from the same file only. Parent POMs, imported BOMs and version ranges
     are not resolved: those packages are `Declared` with the reason. `gradle.lockfile` when present.
   - npm also gets `yarn.lock` and `pnpm-lock.yaml` readers (later, by demand).
3. **The untrusted-repository rule: no program ever resolves dependencies.** Pacenotes never runs cargo, npm, dotnet, pip,
   go, mvn or gradle on an analysed repository (they read repository configuration that can name programs to run, and
   build scripts run code). Only the lockfile and manifest readers are used. ADR 0022's planned check, which fails when a
   manifest under `apps/pacenotes/` enables `paddock-repo-graph`'s `cargo-metadata`, extends to `paddock-deps`'
   `cargo-metadata` and `fetch` features.
4. **Licences without a build.** In v1 the inventory shows the licence when the lockfile carries it (npm), and "not known
   offline" otherwise, never a guess. Licence resolution (v1.1 in the proposal) reads package caches already on the
   machine, read-only and only with the user's opt-in, because they are outside the workspace allow-list: the Cargo
   registry, the NuGet global packages folder, the Go module cache and the local Maven repository. A network lookup is a
   separate opt-in through `Egress` (open question 5). Licence detection runs in a worker under `paddock-guard`, with the
   limits above.
5. **Export.** CycloneDX 1.6 from the same writer (v1.1), with Pacenotes' tier and confidence labels kept as CycloneDX
   properties.

## Phases and work packages

Estimates are agent-days for an implementer; DP3 and PN1 start with a researcher. Every package starts with its failing
tests, lands on its own, and passes the gates named. The common gates for crate work are `cargo fmt --check`, clippy with
`-D warnings` (and xwin clippy, ADR 0021), nextest, `cargo llvm-cov` at 99 % lines or more for the crate, `cargo hakari
generate --diff`/`verify`, `node scripts/publish.test.mjs` and `cargo deny check`.

| Phase | WP | Content | Failing tests first | Gates beyond the common ones | Days |
|---|---|---|---|---|---|
| **1 About** (after the Dyno 1.0 and Spotter 1.0 tags, outside the Pitwall 1.1 freeze; open question 1) | AB1 | `crates/paddock-about` (new-crate checklist): About, legal documents, What's New with `SeenVersion`, `notices!()` and `licence!()`, command table and `tauri` adapter, generated TS; `paddock-updater` loses About; `paddock-shell` wires both | a `compile_fail` doc test for a missing notices file and one for a placeholder file; `.legal` refuses a bad id; What's New never shows another version's notes (moved test); the updater's command table no longer lists About commands | the shell's and the About dialog's Playwright specs (Chromium); the generated-TS sync test | 2 |
| | AB2 | Migrate Debrief, Dyno, Spotter, Pitwall, Pitcrew in one change; delete Debrief's `legal.rs`; notices blocks and generated notices for Pitwall and Pitcrew; the every-app conformance check | the every-app check fails on `main`'s Pitwall and Pitcrew before the change; Debrief's About test lists "Third-party notices" through the shared command | each app's affected gates (`scripts/affected.mjs`) and one real-app smoke of Debrief | 2 |
| **2 Inventory core** (new crate only; can run beside a release once its `crates/Cargo.toml` and hakari lines land between releases) | DP1 | `crates/paddock-deps` core: `Inventory`, `Ecosystem`, `licences.toml` policy, evaluation on `spdx`, scoped exceptions with reasons, the deny-list equality test | the five rows of the table in Problems 2; `MPL-2.0+` satisfies `MPL-2.0`; `GPL-2.0` (deprecated) is refused; an exception without a reason does not load | `cargo deny` over the new dependencies | 1.5 |
| | DP2 | npm: the typed lock model moved into `paddock-repo-graph` with `Scope`, the production closure, `license` from the lock, licence files from `node_modules` | golden: the JS tool's npm half on synthetic lock fixtures (workspaces, nested `node_modules`, links, optional peers); `lock_diff`'s existing tests stay green | `paddock-repo-graph`'s tests and coverage | 1.5 |
| | DP3 | Cargo: the widened `CargoMetadata` port and adapter (`cargo-metadata` feature), production closure (normal dependencies, all targets, no build or dev), licence-file scan with `spdx` detection, canonical fallback, accepted-order choice; `cargo::Lockfile` on `cargo-lock` | golden: cargo-about 0.9.x JSON on synthetic crates (dual licences, a crate without a licence file, a crate with two MIT files from different holders, an `AND` expression, a `LicenseRef`); never-panics on lockfile bytes | a local comparison against today's notices of Debrief, Dyno and Spotter (no difference except reviewed ones) | 3 |
| | DP4 | Writers: notices (byte for byte but the header), CycloneDX 1.6 (schema test, reproducible serial number and timestamp) | golden notices from the JS `render` on the DP2/DP3 fixtures; a BOM made twice from the same inputs is identical; the schema test fails on a BOM missing `specVersion` | – | 2 |
| **3 Integration** (ADR 0022 R6, notices part) | IN1 | `./dev deps notices|sbom|inventory`; the `app-gates` step; shadow week; then delete the JS tool, `scripts/notices.sh`, `about.toml` and the cargo-about install; record the step's time before and after in `docs/benchmarks.md` (rule 48) | `--check` fails on a stale file and on a refused licence; a missing `node_modules` gives the next-step message | the shadow week ends with no unexplained difference; ADR 0022 §3.7 baseline shrinks | 1.5 |
| | IN2 | SBOM in `app-release.yml` and `debrief.yml` as release assets; remove the cargo-cyclonedx and cyclonedx-npm steps | a `workflow_dispatch` dry run of `app-release.yml` lists `<app>-<version>.cdx.json` | dry run per app (ADR 0022 phase 2 acceptance) | 1 |
| **4 Pacenotes** (stage 2, after Debrief 1.2) | PN1 | NuGet, PyPI, Go and Maven readers; workspace discovery through `paddock-repo-files`; the extended feature check for `apps/pacenotes/` | per reader: golden inventories on synthetic repositories; never-panics corpus; a pom with a DTD is refused; the feature check fails on a planted `cargo-metadata` | Pacenotes' F1 port checks; fuzz targets added to `app-fuzz.yml` | 4 |
| | PN2 | Offline licence resolution from the opt-in package caches; detection in a guarded worker; CycloneDX export with Pacenotes' labels | a package with no cached sources says "not known offline"; a cache outside the opt-in is never read (fake file source records every read) | – | 2 |
| | | **Total** | | | **about 20.5** (14.5 for releases, 6 for Pacenotes) |

## Risks

| Risk | Mitigation |
|---|---|
| Our licence-file choices differ from cargo-about's, so the notices are not byte for byte the same | cargo-about stays the oracle in DP3's golden tests and in the shadow week; `spdx` is pinned to cargo-about's minor version and the threshold to 0.8; fallback: the `cargo_about` library behind `licence-files` (§3) |
| A new `spdx` release changes detection and so the notices | pinned minor; Renovate's update of `spdx` regenerates every app's notices in the same PR, reviewed as a diff |
| Phase 1 changes `paddock-updater` and `paddock-shell`, which Pitwall 1.1 and Debrief 1.2 also change | land AB1–AB2 in the window after the Dyno and Spotter tags and before Pitwall 1.1's freeze, or after its tag (open question 1); rule 1 keeps tags serialised |
| A new crate's hakari line changes feature unification for every crate | land the `crates/Cargo.toml` member and hakari change between releases, as their own small PR |
| Pitwall and Pitcrew are not on the shell, so they have no About to register notices in | open question 3 |
| The SBOM omits something a consumer expects (build tools, the WebView runtime, bundled binaries such as libduckdb) | the BOM documents what it covers in `metadata.properties`; bundled binaries are added from `paddock-runtime`'s pin catalogues in a later WP (open question 6) |
| Hostile manifests in Pacenotes (huge files, deep nesting, XML entities) | limits on every input, never-panics corpora, fuzzing, DTDs refused, readers run in a guarded worker |
| Licence text fetching leaks dependency names | off by default, explicit flag or click, through `Egress`; never in release builds |

## Consequences

- `paddock-updater` does one job; About can be used without it.
- One mechanism registers notices, and a missing or placeholder file fails the build.
- One Rust implementation answers "what do we ship and under which licence" for notices, SBOMs and Pacenotes. The JS tool,
  `scripts/notices.sh`, `about.toml`, cargo-about, cargo-cyclonedx and cyclonedx-npm leave the release path.
- The licence check gets correct `AND` semantics and scoped, reasoned exceptions; two expressions the JS check accepts
  today will be refused.
- Every app release carries a reproducible CycloneDX 1.6 SBOM.
- Pacenotes starts its inventory on a tested core and adds only readers.

## Open questions for the user

1. **When does phase 1 land?** It changes `paddock-updater` and `paddock-shell`, which Pitwall 1.1 and Debrief 1.2 depend
   on. Proposal: right after the Dyno and Spotter tags, before Pitwall 1.1's freeze, so Pitwall 1.1 ships on it.
   Otherwise after the Pitwall 1.1 tag.
2. **Phase 2 before or with ADR 0022's R6?** Proposal: DP1–DP4 as soon as the crate's member line can land between
   releases; IN1 replaces R6's notices part.
3. **Pitwall and Pitcrew before they move to the shell.** Proposal: they get the notices file, the generated SBOM and the
   conformance check now, and register `notices!()` when they adopt the shell (Pitwall in 1.1, Pitcrew when it resumes).
   The alternative is a small About of their own until then, which is a copy we would delete.
4. **The other npm-only exceptions.** CC-BY-4.0 (npm only) is approved. `OFL-1.1` (fonts), `Python-2.0` and
   `BlueOak-1.0.0` are in `NPM_EXTRA` today without a recorded approval. Keep them as npm-only exceptions with these
   reasons?
5. **Network licence lookups in Pacenotes.** Allow an opt-in lookup (for example crates.io, the npm registry, NuGet,
   deps.dev) through `Egress`, or stay strictly offline with only the opt-in local caches?
6. **CycloneDX version and scope.** 1.6 as briefed (1.7 exists, but no Rust library releases 1.6 yet), one combined BOM per
   app, dev dependencies left out. Should bundled binaries (libduckdb, FFmpeg, llama.cpp from `paddock-runtime` pins)
   and the first-party `paddock-*` crates be components too? Proposal: yes for both, first-party ones without hashes.

## Sources

Verified (read or run on 2026-10-29):

- crates.io API (`https://crates.io/api/v1/crates/<name>`) for licence, latest version, date and downloads of `spdx`,
  `cargo_metadata`, `cargo-lock`, `cyclonedx-bom`, `serde-cyclonedx`, `cargo-about`, `krates`, `roxmltree`,
  `jsonschema`, `uuid`, `zstd`, `askalono`, `packageurl`, `purl`.
- Sources from `https://static.crates.io/crates/<name>/<name>-<version>.crate`: `spdx` 0.13.6 (`Cargo.toml` features,
  `src/lexer.rs` `ParseMode`, `src/identifiers.rs` list 3.29.0, `src/detection/`), `cyclonedx-bom` 0.8.1
  (`src/models/bom.rs` `SpecVersion` 1.3–1.5), `cargo-about` 0.9.3 (`[lib]` target; `src/licenses.rs` threshold 0.8;
  `src/generate.rs` canonical-text fallback; unconditional `ureq`/`rustls`/`ring` dependencies), `cargo_metadata` 0.23.1
  (dependencies).
- The JS licence check probed with synthetic expressions (Problems 2), from `062587f80:conformance/src/notices/`.

Read, not verified further:

- CycloneDX specification releases (1.6 and 1.7): <https://github.com/CycloneDX/specification/releases>
- `cyclonedx-rust-cargo` issue #769 and PR #872 (1.6 and 1.7 support merged 2026-10-08, unreleased):
  <https://github.com/CycloneDX/cyclonedx-rust-cargo/pull/872>
- SPDX license expressions: <https://spdx.github.io/spdx-spec/v2.3/SPDX-license-expressions/>
- npm lockfile v2/v3: <https://docs.npmjs.com/cli/v10/configuring-npm/package-lock-json>
- Package URL specification: <https://github.com/package-url/purl-spec>
- `roxmltree` refusing DTDs by default: <https://docs.rs/roxmltree/latest/roxmltree/struct.ParsingOptions.html>
  (to confirm in PN1)

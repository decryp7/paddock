# 28. `paddock-about` and `paddock-deps`: About and legal texts out of the updater; one dependency inventory for notices, SBOMs and Pacenotes

- Status: Accepted (2026-10-29), by the user, after an independent review (rounds 1–4). The user approved the direction on 2026-10-29: About becomes its own crate, and one Rust
  dependency inventory replaces the JS notices tool, cargo-about and the two CycloneDX tools. Revised the same day after
  an independent review (five P1s: What's New for existing users, Debrief not on the shell, every app green per step, a
  fail-closed licence policy, owned paths per package). The user decided the last two open questions the same day
  (Decided, at the end); none remain.
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
  updater's `notes` module and writes `whats_new_seen` (`record.rs` line 27) into the updater's `RecordStore`, the file
  `updates.json` in the app's config folder (line 12; line 67: "the updater and About share it"). `notes::decide`
  (`notes.rs` lines 71–77) treats a missing `seen` as a first run: it only records the version and shows nothing. The
  updater's command table (`lib.rs`) serves both the update commands and the four
  About commands (`about_info`, `about_legal`, `about_whats_new`, `about_whats_new_start`). The adapter
  (`src/tauri.rs`, `install`) builds About from `UpdatesBuilder::legal(...)` entries and sets
  `updates_available`. The crate's description names five jobs: update state machine, release information, signatures,
  install hand-off and service re-registration, and About and What's New.
- **The UI side** is `ui/projects/paddock-ui/shell/about/` (About, Updates and What's New dialogs, one store, the
  generated files from `paddock-updater` in `generated/`: `about.ts`, `about-commands.ts` and `dialog-contracts.ts`).
- **Who uses the shared About.** On `main`, `shell-demo` (`src-tauri/src/lib.rs` lines 301–306, two `.legal(...)`
  calls) and `workbench-demo` use it through the shell. `dyno/shell-1.0` and `spotter/shell-1.0` move Dyno and Spotter
  onto the shell, and `dyno/notices-1.0` and `spotter/notices-1.0` register three legal documents there.
- **Debrief is not on the shell.** It has its own Angular About (`apps/debrief/app/src/app/about/`), its own command
  (`apps/debrief/app/src-tauri/src/legal.rs`, `legal_text`) and the legal ids `notices`, `mit` and `apache`
  (`legal-source.ts` line 6). It ships the only `THIRD_PARTY_NOTICES.md` on `main` (15,966 lines: 768 Rust crates, 92
  npm packages, 385 distinct licence texts). Pitwall and Pitcrew have no notices and no legal texts at all.
- **The notices generator.** `conformance/src/notices/` (257 lines of JS on `main`). The shared version on
  `feat/shared-notices` (not merged yet; it arrives with the Dyno and Spotter notices branches) reads the app's `notices`
  block from `paddock.ci.json` and adds `scripts/notices.sh` (installs cargo-about 0.9.2, runs `npm ci
  --ignore-scripts`, then the JS); `scripts/notices.sh` is not on `main` today. Rust licences come from `cargo about generate`; npm
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
- the four About commands and the generated TypeScript in `ui/projects/paddock-ui/shell/about/generated/` that belongs
  to them: `about.ts`, `about-commands.ts` and the About part of `dialog-contracts.ts` (the Updates dialog's contract
  stays with the updater, so that file is split into `about-dialog-contracts.ts` and `updates-dialog-contracts.ts`),
  regenerated from the new crate.

The updater keeps the state machine, the manifest, signatures, the install hand-off and the service hooks, and its
description shrinks to that. Its `RecordStore` loses `whats_new_seen`.

**What's New gets its own small record, and existing users keep their place in it.** `paddock-about` owns a
`SeenVersion` port (read and write one version string) with a file adapter (`about.json` in the app's config folder, a
`paddock-env` versioned file). Because `notes::decide` treats a missing value as a first run (it records the version and
shows nothing), dropping the old value would silently skip What's New for every existing user on the first version that
ships this crate. So the file adapter migrates once from the updater's file, **eagerly, when it is constructed**: when
`about.json` is missing and `updates.json` in the same folder has `whats_new_seen`, that value is written to
`about.json` before the constructor returns. It must be eager because, once `Record` no longer has `whats_new_seen`,
the updater's next write of `updates.json` drops the field: `Updater::new` writes when it clears a failed install
(`updater.rs` lines 258–259) and the automatic check writes `last_checked` (line 485), and a tray app may ask
`about_whats_new_start` only after such a write. So `paddock-shell` builds the `SeenVersion` adapter (and with it About)
**before** it calls `paddock_updater::tauri::install`, and every app gets About through the shell. The read is a
function of the folder alone (no dependency on `paddock-updater`; it parses only that one field, with the file size
capped) and it never writes `updates.json`. This is a data migration for apps that are past v1, not an API shim; it can
be deleted once every app has shipped a release with `paddock-about`. Upgrade tests (failing first):

- a folder with only `updates.json` (`whats_new_seen` = the previous version) and a running version with notes gives
  `Show`;
- **the updater writes `updates.json` first** (the adapter constructed, then a failed-install clear and a `last_checked`
  write through the real `RecordStore` that drop the field), **then What's New still gives `Show`**;
- the shell's set-up test (a fake `RecordStore` that records the order) shows `about.json` written before the updater's
  first write;
- with `updates.json` missing too it gives `Remember`; a corrupt or oversized `updates.json` gives `Remember` and a log
  line, never an error.

**Dependencies.**

```
paddock-about        (pure core; feature `tauri`: managed state, commands, the adapter)
   ^          ^
   |          |
   |     paddock-shell  (wires About and the updater; sets `updates_available` from the updater)
   |          |
   |     paddock-updater (no dependency on paddock-about)
   |
every app with About, through paddock-shell (Debrief from Debrief 1.2's M2c)
```

`paddock-updater` does not depend on `paddock-about`. The only fact About took from the updater is "this build can
update itself". `paddock-shell`, which already depends on the updater, gains `paddock-about` and sets it:
`About::updates_available(updater.available())`. `paddock-about`'s dependencies are
`paddock-commands`, `paddock-dialogs` (the About dialog contract, ADR 0026), `paddock-files` (the releases link, as
today), `paddock-env`, `serde`, and `tauri` behind its feature.

**Notices are a required argument.** The app hands its notices and licences to the shell's builder, and the shell
builds `About` (whose constructor takes the notices) with the `SeenVersion` adapter, before it installs the updater. The
app never calls `About::new` itself, and cannot get an About without notices; the compiler is the every-app check, and
no new conformance script is written (ADR 0022 §3.7(a) allows no new JS tooling file):

```rust
// Names are illustrative; the shape is the decision.
// In the app (Dyno, for example): the only About code it has.
paddock_shell::builder("Dyno")
    .notices(paddock_about::notices!())                 // required: resources/THIRD_PARTY_NOTICES.md, id `notices`
    .legal(paddock_about::licence!(Mit))                // id `licence-mit`
    .legal(paddock_about::licence!(Apache))             // id `licence-apache`
    .changelog(include_str!("../../CHANGELOG.md"))
    /* other shell settings */;

// Inside paddock-shell's set-up, in this order (§1, the What's New migration):
let seen = paddock_about::SeenFile::open(&config_dir);  // migrates whats_new_seen from updates.json now
let about = About::new(name, version, seen, notices)    // `notices` from the builder
    .legal_all(legal)?;
paddock_updater::tauri::install(app, updates_builder)?; // the updater may write updates.json from here on
```

The builder's `.notices` is required by type (the builder without it has no `build`), so forgetting it is a compile
error in the app.

- `notices!()` expands to a `Notices` value (id `notices`, title "Third-party notices") with
  `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/THIRD_PARTY_NOTICES.md"))`. A missing file fails the
  build with rustc's "couldn't read" error, naming the path. `notices!("path")` takes another path relative to the
  manifest.
- The macro also checks the file's first line at compile time (a `const` assertion that it starts with
  `# Third-party notices`), so an empty or hand-written placeholder fails the build too.
- **Legal ids are fixed family-wide:** `notices`, `licence-mit`, `licence-apache`. Debrief's `mit` and `apache` change in
  its migration (its UI's `LegalDoc` union in `legal-source.ts` with it).
- `licence!(Mit | Apache)` embeds `paddock-about`'s own copies of `LICENSE-MIT` and `LICENSE-APACHE`. A unit test in
  `paddock-about` compares them with the repository root's files and with every `apps/*/LICENSE-*` (the copies the apps
  ship beside their sources, which conformance C28 already keeps equal to the template), so a difference fails in the
  crate's own tests.
- `LegalDoc` and `Notices` are typed values (id newtype checked at construction). `.legal(id, title, text)` with strings
  stays for other documents.

**The other every-app check** (rule 40) is in Rust: `./dev deps notices --all --check` (§4) fails when an app with a Tauri
manifest has no `notices` block in its `paddock.ci.json` or its notices file is stale. One run covers every app.

**Migration, each step landing with every app green.**

- **AB1, one change** (not additive-then-remove, so no step leaves two Abouts): the new crate; `paddock-updater` loses
  About, `UpdatesBuilder::legal` and `.changelog`; `paddock-shell`'s builder takes them; every app that uses the shared
  About moves to the macros in the same change: Dyno and Spotter (on the shell after their 1.0 tags), `shell-demo` and
  `workbench-demo`. Their legal ids become `licence-mit` and `licence-apache`.
- **AB2, Debrief: a delta to Debrief 1.2's M2c.** `docs/plans/debrief-1-2.md` has Debrief adopt the shell (line 7),
  and its package M2c (line 121) moves About, Updates and What's New onto the shell and `paddock-updater` and deletes
  `src-tauri/src/updater/`; the plan rules out throwaway work in Debrief's old UI (line 51). So Debrief gets
  `paddock-about` through the shell in M2c, and AB2 is only what M2c adds for this ADR: the `notices!()` argument, the ids
  `notices`, `licence-mit` and `licence-apache`, and the deletion of `legal.rs` and its `legal_text` command with
  Debrief's own About files. Debrief does not use the updater's About, so AB1 leaves it green.
- **Pitwall and Pitcrew (decided).** They get a `notices` block, a generated `THIRD_PARTY_NOTICES.md` and their SBOM in
  IN1 and IN2, and the `notices!()` registration when they adopt the shell (Pitwall in 1.1, Pitcrew when it resumes). A
  temporary About of their own would be a copy we delete (rule 42).

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
    pub licence: Licence,              // Spdx { original, expr } | NonSpdx(String) | Unknown
    pub texts: Vec<LicenceText>,       // { id: Option<LicenceId>, text, origin: File(path) | Canonical }
    pub reach: Reach,                  // Direct | Transitive
    pub scope: Scope,                  // Production | Build | Dev   (Optional for npm optional dependencies)
    pub hashes: Vec<Hash>,             // Cargo.lock checksum (SHA-256), npm integrity (SHA-512)
    pub confidence: Confidence,        // Exact (lockfile, resolved metadata) | Declared (manifest range only)
}
```

`spdx::Expression` has no serde support, so `Licence::Spdx` keeps the original string beside the parsed form;
serialisation writes the original and deserialisation parses it again with the same `ParseMode` (a round-trip test
covers it).

Workspace members (our own crates and packages) are `Source::Workspace` and path dependencies are `Source::Path`: notices
skip both, as today (`rustCrates` skips every crate without a `source`); the SBOM lists them as first-party components.

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
adapter in the family. `paddock-repo-graph`'s `FileSource` gains a size cap (`read_to_string(path, max_bytes)`, a
`Limit` error past it), so no reader can be handed an unbounded file.

**Licence texts** come from a `LicenceTexts` port. Its adapter (feature `licence-files`) reads candidate files from a
package's own folder (crate sources from `cargo metadata`'s `manifest_path`, npm packages from `node_modules`). It
detects each file's licence with the `spdx` crate's `detection` (threshold 0.8, the value cargo-about 0.9.2 and 0.9.3
use, `src/licenses.rs` line 110). When a chosen licence has no matching file, it falls back to the canonical text from
`spdx`'s `text` feature, as cargo-about does (`src/generate.rs` lines 141–154). The feature carries about 6 MB of SPDX texts and a
2 MB detection cache (measured in `spdx` 0.13.4's sources: 6.0 MB and 2.1 MB), so only `paddock-dev` enables it; apps embed only their
generated notices.

**Licence policy.** One file, `crates/paddock-deps/licences.toml`, replaces `templates/repo/about.toml` and
`licences.mjs`. It lives only there: `paddock-deps` embeds it at compile time (`include_str!`), so `./dev` and Pacenotes
evaluate the same compiled policy and no app has a copy.

```toml
allow = ["MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
         "0BSD", "CC0-1.0", "BSL-1.0", "Unicode-3.0", "Unlicense", "MPL-2.0", "CDLA-Permissive-2.0"]

[[exception]]
licence = "CC-BY-4.0"
ecosystems = ["npm"]
packages = ["caniuse-lite"]          # names (any version) or name@semver-requirement; omitted = any package
reason = "Build-time browser data; its attribution is carried by the notices."
approved = "2026-10-29, the user"
```

**One acceptance rule, fail-closed.** A package whose licence parses as an SPDX expression is accepted if and only if
`spdx::Expression::evaluate` returns true, with a predicate that returns true for a licence requirement when it is in
`allow`, or when an exception covers it for this package's ecosystem and name. `evaluate` is the only acceptance path for
SPDX expressions. A `NonSpdx` or `Unknown` licence is accepted only through a package-scoped exception (below). Nothing
else (a normalisation, a guessed id, a licence file found on disk) can accept a package.

- `OR` needs one accepted branch, `AND` needs all, `WITH` needs the exact `<licence> WITH <exception>` pair in `allow`.
- **Refused unless a package-scoped exception names them** (an exception with `packages`; an ecosystem-wide exception
  does not cover these): a licence that is not an SPDX expression (`NonSpdx`), a missing or empty field (`Unknown`),
  `NOASSERTION`, `UNLICENSED`, `SEE LICENSE IN <file>`, and any `LicenseRef-…`. For these the exception carries the
  reviewed licence text's file and a reason.
- **Parsing.** A custom `spdx::ParseMode`: `/` as `OR` allowed, `+` after GPL ids allowed, deprecated ids allowed, unknown
  ids not allowed, and **imprecise names off** (`allow_imprecise_license_names: false`), so "Apache 2" or "BSD" never
  become an identifier we did not read. Every lax reading (a slash, a deprecated id) is reported on the package as a
  normalisation; a normalisation changes only how the expression is read, never whether a requirement is allowed.
- **What `+` and deprecated ids mean.** `MPL-2.0+` is the requirement "MPL-2.0 or later"; the allowed licensee `MPL-2.0`
  satisfies it because the ids are the same (`spdx` 0.13.4, `Licensee::satisfies`, `src/licensee.rs` lines 192–226; the same in 0.13.6).
  The deprecated `GPL-2.0` parses to the deprecated identifier `GPL-2.0`; no GPL identifier is in `allow`, so it
  evaluates false and is refused, whichever GPL variant was meant.
- **Exceptions** name their licence, ecosystems, optional packages, reason and approval; one without a reason or approval
  does not load. A package accepted only through an exception says so in the report.
- **The npm-only exceptions (decided by the user, 2026-10-29):** `CC-BY-4.0`, `OFL-1.1`, `Python-2.0` and
  `BlueOak-1.0.0`, each for npm only and scoped to the packages that use it (fonts for OFL-1.1, for example). No app's
  production closure uses `Python-2.0` or `BlueOak-1.0.0` today (packages under them are in the root lock only as dev
  tooling), so they get no `[[exception]]` entry until a shipped package needs one; DP1 confirms this from every app's
  inventory. A new npm package under any of the four needs its name added, which is a reviewed diff.
- **cargo deny stays a second gate** for Rust (advisories, sources, bans and licences). A test in `paddock-deps` reads
  every `deny.toml` in the repository, found by a walk (today twelve: `crates/`, `tools/paddock-dev/`,
  `templates/repo/`, seven `apps/<app>/deny.toml` and `apps/shell-demo/src-tauri/deny.toml` and
  `apps/workbench-demo/src-tauri/deny.toml`), so a new one is covered without editing the test, and checks three
  things: each `[licenses].allow` equals `allow`; **every** `[licenses].exceptions` entry of **every** deny.toml
  (for example `crates/deny.toml`'s `webpki-root-certs` / `CDLA-Permissive-2.0`) has a cargo-scoped `[[exception]]` in
  `licences.toml` with the same crate and licence; and every cargo-scoped `[[exception]]` appears in **at least one**
  deny.toml (an exception no workspace uses is stale and fails).
- **Published packages are out of scope.** The policy covers what the apps ship. It is not applied to the packages we
  publish (the shared npm and crate packages). If it ever is, note that the `eslint` preset workspace's production closure
  pulls in `minimatch` 10.2.6, which is `BlueOak-1.0.0` (root `package-lock.json`), so that workspace would need its own
  package-scoped entry.

DP1's failing tests are the rows of the table in Problems 2 and these: `MIT AND GPL-3.0-only` refused; `MIT OR
GPL-3.0-only` accepted; `MPL-2.0+` accepted; `GPL-2.0` refused; `MIT/Apache-2.0` accepted with a normalisation reported;
`Apache 2` refused (imprecise names off); `CC-BY-4.0` refused for a crate and for an npm package not named in the
exception, accepted for `caniuse-lite`; `NOASSERTION`, `UNLICENSED`, `SEE LICENSE IN LICENSE.txt`,
`LicenseRef-Proprietary`, `""` and a missing field refused, and accepted only through a package-scoped exception;
`Apache-2.0 WITH LLVM-exception` accepted and `MIT WITH LLVM-exception` refused; an exception without a reason does not
load; the deny.toml equality test fails on a planted difference.

**Outputs (writers, pure functions of the inventory).**

1. **`THIRD_PARTY_NOTICES.md`**, equal byte for byte to today's output apart from the header line that names the
   generator. That means the same order (kind, then name, then version, as plain string comparison), the same text
   normalisation (line endings to LF, trailing blanks per line and outer blank lines removed), texts de-duplicated by
   SHA-256 and labelled `<id>-<n>` in (id, hash) order, the "Used by" wrap at 96 columns, the npm text ids guessed from
   the file name, and cargo-about's choice of licences for an `OR` expression (the first branch the accepted list
   satisfies: `MIT OR Apache-2.0` lists only the MIT text, as Debrief's notices show for `anyhow` and `serde`).
2. **CycloneDX 1.6 JSON**, one file per app per release (`<app>-<version>.cdx.json`, both ecosystems in one BOM):
   `metadata.component` is the app with its licence. Components (decided): third-party packages; our own `paddock-*`
   crates and app crates as first-party components without hashes; and the bundled binaries (libduckdb, FFmpeg,
   llama.cpp and the other pins in `paddock-runtime`'s catalogues that an app ships), from those catalogues with their
   pinned hashes. Components carry `purl` (`pkg:cargo/…`, `pkg:npm/…`), `bom-ref`,
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

**No network by default, and none in the release path.** The core has none. Fetching a licence text is the feature
`fetch`, off by default, through `paddock-net`'s `Egress` with a host allow-list and `set_offline`. `paddock-dev` never
enables it, so no `./dev` command (release, notices or SBOM) can fetch: the release is reproducible from the lockfiles
and sources. This is enforced by the same `cargo tree -e features` check that guards Pacenotes (§5), applied to
`tools/paddock-dev`. Pacenotes may enable it, for opt-in lookups per repository only (§5, decided by the user).

### 3. Library choices (researched 2026-10-29)

Facts from the crates.io API and the published sources unless marked otherwise. "Recent downloads" is crates.io's 90-day
count.

| Need | Choice | Licence | Maintenance | Why, and the rejected alternatives |
|---|---|---|---|---|
| SPDX expressions, evaluation, deprecated spellings, canonical texts, licence detection | **`spdx` `=0.13.4`** (EmbarkStudios) | Apache-2.0 (0.13.4's manifest) | 0.13.6 published 2026-10-02; ~7.0 M recent downloads; used by cargo-deny and cargo-about | It is what cargo-about and cargo-deny use, so our evaluation and detection agree with both. It has `ParseMode::LAX` (slash, imprecise names, `+` on GPL, deprecated ids; `src/lexer.rs`), the SPDX licence list 3.28.0 in 0.13.4 (3.29.0 in 0.13.6), `text` and `detection` features. **Pin it exactly to the oracle's version:** `spdx = "=0.13.4"`, the version in cargo-about 0.9.2's `Cargo.lock` (the pinned oracle in `scripts/notices.sh` and the workflows; 0.9.3 locks 0.13.6). Detection scores decide which files count, so a different store can change the notices. To move to 0.13.6, bump the oracle to cargo-about 0.9.3 first and regenerate every app's notices in that change. Writing our own: no, the SPDX grammar and the licence list are exactly what a shared crate should not re-implement. |
| `cargo metadata` types | **Own serde structs** for the ~10 fields used | ours | – | `paddock-repo-graph` already parses this JSON with `serde_json`; ids are used as opaque keys, so the PackageId format change does not matter. Rejected: `cargo_metadata` 0.23.1 (MIT, 2025-11-12, ~65 M recent downloads, well maintained) would add `derive_builder` and `cargo-platform` and a 0.x API that breaks every few releases for a dozen fields. Fallback if the subset grows: adopt it. |
| `Cargo.lock` without running cargo (Pacenotes) | **`cargo-lock` 11.x** (RustSec) | Apache-2.0 OR MIT | 11.1.0 published 2026-07-24; ~7.7 M recent downloads; part of the RustSec project | Parses every lockfile version, including dependency lists and checksums, without cargo. Writing our own is possible (TOML) but the format has three versions and RustSec maintains them. |
| CycloneDX 1.6 JSON | **Own writer** (serde structs for the subset we emit) | ours; schema Apache-2.0 | – | `cyclonedx-bom` 0.8.1 (Apache-2.0, published 2026-03-19) supports spec 1.3–1.5 only (`src/models/bom.rs`, `SpecVersion`; read 2026-10-29). Support for 1.6 and 1.7 was merged upstream on 2026-10-08 (PR #872), but no release has it yet. `serde-cyclonedx` 0.10.0 (MIT, 2025-06-17) is generated from schemas and small. We emit about fifteen fields; a writer of our own with a schema test is smaller than either dependency. Revisit `cyclonedx-bom` when a release with 1.6 ships (it would also give us 1.7 and validation). |
| Licence files and detection | **Our scan on `spdx` detection**, matching cargo-about's rules | ours | – | See "Risks": reproducing cargo-about's choices is the hard part. Rejected for production use: the `cargo_about` library (MIT OR Apache-2.0, 0.9.3 published 2026-10-26; it has a `lib` target). It links `ureq`, `rustls` and `ring` unconditionally, its library API is the binary's internals (no stability promise), and it brings `krates`. It stays the **oracle**: the golden and shadow comparisons run today's cargo-about against our output. Fallback if DP3 cannot close the differences in its budget: use `cargo_about::licenses::Gatherer` for Rust crates behind the `licence-files` feature. |
| XML (Maven, NuGet `.csproj`) | **`roxmltree`** | MIT OR Apache-2.0 | 0.21.2 published 2026-10-24; already in the family's lockfiles (0.20.0) | `ParsingOptions` defaults to `allow_dtd: false` (a document with a DTD is an error) and `nodes_limit: u32::MAX` (`src/parse.rs` lines 340–376, read 2026-10-29). We keep DTDs refused and **set `nodes_limit`** from `Limits`; PN1 tests a billion-laughs document and an over-limit one. |
| Schema test | **`jsonschema`**, dev-dependency only, default features off | MIT | 0.60.2 published 2026-10-26; already in the family's lockfiles (0.33.0) | Validates the CycloneDX output in tests. The 1.6 schema references `spdx.schema.json` and `jsf-0.82.schema.json`; all three are vendored under `tests/fixtures/schema/` and resolved locally (no network in tests). |
| Compile-fail tests of `notices!()` | **`trybuild`**, dev-dependency of `paddock-about` | MIT OR Apache-2.0 | dtolnay; widely used | Checks the error text of a missing and of a placeholder notices file, which a `compile_fail` doc test cannot. |
| UUID v5 for `serialNumber` | **`uuid`** (`v5`) | Apache-2.0 OR MIT | 1.29.0 published 2026-10-23; already in the family | – |

Every crate above is on the allow list; `cargo deny` checks the transitive ones in DP1. `spdx` depends on `zstd` 0.13
(MIT, crates.io, 2026-10-29) only through its `detection-cache` feature (`spdx` 0.13.4 `Cargo.toml`: `zstd` is optional and
named only by `detection-cache`); `paddock-deps` enables `detection` and the cache, so `zstd` and `zstd-sys` are in
`paddock-dev`'s tree, not in any app's.

### 4. Integration

- **`paddock-dev`** gets `./dev deps notices <app> [--check]`, `./dev deps sbom <app> --out <dir>` and
  `./dev deps inventory <app> --json`, reading the app's `notices` block in `paddock.ci.json` (the fields of
  `feat/shared-notices`: `name`, `manifests`, `npm`, `out`; `about` and `rustJson` go). This is the notices part of ADR
  0022's R6, which shrinks by it. The tool runs `cargo metadata --locked --offline`; when sources are missing it fails with
  "run `cargo fetch --locked`", and when `node_modules` is missing with "run `npm ci --ignore-scripts`".
- **Deleted in the same change** (ADR 0022 §3.7, the baseline shrinks): `scripts/notices.sh`,
  `conformance/src/notices/`, `conformance/bin/paddock-notices.mjs`, `conformance/test/notices.test.mjs`,
  `templates/repo/about.toml`, and every cargo-about install and call: `app-licences.yml` line 59 (`cargo-about@0.9.2`
  in the install step; `cargo-deny` stays) and lines 80–85 (the "Third-party notices are up to date" step, replaced by
  `./dev deps notices --check`), and `debrief.yml` lines 362–366 (the cargo-about install and its comment) with the
  cache comment at lines 368–370 reworded (the cache stays for cargo deny). `debrief.yml` lines 1023–1029 (the SBOM
  step) go in IN2. Line numbers are `origin/main` at
  `b618b5c97`; the `feat/shared-notices` lines in `app-gates` go too if that branch has merged.
- **The conformance check that notices are current stays,** as the `app-gates` step `./dev deps notices <app> --check`
  on the Linux leg (where `feat/shared-notices` put the JS check), plus `./dev deps notices --all --check` (§1).
- **SBOMs for every app.** `app-release.yml` (Dyno, Spotter, Pitwall, Pitcrew, the demos) and `debrief.yml` run
  `./dev deps sbom` and attach `<app>-<version>.cdx.json` as a release asset. Debrief's `cargo cyclonedx` and `npx --yes
  @cyclonedx/cyclonedx-npm` steps are removed, and with them the unpinned download. Before they go, IN2 runs both old
  tools and `./dev deps sbom` once on Debrief and compares the sets of `purl`s (and their versions); every difference is
  explained in the PR (for example build dependencies that cargo-cyclonedx lists and we leave out).
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
   `cargo-metadata` feature. `paddock-deps`' `fetch` feature is allowed under `apps/pacenotes/` only in the one crate
   that runs the opt-in lookups (PN3) and forbidden in every other Pacenotes crate; it stays forbidden in
   `tools/paddock-dev` (IN1's check, kept by PN3's test). The per-crate `fetch` allowance is a manifest rule only: Cargo unifies
   features across a binary, so the runtime protection is the opt-in setting, the six-host `Egress` allow-list and PN3's
   "setting off records no request" test.
4. **Licences without a build.** In v1 the inventory shows the licence when the lockfile carries it (npm), and "not known
   offline" otherwise, never a guess. Licence resolution (v1.1 in the proposal) reads package caches already on the
   machine, read-only and only with the user's opt-in, because they are outside the workspace allow-list: the Cargo
   registry, the NuGet global packages folder, the Go module cache and the local Maven repository. It reads the
   declared licence from each cached package's own metadata (the `license` field of the crate's `Cargo.toml`, the
   `.nuspec`, the POM, the Python package's metadata), which needs no detection. Detecting licence files is only for
   packages without a declared licence and for showing texts; it runs in a worker under `paddock-guard` with the limits
   above, with `spdx`'s detection compiled into that worker only. Online lookups (decided by the user, 2026-10-29) fill in what the caches do not:
   - **Opt-in per repository**, off by default; the setting names the hosts and says that they learn the dependency
     names.
   - **Only through `paddock-net`'s `Egress`, only to the registries' official metadata APIs:** `crates.io`,
     `registry.npmjs.org`, `api.nuget.org` (nuget.org), `pypi.org`, `proxy.golang.org` and Maven Central
     (`repo1.maven.org`, POM files). The host allow-list is these six and nothing else; redirects to another host are
     refused (Egress checks every hop).
   - **Cached** per package and version with the source URL and the date fetched, in Pacenotes' store; a cached answer is
     used before any request, and the inventory shows where and when each licence came from.
   - **Rate-limited and bounded:** a few requests at a time per host (rule 13), a per-host rate with back-off on 429,
     a response size cap, a timeout, and a cap on lookups per run; the job shows live progress and Cancel works (rules
     10–12).
   - **Never in `paddock-dev`:** the `fetch` feature stays out of the CI binary (§2), so release commands cannot look
     anything up.
5. **Export.** CycloneDX 1.6 from the same writer (v1.1), with Pacenotes' tier and confidence labels kept as CycloneDX
   properties.

## Phases and work packages

Estimates are agent-days for an implementer; DP3 and PN1 start with a researcher. Every package starts with its failing
tests, lands on its own with every app green, and passes the gates named. The common gates for crate work are `cargo fmt
--check`, clippy with `-D warnings` (and xwin clippy, ADR 0021), nextest, `cargo llvm-cov` at 99 % lines or more for the
crate, `cargo hakari generate --diff`/`verify`, `node scripts/publish.test.mjs` and `cargo deny check`.

**Ownership.** The "Owns" paths of the packages are disjoint, so packages of different phases can run in parallel
within load. The **integrating agent (the coordinator)** makes every change to the shared integration points (root
AGENTS.md): it adds the `crates/Cargo.toml` member and `[workspace.dependencies]` lines, regenerates and commits
`crates/Cargo.lock` and `crates/paddock-workspace-hack/` (`cargo hakari generate`), adds the `crates/README.md` rows and the
`docs/adr/README.md` status, edits `crates/paddock-updater/src/lib.rs`, `crates/paddock-shell/src/lib.rs` and
`crates/paddock-repo-graph/src/lib.rs` (re-exports and command tables), `tools/paddock-dev/src/cli.rs` (the subcommand
line), and everything under `.gitea/`. A package hands it those lines in its PR description. DP1 owns the
`paddock-deps` skeleton: `src/lib.rs` with every module declared (`policy`, `inventory`, `npm`, `cargo`, `texts`,
`notices`, `cyclonedx`, `nuget`, `pypi`, `go`, `maven`, `caches`, `fetch`) as empty files, and `tests/it/main.rs` with one `mod`
per later package, so later packages own only their module folders. The one shared file is `tools/paddock-dev/src/plan.rs`: DP2 owns its `DiskFiles` and DP3 its `CargoCli` call,
different items, so the two merge in either order.

**When.** Phases 1 and 2 start after the Dyno 1.0 and Spotter 1.0 tags (decided: DP1–DP4 and IN1 do not wait for
anything else in ADR 0022; IN1 is R6's notices part). Every package that changes a crate a release depends on, the
workspace hack or a workflow (AB1, DP1, DP2, DP3, IN1, IN2) lands **between releases**, never during a release's freeze
(rule 1). DP4 touches only `paddock-deps`, which no release depends on until IN1.

| Phase | WP | Content | Owns | Depends on | Failing tests first | Gates beyond the common ones | Days |
|---|---|---|---|---|---|---|---|
| **1 About** | AB1 | `paddock-about` (new-crate checklist), the What's New migration from `updates.json`, `notices!()`/`licence!()`, command table, `tauri` adapter, generated TS; `paddock-updater` loses About; `paddock-shell` wires both; Dyno, Spotter, `shell-demo` and `workbench-demo` move to it, ids `licence-mit`/`licence-apache` | `crates/paddock-about/**`; `crates/paddock-updater/src/{about.rs,notes.rs,record.rs,generated.rs,tauri.rs,tauri/**}`, its `tests/**` and `Cargo.toml`; `crates/paddock-shell/src/` except `lib.rs`, its `Cargo.toml`; `ui/projects/paddock-ui/shell/about/**`; the About wiring in `apps/{dyno,spotter}/app/src-tauri/src/lib.rs` and `apps/{shell-demo,workbench-demo}/src-tauri/src/lib.rs`, and those four apps' `src-tauri/Cargo.toml`; the `notices` blocks of `apps/{shell-demo,workbench-demo}/paddock.ci.json` and their generated `src-tauri/resources/THIRD_PARTY_NOTICES.md` (made with today's `scripts/notices.sh` until IN1, so IN1's `--all --check` passes for them) | the Dyno 1.0 and Spotter 1.0 tags (both apps on the shell); `feat/shared-notices` merged (it arrives with the Dyno and Spotter notices branches); lands before Pitwall 1.1's freeze (decided) | `trybuild`: a missing and a placeholder notices file fail with the path in the error; the upgrade tests of §1, including "the updater writes `updates.json` first, then What's New still gives Show" and the shell's order test; `About::new` without notices does not compile; the licence copies equal every app's; the updater's command table has no About command; What's New never shows another version's notes (moved) | the shell's About, Updates and What's New Playwright specs (Chromium); the generated-TS sync tests; the affected gates of Dyno, Spotter and both demos | 3.5 |
| | AB2 | A delta to Debrief 1.2's M2c (which moves Debrief's About, Updates and What's New onto the shell and `paddock-updater`): Debrief's About gets `notices!()` and the ids `notices`, `licence-mit`, `licence-apache`; `legal.rs`, `legal_text` and Debrief's own About files are deleted in M2c | the lines M2c owns for About in `apps/debrief/app/src-tauri/src/lib.rs` and its `Cargo.toml`; `apps/debrief/app/src-tauri/src/legal.rs` (deleted); `apps/debrief/app/src/app/about/**` (deleted by M2c) | AB1; Debrief 1.2's M2c | Debrief's About, through the shell's command, lists the three ids; asking for `mit` is refused | Debrief's affected gates; one real-app smoke | 0.5 |
| **2 Inventory core** | DP1 | `paddock-deps` skeleton and policy: `Inventory`, `Licence` with serde round trip, the custom `ParseMode`, `licences.toml`, `evaluate` as the only acceptance path, scoped exceptions, the deny.toml equality test and the cargo exceptions mirrored | `crates/paddock-deps/{Cargo.toml,licences.toml,src/lib.rs,src/policy/**,src/inventory/**,src/*/mod.rs stubs,tests/it/main.rs,tests/it/policy.rs}`; the `[licenses]` sections of every `deny.toml` in the repository (found by a walk; twelve today, including `apps/shell-demo/src-tauri/deny.toml` and `apps/workbench-demo/src-tauri/deny.toml`) | phase 1's window (not AB1 itself) | the cases listed under "One acceptance rule" | `cargo deny` over the new dependencies | 2 |
| | DP2 | npm: the typed lock model in `paddock-repo-graph` with `Scope`; `FileSource` size cap; the production closure; `license` from the lock; licence files from `node_modules` | `crates/paddock-repo-graph/src/{lock.rs,source.rs,npm.rs}` and their tests; `crates/paddock-deps/src/npm/**`, `tests/it/npm.rs`, `tests/fixtures/npm/**`; `tools/paddock-dev/src/plan.rs`'s `DiskFiles` (its `FileSource` implementation, line 406, gains the size cap) | DP1 | golden: the JS tool's npm half on synthetic locks (workspaces, nested `node_modules`, links, optional peers); a file over the cap is a `Limit` error; `lock_diff`'s tests stay green | `paddock-repo-graph`'s tests and coverage | 1.5 |
| | DP3 | Cargo: the widened `CargoMetadata` port and adapter, production closure (normal dependencies, all targets), `Source::Path` and `Workspace` skipped in notices, licence-file scan with `spdx` `=0.13.4` detection, canonical fallback, accepted-order choice; `cargo::Lockfile` on `cargo-lock` | `crates/paddock-repo-graph/src/cargo.rs` and its tests; `crates/paddock-deps/src/{cargo,texts}/**`, `tests/it/cargo.rs`, `tests/fixtures/cargo/**`; the `CargoCli` call in `tools/paddock-dev/src/plan.rs` (line 433) when the port's arguments widen | DP1 | golden: cargo-about 0.9.2 JSON on synthetic crates (dual licences, no licence file, two MIT files from different holders, an `AND` expression, a `LicenseRef`, a path dependency); never-panics on lockfile bytes | a local comparison with today's notices of Debrief, Dyno and Spotter (only reviewed differences) | 3 |
| | DP4 | Writers: notices (byte for byte but the header) and CycloneDX 1.6 (vendored schemas, reproducible serial and timestamp, first-party and bundled-binary components) | `crates/paddock-deps/src/{notices,cyclonedx}/**`, `tests/it/{notices,cyclonedx}.rs`, `tests/fixtures/{notices,schema}/**` | DP1 (fixtures from DP2 and DP3 when they land; synthetic inventories until then) | golden notices from the JS `render`; a BOM made twice is identical; the schema test fails on a BOM without `specVersion` | – | 2 |
| **3 Integration** | IN1 | `./dev deps notices|sbom|inventory`, `--all --check`; shadow week; notices blocks and generated notices for Pitwall and Pitcrew; then the deletions of §4; the step's time before and after in `docs/benchmarks.md` (rule 48) | `tools/paddock-dev/src/deps/**` and its tests, `tools/paddock-dev/{Cargo.toml,Cargo.lock}`; `apps/{pitwall,pitcrew}/paddock.ci.json` notices blocks and their generated `THIRD_PARTY_NOTICES.md`; the deleted files of §4 outside `.gitea/` | DP2, DP3, DP4 | `--check` fails on a stale file and on a refused licence; `--all --check` fails for an app without a `notices` block; a missing `node_modules` gives the next-step message; the feature check fails when `tools/paddock-dev` enables `fetch` | shadow week with no unexplained difference; ADR 0022 §3.7 baseline shrinks | 1.5 |
| | IN2 | SBOMs in `app-release.yml` and `debrief.yml` as release assets; the one-time `purl` comparison; remove the cargo-cyclonedx and cyclonedx-npm steps and the cargo-about lines of §4 | done by the integrating agent (all of it is `.gitea/`) | IN1 | a `workflow_dispatch` dry run of `app-release.yml` lists `<app>-<version>.cdx.json` | dry run per app (ADR 0022 phase 2 acceptance) | 1 |
| **4 Pacenotes** (stage 2, after Debrief 1.2) | PN1 | NuGet, PyPI, Go and Maven readers; workspace discovery through `paddock-repo-files`; `roxmltree` with `nodes_limit`; the feature check extended to `apps/pacenotes/` | `crates/paddock-deps/src/{nuget,pypi,go,maven}/**`, `tests/it/{nuget,pypi,go,maven}.rs`, `tests/fixtures/{nuget,pypi,go,maven}/**`; the fuzz targets under `crates/paddock-deps/fuzz/` | DP1; Pacenotes stage 2 | per reader: golden inventories on synthetic repositories; never-panics corpus; a POM with a DTD and one over the node limit refused; the feature check fails on a planted `cargo-metadata` in any Pacenotes crate and on `fetch` in any Pacenotes crate but PN3's lookup crate | Pacenotes' F1 port checks; fuzz targets on `app-fuzz.yml` | 4 |
| | PN2 | Licences from the opt-in package caches (declared `license` fields first), detection in a guarded worker, CycloneDX export with Pacenotes' labels | `crates/paddock-deps/src/caches/**`, `tests/it/caches.rs`; Pacenotes' adoption under `apps/pacenotes/crates/` | PN1, DP3, DP4 | a package without cached sources says "not known offline"; a cache outside the opt-in is never read (a fake file source records every read) | – | 2 |
| | PN3 | Opt-in online licence lookups (`fetch` feature): one client per registry API on `Egress`, the six-host allow-list, cache with source and date, rate limits and caps, the per-repository setting | `crates/paddock-deps/src/fetch/**`, `tests/it/fetch.rs`, `tests/fixtures/fetch/**` (recorded synthetic API answers); Pacenotes' setting and job under `apps/pacenotes/` | PN2; `paddock-net` | with the setting off, a fake wire records no request; a host outside the six (and a redirect to one) is refused; a cached answer makes no request and keeps its source and date; a 429 backs off and a run stops at its lookup cap with a "limit reached" result; an oversized or malformed answer is a typed error for that package only; Cancel stops the job with the cache consistent; the feature check fails when `tools/paddock-dev` enables `fetch` (IN1's test, kept) | tests on a fake `Egress` wire only, never the real hosts (rule 7) | 1.5 |
| | | **Total** | | | | | **about 22.5** (15 for releases, 7.5 for Pacenotes) |

## Risks

| Risk | Mitigation |
|---|---|
| Our licence-file choices differ from cargo-about's, so the notices are not byte for byte the same | cargo-about 0.9.2 stays the oracle in DP3's golden tests and in the shadow week; `spdx` is pinned to `=0.13.4` (the oracle's) and the threshold to 0.8; fallback: the `cargo_about` library behind `licence-files` (§3) |
| A new `spdx` release changes detection and so the notices | exact pin; an update is a deliberate change that bumps the oracle and regenerates every app's notices in the same PR, reviewed as a diff (Renovate is told to leave `spdx` alone) |
| Existing users miss What's New for the version that ships `paddock-about` | the one-time read of `whats_new_seen` from `updates.json` and its three upgrade tests (§1) |
| The fail-closed policy refuses packages the JS tool let through, blocking a release | DP1 runs the policy over every app's current inventory first; each refusal becomes a package-scoped exception with a reason for the user to approve, or a dependency change, before IN1 switches the check |
| Phase 1 changes `paddock-updater` and `paddock-shell`, which Pitwall 1.1 and Debrief 1.2 also change | AB1 lands after the Dyno and Spotter tags and before Pitwall 1.1's freeze (decided), and Pitwall 1.1's shell move adopts `paddock-about` directly; AB2 is part of the Debrief 1.2 plan; rule 1 keeps tags serialised |
| A new crate's hakari line changes feature unification for every crate | the integrating agent lands the member and hakari change between releases, as its own small PR |
| The SBOM omits something a consumer expects (build tools, the WebView runtime) | bundled binaries and first-party crates are components (§2); the BOM states what it covers in `metadata.properties` |
| Hostile manifests in Pacenotes (huge files, deep nesting, XML entities) | limits on every input, never-panics corpora, fuzzing, DTDs refused, readers run in a guarded worker |
| Licence text fetching leaks dependency names | off by default, explicit flag or click, through `Egress`; never in release builds |

## Consequences

- `paddock-updater` does one job; About can be used without it.
- One mechanism registers notices, and a missing or placeholder file fails the build.
- One Rust implementation answers "what do we ship and under which licence" for notices, SBOMs and Pacenotes. The JS tool,
  `scripts/notices.sh`, `about.toml`, cargo-about, cargo-cyclonedx and cyclonedx-npm leave the release path.
- The licence check is fail-closed: one evaluation path, correct `AND` semantics, and scoped, reasoned exceptions.
  Expressions the JS check accepts today (`MIT AND GPL-3.0-only`, unknown and non-SPDX licences) will be refused.
- Every app release carries a reproducible CycloneDX 1.6 SBOM.
- Pacenotes starts its inventory on a tested core and adds only readers.

## Decided while writing (2026-10-29)

- **Phase 2's start:** DP1–DP4 start after the Dyno and Spotter tags, each landing between releases; IN1 is ADR 0022 R6's
  notices part and R6 shrinks by it.
- **Pitwall and Pitcrew:** notices file, SBOM and the `--all --check` now (IN1, IN2); `notices!()` when they adopt the
  shell. No temporary About.
- **npm-only exceptions** (the user): `CC-BY-4.0`, `OFL-1.1`, `Python-2.0` and `BlueOak-1.0.0`, for npm only, scoped per
  package where the current lock allows.
- **CycloneDX:** 1.6 (1.7 exists, but no Rust library releases 1.6 yet), one combined BOM per app, dev dependencies left
  out, first-party crates and bundled binaries included.
- **Debrief** gets `paddock-about` through the shell in Debrief 1.2's M2c; AB2 is the delta (§1).

## Decided by the user (2026-10-29)

1. **AB1 lands after the Dyno 1.0 and Spotter 1.0 tags and before Pitwall 1.1's freeze.** Pitwall 1.1's move to the shell
   then adopts `paddock-about` directly (its `notices!()` registration is part of that move).
2. **Pacenotes may make opt-in online licence lookups, per repository,** through `paddock-net`'s `Egress` only and only
   to the registries' official metadata APIs (crates.io, registry.npmjs.org, nuget.org, pypi.org, proxy.golang.org,
   Maven Central): off by default, never in `paddock-dev` release commands (the `fetch` feature stays out of the CI
   binary), results cached with source and date, rate-limited and bounded (§5 point 4, PN3).

No open questions remain.

## Sources

Verified (read or run on 2026-10-29):

- crates.io API (`https://crates.io/api/v1/crates/<name>`) for licence, latest version, date and downloads of `spdx`,
  `cargo_metadata`, `cargo-lock`, `cyclonedx-bom`, `serde-cyclonedx`, `cargo-about`, `krates`, `roxmltree`,
  `jsonschema`, `uuid`, `zstd`, `askalono`, `packageurl`, `purl`.
- Sources from `https://static.crates.io/crates/<name>/<name>-<version>.crate`: `spdx` 0.13.6 (`Cargo.toml` features,
  `src/lexer.rs` `ParseMode`, `src/identifiers.rs` list 3.29.0, `src/detection/`) and `spdx` 0.13.4 (licence
  Apache-2.0, list 3.28.0, `zstd` 0.13 optional, `src/licensee.rs` `satisfies`, `src/text` 6.0 MB, `src/detection` 2.1 MB), `cyclonedx-bom` 0.8.1
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

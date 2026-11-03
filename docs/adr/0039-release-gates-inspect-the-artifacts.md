# 39. Release gates inspect the artifacts: the published installers are opened, read and started, not the build steps trusted

- Status: Proposed (2026-11-03). The user approved drafting it on 2026-11-03 (ER-21 redesigns). No work starts before it is
  accepted.
- Date: 2026-11-03
- Related: ER-15 / rule 9 (releases only from a tag in CI), ER-21 / rule 40, ER-27 / rule 56 (logs explain every failure),
  rule 23 (ad-hoc seal, `codesign --verify`, sidecars started from the built app), rule 25 (before tagging), rule 54 (the
  release path builds installers before it lands), rule 55 (copies); ADR 0028 (`paddock-about`, notices in About),
  ADR 0032 (Windows binaries link the C runtime statically, `windows-crt-imports.mjs`), ADR 0022 (the release kit in
  `./dev`, the macOS bundle verify copies in G.1); Dyno ADR 0026 (macOS sockets from the kernel list, the dotted
  identifier); lessons "tauri-build copies every sidecar over the binary cargo just linked" (2026-11-01), "An unbuilt UI
  hid a broken installer build because tests passed" (2026-10-30), and "`| grep -q` and `| head` under pipefail lose a
  SIGPIPE race" (2026-11-01, on branch `fix/first-release-notes-mawk`).
- Evidence (read at `origin/main` `9da2ccf59` = `dyno-v1.0.0`, and on the named branches, 2026-11-03):
  - `.gitea/workflows/app-release.yml`:
    - `bundle` runs the app's `bundle-check` script against the build folder (`$PADDOCK_BUILD_FOLDER/dist/bundle`, line
      245), not against the artifact it uploads, and not on Windows (line 241);
    - `macos-verify` checks the seal, the Team ID and the sidecar self-tests on the built app;
    - `release` only checks that a file matches each of `*.dmg`, `*-setup.exe`, `*.deb` and `*.AppImage` (lines 307 to
      316), and publishes.
  - `apps/dyno/scripts/check-bundle.sh` (84 lines; the only app `bundle-check`, set in `dyno-release.yml`):
    - checks run paths, presence of `dyno-agent` and `dyno`, and the code-signing identifiers of those two Mach-O against
      `^net\.decryptology\.dyno\.`;
    - says "Windows is not checked here";
    - on `main` it still pipes into `grep -q` under `set -o pipefail` (`file -b "$f" | grep -q '^ELF' || continue`, and
      the Mach-O twin). That is fixed in `a11410909` on `fix/first-release-notes-mawk`, which is not yet on `main`.
  - `conformance/release/windows-crt-imports.mjs` runs only in `.gitea/workflows/debrief.yml:972`. No other app's
    installers are checked for `VCRUNTIME140.dll` imports.
  - The in-progress fix on `fix/dyno-agent-signing-id` (uncommitted in its worktree, read 2026-11-03, so unverified until
    committed): its amendment to Dyno ADR 0026 records that the published `Dyno_1.0.0` bundle has the right identifiers
    (`net.decryptology.dyno.agent`, `net.decryptology.dyno.cli`), and that macOS 27.2 shows the agent only its own sockets
    when the agent runs as a child of the ad-hoc signed `.app` started by LaunchServices. It sees the whole list from a
    shell or as a launchd job. Its `check-bundle.sh` adds a run of `dyno-agent view` as a launchd job.

## Context

| # | Date | Instance | What the gate checked | What was wrong |
|---|---|---|---|---|
| 1 | 2026-11-01 (released), 2026-11-03 (reported) | Dyno 1.0.0: the macOS Network page said "macOS showed this program only its own sockets… needs a code-signing identifier with a dot" | the identifier string of two files in the build folder | per the in-progress fix: the identifier was right, and the behaviour the property stands for (the whole socket list) failed in the way the app launches its agent. The gate checked a proxy, not the behaviour as shipped |
| 2 | 2026-11-01 | the Dyno signing-identifier test saw a 0-byte `dyno-da39a3ee…`: tauri-build copied empty sidecar placeholders over the linked binaries (fix `f61bd8805`) | a binary in the build folder at a moment that depended on build order | 20 of 20 failures in the rerun loop (lesson). The same placeholders exist in Pitwall and Pitcrew (`origin/fix/sidecar-placeholders-shared`) |
| 3 | 2026-10-30 | Dyno's installer build failed on a clean runner; a leftover `ui/dist` hid it (rule 54) | the steps' exit codes on a warm working tree | the artifact was never built from a clean tree |
| 4 | 2026-10-30 (ADR 0032) | sidecars imported `VCRUNTIME140.dll`; only a clean PC shows it | Debrief only | Dyno, Spotter, Pitwall and Pitcrew installers are not checked |
| 5 | 2026-11-01 | the Dyno 1.0 tag run's `first-release-notes.sh` said "the section is empty" for a 45-line section (SIGPIPE under pipefail) | a pipeline's exit code | **the gate step lied**. `check-bundle.sh` has the same shape (`file | grep -q … \|\| continue`): a lost race skips a binary's checks and still passes |
| 6 | standing | the `release` job publishes whatever matches four file patterns | presence by name | no version, size, signature, notices or content check on what is published |
| 7 | standing | Windows installers: no PE version, no manifest, no sidecar presence check | nothing | (no shipped defect yet) |
| 8 | standing | notices: Dyno bundles `THIRD_PARTY_NOTICES.md` (`d9ea2bb75`), and the notices check compares the committed file with the generator | the committed file | nothing checks that the file is in each installer, or that it is the committed one |

**Pattern.** A release ships a defect that a gate claimed to cover whenever the gate checks a build step, a build folder
or a proxy value instead of the artifact the user downloads, opened and started the way the product starts it. A related
failure is a gate step whose failure mode is "pass" (a lost pipe race, an empty loop).

**Invariant.** Every property a release promises is checked on the downloaded release artifacts (the files the `release`
job publishes), by a verifier that reads them and prints positive evidence (how many binaries were checked, and what each
had). It fails closed when the evidence is missing.

## Decision

### 1. Each app declares what its artifacts must be: `apps/<app>/release.toml`

This is data, read by one verifier, never by per-app scripts:

```toml
version = 1
[artifacts]                         # what each OS must publish; every pattern must match exactly one file
macos   = ["{Name}_{version}_universal.dmg", "{Name}.app.tar.gz", "{Name}.app.tar.gz.sig"]
windows = ["{Name}_{version}_x64-setup.exe", "{Name}_{version}_x64-setup.exe.sig"]
linux   = ["{name}_{version}_amd64.deb", "{Name}_{version}_amd64.AppImage"]

[[executable]]                      # every program each bundle must carry
name = "dyno-agent"                 # dyno-agent, dyno-agent.exe
min_bytes = 1_000_000               # a placeholder is never this size (instance 2)
macos_identifier = "net.decryptology.dyno.agent"   # exact, per Mach-O (instance 1's proxy, still checked)
probe = ["--self-test"]             # started from the extracted artifact (rule 23)

[[probe]]                           # behaviour checks, started the way the product starts them
os = "macos"
how = "launched-by-app"             # | "launchd-job" | "direct"
run = ["dyno-agent", "view"]
expect = "cut_down=false"           # the evidence line the program prints

[windows]
pe_version = "{version}"            # FileVersion and ProductVersion of every .exe
manifest = { execution_level = "asInvoker" }
crt_static = true                   # ADR 0032, every .exe and .dll

[notices]
path = { macos = "Contents/Resources/resources/THIRD_PARTY_NOTICES.md", windows = "resources/THIRD_PARTY_NOTICES.md", linux = "usr/lib/{name}/resources/THIRD_PARTY_NOTICES.md" }
same_as = "app/src-tauri/resources/THIRD_PARTY_NOTICES.md"   # byte-identical to the committed file
```

`{version}` is read from the app's single version source (rule 6), never typed. Every Mach-O in the `.app` must carry the
family Team ID and the declared identifier, and an undeclared executable fails. Executables are counted both ways.

### 2. One verifier: `./dev release verify <app> --artifacts <dir>`

This is a Paddock-class command (ADR 0022 G.2: it knows the family's bundle layout). It is written in Rust, so no gate
logic lives in shell pipelines, and instance 5's class goes away with them. It reads the artifacts directly:

| Check | How | Runs on |
|---|---|---|
| Patterns, exactly one match each; versions in file names | file names | Linux job |
| `latest.json`: versions, URLs and signatures match the artifacts | parse, `minisign` verification with the public key | Linux job |
| `.deb` and AppImage contents: executables, sizes, run paths, notices | `ar`/`tar` readers; AppImage via `--appimage-extract` in a temp folder | Linux job |
| Windows PE: version resources, manifest, imports (ADR 0032), sizes | an object-file reader crate (`object` or `pelite`, licences checked by `cargo deny` at implementation); NSIS unpacked with `7z` from the image | Linux job (no Windows host needed to read a PE) |
| macOS: DMG mounted read-only, `codesign --verify --deep --strict`, identifier and Team ID of every Mach-O, run paths, notices | `hdiutil`, `codesign -dv`, a Mach-O reader for load commands | macOS job |
| Probes: each executable's `probe`, and each `[[probe]]` started as declared (`launched-by-app` starts the app's own launcher path in a test mode; `launchd-job` uses `launchctl submit` with a unique label, then removes it) | the extracted artifact, in a temp folder | the OS's job; on macOS, the probes that show windows run in the local VM (decision "VMs are local only"), not on the CI Mac (ER-14) |

Each check prints what it saw ("4 Mach-O checked: dyno-app net.decryptology.dyno … "). If it checked fewer items than
declared, or none at all, it fails. Every failure is one `::error::` line with the cause and the next step (rule 56).

### 3. The verifier gates the release, and runs again on what was published

`app-release.yml` gets a `verify` job (matrix Linux and macOS) that `needs: [bundle]`. It downloads the uploaded artifacts,
which are exactly the files the `release` job will publish, and runs the verifier. `release` `needs: [verify]`. After
publishing, the `release` job downloads the assets from the Gitea release and runs the Linux checks once more (sizes,
hashes, `latest.json`), so a publish step that drops or corrupts a file fails the run.

The app's `bundle-check` scripts and the three macOS verify copies (ADR 0022 G.1:
`conformance/release/macos-verify-bundle.sh`, Debrief's, the demo's) are replaced by `release.toml` entries and deleted
(rule 55). `windows-crt-imports.mjs` becomes the verifier's `crt_static` check for every app.

### 4. Gates fail closed and are proved against the historical defects

Each check has a test that feeds it a broken synthetic artifact, made in the test (never a real user file, ER-14):
- a 0-byte sidecar;
- a Mach-O with an undotted identifier, made with `codesign -s - -i dyno-agent` on macOS;
- a bundle without notices, or with notices that differ from the committed file;
- a PE importing `VCRUNTIME140.dll`;
- an `.exe` whose version is not the app's;
- a missing pattern match;
- a probe that prints `cut_down=true`;
- a pipe-shaped false pass: the verifier's own "checked N of M" must fail when M is 0.

The verifier is tested once, in `paddock-dev` (rule 43). Apps only declare.

C62 (pipe-fed `grep -q` and `head` under pipefail, an error) lands on `main` from `fix/first-release-notes-mawk` before
the next tag, so the remaining shell steps of release workflows cannot lie that way.

### 5. Release-critical behaviour gets a probe, not only a property

Instance 1 shows that a correct identifier string was a proxy. When a design depends on an OS behaviour (the kernel's
socket list, a login item's registration, a launch context), the app's `release.toml` names a probe that exercises it
from the artifact, started the way the product starts it. Dyno's `dyno-agent view` (`launched-by-app` and `launchd-job`)
is the first. A design or ADR that relies on such a behaviour names its probe (a review question, added to the release
checklist).

## Checks that make the class detected

| Check | Enumerates | Fails today on |
|---|---|---|
| `release verify` in the `verify` job | every artifact, every executable, every Mach-O and PE | instances 2, 4, 6, 7, 8; instance 1 through its probe |
| Fail-closed evidence counts | every check | instance 5's shape |
| Broken-artifact tests | one per historical defect | each defect above |
| Conformance (provisional **C69**): every app with a release workflow has `release.toml`, and its workflow calls `app-release.yml` with `verify` | every `<app>-release.yml` and `debrief.yml` | every app today |
| C62 on `main` | every pipefail script and bash step | `check-bundle.sh` on `main` |
| Post-publish re-verify | the published assets | a broken publish |

## Rollout and order

1. **WP-1:** the `release.toml` schema, the verifier core with the Linux-job checks (patterns, `latest.json`, deb and
   AppImage, PE versions, manifest, CRT imports, notices), and the broken-artifact tests.
2. **WP-2:** the macOS checks (DMG, codesign per Mach-O, Team ID, run paths); delete the three macOS verify copies and
   Dyno's `check-bundle.sh`.
3. **WP-3:** probes: `--self-test` per executable from the artifact; `[[probe]]` with `launchd-job` and
   `launched-by-app`; Dyno's `view` probe (with the `fix/dyno-agent-signing-id` outcome).
4. **WP-4:** the `verify` job, `release` `needs: [verify]`, post-publish re-verify, C69.
5. **WP-5:** C62 to `main` (already written; merge only).
6. **Adoption:** Dyno first (1.1.0), then Spotter (1.0), Pitwall (1.1), Pitcrew (1.1), the demos, and Debrief (its own
   `debrief.yml`, whose CRT step is replaced).

Rule 1 orders this against the tags. WP-5 and Dyno's probe come before Dyno 1.1.0. Whether the Spotter 1.0 tag waits for
WP-1 to WP-4 is decision D1.

**Rollback.** The `verify` job can be made non-blocking (`continue-on-error`) for one tag, with a written waiver in the
release notes PR. The existing bundle checks remain until WP-2 deletes them.

## Costs (agent-days, estimates)

| WP | Days |
|---|---|
| WP-1 schema, verifier core, Linux-job checks, broken fixtures | 3.5 |
| WP-2 macOS checks, copies deleted | 2 |
| WP-3 probes (generic launch modes, Dyno's view) | 1.5 |
| WP-4 workflow jobs, post-publish, C69 | 1 |
| WP-5 C62 to `main` | 0.25 |
| Adoption: 6 apps plus 2 demos, about 0.4 each (mostly a `release.toml`) | 3 |
| **Total** | **11.25** |

## Consequences

- A release run gains a verify job (minutes, mostly downloading and unpacking), and a defect it finds stops the tag
  before publishing. The tag then needs a new patch version, as today.
- What a release promises is readable in one file per app, and reviews of a release-path change (rule 54) read it.
- Probes that start programs from the artifact must leave nothing behind (ER-14): temp folders, unique launchd labels
  removed, processes ended by identity (rule 53).
- macOS probes that need a GUI context stay in the local VM. The CI `verify` job covers everything else, and the tag
  checklist (memory "tag when green") requires the VM probe to be green.

## Alternatives considered

- **Keep per-app `check-bundle.sh` scripts and add lines.** That is how instance 5's shape and three copies of macOS
  verification arose. Rejected.
- **Verify in the `bundle` job against the build folder.** This is today's design. It misses what the upload and publish
  steps change, and build-order effects (instance 2). Rejected; the build-folder checks may stay as early warnings.
- **Trust Tauri's bundler and codesign exit codes.** Instances 1 and 2 passed both. Rejected.
- **Verify only after publishing.** That is too late: a bad release is already on the updater feed. Kept as the second
  pass only.

## Decisions for the user

- **D1:** does the Spotter 1.0 tag wait for WP-1 to WP-4 (about 8 agent-days before the tag), or does it ship with today's
  checks plus WP-5 and a manual artifact inspection, adopting the verifier in 1.0.1?
- **D2:** the instance-1 root cause is still open on `fix/dyno-agent-signing-id`: "start the agent from the app through
  launchd instead of as a child" (the open design question there). The probe in §5 checks either answer, but the choice
  is Dyno's.

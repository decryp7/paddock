# Shared CI actions

Composite actions and reusable workflows of the Paddock apps (plan X10). An app's workflow is its own test jobs plus a few
`uses:` lines. Pin them by release tag; Renovate bumps the tag together with the crates and packages (`renovate/app.json`).

```yaml
- uses: decryp7/paddock/.gitea/actions/setup-node@v0.3.0
- uses: decryp7/paddock/.gitea/actions/tauri-bundle@v0.3.0
  with: { bundles: "deb,appimage", signing-private-key: "${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}", updater-pubkey: "${{ secrets.TAURI_UPDATER_PUBKEY }}" }
```

| Action | What | Replaces |
|---|---|---|
| `app-gates` | one app's matrix leg in the monorepo: its `paddock.ci.json` commands and the Rust gates of its Cargo workspace, only the halves the change reaches (ADR 0014 §6); on Windows and macOS legs only the Rust gates that one OS cannot decide for all | the per-app `shell-demo-app`/`workbench-demo-app` jobs and, at import, each app's `build.yml` |
| `plan` | skip a nightly while main is unchanged since a green nightly; skip a main push whose commit is tagged | Debrief's `plan` job |
| `nightly-mark` | record a fully green nightly (the marker `plan` looks up) | Debrief's `nightly-mark` job |
| `release-verdict` | state in the job summary whether a release run built installers; fail a nightly, manual or tag run that built none | (new: rule 54, issues research 2026-10-30 pattern E) |
| `setup-rust` | toolchain with components and targets, sccache when the runner has it (shared Redis backend on every OS, `vars.SCCACHE_REDIS_ENDPOINT`/`_DB`; sccache is `RUSTC_WRAPPER` itself, never a script around it; a job with a build folder outside its checkout sets it with `--config build.target-dir=...`, not `CARGO_TARGET_DIR`, which sccache hashes), the Tauri Linux libraries | `dtolnay/rust-toolchain` + apt lines |
| `job-isolation` | a host job's slot and its own temp folder (see Job isolation below): `acquire` first, `release` last with `if: always()`; nothing in a container | each job's ad hoc `~/.paddock-ci` paths |
| `stable-workspace` | host runners: moves the checkout to a fixed folder of the job's slot (`src/<name>-<slot>` in the CI home) and links the old path to it (where the folder cannot move, as under act on Windows, keeps a clone there at the same commit), and sets `PADDOCK_WORKSPACE` for the cargo steps' `working-directory`, so sccache sees the same working folder every run (the work folder is hashed per job) | new |
| `build-folder` | a job's Rust build folder as cargo configuration: `target/` in a Linux container, the slot's kept `target-<name>-<slot>` on a host runner, written to the workspace's `.cargo/ci.toml` (included by its `.cargo/config.toml`), so every cargo command and `tauri build` use it; sets `PADDOCK_BUILD_FOLDER` | `CARGO_TARGET_DIR` in the app workflows, Debrief's `scripts/ci/build-folder.sh` |
| `host-tools` | the pinned cargo-nextest: kept in the CI home's shared `tools` folder on the host runners (checksum-verified, downloaded only when that version is missing), the image's on Linux; fails when the runner has another version | `taiki-e/install-action` in the crate job |
| `node-caches` | npm's download cache and the Angular build cache kept between runs (hosts: the slot's `npm-<slot>` and `angular/<name>-<slot>` kept folders; Linux containers: `npm/` and `angular/<job>` on the `/sccache` mount, shared by two runs of one job on a Linux host until containers get slots, ADR 0025) | nothing kept them before |
| `job-tmpdir` | the job's own temp folder (the runner's `$RUNNER_TEMP`) as `TMPDIR` (`TMP`, `TEMP` on Windows) and `PADDOCK_JOB_TEMP`, and on Unix a short private socket folder `PADDOCK_SOCKET_BASE` (`/tmp/pd-XXXXXXXX` from mktemp); `mode: remove` (last step, `if: always()`) removes the socket folder. One implementation, `conformance/ci/job-tmpdir.sh`: job-isolation's `acquire`/`release` run it on the host runners, this action in jobs without job-isolation (Linux containers that check for temp leaks). Nothing else sets `TMPDIR`, `TMP` or `TEMP` (a conformance test) | jobs writing to the runner user's temp folder, which local runs on the same host use too (ER-14) |
| `sccache-stats` | `sccache --show-stats` in the job log (last step, `if: always()`; composite actions have no post step) | new |
| `setup-node` | Node from `app/.nvmrc`, npm cache | `actions/setup-node` blocks |
| `strip-signing-keys` | whitespace out of the Tauri keys for the following steps (rule 25, C06) | inline `tr -d` |
| `build-jobs` | `CARGO_BUILD_JOBS` = the smaller of (the host's cores − the cores kept for other work) / the jobs the host runs at once and (its RAM − the RAM kept) / (jobs per host × 1.5 GB per rustc), divided by `split` (the builds the job runs at once: `cargo mutants --jobs N`). Runner `runner.envs`: `PADDOCK_CI_JOBS_PER_HOST`, `PADDOCK_CI_RESERVED_CORES`, `PADDOCK_CI_RESERVED_RAM_MB`, `PADDOCK_CI_RUSTC_MB`, or an exact `PADDOCK_CI_BUILD_JOBS`; else the class's layout: macOS 3 jobs (three runners on the developer Mac) with 2 cores kept for its agents and the system ((14 − 2) / 3 = 4); Linux (16 cores, 4 jobs at once) and Windows (8 cores, 2 jobs at once) unset (cargo's default, every core, as before) until an A/B of job durations decides their share. With it, `NEXTEST_TEST_THREADS` = the runner's `PADDOCK_CI_TEST_THREADS`, else the class's measured number (macOS 8), else the job's core share (not capped by RAM), divided by `split`; it overrides the nextest profile's `num-cpus`, and is left unset where `CARGO_BUILD_JOBS` is. One implementation, `conformance/ci/build-jobs.sh` (logs the host's cores and RAM). An early step of every job that runs cargo (before `job-isolation`, which builds paddock-dev); `setup-rust` runs it when no step did. Nothing else sets `CARGO_BUILD_JOBS` or a fixed cargo `-j` (conformance tests) | `CARGO_BUILD_JOBS=4` (macOS) and `=8` (Windows) in each workflow; the agents' local cap of 2 (ER-24) never applies on CI |
| `build-env` | `PADDOCK_BUILD_COMMIT` and `PADDOCK_BUILD_DATE` for the About page (`option_env!`) | nothing set them before |
| `tauri-bundle` | `tauri build` with or without updater signing (hardened runtime on only with a real Apple identity), `npm ci` at `root`, installers in `dist/`, version check; for NSIS, no shipped .exe or .dll imports the VC++ runtime (ADR 0032) | Pitwall `scripts/ci/tauri-bundle.sh`, Pitcrew's bundle step |
| `macos-verify` | `codesign --verify --deep --strict` on the .app and the DMG, sidecars with `--self-test` (rule 23) | `macos-verify-bundle.sh` copies |
| `install-test-browser` | the pinned Chrome for Testing (SHA-256 checked), `<APP>_REQUIRE_TEST_BROWSER=1` | Debrief's `install-test-browser.mjs` |
| `sidecar-self-test` | rule 23 on Linux and Windows bundles: every sidecar starts with `--self-test` (`paddock-sidecar self-test`); a .deb is unpacked, never installed | the shell demo's release job |
| `keychain` | conformance C99 (no macOS Keychain, ADR 0057): the one command of ci.yml's `c99` job and of every release path, `scope` = the app's folder on a release (ER-31) | new |
| `conformance` | `paddock-conformance` with the Paddock checkout, summary in the job | new |
| `publish-release` | release notes with the First launch block (and an app's `notes-extra`), `latest.json`, the Gitea release for `v*` or `<app>-v*` tags, `latest.json` also on the moving `<app>-latest` release | the release job's steps |
| `host-cleanup` | this job's stray processes (patterns anchored on `{tmp}`, `{build}` or `{slot}`), temp folders and build outputs on a host runner, never another job's | Pitwall `macos-cleanup.sh` |
| `prune-build-folder` | caps the job's kept build folder (of its own slot) on a host runner (incremental caches first, then the folder) | Dyno `ci-prune.sh` |

Reusable workflows: `app-release.yml` (bundles on three platforms, verify, publish; in the monorepo called from `<app>-release.yml`, see below) and
`app-fuzz.yml` (cargo-fuzz in shards for a fuzz folder; `nightly.yml` calls it for the shared crates). The repositories that once called
`app-mutants.yml`, `app-never-panics.yml`, `app-e2e.yml` and `app-licences.yml` were merged into this one (ADR 0014), so those four workflows and
the `licences`, `cache-restore` and `cache-save` actions behind them were deleted (ADR 0060: a check no reachable CI line runs is deleted or
registered); `git log` has them.

Scripts the actions run live in `conformance/ci/` and `conformance/release/` (tested with `node --test`); the actions only wire
inputs to them.

## Moving an app onto the kit (pattern, pilot: Pitcrew, WP-C1 of docs/research/2026-10-nx-vs-scripts.md)

An app on the kit has no gates workflow of its own and no copy of a kit script. What it keeps is its facts:

1. **`apps/<app>/paddock.ci.json`** is the contract of its legs in `ci.yml` (`scripts/affected.mjs` plans them, `app-gates` runs them).
   Fields: `cargo` (the Cargo workspace, relative to the app), `os` (default Linux only; add `windows-latest` and `macos-latest` for
   the code that differs there: those legs run the Rust gates only), `nextestConfig`, `prepare` and `checks` (always, Linux), `rustChecks` (Linux, only when the change reaches the Rust half), `hostRustChecks` (the same on the Windows and macOS legs, run after their clippy and tests: for a check that needs a build the tests do not make, such as a non-test `cargo check`), `ui`
   (the UI half: audit, lint, build, tests; one `cd` holds for the lines after it), `angularDir` (the Angular cache kept between runs), `lastPackages` (packages tested apart and last on Linux, left out of host legs), `notices` (`{ name, manifests, npm, out }`: the app's THIRD_PARTY_NOTICES.md, Rust through cargo-about and npm through the root lock; `app-gates` checks it is up to date on the Linux leg, `scripts/notices.sh <app>` regenerates it; paths are relative to the app) and `windowsClippy` (true: clippy also on the Windows leg, which skips it by default; for Windows-only code such as a service) and
   `e2e` (commands of the end-to-end legs, run once per shard with `$E2E_SHARD`/`$E2E_INDEX`), `e2eShards` (how many run at the same time), `e2eWebkit` (commands of a WebKit leg of its own, on main, tags, the nightly and a dispatch with `webkit`; an e2e leg runs only when the change reaches the UI half, a Rust-only change adds none), `e2eArtifacts` (the failure report folder uploaded `if: failure()`), `testBrowser` and `display` (a real browser in the tests: the pinned Chrome for Testing, a virtual display on Linux) and
   `processPatterns` (`pkill -f` patterns for the test processes `host-cleanup` ends on a host; each is anchored on a folder of the job: `{tmp}` its own temp folder, `{build}` its build folder, `{slot}` its slot in a kept folder's name, e.g. `{build}[/\\]debug[/\\]pitwall-fake-engine`, never a bare program or runner name; there are no exact process names, which would match every job on the host). Remove `workflow`: an app with a `workflow` is not a leg.
   An app with host legs needs `include = [{ path = "ci.toml", optional = true }]` in its `.cargo/config.toml` and `.cargo/ci.toml`
   in its `.gitignore` (`build-folder` fails the leg otherwise).
2. **`.gitea/workflows/<app>-release.yml`** is the thin caller (copy `pitcrew-release.yml`): triggers (`<app>-v*` tag, manual; no push to main and no schedule: `nightly.yml` builds the installers), a `plan` job (the shared `plan` action with `app`, `tag-pattern: <app>-v*`: affected.mjs for the app), a `gates` job that
   calls `ci.yml` with `app: <app>` for a tag (every leg of that app, both halves, nothing else), and a `release` job that runs only when `needs.gates.result == 'success'` (`skipped` is allowed only when there is no tag: `!startsWith(github.ref, 'refs/tags/')`, so a tag never publishes past skipped gates) and calls
   `app-release.yml` with the app's facts (`app-dir`, `cargo-dir`, `tag-prefix`, `latest-tag`, `sidecars` (empty: every `bundle.externalBin`, with `--self-test`), `setup-node` (false in the Paddock CI image), `cliff-config`, `cliff-paths`,
   `notes-extra`) and maps the family's `PADDOCK_TAURI_*` secrets to the names the kit declares. The workflow's name marks only that
   app in the plan (`appOfWorkflow`).
3. **Delete the copies**, after checking each is the kit's (diff it; port any behaviour the copy has and the kit lacks into the kit as a
   parameter or a default, with a test in `conformance/test/`, never keep the copy): `scripts/release/*` (`conformance/release/`),
   `scripts/ci/macos-verify-bundle.sh` (`conformance/release/macos-verify-bundle.sh`), `scripts/commit-lint.mjs`
   (`paddock-conformance commit-lint`, run on pull requests by `ci.yml`'s `commits` job), `scripts/export-diagrams.sh`
   (`diagrams/scripts/export-diagrams.sh`, run from the app folder with an absolute source), `scripts/check-index.mjs`
   (`conformance/ci/check-index.mjs`, called from `tauri.conf.json`'s `beforeBuildCommand`), and the old workflow.
4. **Keep what is the app's own**: `sidecar.mjs` (it uses the shared `cargoTargetDir`), bootstrap and verify scripts for the service
   (launchd, systemd, Windows), checks of its own as `checks` entries or small scripts.
5. **Update the pins**: `conformance/test/ci-actions.test.mjs` (`KIT_APPS`), the app's docs (releasing, development, macos) and its
   `AGENTS.md` test command.
6. **Verify before the deletion merges**: the local checks (`node --test scripts/*.test.mjs conformance/test/*.test.mjs`), then on the
   branch: a dispatch of `ci.yml` (everything or a base), a dispatch of `<app>-release.yml` (installers, no release; with `gates: true`
   for the app's legs), and, for the first release, a tag on a throw-away fork or the real tag.

### Changes to the kit that affect callers

- `app-release.yml`: **`cargo-dir` is now required** (the Cargo workspace whose `.cargo/config.toml` includes `ci.toml`; the build folder is the
  `build-folder` action's, `CARGO_TARGET_DIR` is gone). New inputs: `root`, `tag-prefix`, `latest-tag`, `notes-extra`, `cliff-config`,
  `cliff-paths`, `setup-node`, `bundle-check` (an app's own script run on Linux and macOS with the bundle folder, e.g. Dyno's run paths); new optional secret `APPLE_SIGNING_IDENTITY`. The release job runs for `<tag-prefix>v*` tags (was `v*`), and the
  bundle job checks the tag's version against the app's `tauri.conf.json` first (`conformance/ci/check-tag.mjs`). Artifacts are uploaded
  from `$PADDOCK_WORKSPACE/dist` (a contract test pins this for every job that uses `stable-workspace`).
- `plan` action: inputs `app`, `tag-pattern`, `nightly-marker`, `token`. `macos-verify`: `tauri-conf`. `host-cleanup`: `process-patterns`
  (`pkill -f`; scope each to the app's own temp path, never `gitea-runner`, which is the real runner). `publish-release`: `tag-prefix`,
  `latest-tag` (skipped for a pre-release tag), `notes-extra`. `tauri-bundle`: `root`, `apple-signing-identity`.

## Job isolation (ADR 0025)

Any number of jobs may run on one host at once: Pitcrew's pool of capacity-1 runners (Pitcrew ADR 0021: `mac-1` ...
`mac-N`, each its own runner folder and `.runner`), all running as one user and so sharing **one CI home**. No job may
touch another's folders, processes or ports; the job's slot derives its folders and ports. `conformance/ci/job-isolation.sh`
is the one way to them; a conformance test (`conformance/test/job-isolation.test.mjs`) fails when a workflow, action or CI
script spells a CI-home path itself, or when a host job does not take and release a slot. Do not raise a runner's
`capacity` instead: add runners to the pool.

**The runner's job folder** (verified 2026-10-28 on the Mac's `gitea-runner` v5.2.3, host mode, `workdir_parent:
~/gitea-runner/work`, and from the Windows runner's job log: v5.2.2, `C:\gitea-runner\work\<16 hex>\{hostexecutor,tmp}`):
every job gets `<workdir_parent>/<16 random hex>/` with `hostexecutor/` (the checkout), `tmp/` (`$RUNNER_TEMP`),
`tool_cache/` and `act/`, distinct for concurrent jobs and removed when the job ends
(sources: [act/runner/run_context.go](https://gitea.com/gitea/runner/src/branch/main/act/runner/run_context.go),
`startHostEnvironment`: the folder from 8 random bytes, `CleanUp` removes it;
[act/container/host_environment.go](https://gitea.com/gitea/runner/src/branch/main/act/container/host_environment.go),
`GetRunnerContext`: `"temp": e.TmpDir`, so `RUNNER_TEMP` is the job's own `tmp/`, and `Remove` ends the job's process
group before deleting the folder). The Mac's `config.yaml` sets no `runner.timeout`, so a job may run 3 h (the runner's
default, and Gitea's own job timeout:
[config.example.yaml](https://gitea.com/gitea/runner/src/branch/main/internal/pkg/config/config.example.yaml)). The
Windows runner's `runner.timeout` is not verified. Linux jobs run in a container on a Docker network of their own
(`GITEA-ACTIONS-TASK-<n>-...-network`, created and removed per job: the Linux job logs), so their ports never collide.

**What goes where, and why:**

| State | Where | Why |
|---|---|---|
| Test temp folders, the clean-up's temp sweep, `{tmp}` process patterns | the job's `$RUNNER_TEMP` (acquire sets `TMPDIR`, and `TMP`/`TEMP` on Windows, and `PADDOCK_JOB_TEMP` to it through `conformance/ci/job-tmpdir.sh`, and a short socket folder `PADDOCK_SOCKET_BASE` that release removes) | it need not outlive the job; the runner gives each job its own and deletes it after |
| Build folder (`target`), stable checkout (`src`), Angular cache, npm's cache, test browser, libduckdb, Debrief's Windows `node_modules`, sccache's disk cache | kept folders of the job's **slot**: `<ci-home>/<layout>-s<n>` (process patterns name one as `[/\\]<kept folder name>-{slot}`, at the end of the pattern; `{slot}` becomes `s<n>([^0-9]\|$)`) | a stable absolute path keeps them warm (cargo fingerprints and sccache keys hold absolute paths); one slot per concurrent job means no two jobs write one. sccache's local storage takes one server at a time ([docs/Local.md](https://github.com/mozilla/sccache/blob/main/docs/Local.md): "Multiple concurrent servers will race and cause spurious build failures"), so each slot has its own, sized `PADDOCK_CI_SCCACHE_GB` (40) / `PADDOCK_CI_SLOTS` |
| pinned tools, the paddock-dev binary | **shared** kept folders | safe for concurrent writers (renamed into place) |
| sccache server port, the WebKit job's ports | `job-isolation.sh port <name>`, declared in `conformance/ci/ports.txt` (base, width, stride) | one block per slot; a test checks no two blocks overlap each other or any end-to-end port, for every slot up to `PADDOCK_CI_SLOTS` |
| Tart VMs (service-smoke) | a host-wide VM slot under `TART_HOME` (`tools/macos-vm-smoke.sh`), VM names with the job's `PADDOCK_VM_PREFIX` | one VM at a time on the Mac (two at most, macOS's limit); the leftover check sees only the job's own VMs |

The kept folders are declared once, in `conformance/ci/kept-folders.txt` (kind, layout, `slot` or `shared`). A new cache is
one line there and `bash conformance/ci/job-isolation.sh path <kind> [<name>]` where it is used.

**Slots.** The `job-isolation` action (`acquire`, the first bash step of a host job: after Git bash on Windows, before
`stable-workspace`, `build-folder` and any kept folder) runs `./dev ci slot acquire` (ADR 0022's `paddock-dev`, built once
per host and input hash into the shared `tools` kept folder): a detached holder process takes the first free
`s1..s$PADDOCK_CI_SLOTS` (default 4) as an OS file lock on `<ci-home>/slots/s<n>.lock` and keeps it until the job's
`RUNNER_TEMP` is gone or the release asks it to stop. The kernel drops the lock when the holder dies, so there is no
stale lock and nothing is ever reclaimed. The action exports `PADDOCK_CI_SLOT` and `PADDOCK_CI_SLOT_TOKEN`. The job's last
step is `job-isolation` with `mode: release` and `if: always()` (after `host-cleanup`, `prune-build-folder` and
`sccache-stats`, which use the slot's folders and port); it fails the job when the holder died before the job ended (its
isolation was then not guaranteed). A full house waits about 2 minutes, then fails and lists the holders. Linux containers
take no slot (one job per Linux host today): acquire and release do nothing there and everything uses `s1`. Without a slot
a host job's `build-folder`, `setup-rust` and `node-caches` fail and `stable-workspace` builds in place with a warning.

**Stalled holder.** `bash conformance/ci/job-isolation.sh status` (or `./dev ci slot status --max N --root <ci-home>`)
lists each slot, the job folder its holder watches and the holder's pid. A holder whose job folder still exists after its
job ended (the runner did not delete it) keeps its slot until its cap (`DEFAULT_MAX_HOLD_SECS`: the 3 h job timeout plus 30
minutes): delete that job folder (the holder ends within a second) or end the holder's pid to free it sooner. Never delete `slots/*.lock` by hand while a holder runs.

**The CI home** is `ci-home.sh`'s (`PADDOCK_CI_HOME`, else `~/.paddock-ci`); a relocated home that is missing stops the job.
Every runner of a host uses the same CI home (same user, or the same `PADDOCK_CI_HOME`).

**Adding runners to a host:** merge the change that adds this; set `PADDOCK_CI_SLOTS` in every runner's `runner.envs` to
the pool's maximum (only above 4); then register the extra runners (Pitcrew, capacity 1 each). Keep each runner's
`capacity: 1`. Set `PADDOCK_CI_JOBS_PER_HOST` in every runner's `runner.envs` to the number of jobs the host now runs
at once, so `build-jobs` divides the cores between them.

**How disk is bounded.** A slot's kept folders are deleted only under `./dev ci slot idle-run`, which holds that slot's
lock for the whole deletion, so no job can take the slot meanwhile and a held slot is never touched. At every acquire and
release: the folders of idle slots above `PADDOCK_CI_KEEP_SLOTS` (default `PADDOCK_CI_SLOTS`), and kept folders no job
asked for in `PADDOCK_CI_ORPHAN_DAYS` (default 14; `path` renews a folder's time stamp, so folders no workflow names any
more age out, old-layout ones too). At acquire, below `PADDOCK_CI_MIN_FREE_GB` (default 30) free on the CI home's disk,
idle slots' folders go from the highest slot down until the floor is met; if it cannot be, the job fails with that
message. Each build folder is capped by `prune-build-folder` (20 GB by default, its own slot only). The first job in slot
`s1` renames a kept folder of the old unslotted layout (`target-<name>`, `src/<name>`, `angular/<name>`,
`test-browser-<name>`, `libduckdb/<name>`, `sccache`, `npm`) to its `-s1` name, so the switch keeps the caches warm and adds no
copy. Left for a one-time manual removal: the crate job's old `test-browser` folder and Debrief's old `~/.debrief-ci` on
Windows (other names now).

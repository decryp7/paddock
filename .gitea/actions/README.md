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
| `cache-restore`, `cache-save` | the restore, then save-always cache pattern: a failing run still warms the cache; saved only when the key had no exact hit | the `actions/cache` blocks in every job |
| `setup-rust` | toolchain with components and targets, sccache when the runner has it (shared Redis backend on every OS, `vars.SCCACHE_REDIS_ENDPOINT`/`_DB`; sccache is `RUSTC_WRAPPER` itself, never a script around it; a job with a build folder outside its checkout sets it with `--config build.target-dir=...`, not `CARGO_TARGET_DIR`, which sccache hashes), the Tauri Linux libraries | `dtolnay/rust-toolchain` + apt lines |
| `stable-workspace` | host runners: moves the checkout to a fixed folder under `~/.paddock-ci/src` and links the old path to it (where the folder cannot move, as under act on Windows, keeps a clone there at the same commit), and sets `PADDOCK_WORKSPACE` for the cargo steps' `working-directory`, so sccache sees the same working folder every run (the work folder is hashed per job) | new |
| `build-folder` | a job's Rust build folder as cargo configuration: `target/` in a Linux container, a kept `~/.paddock-ci/target-<name>` on a host runner, written to the workspace's `.cargo/ci.toml` (included by its `.cargo/config.toml`), so every cargo command and `tauri build` use it; sets `PADDOCK_BUILD_FOLDER` | `CARGO_TARGET_DIR` in the app workflows, Debrief's `scripts/ci/build-folder.sh` |
| `host-tools` | the pinned cargo-nextest: kept under `~/.paddock-ci/tools` on the host runners (checksum-verified, downloaded only when that version is missing), the image's on Linux; fails when the runner has another version | `taiki-e/install-action` in the crate job |
| `node-caches` | npm's download cache and the Angular build cache kept between runs (hosts: `~/.paddock-ci`; Linux containers: `npm/` and `angular/<job>` on the `/sccache` mount) | nothing kept them before |
| `sccache-stats` | `sccache --show-stats` in the job log (last step, `if: always()`; composite actions have no post step) | new |
| `setup-node` | Node from `app/.nvmrc`, npm cache | `actions/setup-node` blocks |
| `strip-signing-keys` | whitespace out of the Tauri keys for the following steps (rule 25, C06) | inline `tr -d` |
| `build-env` | `PADDOCK_BUILD_COMMIT` and `PADDOCK_BUILD_DATE` for the About page (`option_env!`) | nothing set them before |
| `tauri-bundle` | `tauri build` with or without updater signing (hardened runtime on only with a real Apple identity), `npm ci` at `root`, installers in `dist/`, version check | Pitwall `scripts/ci/tauri-bundle.sh`, Pitcrew's bundle step |
| `macos-verify` | `codesign --verify --deep --strict` on the .app and the DMG, sidecars with `--self-test` (rule 23) | `macos-verify-bundle.sh` copies |
| `install-test-browser` | the pinned Chrome for Testing (SHA-256 checked), `<APP>_REQUIRE_TEST_BROWSER=1` | Debrief's `install-test-browser.mjs` |
| `licences` | `cargo deny`, `npm audit`, commit messages on pull requests | the `licences` job in five workflows |
| `sidecar-self-test` | rule 23 on Linux and Windows bundles: every sidecar starts with `--self-test` (`paddock-sidecar self-test`); a .deb is unpacked, never installed | the shell demo's release job |
| `conformance` | `paddock-conformance` with the Paddock checkout, summary in the job | new |
| `publish-release` | release notes with the First launch block (and an app's `notes-extra`), `latest.json`, the Gitea release for `v*` or `<app>-v*` tags, `latest.json` also on the moving `<app>-latest` release | the release job's steps |
| `host-cleanup` | stray processes, temp folders and build outputs on a host runner | Pitwall `macos-cleanup.sh` |

Reusable workflows: `app-release.yml` (bundles on three platforms, verify, publish; in the monorepo called from `<app>-release.yml`, see below), `app-fuzz.yml` (cargo-fuzz shards from a JSON list), `app-mutants.yml` (cargo-mutants, weekly and on the changed lines) and `app-never-panics.yml` (the never-panics tests with many cases; plan X41), `app-e2e.yml` (Chrome for Testing,
Playwright), `app-licences.yml`. They need the Gitea on dev.decryptology.net to resolve cross-repository `uses:` (plan
decision D7: a one-hour spike before the first app adopts them); if it does not, an app uses the composite actions only,
which are the same steps.

Scripts the actions run live in `conformance/ci/` and `conformance/release/` (tested with `node --test`); the actions only wire
inputs to them.

## Moving an app onto the kit (pattern, pilot: Pitcrew, WP-C1 of docs/research/2026-10-nx-vs-scripts.md)

An app on the kit has no gates workflow of its own and no copy of a kit script. What it keeps is its facts:

1. **`apps/<app>/paddock.ci.json`** is the contract of its legs in `ci.yml` (`scripts/affected.mjs` plans them, `app-gates` runs them).
   Fields: `cargo` (the Cargo workspace, relative to the app), `os` (default Linux only; add `windows-latest` and `macos-latest` for
   the code that differs there: those legs run the Rust gates only), `nextestConfig`, `prepare` and `checks` (always, Linux), `ui`
   (the UI half: audit, lint, build, tests; one `cd` holds for the lines after it), `angularDir` (the Angular cache kept between runs) and
   `processes` (exact names `host-cleanup` kills on a host) and `processPatterns` (`pkill -f` patterns for test processes that run under another name; each must name the app's own temp folder, e.g. `pitcrew-it-`, never a bare runner name). Remove `workflow`: an app with a `workflow` is not a leg.
   An app with host legs needs `include = [{ path = "ci.toml", optional = true }]` in its `.cargo/config.toml` and `.cargo/ci.toml`
   in its `.gitignore` (`build-folder` fails the leg otherwise).
2. **`.gitea/workflows/<app>-release.yml`** is the thin caller (copy `pitcrew-release.yml`): triggers (push to main, `<app>-v*` tag,
   nightly, manual), a `plan` job (the shared `plan` action with `app`, `tag-pattern: <app>-v*`: affected.mjs for the app, and the skip of a main push whose commit carries the tag), a `gates` job that
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
  `cliff-paths`, `setup-node`; new optional secret `APPLE_SIGNING_IDENTITY`. The release job runs for `<tag-prefix>v*` tags (was `v*`), and the
  bundle job checks the tag's version against the app's `tauri.conf.json` first (`conformance/ci/check-tag.mjs`). Artifacts are uploaded
  from `$PADDOCK_WORKSPACE/dist` (a contract test pins this for every job that uses `stable-workspace`).
- `plan` action: inputs `app`, `tag-pattern`, `nightly-marker`, `token`. `macos-verify`: `tauri-conf`. `host-cleanup`: `process-patterns`
  (`pkill -f`; scope each to the app's own temp path, never `gitea-runner`, which is the real runner). `publish-release`: `tag-prefix`,
  `latest-tag` (skipped for a pre-release tag), `notes-extra`. `tauri-bundle`: `root`, `apple-signing-identity`.

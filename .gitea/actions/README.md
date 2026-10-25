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
| `plan` | skip a nightly while main is unchanged since a green nightly; skip a main push whose commit is tagged | Debrief's `plan` job |
| `nightly-mark` | record a fully green nightly (the marker `plan` looks up) | Debrief's `nightly-mark` job |
| `cache-restore`, `cache-save` | the restore, then save-always cache pattern: a failing run still warms the cache; saved only when the key had no exact hit | the `actions/cache` blocks in every job |
| `setup-rust` | toolchain with components and targets, sccache when the runner has it (shared Redis backend on every OS, `vars.SCCACHE_REDIS_ENDPOINT`/`_DB`; a wrapper keeps `CARGO_TARGET_DIR` out of the cache key), the Tauri Linux libraries | `dtolnay/rust-toolchain` + apt lines |
| `sccache-stats` | `sccache --show-stats` in the job log (last step, `if: always()`; composite actions have no post step) | new |
| `setup-node` | Node from `app/.nvmrc`, npm cache | `actions/setup-node` blocks |
| `strip-signing-keys` | whitespace out of the Tauri keys for the following steps (rule 25, C06) | inline `tr -d` |
| `build-env` | `PADDOCK_BUILD_COMMIT` and `PADDOCK_BUILD_DATE` for the About page (`option_env!`) | nothing set them before |
| `tauri-bundle` | `tauri build` with or without updater signing, installers in `dist/`, version check | Pitwall `scripts/ci/tauri-bundle.sh` |
| `macos-verify` | `codesign --verify --deep --strict` on the .app and the DMG, sidecars with `--self-test` (rule 23) | `macos-verify-bundle.sh` copies |
| `install-test-browser` | the pinned Chrome for Testing (SHA-256 checked), `<APP>_REQUIRE_TEST_BROWSER=1` | Debrief's `install-test-browser.mjs` |
| `licences` | `cargo deny`, `npm audit`, commit messages on pull requests | the `licences` job in five workflows |
| `sidecar-self-test` | rule 23 on Linux and Windows bundles: every sidecar starts with `--self-test` (`paddock-sidecar self-test`); a .deb is unpacked, never installed | the shell demo's release job |
| `conformance` | `paddock-conformance` with the Paddock checkout, summary in the job | new |
| `publish-release` | release notes with the First launch block, `latest.json`, the Gitea release | the release job's steps |
| `host-cleanup` | stray processes, temp folders and build outputs on a host runner | Pitwall `macos-cleanup.sh` |

Reusable workflows: `app-release.yml` (bundles on three platforms, verify, publish), `app-fuzz.yml` (cargo-fuzz shards from a JSON list), `app-mutants.yml` (cargo-mutants, weekly and on the changed lines) and `app-never-panics.yml` (the never-panics tests with many cases; plan X41), `app-e2e.yml` (Chrome for Testing,
Playwright), `app-licences.yml`. They need the Gitea on dev.decryptology.net to resolve cross-repository `uses:` (plan
decision D7: a one-hour spike before the first app adopts them); if it does not, an app uses the composite actions only,
which are the same steps.

Scripts the actions run live in `conformance/ci/` and `conformance/release/` (tested with `node --test`); the actions only wire
inputs to them.

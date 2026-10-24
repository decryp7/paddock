# @decryp7/paddock-conformance

One check every Paddock app runs (docs/conformance-proposal.md). Pure ESM, no dependencies, no network except the optional
Gitea API call of the tag preflight. Not published yet.

```sh
node conformance/bin/paddock-conformance.mjs --app pitcrew [--root <app checkout>] [--only C04,C06] [--json] \
    [--out conformance.json] [--summary summary.md] [--bundle <dir>] [--artifacts <dir>] [--paddock <paddock checkout>]
node conformance/bin/paddock-conformance.mjs --app pitcrew --preflight v1.2.3 --api https://host/api/v1 --repo owner/name
node conformance/bin/paddock-conformance.mjs run -- cargo test        # C14 hermetic wrapper
node conformance/bin/paddock-conformance.mjs commit-lint --range origin/main..HEAD   # C23, part 1
node conformance/bin/paddock-conformance.mjs list
```

Exit status: 0 no unwaived error, 1 findings, 2 usage. `GITHUB_STEP_SUMMARY` receives the Markdown summary when set.

## Checks

| Id | What | Needs |
|---|---|---|
| C03 | icons equal `icons/<app>/tauri` of Paddock and fill the canvas; CI runs an icon check | `--paddock` for the hash part |
| C04 | macOS signing: static (hardened runtime off while ad-hoc, workflow verifies .app and DMG) and, with `--bundle`, the real bundle | `--bundle` for the bundle part |
| C06 | every step exporting a Tauri key strips whitespace first | |
| C07 | version agreement; release job needs the gates; with `--preflight vX.Y.Z` the tag preflight | `--api --repo` for the CI-green part |
| C08 | the shared release scripts are used; with `--artifacts` the release files | `--artifacts` |
| C14 | tests run hermetically (`run --`); workflows that run tests without it are warned | |
| C15 | no test or script uses the installed Chrome; CI requires the test browser | |
| C16 | no spec or mock hard-codes the current app version | |
| C23 | commit messages (no Co-Authored-By, no AI attribution); `commit-lint` CLI | a git checkout |

## Configuration

`paddock.conformance.json` (optional): `app`, `appDir`, `tauriDir`, `workflowsDir`, `sidecars` (`[{ "name": "x-worker", "args": ["--self-test"] }]`),
`nonGates` (job ids the release job need not wait for), `commitRange`.

`paddock.waivers.toml`:

```toml
[[waiver]]
check = "C16"
path = "app/src/app/core/mock/*"   # exact path, or a prefix ending in *
reason = "mock data of the parser registry, not the app version"
expires = "2026-12-01"
```

A waiver past its date, or without a reason or date, is an error itself (W001, W002). No silent skips.

## Release scripts (`release/`)

`publish.mjs`, `latest-json.mjs` (the apps' copies, unified; output equals Pitwall's), `strip-keys.mjs`
(`eval "$(node release/strip-keys.mjs)"`), `macos-verify-bundle.sh` (captures command output before matching).

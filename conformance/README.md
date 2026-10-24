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
| C13 | Rust lint preset (`lint/`): `[workspace.lints]` at least as strict, every member crate `[lints] workspace = true`, `clippy.toml` keeps every `disallowed-methods` path | `--paddock` |
| C14 | tests run hermetically (`run --`); workflows that run tests without it are warned | |
| C15 | no test or script uses the installed Chrome; CI requires the test browser | |
| C16 | no spec or mock hard-codes the current app version | |
| C23 | commit messages (no Co-Authored-By, no AI attribution); `commit-lint` CLI. With `--paddock`: the shared doc blocks of `AGENTS.md` (rules and the common sections), `docs/suite-lessons.md`, `docs/releasing.md` equal `docs/blocks` | a git checkout; `--paddock` for the blocks |
| C28 | the repo baseline (`.editorconfig`, `rustfmt.toml`, licences, `deny.toml` tables, `cliff.toml`, `lefthook.yml`, Renovate preset, tsconfig, Cargo profiles, ...) equals `templates/repo` | `--paddock` |
| C29 | each window's initial JS (from `ng build --stats-json`) is within its `windowWeight` budget in `paddock.conformance.json`; a budget above measured + 10% warns (rule 35). `window-weight [kind...] [--max kB] [--record]` prints a window's weight and records budgets | the build stats |

## Sync (X27, X43)

```sh
paddock-conformance sync --app pitcrew [--root <app>] [--paddock <paddock checkout>] [--write]
```

Shows what differs from Paddock's `templates/repo` (C28) and `docs/blocks` (C23); `--write` repairs it. A doc block sits
between `<!-- paddock:block NAME rev=XXXXXXXX -->` and `<!-- /paddock:block NAME -->`; the section an app had under the
same heading is replaced the first time, text outside the markers is never touched. The `rules` block is derived from
`docs/rules.md`. Change the text in Paddock first. Not every app section moves: put what is specific to the app in its
own sections ("Design principles in Pitcrew").

## Third-party notices (X42)

```sh
paddock-notices --app Pitcrew [--root .] [--manifest app/src-tauri/Cargo.toml ...] [--npm app] [--out app/src-tauri/resources]
                [--rust-json cargo-about.json ...] [--about about.toml] [--check]
```

Writes `THIRD_PARTY_NOTICES.md` (Rust licences from `cargo about generate` with the app's `about.toml`, npm production
dependencies from `package-lock.json` and `node_modules`), deterministic. `--check` fails when the committed file is stale;
a package none of whose licence options is permissive fails both modes. The allowed licences are one list
(`src/notices/licences.mjs`), equal to `templates/repo/deny.toml` and `about.toml` (a test keeps them equal). Install first:
`cargo install cargo-about --locked --features cli`, `npm ci --omit=dev --ignore-scripts`.

## Configuration

`paddock.conformance.json` (optional): `app`, `appDir`, `tauriDir`, `workflowsDir`, `sidecars` (`[{ "name": "x-worker", "args": ["--self-test"] }]`),
`nonGates` (job ids the release job need not wait for), `commitRange`, `appName` (display name for templates; default the id with a capital),
`docBlocks` (`{ "AGENTS.md": ["rules", ...] }` replaces the default targets), `windowWeight` (`{ stats, windows: { main: { maxKb, entries? } } }`), `baselineSkip` (template paths this app does not take; prefer a waiver).

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

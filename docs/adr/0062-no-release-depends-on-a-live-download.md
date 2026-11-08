# 62. No release depends on a live third-party download: one inventory of build inputs, pinned by hash, served from our own store, and a check over every fetch

- Status: Proposed (2026-11-08; ER-21 redesign, user decision 2026-11-08 "do all redesigns now"; revision 1, for independent review)
- Date: 2026-11-08
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-27 / rule 56 (logs explain every failure; one
  `::error::` per failing step), ER-33 / rule 71 (a check counts only if its probe can see the thing), ER-14 (tests never
  touch the machine), ER-15 (release only from a tag in CI), rule 54 (a change to a release path builds the installers),
  rule 55 and ADR 0034 (one implementation per mechanism), ADR 0022 (tooling in Rust, `./dev`; floating `stable`), ADR 0025
  (CI job isolation and kept folders), ADR 0028 (`paddock-deps`: licence policy, notices), ADR 0039 (release gates inspect
  the artifacts), ADR 0049 (feature switches), Dyno ADR 0027 §7 (smartmontools is never bundled or mirrored), Sighter
  ADR 0001 (weights mirrored on Gitea).
- Scope: what a **build, test or release step** downloads: CI jobs, the scripts they run, the CI image build and the
  local VM harness. Downloads that a shipped app makes for its user (Chromium, llama.cpp and ffmpeg runtimes, models,
  smartmontools, the WebView2 bootstrapper, updates) are product features under `paddock-runtime`, `paddock-download` and
  ADR 0051. They are out of scope here, except where a test or the VM harness triggers one.

## Pattern

| When | What failed | Why |
| --- | --- | --- |
| 2026-11-08, `dyno-release` runs 7693 and 7694 (coordinator log `2026-10-29-overnight.md:1340`, `:1344`) | The Linux bundle of the Dyno 1.1.0 release candidate failed twice. The macOS and Windows bundles passed. | `tauri build --bundles deb,appimage` downloads `linuxdeploy` and `AppRun` from `github.com/tauri-apps/binary-releases` at bundle time. GitHub answered HTTP 504 for about an hour, both to CI and to the Mac, while githubstatus said "operational". The job log had no `::error::` naming the download (an ER-27 gap). The tag waited for GitHub to recover. |
| 2026-11-08, Windows VM rounds of the Dyno 1.1.0 release record (user report) | Rows of the VM harness broke because the disk-health tool's download timed out. | The harness clicks Dyno's "Download and install smartmontools" button (`scripts/vm-smoke/win-helper.mjs:130` on `test/dyno-vm-harness-1.1-win`). The helper then fetches the pinned installer live from `github.com/smartmontools/.../releases` (`apps/dyno/crates/dyno-agent/tools/tool-pins.toml`), in every round. |

A survey of `origin/main` at `9e18230c5` (section "Inventory today") found 25 more inputs that a build, test or release
step fetches from a third-party host at run time. At least five of them are on a release path with no hash, or with no
pinned version at all.

**The pattern: build inputs are undeclared.** Each step that needs a file from outside the repository fetches it in its
own way, from the publisher, at the moment it runs. Some steps already pin and verify their files (cargo-nextest,
sccache, libduckdb, Chrome for Testing, the CI image's tools). But each does it with its own copy of "download, check
SHA-256, cache", and the copies differ:

- They keep their caches in different places.
- They fail in different ways: `fetch-verified.sh` writes to stderr with no `::error::`, the sccache step warns and
  carries on, and Tauri's bundler reports neither the URL nor any detail of a hash error.
- Nobody keeps a list of them.

Nothing stops a new step from adding a fetch, and nothing shows which releases depend on which hosts. So any outage of
GitHub, Google storage, Microsoft's CDNs, crates.io or a publisher's mirror can block a release, and we find out on the
day.

The fix is not more retries: a 504 that lasts an hour outlasts any retry. It is a mechanism that makes the class
impossible on release paths and detected everywhere else.

## Context

### What a fetch costs today

- **Release paths.** Every Tauri app with an AppImage builds its Linux bundle in a fresh `paddock-ci:v2` container: Dyno,
  Pitwall, Pitcrew and Spotter through `app-release.yml`, and the shell and workbench demos through their own release
  workflows. Because the container is fresh, Tauri's tools folder (`~/.cache/tauri`) is empty and the helpers are
  downloaded on every run.
- **Tauri's own downloader.** These facts come from tauri-bundler at `tauri-cli-v2.12.3`, the version the root lock pins.
  They were read from `crates/tauri-bundler/src/bundle/linux/appimage/linuxdeploy.rs`, `bundle/windows/nsis/mod.rs` and
  `utils/http_utils.rs` on 2026-11-08.
  - It downloads three files:
    - `AppRun-x86_64` from `binary-releases/releases/download/apprun-old/`;
    - `linuxdeploy-x86_64.AppImage` from `.../download/linuxdeploy-07333c6/`;
    - the optional `linuxdeploy-plugin-appimage-x86_64.AppImage` from
      `github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/`. This is a **moving** build. If
      this download fails, Tauri logs it and linuxdeploy falls back to its built-in plugin. Either way, the AppImage
      runtime we ship can change without any change of ours.
  - It checks **no hash** for any Linux helper. It reuses any file already in the tools folder (`if !path.exists()`).
  - The tools folder is `dirs::cache_dir()/tauri`. That is `$XDG_CACHE_HOME` or `$HOME/.cache` on Linux and
    `%LOCALAPPDATA%` on Windows. It is `<target>/.tauri` instead when the configuration sets a local tools folder.
  - On every run, Tauri **zeroes three bytes at offset 8** of the cached linuxdeploy AppImage (`dd ... seek=8`). After its
    first use, the cached file no longer matches its published hash.
  - Two environment variables rewrite `https://github.com/...` download URLs: `TAURI_BUNDLER_TOOLS_GITHUB_MIRROR_TEMPLATE`
    (placeholders `<owner>`, `<repo>`, `<version>`, `<asset>`) and `TAURI_BUNDLER_TOOLS_GITHUB_MIRROR`. The downloader
    sets no retry and no timeout, and its errors carry no URL.
  - On Windows it downloads `nsis-3.13.zip` and `nsis_tauri_utils.dll` 0.5.3, pinned by SHA-1 in Tauri, and keeps them
    in `%LOCALAPPDATA%\tauri\NSIS` on the host runner. A fresh runner, or a Tauri CLI bump that changes the NSIS pin,
    downloads them during a release.
  - WebView2: our apps leave `webviewInstallMode` at its default (`downloadBootstrapper`), so nothing is fetched at
    bundle time. The installer fetches the bootstrapper on the user's PC, which is product behaviour and out of scope.
- **The CI image** already pins and verifies sccache, cargo-hakari, cargo-machete, shellcheck, libduckdb and Chrome for
  Testing (`ci-image/Dockerfile`, `fetch-verified.sh`). But it fetches them from the publishers whenever the image is
  built. It carries none of the Tauri helpers and none of the release tools (git-cliff, cargo-cyclonedx, cargo-about).
- **The stores we already have:**
  - the runners' kept folders (ADR 0025; `conformance/ci/kept-folders.txt` has a shared `tools` folder);
  - the Linux runner's host mount `/sccache`, whose subfolders already hold the npm and Angular caches;
  - the Gitea container registry (`garage`, `paddock-ci`);
  - Gitea generic packages, already used for Sighter's weights (`decryp7/sighter-weights` 1.11.2, mirrored on 2026-10-30
    with SHA-256 pins);
  - the user's `actions/cache` server (`172.16.1.188:8089`).

### Inventory today (origin/main 9e18230c5)

"Release" means the step runs in a tag's release run or in its gates. "Cached" means a later run on the same runner does
not fetch the file again.

| # | Input | Where | Host | Version pinned | Hash | Cached | Release |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A1 | linuxdeploy (Tauri AppImage helper) | Tauri bundler via `conformance/ci/tauri-bundle.sh` (`app-release.yml` bundle ubuntu; demo releases) | github.com | by tag name | none | no (fresh container) | yes |
| A2 | AppRun | same | github.com | by tag name | none | no | yes |
| A3 | linuxdeploy-plugin-appimage | same (optional; silent fallback) | github.com | **no** (`continuous`) | none | no | yes |
| A4 | NSIS 3.13, nsis_tauri_utils 0.5.3 | Tauri bundler, Windows host | github.com | yes | SHA-1 (Tauri) | host folder | yes |
| B1 | cargo-nextest 0.9.149 | `.gitea/actions/host-tools` | github.com | yes | SHA-256 | kept `tools` | gates |
| B2 | sccache 0.19.0 | `.gitea/actions/setup-rust` | github.com | yes | SHA-256 | tool cache | gates (degrades to no cache) |
| B3 | Rust toolchain `stable` | `setup-rust` (`rustup toolchain install`); `dtolnay/rust-toolchain@stable` (debrief-mutants, -corpus, -bench, workbench-demo-release) | static.rust-lang.org | **no** (floating, ADR 0022) | rustup's | host | yes (host bundles) |
| B4 | git-cliff | `cargo install git-cliff --locked` (`app-release.yml:386`, `debrief.yml:1075`) | crates.io, compiled | **no** | lock only | if on PATH | **yes** (release notes) |
| B5 | cargo-cyclonedx | `cargo install` (`debrief.yml:1095`) | crates.io, compiled | **no** | lock only | if on PATH | **yes** (SBOM) |
| B6 | cargo-about 0.9.2 | `scripts/notices.sh` (app-gates notices check, Linux leg) | crates.io, compiled | yes | lock only | if on PATH (container: never) | gates |
| B7 | tauri-driver | `cargo install` (`debrief.yml:666`) | crates.io, compiled | **no** | lock only | if on PATH | Debrief full runs and tags |
| B8 | cargo-mutants, cargo-fuzz | `app-mutants.yml`, `app-fuzz.yml`, `debrief-mutants.yml`, `debrief-fuzz.yml` | crates.io, compiled | **no** | lock only | if on PATH | nightly |
| B9 | cargo-about, cargo-deny, cargo-hakari via `taiki-e/install-action@v2` | `app-licences.yml:57`, `publish.yml:76` | github.com | partly | the action's | no | licences, shared publish |
| C1 | Chrome for Testing 156 | `install-test-browser` action (`ci.yml:488`, `:646`; `debrief.yml:292` with Debrief's own pin in `runtime.rs`) | storage.googleapis.com | yes | SHA-256 | kept folder; image on Linux | gates |
| C2 | Chrome for Testing (Spotter's own) | `apps/spotter/scripts/install-test-browser.mjs`, `chromium-manifest.toml` | storage.googleapis.com | yes | SHA-256 | home cache | gates |
| C3 | Playwright WebKit and Chromium | `ci.yml:207` (host, kept per version); `shell-demo-release.yml:66`, `workbench-demo-release.yml:66` (`--with-deps`: also apt) | Playwright CDN, Ubuntu archive | by Playwright version | none | host only | demo releases |
| C4 | msedgedriver | `apps/debrief/app/e2e-tauri/scripts/fetch-msedgedriver.mjs` (`debrief.yml:670`) | msedgedriver.microsoft.com | follows the runner's WebView2 (moves with Windows updates) | none | stamp file | Debrief full runs and tags |
| C5 | libduckdb 1.5.6 | three copies: `fetch-libduckdb.sh` (CI), `xtask libduckdb` (dev), `app/scripts/libduckdb.mjs` (bundle) | github.com | yes | SHA-256 | host slot folder; Linux: image when versions match, else `RUNNER_TEMP` every run | **yes** (Debrief ships it) |
| D1 | Actions: `actions/checkout@v4`, `upload-artifact`, `download-artifact`, `cache`, `setup-node@v4`, `docker/*`, `dtolnay/*`, `taiki-e/*` | every workflow | the runner's default actions host (not measured: github.com or a mirror) | tags, a few SHAs | git | runner's action cache | every job |
| D2 | Node | `actions/setup-node@v4` (via `.gitea/actions/setup-node`) | nodejs.org / GitHub manifest | `.nvmrc` major | the action's | tool cache | yes |
| D3 | `catthehacker/ubuntu:act-latest` | `ci-image.yml:25` | Docker Hub | **no** (`latest`) | none | runner | image build |
| D4 | CI image build inputs | `ci-image/Dockerfile` (`fetch-verified`, `npx playwright install --with-deps`, apt) | github.com, googleapis, Playwright CDN, Ubuntu | yes (apt no) | SHA-256 (apt: archive signatures) | image | image build only |
| E1 | npm packages | `npm ci` (root lock, sha512 integrity) | registry.npmjs.org | lock | sha512 | kept npm cache | yes (on a lock change) |
| E2 | Rust crates | cargo (lock checksums) | index.crates.io, static.crates.io | lock | SHA-256 | cargo home | yes (on a lock change) |
| E3 | apt packages | `setup-rust:195` (outside the image) | Ubuntu archive | **no** | archive signatures | no | rarely |
| F1 | smartmontools 7.5 (Dyno helper's download) | VM harness rows (`win-helper.mjs`) | github.com | yes | SHA-256 and size | **no** (every round) | VM release rows |
| F2 | VM base packages | base build scripts (`scratch/dyno-vm-checks/linux-base-*.sh`) | distribution archives | no | archive signatures | snapshot | base build only |
| F3 | Sighter weights | Gitea generic package | **ours** | yes | SHA-256 | (not applicable) | (the model to follow) |
| F4 | DirectML redistributable (Sighter, on its branch) | build step | nuget.org | yes | SHA-256 | (not measured) | Sighter release, when it lands |
| G | Runner prerequisites, never fetched: 7-Zip (`conformance/release/check-per-machine-installer.mjs:55`), gpg (`verify-tool-pin.sh`), Xcode tools | release gates | (none) | (none) | (none) | installed | yes |

Some Rust and TypeScript tests name third-party URLs, but only as data against local mock servers
(`debrief-ai/tests/it/hf_download.rs`, `runtimes.rs`). ER-14 already covers tests, and section 6 adds a probe that shows
they really stay local.

## Decision

### 1. The rule (proposed rule 72; number assigned at landing)

**No build, test or release step contacts a third-party host.**

- Every file a step needs from outside the repository is an entry in one inventory, `build-inputs.toml`, pinned by
  version, size and SHA-256.
- It is served from our own store: a runner's kept folder, the CI image, or a Gitea mirror.
- Only `./dev inputs mirror` contacts a publisher's host. A person runs it, or dispatches it, to fill the store.
- If a gate or release cannot find an input in the store, it fails at once with an `::error::` that names the input. It
  never falls back to the publisher.

Some classes are exempt. Each is declared in the inventory (section 2), so the exemption itself stays visible:

- `registry`: npm and crates.io, already pinned by lockfile hashes and cached (D1).
- `toolchain`: ADR 0022's floating Rust `stable`, fetched only by a warm-up job and never by a gate (D2).
- `prerequisite`: tools the user installs on the runner (7-Zip, gpg, Xcode tools). They are checked to be present and
  never fetched.
- `image-build` and `base-build`: inputs fetched while building the CI image or a VM base, all pinned. For the jobs that
  use them, the image or the snapshot is the store.

### 2. The inventory: `build-inputs.toml`

One file at the repository root lists every outside file that any workflow, script, image or harness uses. An entry
reuses the shape of `paddock-runtime`'s catalogue (`chromium.toml`: one `[[platform]]` per system with `url`, `size` and
`sha256`), so one parser and one set of tests serve both (ER-22).

```toml
format = 1

[[input]]
id = "tauri-appimage-linuxdeploy"
kind = "bundler-helper"             # tool | bundler-helper | test-browser | library | action | image | registry
                                    # | toolchain | prerequisite | vm-asset | model
version = "07333c6"
tied_to = { npm = "@tauri-apps/cli", version = "2.12.3" }   # a bump of the CLI fails the check until this is re-read
source = "https://github.com/tauri-apps/binary-releases"
licence = "MIT"                     # SPDX, judged by the family policy (ADR 0028, crates/deny.toml); checked in WP-1
ships = false                       # true: some of its bytes end up in an installer, so the notices must list it
redistribute = true                 # false: never on Gitea (GPL tools, D4); host-side store only
mirror = "build-inputs/tauri-appimage/07333c6"   # Gitea generic package/version
used_by = [".gitea/workflows/app-release.yml#bundle", ".gitea/workflows/shell-demo-release.yml#bundle",
           ".gitea/workflows/workbench-demo-release.yml#bundle"]
release_path = true
seed = { tool = "tauri", file = "linuxdeploy-07333c6-x86_64.AppImage" }   # where the consumer expects it

[[input.platform]]
key = "linux-x86_64"
url = "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-07333c6/linuxdeploy-x86_64.AppImage"
size = 0                            # filled in by `./dev inputs mirror` after approval; reviewed in the diff
sha256 = ""
```

Rules for entries:

- **Moving upstream builds are snapshotted.** A `continuous`, `latest` or `master` URL is allowed only as provenance. The
  entry pins the exact file we took (its size and SHA-256), and the mirror serves that file. The linuxdeploy AppImage
  plugin is pinned this way, so the AppImage runtime we ship stops changing under us.
- **Runner-matched inputs** such as msedgedriver are pinned against a pinned counterpart (D5), never against whatever the
  runner happens to have.
- **`ships = true` entries feed the notices.** AppRun and the AppImage runtime end up inside every AppImage, yet no app's
  notices list them today. These entries go to `paddock-deps`' notices (ADR 0028).
- An entry's licence must pass the family policy. `redistribute = false` keeps an entry off Gitea.

### 3. The store: three tiers and one resolver

| Tier | What | Who fills it |
| --- | --- | --- |
| 1. Kept folder | Content-addressed `tools/inputs/<sha256>/<file>` in each host runner's shared kept `tools` folder (ADR 0025, `job-isolation.sh path tools`). For Linux containers, the runner's existing host mount gets an `inputs/` subfolder beside `npm/` and `angular/`; this needs a runner `config.yaml` change, which is shown to the user first. | `./dev inputs ensure`, when tiers 2 and 3 miss. |
| 2. CI image | Linux tools baked in at fixed paths, each checked by `paddock-ci-verify`. The image's ARG pins are generated from the inventory (`./dev inputs dockerfile-args`), and a test checks that they agree. | The image build (`image-build` class). |
| 3. Gitea mirror | Generic packages under `build-inputs/<id>/<version>/<file>` on `dev.decryptology.net`. Each version is uploaded once and never re-uploaded (the lesson from Sighter's weights). Only `redistribute = true` entries. | `./dev inputs mirror <id>`, run by a person or by a manually dispatched workflow. |

The resolver tries tier 1, then tier 2, then tier 3. It verifies size and SHA-256 every time it hands out a file. In CI
it never goes past tier 3.

The `actions/cache` server is not a tier (D8). It is keyed and evicts entries, and it gives containers nothing the host
mount does not.

### 4. One fetcher: `./dev inputs`

A module of `tools/paddock-dev` (ADR 0022: tooling in Rust; external commands go through `paddock-proc`), with these
commands:

- `./dev inputs ensure <id>... [--seed]` resolves an input, verifies it, places it and prints its path. With `--seed` it
  places the file where its consumer looks (the entry's `seed`). It writes one line per input to the job summary: id,
  version, tier, bytes and milliseconds.
- `./dev inputs seed tauri` copies the Tauri helpers for the locked CLI into the folder Tauri reads. It computes that
  folder the way `dirs::cache_dir()` does, and logs it.
  - It makes a **fresh copy on every run**, because Tauri patches the cached linuxdeploy in place (see Context) and a
    reused copy can no longer be verified.
  - On Windows it seeds `%LOCALAPPDATA%\tauri\NSIS` the same way, so neither a fresh runner nor a CLI bump fetches NSIS
    during a release.
  - It also sets `TAURI_BUNDLER_TOOLS_GITHUB_MIRROR_TEMPLATE` to the Gitea mirror. A helper we failed to seed then comes
    from Gitea, not GitHub, and the egress guard (section 6) reports it.
- `./dev inputs mirror <id> [--new-pin]` is the only command that contacts a publisher.
  - It refuses to run in CI unless the mirror workflow dispatched it.
  - With an existing pin, it verifies the download against the pin. With `--new-pin`, it prints the size and SHA-256
    for review in the diff.
  - It then uploads the file to Gitea (tier 3). A `redistribute = false` entry goes to the named host-side store instead.
- `./dev inputs list [--job <workflow#job>] [--release]` shows what a job, or the release paths, depend on.

`./dev inputs` replaces the five existing copies of "fetch, verify, cache":

- `ci-image/fetch-verified.sh`, which stays only for the image build;
- the `host-tools` action;
- `setup-rust`'s sccache step;
- libduckdb's three implementations;
- the two `install-test-browser` scripts.

Each copy stays registered in `docs/known-copies.toml` until it is gone (rule 55).

### 5. Errors and logs (ER-27)

Every failed input prints one annotation in one format, then exits non-zero:

```
::error title=build input tauri-appimage-linuxdeploy::could not get tauri-appimage-linuxdeploy 07333c6 (linuxdeploy-x86_64.AppImage): not in the kept folder, not in the image, Gitea answered HTTP 404 for build-inputs/tauri-appimage/07333c6. Next: run `./dev inputs mirror tauri-appimage-linuxdeploy` on a workstation, then re-run this job.
```

- **Causes are named, never just an exit code:** the HTTP status, a connect or read timeout with its limit, a size or
  SHA-256 mismatch (expected and got), or the missing tier.
- **Timeouts are bounded:** connect 15 s; a read that stalls for 30 s fails, and long reads print progress lines. Each
  tier gets at most two retries, with backoff, on a 5xx or a timeout.
- **Fetches Tauri adds later are named too.** When `tauri build` fails, `tauri-bundle.sh` scans the tail of its output
  for a `Downloading https://...` line and names that URL in its `::error::`. A fetch that a later Tauri version adds is
  then named on its first failure.

### 6. Proof that nothing fetches: the egress guard (ER-33)

A static check cannot see a fetch that a tool makes on its own, and Tauri's helpers are exactly that. So jobs run behind
a small logging proxy, `./dev inputs egress-guard`, which starts before the first build step.

- **Setup.** `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` point at the proxy, and so does Node, through
  `NODE_USE_ENV_PROXY=1` (Node 24, the major in `.nvmrc`). `NO_PROXY` names `dev.decryptology.net`, `172.16.1.188`,
  `localhost` and `127.0.0.1`.
- **Recording and refusing.** Every CONNECT to another host is recorded with its step. In `refuse` mode the connection
  is refused, and the step's failure is annotated
  `::error::step "<name>" tried to reach github.com:443, which is not a build input`.
- **It is a tripwire, not a sandbox.** Cargo, curl, ureq (Tauri) and Node with `NODE_USE_ENV_PROXY` honour these
  variables; a program opening a raw socket would not.
- **The guard proves it can see.** Its first act is a request to a denied host, which must be recorded and refused. If
  it is not, the job summary says `egress guard: check broken`, and a release run fails (ER-33).
- **Rollout.** The guard runs in `log` mode on nightly and branch runs for one week, and the job summary lists every host
  reached. Then it switches to `refuse` on release workflows and gates (D7).
- **Registries.** `registry` entries pass while their lockfile is unchanged and a cache exists. A cache miss is logged,
  so the miss rate behind D1 is measured, not guessed.

### 7. The check: build inputs are declared (conformance, number assigned at landing)

A static check runs over `.gitea/**`, `conformance/ci/**`, `conformance/release/**`, `scripts/**`, `apps/*/scripts/**`,
every `xtask`, `ci-image/**`, the VM harness and the `scripts` of every `package.json`. It checks six things:

1. **Every fetch site maps to an inventory entry.** A fetch site is any of: `curl`, `wget`, `Invoke-WebRequest`, `iwr`,
   `fetch(`, an `https://` URL in a run step, `npx playwright install`, `npx -y`, `cargo install`,
   `rustup toolchain install`, `apt-get install`, `pip install`, a `uses:` of an action outside this repository, and a
   container `image:` line. The site must call `./dev inputs`, or carry a `# build-input: <id>` marker within three
   lines (the C54 marker style). Comments and README prose are ignored.
2. **The entry matches the site.** The entry exists, its URL matches the site's literal or template, and its `used_by`
   lists the site's workflow and job. An entry that nothing uses is reported as stale.
3. **Release paths never depend on a publisher.** A `release_path = true` entry has tier 2 or tier 3, not only the
   publisher. A `redistribute = false` entry has no `mirror`.
4. **Entries are well formed.** Every SHA-256 is 64 lowercase hex digits, every size is positive, and every licence
   passes the family policy.
5. **`tied_to` agrees with the lockfiles.** A Renovate bump of `@tauri-apps/cli` fails until the Tauri entries are
   re-read. A helper, `./dev inputs tauri-scan`, lists the download URLs found in the locked Linux CLI binary for
   comparison.
6. **The check's own tests fail on each historical instance:** a bare `cargo install git-cliff`, the unseeded AppImage
   bundle, `taiki-e/install-action`, and a `continuous` URL without a pin.

The check starts with a ratchet baseline of today's sites (section "Inventory today"), which may only shrink.

## Checks that make the class detected

| Failure | Caught by |
| --- | --- |
| A new script or step fetches from a publisher | Section 7, items 1 and 2 |
| A release path depends on the publisher's host | Section 7, item 3; the guard in `refuse` mode on release workflows |
| A tool fetches something on its own (Tauri, Playwright, an action) | Section 6 guard; `tauri-bundle.sh`'s URL scan |
| A Tauri CLI bump brings new helpers or new pins | Section 7, item 5 |
| A moving upstream build changes what we ship | Snapshot pins (section 2); SHA-256 on every hand-out |
| A fetch fails with no cause in the log | Section 5 format, unit-tested per cause |
| The guard is blind | Its self-test (section 6) |

## Work packages and estimates

The estimates are raw agent-days, without review rounds; add about 20 % for those. The first package does not depend on
the others. It can start as soon as this ADR is accepted, because the problem it fixes can block any release today.

| WP | What | Days |
| --- | --- | --- |
| **WP-1** | **AppImage helpers for every Tauri app's Linux bundle.** (a) Pin `AppRun-x86_64`, linuxdeploy `07333c6` and a snapshot of `linuxdeploy-plugin-appimage` for CLI 2.12.3, with size, SHA-256 and licence checked. These are the first `build-inputs.toml` entries, already in the final format. (b) Mirror them on Gitea. (c) Make `tauri-bundle.sh` seed a fresh, verified copy into the Tauri tools folder before `tauri build`; until `./dev inputs` exists, a small shell step reads the entries (WP-4 replaces it). (d) Set `TAURI_BUNDLER_TOOLS_GITHUB_MIRROR_TEMPLATE` to Gitea. (e) Add the `::error::` URL scan of a failed `tauri build`. (f) Seed NSIS on Windows the same way. (g) Prove in CI that the Linux bundle succeeds with GitHub refused: a dead proxy for everything except Gitea and the registries, with the self-test. (h) Add the `tied_to` test against the lock. (i) Rule 54: before landing, dispatch the release workflows, with bundles, of Dyno, Pitwall, Pitcrew, Spotter and both demos. | 2.5–4 |
| WP-2 | The inventory: the format; the parser, sharing `paddock-runtime`'s catalogue types; every row of "Inventory today" as an entry with its licence and `used_by`; the `ships` list handed to `paddock-deps`. | 2–3 |
| WP-3 | `./dev inputs ensure`, `seed`, `mirror` and `list`; the three tiers; the error format and job summary; the kept-folder entry in `kept-folders.txt`; the runner mount change, shown to the user first; the Gitea mirror of every redistributable entry. | 3–5 |
| WP-4 | Adoption. Replace the five copies, with known-copies entries until they are gone. Pin git-cliff, cargo-cyclonedx, cargo-about, tauri-driver, cargo-mutants and cargo-fuzz as prebuilt, hashed binaries in the image and the host store. Replace `taiki-e/install-action` and `dtolnay/rust-toolchain`. Serve the Playwright browsers from the store (`PLAYWRIGHT_BROWSERS_PATH`). Keep apt in the image only. Build CI image v3 with the new tools (D9). | 4–6 |
| WP-5 | The conformance check (section 7) with its ratchet baseline, its markers and a test per historical instance. | 2–3 |
| WP-6 | The egress guard: the proxy, its self-test, the `log` week, then `refuse` on release workflows and gates; the registry miss counter. | 2–4 |
| WP-7 | Actions and images. Measure the runners' default actions host. Mirror the actions we use into our Gitea and pin them by commit. For the image build's base, use a pinned digest from our registry instead of `catthehacker/ubuntu:act-latest`. Needs the user for the Gitea and runner settings. | 1–2 |
| WP-8 | Pin msedgedriver against a pinned WebView2 Fixed Version runtime for the Debrief Windows e2e (D5); move the evergreen run to nightly. | 1.5–3 |
| WP-9 | VM harness: smartmontools from a host-side store (D4); the VM base inputs listed (`base-build`); the harness reports `not measured` when an input is missing, instead of failing rows (ER-33). | 1.5–3 |
| WP-10 | The rule text, a lessons entry, an ER line proposal for `engineering-rules` (next free ER number), and the docs (`ci-image/README.md`, `.gitea/actions/README.md`). | 0.5–1 |
| | **Total** | **20.5–34** (about 25–41 with review rounds) |

Order:

1. WP-1 now.
2. WP-2 and WP-3.
3. WP-4 and WP-5 in parallel.
4. WP-6.

WP-7, WP-8 and WP-9 can run beside WP-4 once WP-3 is in. WP-10 lands with WP-5.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | npm and crates.io | (a) Exempt them as `registry`: lockfile hashes plus kept caches, with misses measured by the guard. (b) Run a pull-through proxy for npm and cargo on the home network. | (a) now; revisit if the guard shows misses on release runs |
| D2 | ADR 0022's floating Rust `stable` on host runners | (a) Gates and releases use the installed `stable` and never update it; a nightly warm-up job updates each host. (b) Keep updating in every job. | (a): the fetch leaves the gates, and ADR 0022's floating `stable` stays |
| D3 | Visibility of the Gitea `build-inputs` packages | (a) The same visibility as `sighter-weights`. (b) Private, with a read token in CI. | (a), with redistributable licences only |
| D4 | smartmontools in the VM harness (GPL; Dyno ADR 0027 §7 says it is never mirrored) | (a) A host-side store on the VM host. The harness serves it to the VM through a download-base switch that exists only in debug builds (ADR 0049 switches). The helper still checks size and SHA-256 against its own table, so the bytes cannot change. (b) Keep the live download, and report the rows `not measured` when it fails. | (a) |
| D5 | msedgedriver | (a) Gates use a pinned WebView2 Fixed Version runtime (`WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`) and its matching driver, both inventory entries. The evergreen runtime runs nightly, with the driver fetched for its version. (b) Keep following the runner's WebView2. | (a): gates stop drifting with Windows updates |
| D6 | Sighter's DirectML (decided 2026-10-30: fetched from nuget.org, not mirrored) | (a) Keep that decision: a `redistribute = false` entry in the host-side store, filled by `./dev inputs mirror`, so the release still never fetches live. (b) Mirror it on Gitea. | (a) |
| D7 | Egress guard | (a) `log` for a week, then `refuse` on release workflows and gates. (b) `log` only. | (a) |
| D8 | The `actions/cache` server | (a) Not a tier for build inputs, because it is keyed and evicts. (b) A tier for containers. | (a) |
| D9 | CI image | (a) v3 now with the build-input tools, and DuckDB 2.0 in a later v4. (b) Wait for DuckDB 2.0. | (a) |

## Consequences

- A release no longer waits for GitHub, Google, Microsoft or a publisher's mirror. An outage costs nothing until someone
  bumps a pin.
- Bumping a pin becomes a deliberate step: `./dev inputs mirror --new-pin`, a reviewed diff of sizes and hashes, and an
  upload. Renovate PRs for pinned tools fail the check until that is done.
- The AppImage runtime and AppRun we ship become pinned, listed and credited.
- The Gitea mirror grows by the tools we use. The largest are Chrome for Testing, at about 200 MB per platform, and the
  Playwright browsers. Old versions are pruned after the last tag that used them.
- The release path gains one component, the store. If it fails, the error names it. Gitea is already a dependency of
  every release (registry, releases, the CI image).

## Alternatives considered

- **More retries and longer timeouts.** The 504s lasted about an hour; retries only move the failure.
- **Tauri's mirror variable alone.** It moves the host, but adds no hash check for the Linux helpers and does not pin the
  `continuous` plugin. It stays only as a fallback behind the seeded copy.
- **Binaries in git (or LFS).** It bloats the repository in every worktree, and the family does not use LFS.
- **A general pull-through proxy (Nexus, Artifactory) for everything.** It is another service to run and secure, for
  files that change a few times a year. It is reconsidered under D1 if the registries prove a problem.
- **`actions/cache` as the store.** It is keyed, it evicts, and host runners' kept folders cannot see it (D8).
- **Pinning in each script, as today.** That is the pattern this ADR ends: five copies, no list and no check.

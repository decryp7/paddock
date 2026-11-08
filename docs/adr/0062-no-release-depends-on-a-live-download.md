# 62. No release depends on a live third-party download: one inventory of build inputs, pinned by hash, served from our own private store, and a check over every fetch

- Status: Proposed (2026-11-08; ER-21 redesign, user decision 2026-11-08 "do all redesigns now"; revision 2 answers the
  first independent review: P1-1 to P1-4 and the P2s, see "Revision 2")
- Date: 2026-11-08
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-27 / rule 56 (logs explain every failure; one
  `::error::` per failing step), ER-33 / rule 71 (a check counts only if its probe can see the thing), ER-14 (tests never
  touch the machine), ER-15 (release only from a tag in CI), rule 54 (a change to a release path builds the installers),
  rule 55 and ADR 0034 (one implementation per mechanism), ADR 0022 (tooling in Rust, `./dev`; floating `stable`), ADR 0025
  (CI job isolation and kept folders), ADR 0028 (`paddock-deps`: licence policy, notices), ADR 0039 (release gates inspect
  the artifacts), Dyno ADR 0027 §7 (smartmontools is never bundled or mirrored), Sighter ADR 0001 (weights mirrored on
  Gitea).
- Scope: what a **build, test or release step** downloads: CI jobs, the scripts they run, the CI image build and the
  local VM harness. Downloads that a shipped app makes for its user (Chromium, llama.cpp and ffmpeg runtimes, models,
  smartmontools, the WebView2 bootstrapper, updates) are product features under `paddock-runtime`, `paddock-download` and
  ADR 0051. They are out of scope here, except where a test or the VM harness triggers one.

## Pattern

| When | What failed | Why |
| --- | --- | --- |
| 2026-11-08, `dyno-release` run 7693, job 31430 `bundle (ubuntu-latest)` | The Linux bundle of the Dyno 1.1.0 release candidate. The macOS and Windows bundles passed. | `tauri build --bundles deb,appimage` downloads its AppImage helpers at bundle time. The log (lines 2383 to 2386) shows `Downloading .../apprun-old/AppRun-x86_64` succeed, then `Downloading https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-07333c6/linuxdeploy-x86_64.AppImage` followed 11 s later by `failed to bundle project: http status: 504`. The only annotation was `Process completed with exit code 1`, with no URL (an ER-27 gap). |
| 2026-11-08, run 7694, job 31446 | The same job, re-dispatched. | This time the first download failed: `Downloading https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64`, then `http status: 504` (log lines 2378 to 2380). GitHub answered 504 for about an hour, to CI and to the Mac, while githubstatus said "operational" (coordinator log `2026-10-29-overnight.md:1344`). The tag waited for GitHub to recover. |
| 2026-11-08, Windows VM rounds of the Dyno 1.1.0 release record (user report) | Rows of the VM harness broke because the disk-health tool's download timed out. | The harness clicks Dyno's "Download and install smartmontools" button (`scripts/vm-smoke/win-helper.mjs:130` on `test/dyno-vm-harness-1.1-win`). The helper then fetches the pinned installer live from `github.com/smartmontools/.../releases` (`apps/dyno/crates/dyno-agent/tools/tool-pins.toml`), in every round. |

Neither failed run got as far as a third download: the AppImage type-2 runtime, which linuxdeploy's AppImage step fetches
from a moving `continuous` release on every build and puts inside every AppImage we ship (Context).

A survey of `origin/main` at `9e18230c5` (section "Inventory today") found 29 more inputs that a build, test or release
step fetches from a third-party host at run time, or reads live from a third-party service. At least eight of them are on
a release path with no hash, or with no pinned version at all.

**The pattern: build inputs are undeclared.** Each step that needs a file from outside the repository fetches it in its
own way, from the publisher, at the moment it runs. Some steps already pin and verify their files (cargo-nextest,
sccache, libduckdb, Chrome for Testing, the CI image's tools). But each does it with its own copy of "download, check
SHA-256, cache", and the copies differ:

- They keep their caches in different places.
- They fail in different ways: `fetch-verified.sh` writes to stderr with no `::error::`, the sccache step warns and
  carries on, and Tauri's bundler reports neither the URL nor any detail of a hash error.
- Nobody keeps a list of them.

Nothing stops a new step from adding a fetch, and nothing shows which releases depend on which hosts. So any outage of
GitHub, Google storage, Microsoft's CDNs, crates.io, the npm registry or a publisher's mirror can block a release, and we
find out on the day.

The fix is not more retries: a 504 that lasts an hour outlasts any retry. It is a mechanism that makes the class
impossible on release paths and detected everywhere else.

## Context

### What a fetch costs today

- **Release paths.** Every Tauri app with an AppImage builds its Linux bundle in a fresh `paddock-ci:v2` container: Dyno,
  Pitwall, Pitcrew and Spotter through `app-release.yml`, and the shell and workbench demos through their own release
  workflows. Because the container is fresh, Tauri's tools folder (`~/.cache/tauri`) is empty and the helpers are
  downloaded on every run. The same container also downloads every crate on every run (job 31430, line 1066:
  `Downloading crates ...`), so cargo's cache does not help there.
- **Tauri's own downloader.** These facts come from tauri-bundler 2.10.3, the version tauri-cli 2.12.3 pins (the root
  lock pins `@tauri-apps/cli` 2.12.3). They were read from the `tauri-cli-v2.12.3` tag:
  `crates/tauri-bundler/src/bundle/linux/appimage/linuxdeploy.rs`, `bundle/windows/nsis/mod.rs` and
  `utils/http_utils.rs`, on 2026-11-08.
  - It downloads three files into its tools folder:
    - `AppRun-x86_64` from `binary-releases/releases/download/apprun-old/`;
    - `linuxdeploy-x86_64.AppImage` from `.../download/linuxdeploy-07333c6/`;
    - the optional `linuxdeploy-plugin-appimage-x86_64.AppImage` from
      `github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/`. If this download fails, Tauri
      logs it and linuxdeploy uses its built-in AppImage plugin instead. The plugin's appimagetool is the same in both
      cases, so the plugin is not what moves.
  - It checks **no hash** for any Linux helper. It reuses any file already in the tools folder (`if !path.exists()`).
  - The tools folder is `dirs::cache_dir()/tauri`. That is `$XDG_CACHE_HOME` or `$HOME/.cache` on Linux and
    `%LOCALAPPDATA%` on Windows. It is `<target>/.tauri` instead when the configuration sets a local tools folder.
  - On every run, Tauri **zeroes three bytes at offset 8** of the cached linuxdeploy AppImage (`dd ... seek=8`). After its
    first use, the cached file no longer matches its published hash.
  - Two environment variables rewrite `https://github.com/...` download URLs: `TAURI_BUNDLER_TOOLS_GITHUB_MIRROR_TEMPLATE`
    and `TAURI_BUNDLER_TOOLS_GITHUB_MIRROR`. The downloader sets no retry and no timeout, sends no credentials, and its
    errors carry no URL.
  - On Windows it downloads `nsis-3.13.zip` and `nsis_tauri_utils.dll` 0.5.3, pinned by SHA-1 in Tauri, and keeps them
    in `%LOCALAPPDATA%\tauri\NSIS` on the host runner. A fresh runner, or a Tauri CLI bump that changes the NSIS pin,
    downloads them during a release. Two paths build NSIS installers: `conformance/ci/tauri-bundle.sh` (through
    `app-release.yml` and the demo releases) and Debrief's own `npx tauri build` step (`debrief.yml:956`).
  - WebView2: our apps leave `webviewInstallMode` at its default (`downloadBootstrapper`), so nothing is fetched at
    bundle time. The installer fetches the bootstrapper on the user's PC, which is product behaviour and out of scope.
- **The AppImage type-2 runtime.** The appimagetool inside linuxdeploy's AppImage plugin (built-in or downloaded) fetches
  the runtime from `github.com/AppImage/type2-runtime/releases/download/continuous/runtime-<arch>` on every build when it
  is not given `--runtime-file`. It neither caches nor verifies it. The runtime is the start-up code of every AppImage we
  ship, so it can change between two builds of the same commit. The plugin passes `LDAI_RUNTIME_FILE` to appimagetool as
  `--runtime-file` (read from `linuxdeploy-plugin-appimage/src/main.cpp`, 2026-11-08; appimagetool also reads
  `APPIMAGETOOL_RUNTIME_FILE`). The runtime's notices name libfuse (LGPL-2.1), which is linked into it.
- **What the helpers carry (licences).** linuxdeploy bundles patchelf and binutils' strip (GPL-3.0). The AppImage plugin
  bundles mksquashfs and desktop-file-validate (GPL-2.0) and zsyncmake (Artistic-2.0). Redistributing these files, which
  a public mirror would do, brings the GPL's source obligations with it. Only AppRun and the type-2 runtime end up in our
  installers.
- **The CI image** already pins and verifies sccache, cargo-hakari, cargo-machete, shellcheck, libduckdb and Chrome for
  Testing (`ci-image/Dockerfile`, `fetch-verified.sh`). But it fetches them from the publishers whenever the image is
  built. It carries none of the Tauri helpers and none of the release tools (git-cliff, cargo-cyclonedx, cargo-about,
  cyclonedx-npm).
- **The stores we already have:**
  - the runners' kept folders (ADR 0025; `conformance/ci/kept-folders.txt` has a shared `tools` folder);
  - the Linux runner's host mount `/sccache`, whose subfolders already hold the npm and Angular caches;
  - the Gitea container registry (`garage`, `paddock-ci`);
  - Gitea generic packages, already used for Sighter's weights (`decryp7/sighter-weights` 1.11.2, mirrored on 2026-10-30
    with SHA-256 pins). A generic package's address is `/api/packages/<owner>/generic/<name>/<version>/<file>`, and its
    visibility follows its owner's;
  - the user's `actions/cache` server (`172.16.1.188:8089`).

### Inventory today (origin/main 9e18230c5)

"Release" means the step runs in a tag's release run or in its gates. "Cached" means a later run on the same runner does
not fetch the file again.

| # | Input | Where | Host | Version pinned | Hash | Cached | Release |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A1 | linuxdeploy (Tauri AppImage helper) | Tauri bundler via `conformance/ci/tauri-bundle.sh` (`app-release.yml` bundle ubuntu; demo releases) | github.com | by tag name | none | no (fresh container) | yes |
| A2 | AppRun (ships in every AppImage) | same | github.com | by tag name | none | no | yes |
| A3 | linuxdeploy-plugin-appimage | same (optional; falls back to linuxdeploy's built-in plugin, with the same appimagetool) | github.com | **no** (`continuous`) | none | no | yes |
| A4 | NSIS 3.13, nsis_tauri_utils 0.5.3 | Tauri bundler on the Windows host: `tauri-bundle.sh` (`app-release.yml`, demo releases) and Debrief's own `npx tauri build` (`debrief.yml:956`) | github.com | yes | SHA-1 (Tauri) | host folder | yes |
| A5 | AppImage type-2 runtime (ships in every AppImage) | appimagetool inside the AppImage plugin, called by linuxdeploy (same jobs as A1) | github.com | **no** (`continuous`) | none | no | **yes** |
| B1 | cargo-nextest 0.9.149 | `.gitea/actions/host-tools` | github.com | yes | SHA-256 | kept `tools` | gates |
| B2 | sccache 0.19.0 | `.gitea/actions/setup-rust` | github.com | yes | SHA-256 | tool cache | gates (degrades to no cache) |
| B3 | Rust toolchain `stable` | `setup-rust` (`rustup toolchain install`); `dtolnay/rust-toolchain@stable` (debrief-mutants, -corpus, -bench, workbench-demo-release) | static.rust-lang.org | **no** (floating, ADR 0022) | rustup's | host | yes (host bundles) |
| B4 | git-cliff | `cargo install git-cliff --locked` (`app-release.yml:386`, `debrief.yml:1075`) | crates.io, compiled | **no** | lock only | if on PATH | **yes** (release notes) |
| B5 | cargo-cyclonedx | `cargo install` (`debrief.yml:1095`) | crates.io, compiled | **no** | lock only | if on PATH | **yes** (SBOM) |
| B6 | cargo-about 0.9.2 | `taiki-e/install-action` in `app-gates` (`.gitea/actions/app-gates/action.yml:74`, the notices check) and `debrief.yml:386`; `scripts/notices.sh` falls back to `cargo install` where it is missing | github.com (action); crates.io (fallback) | yes | the action's | no (container) | gates |
| B7 | tauri-driver | `cargo install` (`debrief.yml:666`) | crates.io, compiled | **no** | lock only | if on PATH | Debrief full runs and tags |
| B8 | cargo-mutants, cargo-fuzz | `app-mutants.yml`, `app-fuzz.yml`, `debrief-mutants.yml`, `debrief-fuzz.yml` | crates.io, compiled | **no** | lock only | if on PATH | nightly |
| B9 | cargo-about, cargo-deny, cargo-hakari via `taiki-e/install-action@v2` | `app-licences.yml:57`, `publish.yml:76` | github.com | partly | the action's | no | licences, shared publish |
| B10 | `@cyclonedx/cyclonedx-npm` | `npx --yes @cyclonedx/cyclonedx-npm` (`debrief.yml:1098`) | registry.npmjs.org | **no** (not in the lock) | none | npm cache | **yes** (SBOM) |
| C1 | Chrome for Testing 156 | `install-test-browser` action (`ci.yml:488`, `:646`; `debrief.yml:292` with Debrief's own pin in `runtime.rs`) | storage.googleapis.com | yes | SHA-256 | kept folder; image on Linux | gates |
| C2 | Chrome for Testing (Spotter's own) | `apps/spotter/scripts/install-test-browser.mjs`, `chromium-manifest.toml` | storage.googleapis.com | yes | SHA-256 | home cache | gates |
| C3 | Playwright WebKit and Chromium | `ci.yml:207` (host, kept per version); `shell-demo-release.yml:66`, `workbench-demo-release.yml:66` (`--with-deps`: also apt) | Playwright CDN, Ubuntu archive | by Playwright version | none | host only | demo releases |
| C4 | msedgedriver | `apps/debrief/app/e2e-tauri/scripts/fetch-msedgedriver.mjs` (`debrief.yml:670`) | msedgedriver.microsoft.com | follows the runner's WebView2 (moves with Windows updates) | none | stamp file | Debrief full runs and tags |
| C5 | libduckdb 1.5.6 | three copies: `fetch-libduckdb.sh` (CI), `xtask libduckdb` (dev), `app/scripts/libduckdb.mjs` (bundle) | github.com | yes | SHA-256 | host slot folder; Linux: image when versions match, else `RUNNER_TEMP` every run | **yes** (Debrief ships it) |
| D1 | Actions: `actions/checkout@v4`, `upload-artifact`, `download-artifact`, `cache`, `setup-node@v4`, `docker/*`, `dtolnay/*`, `taiki-e/*` | every workflow | the runner's default actions host (not measured: the job log says only `Download action repository 'actions/checkout@v4' (SHA:11d5960a…)`, job 31430 line 49) | tags, a few SHAs | git | runner's action cache | every job |
| D2 | Node | `actions/setup-node@v4` (via `.gitea/actions/setup-node`) | nodejs.org / GitHub manifest | `.nvmrc` major | the action's | tool cache | yes |
| D3 | `catthehacker/ubuntu:act-latest` | `ci-image.yml:25` | Docker Hub | **no** (`latest`) | none | runner | image build |
| D4 | CI image build inputs | `ci-image/Dockerfile` (`fetch-verified`, `npx playwright install --with-deps`, apt) | github.com, googleapis, Playwright CDN, Ubuntu | yes (apt no) | SHA-256 (apt: archive signatures) | image | image build only |
| E1 | npm packages | `npm ci` (root lock, sha512 integrity) | registry.npmjs.org | lock | sha512 | kept npm cache | yes (on a lock change) |
| E2 | Rust crates | cargo (lock checksums) | index.crates.io, static.crates.io | lock | SHA-256 | cargo home on hosts; **none in containers** (every run) | **yes** |
| E3 | apt packages | `setup-rust:195` (outside the image) | Ubuntu archive | **no** | archive signatures | no | rarely |
| L1 | RustSec advisory database | `cargo deny check` (licences action; `debrief.yml:409`; `publish.yml:43`; demo releases `:76`) clones it | github.com/rustsec/advisory-db | **no** (live data) | git | cargo-deny's folder (host) | **yes** |
| L2 | npm audit advisories | `npm audit --omit=dev` (licences action `:24`; `debrief.yml:455`; Dyno, Pitcrew and Pitwall `paddock.ci.json`) | registry.npmjs.org audit endpoint | **no** (live data) | none | none | **yes** |
| F1 | smartmontools 7.5 (Dyno helper's download) | VM harness rows (`win-helper.mjs`) | github.com | yes | SHA-256 and size | **no** (every round) | VM release rows |
| F2 | VM base packages | base build scripts (`scratch/dyno-vm-checks/linux-base-*.sh`) | distribution archives | no | archive signatures | snapshot | base build only |
| F3 | Sighter weights | Gitea generic package | **ours** | yes | SHA-256 | (not applicable) | (the model to follow) |
| F4 | DirectML redistributable (Sighter, on its branch) | build step | nuget.org | yes | SHA-256 | (not measured) | Sighter release, when it lands |
| G | Runner prerequisites, never fetched: 7-Zip (`conformance/release/check-per-machine-installer.mjs:55`), gpg (`verify-tool-pin.sh`), Xcode tools | release gates | (none) | (none) | (none) | installed | yes |

Some Rust and TypeScript tests name third-party URLs, but only as data against local mock servers
(`debrief-ai/tests/it/hf_download.rs`, `runtimes.rs`). ER-14 already covers tests, and the dead-proxy run (section 6)
shows they really stay local.

## Decision

### 1. The rule (proposed rule 72; number assigned at landing)

**No build, test or release step contacts a third-party host.**

- Every file a step needs from outside the repository is an entry in one inventory, `build-inputs.toml`, pinned by
  version, size and SHA-256.
- It is served from our own store: a runner's kept folder, the CI image, or a private Gitea mirror.
- Only `./dev inputs mirror` contacts a publisher's host. A person runs it, or dispatches it, to fill the store.
- If a gate or release cannot find an input in the store, it fails at once with an `::error::` that names the input. It
  never falls back to the publisher.

Some classes are exempt or follow their own policy. Each is declared in the inventory (section 2), so the exemption
itself stays visible:

- `registry`: npm and crates.io, already pinned by lockfile hashes (D1).
- `toolchain`: ADR 0022's floating Rust `stable`, fetched only by a warm-up job and never by a gate (D2).
- `live-data`: data that is meant to change, such as advisory databases. These follow the policy in section 3a.
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
id = "appimage-type2-runtime"
kind = "bundler-helper"             # tool | bundler-helper | test-browser | library | action | image | registry
                                    # | toolchain | live-data | prerequisite | vm-asset | model
version = "2026-11-snapshot"        # our snapshot of a `continuous` build; the hash is the identity
source = "https://github.com/AppImage/type2-runtime"
licence = "MIT AND LGPL-2.1-or-later"   # SPDX, judged by the family policy (ADR 0028); libfuse is linked in
ships = true                        # its bytes are inside every AppImage, so the notices must list it
redistribute = true                 # false: the private store's host-side tier only (D4)
package = "appimage-type2-runtime"  # Gitea generic package name in the private org; the version is `version`
used_by = [".gitea/workflows/app-release.yml#bundle", ".gitea/workflows/shell-demo-release.yml#bundle",
           ".gitea/workflows/workbench-demo-release.yml#bundle"]
release_path = true
seed = { env = "LDAI_RUNTIME_FILE" }    # how the consumer is pointed at it

[[input.platform]]
key = "linux-x86_64"
url = "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64"   # provenance only
size = 0                            # filled in by `./dev inputs mirror --new-pin` after approval; reviewed in the diff
sha256 = ""
```

Rules for entries:

- **Moving upstream builds are snapshotted.** A `continuous`, `latest` or `master` URL is allowed only as provenance. The
  entry pins the exact file we took (its size and SHA-256), and the store serves that file. The type-2 runtime and the
  AppImage plugin are pinned this way, so the start-up code of our AppImages stops changing under us.
- **`tied_to`** names a lockfile package whose version decides the entry (the Tauri helpers carry
  `tied_to = { npm = "@tauri-apps/cli", version = "2.12.3" }`). A bump of that package fails the check until the entry
  is re-read.
- **Runner-matched inputs** such as msedgedriver are pinned against a pinned counterpart (D5), never against whatever the
  runner happens to have.
- **`ships = true` entries feed the notices.** AppRun and the type-2 runtime end up inside every AppImage, yet no app's
  notices list them today. These entries go to `paddock-deps`' notices (ADR 0028), with libfuse's LGPL-2.1 text for the
  runtime.
- **Licences.** An entry's licence must pass the family policy for its use. Tools that run in CI and are not shipped
  (linuxdeploy with patchelf and strip under GPL-3.0; the plugin with mksquashfs and desktop-file-validate under
  GPL-2.0 and zsyncmake under Artistic-2.0) may be stored privately for our own use; storing them privately does not
  redistribute them. `redistribute = false` keeps an entry out of every Gitea package, even a private one (D4).

### 3. The store: three tiers and one resolver

| Tier | What | Who fills it |
| --- | --- | --- |
| 1. Kept folder | Content-addressed `tools/inputs/<sha256>/<file>` in each host runner's shared kept `tools` folder (ADR 0025, `job-isolation.sh path tools`). For Linux containers, the runner's existing host mount gets an `inputs/` subfolder beside `npm/` and `angular/`; this needs a runner `config.yaml` change, which is shown to the user first. | `./dev inputs ensure`, when tiers 2 and 3 miss. |
| 2. CI image | Linux tools baked in at fixed paths, each checked by `paddock-ci-verify`. The image's ARG pins are generated from the inventory (`./dev inputs dockerfile-args`), and a test checks that they agree. | The image build (`image-build` class). |
| 3. Private Gitea mirror | Generic packages owned by a **private** Gitea organisation (D3), one package per input: `/api/packages/<private-org>/generic/<id>/<version>/<file>`. CI reads with a read-only token kept as a secret. Each version is uploaded once and never re-uploaded (the lesson from Sighter's weights). | `./dev inputs mirror <id>`, run by a person or by a manually dispatched workflow. |

The resolver tries tier 1, then tier 2, then tier 3. It verifies size and SHA-256 every time it hands out a file. In CI
it never goes past tier 3.

The `actions/cache` server is not a tier (D8). It is keyed and evicts entries, and it gives containers nothing the host
mount does not.

**The public alternative, written down and not recommended:** a public Gitea organisation would need no token, but it
would redistribute GPL-3.0 and GPL-2.0 binaries (A1, A3) and so would have to offer their corresponding sources next to
them, for every version we keep. That is extra work and an obligation, for no gain in a family of personal tools.

### 3a. Live data (`live-data` class)

Advisory databases are meant to change, so they cannot be pinned. Each `live-data` entry declares a refresh job and a
maximum age.

- **RustSec advisory database (L1).** The store keeps a git mirror of `rustsec/advisory-db`, as a Gitea pull mirror in
  the private organisation. A nightly job refreshes it. CI points cargo-deny at the store copy (`db-urls` set to the
  mirror) instead of github.com.
  - On a release run, a mirror older than 7 days **fails** the run with an `::error::` that names the age and the
    refresh job. A mirror older than 2 days gives a warning.
- **npm audit (L2).** The npm registry's audit endpoint has no offline form. So the nightly job runs
  `npm audit --omit=dev --audit-level=high` for each lockfile and records the result with the lockfile's hash.
  - A release run accepts a passing record for the **same lockfile hash** that is at most 48 hours old.
  - Without such a record, the release run tries a live audit once. If the endpoint cannot be reached, the run fails
    with an `::error::` that names it, and the next step is to wait for, or dispatch, the nightly audit. A live audit
    that finds advisories fails as it does today.
  - Branch runs keep the live audit, as a warning when the endpoint cannot be reached.

### 4. One fetcher: `./dev inputs`

A module of `tools/paddock-dev` (ADR 0022: tooling in Rust; external commands go through `paddock-proc`), with these
commands:

- `./dev inputs ensure <id>... [--seed]` resolves an input, verifies it, places it and prints its path. With `--seed` it
  points the consumer at it (the entry's `seed`: a file in a tools folder, or an environment variable). It writes one
  line per input to the job summary: id, version, tier, bytes and milliseconds.
- `./dev inputs seed tauri` prepares everything `tauri build` would otherwise fetch, for the locked CLI.
  - On Linux it copies AppRun, linuxdeploy and the AppImage plugin into the folder Tauri reads, computed the way
    `dirs::cache_dir()` does and logged. It makes a **fresh copy on every run**, because Tauri patches the cached
    linuxdeploy in place (see Context) and a reused copy can no longer be verified.
  - It sets `LDAI_RUNTIME_FILE` (and `APPIMAGETOOL_RUNTIME_FILE`) to the verified type-2 runtime, so appimagetool never
    fetches it.
  - On Windows it seeds `%LOCALAPPDATA%\tauri\NSIS` the same way, for both NSIS paths (`tauri-bundle.sh` and
    `debrief.yml:956`), so neither a fresh runner nor a CLI bump fetches NSIS during a release.
  - It sets no mirror-template variable. Tauri's downloader sends no token, so it cannot read a private store, and a
    `continuous` asset has no stable version for the template's `<version>`. Anything Tauri still fetches is caught by
    the dead-proxy run (section 6) and named by the URL scan (section 5).
- `./dev inputs mirror <id> [--new-pin]` is the only command that contacts a publisher.
  - It refuses to run in CI unless the mirror workflow dispatched it.
  - With an existing pin, it verifies the download against the pin. With `--new-pin`, it prints the size and SHA-256
    for review in the diff.
  - It then uploads the file to the private Gitea organisation (tier 3). A `redistribute = false` entry goes to the
    named host-side store instead.
- `./dev inputs list [--job <workflow#job>] [--release]` shows what a job, or the release paths, depend on.

`./dev inputs` replaces the five existing copies of "fetch, verify, cache":

- `ci-image/fetch-verified.sh`, which stays only for the image build;
- the `host-tools` action;
- `setup-rust`'s sccache step;
- libduckdb's three implementations;
- the two `install-test-browser` scripts.

WP-1 registers these five, and its own interim reader (section "Work packages"), in `docs/known-copies.toml` with the
WP that deletes each (rule 55, ADR 0034). Each entry leaves with its files.

### 5. Errors and logs (ER-27)

Every failed input prints one annotation in one format, then exits non-zero:

```
::error title=build input appimage-type2-runtime::could not get appimage-type2-runtime 2026-11-snapshot (runtime-x86_64): not in the kept folder, not in the image, the private store answered HTTP 404 for appimage-type2-runtime/2026-11-snapshot. Next: run `./dev inputs mirror appimage-type2-runtime` on a workstation, then re-run this job.
```

- **Causes are named, never just an exit code:** the HTTP status, a connect or read timeout with its limit, a size or
  SHA-256 mismatch (expected and got), a missing or refused token, or the missing tier.
- **Timeouts are bounded:** connect 15 s; a read that stalls for 30 s fails, and long reads print progress lines. Each
  tier gets at most two retries, with backoff, on a 5xx or a timeout.
- **`tauri build` runs with `--verbose` in CI,** so linuxdeploy's and appimagetool's own downloads appear in the log.
- **Fetches Tauri adds later are named too.** When `tauri build` fails, the build step scans the tail of its output for
  the last `Downloading https://...` line (the form in jobs 31430 and 31446) or an appimagetool download line, and names
  that URL in its `::error::`. A fetch that a later Tauri version adds is then named on its first failure.

### 6. Proof that nothing fetches: the dead-proxy run (ER-33)

A static check cannot see a fetch that a tool makes on its own, and Tauri's helpers are exactly that. So a run proves it:

- **The run.** The release workflows' bundle and gate steps run with `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` set to
  a closed local port (`http://127.0.0.1:9`), and `NODE_USE_ENV_PROXY=1` (Node 24, the major in `.nvmrc`). `NO_PROXY`
  names the private store's host (`dev.decryptology.net`), `172.16.1.188`, `localhost`, `127.0.0.1`, and the registries
  while D1 exempts them (`registry.npmjs.org`, `index.crates.io`, `static.crates.io`). Any other fetch through cargo,
  curl, ureq (Tauri) or Node fails at once, and the URL scan (section 5) names it.
- **When.** Nightly, and in every rule-54 dispatch of a release workflow with bundles. Once it is green there for a week
  (D7), it is also set on tag release runs.
- **The probe proves it can see.** The step first fetches a non-exempt URL through the same variables and expects a
  refusal. If the fetch succeeds, the job summary says `dead-proxy run: check broken`, and the run fails (ER-33).
- **It is a tripwire, not a sandbox.** A program that opens raw sockets, or ignores proxy variables, gets past it.
- **Revisit if:** the dead-proxy run misses a fetch that later breaks a release, or the registries' misses need to be
  counted for D1. Then build the full guard: a logging proxy that records every host per step and refuses in `refuse`
  mode (revision 1's section 6, kept as WP-6).

### 7. The check: build inputs are declared (conformance, number assigned at landing)

A static check runs over `.gitea/**`, `conformance/ci/**`, `conformance/release/**`, `scripts/**`, `apps/*/scripts/**`,
every `xtask`, `ci-image/**`, the VM harness and the `scripts` of every `package.json`. It checks six things:

1. **Every fetch site maps to an inventory entry.** A fetch site is any of: `curl`, `wget`, `Invoke-WebRequest`, `iwr`,
   `fetch(`, an `https://` URL in a run step, `npx playwright install`, `npx -y` and `npx --yes`, `cargo install`,
   `cargo deny`, `npm audit`, `rustup toolchain install`, `apt-get install`, `pip install`, a `uses:` of an action
   outside this repository, and a container `image:` line. The site must call `./dev inputs`, or carry a
   `# build-input: <id>` marker within three lines (the C54 marker style). Comments and README prose are ignored.
2. **The entry matches the site.** The entry exists, its URL matches the site's literal or template, and its `used_by`
   lists the site's workflow and job. An entry that nothing uses is reported as stale.
3. **Release paths never depend on a publisher.** A `release_path = true` entry has tier 2 or tier 3, or is a
   `live-data` entry with a refresh job and a maximum age. A `redistribute = false` entry has no `package`.
4. **Entries are well formed.** Every SHA-256 is 64 lowercase hex digits, every size is positive, and every licence
   passes the family policy for its use.
5. **`tied_to` agrees with the lockfiles.** A Renovate bump of `@tauri-apps/cli` fails until the Tauri entries are
   re-read. A helper, `./dev inputs tauri-scan`, lists the download URLs found in the locked Linux CLI binary for
   comparison.
6. **The check's own tests fail on each historical instance:** a bare `cargo install git-cliff`, `npx --yes` of a
   package not in the lock, the unseeded AppImage bundle, the runtime without `LDAI_RUNTIME_FILE`,
   `taiki-e/install-action`, and a `continuous` URL without a pin.

The check starts with a ratchet baseline of today's sites (section "Inventory today"), which may only shrink.

## Checks that make the class detected

| Failure | Caught by |
| --- | --- |
| A new script or step fetches from a publisher | Section 7, items 1 and 2 |
| A release path depends on the publisher's host | Section 7, item 3; the dead-proxy run (section 6) |
| A tool fetches something on its own (Tauri, linuxdeploy, appimagetool, Playwright, an action) | The dead-proxy run; `--verbose` and the URL scan (section 5) |
| A Tauri CLI bump brings new helpers or new pins | Section 7, item 5 |
| A moving upstream build changes what we ship | Snapshot pins (section 2); SHA-256 on every hand-out |
| An advisory database is stale on a release | Section 3a maximum ages |
| A fetch fails with no cause in the log | Section 5 format, unit-tested per cause |
| The dead-proxy run is blind | Its probe (section 6) |

## Work packages and estimates

The estimates are raw agent-days, without review rounds; add about 20 % for those. Dyno 1.1.0 is already tagged, so the
first slice is no longer needed for Dyno's tag. It lands now, because Pitwall, Pitcrew and Spotter build AppImages and
Debrief builds NSIS installers on their next release runs.

### First slice: no live fetch on any release path (about 4–6 days)

| WP | What | Days |
| --- | --- | --- |
| **WP-1** | **AppImage and NSIS helpers for every Tauri app's bundles.** (a) Pin `AppRun-x86_64`, linuxdeploy `07333c6`, a snapshot of `linuxdeploy-plugin-appimage` and a snapshot of the type-2 runtime for tauri-cli 2.12.3, with size, SHA-256 and licence checked. These are the first `build-inputs.toml` entries, already in the final format; the runtime and AppRun carry `ships = true`. (b) Create the private Gitea organisation and its CI read token (with the user), and upload the entries. (c) Make `tauri-bundle.sh` seed a fresh, verified copy into the Tauri tools folder before `tauri build`, and set `LDAI_RUNTIME_FILE` and `APPIMAGETOOL_RUNTIME_FILE`. Until `./dev inputs` exists, a small shell step reads the entries; it is registered in known-copies, and WP-3 replaces it. (d) Run `tauri build --verbose` in CI, and add the `::error::` URL scan of a failed build. (e) Seed NSIS on Windows for both NSIS paths: `tauri-bundle.sh` and Debrief's `debrief.yml:956`. (f) Add the `tied_to` test against the lock. (g) Measure the runners' default actions host (the question WP-7 acts on), and record it in the inventory. (h) Register the five existing fetch copies and the interim reader in `docs/known-copies.toml`. (i) Rule 54: before landing, dispatch the release workflows, with bundles, of Pitwall, Pitcrew, Spotter, Dyno, both demos and Debrief. | 2–3 |
| WP-1b | **Release tools as hashed prebuilt binaries.** git-cliff, cargo-cyclonedx and cargo-about from their published release archives (SHA-256 pinned, in the private store and the kept folder), replacing `cargo install` (B4, B5) and `taiki-e/install-action` (B6, B9) on release paths; `@cyclonedx/cyclonedx-npm` pinned in the root lock as a dev dependency and run from it, replacing `npx --yes` (B10). | 1–1.5 |
| WP-1c | **The live-data policy** (section 3a): the RustSec pull mirror and cargo-deny's `db-urls`, the nightly audit record per lockfile hash, the release-run rules. | 0.5–1 |
| WP-1d | **The dead-proxy run** (section 6) with its probe, nightly and in every rule-54 dispatch. | 0.5 |
| | **First slice total** | **4–6** |

### Hardening (after the first slice)

| WP | What | Days |
| --- | --- | --- |
| WP-2 | The inventory: the format; the parser, sharing `paddock-runtime`'s catalogue types; every row of "Inventory today" as an entry with its licence and `used_by`; the `ships` list handed to `paddock-deps`. | 2–3 |
| WP-3 | `./dev inputs ensure`, `seed`, `mirror` and `list`; the three tiers; the error format and job summary; the kept-folder entry in `kept-folders.txt`; the runner mount change, shown to the user first; the private mirror of every entry; the interim reader of WP-1 deleted. | 3–5 |
| WP-5 | The conformance check (section 7) with its ratchet baseline, its markers and a test per historical instance. | 2–3 |
| WP-4 | Adoption on the remaining paths. Replace the five copies (each known-copies entry leaves with its files). Pin tauri-driver, cargo-mutants and cargo-fuzz as prebuilt, hashed binaries. Replace `dtolnay/rust-toolchain`; `setup-rust` skips `rustup toolchain install` when the toolchain is present (D2). Serve the Playwright browsers from the store (`PLAYWRIGHT_BROWSERS_PATH`). Keep apt in the image only. | 3–5 |
| WP-7 | Actions and images: mirror the actions we use into the private organisation and pin them by commit; for the image build's base, use a pinned digest from our registry instead of `catthehacker/ubuntu:act-latest`. Needs the user for the Gitea and runner settings. | 1–2 |
| WP-9 | VM harness: smartmontools pre-staged from a host-side store (D4); the VM base inputs listed (`base-build`). | 1–2 |
| WP-10 | The rule text, a lessons entry, an ER line proposal for `engineering-rules` (next free ER number), and the docs (`ci-image/README.md`, `.gitea/actions/README.md`). | 0.5–1 |
| | **Hardening total** | **12.5–21** |

### Deferred

| WP | What | Days |
| --- | --- | --- |
| WP-6 | The full egress guard (logging proxy, per-step hosts, `refuse` mode), only on section 6's "revisit if". | 2–4 |
| WP-8 | msedgedriver against a pinned WebView2 Fixed Version runtime for the Debrief Windows e2e (D5). Until then, C4 stays on Debrief's full runs and is listed as a known live fetch outside the release bundles. | 1.5–3 |
| (D9) | CI image v3 with the build-input tools, after the current releases. | (in WP-4 then) |

**Total:** first slice 4–6, hardening 12.5–21, deferred 3.5–7 raw days; about 20–34 with review rounds for everything.

Order:

1. The first slice now: WP-1, then WP-1b, WP-1c and WP-1d in parallel.
2. WP-2 and WP-3.
3. WP-5 and WP-4 in parallel.
4. WP-7 and WP-9 beside WP-4, once WP-3 is in. WP-10 lands with WP-5.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | npm and crates.io | (a) Exempt them as `registry`: lockfile hashes, with the kept caches on hosts. Linux containers download every crate on every run today (job 31430), so a crates.io outage still blocks a Linux release until WP-3's mount gives containers a cargo cache. (b) Run a pull-through proxy for npm and cargo on the home network. | (a) now, with the container cargo cache in WP-3; revisit if a registry outage blocks a release |
| D2 | ADR 0022's floating Rust `stable` on host runners | (a) Gates and releases use the installed `stable`: `setup-rust` skips `rustup toolchain install` when the toolchain and components are present. A nightly warm-up job updates each host. (b) Keep updating in every job. | (a): the fetch leaves the gates, and ADR 0022's floating `stable` stays |
| D3 | Where the Gitea store lives | (a) A **private** Gitea organisation that owns the generic packages (visibility follows the owner), read by CI with a read-only token; private storage of the GPL tools we run in CI is our own use, not redistribution. (b) A public organisation, which then has to offer the corresponding sources of the GPL-3.0 and GPL-2.0 binaries (A1, A3) next to each stored version. | **(a)** |
| D4 | smartmontools in the VM harness (GPL; Dyno ADR 0027 §7 says Dyno never bundles or mirrors it) | (a) Keep it out of Gitea, even the private store (`redistribute = false`). The harness keeps the pinned installer in a host-side store on the VM host and pre-stages smartmontools in the VM before the reading rows, so those rows no longer depend on a download. One "Download and install" row still exercises the product's own download from upstream. The harness first checks that upstream is reachable from the VM, and reports the row `not measured` when it is not (ER-33). The VM tests release builds, so no product switch is involved. (b) Keep the live download for every row, and report the rows `not measured` when it fails. | (a) |
| D5 | msedgedriver | (a) Gates use a pinned WebView2 Fixed Version runtime (`WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`) and its matching driver, both inventory entries. The evergreen runtime runs nightly, with the driver fetched for its version. (b) Keep following the runner's WebView2. | (a), deferred (WP-8) |
| D6 | Sighter's DirectML (decided 2026-10-30: fetched from nuget.org, not mirrored) | (a) Keep that decision: a `redistribute = false` entry in the host-side store, filled by `./dev inputs mirror`, so the release still never fetches live. (b) Store it in the private Gitea organisation. | (a) |
| D7 | The dead-proxy run | (a) Nightly and in every rule-54 dispatch now; also on tag release runs after a green week. (b) Nightly and dispatches only. | (a) |
| D8 | The `actions/cache` server | (a) Not a tier for build inputs, because it is keyed and evicts. (b) A tier for containers. | (a) |
| D9 | CI image | (a) v3 with the build-input tools after the current releases (Pitwall 1.1, Debrief 1.2), DuckDB 2.0 in a later version. (b) v3 now. | (a): the first slice needs no image change |

## Consequences

- A release no longer waits for GitHub, Google, Microsoft or a publisher's mirror. An outage costs nothing until someone
  bumps a pin.
- Bumping a pin becomes a deliberate step: `./dev inputs mirror --new-pin`, a reviewed diff of sizes and hashes, and an
  upload. Renovate PRs for pinned tools fail the check until that is done.
- The AppImage runtime and AppRun we ship become pinned, listed and credited, with libfuse's LGPL-2.1 notice.
- A release run fails on a stale advisory database instead of passing without one.
- The private store grows by the tools we use. The largest are Chrome for Testing, at about 200 MB per platform, and the
  Playwright browsers. Old versions are pruned after the last tag that used them.
- The release path gains one component, the private store, and one secret, its read token. If either fails, the error
  names it. Gitea is already a dependency of every release (registry, releases, the CI image).

## Alternatives considered

- **More retries and longer timeouts.** The 504s lasted about an hour; retries only move the failure.
- **Tauri's mirror variable.** It moves the host, but adds no hash check for the Linux helpers, sends no token (so it
  cannot read a private store), cannot address a `continuous` asset by version, and does not reach appimagetool's runtime
  download. Dropped in revision 2.
- **A public mirror.** It would redistribute GPL binaries and bring the duty to offer their sources (D3 (b)).
- **Binaries in git (or LFS).** It bloats the repository in every worktree, and the family does not use LFS.
- **A general pull-through proxy (Nexus, Artifactory) for everything.** It is another service to run and secure, for
  files that change a few times a year. It is reconsidered under D1 if the registries prove a problem.
- **`actions/cache` as the store.** It is keyed, it evicts, and host runners' kept folders cannot see it (D8).
- **A logging egress proxy from the start.** More code than a closed-port proxy for the same first answer; kept as WP-6
  behind a "revisit if".
- **Pinning in each script, as today.** That is the pattern this ADR ends: five copies, no list and no check.

## Revision 2 (answers the first review, 2026-11-08)

- **P1-1, the type-2 runtime:** added as A5 with a `ships = true` entry, seeded through `LDAI_RUNTIME_FILE`; `tauri build
  --verbose` in CI; the exact failing URLs of runs 7693 and 7694 quoted from their job logs (linuxdeploy in 7693, AppRun
  in 7694; neither reached the runtime). A3's reasoning corrected: the plugin's appimagetool is the same, and the runtime
  is what moves. The bundler version is given as 2.10.3, pinned by tauri-cli 2.12.3.
- **P1-2, licences:** the GPL-3.0, GPL-2.0 and Artistic-2.0 tools inside the helpers are listed; the store is a private
  organisation with a CI read token (D3 (a)), with the public-with-sources alternative written down; package paths
  follow Gitea's `<owner>/generic/<name>/<version>/<file>` layout; D3 and D4 agree; libfuse's LGPL-2.1 is in the
  runtime's entry and notices.
- **P1-3, missing release-path fetches:** B10 (`npx --yes @cyclonedx/cyclonedx-npm`, `debrief.yml:1098`); B6 corrected
  (cargo-about comes from `taiki-e/install-action` in `app-gates:74` and `debrief.yml:386`); a `live-data` class with
  the policy for RustSec (L1) and npm audit (L2) in section 3a; Debrief's own NSIS path (`debrief.yml:956`) in A4 and
  WP-1 (e).
- **P1-4, D4:** no debug-only switch; smartmontools is pre-staged from a host-side store for the reading rows, and one
  "Download and install" row reports `not measured` when upstream cannot be reached.
- **P2s:** the dead-proxy run replaces the custom proxy, with the full guard deferred behind "revisit if"; the
  mirror-template fallback is dropped; a first slice of about 4–6 days ("no live fetch on any release path") comes
  first, with WP-2, WP-3 and WP-5 as hardening and WP-6 and WP-8 deferred; Dyno 1.1.0 is tagged, so WP-1 lands now and
  D9 waits for the current releases; D2 says `setup-rust` skips `rustup toolchain install` when the toolchain is
  present; WP-1 measures the actions host; WP-1 registers the five copies and its interim reader in known-copies.
- **Found while revising:** Linux release containers download every crate on every run (job 31430, line 1066), so E2
  and D1 now say so.

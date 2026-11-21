# Developer machine setup

One setup for every Paddock app (Debrief, Pitcrew, Pitwall, Spotter, Dyno, and later Sighter and Pacenotes). Each app's
`docs/development.md` adds only what is specific to it (for example Debrief's libduckdb). The binding way of working is in
[rules.md](rules.md); this page is how to get a machine ready for it.

Check a machine, and install what is missing on macOS, with:

```sh
scripts/setup-dev.sh            # reports what is present and what is missing; changes nothing
scripts/setup-dev.sh --install  # macOS: installs the missing tools, then reports again
```

## Tools

| Tool | Why | macOS | Linux (Debian/Ubuntu) | Windows |
| --- | --- | --- | --- | --- |
| Rust stable + `rustfmt`, `clippy` | every app | [rustup](https://rustup.rs) | rustup | rustup |
| `x86_64-pc-windows-msvc` target | type-check Windows code from a Mac or Linux | `rustup target add x86_64-pc-windows-msvc` | same | n/a |
| Node.js (each app's `app/.nvmrc`) + npm | Angular UI, scripts | nvm | nvm | nvm-windows |
| Tauri 2 platform dependencies | desktop shell | Xcode Command Line Tools | `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf` | MSVC Build Tools, WebView2 |
| `sccache` | one Rust build cache shared by every checkout and worktree | `brew install sccache` | `cargo install sccache --locked` | same |
| `cargo-nextest` | faster, isolated Rust test runs; flaky-test report | `brew install cargo-nextest` | `cargo install cargo-nextest --locked` | same |
| `cargo-deny`, `cargo-llvm-cov` | licence and advisory gate, coverage | `cargo install --locked cargo-deny cargo-llvm-cov` | same | same |
| `cargo-hakari`, `cargo-machete` | one feature set per dependency, unused dependencies | `cargo install --locked cargo-hakari cargo-machete` (or the prebuilt release binaries) | same | same |
| Docker (or OrbStack) | run the Linux CI image locally before pushing | Docker Desktop or OrbStack | docker | Docker Desktop |
| Playwright browsers | e2e: Chromium in branches, Chromium + WebKit on the combined branch and in CI | `npx playwright install chromium webkit` in `app/` | `npx playwright install --with-deps chromium webkit` | `npx playwright install chromium` |
| Chrome for Testing (pinned) | tests that drive a real browser; never the installed Google Chrome (rule 7) | each app's `install-test-browser` script sets `CHROMIUM_TEST_PATH` | same | same |

## Where things live

Code, scripts and docs never name a disk (C125, rule 40): a disk gets renamed or replaced, and a hard-coded path then goes
stale without a sound (on 2026-11-14 the dev data moved disks, and the guards, the VM scripts and the dev tool's defaults all
pointed at disks that were gone). Tools derive every folder from one setting per machine, `git config paddock.worktreeRoot`
in the main checkout (repo-local, so every worktree sees it), and fail with an `::error::` and this next step when it is not
set. This section is the one place that names this machine's layout; everything else writes `<dev-targets>` or `<dev disk>`
and links here.

| Name | What is there | How the tools find it |
| --- | --- | --- |
| `<dev-targets>` | worktrees (`wt/`), build folders, agent scratch (`scratch/<agent id>`), `sccache`, `paddock-dev-home`, the disk index `.paddock-disk.sqlite` | `PADDOCK_DEV_TARGETS`, else the parent of `paddock.worktreeRoot`, else of `PADDOCK_WORKTREE_ROOT` |
| `<dev disk>` | the folder that holds `<dev-targets>` | the parent of `<dev-targets>` |
| `<dev disk>/vm-data` | the test VMs: the Windows test VM `paddock-win11-test`, the Tart images `tart-from-home` | `PADDOCK_VM_DATA`, else derived; the Windows VM's guard reads git config only, with an empty environment |
| `<dev disk>/ci/locks/test-machines` | the lock of the test machines (`scripts/vm-lock.sh`) | `PADDOCK_VM_LOCK_ROOT`, else derived |
| the main checkout | `paddock`, with `engineering-rules` next to it | the folder that holds the git common directory |

The derivation is `tools/paddock-dev/src/places.rs`; scripts ask `./dev places` (`--get <name>` for one folder, `--pinned` to
ignore every environment variable). The hooks, which run before any binary is built, have the one copy,
`.claude/hooks/lib/paddockpaths.py` (docs/known-copies.toml, `dev-places`; a parity test runs both on one table).

The Mac mini's layout since 2026-11-14 (one SSD for all dev data; media and backups are on another disk, which no tool reads):

| Name | Folder |
| --- | --- |
| `<dev disk>` | `/Volumes/vault-dev` |
| `<dev-targets>` | `/Volumes/vault-dev/dev-targets` (`paddock.worktreeRoot` = `/Volumes/vault-dev/dev-targets/wt`) |
| the main checkout | `/Volumes/vault-dev/dev.decryptology.net/paddock` |

After a disk move: move the folders, run `git config paddock.worktreeRoot <new dev-targets>/wt` in the main checkout,
update the shell profile's `SCCACHE_DIR` and `PADDOCK_DEV_HOME`, and change the table above. Nothing else names the disk.

## Environment

Put these in the shell profile (`~/.zshenv` on macOS):

```sh
export SCCACHE_DIR=<dev-targets>/sccache   # on the dev disk ("Where things live" below), not the system disk
export SCCACHE_CACHE_SIZE=60G       # the default 10G is too small for several apps
export CARGO_BUILD_JOBS=2           # per job; several run at once (Mac mini: about five heavy jobs at most)
```

Do not export `RUSTC_WRAPPER` in the profile: `eval "$(./dev sccache env)"` sets it per shell (a shim, below), and a profile value of `sccache` makes `./dev` decline its own. The shared compile cache's hit rate is `sccache --show-stats`.

**sccache.** One server per user serves every checkout and worktree; the cache lives on the big volume
(`SCCACHE_DIR`: `<dev-targets>/sccache`, next to the per-worktree target folders; see "Where things live"). It caches
the dependencies (registry crates, build scripts' C code), which is most of a cold build of a new worktree. It cannot
cache the workspace's own crates in a debug build (incremental) or any link, so an edit-and-test cycle gains nothing
from it. When `sccache --show-stats` stays at 0 requests while builds run, the server is broken: `sccache --stop-server`
(the next build starts it again with the current environment) and check that `RUSTC_WRAPPER` and `SCCACHE_DIR` are set
in the shell that builds. A server started from a shell without `SCCACHE_DIR` writes to `~/Library/Caches/Mozilla.sccache`
on the system disk.

sccache hashes every `CARGO_*` variable into its cache key, `CARGO_TARGET_DIR` included, so worktrees with different target
folders share no entries when the folder comes from the environment (the registry crates then miss in every one). Set the
folder with `--config build.target-dir=...` or `build.target-dir` in a `.cargo/config.toml` above the worktree instead; CI
passes `--config` on every cargo step. Never put a script between cargo and sccache: on Windows a `.cmd` runs through cmd.exe,
whose 8191-character command line limit rustc's arguments for a large crate (windows-sys) exceed. The workspace's own path crates still
key on their checkout path (`CARGO_MANIFEST_DIR`); a symlink does not help, cargo uses the physical path.

### Local sccache

Agents' builds use one local sccache 0.19.0 (the CI pin and checksums, `tools/paddock-dev/src/ci_setup_rust.rs`), set by Paddock's
tools and never by your shell profile (user decision 2026-11-16; docs/research/sccache-basedirs-2026-11-16.md):

- `eval "$(./dev sccache env)"` installs the pinned release into `<PADDOCK_DEV_HOME or ~/.paddock-dev>/tools/sccache/0.19.0/` (never
  `~/.cargo`), starts the one server on the default port 4226, and prints `export RUSTC_WRAPPER=...` lines for that shell. Run it
  once per machine to install; the push gates and `./dev gen check` then use the installed copy by themselves and never download.
- It is off, with one `::warning::` line and a build without the cache, when sccache is not installed, the server does not
  start, or a server on the port uses another cache. The server runs in its own short folder (`<tool cache>/sccache-run`, also its `TMPDIR`, working folder and error log
  `sccache.log`; short because macOS caps socket paths at 104 bytes), never in a job's or an agent's scratch folder, which a job deletes. It
  never exits on its own (`SCCACHE_IDLE_TIMEOUT=0`; stop it with `sccache --stop-server`), so a build that finds none cannot replace it
  with one that has the build's environment. When it comes up read-only (sccache 0.19.0 keeps a failed startup write check for the
  server's whole life: no compile is stored) the tool stops it, starts it once more, and if it is still read-only turns the wrapper off
  and names the log in the warning. It is off without a warning in CI (`CI`, `GITHUB_ACTIONS`), with
  `PADDOCK_SCCACHE=off`, or when `RUSTC_WRAPPER` is already set.
- The build folder goes in as `--config build.target-dir=...` in every tool (see AGENTS.md, "Build folders").
- **One setting picks the backend, `PADDOCK_SCCACHE_BACKEND`:** `local` (the default; `SCCACHE_DIR` and `SCCACHE_CACHE_SIZE` from
  the profile above) or `s3` (a shared cache on an S3-compatible store, through sccache's own S3 support). `s3` needs
  `PADDOCK_SCCACHE_S3_BUCKET` and `PADDOCK_SCCACHE_S3_ENDPOINT` (`host:port`), and optionally `PADDOCK_SCCACHE_S3_REGION` (default
  `auto`), `PADDOCK_SCCACHE_S3_SSL` (`true`; default `false`) and `PADDOCK_SCCACHE_S3_PREFIX` (default `paddock`).
- **S3 credentials are never in the repository, a log or a message.** They are read from `AWS_ACCESS_KEY_ID` and
  `AWS_SECRET_ACCESS_KEY` in the environment, else from `~/.config/paddock/sccache-s3.env` (`$XDG_CONFIG_HOME/paddock/sccache-s3.env`,
  or the file `PADDOCK_SCCACHE_ENV_FILE` names): two `NAME=value` lines, mode 600 (a file others can read is refused). The tools
  print only `credentials configured` or `credentials missing`. The macOS keychain is not read by the tools (it would raise a
  dialog); export the key from it into the file yourself.
- One server takes one cache at a time. To switch the backend, run `sccache --stop-server` once, then the next build starts the
  server with the new setting. A server on the port that uses another cache is never stopped by the tools.

The shared RustFS cache (S3 API `http://172.16.1.220:15000`, bucket `sccache-paddock`, 50 GiB quota, policy `sccache-paddock-rw`):
`PADDOCK_SCCACHE_BACKEND=s3 PADDOCK_SCCACHE_S3_BUCKET=sccache-paddock PADDOCK_SCCACHE_S3_ENDPOINT=172.16.1.220:15000`, plus the
key in the file above. Create the access key bound to that policy in the RustFS console (port 15001) and write it as:

```sh
mkdir -p ~/.config/paddock && umask 077 && cat > ~/.config/paddock/sccache-s3.env <<'KEYS'
AWS_ACCESS_KEY_ID=<access key>
AWS_SECRET_ACCESS_KEY=<secret key>
KEYS
chmod 600 ~/.config/paddock/sccache-s3.env
```

**CI keeps its own caches.** The Mac, Windows and Linux CI jobs run `setup-rust`'s per-slot sccache folders and ports exactly as
before; they do not share the agents' server (user decision 2026-11-17, after a review: the agents' cache and CI's differ in
`CARGO_INCREMENTAL=0`, which sccache hashes, so a shared folder would hit about 4%, and one server for two kinds of user is one more
thing that can stop a runner). The agents share one server and one cache folder among themselves.

**CI's Actions cache.** The CI runners use their built-in `cache.s3` against RustFS (the same server as above): bucket `ci-actions-cache`
with a 7-day expiry for `actions/cache` (the `./dev` binary, npm, Angular, the nightly marker), and bucket `sccache-paddock` with a 30-day
expiry for sccache. The Windows runner has no zstd, so its cache entries are not shared across operating systems (a Windows key never
hits a Linux or macOS entry). No folder is shared between concurrent lanes (ADR 0111): each Linux lane has its own
`<mount>/sccache-<lane>`, and everything kept between jobs goes through a keyed cache.

**Linker on Linux.** On a Linux development machine use mold (`sudo apt install mold`), in `~/.cargo/config.toml`:

```toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
[target.aarch64-unknown-linux-gnu]
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

The gain is largest on aarch64 Linux, which still links with GNU ld; x86_64 Linux already links with rust-lld (the default
since Rust 1.90), where mold saves a little more per link. macOS keeps the default linker (Apple's ld is already fast).

## Shared build cache

CI Rust jobs use sccache's **local disk** on every OS: the job slot's kept folder on a host runner, `/sccache` in a container
(the `setup-rust` input `sccache-backend` defaults to the variable `PADDOCK_CI_SCCACHE_BACKEND`, `local` when it is unset, and so does
`./dev ci setup-rust`, `ci_setup_rust.rs`). No workflow picks another backend; the RustFS store behind the disk is switched on by that
variable (below). The shared Dragonfly (Redis-compatible) server at `172.16.1.188:6379` was CI's backend until 2026-10-25: it
filled up, the Mac job's sccache server failed to start and every rustc call failed (commit 1383e9898); the user decided against a
shared backend on 2026-10-28 and again on 2026-11-17 (CI keeps its per-slot caches). The Redis path is kept for a deliberate trial:

- Gitea Actions variables (set by the user, org or repo level): `SCCACHE_REDIS_ENDPOINT=redis://172.16.1.188:6379` and
  `SCCACHE_REDIS_DB=5`. The `setup-rust` action reads them as its input defaults; they are used only with `auto` or `redis`.
  sccache uses **database 5 only**; never use another database number.
- The database goes in `SCCACHE_REDIS_DB`, never in the URL: sccache 0.18 ignores a `/5` path and writes to database 0.
- `sccache-backend: local | auto | redis | s3` (default: the Gitea variable `PADDOCK_CI_SCCACHE_BACKEND`, `local` when it is unset;
  `auto` is Redis when an endpoint is set, else local; `redis` fails the job when no endpoint is set).
- **`s3`: the local disk first, the shared RustFS store behind it** (docs/research/2026-11-19-sccache.md section 8, user decision
  2026-11-19; trial numbers in docs/benchmarks.md, "CI's sccache on RustFS"). Settings, all in Gitea (Settings, Actions): the
  variables `PADDOCK_SCCACHE_S3_ENDPOINT` (`http://172.16.1.220:15000`) and `PADDOCK_SCCACHE_S3_BUCKET` (`sccache-paddock`), the
  secrets `PADDOCK_SCCACHE_S3_ACCESS_KEY_ID` and `PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY` (one key for every runner class, scoped to
  `sccache-paddock/paddock-ci/*`), and the variable `PADDOCK_CI_SCCACHE_BACKEND=s3` to switch it on for every run, or the dispatch input `sccache_backend=s3` of ci.yml for one run (delete the variable to switch off: every
  job is back on its local disk at the next run). The key reaches only the `setup-rust` step (each use passes it; conformance C141
  checks that, and that the gates job and the release workflows hold none); the sccache server gets the chain and the key, every
  later step only the local disk. A job falls back to its local disk, with a `::warning::`, when the store's HTTP health check
  (`/minio/health/live`, 5 s at most) fails, and a failed S3 write never fails a build. A store that hangs in the middle of a job
  is cut by the **timeout guard** (`paddock_net::stall_guard`, a localhost proxy `setup-rust` starts in front of the store: a request with no
  byte for 10 s fails, then new connections are refused for 60 s, so sccache uses its local disk; the job log shows what it cut).
  The store must be an `http://` endpoint for that. The store's size is the store's job: before a release check
  `rc bucket lifecycle rule list <alias>/sccache-paddock` shows the 14-day expiry rule, as sccache never evicts from S3.
- `setup-rust` uses the sccache already on the runner, else installs a pinned release (checksum-verified, in the runner's tool
  cache) for Linux x86_64/aarch64, macOS aarch64 and Windows x86_64. Bump the version and the four SHA-256 values together.
- It also sets `CARGO_INCREMENTAL=0` and `SCCACHE_IGNORE_SERVER_IO_ERROR=1`, so an unreachable cache slows a build and
  never fails it.
- Hit rates: the `sccache-stats` step at the end of each job prints `sccache --show-stats`; look at "Cache hits rate". A
  cold first run is all misses; later runs of the same crates should mostly hit.

## Builds

- **One feature set per dependency (cargo-hakari).** With resolver 2, `cargo build -p <crate>` resolves features for that
  crate alone, so a dependency can be compiled again with another feature set than `cargo test --workspace` used (in Debrief, one
  `cargo build -p` inside a test recompiled 18 crates). A workspace-hack crate (`crates/paddock-workspace-hack`, settings in
  `crates/.config/hakari.toml`) depends on every such dependency with the union of its features, and every member depends
  on it, so all builds share one compiled copy. Tauri is left out on purpose (the libraries take it without a webview
  runtime). After adding or changing a dependency: `cargo hakari generate && cargo hakari manage-deps`; CI runs
  `cargo hakari generate --diff`, `cargo hakari manage-deps --dry-run` and `cargo hakari verify`. The crate is never
  published: `scripts/publish.mjs` publishes with `cargo hakari publish`, which drops the dependency for the upload.
- **Unused dependencies (cargo-machete).** CI runs `cargo machete` on every crate. A false report (a dependency used only
  by a macro, or for its features) goes into `ignored` of `[workspace.metadata.cargo-machete]` in `crates/Cargo.toml`, or
  of the crate's own `[package.metadata.cargo-machete]`, with a comment saying why.
- **Build profiles.** `[profile.dev]` (tests inherit it) keeps line tables only (`debug = "line-tables-only"`: backtraces
  still name file and line) and dependencies get no debug info, which shrinks the objects and speeds up linking. For a
  debugger session with variables: `CARGO_PROFILE_DEV_DEBUG=full cargo build`. `split-debuginfo` stays at the platform
  default (macOS already keeps debug info out of the binaries; Windows supports only `packed`).
- **Linkers.** On Windows every workspace of the repo links with the toolchain's `rust-lld.exe` (lld-link) instead of
  MSVC's `link.exe` (`.cargo/config.toml` at the repo root: `[target.x86_64-pc-windows-msvc] linker = "rust-lld.exe"`),
  locally, in CI and in the release builds alike; it still needs Visual Studio's build tools for the MSVC and Windows SDK
  libraries. On x86_64 Linux rustc links with its bundled lld by default (Rust 1.90 and later); macOS uses Apple's `ld`.
- **Static C runtime on Windows.** The same file sets `-C target-feature=+crt-static` for every `*-pc-windows-msvc`
  target (ADR 0032), so no binary needs the VC++ redistributable (`VCRUNTIME140.dll`). Setting `RUSTFLAGS` or
  `CARGO_ENCODED_RUSTFLAGS` replaces every configured flag, this one included: add `-C target-feature=+crt-static` to
  it when you set it for a Windows build. `node conformance/release/windows-crt-imports.mjs <exe-or-folder>` checks a
  build; cargo-xwin cross-builds pick the setting up as well.
- **The sccache shim (ER-47).** cc-rs puts `RUSTC_WRAPPER` in front of the C compiler when its file name is `sccache`, cargo hands that
  variable to every build script, and sccache 0.19 cannot probe `clang-cl` (it reads `/Users/...` as `/U` options), so every
  `cargo xwin` lint that compiles C (ring) failed. cc-rs 1.8 to 1.11 has no switch for one target, so `./dev sccache env` (and the
  gates, `./dev gen check`, heavy's build) set `RUSTC_WRAPPER` to `<tool cache>/tools/sccache/<version>/shim/sccache`, a shell
  program named `sccache` that runs `clang-cl`/`cl` (or a compiler with `--target=...-windows-msvc`) directly and everything else
  through the real sccache. Native C stays cached on macOS and Linux; only the cross C compiles are not. Written by `./dev sccache env`
  only (a gate with no shim turns the wrapper off and says so). Policy: `local_sccache::shim_text`; measured: docs/benchmarks.md.

## Folders tools make (owner records)

A folder a tool creates for an agent (a cargo or cargo-xwin target folder, a test scratch folder, a VM run folder) gets an
owner record, `./dev disk record <dir> --kind <build|scratch|...> --link <worktree>` (on branch `feat/dev-disk` until it
lands), so a cleanup knows whose it is. Callers: engineering-rules `scripts/heavy.sh` records `CARGO_TARGET_DIR` (so every
`cargo`, `cargo xwin` and `llvm-cov` run through it is covered; the xwin target folder is whatever `CARGO_TARGET_DIR` names,
no script here makes one), and `scripts/vm-smoke/run*.mjs` record their scratch folder through `scripts/disk-record.sh`,
which skips quietly where `./dev disk` does not exist. The scratch folder an agent's brief names is made by the brief's
author or by `./dev brief` (not on main yet): that call belongs there. The Rust `heavy` tool (`tools/heavy`) makes no folder.

## Tests

- **Rust:** `cargo nextest run` (each app's `.config/nextest.toml`: one test per core but one, slow-test warnings after
  30 s, and a `ci` profile without retries that stops a test after its 60 s budget, waivers in the file with a reason, because a flaky test is fixed rather than retried, with a JUnit report).
  nextest does not run doc tests: `cargo test --doc` runs them.
- **One integration-test binary per crate:** `tests/it/main.rs` declares one module per test file (`mod pool;` for
  `tests/it/pool.rs`), shared helpers in `tests/it/common/` or `tests/it/support/`. Linking one binary per file cost more
  than running the tests (Debrief went from 239 test binaries to 99). A new test file is a new `mod` line, never a new
  `tests/<name>.rs`; run one file's tests with `cargo nextest run -p <crate> -E 'test(/^pool::/)'` or
  `cargo test -p <crate> --test it pool::`. A test that changes something process-wide (environment variables, the panic
  hook, the global allocator, a measured resident size) either serialises that change with a lock or runs in a child
  process of its own (re-running its own test binary with `--exact`), because `cargo test` runs all modules in one process.
- **Golden files (`paddock_testkit::golden`):** a golden is captured on one machine and checked on all of them (the Mac, the
  Linux CI image, which runs as root, and Windows). A value of the capturing machine (a uid, a home folder, a temp path) makes it
  fail elsewhere; normalise it in the test before the comparison (for example `norm()` in
  `tools/paddock-dev/tests/it/host_steps.rs`), so the file says `<uid>`. Conformance C131 (report-only, ER-41) reads the golden
  folders (and `*.golden` and `*.snap` files) for uid and gid numbers, `/Users`, `/home` and `C:\Users` paths and temp paths, by
  fixed patterns, so it gives the same result on every machine; it walks the working folder today (reading git's tracked list is
  the approved train-14 item). A value that is fine (an invented user) gets an entry with a reason in `conformance/c131-allow.json`.
  **Rewriting:** `PADDOCK_UPDATE_GOLDEN=1` rewrites a differing golden and prints the command that runs that test on Linux as root
  in the CI image (`tools/linux-container.sh --id paddock --ci-image -- sh -c 'cd <package folder> && cargo nextest run -p <package> <test>'`;
  the test name comes from `NEXTEST_TEST_NAME`). Run it before you commit the file. `./dev golden confirm [-p <package>] <test filter>`
  (from the workspace of the test, or `--manifest-dir <folder>`) runs the filtered tests on this machine and in that image and passes
  only when both pass; a filter that selects no test fails with "no test matched the filter", and a filter starting with `-` is
  refused. The container needs Docker or OrbStack and obeys `PADDOCK_LINUX_CPUS`, `PADDOCK_LINUX_JOBS`, `PADDOCK_LINUX_MEMORY`,
  `PADDOCK_LINUX_USER`, `PADDOCK_LINUX_CI_REGISTRY` and `PADDOCK_LINUX_HOST_ARCH` (ER-14), all passed through. On an arm64 Mac the CI
  image is a local arm64 build (not the registry's amd64 image).
- **Checked (conformance C37):** a `tests/<name>.rs` outside `tests/it/` fails unless `ownTestBinaries` of
  `paddock.conformance.json` names it with the reason it needs its own binary, and every workspace member needs a CI test
  step and the workspace-hack dependency.

## Parallel work (people and agents)

- One git worktree per task (`git worktree add -b <branch> ~/.<app>-wt-<task> origin/<base>`); the main checkout stays on
  `main`. Never `git stash` (it is shared by all worktrees).
- Own `CARGO_TARGET_DIR` per worktree (`~/.<app>-target-<task>`). Seed a new one as a copy-on-write clone of a recent one
  to skip the first build: `cp -c -R ~/.<app>-target-<old> ~/.<app>-target-<new>` (APFS; no extra disk until it changes).
- Own e2e port per worktree (each app's Playwright config reads it from the environment and requires it in CI).
- Branches: only the affected tests, Playwright in Chromium only. The combined branch: the full gate once, both
  browsers. Everything green locally before pushing to `main` or tagging; CI confirms, it does not find failures.
- Linux-only failures: run the CI image locally, for example
  `docker run --rm -v "$PWD:/src" -w /src <ci-image> cargo nextest run --workspace` (only containers you created).
- Before starting another agent, ask `./dev load` (the coordinator's speed check, ER-24). It prints one line that starts with
  `start`, `hold` or `stop`, and why. The 1- and 5-minute load, the free space of the disk holding `<dev-targets>` (derived, see "Where things live"), and the
  CI queue (queued plus running runs among the newest 200, read-only through `GITEA_TOKEN`) decide it: `start` while the 5-minute load is under 12
  (on 14 cores, scaled to the core count), nothing is low on space and fewer than 4 runs are queued or running; `hold` between
  12 and 20, on a 1-minute spike over 20, with a long CI queue, or when something could not be measured (never `start` on a
  number that is missing); `stop` at a 5-minute load of 20 or more, or under 50 GiB free on a root. `--json` gives the numbers
  for scripts. The limits are flags (`--start-below`, `--hold-above`, `--min-free-gib`, `--ci-hold`, `--disk <path>`, `--no-ci`).
  It decides nothing by itself: it does not start or stop agents, and `heavy.sh` still caps the heavy steps.

## Things that cost time when forgotten

- Don't run macOS tests under `nice`: it is SIP-protected and strips `DYLD_*` variables, so libraries are not found.
- Don't run `cargo fmt --all` or `npx prettier --write` across a repository others are editing; format your own files.
- Check `uptime` before starting another heavy job.
- Tests clean up after themselves: no temp folders, processes, windows or browser profiles left behind (rule 7).

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

Code, scripts and docs never name a disk (rule 40): a disk gets renamed or replaced, and a hard-coded path then goes
stale without a sound (on 2026-11-14 the dev data moved disks, and the guards, the VM scripts and the dev tool's defaults all
pointed at disks that were gone). Tools derive every folder from one setting per machine, `git config paddock.worktreeRoot`
in the main checkout (repo-local, so every worktree sees it), and fail with an `::error::` and this next step when it is not
set. This section is the one place that names this machine's layout; everything else writes `<dev-targets>` or `<dev disk>`
and links here.

| Name | What is there | How the tools find it |
| --- | --- | --- |
| `<dev-targets>` | worktrees (`wt/`), build folders, agent scratch (`scratch/<agent id>`), `sccache`, `paddock-dev-home`, the disk index `.paddock-disk.sqlite`, heavy's run logs (`runs/<agent>/`), the shared npm cache `cache/npm` | `PADDOCK_DEV_TARGETS`, else the parent of `paddock.worktreeRoot`, else of `PADDOCK_WORKTREE_ROOT` |
| `<dev disk>` | the folder that holds `<dev-targets>` | the parent of `<dev-targets>` |
| `<dev disk>/vm-data` | the Tart images `tart-from-home` of the service smoke | `PADDOCK_VM_DATA`, else derived |
| `<dev disk>/ci/locks/test-machines` | the lock of the test machines (`scripts/vm-lock.sh`) | `PADDOCK_VM_LOCK_ROOT`, else derived |
| the main checkout | `paddock`, with `engineering-playbook` next to it | the folder that holds the git common directory |

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

## Shared npm cache (`./dev deps`)

`npm ci` and the `ui/dist` build are done once per content key, not once per worktree. `./dev deps install` gives the worktree its
`node_modules` (the root's and each workspace's) and `./dev deps ui` adds `ui/dist` and the generated css: each is a copy-on-write
clone (APFS `clonefile` through `cp -c`, a reflink where the file system has one, else a plain copy) of an entry in
`<dev-targets>/cache/npm/`, or the one build that makes the entry. A clone is a real folder of its own, never a link into the cache,
so Angular, Vitest and Playwright resolve ordinary paths and no worktree can change another's. `./dev worktree add` clones a warm
`node_modules` at once; with a cold cache it keeps installing only the conformance dependencies and never builds.

- **Keys.** `node_modules`: `package-lock.json`, the Node version and the platform. `ui/dist`: the build guard's own content hashes
  (`sourceHash` in `@decryp7/paddock-e2e`, read from Node), the `node_modules` key and the hash of `ui/angular.json`,
  `ui/package.json`, `ui/tsconfig.json`, `ui/scripts/` and `tsconfig/`. A changed lockfile or source is a new key (a miss), never an
  edit of an old entry.
- **Owner, lock, size, eviction (ER-51).** Owner: `./dev deps` (`owner.json` in the folder). One kernel file lock per key
  (`<name>.lock`), exclusive while an entry is built and stored, shared while one is only cloned out, so two builders of one key
  build once and an entry is never read half made. An entry is made in a staging folder and renamed into place; `entry.json` records
  its folders and file count, and an entry that fails the check is removed and rebuilt. Cap 10 GiB (`PADDOCK_DEPS_CACHE_MAX_GIB`);
  over it, the least recently used entry (folder modification time, set on every hit) goes first, never the one just used or one
  whose lock is held. Every deletion goes through `safe_remove`.
- **Measurements:** [docs/benchmarks.md](benchmarks.md#2026-11-26-shared-npm-and-uidist-cache-featnpm-ui-cache).

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

Agents' builds use one local sccache 0.20.1 (the CI pin and checksums, `tools/paddock-dev/src/ci_setup_rust.rs`), set by Paddock's
tools and never by your shell profile (user decision 2026-11-16; docs/research/sccache-basedirs-2026-11-16.md):

- `eval "$(./dev sccache env)"` installs the pinned release into `<PADDOCK_DEV_HOME or ~/.paddock-dev>/tools/sccache/0.20.1/` (never
  `~/.cargo`), starts the one server on the default port 4226, and prints `export RUSTC_WRAPPER=...` lines for that shell. Run it
  once per machine to install; the push gates and `./dev gen check` then use the installed copy by themselves and never download.
- **Always on, in any shell.** Agents' Bash tool starts a fresh shell for every command, so an `eval` per shell never held (2026-11-26:
  the live server had 0 compile requests in three hours of builds). `./dev sccache env` therefore also writes
  `<worktree root>/.cargo/config.toml` (first line marks it as this tool's; a file of somebody else's is never touched) with
  `[build] rustc-wrapper = "<shim>"`. Cargo reads the `.cargo/config.toml` of every folder above where it runs, so every cargo in every
  worktree, `./dev`'s own build included, compiles through the shim in any shell; the environment's `RUSTC_WRAPPER` still wins (cargo's
  rule: CI's own, whose checkouts are not below this root, and a developer's). It is a machine file, not a line in the repository, because
  Windows cannot start an extensionless script as `rustc-wrapper` (and a `.cmd` file is CRLF, which a shell script cannot be, ER-61). The
  shim sends a rustc compile to sccache only when our server listens on the port; with none, the launcher recorded in
  `<tool cache>/sccache-run/launcher` starts ours (one start at a time, `start.lock`), and when it is still down the compile runs plain
  rustc, so a build never fails because of the cache. To switch it off, delete the file.
- The server starts with `SCCACHE_BASEDIRS` (sccache 0.20: the longest matching base is stripped from every path in a Rust compile's
  key) naming exactly this repository's worktrees, the main checkout included (`git worktree list`, wherever they live; never a
  sibling folder that is another repository), and `SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS=CARGO_MANIFEST_DIR,CARGO_MANIFEST_PATH`
  (crates that bake those into their output stay per checkout). One server serves every worktree and reads the list only when it
  starts, so `./dev sccache env` records what the server started with (`<tool cache>/sccache-run/start-stamp`: the bases and the key
  prefix) and, when the worktree list or the prefix changed, stops our server through its own client (`sccache --stop-server` on our
  port, never a kill) and starts it again once; a server that is not ours is never touched, and builds keep going during the restart
  (`SCCACHE_IGNORE_SERVER_IO_ERROR`). Only `./dev sccache env` restarts; the push gates never do. After `./dev worktree add`, run
  `eval "$(./dev sccache env)"` in the new worktree once: that is what puts it into the list.
- sccache hashes every `CARGO_*` variable of a compile into its key, and CI's setup-rust exports `CARGO_INCREMENTAL=0` and `CARGO_TERM_COLOR=always`
  to every job with the cache. The wrapper shim exports the same two for a rustc compile (`CI_KEY_ENV` in `local_sccache.rs`), so a local key equals CI's and a build here
  reuses what CI stored (docs/benchmarks.md, "Local keys against CI's"); your shell and cargo's output are unchanged.
- It is off, with one `::warning::` line and a build without the cache, when sccache is not installed, the server does not
  start, or a server on the port uses another cache. The server runs in its own short folder (`<tool cache>/sccache-run`, also its `TMPDIR`, working folder and error log
  `sccache.log`; short because macOS caps socket paths at 104 bytes), never in a job's or an agent's scratch folder, which a job deletes. It
  never exits on its own (`SCCACHE_IDLE_TIMEOUT=0`; stop it with `sccache --stop-server`), so a build that finds none cannot replace it
  with one that has the build's environment. When it comes up read-only (sccache 0.19.0 keeps a failed startup write check for the
  server's whole life: no compile is stored) the tool stops it, starts it once more, and if it is still read-only turns the wrapper off
  and names the log in the warning. It is off without a warning in CI (`CI`, `GITHUB_ACTIONS`), with
  `PADDOCK_SCCACHE=off`, or when `RUSTC_WRAPPER` is already set.
- The build folder goes in as `--config build.target-dir=...` in every tool (see AGENTS.md, "Build folders").
- **One setting picks the backend, `PADDOCK_SCCACHE_BACKEND`:** `auto` (the default, user decision 2026-11-21: the local disk first
  with the shared RustFS store behind it, as CI's `s3` backend, when `~/.config/paddock/sccache-s3.env` (below) is there and the store
  answers its HTTP health check, `/minio/health/live`, within 5 s; otherwise the local disk alone, with one `::notice::` (no file: the
  shared cache is not set up on this machine) or `::warning::` (an unreadable file, a store that does not answer) naming the cause),
  `local` (the disk only; `SCCACHE_DIR` and `SCCACHE_CACHE_SIZE` from the profile above) or `s3` (the store alone, through sccache's
  own S3 support; needs `PADDOCK_SCCACHE_S3_BUCKET` and `PADDOCK_SCCACHE_S3_ENDPOINT`). The store is `PADDOCK_SCCACHE_S3_ENDPOINT`
  (`host:port`, default `172.16.1.220:15000`) and `PADDOCK_SCCACHE_S3_BUCKET` (default `sccache-paddock`), each taken from the environment,
  else from an optional line of that name in the key file below, else the default (the one place the defaults are named: this file; the code
  holds them as `DEFAULT_S3_ENDPOINT` and `DEFAULT_S3_BUCKET` in `local_sccache.rs`); optionally
  `PADDOCK_SCCACHE_S3_PREFIX` (default CI's for this OS and arch, `paddock-ci/<OS>-<arch>`, `ci_setup_rust::ci_prefix`; a server still on the older `paddock` prefix is restarted once) and, for `s3`, `PADDOCK_SCCACHE_S3_REGION` (default `auto`) and
  `PADDOCK_SCCACHE_S3_SSL` (`true`; default `false`). With `auto` the first level is the sibling folder `<SCCACHE_DIR>-s3` (what S3
  backfills never reaches the plain folder, as in CI, ADR 0111): the first build after the switch starts from the store, not from
  the old folder, and the two folders together use up to twice `SCCACHE_CACHE_SIZE` on the Mac's disk (each is capped at that size; lower
  the size, or delete the plain folder once the shared cache is in use). The store's health check is CI's own (`curl` with a 3 s connect
  limit, DNS included, and a 5 s total limit). If the store was down at the first `sccache env`, the plain-disk server that call started
  is stopped and started again with the store behind the disk at the next call that finds the store up (one `::notice::`); a server on
  another folder or setup is never touched. The keys go to the server's environment only; builds see the disk setting alone.
- **S3 credentials are never in the repository, a log or a message.** They are read from the key file
  `~/.config/paddock/sccache-s3.env` (`$XDG_CONFIG_HOME/paddock/sccache-s3.env`, or the file `PADDOCK_SCCACHE_ENV_FILE` names): two
  `NAME=value` lines, mode 600 (a file others can read is refused), and only when it holds none from `AWS_ACCESS_KEY_ID` and
  `AWS_SECRET_ACCESS_KEY` in the environment. A stray `AWS_*` in your shell (another project's login) never outranks the file: it is
  ignored for sccache, with one `::notice::`. The tools
  print only `credentials configured` or `credentials missing`. The macOS keychain is not read by the tools (it would raise a
  dialog); export the key from it into the file yourself.
- One server takes one cache at a time. To switch the backend, run `sccache --stop-server` once, then the next build starts the
  server with the new setting. A server on the port that uses another cache is never stopped by the tools.

The shared RustFS cache (S3 API `http://172.16.1.220:15000`, bucket `sccache-paddock`, 50 GiB quota, policy `sccache-paddock-rw`) is
used by default once the key file above exists; nothing else to set. A server already running with the old local-only setting is
left alone with a warning: run `sccache --stop-server` once. Create the access key bound to that policy in the RustFS console (port 15001) and write it as:

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

CI Rust jobs use sccache's **local disk first, the shared RustFS store behind it** on every OS (user decision 2026-11-21): the
job slot's kept folder on a host runner, `/sccache` in a container, then the store. The `setup-rust` input `sccache-backend` and
`./dev ci setup-rust` (`ci_setup_rust.rs`) default to `s3`; no repository variable switches it, and ci.yml's dispatch input
`sccache_backend` (`local` or `s3`) overrides one run for a measurement. A release, rule 54 or tag run stays on the plain local disk
(below). The shared Dragonfly (Redis-compatible) server at `172.16.1.188:6379` was CI's backend until 2026-10-25: it
filled up, the Mac job's sccache server failed to start and every rustc call failed (commit 1383e9898); the user decided against a
shared backend on 2026-10-28 and again on 2026-11-17 (CI keeps its per-slot caches). The Redis path is kept for a deliberate trial:

- Gitea Actions variables (set by the user, org or repo level): `SCCACHE_REDIS_ENDPOINT=redis://172.16.1.188:6379` and
  `SCCACHE_REDIS_DB=5`. The `setup-rust` action reads them as its input defaults; they are used only with `auto` or `redis`.
  sccache uses **database 5 only**; never use another database number.
- The database goes in `SCCACHE_REDIS_DB`, never in the URL: sccache 0.18 ignores a `/5` path and writes to database 0.
- `sccache-backend: s3 | local | auto | redis` (default `s3`; `auto` is Redis when an endpoint is set, else local; `redis` fails the job when no endpoint is set).
- **`s3`: the local disk first, the shared RustFS store behind it** (docs/research/2026-11-19-sccache.md section 8, user decision
  2026-11-19; trial numbers in docs/benchmarks.md, "CI's sccache on RustFS"). Settings, all in Gitea (Settings, Actions): the
  variables `PADDOCK_SCCACHE_S3_ENDPOINT` (`http://172.16.1.220:15000`) and `PADDOCK_SCCACHE_S3_BUCKET` (`sccache-paddock`), the
  secrets `PADDOCK_SCCACHE_S3_ACCESS_KEY_ID` and `PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY` (one key for every runner class, scoped to
  `sccache-paddock/paddock-ci/*`). The key reaches only the `setup-rust` step (each use passes it; conformance C141
  checks that, and that the gates job and the release workflows hold none); the sccache server gets the chain and the key, every
  later step only the local disk. **A release, rule 54 or tag run never reads the shared store** (any branch could have written to
  it: cache poisoning): its workflows hold no key (C141), `./dev ci setup-rust` refuses S3 for a tag ref, a tag rehearsal and `PADDOCK_CI_RELEASE=true` (setup-rust sets it from
  `conformance/ci/kit.sh release-path`, the one definition: tag, rehearsal, or the `PADDOCK_DEV_DOWNLOAD=0` that release and rule 54
  workflows set; the guard never reads that flag itself, since setup-rust sets it for any job without a read token too) with a
  `::notice::` that names the trigger, and the action's shell path takes `s3` as `local`; tests: `the_default_on_a_tag_or_release_run_is_the_plain_local_disk`,
  `the_release_paths_shell_steps_never_use_s3`. A job with no secret passed gets one `::notice::` naming that, and its local disk. A job falls back to its local disk, with a `::warning::`, when the store's HTTP health check
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
- **A dependency added to a shared crate (`./dev locks guard`).** Twice (`toml`, then `oxc` and `syn` in `paddock-repo-graph`) a new
  dependency of a shared crate changed the workspace hack and the features Cargo resolves for `serde_json` (oxc_resolver forces
  `preserve_order`), so every app's `Cargo.lock` changed and Clippy broke in paddock-mcp under `--all-features`. `./dev locks guard`
  fails (report-only for now: warnings, exit 0) when the workspace hack is not what cargo-hakari generates, when the features of a
  watched crate differ from `docs/shared-crate-features.toml`, or when a `Cargo.lock` would change. The watch list is that file's
  `watch` array; the pins are generated: `./dev locks guard --write` after `./dev locks sync`, and the diff of the file is reviewed
  with the change. The push gate runs it as the `locks-guard` row when a manifest, a lockfile, `hakari.toml` or the pins file changed,
  and CI runs it in the `crates` job and in the train gates.
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
lands), so a cleanup knows whose it is. Callers: engineering-playbook `scripts/heavy.sh` records `CARGO_TARGET_DIR` (so every
`cargo`, `cargo xwin` and `llvm-cov` run through it is covered; the xwin target folder is whatever `CARGO_TARGET_DIR` names,
no script here makes one). The scratch folder an agent's brief names is made by the brief's
author: that call belongs there. The Rust `heavy` tool (`tools/heavy`) makes no folder.

## Tests

- **Rust:** `cargo nextest run` (each app's `.config/nextest.toml`: one test per core but one, slow-test warnings after
  30 s, and a `ci` profile without retries that stops a test after its 60 s budget, waivers in the file with a reason, because a flaky test is fixed rather than retried, with a JUnit report).
  Doctests are off everywhere (`[lib] doctest = false`; crates/README.md, "Doctests"): doc examples are text, behaviour is in unit tests.
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
  `tools/paddock-dev/tests/it/host_steps.rs`), so the file says `<uid>`. Conformance C131 (blocking since ci.yml 8869, ER-41) reads the golden
  folders (and `*.golden` and `*.snap` files) for uid and gid numbers, `/Users`, `/home` and `C:\Users` paths and temp paths, by
  fixed patterns, so it gives the same result on every machine; it walks the working folder today (reading git's tracked list is
  the approved train-14 item). A value that is fine (an invented user) gets an entry with a reason in `conformance/c131-allow.json`.
  **Rewriting:** `PADDOCK_UPDATE_GOLDEN=1` rewrites a differing golden and says that CI's Linux job confirms it as root on the Linux runner
  (push the branch). No container or VM runs on this Mac (user, 2026-11-26; docs/rules.md rule 101), so `./dev golden confirm` and the
  local CI-image run are gone.
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

## Mac CI runner (ADR 0120)

The Mac keeps one runner, `d7-mac-mini`, with capacity 4, as a LaunchDaemon: it leaves the user's GUI login session, runs as
the same account (`UserName`, so its toolchains, folders and Full Disk Access grants stay) and has no window server. There is
no GUI lane: jobs that need a window run elsewhere (ADR 0120, D1 and D8 as changed on 2026-11-25). `d7-mac-mini-2` and `-3`
are removed: stopped, their plists parked in `~/gitea-runner-parked-2026-11-25/`, and to be deleted from Gitea's runner list.

The user runs these; an agent never does (rule 7, ER-14). Run them from the repository root. A running job is waited for, visibly
and bounded, before anything changes: `install` and `rollback` tell the runner to take no new job (`launchctl kill SIGTERM`), print
which job runs and how long they have waited, and stop, swap and start only when every job has ended. After `--wait-max SECONDS`
(default 3600) they fail with the old runner still loaded and nothing changed; they never end a job or its helpers, whose own cleanup
does (ER-51). The new daemon must be `state = running` in `launchctl print` and log "declare successfully", or the lane fails.

1. **Restart.** A planned restart is `sudo fdesetup authrestart` (D5, amended 2026-11-25).
2. **Look first:** `bash tools/mac-runner-move.sh install --user decryp7 --daemon d7-mac-mini --retire d7-mac-mini-2 --retire d7-mac-mini-3 --dry-run`
   (no sudo, changes nothing: it renders and shows the plist and prints every step).
3. **Install:** the same line with `sudo` and without `--dry-run`. It ends with one `ok` or `fail` line: `ok` means launchd
   runs the runner in the system domain, as your account, and its log says `declare successfully` within 90 s
   (`--verify-max`). The original agent is kept as `~/Library/LaunchAgents/net.decryptology.gitea-runner.d7-mac-mini.plist.before-adr0120`.
4. **Check later:** `bash tools/mac-runner-move.sh status --user decryp7 --daemon d7-mac-mini --retire d7-mac-mini-2 --retire d7-mac-mini-3`,
   the log `/Library/Logs/gitea-runner/d7-mac-mini.log`, `sudo launchctl print system/net.decryptology.gitea-runner.d7-mac-mini`,
   and Gitea's runner list (`d7-mac-mini` online, with capacity 4 from `~/gitea-runner/config.yaml`).
5. **Roll back:** `sudo bash tools/mac-runner-move.sh rollback --user decryp7 --daemon d7-mac-mini --retire d7-mac-mini-2 --retire d7-mac-mini-3`
   removes the daemon, puts the parked agent back and starts it in your session. The `-2` and `-3` plists parked by hand stay
   where they are. `--dry-run` shows the steps first.

After a restart: once the disk is unlocked (by `authrestart`, or by your password at the FileVault screen) the runner starts
at boot, before anyone logs in. The external disk mounts without a login because `AutomountDisksWithoutUserLogin` is 1 (the
install checks it). A crash restart waits at the FileVault screen for the password, and the runner does not run until then.

### Full Disk Access by a stable identity (user decision 2026-11-25, option A+B)

The runner's work folder is on `/Volumes/vault-dev`, which macOS privacy (TCC) treats as a removable volume. A LaunchDaemon has
no window, so a missing grant is a silent deny, never a prompt (the daemon's checks go to the system `tccd`: the log says
`user tccd unavailable, forwarding ... to system tccd`). TCC checks the stored *designated requirement* (DR) of a program
([TN3127](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements), "Designated
requirement"); an ad-hoc signature's DR is its cdhash, so every runner update and every new test browser is a stranger
([Apple DTS](https://developer.apple.com/forums/thread/795739): ad hoc signed code "does not include a stable DR"). Full Disk
Access is checked before the Removable Volumes service ([Eclectic Light](https://eclecticlight.co/2026/04/20/privacy-how-locations-are-protected/)).

So the runner and the programs that are their own responsible process carry one self-signed identity, `Paddock CI code signing`
(`tools/mac-ci-signing.sh`), and its DR, `identifier "<id>" and certificate leaf = H"<sha1>"`, is the same for every update
signed with it. The runner's children (git, cargo, rustc, test binaries, sccache) inherit the runner's grant and are not
signed; CI builds are never signed.

| Program | Identifier (TCC entry) | Signed by |
| --- | --- | --- |
| `~/gitea-runner/gitea-runner` | `net.decryptology.gitea-runner` (by its path) | `tools/mac-runner-move.sh install` (as you, after the old runner stopped) |
| Chrome for Testing (`install-test-browser`, every copy and pin) | `com.google.chrome.for.testing` (by bundle id) | `conformance/ci/install-test-browser.mjs` after each install |
| Playwright's browsers (any CI job; today Dyno's screenshots) | Chromium's and WebKit's bundle ids; the headless shell `net.decryptology.paddock.ci.chrome-headless-shell` | `conformance/ci/playwright-browsers.sh`, the one install point: one fixed folder on the boot disk, `~/Library/Caches/paddock-ci/ms-playwright`, for every Playwright version |

The keychain is a file of its own, `~/Library/Keychains/paddock-ci-signing.keychain-db`, unlocked with the random password in
`~/.config/paddock/ci-signing.pass` (0600), because the daemon has no unlocked login keychain and no window for a dialog. The CI
hooks find it at that default path (`PADDOCK_CI_SIGNING_KEYCHAIN` only overrides it), so the runner needs no configuration; without
it they print a notice and sign nothing. They sign only in a CI job (`GITEA_ACTIONS` or `GITHUB_ACTIONS`, which the runner sets in every job; the one rule is `mac-ci-signing.sh --ci-hook`), so a
developer's Mac and the tests never sign, even with the identity in place.

**Your one-time steps** (an agent never runs these; rule 7, ER-14). From the repository root, as yourself:

1. **Create the identity** (once; idempotent, `--dry-run` shows the steps): `bash tools/mac-ci-signing.sh create`, then
   `bash tools/mac-ci-signing.sh identity` prints its SHA-1.
2. **Sign the runner and restart it:** `sudo bash tools/mac-runner-move.sh install --user <you> --daemon d7-mac-mini --retire d7-mac-mini-2 --retire d7-mac-mini-3`
   (it stops the runner after its running job, signs it, starts it and checks it declared itself). `status` then says
   `signature the Mac CI identity`.
3. **Full Disk Access** (System Settings, Privacy & Security, Full Disk Access). Remove the old entries first, since they hold the
   ad-hoc cdhash and no longer match: `gitea-runner` (all three: `~/gitea-runner/`, `~/gitea-runners/d7-mac-mini-2/`,
   `~/gitea-runners/d7-mac-mini-3/`). Then add with `+` (Cmd-Shift-G types a path; Cmd-Shift-. shows hidden folders), each
   switched on:
   - `~/gitea-runner/gitea-runner`
   - one signed copy of `Google Chrome for Testing.app`, for example
     `/Volumes/vault-dev/ci/home/test-browser-paddock-s1/chrome-mac-arm64/Google Chrome for Testing.app` (after a Mac CI run
     has installed and signed it; check with `bash tools/mac-ci-signing.sh check "<that .app>"`). The entry is by bundle id, so
     it covers every copy and every later pin.
4. **Check:** `bash tools/mac-ci-signing.sh check --identifier net.decryptology.gitea-runner ~/gitea-runner/gitea-runner`, and
   after the next Mac run `log show --last 1h --predicate 'subsystem == "com.apple.TCC" AND eventMessage CONTAINS "Removable"'`.

**After a runner update:** replace the binary, then run step 2 again. The Full Disk Access entry stays (same path, same DR).
**A new browser pin** is signed by CI and needs nothing. **Not needed:** an entry for Playwright's browsers (they live on the boot
disk and are signed for when they do touch the volume; add `chrome-headless-shell` only if the TCC log names it, as its entry is by
path and the path holds Playwright's revision), and for `pitcrew-service` (CI runs it only as a child of the runner).
**Rotating the certificate** (it is valid for 10 years) means: move the keychain away, `create`, then steps 2 and 3 again.

## Things that cost time when forgotten

- Don't run macOS tests under `nice`: it is SIP-protected and strips `DYLD_*` variables, so libraries are not found.
- Don't run `cargo fmt --all` or `npx prettier --write` across a repository others are editing; format your own files.
- Check `uptime` before starting another heavy job.
- Tests clean up after themselves: no temp folders, processes, windows or browser profiles left behind (rule 7).

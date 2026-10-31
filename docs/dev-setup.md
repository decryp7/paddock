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

## Environment

Put these in the shell profile (`~/.zshenv` on macOS):

```sh
export RUSTC_WRAPPER=sccache        # shared compile cache; `sccache --show-stats` shows the hit rate
export SCCACHE_DIR=/Volumes/d8sst7/dev-targets/sccache   # on the big volume (Mac mini), not the system disk
export SCCACHE_CACHE_SIZE=60G       # the default 10G is too small for several apps
export CARGO_BUILD_JOBS=2           # per job; several run at once (Mac mini: about five heavy jobs at most)
```

Change `RUSTC_WRAPPER` only between builds: it is part of Cargo's fingerprint, so changing it rebuilds the target folder.

**sccache.** One server per user serves every checkout and worktree; the cache lives on the big volume
(`SCCACHE_DIR`, Mac mini: `/Volumes/d8sst7/dev-targets/sccache`, next to the per-worktree target folders). It caches
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

CI Rust jobs on every OS (Linux, Windows, macOS) share one compile cache: a Dragonfly (Redis-compatible) server at
`172.16.1.188:6379`, no password, cache mode, shared with other services. sccache uses **database 5 only**; never use
another database number. sccache's keys include the compiler and target, so one database serves every OS.

- Gitea Actions variables (set by the user, org or repo level): `SCCACHE_REDIS_ENDPOINT=redis://172.16.1.188:6379` and
  `SCCACHE_REDIS_DB=5`. The `setup-rust` action reads them as its input defaults. Unset, sccache falls back to its local disk.
- The database goes in `SCCACHE_REDIS_DB`, never in the URL: sccache 0.18 ignores a `/5` path and writes to database 0.
- `sccache-backend: auto | redis | local` (default `auto`: Redis when an endpoint is set). Use `local` on a runner whose
  disk is faster than the network (the macOS runner, if it proves so); `redis` fails the job when no endpoint is set.
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
- **Checked (conformance C37):** a `tests/<name>.rs` outside `tests/it/` fails unless `ownTestBinaries` of
  `paddock.conformance.json` names it with the reason it needs its own binary, and every workspace member needs a CI test
  step and the workspace-hack dependency.

## Test VMs and real browsers

Tests never start a real browser or the real app on a desktop someone uses (rule 7). Where they may:

- **Linux CI containers:** the jobs run them under `xvfb-run` with `PADDOCK_LIVE_BROWSER=1` (Debrief's `rust tests`, the
  `crates` coverage job, app-gates' `display` wrap). On a Linux desktop, `PADDOCK_LIVE_BROWSER=1` would open real
  windows on it: set it only under `xvfb-run` or in a VM.
- **Local test VMs, never in CI:** `PADDOCK_LIVE_BROWSER=vm`, honoured only in a VM (`paddock_testkit::live_browser`;
  anywhere else it fails the test instead of starting anything). A macOS VM (Tart) is recognised by
  `kern.hv_vmm_present = 1`. A Windows test VM by the machine-wide registry value `HKLM\SOFTWARE\Paddock` `TestVM` = 1,
  which only an administrator can write; the VM's set-up writes it once, and it never goes on a desktop someone uses:

  ```powershell
  reg add HKLM\SOFTWARE\Paddock /v TestVM /t REG_DWORD /d 1 /f   # elevated, in the test VM only
  ```

Outside those places a real-browser test prints where it runs and returns. In test builds each driver's launch and
self-test also refuse an installed browser (paths compared canonically), and any real program outside those places,
however a test found it: only a test's fakes pass, which carry the line `# paddock-fake-browser`
(`paddock_testkit::live_browser::fake_script`); a `#!` line alone proves nothing, since Chrome's and Edge's Linux
launchers are scripts too. Conformance C55 is the backstop. The macOS and Windows real-browser runs, in the VMs, before a release that touches the browser code:

```sh
# shared crates (paddock-chromium), Spotter, Debrief: from each Cargo workspace, in the VM
PADDOCK_LIVE_BROWSER=vm PADDOCK_REQUIRE_TEST_BROWSER=1 cargo nextest run -p paddock-chromium --all-features -E 'binary(it) & test(/^real_browser::/)'
PADDOCK_LIVE_BROWSER=vm SPOTTER_REQUIRE_TEST_BROWSER=1 cargo nextest run -p spotter-render -E 'binary(real_browser)'
PADDOCK_LIVE_BROWSER=vm DEBRIEF_REQUIRE_TEST_BROWSER=1 cargo nextest run -p chromium-driver -p debrief-app \
  -E '(binary(it) & test(/^real_browser::/)) | test(/browser::renderer::tests::(reads_a_scripted|a_challenge_page)/)'
```

`CHROMIUM_TEST_PATH` (and Spotter's `SPOTTER_TEST_BROWSER`) name the pinned Chrome for Testing inside the VM
(`node conformance/ci/install-test-browser.mjs`). The real-window probe of `paddock-dialogs`
(`PADDOCK_REAL_WINDOWS=1 ... --test show_probe`) and Debrief's real-app smoke run there too (apps/debrief/docs/testing-manual.md).

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

## Things that cost time when forgotten

- Don't run macOS tests under `nice`: it is SIP-protected and strips `DYLD_*` variables, so libraries are not found.
- Don't run `cargo fmt --all` or `npx prettier --write` across a repository others are editing; format your own files.
- Check `uptime` before starting another heavy job.
- Tests clean up after themselves: no temp folders, processes, windows or browser profiles left behind (rule 7).

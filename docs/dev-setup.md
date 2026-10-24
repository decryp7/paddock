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
| Docker (or OrbStack) | run the Linux CI image locally before pushing | Docker Desktop or OrbStack | docker | Docker Desktop |
| Playwright browsers | e2e: Chromium in branches, Chromium + WebKit on the combined branch and in CI | `npx playwright install chromium webkit` in `app/` | `npx playwright install --with-deps chromium webkit` | `npx playwright install chromium` |
| Chrome for Testing (pinned) | tests that drive a real browser; never the installed Google Chrome (rule 7) | each app's `install-test-browser` script sets `CHROMIUM_TEST_PATH` | same | same |

## Environment

Put these in the shell profile (`~/.zshenv` on macOS):

```sh
export RUSTC_WRAPPER=sccache        # shared compile cache; `sccache --show-stats` shows the hit rate
export SCCACHE_CACHE_SIZE=60G       # the default 10G is too small for several apps
export CARGO_BUILD_JOBS=2           # per job; several run at once (Mac mini: about five heavy jobs at most)
```

Change `RUSTC_WRAPPER` only between builds: it is part of Cargo's fingerprint, so changing it rebuilds the target folder.

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

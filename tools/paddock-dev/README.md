# paddock-dev (`./dev`)

The developer and CI tool of ADR 0022. Run it with `./dev <command>` (`dev.cmd` on Windows) from anywhere in the repository.

- **Binary cache.** `./dev` names the binary by an input hash (`inputs.txt` plus the compiler cargo uses) and keeps it in
  `$PADDOCK_DEV_HOME/bin/<hash>/` (default `~/.paddock-dev`). **Agents set `PADDOCK_DEV_HOME=/Volumes/d7sst7/dev-targets/paddock-dev-home`
  (never a folder in `~`); CI sets it under `PADDOCK_CI_HOME`. Tests always use a temporary `PADDOCK_DEV_HOME`.**
- **Build folder.** A miss, or uncommitted changes under an input path, builds into `$PADDOCK_DEV_TARGET_DIR`
  (default `tools/paddock-dev/target/`, ignored by git). Agents point it outside `~`, with `CARGO_BUILD_JOBS=2`, through `heavy.sh`.
- **Release jobs** set `PADDOCK_DEV_FRESH=1`: build from source in a fresh folder, no compiler wrapper.
- Commands today: `self hash`, `ci slot acquire|release|status`, `fetch chrome|libduckdb|pin|llama|ffmpeg`, `graph affected`. `ci slot acquire` prints `sN <token>` (JSON:
  `slot`, `token`); `release --slot sN --token <token>` frees only the holder that token belongs to. The holder is a detached process
  started in the CI home; it ends when the `--owner` job folder is gone, so run the tool from outside that folder. `--json` writes one object; exit codes are
  `paddock-cli`'s (2 usage, 4 refused, ...).
- When a path dependency in `crates/` changes its dependencies, regenerate this lockfile in the same change.

## `dev fetch` (R3)

Pinned downloads on `paddock-runtime` catalogues, `paddock-download` and `paddock-archive`. Every command runs as one `paddock-jobs` job
on a small pool, so progress is phase-labelled on standard error from the first moment ("Connecting", "Downloading 12.3 MB of 195 MB",
"Verifying", "Unpacking", "Installing"), Ctrl+C cancels in every phase, and an archive is checked against its pinned size and SHA-256
before it is unpacked or used; a failed or cancelled fetch leaves no half-written folder or file behind.

| Command | What it does | Replaces |
|---|---|---|
| `fetch chrome [--catalogue F] [--dir D] [--platform K] [--env-prefix P]` | installs the Chrome for Testing build a catalogue pins (default `crates/paddock-chromium/src/chromium.toml`); prints `CHROMIUM_TEST_PATH=...` and appends it to `$GITHUB_ENV` | `conformance/ci/install-test-browser.mjs --catalogue` (still used by the shared action until R1/R2) |
| `fetch libduckdb [--cache D] [--shell sh\|powershell] [--cargo-config]` | Debrief's pinned libduckdb; prints the `export` lines, `--cargo-config` writes `apps/debrief/.cargo/libduckdb.toml` | `cargo xtask libduckdb` (deleted); `fetch-libduckdb.sh`, `libduckdb.mjs`, `libduckdb-env.sh` stay until CI calls the tool (R2) |
| `fetch pin <catalogue.toml> [--version V \| --chrome-for-testing [CHANNEL]]` | re-pins version, URLs, sizes and SHA-256 with `toml_edit` (comments and order stay) | `conformance/ci/pin-runtime.mjs` (stays: `install-test-browser.mjs` imports it) |
| `fetch llama [--tag T] [--verify] [--out F]...` | re-pins `llama-runtimes.json` | `update-llama-runtimes.py` (deleted) |
| `fetch ffmpeg [--tag T] [--branch B] [--verify] [--out F]...` | re-pins `ffmpeg-runtimes.json` | `update-ffmpeg-runtimes.py` (deleted) |

Online access is limited to what a command names: Chrome for Testing's storage and channel list, the DuckDB release on GitHub, the URLs of
the catalogue being pinned and the GitHub release API of the one repository a manifest comes from (`$GITHUB_TOKEN` lifts the anonymous
rate limit). **Parallelism and cost** (rule 51): the work is network-bound, so `--jobs` (default 3, at most 8) archives are measured or
verified at once; each download uses up to four connections and the downloader caps a host at four across all of them, so more jobs
would only queue. One pass per archive: the digest is computed while the bytes are written and a measured archive is deleted at once.
Tests use a fake transport or a loopback server and never the real network.

## Not ported yet (R0b gaps, listed on purpose)

`graph affected` reproduces `scripts/affected.mjs`'s plan for a file list, a base and head revision, and `--all` (recorded cases in
`tests/golden/`, and the script itself as a live oracle while it exists). Not ported until R4: `--ci` (the base from the event, the
Gitea API for the last green run, the job outputs and summary), `--app` / `PLAN_APP`, the CI matrix legs and nextest narrowing.
The prebuilt download of the binary, `dev-tool.yml` and the `dev-build` job, and `setup-rust`'s image fast path are R0b too.

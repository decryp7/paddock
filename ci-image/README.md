# ci-image: the shared CI image (plan X53)

`paddock-ci` is what every Linux CI job of the Paddock family runs in. It builds on the `garage` image (Rust toolchain with clippy,
rustfmt and llvm-tools, cargo-nextest, cargo-llvm-cov, cargo-deny, Tauri's Linux libraries, Xvfb and D-Bus, Node, Playwright's
Chromium and WebKit with their dependencies, sccache) and adds what jobs still fetched on every run.

| Tool | Pin | Source |
|---|---|---|
| sccache | 0.18.0 (same checksums as `.gitea/actions/setup-rust`) | GitHub release, SHA-256 checked |
| cargo-hakari, cargo-machete | 0.9.39, 0.9.2 | GitHub releases, SHA-256 checked |
| libduckdb | 1.5.6 (the `duckdb` crate's), in `/opt/libduckdb`, `DUCKDB_LIB_DIR` set | GitHub release, SHA-256 checked |
| Chrome for Testing | 156.0.8078.47 (the build the apps download: `paddock-chromium`'s `chromium.toml`; Playwright keeps its own Chromium), `CHROMIUM_TEST_PATH` set | Google storage, SHA-256 checked (amd64 only) |
| Node | the major in `/.nvmrc` (checked, not installed) | garage |
| Playwright | the version `ui/package-lock.json` locks | garage, re-installed if absent |

The last Dockerfile step runs `verify.sh` (`paddock-ci-verify`): the build fails when a tool is missing or off its pin, and
`/etc/paddock-ci-release` lists every version. Jobs test `[ -f /etc/paddock-ci-release ]`; the `setup-rust` action then skips its
toolchain install.

## Building

Manual dispatch of `.gitea/workflows/ci-image.yml` (linux/amd64, pushed to `dev.decryptology.net/decryp7/paddock-ci` as
`v<VERSION>`, a dated run tag and `latest`). Never push from a workstation. Bump `ci-image/VERSION` with any change here and
point `ci.yml` at it; `scripts/ci-image.test.mjs` checks the two agree and that the pins agree with the repo.
Local smoke test: `docker build --platform linux/amd64 -t paddock-ci-smoke ci-image`, then remove it.

## Bumping a pin

Change the version ARG, download each asset, `shasum -a 256` it (machete publishes `.sha256` files to compare), put the sums in the
matching `*_SHA256_*` ARGs. A mismatch makes the build fail rather than trust an unverified file.

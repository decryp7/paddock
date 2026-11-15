@echo off
setlocal
rem ./dev for Windows, stage 0 (shell-to-rust WP-1): needs no Git Bash. It builds the launcher, tools/dev-launch, with cargo (no-op when
rem it is up to date) and runs it; see the `dev` script next to this file for the same steps in sh.
set "GIT_DIR="
set "GIT_WORK_TREE="
set "ROOT="
for /f "delims=" %%R in ('git rev-parse --show-toplevel 2^>nul') do set "ROOT=%%R"
if not defined ROOT (echo dev: run this inside the Paddock repository. 1>&2 & exit /b 2)
set "L=%ROOT%/tools/dev-launch"
if not exist "%L%/Cargo.toml" (echo dev: tools/dev-launch is not in this checkout ^(a sparse checkout without it^). Run: git sparse-checkout add tools 1>&2 & exit /b 2)
rem rustup 1.28 does not install a pinned toolchain by itself: install the pin as the old wrapper did (rustup reads rust-toolchain.toml in
rem tools/paddock-dev), else build the launcher with any installed one; the launcher holds the install lock for every later install.
set "TC="
set "SETRT="
cargo --version >nul 2>&1 && goto build
where rustup >nul 2>&1 || (echo ::error::./dev: no Rust toolchain runs here and rustup is not installed. Install Rust with rustup ^(https://rustup.rs^), then run dev again. 1>&2 & exit /b 2)
echo dev: cargo does not run here; installing the toolchain rust-toolchain.toml pins with rustup. 1>&2
pushd "%ROOT%/tools/paddock-dev" && (rustup toolchain install 1>&2 || echo dev: 'rustup toolchain install' failed; trying an installed toolchain. 1>&2) & popd
cargo --version >nul 2>&1 || for /f "tokens=1" %%T in ('rustup toolchain list 2^>nul') do if not defined TC set "TC=%%T"
if defined TC if not defined RUSTUP_TOOLCHAIN set "RUSTUP_TOOLCHAIN=%TC%" & set "SETRT=1"
:build
cargo build -q --release --locked --manifest-path "%L%/Cargo.toml" --target-dir "%L%/target" || (echo ::error::./dev could not build its launcher ^(tools/dev-launch, Rust 1.89 or newer^). Install Rust with rustup ^(https://rustup.rs^) and run dev again; to see why, run 'cargo build --release --locked' in tools/dev-launch. 1>&2 & exit /b 1)
if defined SETRT set "RUSTUP_TOOLCHAIN="
"%L%/target/release/dev-launch.exe" %*
exit /b %ERRORLEVEL%

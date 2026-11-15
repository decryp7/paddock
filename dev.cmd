@echo off
setlocal
rem ./dev for Windows, stage 0 (shell-to-rust WP-1): needs no Git Bash. The same steps as the `dev` script next to it: build the launcher,
rem tools/dev-launch, when its sources changed, publish it under a stamp of its sources (git's tree id, plus a hash of any uncommitted
rem change; a running launcher is never replaced, which Windows cannot do) and run it.
set "GIT_DIR="
set "GIT_WORK_TREE="
set "CARGO_BUILD_TARGET="
set "ROOT="
for /f "delims=" %%R in ('git rev-parse --show-toplevel 2^>nul') do set "ROOT=%%R"
if not defined ROOT (echo dev: run this inside the Paddock repository. 1>&2 & exit /b 2)
set "L=%ROOT%/tools/dev-launch"
if not exist "%L%/Cargo.toml" (echo dev: tools/dev-launch is not in this checkout ^(a sparse checkout without it^). Run: git sparse-checkout add tools 1>&2 & exit /b 2)
set "KEY=untracked"
for /f "delims=" %%K in ('git -C "%ROOT%" rev-parse HEAD:tools/dev-launch 2^>nul') do set "KEY=%%K"
set "DIRT="
set "STF=%TEMP%\dev-launch-stamp-%RANDOM%%RANDOM%.txt"
git -C "%ROOT%" status --porcelain --untracked-files=all -- tools/dev-launch > "%STF%" 2>nul
for /f "usebackq delims=" %%D in ("%STF%") do set "DIRT=1"
if defined DIRT git -C "%ROOT%" diff HEAD -- tools/dev-launch >> "%STF%" 2>nul
if defined DIRT for /f "delims=" %%H in ('git hash-object "%STF%"') do set "KEY=%KEY%-%%H"
del "%STF%" >nul 2>&1
set "BIN=%L:/=\%\target\bin\dev-launch-%KEY%.exe"
if exist "%BIN%" goto run

set "TC="
set "OLDRT=%RUSTUP_TOOLCHAIN%"
cargo --version >nul 2>&1 && goto build
where rustup >nul 2>&1 || (echo ::error::./dev: no Rust toolchain runs here and rustup is not installed. Install Rust with rustup ^(https://rustup.rs^), then run dev again. 1>&2 & exit /b 2)
rem The first installed toolchain whose rustc is 1.89 or newer; "no installed toolchains", an old one and one that does not run are skipped.
for /f "tokens=1" %%T in ('rustup toolchain list 2^>nul') do if not defined TC call :try %%T
if defined TC goto build
set "ANY="
for /f "tokens=1" %%T in ('rustup toolchain list 2^>nul') do if /i not "%%T"=="no" set "ANY=1"
if defined ANY (echo ::error::./dev: no installed Rust toolchain is 1.89 or newer, which the launcher needs. Install one with 'rustup toolchain install stable', then run dev again. 1>&2 & exit /b 2)
if "%PADDOCK_TOOLCHAIN_FAILED%"=="1" (echo ::error::./dev: no Rust toolchain is installed and an earlier step of this job failed to install one ^(PADDOCK_TOOLCHAIN_FAILED=1^). Install the toolchain rust-toolchain.toml pins by hand, then run dev again. 1>&2 & exit /b 2)
rem The one install stage 0 makes: a machine with rustup and no toolchain at all (rustup reads the pin in tools/paddock-dev). No lock here: a
rem person ran dev.cmd; CI starts the install through the `dev` script, which has one.
echo dev: no Rust toolchain is installed; installing the toolchain rust-toolchain.toml pins with rustup. 1>&2
pushd "%ROOT%\tools\paddock-dev" && (rustup toolchain install --no-self-update 1>&2 & popd)
for /f "tokens=1" %%T in ('rustup toolchain list 2^>nul') do if not defined TC call :try %%T
if not defined TC (echo ::error::./dev: 'rustup toolchain install' did not leave a working toolchain. Run it by hand in tools\paddock-dev to see why, then run dev again. 1>&2 & exit /b 1)

:build
if defined TC set "RUSTUP_TOOLCHAIN=%TC%"
cargo build -q --release --locked --manifest-path "%L%/Cargo.toml" --target-dir "%L%/target" || (echo ::error::./dev could not build its launcher ^(tools/dev-launch, Rust 1.89 or newer^). Install Rust with rustup ^(https://rustup.rs^) and run dev again; to see why, run 'cargo build --release --locked' in tools/dev-launch. 1>&2 & exit /b 1)
set "RUSTUP_TOOLCHAIN=%OLDRT%"
if not exist "%L:/=\%\target\bin" mkdir "%L:/=\%\target\bin"
set "NEW=%BIN%.new.%RANDOM%%RANDOM%"
set "TRY=0"
:copy
copy /y "%L:/=\%\target\release\dev-launch.exe" "%NEW%" >nul 2>&1 && goto copied
set /a TRY+=1
if %TRY% LSS 20 (ping -n 1 -w 200 192.0.2.1 >nul & goto copy)
echo ::error::./dev could not copy the launcher into tools\dev-launch\target\bin. Close programs that use it, then run dev again. 1>&2
exit /b 1
:copied
rem Published by a rename; if a dev with the same stamp published first (Windows will not replace a running program), theirs is the same bytes.
move /y "%NEW%" "%BIN%" >nul 2>&1 || (del "%NEW%" >nul 2>&1 & if not exist "%BIN%" (echo ::error::./dev could not publish the launcher as %BIN%. 1>&2 & exit /b 1))
rem Launchers of other stamps go (a running one stays until it ends).
for %%F in ("%L:/=\%\target\bin\dev-launch-*") do if /i not "%%~nxF"=="dev-launch-%KEY%.exe" del /q "%%F" >nul 2>&1

:run
"%BIN%" %*
exit /b %ERRORLEVEL%

:try
set "VER="
for /f "usebackq tokens=2" %%V in (`cmd /c "set RUSTUP_TOOLCHAIN=%~1&& rustc --version 2>nul"`) do set "VER=%%V"
if not defined VER exit /b 0
set "MAJ="
set "MIN=0"
for /f "tokens=1,2 delims=." %%A in ("%VER%") do (set "MAJ=%%A" & set /a MIN=%%B 2>nul)
if "%MAJ%"=="1" if %MIN% GEQ 89 set "TC=%~1"
exit /b 0

@echo off
setlocal
rem ./dev for Windows, stage 0 (shell-to-rust WP-1): needs no Git Bash. The same steps as the `dev` script next to it: build the launcher,
rem tools/dev-launch, when its sources changed, publish it under a stamp of its sources (git's tree id, plus a hash of any uncommitted
rem change and of untracked files; a running launcher is never replaced, which Windows cannot do) and run it. It installs nothing.
set "GIT_DIR="
set "GIT_WORK_TREE="
set "CARGO_BUILD_TARGET="
if /i "%~1"=="--job-setup" goto jobsetup
set "ROOT="
for /f "delims=" %%R in ('git rev-parse --show-toplevel 2^>nul') do set "ROOT=%%R"
if not defined ROOT (echo dev: run this inside the Paddock repository. 1>&2 & exit /b 2)
set "L=%ROOT%/tools/dev-launch"
if not exist "%L%/Cargo.toml" (echo dev: tools/dev-launch is not in this checkout ^(a sparse checkout without it^). Run: git sparse-checkout add tools 1>&2 & exit /b 2)
set "KEY=untracked"
for /f "delims=" %%K in ('git -C "%ROOT%" rev-parse HEAD:tools/dev-launch 2^>nul') do set "KEY=%%K"
set "DIRT="
set "STF=%TEMP%\dev-launch-stamp-%RANDOM%%RANDOM%.txt"
git --no-optional-locks -C "%ROOT%" status --porcelain --untracked-files=all -- tools/dev-launch > "%STF%" 2>nul
for /f "usebackq delims=" %%D in ("%STF%") do set "DIRT=1"
if defined DIRT git -C "%ROOT%" diff --no-ext-diff --no-textconv HEAD -- tools/dev-launch >> "%STF%" 2>nul
if defined DIRT git -C "%ROOT%" ls-files -o --exclude-standard -- tools/dev-launch > "%STF%.u" 2>nul
if defined DIRT git -C "%ROOT%" hash-object --stdin-paths < "%STF%.u" >> "%STF%" 2>nul
if defined DIRT for /f "delims=" %%H in ('git hash-object "%STF%"') do set "KEY=%KEY%-%%H"
del "%STF%" "%STF%.u" >nul 2>&1
set "BIN=%L:/=\%\target\bin\dev-launch-%KEY%.exe"
set "OLDAI=%RUSTUP_AUTO_INSTALL%"
if exist "%BIN%" goto run

rem rustup 1.28.1 and later install a missing pin from any proxy that runs below rust-toolchain.toml, unlocked and unbounded (run 8060):
rem every probe says it may not. Every install is the launcher's, under its lock and limit.
set "RUSTUP_AUTO_INSTALL=0"
set "TC="
set "OLDRT=%RUSTUP_TOOLCHAIN%"
cargo --version >nul 2>&1 && goto build
where rustup >nul 2>&1 || (call :failed & echo ::error::./dev: no Rust toolchain runs here and rustup is not installed. Install Rust with rustup ^(https://rustup.rs^), then run dev again. 1>&2 & exit /b 2)
rem The first installed toolchain whose rustc is 1.89 or newer; "no installed toolchains", an old one and one that does not run are skipped.
for /f "tokens=1" %%T in ('rustup toolchain list 2^>nul') do if not defined TC call :try %%T
if defined TC goto build
call :failed
set "ANY="
for /f "tokens=1" %%T in ('rustup toolchain list 2^>nul') do if /i not "%%T"=="no" set "ANY=1"
call :instcmd
if defined ANY (echo ::error::./dev: no installed Rust toolchain is 1.89 or newer, which the launcher needs. Install the toolchain rust-toolchain.toml pins: %INSTCMD%, then run dev again. 1>&2 & exit /b 2)
echo ::error::./dev: rustup has no toolchain installed, and ./dev installs none ^(it needs one to build its launcher^). Install the toolchain rust-toolchain.toml pins: %INSTCMD%, then run dev again. 1>&2
exit /b 2

:build
call :buildlauncher || exit /b 1
if not exist "%L:/=\%\target\bin" mkdir "%L:/=\%\target\bin"
set "NEW=%BIN%.new.%RANDOM%%RANDOM%"
set "TRY=0"
:copy
copy /y "%L:/=\%\target\release\dev-launch.exe" "%NEW%" >nul 2>&1 && goto copied
set /a TRY+=1
if %TRY% LSS 20 (powershell -NoProfile -Command Start-Sleep -Milliseconds 200 & goto copy)
echo ::error::./dev could not copy the launcher into tools\dev-launch\target\bin. Close programs that use it, then run dev again. 1>&2
exit /b 1
:copied
rem Published by a rename; if a dev with the same stamp published first (Windows will not replace a running program), theirs is the same bytes.
move /y "%NEW%" "%BIN%" >nul 2>&1 || (del "%NEW%" >nul 2>&1 & if not exist "%BIN%" (echo ::error::./dev could not publish the launcher as %BIN%. 1>&2 & exit /b 1))
rem When this launcher was published, for the prune of the next stamp (seconds since the epoch, from PowerShell).
for /f %%N in ('powershell -NoProfile -Command "[DateTimeOffset]::UtcNow.ToUnixTimeSeconds()"') do (set "NOW=%%N" & >"%BIN%.at" echo %%N)
rem Launchers of other stamps go once a minute old (another dev may be about to run one it published a moment ago); a running one stays
rem until it ends, and a .new file is never another run's to remove.
for %%F in ("%L:/=\%\target\bin\dev-launch-*.exe") do if /i not "%%~nxF"=="dev-launch-%KEY%.exe" call :prune "%%~fF"

:run
set "RUSTUP_AUTO_INSTALL=%OLDAI%"
"%BIN%" %*
exit /b %ERRORLEVEL%

:buildlauncher
setlocal
set "RUSTC_WRAPPER="
set "RUSTC_WORKSPACE_WRAPPER="
set "CARGO_BUILD_RUSTC_WRAPPER="
set "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER="
if defined TC set "RUSTUP_TOOLCHAIN=%TC%"
cargo build -q --release --locked --manifest-path "%L%/Cargo.toml" --target-dir "%L%/target" || (echo ::error::./dev could not build its launcher ^(tools/dev-launch, Rust 1.89 or newer^). Install Rust with rustup ^(https://rustup.rs^) and run dev again; to see why, run 'cargo build --release --locked' in tools/dev-launch. 1>&2 & exit /b 1)
endlocal & exit /b 0

:prune
set "AT="
if exist "%~1.at" set /p AT=<"%~1.at"
if not defined AT set "AT=0"
for /f %%N in ('powershell -NoProfile -Command "[DateTimeOffset]::UtcNow.ToUnixTimeSeconds()"') do set /a AGE=%%N-%AT% 2>nul
if %AGE% GEQ 60 del /q "%~1" "%~1.at" >nul 2>&1
exit /b 0

:failed
if /i "%GITHUB_ACTIONS%"=="true" if defined GITHUB_ENV >>"%GITHUB_ENV%" echo PADDOCK_TOOLCHAIN_FAILED=1
exit /b 0

:instcmd
set "CH=<the channel of rust-toolchain.toml>"
set "COMPS="
for /f "usebackq delims=" %%L in (`findstr /b /c:"channel" "%ROOT%\rust-toolchain.toml" 2^>nul`) do set LINE=%%L
if defined LINE call :chan
for /f "usebackq delims=" %%L in (`findstr /b /c:"components" "%ROOT%\rust-toolchain.toml" 2^>nul`) do set LINE=%%L
if defined LINE call :comps
set "INSTCMD=rustup toolchain install %CH% --profile minimal%COMPS%"
exit /b 0
:chan
set LINE=%LINE:"=%
for /f "tokens=2 delims== " %%V in ("%LINE%") do set "CH=%%V"
set "LINE="
exit /b 0
:comps
set LINE=%LINE:"=%
set LINE=%LINE:[=%
set LINE=%LINE:]=%
set LINE=%LINE:,= %
for /f "tokens=2* delims== " %%A in ("%LINE%") do for %%X in (%%A %%B) do set "COMPS=%COMPS% --component %%X"
set "LINE="
exit /b 0

:try
set "VER="
for /f "usebackq tokens=2" %%V in (`cmd /c "set RUSTUP_TOOLCHAIN=%~1&& rustc --version 2>nul"`) do if not defined VER set "VER=%%V"
if not defined VER exit /b 0
set "MAJ="
set "MIN=0"
for /f "tokens=1,2 delims=." %%A in ("%VER%") do (set "MAJ=%%A" & set /a MIN=%%B 2>nul)
if "%MAJ%"=="1" if %MIN% GEQ 89 set "TC=%~1"
exit /b 0

:jobsetup
rem `dev.cmd --job-setup <arguments>`: runs `dev.cmd <arguments>` and, when that fails for any reason (a launcher that cannot be built
rem included), marks the job with PADDOCK_JOB_ABORTED=1 and says so once, as the `dev` script does.
set "ARGS=%*"
set "ARGS=%ARGS:*--job-setup=%"
call "%~f0" %ARGS%
set "RC=%ERRORLEVEL%"
if "%RC%"=="0" exit /b 0
echo ::error::A job setup step failed ^(exit %RC%: ./dev %ARGS%^). The job's later steps are skipped, because they would run without the slot, temp folders and core share that step sets up. Read the error above it, fix that, and rerun the job. 1>&2
if defined GITHUB_ENV >>"%GITHUB_ENV%" echo PADDOCK_JOB_ABORTED=1
exit /b %RC%

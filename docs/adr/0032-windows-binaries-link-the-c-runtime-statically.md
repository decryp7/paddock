# 32. Windows binaries link the C runtime statically

- Status: Proposed (2026-10-30). Release blocker for Dyno 1.0.0; applies to every app with a Windows installer
- Date: 2026-10-30
- Related: ER-21 (recurring bugs mean a missing design rule: one mechanism plus a check over every instance), ER-14,
  rule 23 (verify what was built, not what was meant); ADR 0014 §2 (the root `.cargo/config.toml` is read by every
  workspace)
- Evidence: the Dyno 1.0 VM run (2026-10-29/30, clean Windows 11 VM, `dyno-agent.exe` exit 0xC0000135 until Microsoft's
  `vc_redist.x64.exe` was installed); the imports measured below with `llvm-readobj --coff-imports` (LLVM from Homebrew)
  and with the new check, on cargo-xwin 0.23.1 cross-builds from the Mac mini

## Context

On a clean Windows 11 PC, Dyno's `dyno-agent.exe` did not start: the loader stopped it with 0xC0000135
(`STATUS_DLL_NOT_FOUND`) because it imports `VCRUNTIME140.dll`. That DLL comes with Microsoft's Visual C++
redistributable, not with Windows. Our CI runners have the redistributable (Visual Studio's build tools install it), so
no CI job, smoke run or installer test could show the failure. Only a clean machine could.

Rust on `*-pc-windows-msvc` links the C runtime dynamically by default; the Rust reference lists no MSVC target as
static by default and selects the runtime with the `crt-static` target feature
([Linkage, "Static and dynamic C runtimes"](https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes),
read 2026-10-30). The C runtime has three parts: the startup code, the VC runtime (`vcruntime140.dll`) and the Universal
CRT (`ucrtbase.dll`, reached through `api-ms-win-crt-*.dll`). The UCRT "is a Microsoft Windows operating system component.
It's included as part of the operating system in Windows 10 or later"
([Universal CRT deployment](https://learn.microsoft.com/en-us/cpp/windows/universal-crt-deployment), ms.date 2022-02-07,
read 2026-10-30). The VC runtime is not.

**Why the main app binaries were fine.** `tauri-build` 2.7.4 (Dyno, Pitwall, Pitcrew, Spotter and the demos; Debrief
pins 2.7.3) links the VC runtime statically into the app crate it builds, keyed on the target, so cross-builds get it too:
`try_build` matches `CARGO_CFG_TARGET_ENV == "msvc"` and calls `static_vcruntime::metabuild()` when
`build > windows > staticVCRuntime` is true, which is the default (tauri-build 2.7.4 `src/lib.rs`; tauri-utils 2.10.2
`WindowsBuildConfig::default`, `static_vc_runtime: true`; read in the crates.io sources on 2026-10-30; the
[config reference](https://v2.tauri.app/reference/config/) shows the same default). `static_vcruntime` 3.0.0 emits
`/NODEFAULTLIB` and `/DEFAULTLIB` link arguments: static VC runtime, dynamic UCRT. Link arguments from a build script
reach only that package's binaries, so **every sidecar, agent, service and CLI** (each its own crate, built by
`cargo build -p ...` in the app's `beforeBuildCommand`) linked the VC runtime dynamically.

### Every Windows binary the family ships (before)

Inventory from each app's `tauri.conf.json` (`bundle.externalBin`, `bundle.resources`) and its sidecar scripts, at
`dfc9cdc94`. "Before" is measured where a Windows build existed and otherwise follows from the mechanism above.

| App | Binary | Kind | VC runtime before | How known |
| --- | --- | --- | --- | --- |
| Dyno | `dyno-app.exe` | Tauri app | static (tauri-build) | measured: no `VCRUNTIME140.dll` import |
| Dyno | `dyno-agent.exe` | sidecar, agent | **dynamic** | measured: imports `VCRUNTIME140.dll`; failed on the VM |
| Dyno | `dyno.exe` | sidecar, CLI | **dynamic** | measured: imports `VCRUNTIME140.dll` |
| Pitwall | app | Tauri app | static (tauri-build) | mechanism |
| Pitwall | `pitwall-server.exe` | sidecar, service | **dynamic** | mechanism (own crate, `cargo build -p pitwall-server`) |
| Pitcrew | app | Tauri app | static (tauri-build) | mechanism |
| Pitcrew | `pitcrew-service.exe` | sidecar, service | **dynamic** | mechanism |
| Spotter | app (its service runs from the same exe) | Tauri app | static (tauri-build) | mechanism |
| Debrief | app | Tauri app | static (tauri-build 2.7.3, same code) | mechanism |
| Debrief | `debrief-ai-worker.exe`, `debrief-plugins.exe`, `debrief-store-worker.exe` | sidecars | **dynamic** | mechanism |
| Debrief | `lib/duckdb.dll` | prebuilt DuckDB library | none: imports only `KERNEL32`, `RstrtMgr`, `WS2_32` | measured on the fetched library |
| shell-demo | app, `shell-demo-agent.exe` | Tauri app, sidecar | static, **dynamic** | mechanism |
| workbench-demo | app | Tauri app | static | mechanism |

So every app with a sidecar was affected: Dyno, Pitwall, Pitcrew, Debrief and shell-demo. The NSIS installers
(`tauri.windows.conf.json`) do not install the redistributable and have no hook that does. The WebView2 runtime is a
separate matter, and Tauri's bootstrapper handles it.

## Decision

1. **One mechanism, set once: `+crt-static` for every Windows MSVC target, in the root `.cargo/config.toml`.**

   ```toml
   [target.'cfg(all(windows, target_env = "msvc"))']
   rustflags = ["-C", "target-feature=+crt-static"]
   ```

   Every workspace in the repo reads this file (ADR 0014 §2), so every binary of every app, sidecars included, links the
   whole C runtime statically. The `cfg` form also covers `aarch64-pc-windows-msvc`. It is joined with the existing
   `[target.x86_64-pc-windows-msvc]` table, since Cargo joins every matching `<triple>` and `<cfg>` entry
   ([Cargo configuration, `target.<cfg>.rustflags`](https://doc.rust-lang.org/cargo/reference/config.html#targetcfgrustflags),
   read 2026-10-30). The same page notes the limit: `CARGO_ENCODED_RUSTFLAGS` and `RUSTFLAGS` replace all configured
   flags. No workflow sets either (checked with `grep` over `.gitea/` and `conformance/ci/`), and
   `docs/dev-setup.md` tells developers to add the flag when they do. cargo-xwin 0.23.1 reads the configured flags,
   recognises `+crt-static`, adds its own link arguments and a static-runtime CMake toolchain, and passes them as
   `CARGO_TARGET_<TRIPLE>_RUSTFLAGS` (its `src/compiler/common.rs` and `clang_cl.rs`, read 2026-10-30), so the Mac
   cross-builds follow the same setting.

   **Tauri's partial mechanism is switched off: `"build": { "windows": { "staticVCRuntime": false } }` in every app's
   `tauri.conf.json`.** The two do not combine. `static_vcruntime` adds `/NODEFAULTLIB:libucrt.lib` and
   `/DEFAULTLIB:ucrt.lib` to the app binary, and cargo-xwin's `+crt-static` mode adds `-nodefaultlib:ucrt` and
   `-defaultlib:libucrt`. Together they exclude both UCRT libraries, and the link fails with undefined `strlen`, `floor`,
   `_initialize_onexit_table` and more (measured 2026-10-30 on `dyno-app`). With one mechanism every binary is built the
   same way, wherever it is built. A test checks that every app keeps the setting.

   **A new app gets both halves.** `scripts/new-app.mjs` copies a demo's `tauri.conf.json` (with `staticVCRuntime`
   false) into a repository of its own, outside this one, where the root configuration does not reach. It therefore also
   writes the new repository's `.cargo/config.toml` with the same table, taken from this repository's root configuration
   (one source), and fails when that table is missing. Without it, the new app's sidecars and its app binary would both
   import `VCRUNTIME140.dll`. `scripts/new-app.test.mjs` checks both halves.

2. **One check over every instance: `conformance/release/windows-crt-imports.mjs`.** It reads the PE import and
   delay-import tables (pure Node, no `dumpbin` or `objdump`, so it runs on the Windows runner as it is) and fails on
   any DLL of the redistributable: a family name (`vcruntime`, `msvcp`, `msvcr`, `concrt`, `vccorlib`, `vcomp`, `vcamp`,
   `mfc`, `mfcm`), its version and any suffix. That includes `vcruntime140_1`, `vcruntime140_threads`,
   `msvcp140_atomic_wait`, `mfc140u` and the debug builds (`msvcp140d`, `msvcp140d_atomic_wait`, `ucrtbased`). The UCRT
   (`ucrtbase.dll`, `api-ms-win-crt-*`) and the system's `msvcrt.dll` are allowed. A delay-load table in the old
   VA-based format (before Visual C++ 7) is reported as a finding that names the format and says to inspect the file
   with `dumpbin /imports`; it is never read as if it held RVAs.

   **It checks what the installer ships, named by the configuration, not a folder listing.** With `--src-tauri` and
   `--bin-dir` it merges `tauri.conf.json`, `tauri.windows.conf.json` and any `--config` file the build used (as a JSON
   merge patch, like Tauri). It then checks:
   - the main binary (`mainBinaryName`, else Cargo's `default-run`, else the package name);
   - every `bundle.externalBin` sidecar, from the build folder;
   - every `.exe` and `.dll` among `bundle.resources`.

   A missing main binary or sidecar is an error. Other files in a build folder that a host runner keeps between runs are
   never read, so a stale binary can neither fail a run nor stand in for a missing one. Reading a folder never throws:
   an unreadable path is a finding, and symbolic links to folders are not followed. The callers pass the build folder
   in Windows form (`cygpath -m`), since Node on Windows does not read Git Bash's `/c/...` paths. It runs right after
   `tauri build`, before the installers are collected, in every path that builds a Windows installer:
   - `conformance/ci/tauri-bundle.sh` (the shared `tauri-bundle` action, used by `app-release.yml` for Dyno, Pitwall,
     Pitcrew and Spotter), for an `nsis` or `msi` bundle;
   - `.gitea/workflows/debrief.yml`, one step after Debrief's own build (it does not use the action), with
     `--config` for its `tauri.sidecars.conf.json`;
   - `scripts/demo/bundle.sh` (shell-demo and workbench-demo).

   No new CI job: the check reads a few files in the job that built them, in well under a second. The tests
   (`conformance/test/windows-crt-imports.test.mjs` and the NSIS case in `ci-scripts.test.mjs`) use synthetic PE files
   built byte by byte. The same test file also checks three things:
   - the root configuration keeps the flag;
   - every app's `tauri.conf.json` keeps `staticVCRuntime` false;
   - the three build paths call the check with the shipped list.

   **What an import check cannot see.** A DLL that a program loads at run time with `LoadLibrary` is in no import table.
   Neither its own needs nor the fact that the program loads it show up. Such a DLL must be shipped as a listed
   resource, where the check reads it like any other file, or be a Windows component.

## Alternatives considered

| Option | For | Against | Verdict |
| --- | --- | --- | --- |
| **`+crt-static` in the root cargo config, Tauri's `staticVCRuntime` off** (chosen) | one line covers every binary of every app, present and future; also correct for C code built by `cc`/`cmake` (they read `CARGO_CFG_TARGET_FEATURE` and switch to `/MT`); no installer change; no extra download or admin rights (Dyno and Spotter install per user) | runtime fixes now come only with our releases, not through Windows Update; binaries grow by about 100 kB each (measured below); `RUSTFLAGS` overrides it silently (the check catches that) | **chosen** |
| `static_vcruntime` crate in every sidecar's `build.rs` (what tauri-build does for the app) | keeps the UCRT dynamic, so it is serviced by Windows Update | one `build.rs` line per binary crate: the next new sidecar forgets it, exactly this bug's shape (ER-21); its own docs recommend adding `+crt-static` as well "for the best compatibility" | rejected as the mechanism; the check would catch a forgotten crate, but only at release time |
| Installer runs `vc_redist.x64.exe` (central deployment, Microsoft's recommendation) | Windows Update services the runtime | about 25 MB more per installer or a download at install time (Local first: no network by default); needs admin rights, which Dyno and Spotter's per-user installers do not have; another failure mode during install | rejected |
| Tauri's `bundle.windows.bundleVCRuntime` (copies the DLLs beside the app: local deployment) | no admin rights; a single setting | Microsoft discourages local deployment ("Microsoft can't automatically update locally deployed" libraries); per app, so each app could forget it; the copies are not updated either | rejected |
| Document "install the redistributable" | no build change | users meet 0xC0000135 first; violates "installs and starts on a clean PC" | rejected |

Microsoft recommends central deployment and dynamic linking for servicing: "we don't recommend this approach because
statically linked libraries can't be updated in place"
([Deployment in Microsoft C++](https://learn.microsoft.com/en-us/cpp/windows/deployment-in-visual-cpp), updated
2025-11-05, read 2026-10-30). For us that trade-off is acceptable. Our apps update themselves through signed feeds, and
each release builds with the current toolchain, so a runtime fix reaches users the next time we ship. The runtime
surface our Rust binaries use is small: panic handling, `memcpy`/`memset`, startup code. Static linking is also what
the Rust tools many people install use: ripgrep's `.cargo/config.toml` sets the same flag (secondary, not re-read for
this ADR).

## Consequences

- No Paddock app needs the VC++ redistributable. A clean Windows 10/11 PC is enough. The Dyno tag blocker is resolved by
  rebuilding, with no installer change: the NSIS installers and their hooks stay as they are.
- Binaries grow a little; see the measurements. The app binaries now also link the UCRT statically, like the sidecars. Performance and memory do not change measurably for this kind of code.
- **CRT state is per module now.** Every statically linked module (each exe, and a DLL such as `duckdb.dll`, which
  carries its own runtime) has its own heap, `FILE` table, file descriptors, `errno` and locale. Memory, `FILE*` and
  descriptors must never cross a DLL boundary: what `duckdb.dll` allocates is freed by DuckDB's own functions
  (`duckdb_free`, `duckdb_destroy_*`), never by `free()` on our side, and no `FILE*` or descriptor is handed over. The
  duckdb crate already frees DuckDB's error strings with `duckdb_free` (duckdb 1.10506.1 `src/lib.rs`, read 2026-10-30).
  Rust's own allocations do not use the CRT heap; Rust's `System` allocator on Windows calls `HeapAlloc` (secondary, not
  re-read). A future C API that returns memory or handles must follow the same rule.
- A third-party DLL we bundle must also be free of the redistributable; the check covers it when it is listed in
  `bundle.resources` (`duckdb.dll` today: clean). Runtimes downloaded at run time (Chromium for Spotter and Debrief's browser) are not in
  the installer and are not checked: they are the vendor's builds. Chrome for Testing is believed to carry its own
  runtime (unverified).
- **Sighter (ONNX Runtime, DirectML).** The detector plan links ONNX Runtime statically through `ort` (pyke's prebuilt
  archive, `download-binaries`) into an inference sidecar ([Sighter detector research](../research/2026-10-sighter-detector.md)).
  That sidecar is built in this repository, so it gets `+crt-static` like every other binary. Whether pyke's Windows
  archive was compiled for the static runtime (`/MT`) is unverified. If it was not, the link fails with a runtime
  mismatch: loud, at build time, never on a user's PC. Then the order of preference is:
  1. an ONNX Runtime built with the static runtime (its build option `--enable_msvc_static_runtime`; unverified,
     check in the Sighter spike);
  2. ONNX Runtime as a DLL that carries its own runtime, shipped as a listed resource the check reads;
  3. only as a last resort, an amendment to this ADR for app-local VC runtime DLLs.

  `DirectML.dll`, if Sighter must ship it rather than use the copy in Windows, is a listed resource and is checked
  like `duckdb.dll`. Any DLL that ONNX Runtime opens with `LoadLibrary` (an execution provider) is invisible to the
  import check (see Decision 2), so Sighter lists every one it ships. The Sighter spike records the measured imports.
- Third-party plugins built with Debrief's plugin SDK outside this repo do not read this configuration. The SDK
  template should carry the same flag (follow-up).
- A Windows build with `RUSTFLAGS` set loses the flag. The release check then fails, which is loud and early.

## Verification

Cross-built on the Mac with cargo-xwin 0.23.1 (`nice -n 10`, `CARGO_BUILD_JOBS=2`, `--profile dist`,
`x86_64-pc-windows-msvc`) and inspected with `llvm-readobj --coff-imports` and the new check:

| Binary | Before (`dfc9cdc94`, same profile) | After (this change) | Size before → after |
| --- | --- | --- | --- |
| `dyno-agent.exe` | `VCRUNTIME140.dll` + 8 `api-ms-win-crt-*` | none of either | 8,045,568 → 8,155,136 B (+107 KiB, +1.4 %) |
| `dyno.exe` | `VCRUNTIME140.dll` + 6 `api-ms-win-crt-*` | none of either | 1,115,648 → 1,211,904 B (+94 KiB, +8.6 %) |
| `dyno-app.exe` | no `VCRUNTIME140.dll`, 9 `api-ms-win-crt-*` (tauri-build) | none of either; manifest still embedded | not comparable (before: `release` profile and the real UI; after: `dist` and a stub UI) |
| `pitwall-server.exe` | dynamic (mechanism) | none of either | 11,007,488 B |
| `pitcrew-service.exe` | dynamic (mechanism) | none of either | 8,623,616 B |
| `debrief-ai-worker.exe`, `debrief-plugins.exe`, `debrief-store-worker.exe` | dynamic (mechanism) | none of either; the store worker imports `duckdb.dll`, which is clean | 1,725,952 / 11,604,480 / 28,332,032 B |
| `shell-demo-agent.exe` (`release`) | dynamic (mechanism) | none of either | 841,216 B |
| `spotter-app.exe` (aws-lc-sys C code) | static VC runtime (tauri-build) | none of either; aws-lc-sys's C code links without a runtime mismatch | 26,405,888 B |

With Tauri's `staticVCRuntime` still at its default, the first `dyno-app` link failed (undefined UCRT symbols, see
Decision 1); with it off, the app links. Building `dyno-app` with cargo-xwin also needs an `mt.exe` on `PATH` for the
manifest (the `llvm-mt` shim in Dyno's testing manual); CI builds natively and is not affected.

Not verified here: a native MSVC build on the Windows runner (the next CI run on this branch shows it, and the new
check runs there on the real installer binaries), and a start on a clean Windows PC. The local test VM was not at
`clean-baseline` (it holds the user's prepared Dyno 1.0 update test with the redistributable installed) and was left
untouched.

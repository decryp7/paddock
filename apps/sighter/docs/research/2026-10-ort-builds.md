# Sighter's ONNX Runtime builds: what to build, how, and where (2026-10-30)

Question (stage-2 plan SW5, ADR 0001 decision 4 and plan item P5): which ONNX Runtime do we pin, how does `ort` link it,
can it be built with the static MSVC runtime that ADR 0032 requires together with DirectML, where does `DirectML.dll`
come from and under which terms, which flags does CoreML need, and how long do the builds take on our runners? It feeds
the scripts in [scripts/ort](../../scripts/ort/README.md) and the draft workflow `.gitea/workflows/sighter-ort.yml`.

How to read it: **verified** means read today in the source at the pinned commit, in the package, or measured on this
Mac (synthetic inputs only); **read** means taken from an official page without a way to check it; **(secondary)** or
**projection** means what it says. No full ONNX Runtime build was run for this note (the Mac is shared; the brief
forbids it): build times are projections from pyke's published CI runs.

## Answer in short

- **Pin ONNX Runtime 1.31.1** (tag `v1.31.1`, commit `6af1a932e2286d5c58166085d24c68783033edee`, released 2026-10-26).
  It is a patch over 1.31.0 (an OpenVINO + Python build fix and a WASM test fix); `ort` 2.0.0-rc.15 targets 1.31 (API 31).
- **Build all four variants ourselves**; mirror no prebuilt ONNX Runtime. Every official or pyke build fails one of our
  constraints: Microsoft's release archives carry telemetry (the Linux x64 1.31.1 library contains the 1DS SDK, mbedTLS
  and a Microsoft collector host, measured), and pyke's Windows archives are built against the DLL C runtime (`/MD`),
  which cannot link into a `+crt-static` sidecar (ADR 0032).
- **Take `DirectML.dll` from nuget.org, pinned** (NuGet's SHA-512 plus our SHA-256 of the DLL), not mirrored. It needs
  no VC++ redistributable (measured). **But it registers a TraceLogging provider in the Microsoft telemetry group**
  (byte search of the DLL, below), so the DirectML gate's check (2), "no provider, or shown inert", will not pass on
  "no provider". The user decided: ship it only if the ETW session shows that provider writes no events ("Decided").
- **Static CRT with DirectML is accepted by ONNX Runtime's build** (`--enable_msvc_static_runtime` with `--use_dml`
  parses; it sets `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` and the static-runtime switches of ONNX, protobuf and
  Abseil). Compiling it is not verified until the first Windows run.
- **Delay-loading DirectML is the consumer's job** in a static build: ONNX Runtime applies its `/DELAYLOAD` list only to
  `onnxruntime.dll`. `sighter-infer` passes it from the archive's `link.txt`; the CPU-only variant imports no DirectML.
- **Projected cold build times** (every run is cold: jobs use their own temporary folder): Linux x64 CPU 15–60 min
  depending on the runner; macOS arm64 CoreML 15–35 min; on an 8-core, 16 GB Windows runner (6 compile jobs) DirectML
  40–60 min and CPU 35–55 min, one after the other. Compile jobs are detected at run time. All unmeasured.

## 1. The ONNX Runtime version

| Fact | Status |
| --- | --- |
| Releases: v1.31.1 (2026-10-26), v1.31.0 (2026-10-09), v1.30.1 (2026-10-29, a 1.30 patch), v1.30.0 (2026-09-10) | verified, GitHub releases API |
| v1.31.1 = commit `6af1a932e…`; v1.31.0 = `402979ac1…` | verified, GitHub refs API |
| 1.31.1 changes: an OpenVINO + Python provider-info namespace fix (#33327) and a WASM test-only change (#33226) | verified, release notes |
| 1.31.0: "Supported native builds now default to 1DS telemetry … can be disabled with --no_telemetry" | read, release notes (research note 2026-10-29 §12) |
| Third-party sources come through CMake FetchContent from `cmake/deps.txt`, each URL pinned by SHA-1; the only submodule a build needs is `cmake/external/onnx` | verified, `cmake/deps.txt` and `.gitmodules` at the tag |
| `onnxruntime/cmake/CMakeLists.txt` needs CMake ≥ 3.28 | verified |

**Choice: 1.31.1.** A patch release on the version ADR 0001 named (1.31.x), no API change. Renovate watches the tag
(SW5's Renovate line goes through the integrator); a bump changes `pins.env` and rebuilds every variant.

## 2. How `ort` links ONNX Runtime

Read in `ort-sys` 2.0.0-rc.15 (`build/main.rs`, `build/static_link/mod.rs`, `build/vars.rs`), verified 2026-10-30:

- With `ORT_LIB_PATH` (or `ORT_LIB_LOCATION`) set, `ort-sys` links **statically** unless `ORT_PREFER_DYNAMIC_LINK` is set.
  If the folder holds a single `onnxruntime.lib` / `libonnxruntime.a`, it links just that (`static=onnxruntime`).
  Otherwise it walks a CMake build tree with a hard-coded list of about 60 internal and Abseil libraries, which changes
  between ONNX Runtime versions. **We ship one merged archive**, as pyke does, so the link does not depend on that list.
- For a user-provided build `ort-sys` adds only the C++ runtime (`c++` on Apple, `stdc++` on Linux), `Foundation` and
  `CoreML` on Apple, and `shell32` on Windows. The DirectX libraries (`dxguid`, `DXCORE`, `DXGI`, `D3D12`, `DirectML`)
  are added only for pyke's archives or when the CMake tree contains `onnxruntime_providers_dml`. **So our archive
  carries `link.txt`**, and the consumer's `build.rs` adds those lines (`scripts/ort/smoke/build.rs` is the reference).
- Without `ORT_LIB_PATH` and without `download-binaries`, `ort-sys` defers a clear error to the link step, so
  `cargo check` and `cargo clippy` still work (verified here: the smoke program checks and lints, and `cargo test` fails
  at the link with `ort-sys`'s message).
- `download-binaries` fetches pyke's archives from `cdn.pyke.io` by `build/download/dist.tsv` (hash-pinned). For 1.31.0 it
  lists, among others, `x86_64-pc-windows-msvc` with `directml` (no CPU-only Windows archive), `aarch64-apple-darwin` with
  `coreml`, `x86_64-unknown-linux-gnu` with `none` (verified).
- `ort` features used: `default-features = false`, `std`, `api-31`, plus `coreml` (macOS) or `directml` (Windows), which
  only expose the EP types (`ort::ep::CoreML`, `ort::ep::DirectML`). `EnvironmentBuilder::with_telemetry(false)` and
  `ExecutionProviderDispatch::error_on_failure()` exist (verified in the crate source).

**pyke's build recipe** (`pykeio/ort-artifacts`, Apache-2.0, `src/build.ts`, read 2026-10-30): static Windows builds set
`CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL` and the ONNX/protobuf DLL-runtime switches. **So pyke's Windows archives
are `/MD`** (verified in the script, not in the archive), which answers ADR 0032's open question: they would not link
into a `+crt-static` binary without a runtime mismatch. Its macOS CoreML archive passes our telemetry check (below).

**Smoke test on this Mac (verified, 2026-10-30):** the `smoke` program, built against pyke's 1.31.0 macOS CoreML archive
(scratch copy with `download-binaries`, not committed), ran `cpu` and `coreml` with `error_on_failure`: `ok cpu 88` and
`ok coreml 64` (ms, at a 1-minute load of about 20). That shows the in-memory model encoding, the API use and the CoreML
registration work; it says nothing about our own build yet.

## 3. Static MSVC runtime with DirectML

ADR 0032 makes every Windows binary `+crt-static` (`/MT`). ONNX Runtime linked statically into `sighter-infer` must be
compiled `/MT` too, or the link fails (`LNK2038` runtime mismatch: loud, at build time).

- `tools/ci_build/build_args.py` has `--enable_msvc_static_runtime`; `build.py` then sets
  `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded$<$<CONFIG:Debug>:Debug>`, `ONNX_USE_MSVC_STATIC_RUNTIME=ON`,
  `protobuf_MSVC_STATIC_RUNTIME=ON`, `ABSL_MSVC_STATIC_RUNTIME=ON` (verified at the tag).
- Nothing in `build.py` rejects `--use_dml` with `--enable_msvc_static_runtime` (the only DirectML rejections are with
  `--minimal_build` and with the GDK without a DirectML path). Our exact flag sets parse on each OS with ONNX Runtime's
  own argument parser (verified with its `build_args.py` and a stub for OS detection; telemetry ends up off, static
  runtime on, DirectML on). **Whether it compiles is verified only by the first Windows run.**
- `DirectML.lib` is an import library (1,956 bytes) and does not care about the runtime. `DirectML.dll` 1.15.4 imports
  only Windows API sets and `d3d12.dll`: ADR 0032's checker reports "none needs the VC++ redistributable" for both the
  x64 and arm64 DLLs (verified).
- So ADR 0032's first preference holds: ONNX Runtime built with the static runtime. No DLL fallback or ADR amendment is
  needed, pending the first Windows build.

**Delay-load.** `cmake/onnxruntime_providers_dml.cmake` adds `/DELAYLOAD:DirectML.dll`, `d3d12.dll`, `dxgi.dll`,
`dxcore.dll` and others to `onnxruntime_DELAYLOAD_FLAGS`, and `cmake/onnxruntime.cmake` applies them only when
`onnxruntime_BUILD_SHARED_LIB` is on: "delay-loading is meaningless for a static lib anyway. Consumers that need
delay-load in a static build … apply onnxruntime_DELAYLOAD_FLAGS to their own target" (verified). ADR 0001 asked whether
our build's DirectML import is delay-loaded: **in a static build that is decided by `sighter-infer`'s own link**.
`link.txt` of the DirectML variant lists the four DLLs and `delayimp`; `check-windows-imports.sh` fails the build job
unless the smoke binary delay-loads `DirectML.dll` and imports none of them directly. One consequence for SW17: with a
delay-loaded import a missing `DirectML.dll` raises a delay-load exception at the first DirectML call, not at start, so
`sighter-infer` must check that `DirectML.dll` loads (`LoadLibraryExW`) before it registers the DirectML EP.

## 4. Telemetry in a `--no_telemetry` build

- Defaults (verified with ONNX Runtime's parser): without `--no_telemetry`, telemetry is on for native Windows and Linux
  x64 builds; with it, `onnxruntime_USE_TELEMETRY=OFF`.
- **Measured on Microsoft's own release archive** `onnxruntime-linux-x64-1.31.1.tgz` (SHA-256 `c849e36a…16bdc9`, matching
  the release API digest): `libonnxruntime.so.1.31.1` contains `Microsoft::Applications::Events` (the 1DS SDK),
  `mbedtls_ssl_handshake` and `events.data.microsoft.com`. Mirroring official builds is therefore out, even for CI tests
  (the egress test would see what a user build does not do).
- **Windows**: `cmake/onnxruntime_common.cmake` compiles `core/platform/windows/telemetry.cc` and
  `core/platform/windows/logging/*.cc` on every Windows build, whatever the flag (verified). With `--no_telemetry`,
  `windows/env.h` makes the session's telemetry provider the no-op base class (no `USE_WINDOWS_TELEMETRY`, no
  `USE_1DS_TELEMETRY`). The `Microsoft.ML.ONNXRuntime` TraceLogging provider is defined in that file but registered only
  by `WindowsTelemetry`'s constructor; in a public source build its "Microsoft telemetry" option is an **all-zero group**
  (`include/onnxruntime/core/platform/windows/TraceLoggingConfig.h`), so even if registered it is not in Microsoft's
  telemetry group. The ETW logging sink (`ONNXRuntimeTraceLoggingProvider`, no telemetry option) registers when the
  environment is created and only emits when a local trace session enables it.
- `check-telemetry.sh` therefore **fails** on 1DS (plain and mangled), curl, mbedTLS, collector host names and the
  Microsoft telemetry group GUID, and **reports** the two ONNX Runtime provider names. Checked against known samples:
  Microsoft's Linux library fails (4 findings), pyke's macOS CoreML archive and the smoke binary linked with it pass,
  `DirectML.dll` fails on the telemetry group unless `--allow-telemetry-group` is given.

## 5. DirectML: source, licence, telemetry

| Fact | Status |
| --- | --- |
| NuGet `Microsoft.AI.DirectML` 1.15.4, published 2024-10-28, the latest version; ONNX Runtime 1.31.1's `cmake/external/dml.cmake` restores exactly 1.15.4 | verified (NuGet API, CMake file) |
| Package 202,292,617 bytes; NuGet SHA-512 `/edn9WkEq…E7yhQ==` matches the download | verified |
| `bin/x64-win/DirectML.dll` 18,527,776 bytes, SHA-256 `9c9e6d82…4e92da1`; `bin/arm64-win/DirectML.dll` 18,444,832 bytes, SHA-256 `77b0db83…a4ab9d4` | measured |
| Licences in the package: `LICENSE.txt` for `bin/` (Microsoft Software License Terms, DirectML), `LICENSE-CODE.txt` (MIT) for `include/` and `build/`, `ThirdPartyNotices.txt` (MIT and BSD notices) | verified (package `README.md`) |
| Terms: install and use "solely for use on Windows and Xbox"; "copy and distribute the software … in applications and services you develop"; not "as a stand-alone offering"; not "remove, minimize, block, or modify any notices"; no reverse engineering, decompiling or disassembling (3b); "may collect information about you and your use of the software, and send that to Microsoft"; section 6, "may periodically check for updates, and download and install them" | verified (`LICENSE.txt` in the package) |
| `DirectML.dll` (x64) imports `EventRegister`, `EventSetInformation`, `EventWriteTransfer`, `EventUnregister` (`api-ms-win-eventing-provider-l1-1-0.dll`), the TraceLogging pattern | measured (import table, `llvm-readobj --coff-imports`) |
| Its data holds the provider names `Microsoft.Windows.AI.MachineLearning.Dml` and `…Dml.Diagnostics`; right after the first comes a provider trait of type "group" with GUID `{4f50731a-89cf-4782-b3e0-dce8c90476ba}`, in both x64 and arm64 DLLs | measured (byte search; no disassembly) |
| That GUID is the Microsoft telemetry provider group used by `TraceLoggingOptionMicrosoftTelemetry` | (secondary: .NET WPF's `TelemetryEventSource.cs` labels it "Microsoft OSG Telemetry Group GUID"; no Microsoft page found) |
| No outbound connection is evident from the DLL itself: it imports no WinHTTP, WinINet or socket API (only the API sets listed) | measured (import table); the egress test (P6) still has to show it at run time |

**Why we fetch rather than mirror.** Hosting the package or the DLL on our Gitea is distribution outside an application;
whether a private, authenticated package registry counts as "stand-alone" is a licence reading the user should make. The
build therefore fetches from nuget.org, pinned by hash (`fetch-directml.sh`); the paddock-runtime catalogue has no
DirectML entry (it would need a zip or tar archive on our server). Using `--dml_path` with that copy also stops ONNX
Runtime's CMake from fetching `nuget.exe` 5.3.0 unpinned and restoring the package itself.

**What this means for the DirectML gate (P6).** Check (2) asks that `DirectML.dll` "registers no TraceLogging/ETW
provider, or the provider is shown to be inert". The first is already answered: it has one, in the telemetry group. A
telemetry-group provider's events are collected by Windows' own diagnostic pipeline according to the PC's diagnostic data
setting, not by our process, so "inert" cannot be shown in general; at best an ETW session on the two S2 PCs shows which
events are written while the EP runs. The system copy (`System32\DirectML.dll`) is a Windows component and very likely
carries the same provider (not checked). The user's decision is under "Decided (user, 2026-10-30)".

## 6. CoreML build flags

- `--use_coreml` (Apple only), `--osx_arch arm64` (the default on an arm64 host), `--apple_deploy_target <ver>`, which
  `build.py` passes as `CMAKE_OSX_DEPLOYMENT_TARGET` (verified in `build_args.py`/`build.py`).
- `--apple_deploy_target 13.4` (user, 2026-10-30): the floor the family already met with pyke's archive (Pacenotes ADR 0001); CoreML's
  `MLProgram` format needs macOS 12 or later (secondary: ONNX Runtime CoreML EP page, not re-read
  today).
- The CoreML EP cannot be used in a basic minimal build (`cmake/onnxruntime_providers_coreml.cmake`); we build full.
- `ort-sys` adds `CoreML` and `Foundation` and `c++` for our archive; the smoke binary built on pyke's archive linked only
  system libraries (`otool -L`: libc++, Foundation, CoreML, CoreFoundation, libobjc, libSystem; verified).

## 7. Build times and machine needs

The jobs name runner labels only (`ubuntu-latest`, `macos-latest`, `windows-latest`), never a host, and any runner of
the right OS may take them (user, 2026-10-30). `build-ort.sh` detects the cores and memory at run time and uses cores
minus one compile jobs, at most one per 2 GB after 4 GB for the OS and the linker. The runners we know today, for the
estimates only: two Linux hosts (one about 1.5× slower than the other, docs/research/2026-10-ci-timings.md), a 14-core
Apple-silicon Mac shared with agents, and an 8-core, 16 GB Windows VM (user, 2026-10-30), where the rule gives 6 jobs.
**No ONNX Runtime build was timed here.** The only measured times are pyke's own CI runs of the
same kind of build (GitHub-hosted runners; job times from the GitHub Actions API, run 36487195855, 2026-09-28):

| pyke job (static) | Runner | Time |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu`, CPU | ubuntu-24.04 (4 vCPU) | 42 min |
| `aarch64-apple-darwin`, CoreML | macos-15 (3-core M1) | 24 min |
| `x86_64-pc-windows-msvc`, DirectML | windows-2022 (4 vCPU) | 66 min |
| `aarch64-pc-windows-msvc`, DirectML (cross build) | windows-2022 (4 vCPU) | 40 min |

Projections (scaled by compile jobs, unmeasured): Linux CPU **15–30 min** on the faster Linux host and **40–60 min** on
the slower one; macOS CoreML **15–35 min** on the shared Mac (the build runs niced, so other work slows it); on the
8-core, 16 GB Windows VM with 6 jobs, Windows DirectML **40–60 min**, Windows CPU **35–55 min** and, if enabled, the arm64
DirectML cross build **25–40 min**. The Windows variants run one after the other, about 1.5–2.5 h in all. Six jobs,
not seven, because 16 GB allows one MSVC compile job per 2 GB after about 4 GB for Windows and the linker (rule 51). The
times include fetching the sources (a few hundred MB) and building the third-party libraries. Every run is cold: a job
keeps nothing between runs. The cache is the published archive itself, rebuilt only when its key (the pins) changes. Machine needs: CMake ≥ 3.28, Python ≥ 3.10 (ONNX Runtime's `build.py` and ONNX's code generation; no Python of
ours), Git, a C++20 compiler (Visual Studio 2022 17.x with the C++ tools on Windows; Xcode command-line tools on macOS;
GCC 13 in the Ubuntu 24.04 image); disk about 10–15 GB per variant for sources and objects (projection); RAM: the
script allows one compile job per 2 GB.

Archive sizes, for the package store: pyke's static macOS CoreML library is 83.5 MB (measured, cached copy), its
Windows DirectML `onnxruntime.lib` 360 MB (research note 2026-10-29 §12); compressed archives of ours will be smaller
(not measured).

## 8. Recommendation

1. **Self-build every variant** with `scripts/ort/build-ort.sh`: `--no_telemetry`, static, Release, unit tests off,
   `FETCHCONTENT_TRY_FIND_PACKAGE_MODE=NEVER`; Windows with `--enable_msvc_static_runtime`; DirectML from our pinned
   copy. Merge to one archive, write `link.txt` and `BUILD-INFO.txt`, check telemetry, pack.
2. **Mirror nothing of ONNX Runtime.** Fallbacks, in order, if a self-build fails and the release cannot wait: (a) the
   macOS CoreML variant from pyke's archive, which passes the telemetry check and needs no CRT change (an ADR 0001
   amendment, since decision 8 says self-built); (b) none for Windows: pyke's archives are `/MD` and Microsoft's carry
   telemetry, so a Windows build failure is fixed, not worked around.
3. **CI**: `.gitea/workflows/sighter-ort.yml`, manual dispatch plus a monthly build-only run, never on push. Linux on
   any `ubuntu-latest` runner in the `paddock-ci` image, macOS on any `macos-latest` (Apple silicon) runner, Windows on
   any `windows-latest` runner, the Windows variants one after the other; labels only, own temporary folders, compile
   jobs detected at run time (user, 2026-10-30). Each job runs the telemetry check,
   the smoke program (CPU, plus CoreML on macOS; DirectML on Windows as an informational step) and, on Windows, the
   import checks. Publishing only on a dispatch from `main` with `publish: true`, to the generic package
   `sighter-onnxruntime`, version `<ORT>-p<rev>`, never overwriting.
4. **Windows arm64 + DirectML** (plan SW5) waits: no arm64 runner, and the brief scoped it out. pyke cross-builds it on an
   x64 Windows runner in 40 min, so it is one more job on a Windows runner when wanted (commented in the workflow) (`--arm64` in ONNX Runtime's build).

## Decided (user, 2026-10-30)

- **DirectML telemetry provider: ship only if silent.** `DirectML.dll` is bundled only if the S2 ETW session shows that
  the `Microsoft.Windows.AI.MachineLearning.Dml` provider writes no events while the DirectML EP runs; Windows'
  diagnostic settings then govern it like any OS component. If it writes events, the file is not shipped (the system
  copy through the delay-loaded import, or the `onnxruntime-cpu` variant). P6's check (2) is read that way.
- **DirectML source: fetch, never mirror.** `fetch-directml.sh` keeps fetching `Microsoft.AI.DirectML` from nuget.org at
  build time, pinned by NuGet's SHA-512 and the DLL's SHA-256; nothing is put on our package registry, so the
  `paddock-runtime` catalogues have no DirectML entry.
- **P6 method accepted.** Reading the DLL's bytes (import table, strings, provider traits) and recording its ETW events
  is treated as acceptable under the licence's no-reverse-engineering clause (3b); no disassembly.
- **Runners.** The builds run on any runner of the right OS: labels only (`ubuntu-latest`, `macos-latest`,
  `windows-latest`), never a host name; each job uses its own temporary folder and detects its compile jobs at run time.
- **macOS minimum.** 13.4 (`MACOS_DEPLOYMENT_TARGET` in `pins.env`).
- **Windows CI VM.** 8 cores and 16 GB, so the run-time rule gives 6 compile jobs there (used for the estimates).

## Still open

- **Hardware.** Only S2's two Windows PCs (U-H1), a release gate.

## Sources (all read 2026-10-30)

- ONNX Runtime releases and tags: https://api.github.com/repos/microsoft/onnxruntime/releases ,
  https://github.com/microsoft/onnxruntime/releases/tag/v1.31.1
- At commit 6af1a932e: `tools/ci_build/build.py`, `tools/ci_build/build_args.py`, `cmake/CMakeLists.txt`,
  `cmake/onnxruntime.cmake`, `cmake/onnxruntime_common.cmake`, `cmake/onnxruntime_providers_dml.cmake`,
  `cmake/external/dml.cmake`, `cmake/onnxruntime_providers_coreml.cmake`, `cmake/deps.txt`, `.gitmodules`,
  `onnxruntime/core/platform/windows/{telemetry.cc,env.h,env.cc,logging/etw_sink.cc,logging/etw_sink.h}`,
  `include/onnxruntime/core/platform/windows/TraceLoggingConfig.h`, under
  https://github.com/microsoft/onnxruntime/tree/6af1a932e2286d5c58166085d24c68783033edee
- `ort` and `ort-sys` 2.0.0-rc.15 crate sources: https://crates.io/crates/ort , https://crates.io/crates/ort-sys ;
  linking guide https://ort.pyke.io/setup/linking
- pyke's build recipe and CI: https://github.com/pykeio/ort-artifacts (`src/build.ts`, `.github/workflows/build.yml`),
  job times https://api.github.com/repos/pykeio/ort-artifacts/actions/runs/36487195855/jobs
- DirectML package: https://www.nuget.org/packages/Microsoft.AI.DirectML/1.15.4 , licence
  https://www.nuget.org/packages/Microsoft.AI.DirectML/1.15.4/License , NuGet catalogue entry
  https://api.nuget.org/v3/catalog0/data/2024.10.28.16.43.22/microsoft.ai.directml.1.15.4.json
- Telemetry group GUID (secondary):
  https://source.dot.net/PresentationFramework/src/Microsoft.DotNet.Wpf/src/Shared/Telemetry/Managed/TelemetryEventSource.cs.html ;
  provider groups: https://learn.microsoft.com/en-us/windows/win32/api/traceloggingprovider/nf-traceloggingprovider-traceloggingoptiongroup
- Family: ADR 0032 (branch `fix/windows-static-crt`), [ADR 0001](../adr/0001-object-detector.md),
  [research note 2026-10-29](2026-10-29-rf-detr.md) §11–12, `docs/research/2026-10-ci-timings.md`.

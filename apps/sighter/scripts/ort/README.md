# Sighter's ONNX Runtime builds

Stage-2 plan SW5 (ADR 0001 decision 4, plan item P5). Research, sources and build-time estimates:
[docs/research/2026-10-ort-builds.md](../../docs/research/2026-10-ort-builds.md).

App builds never compile ONNX Runtime. `.gitea/workflows/sighter-ort.yml` builds each variant once per pin, checks it and
publishes it to the Gitea generic package `sighter-onnxruntime`; `sighter-infer` links the archive through `ort`'s
custom-library mode (`ORT_LIB_PATH`, no `download-binaries`, no `load-dynamic`, no `copy-dylibs`).

The workflow runs only on a manual dispatch, as part of release preparation. Its jobs use runner labels only, never a
host name, and work in folders of their own that are removed at the end. They detect the number of compile jobs at run
time (`detect-jobs.sh`; user, 2026-10-30). A separate publish job runs no build code: it downloads the build artifacts,
re-checks each `.sha256` and uploads them.

The byte and import checks are shared code in `conformance/release/` (ER-22; Pacenotes needs the telemetry check too):
`telemetry-bytes.mjs`, and `windows-crt-imports.mjs` with its `--delay-load`, `--not-direct`, `--no-import` and `--no-network` rules.
Their tests are in `conformance/test/`. The scripts here are tested by `tests/run.sh` on synthetic fixtures. Sighter's
CI leg (`paddock.ci.json` `checks`) runs shellcheck, `tests/run.sh`, those two conformance tests, and `cargo fmt`,
`check` and `clippy` of `smoke/`.

This branch depends on `fix/windows-static-crt` (ADR 0032), which is merged into it: the import checker, and the root
`.cargo/config.toml`'s `+crt-static`, which the `/MT` ONNX Runtime needs in every binary that links it.

| File | What it does |
| --- | --- |
| `pins.env` | The one place for every pin: ORT tag and commit, pipeline revision, macOS floor, DirectML package and DLL hashes, package URL |
| `build-ort.sh <variant> <work> [--jobs N] [--dry-run]` | Clones the pinned tag, or copies the tree kept in `ORT_SOURCE_CACHE` (set only on runs that do not publish) after `check-source.sh`, builds with `--no_telemetry`, static, Release, no tests; checks CMake's telemetry switches; merges the static libraries into one; writes `link.txt` and `BUILD-INFO.txt`; runs the byte check; packs a `.tar.gz` and its catalogue lines. `ORT_USE_CCACHE=1` uses ccache (never for a published build) |
| `check-source.sh <dir> <commit> [submodule]...` | A source tree is trusted only if HEAD is the pin, `git fsck --full --strict` passes and `git status --porcelain --ignored --untracked-files=all` is empty, in the tree and each named submodule |
| `cache-sync.sh <restore\|save> <work> <cache>` | Moves the downloads (third-party source archives, the DirectML package) between the work folder and `apps/sighter/.ort-cache`, the stable path the workflow caches |
| `detect-jobs.sh` | Compile jobs for this runner: cores minus one (the cgroup CPU quota counts), at most one per 2 GB of available memory after 2 GB kept back (cgroup v1 and v2 limits count); `ORT_BUILD_JOBS` wins |
| `check-cmake-telemetry.sh <build>/Release` | `onnxruntime_USE_TELEMETRY` and `onnxruntime_USE_WINDOWS_TELEMETRY` OFF in `CMakeCache.txt`; no compile command defines `USE_1DS_TELEMETRY` or `USE_WINDOWS_TELEMETRY` |
| `fetch-directml.sh <x64\|arm64> <cache>` | Gets `Microsoft.AI.DirectML` 1.15.4 from nuget.org (never mirrored; user, 2026-10-30), checks NuGet's SHA-512 on every run (a cached package too) and the DLL's SHA-256, lays it out for `--dml_path` |
| `check-catalogue.sh` | Fails while a catalogue still has placeholder pins |
| `publish-ort.sh <folder>` | The publish job's step: re-checks every `.sha256`, uploads archive and `.sha256` with the token in a header file (never on a command line); a 409 with identical content is success, different content fails |
| `tests/run.sh` | Tests of the scripts above on synthetic fixtures (`tests/fixtures`), publishing against a local stand-in for Gitea (`tests/mock-packages.mjs`) |
| `catalogue/*.toml` | `paddock-runtime` catalogues: `onnxruntime` (what ships per platform) and `onnxruntime-cpu` (Windows CPU-only). **Placeholder pins until the first published run** |
| `smoke/` | The minimal `ort` program: links an archive through `ORT_LIB_PATH` (and `link.txt`), registers one EP with `error_on_failure`, runs a one-node model |

Caches: the workflow keeps `apps/sighter/.ort-cache` (git-ignored) between runs. Downloads go in on every run; they
are checked again when used. The source tree and ccache (`CCACHE_MAXSIZE=2G`, one key per variant and pin) are kept
only by runs that do not publish: any branch's run can write a cache, so a published build clones and compiles cold.

**To confirm on the first Windows run:** `--no-network` fails on any import of WinHTTP, WinINet, URLMon, WebSocket,
Winsock (`ws2_32.dll`) or `mswsock.dll`. Rust's standard library reaches Winsock only through `std::net`; whether a
binary that never uses it still imports `ws2_32.dll` is not verified here. If the smoke binary fails on `ws2_32.dll`
alone, inspect where the import comes from before relaxing the rule.

## Variants

| Variant | Runner | Catalogue key | Notes |
| --- | --- | --- | --- |
| `macos-arm64-coreml` | `macos-latest` (Apple silicon) | `onnxruntime`, `macos-aarch64` | macOS 13.4 floor |
| `windows-x64-directml` | `windows-latest` | `onnxruntime`, `windows-x86_64` | `/MT` (ADR 0032); DirectML from `fetch-directml.sh`, delay-loaded by the consumer |
| `windows-x64-cpu` | `windows-latest` | `onnxruntime-cpu`, `windows-x86_64` | `/MT`; no DirectML import: starts with no `DirectML.dll` present |
| `linux-x64-cpu` | `ubuntu-latest` (`paddock-ci` image) | `onnxruntime`, `linux-x86_64` | CI and tests only |
| `windows-arm64-directml` | not built yet | | Plan SW5 lists it; a commented, optional cross-build job on an x64 Windows runner is in the workflow (needs the variant in `build-ort.sh`) |

## The Windows import check for what ships

ADR 0032's checker reads the `.exe` and `.dll` files a Tauri configuration names in `bundle.resources`. Sighter has no
Tauri configuration yet, so the build job runs the checker on the smoke binary (with the variant's delay-load or
no-import rule and `--no-network`) and on `DirectML.dll`. When Sighter's app exists, `DirectML.dll` (only if the DirectML gate P6 passes) goes into `bundle.resources` of
its `tauri.windows.conf.json`, next to `sighter-infer.exe` (an `externalBin`), so the release path checks both like
every other app's shipped binaries.

## Consuming an archive (sighter-infer, SW17)

1. Read the catalogue, check it is pinned (`check-catalogue.sh`), fetch and verify the archive for the target.
2. Set `ORT_LIB_PATH=<archive>/lib` for the build (ort-sys then links `onnxruntime` statically and adds the C++ runtime,
   Foundation/CoreML on macOS and shell32 on Windows).
3. In `build.rs`, add every line of `<archive>/link.txt` (`smoke/build.rs` is the reference): system libraries, and on
   Windows `/DELAYLOAD:` for `DirectML.dll`, `d3d12.dll`, `dxgi.dll`, `dxcore.dll` with `delayimp`.
4. Set `ORT_DISABLE_TELEMETRY=1` and build the environment with `with_telemetry(false)` (the second line of defence).

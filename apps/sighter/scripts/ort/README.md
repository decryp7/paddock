# Sighter's ONNX Runtime builds

Stage-2 plan SW5 (ADR 0001 decision 4, plan item P5). Research, sources and build-time estimates:
[docs/research/2026-10-ort-builds.md](../../docs/research/2026-10-ort-builds.md).

App builds never compile ONNX Runtime. `.gitea/workflows/sighter-ort.yml` builds each variant once per pin, checks it and
publishes it to the Gitea generic package `sighter-onnxruntime`; `sighter-infer` links the archive through `ort`'s
custom-library mode (`ORT_LIB_PATH`, no `download-binaries`, no `load-dynamic`, no `copy-dylibs`).

| File | What it does |
| --- | --- |
| `pins.env` | The one place for every pin: ORT tag and commit, pipeline revision, macOS floor, DirectML package and DLL hashes, package URL |
| `build-ort.sh <variant> <work> [--jobs N] [--dry-run]` | Clones the pinned tag, builds with `--no_telemetry`, static, Release, no tests; merges the static libraries into one; writes `link.txt`, `BUILD-INFO.txt`; runs the telemetry check; packs a `.tar.gz` and its catalogue lines |
| `fetch-directml.sh <x64\|arm64> <cache>` | Gets (never mirrors; user, 2026-10-30) `Microsoft.AI.DirectML` 1.15.4 from nuget.org, checks NuGet's SHA-512 and the DLL's SHA-256, lays it out for `--dml_path` |
| `check-telemetry.sh` | Byte search for 1DS, curl, mbedTLS, collector hosts and TraceLogging providers in the Microsoft telemetry group |
| `check-windows-imports.sh` | ADR 0032's VC++ runtime check on the binary and `DirectML.dll`; the CPU variant must not import `DirectML.dll`, the DirectML variant must delay-load it |
| `check-catalogue.sh` | Fails while a catalogue still has placeholder pins |
| `publish-ort.sh` | Uploads an archive and its `.sha256` (CI only); a published file is never replaced |
| `catalogue/*.toml` | `paddock-runtime` catalogues: `onnxruntime` (what ships per platform) and `onnxruntime-cpu` (Windows CPU-only). **Placeholder pins until the first published run** |
| `smoke/` | The minimal `ort` program: links an archive through `ORT_LIB_PATH` (and `link.txt`), registers one EP with `error_on_failure`, runs a one-node model |

## Variants

| Variant | Runner | Catalogue key | Notes |
| --- | --- | --- | --- |
| `macos-arm64-coreml` | `macos-latest` (Mac mini) | `onnxruntime`, `macos-aarch64` | macOS 13.4 floor |
| `windows-x64-directml` | `windows-latest` (VM) | `onnxruntime`, `windows-x86_64` | `/MT` (ADR 0032); DirectML from `fetch-directml.sh`, delay-loaded by the consumer |
| `windows-x64-cpu` | `windows-latest` (VM) | `onnxruntime-cpu`, `windows-x86_64` | `/MT`; no DirectML import: starts with no `DirectML.dll` present |
| `linux-x64-cpu` | `ubuntu-latest` (`paddock-ci` image) | `onnxruntime`, `linux-x86_64` | CI and tests only |
| `windows-arm64-directml` | not built yet | | Plan SW5 lists it; a commented, optional cross-build job on the x64 VM is in the workflow (needs the variant in `build-ort.sh`) |

## The Windows import check for what ships

ADR 0032's checker reads the `.exe` and `.dll` files a Tauri configuration names in `bundle.resources`. Sighter has no
Tauri configuration yet, so `check-windows-imports.sh` runs the checker on the smoke binary and on `DirectML.dll` in the
build job. When Sighter's app exists, `DirectML.dll` (only if the DirectML gate P6 passes) goes into `bundle.resources` of
its `tauri.windows.conf.json`, next to `sighter-infer.exe` (an `externalBin`), so the release path checks both like
every other app's shipped binaries.

## Consuming an archive (sighter-infer, SW17)

1. Read the catalogue, check it is pinned (`check-catalogue.sh`), fetch and verify the archive for the target.
2. Set `ORT_LIB_PATH=<archive>/lib` for the build (ort-sys then links `onnxruntime` statically and adds the C++ runtime,
   Foundation/CoreML on macOS and shell32 on Windows).
3. In `build.rs`, add every line of `<archive>/link.txt` (`smoke/build.rs` is the reference): system libraries, and on
   Windows `/DELAYLOAD:` for `DirectML.dll`, `d3d12.dll`, `dxgi.dll`, `dxcore.dll` with `delayimp`.
4. Set `ORT_DISABLE_TELEMETRY=1` and build the environment with `with_telemetry(false)` (the second line of defence).

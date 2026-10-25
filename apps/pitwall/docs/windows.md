# Pitwall on Windows

Pitwall on Windows is the app (tray icon and window) plus the Pitwall server running as a **Windows service named `Pitwall`**,
so it keeps serving with the window closed. The service (LocalSystem) is a supervisor: it starts the server inside the logged-on user's session (chosen with `WTSEnumerateSessionsW`: active, then console, then lowest id) and restarts it when it exits or the session changes, so the data stays in your own `%APPDATA%\Pitwall`.

> **Not yet verified on a real Windows machine.** The Windows code type-checks and its pure parts are tested, and the Windows CI
> runner builds it, but no real run (engine install, model, service, installer) has been done. CUDA and Vulkan builds are
> unverified. Run the [manual tests](testing-manual.md) to confirm. The Tauri app is not in the repository yet; the server
> and API exist and can be run with `pitwall-server.exe`.

## Install

- Run the **NSIS installer** (`Pitwall_<version>_x64-setup.exe`). Builds are **unsigned** until the signing secrets exist
  ([releasing](releasing.md)), so Windows SmartScreen says "Windows protected your PC": choose **More info > Run anyway**.
- Installing the service needs administrator approval (the standard UAC prompt); the app never asks for a password itself and
  runs unelevated.
- **Requirements:** Windows 10 or 11, x64 (the engine catalog also lists Windows arm64 builds, which nobody has tried); WebView2 for the window (the
  installer bootstraps it).

## The server as a Windows service

- Name **`Pitwall`**, display name "Pitwall local LLM server", description "Serves local language models to the apps on this PC
  (127.0.0.1 only)." Automatic start.
- **Recovery:** if it fails the Service Control Manager restarts it after 10 seconds, then 60 seconds, then 5 minutes (repeated),
  and forgets earlier failures after 24 hours.
- The server also restarts onto a new program after an app update, only when idle.
- Engines are children of the server (which runs in your session), started with no console window and at below-normal priority, and killed with it. Windows
  adds a `conhost.exe` child to console programs; do not count that as a model process.
- Check it: `sc.exe query Pitwall`, `services.msc`. The supervisor's log is `%ProgramData%\Pitwall\logs\service.log`; the server's logs are in `%APPDATA%\Pitwall\logs\`.
- Over **Remote Desktop** your session is not the console session; the window works in your own session and the service does not
  depend on either.

## Files

Data folder: **`%APPDATA%\Pitwall`** (`PITWALL_HOME` overrides it).

| Item | What |
|---|---|
| `settings.toml` | Settings (with a backup copy) |
| `apps.json` | Apps and the hashes of their keys |
| `admin.key` | The secret the window uses |
| `discovery.json` | pid, port and base URL while the server runs |
| `models\<owner>\<repo>\` | Downloaded GGUF models |
| `engines\<build>\` | Installed `llama-server.exe` builds (`cpu`, `vulkan`, `cuda-12`, `cuda-13`, ...) |
| `partial\` | Unfinished downloads |
| `packs\` | Your model catalog packs |
| `logs\` | Service logs, no prompts or responses |

Keys that apps hold are in **Windows Credential Manager**.

## llama.cpp and GPUs

- **CUDA 13** or **CUDA 12** (NVIDIA; the CUDA build comes with its `cudart` archive, both checked), **Vulkan** (NVIDIA, AMD,
  Intel) or **CPU**. The suggested build is chosen for the hardware; the self-test shows the devices the build found. SYCL,
  OpenVINO and ROCm builds can be chosen by hand.
- Large downloads (a CUDA build is hundreds of MB): they resume if interrupted.
- Antivirus products sometimes quarantine freshly unpacked `llama-server.exe`; if the self-test says the engine could not start,
  check the quarantine list and allow the engine folder.

## Uninstall

Use **Settings > Apps** (or the uninstaller) to remove Pitwall, which also removes the `Pitwall` service. Your data in
`%APPDATA%\Pitwall` (models, keys, settings) is kept; delete the folder to remove it, and remove the Pitwall entries in
Credential Manager.

## Verified and not verified

Nothing on Windows has been run for real yet. Unit tests cover the Windows service spec, path rules and the process launcher
flags; the end-to-end tests run on the Windows CI runner where available.

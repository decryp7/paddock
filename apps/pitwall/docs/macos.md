# Pitwall on macOS

Pitwall on macOS is the app (menu-bar icon and window) plus the Pitwall server running as **your own launch agent**: it
starts when you log in, needs no administrator rights and keeps serving when the window is closed. The Apple Silicon path is
the one that has been run for real; see [Verified](#verified-and-not-verified).

> The Tauri app (window, menu bar, installer config) is in the repository, but no installer has been published yet. You can
> build it yourself or run the server by hand (`pitwall-server`) and use the API.

## Install

- **From a release:** open the `.dmg` and drag Pitwall to Applications. Builds are **unsigned** until the signing secrets exist
  ([releasing](releasing.md)): the app is sealed ad-hoc (`signingIdentity: "-"`), which is not a Developer ID. The first time,
  right-click Pitwall > **Open** and confirm, or allow it in **System Settings > Privacy & Security**. Gatekeeper may say the
  developer cannot be verified; that is expected for an unsigned build.
- **Why ad-hoc and not unsigned:** a bundle that is only linker-signed has no sealed resources and `SMAppService` refuses to
  register the launch agent (codesign error -67056). The shared `conformance/release/macos-verify-bundle.sh` (the `macos-verify` action) checks this on a built bundle and starts `pitwall-server --version` from it.
- **Requirements:** macOS 13 or later (for `SMAppService`); Apple Silicon or Intel. Apple Silicon uses the **Metal** build
  (the GPU shares system memory, so the whole RAM is the model budget). Intel Macs have only the CPU build.

## The server as a launch agent

- Label **`net.decryptology.pitwall.service`**. Its property list ships inside the app bundle and the app registers it with
  `SMAppService`; macOS shows a notification and lists Pitwall under **System Settings > General > Login Items**, where you can
  switch it off.
- launchd keeps it alive (`KeepAlive`): a crash or an exit after an update brings it back. The server exits for an update only
  when no request, job or download is in flight.
- launchd starts agents with a minimal `PATH`; the server asks your login shell for the real one (bounded, with a timeout) and
  passes it to llama.cpp.
- Status: `launchctl print gui/$(id -u)/net.decryptology.pitwall.service`. Logs: `~/Library/Application Support/Pitwall/logs/`.
- Without the app you can run `pitwall-server` in a terminal; it binds `127.0.0.1:11435` (`--port` and `--data-dir` change
  that).

## Files

Data folder: **`~/Library/Application Support/Pitwall`** (`PITWALL_HOME` overrides it).

| Item | What |
|---|---|
| `settings.toml` | Settings (with a backup copy) |
| `apps.json` | Apps and the hashes of their keys |
| `admin.key` | The secret the window uses (mode 0600) |
| `discovery.json` | pid, port and base URL while the server runs |
| `models/<owner>/<repo>/` | Downloaded GGUF models |
| `engines/<build>/` | Installed `llama-server` builds, for example `engines/metal/` |
| `partial/` | Unfinished downloads |
| `packs/` | Your model catalog packs |
| `logs/` | Service logs, no prompts or responses |

Keys that apps hold are in the **Keychain** (the app's own item), not here.

## llama.cpp and memory

- Pitwall installs the **Metal** build by itself (it is the suggested one on Apple Silicon; the **llama.cpp** page shows the progress): a download of the official
  `llama-b11669-bin-macos-arm64.tar.gz` archive from GitHub, SHA-256 checked, self-tested with `--list-devices` (expect an `MTL0`
  device). Sizes on Apple Silicon: a Q4_K_M 7B model needs about 5 GB plus KV cache, which matters on 8 and 16 GB Macs; use
  the "will this fit" verdict. The *Low memory* preset releases models after two minutes.
- llama.cpp is the only engine; there is no MLX. The app has a menu bar with **Quit Pitwall** and **Quit Pitwall and Stop Serving**.
- macOS' own memory pressure signal is not read yet; Pitwall acts on the available-memory threshold.

## Uninstall

1. Quit Pitwall from the menu bar and, in **System Settings > General > Login Items**, remove or switch off Pitwall (or use the
   app's "Uninstall service").
2. Drag Pitwall from Applications to the Trash.
3. Optionally delete `~/Library/Application Support/Pitwall` (models, keys, settings) and the Keychain items named Pitwall.

## Verified and not verified

Run on a Mac mini with an **Apple M4 Pro**: Metal llama.cpp `b11669` installed through the app flow with checksum and
self-test; Qwen2.5-0.5B-Instruct Q4_K_M downloaded from Hugging Face and its SHA-256 verified against the value Hugging Face
publishes; OpenAI chat (streaming and not), Anthropic messages, Ollama chat and JSON-schema constrained output all answered,
about 213 generated tokens per second. The test model was deleted afterwards. **Not verified:** the `.dmg` installer, the
launch-agent registration and Intel Macs, until the app is built; use the [manual tests](testing-manual.md).

# Manual tests

Some behaviour depends on the operating system, the GPU or the real engine and cannot be covered by the fake-engine tests, the
browser build with the mock backend, or Tauri's mock runtime: installing a real engine build, loading a real model, GPU use,
the tray, the service and updates. Run this checklist on each platform (Windows, macOS, Linux) before a release, against a
release build (see [releasing](releasing.md)). Automated coverage is described in [development](development.md).

Write down the platform, GPU, build (version and tag) and anything that differs from the expected result. **Only macOS on Apple
Silicon has been run for real so far** (section "Real-model smoke test", all steps passed with Metal `b11669`); Windows, Linux,
CUDA and Vulkan are unverified until these tests are run there and recorded.

Conventions: `PW=http://127.0.0.1:11435`, `PITWALL_KEY` is a key from pairing or **Create app**, `MODEL` is the id of the test
model below (`qwen2.5-0.5b-instruct-q4_k_m`). Use a separate data folder for a clean run: `PITWALL_HOME=/tmp/pitwall-test`
(on Windows a temp folder) when starting `pitwall-server` by hand. Never use real confidential content in prompts.

## Real-model smoke test (every platform)

The test model is **Qwen2.5-0.5B-Instruct Q4_K_M** (about 0.5 GB), from the starter catalog
(`Qwen/Qwen2.5-0.5B-Instruct-GGUF`). Delete it afterwards (Models > Library > Delete) if you do not want to keep it.

| # | Steps | Expected |
|---|-------|----------|
| 1 | Fresh data folder. Start Pitwall. `curl -s $PW/x/v1/health` | `{"ok":true,"name":"pitwall","apiVersion":1,"serving":"serving",...,"loadedModels":0}`. `discovery.json` exists in the data folder with the port. |
| 2 | Engines. Note the suggested build (Metal on a Mac; CUDA/Vulkan/CPU elsewhere). Click Install. | Progress shows the GitHub host. When it ends the card shows tag `b11669`, the devices found and "Self-test passed". The folder `engines/<id>/` contains `llama-server`, its libraries and `build.json`; no `.new` folder is left. |
| 3 | Models > Discover. Find Qwen2.5 0.5B Instruct, check the verdict (Fits), Download the Q4_K_M. | Progress, speed and host `huggingface.co` (or its CDN host). Verifying appears at the end. The model appears in Library as `qwen2.5-0.5b-instruct-q4_k_m` with capabilities chat and json. |
| 4 | Compare the file's SHA-256 with the one Hugging Face shows on the file page (`shasum -a 256 <file>` or `Get-FileHash`). | Identical. (Pitwall already checked it; this confirms the check is meaningful.) |
| 5 | Apps & keys > Create app "smoke" with defaults. Copy the key. `curl -s $PW/v1/models` without a key, then with `-H "Authorization: Bearer $PITWALL_KEY"`. | 401 with "a valid Pitwall API key is required..." without; with the key the model is listed. |
| 6 | OpenAI, not streamed: `curl -si $PW/v1/chat/completions -H "Authorization: Bearer $PITWALL_KEY" -H 'Content-Type: application/json' -d '{"model":"'$MODEL'","messages":[{"role":"user","content":"Say hello in five words."}]}'` | 200 with `choices[0].message.content`, `usage` token counts, and headers `x-pitwall-model: qwen2.5-0.5b-instruct-q4_k_m`, `x-pitwall-engine: llama.cpp`, `x-pitwall-chosen-by: explicit` and `x-pitwall-cold-start-ms` (the model was loaded for this request). The Dashboard shows the model loaded with an idle countdown. |
| 7 | OpenAI, streamed: same with `"stream":true` and `curl -sN`. | `data: {...}` chunks arrive incrementally, a usage chunk, then `data: [DONE]`. |
| 8 | Anthropic: `curl -s $PW/v1/messages -H "x-api-key: $PITWALL_KEY" -H 'Content-Type: application/json' -d '{"model":"'$MODEL'","max_tokens":64,"messages":[{"role":"user","content":"Say hello."}]}'` and again with `"stream":true`. | A `message` object with `content[0].text` and `stop_reason`; the stream shows `message_start`, `content_block_delta` events and `message_stop`. No cold-start header the second time (model already hot). |
| 9 | Ollama: `curl -s $PW/api/chat -H "Authorization: Bearer $PITWALL_KEY" -d '{"model":"'$MODEL'","stream":false,"messages":[{"role":"user","content":"Say hello."}]}'`; then without `"stream":false` | One JSON object with `message.content` and `done:true`; the streamed form is NDJSON lines ending with `"done":true`. `curl -s $PW/api/ps` lists the model with an expiry. |
| 10 | JSON-schema output (OpenAI `response_format` and Ollama `format`, see the [user guide](user-guide.md#openai-compatible)). | The content parses as JSON and has the required keys, every time. |
| 11 | `auto:` selection: `"model":"auto:chat,fast"` and `curl -s $PW/x/v1/select -H "Authorization: Bearer $PITWALL_KEY" -d '{"model":"auto:vision"}'` | The first answers with `x-pitwall-chosen-by: capabilities`; the second is 404 "no installed model offers vision ..." (the test model has no projector). |
| 12 | Speed: note `timings` in the Requests page for a 200-token answer. | Tokens per second is shown. Reference: about 213 generated tokens per second on an M4 Pro with Metal. Record your own number with the machine load next to it. |
| 13 | Requests page and Dashboard. | Every request above is listed with app "smoke", endpoint, priority, queue time, time to first token and tokens. The prompt text is not shown. |
| 14 | Quit the app (not the server). Repeat step 6. | Still answers: the server runs without the window. |
| 15 | Delete the model and the app. | The model file is gone from `models/`; `GET /v1/models` is empty; the key no longer works (401). |

## Memory and queue

| # | Steps | Expected |
|---|-------|----------|
| 16 | Settings > Memory > Low memory preset (release after 2 minutes). Make one request, then wait 2.5 minutes. | The Dashboard countdown reaches zero; the event feed says "Released <model> (Idle)"; `GET /api/ps` is empty. |
| 17 | Send another request. | It answers (a cold start): `x-pitwall-cold-start-ms` is present again. |
| 18 | Pin the model (Keep loaded), wait past the idle time. | It stays loaded. Unpin. |
| 19 | Start a long generation (`max_tokens` 2000), and meanwhile press Release memory now. | The busy model is **not** released; it is after the answer ends. |
| 20 | Pause serving, send a request. | 503 "Pitwall has paused serving. Resume it in Pitwall's window or tray menu." Resume; requests work. |
| 21 | Send 6 simultaneous requests with `X-Pitwall-Priority: batch` and one interactive. | The interactive one starts first; all complete; `GET /x/v1/jobs` shows the queue while they run. `DELETE /x/v1/jobs/<id>` for a queued one cancels it. |
| 22 | Load a model that does not fit (a large model on a small machine). | The verdict says Won't fit before download; if forced, the request fails with `insufficient_memory`, the machine does not start swapping. |
| 23 | Kill the `llama-server` process of a loaded model. | Within about five seconds the event feed says the engine exited unexpectedly and is restarting in 1 s; the model is loaded again by itself and the Dashboard shows no problem. Pitwall itself is unaffected. |
| 23a | Kill it again each time it comes back, six times within ten minutes (the delays double: 1, 2, 4, 8 s). | After the sixth the Dashboard shows the model with "keeps crashing", the reason and a Retry button; a request for it gets an error saying Pitwall stopped restarting it. Retry loads it again. |
| 23b | Memory pressure: with a model loaded and idle, fill memory with another program (macOS `memory_pressure -l warn`; Linux `stress-ng --vm 1 --vm-bytes 90%`; Windows: open many large apps) until the OS reports pressure. | Within five seconds the idle model is released with reason pressure (event feed); a busy model is not touched. |
| 23c | Idle context: set a model's release time to 2 minutes (Models > Release when idle), use it once and wait 70 seconds. | The event feed says the context was cleared and the weights stay loaded; after two minutes the model is released. |
| 23d | Start Pitwall a second time (double-click the app again; Windows/Linux also launch from the command line). | No second tray icon appears; the existing window comes to the front (rebuilt if it was closed). Autostart at login while it is already running changes nothing. |

## Engines, packs and failures

| # | Steps | Expected |
|---|-------|----------|
| 24 | Install a second build (for example CPU next to the GPU one) and select it, then Roll back. | The active build changes each time and chat requests keep working. |
| 25 | Disconnect the network midway through a model download, reconnect, start it again. | The message says the part is kept; the download resumes (same `.part` file) and ends verified. |
| 26 | Start an install with offline mode on. | "Pitwall is in offline mode: downloads are off. ..." No file is created. Turn it off. |
| 27 | Put a broken `bad.engine.toml` (for example no `[health]`) and a valid `*.models.toml` in `packs/`; restart the server. | The Engines page shows the problem for `bad.engine.toml` naming what to fix; the models pack still loads. |
| 28 | Put the [example engine pack](user-guide.md#an-engine-pack) in `packs/` with a real OpenAI-compatible server on `PATH` (llamafile works). Set a model's engine to it. | The engine appears as installed/available; a request starts it on a loopback port and answers; the response header names the engine. |

## Pairing and security

| # | Steps | Expected |
|---|-------|----------|
| 29 | `curl -s -X POST $PW/x/v1/pair -d '{"name":"Pair test"}'` | 202 with `pairingId`, a six-digit `code`, `expiresInSecs: 300`. The Dashboard and Apps & keys show the same code and name. |
| 30 | Poll `GET $PW/x/v1/pair/<id>` before approving, approve with a limited permission set (one model, 5 requests per minute), poll again, poll a third time. | `pending`; then `approved` with a `pw_...` key; then `approved` with `key: null` and "the key was already delivered". |
| 31 | Use the key with a model it is not allowed, and 6 requests in a minute. | 403 "this app may not use the model ..."; 429 "rate limit: at most 5 requests per minute ..." with `Retry-After`. |
| 32 | Deny a second request; let a third expire for 5 minutes. | `denied`. |
| 33 | `curl -s $PW/v1/models -H 'Host: example.com' -H "Authorization: Bearer $PITWALL_KEY"` and with `-H 'Origin: https://example.com'`. | 403 "this request's Host header is not a local address..." and 403 "requests from web pages on other sites are refused". |
| 34 | From another computer on the LAN, connect to `http://<this computer's address>:11435`. | Connection refused: the server listens on loopback only. |
| 35 | Revoke the key in Apps & keys, repeat a request. | 401. |
| 36 | Check `admin.key` permissions (`ls -l`, or `icacls` on Windows). | Readable only by the owner. |
| 37 | Search the logs (`logs/`) for a word from one of your prompts. | Not found. |

## Service, tray and updates

| # | Steps | Expected |
|---|-------|----------|
| 38 | First run (no service installed, nothing asked before): the window opens with "Run Pitwall's server in the background?". Choose Run in the background (or later use Settings > Background service > Install service; Remove service undoes it). macOS: approve in System Settings > General > Login Items (macOS 13 or later). Windows: the administrator (UAC) prompt, installed with the NSIS installer `Pitwall_<version>_x64-setup.exe`. Linux: no prompt (a systemd user unit). "No thanks" is remembered; "Not now" asks again next time. Start at login is a separate switch in Settings > General (your OS's login item). Check the service: Windows `sc.exe query Pitwall`; macOS `launchctl print gui/$(id -u)/net.decryptology.pitwall.service`; Linux `systemctl --user status pitwall.service`. | The service runs; `GET /x/v1/health` answers. |
| 39 | Kill the server process. | The OS restarts it within about 10 seconds (Windows: recovery actions at 10 s, 60 s, 5 min; systemd `RestartSec=10`; launchd KeepAlive). The window reconnects. |
| 40 | Log out and in (Linux: with `loginctl enable-linger $USER` the server also runs with nobody logged in). | The server is running again without opening the app. |
| 41 | Close the window; use the tray/menu-bar icon > Open. Repeat three times; on macOS also use the Dock icon. | The window reopens with content every time (never blank). Quit from the tray exits the app; the server keeps running. Linux with no tray: closing the window quits the app only. |
| 42 | Install a newer build over the old one while the server is idle (and once while a request streams). | The server restarts itself onto the new program only when idle; the streaming request finishes first; `version` in `/x/v1/health` changes; models reload on demand. |
| 43 | Uninstall the service. | The server stops; the data folder, models and keys stay. |

## UI

| # | Steps | Expected |
|---|-------|----------|
| 44 | Walk every screen (Dashboard, Models, Engines, Apps & keys, Requests, Playground, Benchmarks, Settings, tray popover) in light and dark and with the system theme changing. | Readable, no clipped text, status colours always with words or shapes; keyboard focus rings visible; text is not selectable except where it should be (log text, ids). |
| 45 | Disconnect the server (stop it) while the window is open. | Panels show an inline error with Retry, never an endless skeleton. |
| 46 | Run a chat in the Playground, cancel it midway. | Generation stops; the job leaves the queue. |
| 47 | Benchmarks > Run on the test model, then quit and start Pitwall again. | A tokens-per-second figure appears and is still listed after the restart (`benchmarks.json` in the data folder). |
| 47a | Settings > Privacy > Keep request content on, make a request that contains a fake key (`pw_` followed by 30 letters), open it in Requests. | The prompt and the answer show; the key is shown as `[redacted]`; turning the setting off keeps no new content; restarting Pitwall empties the list. |
| 47b | Settings > Updates > Check for updates (a build with an updater key only). | Nothing is contacted until you press the button; with an update, Install and restart. A build without a key says "Not configured". |
| 47c | Make a few requests with an app's key, then Apps & keys > that app > Usage over time. Switch 24h, 7d, 30d and Tokens, Requests, Peak memory. | Totals match the requests made; the chart has one bar per period; peak memory equals the size of the model that served them (Models shows its memory); the Dashboard's Usage by app chart shows the same app, and its legend link opens the app. |
| 47d | Set Requests per day to 3 for the app and make 4 requests; then set Memory share to 1 percent and make another. | The 4th answers 429 "this app has made its 3 requests for today ..."; the Limits list shows "Limit reached"; at 80 % (the 3rd of 4, or 2.4) a "Close to the limit" warning and one event appear; the memory share answers 403 naming the model's MiB and the share. Raising the limits lets requests through again. |
| 47e | Quit Pitwall, damage `usage.json` (write `{ not json`), start it. | It starts; the app's usage comes back from `usage.json.bak` (at most 30 s older); with both damaged the charts start empty. |
| 47f | Models > Presets > New preset: name `try-it`, serves from the test model, temperature 0.2, system prompt "Answer in one word." Then `curl` a chat with `"model":"try-it"` and a user message only. | The editor shows "Would serve" the test model before you save; the answer carries `X-Pitwall-Alias: try-it`; `GET /v1/models` lists `try-it`; Requests shows the row "via try-it" and the details "Preset asked for / Resolved to". Sending your own system message or temperature in the request leaves the preset's out. |
| 47g | Delete the test model (Models > Library), then repeat the `curl` with `try-it`. Edit the preset to an installed model. | 404 "the preset "try-it" points to the model ..., which is no longer installed"; the list says "Model removed"; after the edit the request works. |
| 47h | Quit Pitwall, damage `presets.json` (write `{ not json`), start it. | It starts; the presets come back from `presets.json.bak`; with both damaged the list is empty and the Dashboard events say so. |
| 48 | Reduced-motion and large-text settings of the OS. | Animations are off; layouts survive. |

## Command line

| # | Steps | Expected |
|---|-------|----------|
| 29 | With Pitwall running run `pitwall status`, `pitwall ps`, `pitwall models`, each also with `--json`. | Tables for people, the server's JSON with `--json`; no key appears in any output. |
| 30 | `pitwall pull Qwen/Qwen2.5-0.5B-Instruct-GGUF/qwen2.5-0.5b-instruct-q4_k_m.gguf`; interrupt it with Ctrl-C and run it again. | Progress on stderr naming `huggingface.co`; the second run resumes; the Models page shows the same download. With offline mode on: exit code 4 and the offline message. |
| 31 | `pitwall load <model>`, `pitwall ps`, `pitwall unload <model>`; load a model larger than the free memory. | Loaded, listed, released; the oversized load exits with 4 and says to unload another model. |
| 32 | `pitwall keys create cli-test --new`, then `export PITWALL_API_KEY=...; pitwall chat <model> "Say hi"`. | The key is printed once on stdout; the answer streams to stdout. `pitwall keys` shows only the id and prefix. |
| 33 | `pitwall keys revoke cli-test <key-id>`, then chat again. | The chat fails with exit 1 and a message about the key. |
| 34 | Quit Pitwall (and stop the service), run `pitwall status`; run `pitwall frobnicate`. | Exit 3 with "start the Pitwall app"; exit 2. |
| 35 | On a machine without the Pitwall app, put the server's `admin.key` in a file and run `pitwall --key-file <file> status` and with `PITWALL_ADMIN_KEY`. | Both work; `pitwall status --key abc` is refused with exit 2. |

Automated: `cargo test -p pitwall-cli` runs the unit tests and `tests/cli.rs`, which starts the built binary against a fake
local server on an ephemeral port with a temp data folder.

## Per-platform extras

- **Real-engine automated run (any platform):** `PITWALL_LLAMA_E2E=1 cargo test -p pitwall-server --test llama_e2e` downloads the
  pinned engine build and a tiny model into a temporary folder, asks one chat question and deletes everything. It is off in CI.
- **macOS (13 or later):** install from the `.dmg` (unsigned: right-click > Open the first time, or allow in System Settings > Privacy and
  Security); the Metal build uses the GPU (`--list-devices` shows `MTL0`). Run `scripts/ci/macos-verify-bundle.sh` on a built
  bundle.
- **Linux:** install the `.deb` and run the AppImage; with an NVIDIA card check `nvidia-smi` shows the process under the CUDA
  build, with an AMD/Intel card the Vulkan build; in a container without a GPU only the CPU build works. The AppImage's temporary
  mount must not be used by the service (the unit points at a stable copy).
- **Windows:** install with the NSIS installer (SmartScreen warns while it is unsigned: More info > Run anyway); the service
  `Pitwall` shows in `services.msc`; over Remote Desktop the window and server both work; the `llama-server.exe` processes run
  without a console window and at below-normal priority.

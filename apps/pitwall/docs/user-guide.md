# User guide

How to install Pitwall's engine, get a model, connect your apps and keep memory in check. For what each screen does see
[features](features.md); for platform specifics see [macOS](macos.md), [Linux](linux.md) and [Windows](windows.md).

Pitwall is **local only**: nothing you ask and nothing a model answers leaves your computer. The only network traffic is a
download you start (engines from GitHub, models from Hugging Face), and offline mode turns that off too
([ADR 0001](adr/0001-local-only.md)).

## Contents

- [First run](#first-run)
- [Install an engine](#install-an-engine)
- [Get a model](#get-a-model)
- [Will this fit?](#will-this-fit)
- [Connect an app: pairing](#connect-an-app-pairing)
- [API keys, permissions and limits](#api-keys-permissions-and-limits)
- [Using Pitwall from your tools](#using-pitwall-from-your-tools)
- [The `pitwall` command-line tool](#the-pitwall-command-line-tool)
- [Choosing a model by capability, priority and provenance](#choosing-a-model-by-capability-priority-and-provenance)
- [Model presets and aliases](#model-presets-and-aliases)
- [Parallel slots per model](#parallel-slots-per-model)
- [Memory presets and idle release](#memory-presets-and-idle-release)
- [Offline mode](#offline-mode)
- [Packs: add an engine or a model list](#packs-add-an-engine-or-a-model-list)
- [Where Pitwall keeps its files](#where-pitwall-keeps-its-files)
- [Troubleshooting](#troubleshooting)

## First run

1. **Install and open Pitwall** ([macOS](macos.md#install), [Linux](linux.md#install), [Windows](windows.md#install)). The window
   opens on the **Dashboard**. The server is a separate, windowless process that keeps serving when you close the window; the
   tray (menu bar) icon is how you get the window back.
2. On a fresh machine the dashboard says there is no engine and no model, and shows what to do. Follow
   [Install an engine](#install-an-engine) and [Get a model](#get-a-model).
3. **Check the server is up**:

   ```sh
   curl -s http://127.0.0.1:11435/x/v1/health
   ```

   ```json
   {"ok":true,"name":"pitwall","version":"0.1.0","apiVersion":1,"serving":"serving","offline":false,"loadedModels":0}
   ```

   The port is 11435 (next to Ollama's 11434). If another program uses it, Pitwall says "Pitwall cannot listen on ... Is
   another Pitwall (or an Ollama on the same port) running? Change the port in Settings > Server or start with --port."

If you run the server on its own (development, a headless Linux machine) the program is `pitwall-server`; its options are
`--port N` and `--data-dir PATH` (the environment variable `PITWALL_HOME` also sets the data folder).

## Install an engine

An engine is the program that runs a model. Pitwall's own is **llama.cpp**.

1. Open **Engines**. The card for llama.cpp lists the builds available for this machine and marks the **suggested** one:
   **Metal** on a Mac, **CUDA 13** or **CUDA 12** with an NVIDIA card, **Vulkan** for other GPUs, **CPU** if nothing else
   works. You can pick another one; ROCm, SYCL and OpenVINO can be chosen by hand but are never chosen automatically.
2. Click **Install**. Pitwall downloads the official release from GitHub (the host is shown), checks its SHA-256 against the
   value pinned in the catalog, unpacks only the files it expects, and runs a **self-test** (`--version`, `--list-devices`).
   The card then shows the tag (for example `b11669`), the devices the build found and "Self-test passed".
3. If the self-test fails, the new build is **discarded** and the previous one stays active; the message says why. You can
   also switch back to the previous build later with **Roll back**.

Keep **Build: auto** unless a build misbehaves: automatic order is Metal, then CUDA (newest toolkit first), then Vulkan, then
CPU. Only installed builds that work on this machine are considered.

## Get a model

Open **Models** then **Discover**.

- **Recommended** lists models from the built-in starter catalog (and any catalog pack you added) that fit this machine (those that will not fit are left out), such as
  *Qwen2.5 0.5B Instruct* (tiny and fast, 0.5 GB), *Qwen2.5 Coder 1.5B*, *Qwen2.5 7B Instruct* and *Nomic Embed Text v1.5*
  (embeddings).
- **Search Hugging Face** finds GGUF models. Open a repository to see its files, each with a [fit verdict](#will-this-fit).
  Pick a quantisation (Q4_K_M is a good start; Q8_0 is larger and closer to the original), then **Download**.
- A vision model needs its projector file (`mmproj`) in the same folder; download it with the model. Without it the model
  answers "cannot read images".

The **Downloads** list shows progress, speed and the host. A partial file is kept if the connection drops and resumes where it
stopped. When the bytes arrive Pitwall checks the SHA-256 against the one Hugging Face publishes for the file, then checks that
the file is a valid GGUF; a file that fails either check is deleted and the download fails with
"the downloaded file does not match its checksum ... it was discarded".

A model's **id** (what clients put in `"model"`) is its file name without `.gguf`, lowercased with other characters turned into
`-`: `qwen2.5-0.5b-instruct-q4_k_m`. `GET /v1/models` lists them. Clients may also use the Hugging Face repo name
(`Qwen/Qwen2.5-0.5B-Instruct-GGUF`) or `repo:quant`.

Gated models (Llama and others) need you to accept the licence on huggingface.co first; Pitwall says so when Hugging Face
answers 401 or 403 ("the file may be gated"). Pitwall does not take a Hugging Face token.

## Will this fit?

Every model and file in **Models** shows a verdict for this machine at the default context length (8192 tokens, a setting):

| Verdict | Meaning |
|---|---|
| Fits | Enough free memory with room to spare. |
| Tight | Fits only if Pitwall releases idle models first, or with little margin. |
| Won't fit | Needs more than this machine can free. Choose a smaller quantisation, a shorter context or close other apps. |
| Unknown | The size is not known. |

The sentence under it says where the model would run: entirely on the GPU, partly on the GPU and partly on the CPU (slower),
in shared memory (Apple Silicon) or on the CPU. The estimate adds the weights, the vision projector, the KV cache for the
context and compute buffers, so a longer context needs more memory. When a model loads for a request the check runs again;
if it cannot fit even with every idle model released, the request fails with `insufficient_memory` instead of swapping.

## Connect an app: pairing

Apps need a key. The easiest way for a suite app (Debrief, Pitcrew, Spotter, Dyno) is **pairing**, which the app does itself:

1. The app calls `POST /x/v1/pair` with its name and shows a six-digit code.
2. In Pitwall, **Apps & keys** (and the dashboard) shows a pairing request with the same code. Check the codes match, choose
   the permissions and click **Approve** (or **Deny**).
3. The app polls and receives its key **once**, then keeps it in the OS credential store (Windows Credential Manager, macOS
   Keychain, Secret Service on Linux). A request expires after 5 minutes.

You can do the same by hand to try it:

```sh
curl -s -X POST http://127.0.0.1:11435/x/v1/pair \
  -H 'Content-Type: application/json' -d '{"name":"My test tool"}'
# {"pairingId":"pr1-3f9a1c","code":"482913","expiresInSecs":300}

# approve it in Pitwall (Apps & keys), then:
curl -s http://127.0.0.1:11435/x/v1/pair/pr1-3f9a1c
# {"status":"pending"}   ... then {"status":"approved","key":"pw_..."}   (or "denied")
```

The key is in the answer exactly once; a second poll says "the key was already delivered". Pairing works only from this
computer and only when the request has no foreign `Origin`.

Prefer to create a key yourself (for `curl`, an editor plugin, a script)? In **Apps & keys** choose **Create app**, give it
a name and the permissions; the key is shown once in a dialog, so copy it then.

## API keys, permissions and limits

A key looks like `pw_` followed by 43 characters. Pitwall keeps only a hash of it and the first characters (`pw_3f9a1c...`)
to tell keys apart, so a lost key cannot be shown again: add a new key to the app and revoke the old one.

Per app (the permissions editor in **Apps & keys**):

| Setting | Effect when exceeded or off |
|---|---|
| Models | Only the listed models; others answer 403 "this app may not use the model ...". Default: all. |
| Highest priority | A request for a more urgent priority is lowered to this one (see [priority](#choosing-a-model-by-capability-priority-and-provenance)). |
| Tools | Off by default: "this app may not use Pitwall's tools". |
| Extended APIs | `/x/v1` calls that need it answer 403 when off. On by default. |
| Requests per minute | 429 "rate limit: at most N requests per minute for this app; retry in a moment". |
| Requests per day | 429 "this app has made its N requests for today; the budget resets at midnight UTC". |
| Tokens per day | 429 "this app has used its N tokens for today; the budget resets at midnight UTC". |
| Tokens per month | 429 "this app has used its N tokens for this month; the budget resets on the 1st at midnight UTC". |
| Memory share | 403 "the model needs N MiB but this app may use at most P% of the machine's memory ...". The share is a percentage of the machine's RAM; it is checked against the footprint (RAM plus video memory) of the model that would serve the request. |
| Concurrent requests | The app's extra requests wait in the queue. |

Every budget is optional (leave the field empty for no limit) and exists next to the others; a request is refused by the first
one it exceeds. Days and months count in UTC. When an app reaches 80 % of a budget, Pitwall adds a warning to the Dashboard's
events feed (once per period) and the app's detail shows the bar in the "Close to the limit" state.

### Usage per app

Select an app in **Apps & keys** to see its **Usage over time**: requests, prompt and completion tokens, errors and peak memory
for the last 24 hours or 7 days (hourly) or 30 days (daily), the **Limits** with a progress bar and a word for each (Within
limit, Close to the limit, Limit reached) and the app's latest requests (time, endpoint, model, status, tokens, duration;
never the prompt). The Dashboard's **Usage by app** chart shows the same for all apps together; its legend opens the app.

*Peak memory* is the memory Pitwall attributes to the app: for each request, the footprint (RAM plus video memory) of the
model that served it, which includes models its requests loaded or kept hot. In a period it is the largest such value, not a
sum, and not a measurement of the app. Hourly figures are kept 7 days and daily figures 90 days in `usage.json` in the data
folder, on this machine only; the file is rebuilt from nothing if it is damaged.

Send the key as `Authorization: Bearer pw_...` (OpenAI and Ollama clients) or `x-api-key: pw_...` (Anthropic clients).
Without a key you get 401 "a valid Pitwall API key is required. Create one in Pitwall > Apps & keys, or pair the app with POST
/x/v1/pair."

**Tools that cannot send a key** (some Ollama front ends): Settings > Server > **Accept local requests without a key**. It is off by default; it applies only to local requests that carry no browser `Origin`, and those requests are
counted as the app "local".

## Using Pitwall from your tools

The examples use a key in `PITWALL_KEY` and the model from the starter catalog. Replace the model id with one from
`GET /v1/models`. All of them work as written against `http://127.0.0.1:11435`.

```sh
export PITWALL_KEY=pw_...            # from pairing or Create app
export PW=http://127.0.0.1:11435
curl -s $PW/v1/models -H "Authorization: Bearer $PITWALL_KEY"
```

### OpenAI-compatible

```sh
# Not streamed
curl -s $PW/v1/chat/completions \
  -H "Authorization: Bearer $PITWALL_KEY" -H 'Content-Type: application/json' \
  -d '{"model":"qwen2.5-0.5b-instruct-q4_k_m","messages":[{"role":"user","content":"Say hello in five words."}]}'

# Streamed (server-sent events, ends with data: [DONE])
curl -sN $PW/v1/chat/completions \
  -H "Authorization: Bearer $PITWALL_KEY" -H 'Content-Type: application/json' \
  -d '{"model":"qwen2.5-0.5b-instruct-q4_k_m","stream":true,"messages":[{"role":"user","content":"Count to five."}]}'

# JSON-schema constrained output (grammar-enforced by llama.cpp)
curl -s $PW/v1/chat/completions \
  -H "Authorization: Bearer $PITWALL_KEY" -H 'Content-Type: application/json' \
  -d '{"model":"qwen2.5-0.5b-instruct-q4_k_m",
       "messages":[{"role":"user","content":"Name a colour and a fruit."}],
       "response_format":{"type":"json_schema","json_schema":{"name":"pick","schema":{
         "type":"object","properties":{"colour":{"type":"string"},"fruit":{"type":"string"}},
         "required":["colour","fruit"],"additionalProperties":false}}}}'

# Embeddings (needs an embedding model, e.g. nomic-embed-text)
curl -s $PW/v1/embeddings \
  -H "Authorization: Bearer $PITWALL_KEY" -H 'Content-Type: application/json' \
  -d '{"model":"auto:embeddings","input":["first text","second text"]}'
```

With the official SDKs set the base URL and key, for example in Python `OpenAI(base_url="http://127.0.0.1:11435/v1",
api_key=key)`, or the environment variables `OPENAI_BASE_URL=http://127.0.0.1:11435/v1` and `OPENAI_API_KEY=$PITWALL_KEY`.
`POST /v1/responses` (the Responses API) is served too.

### Anthropic-compatible

`max_tokens` is required, as in Anthropic's API. Images must be base64.

```sh
curl -s $PW/v1/messages \
  -H "x-api-key: $PITWALL_KEY" -H 'anthropic-version: 2023-06-01' -H 'Content-Type: application/json' \
  -d '{"model":"qwen2.5-0.5b-instruct-q4_k_m","max_tokens":128,
       "messages":[{"role":"user","content":"Say hello in five words."}]}'

# streamed
curl -sN $PW/v1/messages \
  -H "x-api-key: $PITWALL_KEY" -H 'Content-Type: application/json' \
  -d '{"model":"qwen2.5-0.5b-instruct-q4_k_m","max_tokens":128,"stream":true,
       "messages":[{"role":"user","content":"Count to five."}]}'
```

For the Anthropic SDK or Claude-style tools set `ANTHROPIC_BASE_URL=http://127.0.0.1:11435` and `ANTHROPIC_API_KEY=$PITWALL_KEY`.
A local model is not Claude: the API shape is compatible, the answers are the local model's.

### Ollama-compatible

```sh
curl -s $PW/api/tags -H "Authorization: Bearer $PITWALL_KEY"          # installed models
curl -s $PW/api/ps   -H "Authorization: Bearer $PITWALL_KEY"          # loaded models and when they expire

curl -s $PW/api/chat -H "Authorization: Bearer $PITWALL_KEY" \
  -d '{"model":"qwen2.5-0.5b-instruct-q4_k_m","stream":false,
       "messages":[{"role":"user","content":"Say hello in five words."}]}'

# JSON-schema constrained output: "format" is "json" or a schema
curl -s $PW/api/chat -H "Authorization: Bearer $PITWALL_KEY" \
  -d '{"model":"qwen2.5-0.5b-instruct-q4_k_m","stream":false,
       "messages":[{"role":"user","content":"Name a colour and a fruit."}],
       "format":{"type":"object","properties":{"colour":{"type":"string"},"fruit":{"type":"string"}},
                 "required":["colour","fruit"]}}'
```

Streaming is the Ollama default (`stream` omitted means newline-delimited JSON). Point Ollama clients at
`OLLAMA_HOST=http://127.0.0.1:11435` (and give them a key, or allow keyless local access).

## The `pitwall` command-line tool

`pitwall` is a small program for terminals and scripts. It is a client of the local server like any other: it calls
Pitwall's management API and the standard OpenAI endpoint on `127.0.0.1` and nothing else, and it can only open a
connection to this machine (it uses the same loopback-only client as the server's engine code). It adds no server
surface and sends nothing off-machine; downloads are done by the server, which shows the host and honours offline mode.

```sh
pitwall status                        # version, serving or paused, memory, loaded models, queue, speed
pitwall ps                            # models loaded in memory
pitwall models                        # installed models, size and fit
pitwall load qwen2.5-0.5b-instruct-q4_k_m
pitwall unload qwen2.5-0.5b-instruct-q4_k_m
pitwall pull Qwen/Qwen2.5-0.5B-Instruct-GGUF/qwen2.5-0.5b-instruct-q4_k_m.gguf
pitwall keys                          # apps and their keys (ids and prefixes only)
pitwall keys create my-script --new   # a new app and its first key; the key is printed once
pitwall keys create Debrief           # another key for an existing app (by name or id)
pitwall keys revoke Debrief key-1
pitwall presets                       # aliases and where each one resolves now
pitwall presets show debrief-explain
pitwall chat debrief-explain "Explain this error in one sentence"
echo "Summarise this" | pitwall chat fast
pitwall bench qwen2.5-0.5b-instruct-q4_k_m
```

**Finding the server.** The port comes from `--port`, else the discovery file in the data folder, else 11435. If nothing
answers, the tool exits with code 3 and says to start the Pitwall app or service.

**Keys.** Management commands need the admin key. The tool looks for it, in this order, in `--key-file <path>`, the
environment variable `PITWALL_ADMIN_KEY` (for headless machines: copy `admin.key` from the server's data folder), the OS
credential store (the entry the Pitwall window keeps) and the server's `admin.key`. `chat` uses a per-app key from
`PITWALL_API_KEY` instead, so a script holds a key with an app's limits, never the admin key. A key is never a command-line
argument (it would show in process lists and shell history; `--key` is refused), is never printed in output or errors, and
`keys create` prints the new key once, alone on standard output (progress and notes go to standard error), so
`KEY=$(pitwall keys create ci --new 2>/dev/null)` captures it.

**Output.** Read commands print tables for people; add `--json` for the server's JSON (`status`, `ps`, `models`, `keys`,
`presets`, `bench`, and `keys create`). `chat` streams the answer to standard output as it is generated; without a prompt
argument it reads standard input (at most 1 MB). `pull` shows progress on standard error (a live line on a terminal, a line
per 10 % otherwise) and resumes an interrupted download when you run the same command again; `--no-wait` starts the
download and prints its id. Pressing Ctrl-C stops the tool, not the download.

Every call has a time limit: 30 seconds by default, 10 minutes for `load`, `bench` and the first answer of `chat`; change
it with `--timeout <seconds>`.

| Exit code | Meaning |
| --- | --- |
| 0 | Done |
| 1 | An error (not found, wrong key, a failed download, an unexpected answer); the message says what to fix |
| 2 | The command line is wrong (unknown command or option, missing argument, no prompt) |
| 3 | The server is not running (or not on that port); start Pitwall or pass `--port` |
| 4 | The server refused: not enough memory to load, offline mode, busy, a limit reached, or the model is not loaded |

## Choosing a model by capability, priority and provenance

**Model by capability.** Instead of a model id, an app can ask for what it needs:

```sh
curl -s $PW/v1/chat/completions -H "Authorization: Bearer $PITWALL_KEY" -H 'Content-Type: application/json' \
  -d '{"model":"auto:vision,8k,fast","messages":[{"role":"user","content":"Hi"}]}' -i | grep -i '^x-pitwall'

# ask first, without running anything
curl -s $PW/x/v1/select -H "Authorization: Bearer $PITWALL_KEY" -d '{"model":"auto:tools,32k,quality"}'
# {"model":"qwen2.5-7b-instruct-q4_k_m","chosenBy":"capabilities","loaded":false}
```

Words in `auto:` are capabilities (`chat`, `vision`, `tools`, `json`, `embeddings`, `rerank`), a minimum context (`8k`, `32k`,
`16384`) and one preference: `balanced` (default: an already loaded model first, then the fastest), `fast`, `quality` (the
largest that fits) or `small`. A word it does not know is an error, so a typo does not pick another model silently. If no
installed model qualifies, the error lists what is missing ("no installed model offers vision with at least 8192 tokens of
context; download one in Pitwall > Models"). A request that contains images or tools is routed only to models that can handle
them.

**Priority.** `X-Pitwall-Priority: interactive | background | batch`. Interactive (the default) goes first; background is for
work done while the user does something else; batch is for bulk work and runs when nothing more urgent waits, but rises one
level every 30 seconds so it never starves. An app cannot go above its permission.

**Provenance.** Every answer carries headers that say who answered:

| Header | Meaning |
|---|---|
| `X-Pitwall-Model` | The model that answered. |
| `X-Pitwall-Engine` | The engine that served it: `llama.cpp`, or the id of the engine pack. |
| `X-Pitwall-Chosen-By` | `explicit` (named), `capabilities` (`auto:`) or `default`. |
| `X-Pitwall-Alias` | Present when the `model` field named a preset: the preset's name (`X-Pitwall-Model` is the model behind it). |
| `X-Pitwall-Cold-Start-Ms` | Present when the model had to be loaded first. |

With no `model`, Pitwall uses the default model from Settings, else a loaded one, else the first chat model installed.

## Model presets and aliases

A preset gives an app a stable name and lets you decide what serves it. In **Models > Presets** press **New preset**:

| Field | Meaning |
|---|---|
| Name apps send as the model | e.g. `debrief-explain`. Lowercase letters, digits and `-_.`, up to 48. It cannot be the id of an installed model, `auto` or `default`. |
| Serves from | An installed model id, or a capability selection such as `auto:chat,fast` (the same words as above); with a selection Pitwall picks the best installed model for each request. |
| Context, Temperature, Top-p, Max tokens | Defaults used when the request does not set them. Empty means the request (or the model) decides. |
| System prompt | Added in front of the conversation only when the request has no system message of its own. |
| Engine | llama.cpp, or an installed engine pack. |

While you type, the editor asks the engine what the draft would serve ("Would serve qwen3-8b-instruct-q4_k_m") and shows
anything that stops it being saved, in words. The list shows where each preset stands: *Ready*, *Model removed* (its model
was deleted; edit the preset), *No model matches* (nothing installed has the capabilities) or *Shadowed by a model* (an
installed model now has the same name, and requests for that name go to the model).

Apps use the name like a model id, in any of the three APIs; it is also listed in `GET /v1/models` and `GET /api/tags`:

```sh
curl -s $PW/v1/chat/completions -H "Authorization: Bearer $PITWALL_KEY" -H 'Content-Type: application/json' \
  -d '{"model":"debrief-explain","messages":[{"role":"user","content":"What does this error mean?"}]}' -i | grep -i '^x-pitwall'
# x-pitwall-alias: debrief-explain
# x-pitwall-model: qwen3-8b-instruct-q4_k_m
```

Values the app sends (temperature, top-p, max tokens, its own system message) always win over the preset's. Context and
engine apply when the model is loaded for the request; a model that is already loaded keeps its context and engine until it
is released. In **Requests**, a request made through a preset shows "via <name>" and, in the details, the preset asked for and
the model it resolved to. Presets are stored in `presets.json` in the data folder; a damaged file is restored from its backup.

## Parallel slots per model

A model can serve several requests at once. Each running request uses a *slot*, and every slot keeps its own context in memory,
so more slots mean more memory. In **Models > Library**, the **Slots** button of a model opens a panel:

- Pick 1, 2, 4 or 8 slots, or drag the slider (1 to 16). Under each choice the panel says what it adds ("adds ~3.1 GB").
- Drag **Context per slot** to see how much each slot can hold. The table answers, in words, how much memory the model needs at
  each slot count, the longest context each slot can have, and whether it fits, is tight or will not fit on this machine, and
  which is the most slots that fit at the chosen context.
- **Save** stores the choice. It applies the next time the model loads; a model that is loaded keeps its count until then, and
  requests it is serving are never interrupted. **Use the default** returns to `models.default_slots` (2).

The context you set is the context of one slot: with 4 slots of 8,192 tokens Pitwall starts the engine with a 32,768 token cache
(`--ctx-size`, which the engine splits evenly) and `--parallel 4`. If the model file does not describe its layout the memory per
slot is an estimate, and the panel says so. Requests beyond a model's slots wait in the queue. In **Discover**, each variant
also says how many requests at once the machine could run.

## Memory presets and idle release

Pitwall loads a model when the first request needs it and gives the memory back when it is not needed
([ADR 0006](adr/0006-memory-policy.md)). Choose a preset in Settings > Memory:

| Preset | Release a model after | Release everything after | Pressure threshold | Hot models |
|---|---|---|---|---|
| Low memory | 2 minutes idle | 10 minutes without requests | below 25% of RAM available | 1 |
| Balanced (default) | 10 minutes | 1 hour | below 15% | 3 |
| Performance | 1 hour | 8 hours | below 8% | 6 |

Editing any value makes the preset **Custom**. A load never leaves less than a floor free (1.5 GB Balanced). Rules, in order:

1. A model that is answering a request or loading is **never** released.
2. Under pressure, idle models are released least recently used first until the threshold is met.
3. An idle model is released after its idle time; the next request reloads it (a cold start, shown in the response
   header and the Requests page).
4. After the deep-idle time every unpinned model is released.

**Keep loaded** (pin) in the Models page exempts a model from the idle rule; a model can also have its own idle time. **Release
memory now** on the dashboard unloads every idle model; **Pause serving** makes new requests answer "Pitwall has paused serving.
Resume it in Pitwall's window or tray menu." while running ones finish.

Two parts of the policy are decided in the core but not acted on yet: yielding to a full-screen foreground app and releasing
only the context before the weights. Operating-system memory-pressure signals are not read yet either; the
available-memory threshold is what works today.

## Offline mode

Settings > Network > **Offline mode** disables every download (engines, models, searching Hugging Face). Everything already
installed keeps working, because serving never needed the network. A blocked action says: "Pitwall is in offline mode:
downloads are off. Turn offline mode off in Settings > Network to download." Use it on a machine that must not talk to the
internet, or to be sure nothing downloads in the background (nothing does without your click).

## Packs: add an engine or a model list

Packs are TOML files that you drop in `<data folder>/packs/` (see [where files are](#where-pitwall-keeps-its-files)).
They are data, not code ([ADR 0011](adr/0011-declarative-packs.md)). Packs are read when the server starts, so restart it after adding or changing one. The file name decides the kind: `*.engine.toml` or `*.models.toml`. At most 64 files of
256 KB are read; a pack with the same `id` as an earlier one replaces it (yours replace the built-in ones). A problem is shown
on the Engines page with the file name and what to fix; unknown keys are warnings.

### An engine pack

This example describes a self-installed OpenAI-compatible server that is started by a command on your `PATH`. Because it has no
`[[download]]` table it is an *external* engine: Pitwall only launches it.

`<data folder>/packs/mytool.engine.toml`:

```toml
format = 1                 # required; the pack format this version reads
id = "my-server"           # 1-64 letters, digits, - _ .  (not "llama.cpp")
name = "My OpenAI-compatible server"
description = "A server I installed myself, launched by Pitwall."
version = "1.2.0"          # required: pin the version you tested

[capabilities]             # at least chat or embeddings must be true
chat = true
embeddings = false
vision = false
tools = false

[launch]
# The command may be a bare name found on PATH or an absolute path. {host} is always 127.0.0.1.
command = "my-server"
# {host}, {port} and {model} are required. Others: {mmproj} {context} (per slot) {slots} {alias} {threads} {engine_dir} {binary}.
args = ["--host", "{host}", "--port", "{port}", "--model", "{model}", "--ctx-size", "{context}"]
# api_key_env = "MY_SERVER_API_KEY"   # optional UPPER_CASE variable; Pitwall sets a private key in it
start_timeout_secs = 120             # 5-1800, default 300

[health]
path = "/health"           # must start with /; Pitwall waits for a 200 here

[memory]
overhead_mib = 512         # memory on top of the model file and KV cache
```

To make Pitwall download the engine itself, add one `[[download]]` table per platform. Copy this shape (it is how the built-in
**llamafile** pack, `crates/pitwall-core/src/packs/llamafile.engine.toml`, does it); each platform may appear once, the URL must
be `https` on an allowed host (Hugging Face or GitHub), and the `sha256` and `size` are required:

```toml
[[download]]
platform = "linux-x64"     # windows-x64, windows-arm64, macos-arm64, macos-x64, linux-x64, linux-arm64
url = "https://github.com/example/my-server/releases/download/1.2.0/my-server-linux-x64.tar.gz"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"   # 64 lowercase hex characters
size = 123456789           # bytes
archive = "tar-gz"         # none (the file is the program), zip or tar-gz
binary = "my-server"       # the program's file name inside the download
```

When a pack has downloads, set `command = "{engine_dir}/{binary}"`. Rules Pitwall enforces: the arguments must contain `{host}`,
`{port}` and `{model}`; they must not name `0.0.0.0` or `::`; at most 64 arguments of 300 characters; only the listed placeholders.

To use it for a model, open **Models**, choose the model and set its **Engine** to `my-server` (a model's default is llama.cpp).
A pack engine is as capable as the OpenAI-compatible surface it exposes: no grammar-constrained output unless the engine has it,
and no llama.cpp `/props` capability detection.

### A model catalog pack

`<data folder>/packs/team.models.toml`:

```toml
format = 1
id = "team"                # 1-64 letters, digits, - _ .
name = "Team models"

[[model]]
id = "qwen2.5-3b-instruct"           # your name for it
name = "Qwen2.5 3B Instruct"
repo = "Qwen/Qwen2.5-3B-Instruct-GGUF"   # owner/name on Hugging Face
context = 32768                      # 256 to 10000000
capabilities = ["chat", "json", "tools"]   # chat, vision, tools, json, embeddings, rerank, transcription
tags = ["general", "small"]          # lowercase words; the Discover tab can filter by tag
variants = [
  { quant = "Q4_K_M", file = "qwen2.5-3b-instruct-q4_k_m.gguf", size_bytes = 2104932768 },
  { quant = "Q8_0",   file = "qwen2.5-3b-instruct-q8_0.gguf",   size_bytes = 3285476352 },
]
```

Each variant needs a `file` ending in `.gguf` and a positive `size_bytes` (the fit check uses it until the real file list is
fetched). Models appear under **Discover > Recommended** with a fit verdict; for each model the largest variant that fits (or is tight) is
recommended, and models that will not fit this machine are left out. Up to 500 models and 32 variants per model.

## Where Pitwall keeps its files

| Platform | Data folder |
|---|---|
| macOS | `~/Library/Application Support/Pitwall` |
| Linux | `$XDG_DATA_HOME/pitwall`, else `~/.local/share/pitwall` |
| Windows | `%APPDATA%\Pitwall` |

`PITWALL_HOME` overrides it. Inside:

| Item | What |
|---|---|
| `settings.toml` (+ backup) | Settings; edit in the app |
| `apps.json` (+ backup) | Apps and the hashes of their keys |
| `presets.json` (+ backup) | Model presets and aliases |
| `admin.key` | The secret Pitwall's own window uses; owner-only. Do not share it |
| `discovery.json` | Written while the server runs: pid, port, base URL |
| `models/<owner>/<repo>/` | Downloaded models |
| `engines/` | Installed engine builds (one folder per build id) |
| `partial/` | Unfinished downloads (resumed later) |
| `packs/` | Your packs |
| `logs/` | Service logs (no prompts or responses) |

## Troubleshooting

The messages below are what Pitwall actually says.

| You see | Why and what to do |
|---|---|
| 401 "a valid Pitwall API key is required. Create one in Pitwall > Apps & keys, or pair the app with POST /x/v1/pair." | The request has no key or an unknown one. Send `Authorization: Bearer pw_...` or `x-api-key`. A revoked key looks the same. |
| 403 "this request's Host header is not a local address; Pitwall only answers on 127.0.0.1" | You reached it through another name (a proxy, a tunnel, a LAN address). Use `127.0.0.1` or `localhost`. Network sharing is not built yet. |
| 403 "requests from web pages on other sites are refused" | A web page tried to call Pitwall. By design. |
| 404 "no models are installed. Open Models in Pitwall, search Hugging Face and download one." | Nothing to serve yet. |
| 404 "there is no model named ... Installed: ..." | Use an id from `GET /v1/models`. |
| 404 "the preset ... points to the model ..., which is no longer installed" (code `preset_target_unavailable`) | The preset's model was deleted. Edit the preset in Models > Presets. |
| 404 "no installed model offers ...; download one in Pitwall > Models" | No installed model has the capabilities in `auto:...`. |
| 400 "the model ... cannot read images. Choose a vision model (one downloaded with its mmproj file)." | Get the model's projector file too. |
| 400 "... is not a capability (chat, vision, tools, json, embeddings, rerank), a context like 8k, or fast/quality/small" | A typo in `auto:...`. |
| 400 "`max_tokens` is required" | Anthropic requests must have it. |
| 500 "no llama.cpp engine is installed. Open Engines and install one (it is suggested for this machine)." | Install an engine. |
| 500 "could not load ...: the engine exited (...)" | The engine failed to start; the last lines of its output are in the message. Try another build in Engines, a smaller model or context. |
| 500 "the model did not become ready within 5 minutes" | The load is too slow for this machine (swap, a huge model). Choose a smaller one. |
| 503 "... needs about N GiB of memory but only M GiB can be made free. Close other apps, choose a smaller quantisation or a shorter context." | `insufficient_memory`. Do as it says. |
| 503 "Pitwall is busy: N requests are waiting. Try again shortly or use a lower priority." | The queue is full (`Retry-After: 5`). |
| 503 "Pitwall has paused serving. Resume it in Pitwall's window or tray menu." | Serving is paused. |
| 429 "rate limit: at most N requests per minute for this app" / "has used its N tokens for today" | The app's limits; change them in Apps & keys. |
| 501 "speech-to-text is not available yet: the whisper.cpp engine is planned." | `/v1/audio/transcriptions` is not built. |
| 413 | The request is larger than the limit (32 MiB by default). Settings > Server. |
| Download: "Pitwall is in offline mode ..." | Turn offline mode off. |
| Download: "the file may be gated; accept its licence on huggingface.co first" | A 401 or 403 from Hugging Face. |
| Download: "the download failed: ... Check the connection and retry; the part already downloaded is kept." | Retry; it resumes. |
| Download: "Pitwall only downloads from Hugging Face and GitHub over HTTPS; ... is not allowed" | A pack or redirect named another host. |
| Install: "the new engine failed its self-test and was discarded (...); the previous build stays active" | The build does not run here (missing driver, wrong architecture). Pick another build. |
| Start: "Pitwall cannot listen on 127.0.0.1:11435" | The port is taken. Change it in Settings > Server or start with `--port`. |
| An engine pack is listed with a problem | Read the message on the Engines page: it names the file and the line to fix. |
| A model is slow after a pause | It was released when idle and is loading again (cold start). Pin it or use the Performance preset. |

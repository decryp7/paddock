# 23. A full chat in the Playground: text, images, speech input and MCP tools

- Status: Proposed (2026-11-05). Docs only: nothing here is built. Not yet independently reviewed.
- Date: 2026-11-05
- Related: ADR 0001 (local only), 0003 (API surface), 0007 (security), 0018 (capability gating), 0019 (caching), 0020
  (llama.cpp is the only engine), 0006 (memory). Family: ADR 0007 (MCP across the family), ADR 0013 (workbench and
  assistant),
  ADR 0026 (dialogs), ADR 0031 (patience, not wall clocks), ADR 0037 (composition), ADR 0049 (small merges behind
  switches),
  ADR 0052 on `origin/docs/adr-wiring-outcomes-ui` (wired features). Rules 12, 17, 22, 42, 46, 48, 51, 53; ER-19, ER-22.
  Recce's role: branch `docs/pitwall-rag-via-recce` (Pitwall builds no index of its own).
- Code read: `origin/pitwall/1.1-final` at `b7151a57f`, `origin/main` at `0e667eca7`, `origin/feat/dyno-mcp-readings` at
  `791f26090`.

**What is measured and what is judged.** This ADR contains **no measurement of ours**. Code facts were read in git;
nothing
was built or run. Sizes, licences and accuracy figures are quoted from their publishers, with the source below. Limits
and
choices are judgement and say so. WP-6 produces the first measurements, recorded in `docs/performance.md` (rule 48)
before
anything quotes them.

## Context

The user decided (2026-11-04, confirmed 2026-11-05) that Pitwall 1.1 gets a full chat in the Playground: text, speech
input
(possibly audio models), attached images and MCP tools, with Dyno the first MCP server and Recce later. Tool approval is
decided: ask the first time per tool, remember it, and show every call in the chat.

What exists, read in the code:

| Piece | State | Gap for this ADR |
|---|---|---|
| `pitwall_core::chat` | `Part::Text`, `Part::Image`, `ToolCall`, `Tool`, `ChatEvent::{Delta, Reasoning, ToolCall, Done}`; at most 16 images per request | no audio part |
| OpenAI, Anthropic and Ollama routes | stream text and tool calls, take inline images | `input_audio` is refused, `/v1/audio/transcriptions` answers 501 |
| `POST /mgmt/v1/chat` (Playground) | admin key, `Priority::Interactive` | `PlaygroundMessage` is text only and has no tools; tool-call events fall into `_ => (String::new(), false)` and are lost; `ttft_ms` is always the total time; it awaits `start_chat` (the cold load) before the response starts, while the app opens the stream with a 30 s limit (`commands.rs:387`), so a slow load can end as a timeout (ER-19, by reading, not reproduced) |
| `pitwall_core::gguf::Projector::max_image_pixels` | a per-projector pixel budget | no production caller: only its tests use it, so nothing limits an image's pixels |
| `parse_projector` | reads projectors | refuses an audio-only projector |
| `capabilities.json` and `capabilities_of` | gate tools by the chat template containing `tool`, vision by a projector | no audio; the substring test is crude |
| `paddock-assistant` + `@decryp7/paddock-ui/workbench/assistant` | tool loop with budgets, `Gate` approvals, `Cancel`, malformed calls reported and never run, versioned store, `<pd-assistant>` with markdown, tool cards and `PendingApproval`; only the workbench demo uses it on main | see Decision 1 |
| `paddock-mcp` | server, policy, audit, keys, loopback and stdio serving | client side is request builders only: no connection, no read loop, no cancel, no loopback handshake |
| Dyno's tools | 10 read-only, 2 opt-in actions (`wake_device`, `keep_awake`), results capped at 32 KiB | **nothing listens.** See Decision 7 |

## Decision

### 1. One engine: extend `paddock-assistant`, do not write a second chat (ER-22, rule 42)

The loop, approvals, budgets and pane already exist and only a demo depends on them, so changing them now is cheap.
Pitwall
provides an `LlmProvider` over its own `ServerClient` (admin key, `/mgmt/v1/chat`), not over `paddock_pitwall::Link`,
because
it is the server's own window and needs no pairing.

Shared changes, each a real gap:

| Gap in `paddock-assistant` | Change |
|---|---|
| `ChatMessage.content: String` | message parts (text, attachment references) |
| `Access::ReadOnly` runs without asking | an `ApprovalPolicy` port; the default keeps today's behaviour, Pitwall's implements Decision 8 |
| `Budgets::max_wall` (600 s) and `approval_wait` (120 s) end a turn on a wall clock | both become optional; Pitwall sets neither (ER-19, ADR 0031). A slow CPU model and a user at lunch are not failures |
| Tools fixed at construction | a tool set per turn, so connections can come and go |
| Prompted protocol is the only tool path | the provider may return native tool calls; the prompted protocol stays for providers that need it |
| One JSON file, 4 MiB, 50 conversations | a `ConversationStore` port; Pitwall's store is Decision 3 |

Logic stays in Rust: the loop, approvals, limits, capability checks, image handling and transcription requests. Angular
shows
what the engine reports and sends the user's choices.

The host lives in the **app** process (`pitwall-app`), not in `pitwall-server`. The server keeps its proof that a
request
handler cannot open a connection (ADR 0001), other programs' API requests never reach MCP, the service (LocalSystem on
Windows)
never talks to a user's apps, and approvals have a window to appear in. API clients get no hosted tools in 1.1: the
roadmap's
router row and Pacenotes' toolsets stay later.

### 2. The chat data model and the stream

```text
Conversation { id, title, model, system, params, created, updated, messages }
Message      { role, parts: [Text | Image{sha256, mime, w, h} | Audio{sha256, seconds} | File{sha256, name}],
               calls: [ToolCallRecord], stats, end }
ToolCallRecord { id, server, tool, arguments, approval: Remembered | Once | Denied | NotAsked,
                 outcome, duration_ms, result_text (bounded), result_cut, summary }
```

`PlaygroundRequest` and `PlaygroundEvent` grow additively, mirrored in `api.types.ts` in the same commit:

- request: parts, `tools`, `toolChoice`;
- events: `Phase { queued | loading{elapsed} | generating }`, `Delta`, `Reasoning`, `ToolCall`, `Done`, `Error`;
- the server starts the response **before** the cold load and reports the load as a phase, which removes the 30 s
  problem and
  gives ER-18's live label from the first moment. `ttft_ms` is fixed with a failing test first.

A turn reports: loading model, generating, waiting for your approval, calling Dyno: Top processes, generating. A quiet
phase
shows "Still working" through ADR 0031's policy. Stop works in every phase. A turn ends as answered, stopped, out of
budget
(with **Continue**), too long for the model (never cut silently), or failed with its cause and next step (ADR 0052 §2).

The existing Compare mode stays as it is, text only with no tools. The Playground gets two tabs: Chat and Compare
(judgement:
two models racing through tool approvals is confusing and few would want it).

### 3. Where chats live, and whether they are saved

- **Saved by default**, on this computer, per user, in the app's data folder: one versioned file per conversation
  (atomic with
  backup, loading tolerates damage) and attachments by SHA-256 beside them. One file per conversation means a save costs
  one
  conversation, not the whole history.
- A **"Don't save this chat"** switch per chat and a Settings switch for the default. Delete one or all.
- Not in the server, not in any log, never sent anywhere. Chats and attachments are **left out of Report a problem**,
  checked by
  a test that plants a marker.
- Files are protected by the user's account only, like the request inspector's data. Not encrypted (judgement:
  encryption with
  the key on the same machine adds little). The Settings text says so.
- Limits (judgement): 1,000 conversations, 16 MiB per conversation file, 2 GiB of attachments. At a limit Pitwall
  refuses the new
  item and says what to delete. It never deletes on its own.
- The opt-in request inspector also sees chat requests, since they pass through the same path. Its redactor gets the
  `pmk_`
  prefix next to `pw_` (`pitwall_core::redact`).

### 4. Images

- Added by file picker (the shared `paddock-files` dialog), paste or drag. `dragDropEnabled: false` is already set, so
  HTML5 drop
  works in the webview; the bytes go to Rust at once.
- Rust checks the magic bytes (PNG, JPEG, WebP, GIF, as `ImageMime` does), decodes with size limits on a blocking
  worker,
  downscales to the projector's `max_image_pixels` (finally wiring it, with a test that fails today), strips metadata by
  re-encoding, and stores it. This needs the `image` crate (MIT or Apache-2.0, checked by `cargo deny`). Pixel-bomb and
  corrupt-file
  tests are required.
- Limits: 16 images per request (existing), 32 MiB per request (existing `max_request_bytes`), 25 MiB per source file
  (judgement).
  Over a limit, the message says which and why.
- **Which models:** those with the `vision` capability, meaning a projector with an image encoder. `starter.models.toml`
  already
  marks several; llama.cpp lists Gemma 3 and 4, Qwen2.5-VL, Pixtral, InternVL, SmolVLM, Moondream and others. With an
  image attached
  and a model without vision, Send is disabled with words, and the pane offers a downloaded vision model. Images in the
  history
  stay in the conversation but are not re-sent to a model without vision, and the turn says so.
- Thumbnails are `data:` URLs (the CSP allows `img-src 'self' data:`). Image viewers open in fit mode (rule 45).
- Text and code files: attach as text, UTF-8 only, 256 KiB each (judgement; cheap and expected of a full chat). PDFs and
  other
  documents wait for Recce, which already has readers: no second extractor here.

### 5. Speech input

**Scope.** In: speech to text in the composer, and sending a recorded or attached audio clip to a model that hears. Out:
TTS and
spoken replies (not asked for; llama.cpp's multimodal path takes audio in, and speech output models use their own
runners per
Liquid's docs). Out: live partial transcripts in 1.1.

**The flow is push-to-talk.** Press or hold the mic button (with a shortcut), speak, release. Rust transcribes. The text
lands in
the composer to read and edit; nothing is sent by voice alone. A clip is capped at 5 minutes (16 kHz mono PCM16 is 9.6
MB, which
fits the request limit). Live needs a streaming path the pinned engine does not document (below) and adds a hot
microphone state
(judgement: not worth it for 1.1).

**Engine: llama.cpp itself, unless WP-6 says otherwise.** This is the finding that changes the shape of the work: the
pinned
llama.cpp (`b11669`) lists `ggml-org/Qwen3-ASR-0.6B-GGUF` and `1.7B`, Voxtral and Ultravox in `docs/multimodal.md`, and
its server
takes `input_audio` on `/v1/chat/completions`. So speech input needs **no new engine, no new binary and no new delivery
path**, and
ADR 0020 stands.

| Option | Licence | Size (publisher's) | Runs on | Delivery | Against it |
|---|---|---|---|---|---|
| **A. llama.cpp + Qwen3-ASR 0.6B** (recommended) | Apache-2.0 (Qwen's card) | Q8 805 MB, BF16 1.51 GB | whatever build Pitwall installed: Metal, CUDA, Vulkan, CPU | model library: HF hosts already allowed, SHA-256, GGUF check | larger than Whisper small; the server README calls multimodal experimental; streaming only with vLLM per the card; our speed unknown |
| B. whisper.cpp `whisper-server` | MIT | tiny 75 MiB (~273 MB RAM), base 142 MiB (~388 MB), small 466 MiB (~852 MB), medium 1.5 GiB, large 2.9 GiB | Metal, Core ML, CUDA, Vulkan, CPU | a second pinned engine; `.bin` models fail today's GGUF check | undoes ADR 0020; v1.9.5 lists 2 assets, so macOS and Linux builds probably mean building our own (not fully verified) |
| B2. `whisper-rs` in-process | not checked further | as B | as B | as B | C++ inside a Pitwall process breaks AGENTS.md's languages and containment rules |
| C. sherpa-onnx in a small helper process | Apache-2.0 | by model | CPU first (judgement) | a crate that fetches or links prebuilt libraries, statically by default; new model layout | a new runtime, helper and model format; gives Whisper, Moonshine, Parakeet, SenseVoice in one API |
| D. Moonshine | code MIT; models MIT except legacy non-English non-streaming | sizes not on the project page | desktop OSes per its page | its own SDK; through C otherwise | no Rust binding on its page |
| E. Parakeet TDT 0.6B v3 | CC-BY-4.0 | 0.6B parameters, at least 2 GB RAM | designed for NVIDIA GPUs, Linux preferred | through C | 25 European languages only; weak fit for Macs and CPUs |

Publisher figures, not ours and not comparable across cards: Qwen3-ASR 0.6B mean WER 6.42 on the Hugging Face Open ASR
leaderboard,
30 languages and 22 Chinese dialects. Moonshine claims better accuracy than Whisper large-v3 on the same leaderboard.

**WP-6 decides with numbers**, on the user's Mac and on a CPU-only VM: real-time factor, cold start, memory, and
accuracy on a few
invented clips in the user's languages, for A, for B small, and for C with Moonshine. If A is too slow on CPU or too
poor, C is the
fallback (it keeps ADR 0020 and containment, adding 3 to 4 days to WP-7). The failure of A is not a surprise to plan
around silently: the
result comes back to the user.

**A model that hears** (Gemma 4 E2B and E4B, Voxtral Mini 3B, per llama.cpp's list) uses the same `Part::Audio` and the
same request,
so attaching a clip costs little once speech input exists. It is gated by `modalities.audio`, which `llama.rs` already
reads, and a
`clip.has_audio_encoder` projector, once `parse_projector` stops refusing audio-only ones. Audio files go through
miniaudio's
formats (mp3, wav, flac per the README).

**Server.** A shared transcribe path in the core serves `/mgmt/v1/transcribe` and replaces the 501 on
`/v1/audio/transcriptions`
(`Capability::Transcription` and `JobKind::Transcription` already exist). `capabilities.json` gets `audio`, with its
floor checked
against llama.cpp's history as ADR 0018 requires. Pitwall sends audio inline and refuses a URL: llama.cpp's
`input_audio.url` and
`--media-path` accept remote URLs and local paths, and Pitwall never sets `--media-path` nor forwards a URL, as for
images. A test
pins that `--tools` (llama.cpp's own file and shell tools) is never passed to the engine.

**First use.** The first press offers the model with its size and a fit check, then downloads it as a job with progress
and Cancel.

**Microphone capture: in Rust, in the app, through `cpal`** (Apache-2.0; CoreAudio, WASAPI, ALSA), behind a port so
tests never touch a
device (rule 7). Not through `getUserMedia`, because the webview path differs per OS and fails on one:

| OS | Webview route, as researched | Native route with `cpal` |
|---|---|---|
| macOS | WKWebView prompts have varied by OS version in Tauri's reports; dev-mode problems tracked in wry#1195 | `NSMicrophoneUsageDescription` in the app's `Info.plist` (the only plist today is the service's). The hardened-runtime audio-input entitlement is needed only if the runtime is on: ad-hoc bundles keep it off (lessons), so it joins with signed builds. Grants are tied to the code signature and an ad-hoc update changes it (lessons.md:275), so **a re-prompt per update is likely**: Repair opens the Privacy pane (rule 19), verified on the user's Mac |
| Windows | WebView2 has a permission path (its docs page returned 404 on 2026-11-05, not verified) | Settings > Privacy > Microphone; "desktop apps" off gives a clear message with a link to the pane |
| Linux | Tauri's maintainer (2024): WebKitGTK has no WebRTC; `mediaDevices` is off by default | ALSA, which reaches PipeWire and PulseAudio through their plugins. Build needs `libasound2-dev`, the `.deb` a dependency, and the AppImage a check. Tested on the OrbStack machine |

The Tauri capability set does not grow: the UI only starts and stops through a guarded app command. A visible recording
state with
elapsed time is always on screen while recording. Audio is deleted after transcription and never stored unless the user
attached it as
a clip. Nothing about it reaches logs except duration, size, engine time and outcome.

### 6. MCP client transports

| Transport | In 1.1 | Why |
|---|---|---|
| Family loopback (`pmk_` key handshake, then JSON lines on 127.0.0.1) | **Yes, first** | what family apps with a running process offer. Not HTTP, so ADR 0001's HTTP rule is untouched; the address type admits only 127.0.0.1, as `pitwall-loopback` does; `privacy-check.sh` and a test extend to it |
| stdio, program chosen by the user | **Yes, last and cuttable** (WP-10) | most third-party servers. MCP's security guidance: show the exact untruncated command, say it runs code with the user's rights, require approval. Spawned through `paddock_proc`, stderr to the log capped. All its tools are treated as actions (Decision 8) |
| Streamable HTTP | **No** | any remote form is an outbound connection, which needs its own ADR and the user's approval (AGENTS.md, ADR 0001) |

`paddock_mcp::client` grows a `Connection` over any reader and writer: requests with ids, one read loop, paged
`tools/list`, cancel through
`notifications/cancelled`, a line limit, typed errors. Plus `connect_loopback` and `spawn_stdio`.

**Protocol version.** `paddock-mcp` speaks 2025-06-18 (with two older versions). MCP's current revision is
**2026-07-28**, which removes
`initialize`, adds `server/discover` and carries the version on every request. Family servers and the host move
together, so this does
not affect Dyno or Recce now. A third-party stdio server that is 2026-07-28-only will fail with a clear message until
the client learns the new
shape (2 to 3 days, not in this ADR's table).

### 7. Connecting to Dyno, and the gap in front of it

**Nothing serves MCP today.** At `791f26090`, `serve_loopback` and `serve_stdio` have no production caller in any app:
only Dyno's test
calls `serve_stdio` over a buffer. Dyno builds `McpControl`, keys and settings, but opens no listener and has no `--mcp`
mode. Its user
guide says a client can connect. Its tools call `shell.call_agent` in the app process, so loopback in Dyno's app is the
natural form, and
stdio would need a different design. This must be fixed in Dyno before Pitwall can connect at all (WP-13).

Proposed flow:

1. Shared: when MCP is on, the serving app listens on an ephemeral 127.0.0.1 port and writes a small discovery file in
   its data folder:
   port, pid, start time, protocol. It is a hint only (rule 53): the handshake proves the server. The listener follows
   the
   master switch.
2. Dyno: Settings > AI access > turn on > Issue key > name it "Pitwall". The key is shown once, as today (`pd.mcp.key`).
3. Pitwall: Settings > Connected apps lists family apps found with `paddock-siblings`. Connect opens a credential
   dialog, an OS window
   (rule 17), to paste the key. It is kept in the OS store (`paddock-secrets`), never in a file or log, never shown
   again.
4. Pitwall connects, shows the server's name and tool list, and the user picks which tools chats may offer.
5. Dyno not running: the connection reads "Dyno is not running" with Open Dyno (`paddock-siblings`), and retries with
   the family's
   back-off. A chat never fails because of it; the tools are simply not offered, and the composer says so.

A one-click pairing like Pitwall's own six-digit flow (ADR 0004) would be nicer, but it needs new consent UI in each
server app. Later.

Threat note (judgement): a process of the same user could listen on the port and receive the key. A same-user attacker
can read much
more already, so this is accepted, in line with ADR 0007's loopback model. The key is per client and revocable in Dyno.

### 8. Approval, as the user decided, and where actions differ

Every tool starts **off the model's list until connected and picked**. Then:

| Kind | First call | Later calls |
|---|---|---|
| Read-only tool of a family server | ask: Allow once, Always allow, Deny | runs without asking, shown in the chat |
| Action tool (`wake_device`, `keep_awake`) | ask, showing the exact arguments | **proposed: ask every call** (decision D2 below) |
| Any tool of a user-added stdio server | as an action | as an action, until the user marks that tool "always allow" in Settings |

- The **card is inline in the chat**, not a dialog: it belongs to the turn, the conversation stays usable, and Stop
  works. Rule 17 is about
  dialogs; a decision about a turn in its own transcript is the assistant's existing `PendingApproval`. An independent
  reviewer should confirm
  this reading.
- The card is drawn from the **parsed call in Rust**: server, tool title, every argument. Model text never fills it.
- A remembered approval is keyed by server, tool and a **hash of the tool's name, description and schema**. If the
  server changes them, the
  approval lapses and Pitwall asks again (the "tool changed after approval" attack).
- MCP says annotations are untrusted unless the server is trusted. Read-only versus action comes from family servers'
  `readOnlyHint`,
  with the family identified by a key the user pasted; a stdio server's hints are ignored.
- Settings > Connected apps lists every remembered approval and revokes it. Deny tells the model it was denied and the
  turn goes on.
- Every call in the chat, always: tool, arguments, status, duration, result (expandable, as text), whether it was cut,
  how it was approved.
- Audit: the server keeps its own (`paddock_mcp::AuditLog`). The host adds a bounded `AuditLog` in the app and one
  `pitwall_chat` log line per
  call: correlation id, server, tool, approval kind, outcome, duration, result bytes, arguments through
  `redact_arguments` and no values by
  default (ER-27, no user content).

### 9. The tool loop

Pitwall has one backend, llama.cpp behind `/mgmt/v1/chat`, so there is one loop.

- **Native tool calls** (`tools` through `--jinja`) on models with the `tools` capability. Pieces are assembled in Rust.
- **A model without tool support:** the tool switch is disabled with words, and the pane offers a downloaded model that
  has it. No silent
  prompted fallback: the Hermes prompt is a family mechanism for models that follow it, and asking one that doesn't
  gives unreliable
  calls (judgement). WP-6 tests the capability test against the starter catalog, because the substring check may claim
  too much.
- Calls run one at a time, in the model's order. Invalid JSON or arguments failing the tool's schema
  (`paddock_mcp::schema`) go back to the
  model and never run, and never reach an approval card.
- Budgets (judgement, Settings): 8 rounds, 16 tool calls per turn. On reaching one the turn ends with **Continue**;
  nothing is lost.
- **No UI timeout on a tool call or a turn (ER-19).** MCP's guidance that clients should implement timeouts is met by
  the server's per-tool limit
  (`paddock_mcp::Limits`, the call is abandoned and answered) and by Stop on the host: "Still working" after a quiet
  spell, Stop sends the
  cancel notification and, if nothing answers, ends the connection and reconnects. A lost connection is that call's
  outcome and goes to the
  model as a failed call.

**Limits on what reaches the model.** Text content only; image, audio and resource items are listed in the card and not
sent in 1.1. Each result is
cut to the smaller of 16 KiB and a quarter of the model's context (judgement; Dyno caps at 32 KiB), with the cut stated
to the model and to the
user, who can still read up to the stored cap of 64 KiB. Tool definitions are bounded too: at most 64 tools per turn
(the assistant's existing
limit), sorted by name so the prompt prefix stays the same. The composer's context meter counts them.

### 10. Security

| Threat | Measure |
|---|---|
| A tool result carries instructions (prompt injection) | Results are data: wrapped and labelled as tool output, the system prompt says so (`protocol.rs` already does). OWASP: no method is known to be fool-proof, so **the controls below assume the model can be fooled** |
| The model acts on injected text | No action without approval. The model cannot approve, and cannot reach the approval path: it is a Rust state only a window command changes. An action's arguments are shown in full |
| Read something private, then leak it | No network tool in Dyno or Recce. Actions ask every call (D2). Pitwall fetches no URL: no remote images (CSP), no HTML (the pane's markdown parser builds typed blocks, never `innerHTML`), links open only by the user's click through the OS, showing the address |
| A tool description lies, or changes later | Descriptions are shown before the first approval; the hash in Decision 8 re-asks on a change. Descriptions are capped in length and control characters are stripped, since they go to the model |
| A malicious stdio server | Exact command shown, explicit consent, no one-click import from a file or link, user rights only, nothing elevated. Sandboxing is not attempted in 1.1 and the dialog says so |
| Cloud models | None. ADR 0001 stands: there is no remote backend, so tool results and images stay on this computer. Family ADR 0007 rule 7 is met by construction |
| Keys | `pmk_` keys only in the OS store. Both redactors know `pw_` and `pmk_`. Never in chats |
| Bad files | Images and audio decode with limits, off the UI thread, in Rust with proptests that never panic; the engine gets checked, re-encoded input where it can |
| Webview compromise | Tauri capabilities do not grow; every new command is in the shell's guard and manifest (C-checks); key and approval commands take typed ids |

### Alternatives rejected

- **A chat built only inside Pitwall** with its own loop: a copy of `paddock-assistant`, and rule 55 would register it
  at once.
- **The host in `pitwall-server`:** serves API clients too, but breaks ADR 0001's provable handler rule, has no window
  for approvals, and runs as
  a service in the wrong context on Windows.
- **Pitwall's own document index:** decided against by the user; Recce is one more MCP server here.
- **MCP through llama.cpp's built-in `--tools` or its web UI:** runs shell and file tools in the engine with no
  approval; contrary to ADR 0001 and this ADR.

## Parallelism and cost (rule 51)

| Operation | More threads? | Cheaper? | Status |
|---|---|---|---|
| Tool rounds | The engine generates one answer per slot, so more threads do not help; calls run in sequence so approvals stay readable. Read-only calls could run in parallel later, bounded at 4 | **Yes:** each round re-sends the prompt. `--cache-reuse 256` and slot affinity (ADR 0016, 0019) make the prefix cheap only if it does not change, hence sorted tools and appended-only history | To measure in WP-3 |
| Image preparation | Bounded worker, 2 at a time (judgement: CPU and memory heavy) | Downscale to the projector's budget, dedupe by SHA-256, keep the prepared copy so a later turn does not decode again | To measure in WP-5 |
| Speech | One at a time; other chat work waits through the same scheduler at `Interactive` priority | 16 kHz mono; trim leading and trailing silence by a simple energy check first (a VAD model only if measured worthwhile); keep the model loaded under the existing idle rules | WP-6 measures. Whether speech and chat models fit together goes through the fit check; never into swap (ADR 0006) |
| Saving | One writer per conversation | Save at turn ends and each tool result, never per token; one file per conversation | By design |
| `tools/list` | Bounded per connection | Cached per connection, refreshed on `list_changed` and on opening the chat, not per turn | By design |
| Tool schemas in the prompt | n/a | They take context on every turn: a per-chat tool chooser, and the meter shows the cost. 12 Dyno tools' token cost to be measured in WP-9 | To measure |

Thread counts stay Settings with Auto where they are settings at all. None of this is a large job, so rule 46 does not
apply: one person's chat
turn is the LLM's own purpose. Priority: live work, normal level on Windows (rule 52).

## Testing

- Rust first: the loop, approvals, the hash, limits, parts, store damage, on fakes with an injected clock.
  `pitwall-core` keeps 99 % lines.
- A failing test first for each found bug: tool calls lost at `/mgmt/v1/chat`, `ttft_ms`, the pixel budget not wired,
  the slow cold load.
- A real MCP pair in one test: `serve_loopback` with fake tools against the new client, including the hung tool, Stop,
  the dropped connection, a
  changed tool, a revoked key, an over-long line.
- Injection fixtures: a tool result that says to call `keep_awake`. The test asserts that the action does **not** run
  without a decision, whatever the
  fake model does.
- Mock backend for the pane. Real-engine tests per ADR 0052 §1.4 in `features.toml`: chat, vision and speech with tiny
  models when
  `PITWALL_LLAMA_E2E=1`; Dyno connected for real in one test per OS through its VM row.
- Tests never touch a microphone, the real credential store or the user's apps (rule 7). Real-mic checks are a manual
  row on the user's Mac and the VMs'
  available devices, recorded with a verified precondition (rule 71: a probe that has shown it can see a sound).
- Every screenshot and diagram goes to independent review (rule 22).

## Work packages

Agent-days, ranges, judged from the sizes of similar family work; not measured. Shared-code packages land behind a
switch and after the Dyno tag, or are additive
(`paddock-assistant` has no Dyno or Spotter user). Numbers in **bold** block others.

| WP | What | Days | Needs |
|---|---|---|---|
| 1 | Wire: parts, tools, phases and events at `/mgmt/v1/chat`; response before the load; `ttft_ms`; contract tests | 1.5–2.5 | 1.1 base landed |
| 2 | `paddock-assistant`: parts, `ApprovalPolicy`, optional wall limits, per-turn tools, native calls, store port; TypeScript regenerated; the demo updated | **3–5** | |
| 3 | Pitwall's provider, store, attachments on disk, retention, Report-a-problem exclusion, prompt-cache measurement | 2–3 | 1, 2 |
| 4 | UI: Chat and Compare tabs, `<pd-assistant>` extended (attachments, mic, approval cards, phases, meter), layout at 900 to 1440, a11y, e2e | 4–6 | 2, 3 |
| 5 | Images: picker, paste, drop, decode, downscale, gating, fixtures; real-model test | 2–3 | 3 |
| 6 | Speech spike: the Decision 5 comparison on real hardware; capability test against the catalog; results in `performance.md`; **the user sees the result** | **1–2** | |
| 7 | Speech: capture port and `cpal`, OS permissions and Repair, transcribe path, `Part::Audio`, projector, capability, catalog entry, first-use download, push-to-talk | 4–6.5 (+3–4 if the helper) | 1, 4, 6 |
| 8 | `paddock_mcp::client`: connection, cancel, loopback, stdio; discovery file; tests | **2.5–4** | |
| 9 | Host: Connected apps page, key dialog, tool chooser, approvals store and hash, MCP-to-tool adapter, limits, audit | 3–5 | 2, 8 |
| 10 | User-added stdio servers with consent (cuttable) | 1.5–2.5 | 8, 9 |
| 11 | Security tests and an independent security review of the host | 1.5–2.5 | 9 |
| 12 | Docs: user guide, features, roadmap, `features.toml` rows; images reviewed; real-engine and Dyno tests | 2–3 | all |
| | **Chat total** | **28–45** | |
| 13 | Dyno's listener and switch, with the shared discovery. Counted to Dyno, not here | 1–1.5 | decision D1 |

The rest of the release (shell landing, Report a problem, ADRs 0052, 0053, 0054, VM rounds) is in the 1.1 readiness
report, not here.

## Decisions for the user

Each has a recommended default; nothing is built until they are answered.

| # | Question | Recommended |
|---|---|---|
| D1 | Dyno has no MCP listener. Add it to Dyno 1.1.0 now, or correct Dyno's guide and add it in the next Dyno? | Add it to 1.1.0 (1 to 1.5 days): the tools are in scope and useless without it, and its guide already promises it |
| D2 | Action tools (`wake_device`, `keep_awake`): remember after the first yes, like read tools, or ask every call? | Ask every call. Your rule was made when Dyno's tools were thought read-only |
| D3 | Chats saved by default? | Yes, on this computer, with "Don't save this chat" and a default switch |
| D4 | Speech engine: llama.cpp's own speech models (805 MB, no new engine), or whisper.cpp or sherpa-onnx? | llama.cpp first; a 1 to 2 day test on real hardware decides, and you see the numbers |
| D5 | Speech in 1.1: push-to-talk that fills the composer, plus audio clips for models that hear; no live transcript, no spoken replies? | Yes |
| D6 | Models without tool support: hide tools, or try a prompted fallback? | Disable tools with an explanation |
| D7 | Third-party MCP servers in 1.1? | Family apps first. User-added stdio last and cuttable. Nothing remote |
| D8 | Who may use MCP through Pitwall: only the Playground chat, or other apps' API requests too? | Only the chat in 1.1 |

## Consequences

- Pitwall keeps its promise: no new outbound connection and no new engine. Its only new trust is a key the user pasted.
- `paddock-assistant` becomes a real engine for the first time, proven by a production app before Pacenotes and Debrief
  use it.
- A microphone permission enters the family, with a likely re-prompt per update on ad-hoc macOS builds until signing
  arrives.
- Saved chats are a new store of private text on disk, kept out of bundles and logs by test.
- An 805 MB model for speech is heavy next to Whisper tiny or base. WP-6 may change the default if small and fast
  matters more than languages.
- Large scope: this is more work than everything else left in 1.1 together.

## Sources (read 2026-11-05)

- llama.cpp `b11669`: [multimodal.md](https://raw.githubusercontent.com/ggml-org/llama.cpp/b11669/docs/multimodal.md) (audio models), [server README](https://raw.githubusercontent.com/ggml-org/llama.cpp/b11669/tools/server/README.md) (`input_audio`; `/v1/realtime` and `/v1/audio/transcriptions` not found at this tag). [Server README on master](https://raw.githubusercontent.com/ggml-org/llama.cpp/master/tools/server/README.md): `/v1/realtime` for streaming-encoder models; multimodal "currently an experimental feature"; `--tools` built-ins.
- [Qwen3-ASR 0.6B GGUF](https://huggingface.co/ggml-org/Qwen3-ASR-0.6B-GGUF) (sizes; whether a projector is separate is not stated) and [Qwen's card](https://huggingface.co/Qwen/Qwen3-ASR-0.6B) (licence, languages, WER; streaming only with vLLM).
- [whisper.cpp README](https://github.com/ggml-org/whisper.cpp) (MIT, model table, backends, `whisper-server`), [releases](https://github.com/ggml-org/whisper.cpp/releases) (v1.9.5: per-OS assets not listed on the page read).
- [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) (Apache-2.0, models, platforms), [Rust crate](https://docs.rs/sherpa-onnx). [Moonshine](https://github.com/moonshine-ai/moonshine). [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3). [cpal](https://docs.rs/cpal/latest/cpal/) 0.18.2.
- MCP: [2026-07-28 changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog), [tools, 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/server/tools) (human in the loop, annotations untrusted, timeouts, audit), [security best practices](https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices) (local server compromise).
- [OWASP LLM01](https://genai.owasp.org/llmrisk/llm01-prompt-injection/) (no fool-proof prevention; least privilege, human approval, segregate untrusted content).
- Tauri [#2600](https://github.com/tauri-apps/tauri/issues/2600) (per-OS microphone reports, WebKitGTK), [wry#1195](https://github.com/tauri-apps/wry/issues/1195).
- Not verified at the source: Apple's audio-input entitlement page returned only its title, and Microsoft's WebView2
  permissions page returned 404. Both rows in Decision 5 are to be proved on the machines.

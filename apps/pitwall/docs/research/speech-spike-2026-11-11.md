# Speech input spike (Pitwall 1.1 chat, WP-6), 2026-11-11

The WP-6 spike of [ADR 0023](../adr/0023-playground-chat-vision-speech-mcp.md) Decision 5 and the
[plan](../plans/pitwall-1-1.md) (WP-6, WP-7). The user's D4 and D5 stand: llama.cpp first, decided by this test;
push-to-talk that fills the composer plus clips; no live transcript, no text to speech. Every number below is recorded
in [performance.md](../performance.md#2026-11-11-speech-to-text-llamacpp--qwen3-asr-against-whispercpp-docspitwall-speech-spike-wp-6)
(rule 48), which this note quotes.

**Result: option A works.** llama.cpp `b11669` (the engine Pitwall already installs) with `ggml-org/Qwen3-ASR-0.6B-GGUF`
transcribes a 25 s clip in 0.78 s on this Mac's GPU and in 2.5 s on 4 CPU threads, with the accuracy of Whisper small
and better than Whisper base outside English. It needs no new engine, binary or delivery path. Two figures in the ADR
change: the download is **1.02 GB, not 805 MB** (the audio projector is a second file of 214 MB), and the server takes
**1.1–1.25 GB** of memory. Windows was not measured.

Evidence heads read: `origin/main` `896171397` (ADR 0023 last changed in `34b60ca1f`; the plan and `performance.md` at
that head); `origin/docs/pitwall-1-1-chat` `aee241b62` is an older revision of the ADR and was not used. The engine pin
is shipped: `pitwall-v1.0.3` (`dce486621`) has `"tag": "b11669"` in
`crates/pitwall-core/src/runtimes/llama-runtimes.json` (read at the tag).

**Decided by the user on 2026-11-11, after this spike** (the answers to the first revision's questions are under
[Decisions](#decisions-asked-in-the-first-revision-answered-2026-11-11)):

- **Model and engine: Whisper small Q8_0** (`ggml-small-q8_0.bin`, 264 MB, about 0.55 GB of memory) **run by a second
  engine, whisper.cpp**, not Qwen3-ASR on llama.cpp. This reverses [ADR 0020](../adr/0020-llama-cpp-is-the-only-engine.md)'s
  single-engine stance for speech and changes ADR 0023 Decision 5 (section 7.9 says what to amend).
- **Controls:** a click toggle (click or shortcut starts and stops), Esc discards, the text goes to the composer, clips are
  capped at 5 minutes.
- **Language:** English only.
- **Windows and Linux CPU speed measured first** (0.5 day), before WP-7 is built: [section 5](#5-windows-and-linux-cpu-run-whisper-small-q8_0-whispercpp-v195).

**Second-revision result (sections 5 to 8).** Whisper small Q8_0 with the official whisper.cpp `v1.9.5` binaries is fast
enough on the measured CPUs: on the Windows 11 ARM64 test VM (4 vCPUs) clips of 8.7 s or longer come back with a median
real-time factor of 0.11–0.27 (one median at 0.74 during a host load peak), the 300 s English clip in 33 s (RTF 0.11) on
the quieter run; only the 4.7 s clip is above 0.3 (1.5–2.9 s), because Whisper always encodes a 30 s window. Sizing
`audio_ctx` to the clip removes that floor (0.4–1.9 s for clips up to 13.5 s, same words). Linux (a 4-CPU container, official arm64 build) is a little slower,
RTF 0.17–0.52, 0.12 on 300 s. **x64 CPUs are not measured**: both runs used the Mac's M4 Pro cores. The run found
one blocker that the spike could not see: **the official Windows builds of whisper.cpp, and of llama.cpp `b11669` that
Pitwall installs today, do not start on a clean Windows 11** (`0xC0000135`, no VC++ runtime); section 6. The WP-7 estimate
for the whisper.cpp sidecar is **9.5–13.5 agent-days** if Pitwall builds and ships whisper-server itself (recommended),
or 10.5–15 if it downloads the official release like llama.cpp (section 7), against 6–8 for option A.

Evidence heads read for the second revision (ADR 0091): `origin/main` `896171397`, `origin/docs/pitwall-speech-spike`
`dab3f501a` (this note's first revision); re-read before the commit. All Pitwall code cited below is at `896171397`.

## 1. Can Pitwall's engine transcribe?

**How the engine is bundled.** Pitwall does not ship llama.cpp: it downloads the official release asset for the
platform and checks its SHA-256 against `crates/pitwall-core/src/runtimes/llama-runtimes.json` (tag `b11669`, commit
`620d2225c`, published 2026-10-15). On macOS arm64 that is `llama-b11669-bin-macos-arm64.tar.gz`
(`dbed55493d72…`), which this spike downloaded and checked the same way. `llama-server --version` prints
`build 11669, commit 620d2225c`.

**Whisper through llama.cpp: no.** llama.cpp's audio support (libmtmd) runs audio-in LLMs, not Whisper's
encoder-decoder. At `b11669`, `docs/multimodal.md` lists Ultravox 0.5, Qwen2-Audio, Voxtral Mini 3B, Qwen3-ASR 0.6B and
1.7B, Qwen3-Omni and Gemma 4 E2B/E4B as audio models. Whisper needs whisper.cpp, a sibling project (MIT), measured here
as the reference (option B). Qwen3-ASR support entered llama.cpp in `21a4933042` (2026-04-12, PR #19441), long before
`b11669`; the exact first release number for `capabilities.json` (ADR 0018) is WP-7's to check.

**What Pitwall's code would refuse today.** The Qwen3-ASR projector has `clip.has_audio_encoder` and no vision encoder,
so `parse_projector` (`crates/pitwall-core/src/gguf.rs:487`) refuses it ("this projector has no image encoder"), as
the ADR expects. `capabilities.json` has no `audio` entry yet, and `/v1/audio/transcriptions` is listed in
`crates/pitwall-core/src/api/mod.rs:19` with `Capability::Transcription` already in `select.rs:27`.

### Measurements

Setup in the [results block](../performance.md#2026-11-11-speech-to-text-llamacpp--qwen3-asr-against-whispercpp-docspitwall-speech-spike-wp-6):
Mac mini M4 Pro (14 cores, 64 GB, macOS 27.2), shared with building agents (load 7–42); 9 synthetic clips made with
`say`; 3 requests per clip, median; footprint is `footprint -p` of the server.

Models, as downloaded (SHA-256 checked against Hugging Face):

| Model | Files | Size |
|---|---|---|
| Qwen3-ASR 0.6B Q8_0 (`ggml-org/Qwen3-ASR-0.6B-GGUF`) | `Qwen3-ASR-0.6B-Q8_0.gguf` + `mmproj-Qwen3-ASR-0.6B-Q8_0.gguf` | 804.7 + 214.4 MB = 1,019 MB |
| Whisper tiny / base / small (`ggerganov/whisper.cpp`) | `ggml-tiny.bin`, `ggml-base.bin`, `ggml-small.bin` | 77.7 / 148.0 / 487.6 MB |
| Whisper small Q8_0, large-v3-turbo Q5_0 | `ggml-small-q8_0.bin`, `ggml-large-v3-turbo-q5_0.bin` | 264.5 / 574.0 MB |

Per run (latency in seconds for the 4.7 s, 24.7 s and 38.2 s clips; real-time factor over all 9 clips; WER in percent;
footprint in MB; load at the start of the run):

| Run | 4.7 s | 24.7 s | 38.2 s | RTF | WER en (7 clips) | WER de | WER fr | Footprint | Load |
|---|---|---|---|---|---|---|---|---|---|
| Qwen3-ASR 0.6B, Metal, `-c 8192` | 0.22 | 0.78 | 1.22 | 0.028–0.047 | 0.3 | 0.0 | 0.0 | 1,548 | 42 |
| Qwen3-ASR 0.6B, Metal, `-c 4096` | 0.18 | 0.87 | 1.09 | 0.028–0.042 | 0.3 | 0.0 | 0.0 | 1,105 | 12 |
| Whisper tiny, Metal | 0.12 | 0.25 | 0.27 | 0.007–0.027 | 2.1 | 15.8 | 18.8 | 322 | 30 |
| Whisper base, Metal | 0.15 | 0.39 | 0.73 | 0.016–0.033 | 1.2 | 10.5 | 12.5 | 405 | 28 |
| Whisper small, Metal | 0.24 | 0.48 | 0.74 | 0.019–0.052 | 0.3 | 0.0 | 0.0 | 802 | 30 |
| Whisper small Q8_0, Metal | 0.24 | 0.45 | 0.67 | 0.018–0.051 | 0.3 | 0.0 | 0.0 | 551 | 19 |
| Whisper large-v3-turbo Q5_0, Metal | 0.96 | 1.11 | 1.69 | 0.044–0.206 | 0.0 | 0.0 | 0.0 | 851 | 13 |
| Qwen3-ASR 0.6B, CPU 4 threads (`--device none`) | 0.44 | 2.51 | 3.51 | 0.080–0.102 | 0.3 | 0.0 | 0.0 | 2,232 | 13 |
| Whisper base, CPU 4 threads (`-ng`) | 0.32 | 0.73 | 1.18 | 0.030–0.089 | 1.2 | 10.5 | 12.5 | 443 | 13 |
| Whisper small, CPU 4 threads | 1.98 | 1.83 | 2.62 | 0.068–0.424 | 0.3 | 0.0 | 0.0 | 893 | 14 |
| Whisper small Q8_0, CPU 4 threads | 1.96 | 2.45 | 3.79 | 0.099–0.420 | 0.3 | 0.0 | 0.0 | 665 | 28 |

The 300 s clip (five rounds of three English and one German clip, then one English): Qwen3-ASR on Metal 9.1 s (2.1 s
reading 3,923 audio tokens, 6.7 s writing 1,056 text tokens), WER 0.7%, footprint 1,247 MB at `-c 6144`; on 4 CPU
threads 74.7 s. Whisper small on Metal 4.45 s but WER 11.8%: with `-l auto` it detected English once and dropped all
five German passages.

What the numbers say:

- **Speed is not the question on a Mac.** Every model is 20–100 times faster than real time on Metal. A push-to-talk
  clip of up to 30 s is back in about 1 s with Qwen3-ASR.
- **Accuracy:** Qwen3-ASR 0.6B equals Whisper small here (its one English error is "to rest days" for "two rest days",
  which Whisper tiny, base and small made too; only large-v3-turbo got it right) and handles language changes inside a clip. Whisper base and tiny fail German
  and French words. Caveat: synthetic `say` speech is easy; one clip per language is not a statistic, and real voices
  and microphones will score worse for every model.
- **Cost of A:** about 1 GB more download and 0.3–0.7 GB more memory than Whisper small or base. The KV cache drives
  the footprint: Qwen3-ASR uses about 13 audio tokens per second (3,923 for 300 s), so the 5-minute clip cap needs
  `-c 6144` (1,247 MB); a 2-minute cap would fit `-c 4096` (1,105 MB).
- **CPU only:** Qwen3-ASR's time grows with clip length (RTF about 0.1, and 0.25 on 300 s because text generation
  dominates); Whisper small has a floor of about 1.5–2 s on any clip because it always encodes a 30 s window. Both are
  usable for push-to-talk on these 4 M4 cores. A typical x64 laptop CPU is slower (not measured).
- **Start:** with a warm file cache, the server is ready in 0.53–0.56 s (Qwen3-ASR) and 0.12–0.23 s (Whisper). First
  starts on this loaded host took 1.4–11.6 s and, for the self-built whisper.cpp, 20–33 s while Metal libraries
  compiled once. A cold boot was not measured (purging the file cache needs root).
- **Formats:** llama-server resamples and downmixes itself: a 48 kHz stereo WAV gave the same text in 0.40 s against
  0.32 s for 16 kHz mono. Pitwall should still send 16 kHz mono, because 5 minutes at 48 kHz mono (28.8 MB, 38 MB as
  base64) is over the 32 MiB default request limit (`max_request_bytes`, `crates/pitwall-core/src/config.rs:86`),
  while 16 kHz mono (9.6 MB) fits.
- **Things WP-7 must handle:** the reply starts with `language English<asr_text>` (parse off the prefix; the language
  comes free); `max_tokens` must grow with the clip (1,024 cut the 300 s transcript short; 2,048 was enough, about
  3.5 text tokens per second of speech); llama.cpp logs "audio input is in experimental stage and may have reduced
  quality" at load (`init_audio`, linking discussion #13759).

**Not measured:** Windows (CPU, Vulkan, CUDA builds of `b11669`), Linux, any Intel or AMD CPU, a real voice and a real
microphone (ER-14: the spike opened no audio device), Qwen3-ASR 1.7B and the BF16 files, and option C (sherpa-onnx
with Moonshine): the brief allows model downloads only from `ggerganov/whisper.cpp` and the llama.cpp org, and A
passed, so C stays the fallback the ADR names.

## 2. Microphone capture in the app

Two routes. Read in the locked dependency sources (Pitwall's `Cargo.lock`: `tauri` 2.12.5, `tauri-runtime-wry` 2.12.3,
`tauri-utils` 2.10.2, `wry` 0.57.0); no route was run, because running one opens the user's microphone.

| | Webview: `getUserMedia` | Native: `cpal` in the app process (ADR 0023's choice) |
|---|---|---|
| Permission plumbing | `WebviewWindowBuilder::on_permission_request` (`tauri-2.12.5/src/webview/webview_window.rs:475`) → wry's `requestMediaCapturePermissionForOrigin` (`wry-0.57.0/src/wkwebview/class/wry_web_view_ui_delegate.rs:130`), WebView2's `PermissionRequested`, WebKitGTK's `permission-request` (`wry-0.57.0/src/lib.rs:811-815`) | none in the webview; one guarded app command to start and stop |
| Audio format | WebKit's `MediaRecorder` gives AAC in MP4, which llama-server's miniaudio does not read (mp3, wav, flac per the server README); so PCM through an `AudioWorklet` and a WAV encoder in TypeScript | PCM straight from the device; downmix and resample to 16 kHz in Rust |
| CSP | `getUserMedia` is not a CSP fetch; a worklet module from `'self'` is allowed by `default-src 'self'` (`tauri.conf.json:30`) | no change |
| Linux | WebKitGTK media streams are off by default (ADR 0023, Tauri #2600) | ALSA; `libasound2-dev` to build, the `.deb` dependency |
| Tests | needs a fake `MediaStream` in the browser | a capture port with a fake, as the ADR says (rule 7) |
| Code | less Rust, but audio handling in the UI, which AGENTS.md puts in the core | `cpal` 0.18.2 (Apache-2.0, 2026-08-16, Rust 1.85; CoreAudio, WASAPI, ALSA), an adapter of roughly 150–250 lines (estimate) |

**The least code that works on all three systems is `cpal`.** The webview route saves Rust on macOS and Windows but
needs TypeScript audio code and fails on Linux. Keep the ADR's choice.

**macOS permission, either route.** The capture must run in `Pitwall.app` itself, not in the LaunchAgent service
(`app/src-tauri/macos/net.decryptology.pitwall.service.plist`): TCC asks for the process that opens the device, and the
app is the one with a window to explain it (judgement). The app has no `Info.plist` of its own today; Tauri merges a
file named `Info.plist` next to `tauri.conf.json` (or `bundle.macOS.infoPlist`) into the bundle's
(`tauri-utils-2.10.2/src/config.rs:665-669`), so WP-7 adds one with `NSMicrophoneUsageDescription`. The bundle is
ad-hoc signed with the hardened runtime off (`tauri.macos.conf.json`: `"signingIdentity": "-"`,
`"hardenedRuntime": false`), so no audio-input entitlement is needed yet. The ADR's expected re-prompt after each
ad-hoc update was not checked here.

## 3. Common practice (rule 76)

| Product | Start and stop | Where the text goes | Engine | Source |
|---|---|---|---|---|
| macOS Dictation | the Dictation shortcut (for example Fn twice) or the Microphone key starts; the same key or Esc stops; stops by itself after 30 s of silence; no press-and-hold (holding the key starts Siri) | at the insertion point, editable | on-device or Apple's servers (Keyboard settings) | [Apple](https://support.apple.com/guide/mac-help/use-dictation-mh40584/mac) |
| Windows voice typing | Win+H starts; the microphone button or "stop listening" stops | the focused text box | online (Azure Speech); on-device clean-up on Copilot+ PCs | [Microsoft](https://support.microsoft.com/en-us/windows/use-voice-typing-to-talk-instead-of-type-on-your-pc-fec94565-c4bd-329d-e59a-af033fa5689f) |
| Open WebUI (Ollama front end) | click the microphone, a waveform shows; tick to finish, x to discard; auto-send optional | the prompt, sent by the user unless auto-send is on | local Whisper by default (tiny to large), browser speech API, or a server | [Open WebUI STT](https://docs.openwebui.com/features/chat-conversations/audio/speech-to-text/stt-config) |
| LM Studio (Bionic) | not stated on the page | "sends the transcript into Bionic" | local, Voxtral only; "Auto load voice model" keeps it loaded, else the first use waits for the load | [LM Studio](https://lmstudio.ai/docs/bionic/voice-input) |
| ChatGPT desktop | a "Dictate" microphone in the composer | the composer, editable before sending | OpenAI's service | secondary only (OpenAI's help page returned 403): [getvoibe](https://www.getvoibe.com/resources/dictate-in-chatgpt/) (unverified) |

**Common practice is a click toggle, not press-and-hold:** click or a shortcut starts, the same stops, Esc or a
discard button throws the clip away, the text lands in the composer for editing, nothing is sent by voice. That is the
simplest option and matches D5; hold-to-talk is extra. The family's own idle rules already cover LM Studio's "keep the
model loaded" switch.

## 4. Recommended WP-7 design (first revision, option A; superseded by section 7)

Kept for the record: the user chose Whisper small Q8_0 on whisper.cpp instead (top of this note).

1. **Engine: A.** `Qwen3-ASR-0.6B` Q8_0 plus its Q8_0 projector as one catalog entry (1,019 MB, both SHA-256 pinned),
   run by Pitwall's existing llama-server supervision as its own model with `-c 6144 -np 1` (the 5-minute cap), under
   the existing fit check and idle rules. One request per clip to `/v1/chat/completions` with an `input_audio` part
   (base64 WAV, never a URL; `--media-path` never set), `temperature 0`, `max_tokens` from the clip length
   (about 4 per second plus a margin, at most 2,048). Parse off `language …<asr_text>`.
2. **Core and server.** `Part::Audio`; `parse_projector` accepts an audio-only projector for a transcription model;
   an `audio` capability with its floor checked against llama.cpp's history (ADR 0018); one transcribe path in the core
   behind `/mgmt/v1/transcribe` and `/v1/audio/transcriptions` (replacing the 501). A real-engine test in
   `features.toml` with a short synthetic clip; unit tests with the fake engine for the prefix, `max_tokens` and limits.
3. **Capture.** A `Capture` port in the app; a `cpal` adapter that takes the device's default input, downmixes,
   resamples to 16 kHz mono PCM16 and stops at 5 minutes; a fake for tests. Audio stays in memory and is dropped after
   transcription unless attached as a clip; logs get only duration, size, engine time and outcome.
4. **Permissions.** `Info.plist` with `NSMicrophoneUsageDescription`; a denied or missing grant gives a clear message
   and Repair opens the Privacy pane (macOS) or Settings > Privacy > Microphone (Windows); Linux reports a missing ALSA
   device.
5. **UI.** A microphone button and shortcut that toggle recording, a recording state with elapsed time always on
   screen, Esc discards, a "Transcribing" state, the text into the composer. First use offers the model with size and
   fit check and downloads it as a job with progress and Cancel. Clips for models that hear use the same `Part::Audio`.

**Estimate: 6–8 agent-days** (inside the plan's 6–9; no helper, so not the +3–4): core and server 2–2.5; capture port
and `cpal` on three systems 1.5–2; permissions and Repair 1–1.5; UI and first-use download 1.5–2. Before it starts,
**0.5 day** to repeat this harness on Windows (CPU and Vulkan builds of `b11669`) and on a Linux VM, since A's only
open risk is a slow CPU.

## Decisions asked in the first revision (answered 2026-11-11)

1. **Accept A** (Qwen3-ASR 0.6B, 1.02 GB download, 1.1–1.25 GB memory, no new engine) as Pitwall 1.1's speech model?
   The cheaper alternative is Whisper small Q8_0 (264 MB, 0.55 GB, same accuracy here) through whisper.cpp, a second
   engine that reverses ADR 0020. Recommended: A. **Answered: Whisper small Q8_0 on whisper.cpp.**
2. **Toggle instead of hold:** click or shortcut to start and stop, Esc to discard, as macOS Dictation, Windows voice
   typing and Open WebUI do? Recommended: toggle only in 1.1. **Answered: toggle; Esc discards; text to the composer.**
3. **Clip cap:** keep 5 minutes (`-c 6144`, about 1.25 GB) or cut to 2 minutes (`-c 4096`, about 1.1 GB)?
   Recommended: 5 minutes. **Answered: 5 minutes** (with Whisper the cap no longer changes the memory: section 5).
4. **Windows and Linux first:** run the 0.5-day measurement before WP-7 starts, with sherpa-onnx (option C) as the
   fallback only if A's real-time factor on a Windows CPU is worse than about 0.3? Recommended: yes. **Answered: yes,
   for Whisper small Q8_0** (section 5).
5. **Languages:** this spike tried English, German and French. Which languages does the user dictate in, so the
   Windows run and the WP-7 test clip use them? **Answered: English only.**

## 5. Windows and Linux CPU run (Whisper small Q8_0, whisper.cpp `v1.9.5`)

**What ran.** whisper.cpp `v1.9.5` is commit `d1be6fde1`; its tag has no assets, and the `b5454` release on the same
commit (published 2026-10-06) carries the official binaries (both tags read on GitHub's release pages, 2026-11-11). The
run used those binaries only, each checked against the SHA-256 GitHub lists for it: `whisper-bin-win-cpu-arm64.zip`
(`28c37e7b598c…`) and `whisper-bin-ubuntu-arm64.tar.gz` (`6b95ebfc6044…`). Model: `ggml-small-q8_0.bin` (`49c8fb02b65e…`,
the spike's file; its hash was re-checked inside the VM). Server: `whisper-server -m ggml-small-q8_0.bin -l en -t 4`,
requests to `/inference` with `temperature=0`, the spike's procedure: 3 requests per clip, median; the 300 s clips once.
Clips: the spike's seven English clips (4.7–38.2 s, two with pink noise), its 300 s English-and-German clip `long300`,
and a new 300 s English-only clip `en300` (five rounds of en30, en20, en12, en05, then en30 and en20, padded with 3.8 s of
silence; reference text the same concatenation). WER uses the spike's normaliser.

- **Windows:** the Paddock test VM (`scripts/vm-win.sh`, VMware Fusion), reverted to `clean-baseline`, held through
  `scripts/vm-lock-run.sh`, stopped and reverted again after every attempt. Windows 11 ARM64 build 26300.9457 (the
  registry's product name still says "Windows 10 Pro"), 4 vCPUs of the M4 Pro, 8 GB. The guest harness is PowerShell 5.1
  with `curl.exe` (the same requests; working set and peak working set from `Get-Process`). The build reports `NEON = 1 |
  ARM_FMA = 1 | MATMUL_INT8 = 1 | DOTPROD = 1 | OPENMP = 1`.
- **Linux:** `docker run --rm --cpus 4 --memory 12g train5-linux-dev:2` (Debian 13, glibc 2.41, aarch64), the spike's
  `bench.py` with `/proc` memory (`VmRSS`, `VmHWM`) instead of `footprint`. The official arm64 build reports only
  `NEON = 1 | ARM_FMA = 1 | OPENMP = 1` (no `DOTPROD`, no `MATMUL_INT8`): a portable build, which explains part of the gap
  to Windows on the same cores.
- **Not measured:** any x64 CPU (both systems ran on the Mac's ARM cores; an Intel or AMD laptop is slower per core,
  by how much is unknown here), GPU on Windows (the VM's only adapter is "VMware SVGA 3D"; `vulkan-1.dll` exists, but
  whisper.cpp publishes no Vulkan build for Windows or Linux, only CPU, BLAS, CUDA and an Adreno OpenCL build, so there
  was nothing official to run), a cold boot, a real voice or microphone.
- **Host load:** the Mac was shared with building agents. Linux ran at a 1-minute load of 7–13. Windows run 1
  measured at 14–45, run 2 (20 minutes later, the same steps from a fresh revert) at 5–20; three earlier attempts
  failed (section 6 and the appendix). In each Windows run the servers ran in the order short clips, 300 s clips, sized.

Numbers (latency in seconds, median of 3, min–max in brackets where the spread matters; RTF = median / audio length):

| Clip (audio) | Windows ARM64, run 1 | Windows ARM64, run 2 | Linux arm64 container | Windows, `audio_ctx` sized (run 1 / run 2) | Linux, `audio_ctx` sized |
|---|---|---|---|---|---|
| en05 (4.7 s) | 2.92 (1.52–3.03), RTF 0.63 | 1.91, RTF 0.41 | 2.45, RTF 0.52 | 0.70 / 0.43, RTF 0.15 / 0.09 | 0.56, RTF 0.12 |
| en12 (8.7 s) | 1.86, RTF 0.21 | 1.91, RTF 0.22 | 3.10, RTF 0.36 | 0.97 / 0.79, RTF 0.11 / 0.09 | 0.96, RTF 0.11 |
| en20 (13.5 s) | 2.43 (2.42–10.8), RTF 0.18 | 2.03 (1.84–12.6), RTF 0.15 | 3.35, RTF 0.25 | 1.81 / 1.05, RTF 0.13 / 0.08 | 2.53, RTF 0.19 |
| en30 (24.7 s) | 3.21, RTF 0.13 | 2.64, RTF 0.11 | 4.32, RTF 0.17 | 10.0 (3.30–52.7) / 2.71, RTF 0.41 / 0.11 | 3.28, RTF 0.13 |
| en45 (38.2 s) | 6.05 (5.18–27.3), RTF 0.16 | 4.92 (4.77–29.4), RTF 0.13 | 10.06, RTF 0.26 | - | - |
| en12n (8.7 s, noise) | 2.39 (2.05–39.3), RTF 0.27 | 6.44 (2.97–6.51), RTF 0.74 | 2.81, RTF 0.32 | 1.68 / 0.77, RTF 0.19 / 0.09 | 0.96, RTF 0.11 |
| en20n (13.5 s, noise) | 2.78 (2.25–9.81), RTF 0.21 | 2.32 (2.22–5.34), RTF 0.17 | 2.83, RTF 0.21 | 1.91 / 1.17, RTF 0.14 / 0.09 | 1.51, RTF 0.11 |
| en300 (300 s, English) | 104.7, RTF 0.35 | 33.4, RTF 0.11 | 36.0, RTF 0.12 | - | - |
| long300 (300 s, with German) | 45.5, RTF 0.15 | 48.5, RTF 0.16 | 50.6, RTF 0.17 | - | - |
| WER, 7 English clips (mean) | 0.3% | 0.3% | 0.4% | 0.2% (6 clips, both runs) | 0.2% (6 clips) |
| WER, en300 / long300 | 0.7% / 11.8% | 0.7% / 11.8% | 0.7% / 11.8% | - | - |
| Memory | working set 352 MB idle, peak 472 MB (short clips), 575 MB (300 s); private bytes 928–933 MB | peak 472 MB, 587 MB (300 s); private bytes 928–933 MB | RSS 350 MB idle, peak 500 MB (short), 573 MB (300 s) | peak 423 MB | peak 447 MB |
| Start to `/health` | 2.52 s (first server after boot), then 2.28 and 4.72 s | **50.9 s** (first server after boot), then 5.30 and 3.29 s | 1.53 s first; 0.16–0.18 s later | | |

What the numbers say:

- **Windows is fast enough on these cores.** For every clip of 8.7 s or longer the median RTF is 0.11–0.27 in both runs,
  inside the spike's 0.3, except one noisy-clip median of run 2 (6.4 s, whose fastest request took 3.0 s) during a load
  peak. The 300 s English clip took 33.4 s in run 2 (RTF 0.11) and 104.7 s in run 1, which overlapped a load peak; the
  300 s mixed clip took 45–49 s in both. The one clip above 0.3 in both runs is the shortest: Whisper encodes a 30 s window
  whatever the clip's length, so a 4.7 s clip costs about as much as a 13 s one (1.5–2.9 s). For push-to-talk the
  measure that matters is the wait after the click: **about 2–3 s for a typical dictation, 5–6 s for 38 s of speech**.
- **`audio_ctx` sized to the clip removes the floor.** whisper-server takes `audio_ctx` per request
  (`examples/server/server.cpp:510` at `v1.9.5`); 50 encoder frames per second of audio plus 64 cut the clips up to 13.5 s to
  0.4–1.9 s (0.4–1.2 s in run 2) with the same words (WER 0.2% on 6 clips, the en30 error is the spike's "to rest days"). Whisper was trained
  on full 30 s windows and a shortened context is known to cost accuracy on real speech (judgement, not measured here);
  WP-7 therefore keeps it behind a test on real recordings, see decision W4.
- **Linux is slower than Windows on the same cores** (RTF 0.17–0.52, 300 s in 36 s): its official build lacks the
  `DOTPROD` and `MATMUL_INT8` kernels the Windows arm64 build has. On x64 both official builds ship every CPU variant
  (`ggml-cpu-haswell`, `-skylakex`, `-alderlake`, `-zen4` and others) and pick one at run time.
- **English only works as decided:** with `-l en` the English text is right (0.7% on 300 s) and the German passages of
  `long300` come out as English paraphrase or are dropped (WER 11.8%, as the spike's `-l auto` run). Nothing else in
  the UI should promise other languages.
- **Memory:** 0.35 GB loaded, at most 0.59 GB resident during a 300 s clip on both systems; the 5-minute cap does not
  change it. Windows commits 0.93 GB (private bytes) of which about half is resident: the fit check should count the
  commit on Windows.
- **Start:** 0.2–5.3 s to `/health` once the files are cached; but the first server after a fresh boot took 2.5 s in run 1
  and **50.9 s in run 2** (cause not measured: Defender's `MpCmdRun` was running in the fresh VM during the attempts, and
  the host load rose meanwhile; both are plausible). WP-7 therefore starts the server at the click, shows "Starting the
  speech engine" with elapsed time while recording, and never times out a start the user waits for (ER-19); the clip is
  kept until the server is ready.
- **Outliers on Windows** (a 52.7 s request for a 24.7 s clip, 39.3 s for 8.7 s, all in run 1) coincide with host load
  peaks of 34–45 on the shared Mac; they are the VM waiting for CPU (judgement: the same requests were 2.7 s and 2.4 s in
  run 2). Medians are robust to one of three; the 300 s
  rows are single requests and less so.

## 6. Finding: the official Windows builds need the VC++ runtime (a blocker for both engines)

On the `clean-baseline` VM, before anything was installed, `vcruntime140.dll` and `msvcp140.dll` are absent from
`System32`, and both official builds exit at once with `0xC0000135` (`STATUS_DLL_NOT_FOUND`):

```text
whisper-server --help exit: 0xC0000135                 (whisper-bin-win-cpu-arm64.zip, b5454 = v1.9.5)
llama-server --version (b11669) exit: 0xC0000135       (llama-b11669-bin-win-cpu-arm64.zip, the asset llama-runtimes.json pins)
```

The import tables say why (`llvm-objdump -p`): `whisper-server.exe`, `whisper.dll`, `ggml*.dll` import `MSVCP140.dll` and
`VCRUNTIME140.dll`; the x64 `whisper-bin-x64.zip` also imports `VCRUNTIME140_1.dll` and `VCOMP140.DLL`; llama.cpp's
`llama-server.exe`, `llama-server-impl.dll`, `ggml-base.dll` and `ggml-cpu.dll` (arm64) import the same two. Neither
archive ships them. After Microsoft's signed `vc_redist.arm64.exe` (Authenticode valid, SHA-256 `5139e1440c3a…` as in its
download URL) was installed, whisper-server ran and every number in section 5 comes from that state. The first Windows
attempt, without the runtime, failed with "server exited" for all three runs, which is how this was found.

Consequences:

- **For WP-7** whisper.cpp must not depend on the redistributable: either Pitwall builds whisper-server itself with the
  static CRT (`-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded`), which ADR 0032's rule asks of every Windows binary Paddock
  ships and `conformance/release/windows-crt-imports.mjs` checks on installer files, or it installs the runtime DLLs next
  to the engine. Section 7 recommends the first.
- **For Pitwall 1.0.x (not WP-7's to fix):** the llama.cpp build Pitwall downloads on Windows very likely fails its
  self-test on a PC without the redistributable: `self_test` runs `llama-server --version`
  (`crates/pitwall-server/src/engines.rs:479-500`), which exits with `0xC0000135`, so the install is discarded with "it
  could not start" or "--version failed". The released `pitwall-v1.0.3` has the same pin (`b11669`, including
  `llama-b11669-bin-win-cpu-arm64.zip`) and the same `--version` self-test (`engines.rs:486` at the tag, read with
  `git show`). This is the same class as the Dyno sidecar lesson of 2026-10-30
  (`docs/lessons.md:886`, ADR 0032), reached through a downloaded binary that no installer check sees; only Debrief's
  workflow runs `windows-crt-imports.mjs` at `896171397` (`.gitea/workflows/debrief.yml:1006`). Verified here: the exit
  code of the official binary on a clean VM. Not verified: Pitwall's own install path on that VM. The fix should start
  with a failing test, for example an engines test where the self-test of a build exits with `0xC0000135` and the message
  must name the missing VC++ runtime and the next step, plus a clean-VM run of Pitwall's first engine install.

## 7. WP-7 design for Whisper small Q8_0 on a whisper.cpp sidecar (replaces section 4)

### 7.1 Where it runs

whisper-server runs as **its own child process of `pitwall-server`, like llama-server**: bound to `127.0.0.1` on a
port Pitwall picks, started at the first click (or when the composer opens, if the model is installed), ready when
`/health` answers 200, stopped after the same idle period as chat models, restarted once after a crash and then shown
as an error with Repair. It is never started with `--convert` (that runs `ffmpeg` through `system()`,
`server.cpp:285-321`) nor bound to another host. One request at a time through the existing queue
(`JobKind::Transcription`); a second click while transcribing is refused with a message. The core gets a small
`Transcriber` port (`transcribe(pcm16_16k_mono, deadline) -> Result<Transcript, TranscribeError>`) with the whisper-server
adapter in `pitwall-server` and a fake for tests; nothing C++ runs inside a Pitwall process (AGENTS.md's containment; ADR
0023 already rejected B2, `whisper-rs` in-process). Logs (ER-27): the exact command line, spawn outcome, time to ready,
exit code with the NTSTATUS on Windows, per request the audio length, bytes, engine time and outcome; never the text or
the audio.

### 7.2 How the engine is pinned, fetched and verified

Two ways; decision W1 chooses.

**(a) As llama.cpp is pinned: download the official release at first use.** A second catalog,
`crates/pitwall-core/src/runtimes/whisper-runtimes.json`, in the same shape as `llama-runtimes.json` (`version`, `release`
`v1.9.5`, `tag` `b5454`, `commit` `d1be6fde11ac6e0407606b4e42fe72d34add8037`, `published` `2026-10-06`, per platform a
list of builds with `name`, `url`, `size`, `sha256`). The official assets cover `windows-x64` (`whisper-bin-x64.zip`,
`6ba69e3482d7…`), `windows-arm64` (`whisper-bin-win-cpu-arm64.zip`, `28c37e7b598c…`), `linux-x64`
(`whisper-bin-ubuntu-x64.tar.gz`, `a72becf15d79…`) and `linux-arm64` (`whisper-bin-ubuntu-arm64.tar.gz`, `6b95ebfc6044…`);
all four digests were checked against the downloaded files. Reused unchanged: `Egress::download` with the SHA-256 checked
before unpacking (`engines.rs:401`), the staging folder and roll-back (`engines.rs:415-470`), `DownloadUrl`'s host list
(`github.com` and GitHub's release hosts are already allowed, `crates/pitwall-core/src/privacy.rs:22-24`). Changed:
`valid_asset` hard-codes llama.cpp's release URL prefix (`crates/pitwall-core/src/runtimes/catalog.rs:78-83`), so the
catalog takes its engine's prefix as data; the unpack keep-list (`runtimes::is_expected`, `runtimes/mod.rs:27-37`) gets a
whisper variant (`whisper-server[.exe]`, the `ggml*`/`whisper` libraries, `LICENSE`); the self-test runs
`whisper-server --help` (it has no `--version`; `server.cpp:206`) and then one 1 s synthetic clip with the model.
Against (a): **there is no official macOS binary** (`b5454` ships only `whisper-b5454-xcframework.zip`, a library), so
macOS needs our own build anyway; and **the Windows binaries need the VC++ runtime** (section 6), so Pitwall would also
fetch and place `vcruntime140*.dll`, `msvcp140.dll` and on x64 `vcomp140.dll` beside the engine, a second download that
no installer check covers.

**(b) Recommended: build whisper-server from the pinned commit in Pitwall's release workflow and ship it in the
installer as a second sidecar** (`tauri.conf.json` already bundles `binaries/pitwall-server` through `externalBin`,
`app/src-tauri/tauri.conf.json:37`). The pin is the commit `d1be6fde1` plus the SHA-256 of its source archive, checked
before the build; the build is `cmake -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DWHISPER_SDL2=OFF` with
`-DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON` on macOS (no first-start shader compile from a separate file; the
spike's self-built server spent 20–33 s compiling Metal libraries at its first start, which still needs checking with the
embedded library), `-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` on Windows, and `-DGGML_CPU_ALL_VARIANTS=ON
-DGGML_BACKEND_DL=ON` on x64 so a portable build still uses AVX2 or AVX-512 where present. The binary is then covered by
what every Paddock binary already gets: ad-hoc signing with the app on macOS, `windows-crt-imports.mjs` over the installer
files (to be wired into Pitwall's workflow, see prerequisites), and the installer's own checksum. One mechanism on all
three systems, no runtime download of an executable, no new host. Costs: about 1–3 minutes of C++ build per platform on
each release (not measured), cmake in the Linux CI image (`train5-linux-dev:2` has `g++` 12.2 but no `cmake`), and a
whisper.cpp update rides a Pitwall release. Handy, an open-source Tauri dictation app, does the same in spirit: it builds
its ggml backend per platform and architecture in its own CI and ships it in the app (it links it in-process through
`transcribe-cpp`; [Cargo.toml](https://github.com/cjpais/Handy/blob/main/src-tauri/Cargo.toml), read 2026-11-11).

### 7.3 The model

`ggml-small-q8_0.bin` from `huggingface.co/ggerganov/whisper.cpp` (an allowed host), pinned by name, size and SHA-256
(`49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f`). It is not GGUF, so it lives apart from the model
library (`gguf.rs` would refuse it) in a `speech/` folder, is not listed among chat models, and is downloaded at the
first click as a job with size, the fit check (0.6 GB resident, 0.95 GB committed on Windows), progress and Cancel.

### 7.4 Requests

The core sends 16 kHz mono PCM16 WAV as multipart to `/inference` with `language=en`, `temperature=0`,
`response_format=json` and, if decision W4 says so, `audio_ctx = min(1500, 50 × seconds + 64)`. A 5-minute clip is 9.6 MB,
inside `max_request_bytes` (32 MiB, `crates/pitwall-core/src/config.rs:86`). No timeout on a request the user waits
for (ER-19): "Transcribing" shows the elapsed time and Cancel; a request still running after 1× the audio length (three
times the slowest quiet-host median measured) is logged as slow, not cancelled. Pitwall's API gets the same path: `/mgmt/v1/transcribe`
for the app and `/v1/audio/transcriptions` (OpenAI's multipart shape) instead of today's 501
(`crates/pitwall-core/src/api/mod.rs:19`, `Capability::Transcription` in `select.rs:27`). `Part::Audio` and an audio-only
`parse_projector` are no longer needed for speech input; they stay with "models that hear", which ADR 0023 keeps for
llama.cpp and which is not part of WP-7 any more.

### 7.5 Capture

As section 2 and the ADR: a `Capture` port in the app with a `cpal` adapter (CoreAudio, WASAPI, ALSA) on the default
input device, downmixed and resampled to 16 kHz mono PCM16 in Rust, held in memory, stopped at 5 minutes with a notice,
and dropped after transcription. A fake capture for tests (rule 7); no test opens a device (ER-14).

### 7.6 The toggle

A microphone button in the composer and a shortcut toggle recording; while recording, a recording state with elapsed
time is always visible and Esc (or the discard button) throws the clip away; the second click stops and shows
"Transcribing" with Cancel; the text is inserted in the composer at the caret, never sent. While the server starts (up to 51 s
on a freshly booted VM, section 5) the recording already runs and a line says "Starting the speech engine". The first
click when the model is missing offers the download with its size. Status always in words (rule: status is never colour alone).

### 7.7 Permissions and Repair

macOS: an `Info.plist` with `NSMicrophoneUsageDescription` next to `tauri.conf.json`, capture in `Pitwall.app` (not the
LaunchAgent); a denied grant gives a message and Repair opens Privacy & Security > Microphone; a re-prompt after an
ad-hoc update is likely (ADR 0023) and is checked on the user's Mac. Windows: Settings > Privacy > Microphone with
"Let desktop apps access your microphone" off gives a message and a link (`ms-settings:privacy-microphone`). Linux: no
ALSA device gives a message. Repair also re-verifies the engine and the model against their SHA-256 and re-installs on a
mismatch, and reports the engine's last exit code (so a `0xC0000135` reads as "a runtime is missing", not "crashed").

### 7.8 Estimate

| Part | Agent-days |
|---|---|
| Engine build and pin: (b) the cmake build in the release workflow for 3 OS and 2 architectures, source hash, sidecar bundling, CRT check wired for Pitwall | 2–3 |
| Sidecar supervision: `Transcriber` port, whisper-server adapter, ready, idle stop, crash, logs, fake | 1.5–2 |
| Model: pin, `speech/` store, first-use download job with fit check | 0.5–1 |
| Core and server: transcribe path, `/mgmt/v1/transcribe`, `/v1/audio/transcriptions`, limits, timeout, real-engine test | 1–1.5 |
| Capture: `Capture` port, `cpal` on three systems, resampling, cap | 1.5–2 |
| Permissions and Repair on three systems | 1–1.5 |
| UI: toggle, shortcut, Esc, states, composer insertion, first-use offer | 1.5–2 |
| ADR 0020 and 0023 amendments, user guide | 0.5 |
| **Total** | **9.5–13.5** |

With (a) instead of (b): the catalog and download path, a separate macOS build anyway and the VC++ runtime files make the
first row 3–4.5, **10.5–15** in all. Both are above option A's 6–8 (no second engine) and above the plan's 6–9 for WP-7:
WP-7 sits on the critical path to the tag (plan, `pitwall-1-1.md:185`), so it moves by up to 4.5 days at the top of the
range. Common practice (rule 76) is mixed: Handy builds ggml per platform in its CI and ships it in the app (above);
Open WebUI runs local Whisper inside its own backend (section 3's source); LM Studio installs engine runtimes through its
own runtime manager after install (not checked here, unverified). The cheaper common option is (b): it is what the
family already does for every binary it ships.

**Prerequisites** (ADR 0096; none of these exists for Pitwall at `896171397`): `cmake` in the Linux CI image; the
`windows-crt-imports.mjs` step in Pitwall's release workflow (today only in `debrief.yml`); a C++ toolchain on the macOS
and Windows runners (Windows has Visual Studio's build tools per `docs/lessons.md:891`; macOS has Xcode, not verified for
cmake).

### 7.9 ADRs to amend (WP-7, not this note)

- **ADR 0020** ("llama.cpp is the only engine"): add that whisper.cpp is a second engine for speech to text only, with
  its own pin, behind its own port, not listed among chat engines and not selectable per model; its Consequences
  already say a new engine needs a new ADR or an amendment.
- **ADR 0023 Decision 5:** engine B instead of A, with this note's numbers; push-to-talk becomes the click toggle with Esc
  to discard; English only; `Part::Audio` stays for models that hear, not for dictation; the size (264 MB, 0.6 GB) replaces
  805 MB; Windows needs the static-CRT build (section 6); WP-7's estimate as in 7.8.

## 8. Decisions for the user (second revision)

- **W1. How whisper.cpp is delivered.** (b) Build it from the pinned commit in the release workflow and ship it in the
  installer (9.5–13.5 days; one mechanism, static CRT, no executable downloaded at run time). (a) Download the official
  release at first use like llama.cpp (10.5–15 days; plus our own macOS build and the VC++ runtime files). Recommended:
  **(b)**.
- **W2. The llama.cpp runtime on clean Windows (section 6).** Open a bug for Pitwall 1.0.x now (failing test first, then
  a clean-VM run of the first engine install), separate from WP-7? Recommended: **yes**, and before the Pitwall 1.1 tag.
- **W3. x64 speed.** Both runs used Apple M4 cores. Measure Whisper small Q8_0 on one real x64 Windows PC (0.25 day)
  before WP-7's UI work, or accept the ARM numbers and the `audio_ctx` margin? Recommended: **measure** when an x64 PC
  that is not a CI runner is free; the user's own PC would do, with the user's consent.
- **W4. `audio_ctx` sized to the clip.** Use it from the start (clicks of 5–15 s come back in about 1 s instead of 2–3 s)
  with a switch in Settings, or keep the full window? It held the words on these synthetic clips; real voices are
  untested. Recommended: **use it, behind a WP-7 test on a few recorded clips of the user's own voice** (with consent),
  and fall back to the full window if WER rises.

## Appendix: how to repeat

Clips (texts invented; `refs.tsv` is `id<TAB>voice<TAB>text`):

```bash
C=/Volumes/d7sst7/dev-targets/scratch/pitwall-speech-spike/clips
while IFS=$'\t' read id v t; do say -r 150 -v "$v" -o $id.aiff "$t" && afconvert -f WAVE -d LEI16@16000 -c 1 $id.aiff $id.wav; done < refs.tsv
ffmpeg -i en12.wav -f lavfi -i "anoisesrc=color=pink:amplitude=0.06:sample_rate=16000:seed=7" \
  -filter_complex "[0][1]amix=inputs=2:duration=first:normalize=0" -ar 16000 -ac 1 -c:a pcm_s16le en12n.wav   # and en20n
ffmpeg -i en30.wav -i en20.wav -filter_complex "[0][1]concat=n=2:v=0:a=1" -ar 16000 -ac 1 -c:a pcm_s16le en45.wav
# long300: 8 rounds of en30 en20 en12 de12 through ffmpeg's concat demuxer, cut with -t 300
```

| id | voice | text |
|---|---|---|
| en05 | Daniel | Please summarise the last three error lines for me, in plain words. |
| en12 | Samantha | Load the small coding model, set the context to sixteen thousand tokens, and then tell me how much memory it will use on this computer. |
| en20 | Daniel | Compare the two answers above. Which one handles the empty list correctly, and why does the second one time out after thirty seconds? Please keep the reply short, and show the corrected function at the end. |
| en30 | Samantha | I am planning a trip from Rotterdam to Lyon next spring. Draft a packing list for four days of cycling, keep it under twenty items, and mark anything I could buy on the way. After that, suggest two rest days with a short reason for each, and finish with one sentence about the weather in April. If a train is faster for one of the stages, say which stage and roughly how long the trip takes. |
| de12 | Anna | Fasse bitte die letzten drei Fehlermeldungen kurz zusammen und schlage eine mögliche Ursache für den Absturz des Programms vor. |
| fr10 | Thomas | Explique en deux phrases pourquoi la réponse précédente est trop lente, et propose une solution simple. |

Runs (`M=/Volumes/d7sst7/dev-targets/models`; a later `-c` overrides the harness's `-c 8192`):

```bash
python3 -I bench.py qwen06-metal llama 18731 -m $M/ggml-org/Qwen3-ASR-0.6B-GGUF/Qwen3-ASR-0.6B-Q8_0.gguf \
  --mmproj $M/ggml-org/Qwen3-ASR-0.6B-GGUF/mmproj-Qwen3-ASR-0.6B-Q8_0.gguf -ngl 99        # + -c 4096; + --device none -t 4 -tb 4
python3 -I bench.py wh-small-metal whisper 18732 -m $M/ggerganov/whisper.cpp/ggml-small.bin -l auto -t 4   # + -ng for CPU
CLIPS=long300 REPS=1 MAXTOK=2048 python3 -I bench.py qwen06-metal-c6144-long llama 18750 ... -c 6144
```

The harness (`bench.py`, Python standard library only; the WER normaliser folds a few number words and the two
spellings "summarise" and "timeout"):

```python
"""Speech spike harness: start one engine server, measure cold start, per-clip latency, memory; print JSON lines."""
import base64, json, os, re, signal, statistics, subprocess, sys, time, unicodedata, urllib.request, uuid

S = "/Volumes/d7sst7/dev-targets/scratch/pitwall-speech-spike"
CLIPS = S + "/clips"
LL = S + "/engine/llama-b11669"
WH = S + "/whisper-build/bin"

NUM = {"16,000": "sixteen thousand", "16000": "sixteen thousand", "30": "thirty", "20": "twenty", "3": "three",
       "4": "four", "2": "two", "1": "one", "summarise": "summarize", "timeout": "time out"}

def norm(t):
    t = unicodedata.normalize("NFC", t.lower())
    for k, v in NUM.items():
        t = re.sub(r"(?<![\w,])" + re.escape(k) + r"(?![\w,])", v, t)
    t = re.sub(r"[^\w\s']", " ", t)
    return t.split()

def wer(ref, hyp):
    r, h = norm(ref), norm(hyp)
    d = list(range(len(h) + 1))
    for i in range(1, len(r) + 1):
        p, d[0] = d[0], i
        for j in range(1, len(h) + 1):
            p, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, p + (r[i - 1] != h[j - 1]))
    return d[len(h)] / len(r)

def dur(path):
    import wave
    with wave.open(path) as w:
        return w.getnframes() / w.getframerate()

def footprint(pid):
    out = subprocess.run(["footprint", "-p", str(pid)], capture_output=True, text=True).stdout
    def grab(label):
        m = re.search(label + r":\s*([\d.]+)\s*([KMG]B)", out)
        if not m: return None
        return float(m.group(1)) * {"KB": 1 / 1024, "MB": 1, "GB": 1024}[m.group(2)]
    return grab("phys_footprint"), grab("phys_footprint_peak")

def post(url, body, headers):
    req = urllib.request.Request(url, data=body, headers=headers)
    with urllib.request.urlopen(req, timeout=600) as r:
        return json.loads(r.read())

def run(name, engine, args, port, reps=int(os.environ.get('REPS', '3'))):
    if engine == "llama":
        cmd = [LL + "/llama-server", "--host", "127.0.0.1", "--port", str(port), "--temp", "0", "-c", "8192", "-np", "1", "--no-webui"] + args
    else:
        cmd = [WH + "/whisper-server", "--host", "127.0.0.1", "--port", str(port)] + args
    log = open(f"{S}/out/{name}.server.log", "w")
    t0 = time.monotonic()
    p = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)
    health = f"http://127.0.0.1:{port}/health"
    while True:
        if p.poll() is not None:
            print(json.dumps({"run": name, "error": "server exited", "code": p.returncode})); return
        try:
            with urllib.request.urlopen(health, timeout=2) as r:
                if r.status == 200: break
        except Exception:
            pass
        if time.monotonic() - t0 > 300:
            print(json.dumps({"run": name, "error": "no health in 300 s"})); p.kill(); return
        time.sleep(0.1)
    cold = time.monotonic() - t0
    refs = {l.split("\t")[0]: l.rstrip("\n").split("\t")[2] for l in open(CLIPS + "/refs.tsv", encoding="utf-8")}
    print(json.dumps({"run": name, "cmd": " ".join(cmd), "cold_start_s": round(cold, 2), "load": os.getloadavg()[0]}), flush=True)
    first = True
    for cid in os.environ.get("CLIPS", "en05 en12 en20 en30 en45 en12n en20n de12 fr10").split():
        path = f"{CLIPS}/{cid}.wav"
        times, text = [], ""
        for i in range(reps):
            t = time.monotonic()
            if engine == "llama":
                b64 = base64.b64encode(open(path, "rb").read()).decode()
                body = json.dumps({"messages": [{"role": "user", "content": [
                    {"type": "input_audio", "input_audio": {"data": b64, "format": "wav"}}]}],
                    "temperature": 0, "max_tokens": int(os.environ.get("MAXTOK", "1024")), "cache_prompt": False}).encode()
                r = post(f"http://127.0.0.1:{port}/v1/chat/completions", body, {"Content-Type": "application/json"})
                text = r["choices"][0]["message"]["content"]
            else:
                bnd = uuid.uuid4().hex
                data = open(path, "rb").read()
                parts = []
                for k, v in (("temperature", "0"), ("response_format", "json")):
                    parts.append(f"--{bnd}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n".encode())
                parts.append(f"--{bnd}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n".encode() + data + b"\r\n")
                parts.append(f"--{bnd}--\r\n".encode())
                r = post(f"http://127.0.0.1:{port}/inference", b"".join(parts), {"Content-Type": f"multipart/form-data; boundary={bnd}"})
                text = r["text"]
            el = time.monotonic() - t
            if first:
                print(json.dumps({"run": name, "first_request_s": round(el, 3), "clip": cid}), flush=True)
                first = False
            times.append(el)
        clean = re.sub(r"^.*?<asr_text>", "", text.strip(), flags=re.S).strip()
        med = statistics.median(times)
        print(json.dumps({"run": name, "clip": cid, "audio_s": round(dur(path), 2), "median_s": round(med, 3),
                          "rtf": round(med / dur(path), 3), "wer": round(wer(refs[cid], clean), 3), "raw": text.strip()},
                         ensure_ascii=False), flush=True)
    fp, peak = footprint(p.pid)
    print(json.dumps({"run": name, "footprint_mb": fp, "footprint_peak_mb": peak}), flush=True)
    p.send_signal(signal.SIGTERM)
    try: p.wait(10)
    except subprocess.TimeoutExpired: p.kill()

if __name__ == "__main__":
    name, engine, port = sys.argv[1], sys.argv[2], int(sys.argv[3])
    run(name, engine, sys.argv[4:], port)
```

`cache_prompt: false` matters: without it, the three repeats of a clip reused the cached audio tokens and the medians
were too low (a first run, discarded).

### Second revision: the Windows and Linux run

Scratch folder `/Volumes/d7sst7/dev-targets/scratch/pitwall-speech-measure` (not kept). Binaries from the `b5454` release
(`v1.9.5`), checked with `shasum -a 256 -c` against the digests in section 7.2. The English 300 s clip:

```bash
: > l.txt; for r in 1 2 3 4 5; do for c in en30 en20 en12 en05; do echo "file '$PWD/$c.wav'" >> l.txt; done; done
echo "file '$PWD/en30.wav'" >> l.txt; echo "file '$PWD/en20.wav'" >> l.txt
ffmpeg -v error -f concat -safe 0 -i l.txt -af apad=whole_dur=300 -ar 16000 -ac 1 -c:a pcm_s16le en300.wav
```

Linux: the spike's `bench.py` with `WH`, `CLIPS`, `OUT` and `IDS` from the environment, memory from `/proc/<pid>/status`
(`VmRSS`, `VmHWM`) instead of `footprint`, and `ACX=auto` adding the form field
`audio_ctx = min(1500, int(seconds * 50) + 64)`:

```bash
docker run --rm --cpus 4 --memory 12g -v $D/dl/lin/whisper-bin-ubuntu-arm64:/w/bin:ro -v $D/clips:/w/clips:ro \
  -v $D/scripts:/w/scripts:ro -v $M/ggerganov/whisper.cpp:/w/models:ro -v $D/out:/w/out train5-linux-dev:2 /w/scripts/linux-run.sh
# linux-run.sh: LD_LIBRARY_PATH=/w/bin python3 -I bench_whisper.py lin-small-q8-cpu4 18801 -m /w/models/ggml-small-q8_0.bin -l en -t 4
#               IDS="en300 long300" REPS=1 python3 -I bench_whisper.py lin-small-q8-cpu4-long 18802 ...; ACX=auto for the sized runs
```

Windows: a host driver run as `scripts/vm-lock-run.sh speech-measure-whisper -- win-drive.sh` with
`PADDOCK_VM_SSH_DEADLINE=1200` (the first boot after a revert took longer than the default 300 s once): an `EXIT` trap that
always runs `vm-win.sh stop` and `vm-win.sh revert clean-baseline`; `revert clean-baseline`, `start`, `put` of the two
archives, the clips, the model, `vc_redist.arm64.exe` and the scripts; a precheck (section 6) as `paddock-test`; the
redistributable installed as `paddock-admin` (`/install /quiet /norestart`); the measurement as `paddock-test`; `get` of the
results. System facts come from the registry (`Get-CimInstance` is denied over SSH for `paddock-test` and an earlier
attempt hung on it). The guest harness:

```powershell
# Whisper server harness (Windows guest): the spike's bench.py procedure in PowerShell 5.1 with curl.exe.
# Starts whisper-server, times cold start to /health, posts each clip $Reps times, records median and the raw text
# (WER is computed on the host with the spike's normaliser), then working set and peak working set.
param(
  [string]$Name, [int]$Port, [string]$Bin, [string]$Clips, [string]$Out, [string]$Ids, [int]$Reps = 3,
  [string[]]$ServerArgs, [switch]$AutoAudioCtx
)
$ErrorActionPreference = 'Stop'
function Emit($o) { ($o | ConvertTo-Json -Compress) | Out-File -Append -Encoding utf8 "$Out\$Name.jsonl" }

$args2 = @('--host', '127.0.0.1', '--port', "$Port") + $ServerArgs
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath "$Bin\whisper-server.exe" -ArgumentList $args2 -PassThru -NoNewWindow `
  -RedirectStandardOutput "$Out\$Name.server.out.log" -RedirectStandardError "$Out\$Name.server.log"
$null = $p.Handle
while ($true) {
  if ($p.HasExited) { Emit @{ run = $Name; error = 'server exited'; code = ('0x{0:X8}' -f $p.ExitCode) }; exit 1 }
  $code = & curl.exe -s -o NUL -w '%{http_code}' --max-time 2 "http://127.0.0.1:$Port/health"
  if ($code -eq '200') { break }
  if ($sw.Elapsed.TotalSeconds -gt 300) { Emit @{ run = $Name; error = 'no health in 300 s' }; $p.Kill(); exit 1 }
  Start-Sleep -Milliseconds 50
}
$cold = $sw.Elapsed.TotalSeconds
$p.Refresh()
Emit @{ run = $Name; cmd = "whisper-server $($args2 -join ' ')"; cold_start_s = [math]::Round($cold, 3);
        ws_idle_mb = [math]::Round($p.WorkingSet64 / 1MB, 1) }
$first = $true
foreach ($cid in ($Ids -split ' ')) {
  $path = "$Clips\$cid.wav"
  $times = @()
  for ($i = 0; $i -lt $Reps; $i++) {
    $resp = "$Out\$Name.$cid.json"
    $extra = @()
    if ($AutoAudioCtx) {
      $secs = ((Get-Item $path).Length - 44) / 32000.0
      $extra = @('-F', "audio_ctx=$([math]::Min(1500, [int][math]::Floor($secs * 50) + 64))")
    }
    $t = [Diagnostics.Stopwatch]::StartNew()
    & curl.exe -s --max-time 900 -o $resp -F "file=@$path" -F 'temperature=0' -F 'response_format=json' @extra "http://127.0.0.1:$Port/inference"
    $el = $t.Elapsed.TotalSeconds
    if ($LASTEXITCODE -ne 0) { Emit @{ run = $Name; clip = $cid; error = "curl exit $LASTEXITCODE" }; break }
    if ($first) { Emit @{ run = $Name; first_request_s = [math]::Round($el, 3); clip = $cid }; $first = $false }
    $times += $el
  }
  $s = $times | Sort-Object
  $med = $s[[int][math]::Floor($s.Count / 2)]
  Emit @{ run = $Name; clip = $cid; median_s = [math]::Round($med, 3); min_s = [math]::Round($s[0], 3);
          max_s = [math]::Round($s[-1], 3); reps = $s.Count }
}
$p.Refresh()
Emit @{ run = $Name; ws_mb = [math]::Round($p.WorkingSet64 / 1MB, 1); peak_ws_mb = [math]::Round($p.PeakWorkingSet64 / 1MB, 1);
        private_mb = [math]::Round($p.PrivateMemorySize64 / 1MB, 1) }
Stop-Process -Id $p.Id -Force
```

## Sources (read 2026-11-11)

- llama.cpp `b11669`: [docs/multimodal.md](https://raw.githubusercontent.com/ggml-org/llama.cpp/b11669/docs/multimodal.md),
  [server README](https://raw.githubusercontent.com/ggml-org/llama.cpp/b11669/tools/server/README.md) (`input_audio`,
  miniaudio formats), [mtmd-helper.cpp](https://raw.githubusercontent.com/ggml-org/llama.cpp/b11669/tools/mtmd/mtmd-helper.cpp)
  (decoder resamples to the model's rate, line 323); Qwen3-ASR commits via the GitHub search API
  (`21a4933042`, PR [#19441](https://github.com/ggml-org/llama.cpp/pull/19441)).
- [ggml-org/Qwen3-ASR-0.6B-GGUF](https://huggingface.co/ggml-org/Qwen3-ASR-0.6B-GGUF) and
  [1.7B](https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF) (file sizes, Hugging Face API);
  [ggerganov/whisper.cpp models](https://huggingface.co/ggerganov/whisper.cpp).
- [whisper.cpp releases](https://github.com/ggml-org/whisper.cpp/releases): `v1.9.5` (2026-10-06) has no assets; the
  `b5454` release beside it has Windows and Linux binaries and a macOS xcframework only.
- whisper.cpp `b5454` and `v1.9.5` release pages (assets, digests, both on commit `d1be6fde1`):
  [b5454](https://github.com/ggml-org/whisper.cpp/releases/tag/b5454), [v1.9.5](https://github.com/ggml-org/whisper.cpp/releases/tag/v1.9.5);
  `examples/server/server.cpp` at `v1.9.5` (per-request `audio_ctx`, `--convert`, `--help`), read in the spike's checkout.
- Microsoft's VC++ redistributable for ARM64, [aka.ms/vs/17/release/vc_redist.arm64.exe](https://aka.ms/vs/17/release/vc_redist.arm64.exe)
  (redirects to a download URL that names its SHA-256).
- Handy's [src-tauri/Cargo.toml](https://github.com/cjpais/Handy/blob/main/src-tauri/Cargo.toml) (per-platform ggml
  backends built in its CI).
- [cpal on crates.io](https://crates.io/crates/cpal) (0.18.2, Apache-2.0, dependencies per target).
- Rule 76 rows: the Apple, Microsoft, Open WebUI and LM Studio pages linked in section 3; ChatGPT only from a
  third-party page, marked unverified.

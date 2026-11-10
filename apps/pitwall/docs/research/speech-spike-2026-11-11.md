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

## 4. Recommended WP-7 design

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

## Open decisions for the user

1. **Accept A** (Qwen3-ASR 0.6B, 1.02 GB download, 1.1–1.25 GB memory, no new engine) as Pitwall 1.1's speech model?
   The cheaper alternative is Whisper small Q8_0 (264 MB, 0.55 GB, same accuracy here) through whisper.cpp, a second
   engine that reverses ADR 0020. Recommended: A.
2. **Toggle instead of hold:** click or shortcut to start and stop, Esc to discard, as macOS Dictation, Windows voice
   typing and Open WebUI do? Recommended: toggle only in 1.1.
3. **Clip cap:** keep 5 minutes (`-c 6144`, about 1.25 GB) or cut to 2 minutes (`-c 4096`, about 1.1 GB)?
   Recommended: 5 minutes.
4. **Windows and Linux first:** run the 0.5-day measurement before WP-7 starts, with sherpa-onnx (option C) as the
   fallback only if A's real-time factor on a Windows CPU is worse than about 0.3? Recommended: yes.
5. **Languages:** this spike tried English, German and French. Which languages does the user dictate in, so the
   Windows run and the WP-7 test clip use them?

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
- [cpal on crates.io](https://crates.io/crates/cpal) (0.18.2, Apache-2.0, dependencies per target).
- Rule 76 rows: the Apple, Microsoft, Open WebUI and LM Studio pages linked in section 3; ChatGPT only from a
  third-party page, marked unverified.

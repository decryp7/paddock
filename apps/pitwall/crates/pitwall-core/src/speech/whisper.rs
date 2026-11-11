//! The request to whisper.cpp's `whisper-server` and the reading of its answer.
//!
//! whisper-server (pinned at commit `d1be6fde1`, `v1.9.5`; `examples/server/server.cpp`) takes `POST /inference` as
//! `multipart/form-data` with the audio in the `file` field and its options as further fields (`language`, `temperature`,
//! `response_format`, `audio_ctx`, ...). With `response_format=json` it answers `{"text": "..."}`, one line per segment;
//! errors are `{"error": "..."}` with a 4xx or 5xx status. Read in the source on 2026-11-12.

use super::pcm::Clip;
use super::{wav, SpeechError, SAMPLE_RATE};

/// The language Pitwall asks for (decided 2026-11-11: English only). A fixed language also skips Whisper's detection pass.
pub const LANGUAGE: &str = "en";

/// The largest answer read from the engine: a 5-minute clip is a few thousand words.
pub const MAX_ANSWER_BYTES: usize = 256 * 1024;

/// The longest transcript kept, in bytes, cut at a character boundary.
pub const MAX_TRANSCRIPT_BYTES: usize = 64 * 1024;

/// Whisper's encoder window: 1,500 positions for 30 s.
pub const FULL_AUDIO_CTX: u16 = 1500;

/// Options of one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InferenceOptions {
    /// Whisper's `audio_ctx`: `None` encodes the full 30 s window; a smaller value, sized by [`audio_ctx_for`], makes a short
    /// clip several times faster (the speech note's section 5; decision W4 is the user's, so the caller sets it).
    pub audio_ctx: Option<u16>,
}

/// The `audio_ctx` sized to a clip: 50 positions a second (1,500 for 30 s) plus a margin of 64, at most the full window.
pub fn audio_ctx_for(clip: &Clip) -> u16 {
    let seconds = clip.samples().len().div_ceil(SAMPLE_RATE as usize) as u64;
    (seconds * 50 + 64).min(u64::from(FULL_AUDIO_CTX)) as u16
}

/// A `multipart/form-data` body and its `Content-Type`.
#[derive(Clone, PartialEq, Eq)]
pub struct Multipart {
    /// `multipart/form-data; boundary=...`.
    pub content_type: String,
    /// The body.
    pub body: Vec<u8>,
}

impl std::fmt::Debug for Multipart {
    // Never the body: it holds the user's voice.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Multipart")
            .field("content_type", &self.content_type)
            .field("bytes", &self.body.len())
            .finish()
    }
}

/// The body of `POST /inference` for `clip`. `boundary` is the caller's random token: 16 to 70 ASCII letters and digits
/// (RFC 2046 §5.1.1). A boundary that occurs in the audio is refused; the caller picks another.
pub fn inference_body(clip: &Clip, opts: InferenceOptions, boundary: &str) -> Result<Multipart, SpeechError> {
    if !(16..=70).contains(&boundary.len()) || !boundary.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(SpeechError::Request(
            "the multipart boundary must be 16 to 70 letters and digits".into(),
        ));
    }
    let audio = wav::encode(clip);
    let marker = format!("--{boundary}");
    if audio.windows(marker.len()).any(|w| w == marker.as_bytes()) {
        return Err(SpeechError::Request("the multipart boundary occurs in the audio".into()));
    }
    let mut body = Vec::with_capacity(audio.len() + 1024);
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(format!("{marker}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    };
    field("language", LANGUAGE);
    field("temperature", "0.0");
    field("response_format", "json");
    field("no_timestamps", "true");
    if let Some(ctx) = opts.audio_ctx {
        field("audio_ctx", &ctx.min(FULL_AUDIO_CTX).to_string());
    }
    body.extend_from_slice(
        format!("{marker}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"clip.wav\"\r\nContent-Type: audio/wav\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(&audio);
    body.extend_from_slice(format!("\r\n{marker}--\r\n").as_bytes());
    Ok(Multipart {
        content_type: format!("multipart/form-data; boundary={boundary}"),
        body,
    })
}

/// What was said: plain text, trimmed, segments joined by a space, Whisper's non-speech tags (`[BLANK_AUDIO]`,
/// `(silence)`, `[Music]`) left out. Empty when nothing was heard.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Transcript {
    text: String,
    cut: bool,
}

impl std::fmt::Debug for Transcript {
    // Never the text: it is what the user said.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transcript")
            .field("bytes", &self.text.len())
            .field("cut", &self.cut)
            .finish()
    }
}

impl Transcript {
    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether nothing was heard.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Whether the text was cut at [`MAX_TRANSCRIPT_BYTES`].
    pub fn was_cut(&self) -> bool {
        self.cut
    }

    /// A transcript from the engine's raw text (see the type's documentation).
    pub fn from_engine_text(raw: &str) -> Transcript {
        let mut text = String::new();
        for segment in raw.lines().map(str::trim).filter(|s| !s.is_empty() && !is_non_speech(s)) {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(segment);
        }
        let cut = text.len() > MAX_TRANSCRIPT_BYTES;
        if cut {
            let mut end = MAX_TRANSCRIPT_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        Transcript { text, cut }
    }
}

/// A whole segment that is one bracketed tag, as Whisper writes for silence, music or noise.
fn is_non_speech(segment: &str) -> bool {
    let bracketed = |open: char, close: char| {
        segment.starts_with(open) && segment.ends_with(close) && segment[1..segment.len() - 1].chars().all(|c| c != open && c != close)
    };
    bracketed('[', ']') || bracketed('(', ')')
}

/// Reads whisper-server's answer to `/inference`: its HTTP status and at most [`MAX_ANSWER_BYTES`] of body.
pub fn parse_inference(status: u16, body: &[u8]) -> Result<Transcript, SpeechError> {
    if body.len() > MAX_ANSWER_BYTES {
        return Err(SpeechError::Answer(format!("it is over {} KiB", MAX_ANSWER_BYTES / 1024)));
    }
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| SpeechError::Answer(format!("HTTP {status} without JSON")))?;
    if !(200..300).contains(&status) {
        let detail = value
            .get("error")
            .and_then(serde_json::Value::as_str)
            .map(|e| e.chars().take(200).collect::<String>())
            .unwrap_or_else(|| "no reason given".into());
        return Err(SpeechError::Engine(format!("HTTP {status}, {detail}")));
    }
    let text = value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| SpeechError::Answer("it has no `text`".into()))?;
    Ok(Transcript::from_engine_text(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const B: &str = "pitwallSpeechBoundary0123456789";

    fn clip(samples: usize) -> Clip {
        Clip::from_samples(vec![0x0101; samples]).unwrap()
    }

    #[test]
    fn the_body_carries_the_options_and_the_wav() {
        let m = inference_body(&clip(16_000), InferenceOptions::default(), B).unwrap();
        assert_eq!(m.content_type, format!("multipart/form-data; boundary={B}"));
        let text = String::from_utf8_lossy(&m.body);
        for (name, value) in [
            ("language", "en"),
            ("temperature", "0.0"),
            ("response_format", "json"),
            ("no_timestamps", "true"),
        ] {
            assert!(
                text.contains(&format!(
                    "--{B}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                )),
                "{name}"
            );
        }
        assert!(!text.contains("audio_ctx"), "the full window unless asked");
        assert!(text.contains("name=\"file\"; filename=\"clip.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFF"));
        assert!(m.body.ends_with(format!("\r\n--{B}--\r\n").as_bytes()));
        let wav = wav::encode(&clip(16_000));
        assert!(
            m.body.windows(wav.len()).any(|w| w == wav.as_slice()),
            "the WAV is carried byte for byte"
        );
    }

    #[test]
    fn the_sized_window_is_sent_when_asked_and_never_above_the_full_one() {
        let m = inference_body(&clip(16_000), InferenceOptions { audio_ctx: Some(9_999) }, B).unwrap();
        assert!(String::from_utf8_lossy(&m.body).contains("name=\"audio_ctx\"\r\n\r\n1500\r\n"));
    }

    #[test]
    fn the_window_grows_with_the_clip() {
        assert_eq!(audio_ctx_for(&clip(0)), 64);
        assert_eq!(audio_ctx_for(&clip(1)), 114);
        assert_eq!(audio_ctx_for(&clip(16_000 * 10)), 564);
        assert_eq!(audio_ctx_for(&clip(16_000 * 29)), 1500);
        assert_eq!(audio_ctx_for(&clip(16_000 * 300)), 1500);
    }

    #[test]
    fn refuses_a_bad_boundary() {
        for b in ["short", "has space in it 0123", "ünïcödé-boundary-0123", &"x".repeat(71)] {
            assert!(inference_body(&clip(10), InferenceOptions::default(), b).is_err(), "{b}");
        }
    }

    #[test]
    fn refuses_a_boundary_that_occurs_in_the_audio() {
        // "--AAAAAAAAAAAAAAAA" as little-endian samples: 0x2D2D is "--", 0x4141 is "AA".
        let mut samples = vec![0x2D2D_i16];
        samples.extend(vec![0x4141_i16; 8]);
        let c = Clip::from_samples(samples).unwrap();
        let e = inference_body(&c, InferenceOptions::default(), "AAAAAAAAAAAAAAAA").unwrap_err();
        assert!(e.to_string().contains("occurs in the audio"), "{e}");
    }

    #[test]
    fn reads_the_text_of_an_answer() {
        let t = parse_inference(200, br#"{"text":" Hello there.\n How are you?\n"}"#).unwrap();
        assert_eq!(t.text(), "Hello there. How are you?");
        assert!(!t.was_cut());
    }

    #[test]
    fn silence_and_noise_tags_are_nothing_heard() {
        let t = parse_inference(200, br#"{"text":" [BLANK_AUDIO]\n (silence)\n [Music]\n"}"#).unwrap();
        assert!(t.is_empty());
        let t = parse_inference(200, br#"{"text":"[BLANK_AUDIO]\n Set a timer [for] ten minutes.\n"}"#).unwrap();
        assert_eq!(t.text(), "Set a timer [for] ten minutes.");
    }

    #[test]
    fn an_engine_error_names_the_status_and_reason() {
        let e = parse_inference(400, br#"{"error":"failed to read audio data"}"#)
            .unwrap_err()
            .to_string();
        assert!(e.contains("HTTP 400") && e.contains("failed to read audio data"), "{e}");
        let e = parse_inference(500, b"{}").unwrap_err().to_string();
        assert!(e.contains("no reason given"), "{e}");
    }

    #[test]
    fn refuses_answers_it_cannot_read() {
        assert!(parse_inference(500, b"Internal Server Error")
            .unwrap_err()
            .to_string()
            .contains("without JSON"));
        assert!(parse_inference(200, br#"{"txt":"x"}"#)
            .unwrap_err()
            .to_string()
            .contains("no `text`"));
        let big = vec![b' '; MAX_ANSWER_BYTES + 1];
        assert!(parse_inference(200, &big).unwrap_err().to_string().contains("KiB"));
    }

    #[test]
    fn a_long_transcript_is_cut_at_a_character_boundary() {
        let raw = "é".repeat(MAX_TRANSCRIPT_BYTES);
        let t = Transcript::from_engine_text(&raw);
        assert!(t.was_cut());
        assert!(t.text().len() <= MAX_TRANSCRIPT_BYTES);
        assert_eq!(t.text().len() % 2, 0);
    }

    #[test]
    fn debug_output_never_holds_what_was_said() {
        let t = Transcript::from_engine_text("my secret words");
        assert!(!format!("{t:?}").contains("secret"));
        let m = inference_body(&clip(10), InferenceOptions::default(), B).unwrap();
        assert!(format!("{m:?}").contains("bytes"));
    }

    proptest! {
        #[test]
        fn parsing_never_panics(status in 0u16..1000, body in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = parse_inference(status, &body);
        }

        #[test]
        fn engine_text_never_panics(raw in ".{0,400}") {
            let t = Transcript::from_engine_text(&raw);
            prop_assert!(t.text().len() <= MAX_TRANSCRIPT_BYTES);
        }
    }
}

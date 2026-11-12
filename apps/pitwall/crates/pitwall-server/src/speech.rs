//! Speech to text (ADR 0023 Decision 5, WP-7): the [`Transcriber`] port and its adapter for whisper.cpp's `whisper-server`
//! (the user's choice of 2026-11-11, `docs/research/speech-spike-2026-11-11.md`).
//!
//! The adapter only speaks to a whisper-server that already listens on 127.0.0.1 (through `pitwall_loopback`, so it cannot
//! reach anything else). Starting, pinning and supervising that process, and delivering the engine and the model, are the
//! next slices of WP-7; until they land the server holds [`NoSpeechEngine`]. What is sent and read is built and checked by
//! `pitwall_core::speech`.
//!
//! Logs (ER-27): one `pitwall_speech` line per clip with its length, bytes, the engine's time and the outcome; never the
//! audio and never the text.

use crate::engines::BoxFuture;
use pitwall_core::speech::pcm::Clip;
use pitwall_core::speech::whisper::{self, InferenceOptions, Transcript};
use pitwall_core::speech::SpeechError;
use pitwall_loopback::{Client, LoopbackError, LoopbackPort};
use std::time::Instant;

/// A transcript and what it cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcribed {
    /// What was said (empty when nothing was heard).
    pub transcript: Transcript,
    /// The clip's length after trimming silence, in milliseconds.
    pub speech_ms: u64,
    /// How long the engine took, in milliseconds (0 when nothing needed sending).
    pub engine_ms: u64,
}

/// Why a clip was not transcribed; the message says what to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranscribeError {
    /// No speech engine is installed or running.
    #[error("the speech engine is not installed. Press the microphone again to set it up.")]
    NotInstalled,
    /// The engine did not answer on its port.
    #[error("the speech engine did not answer ({0}). Repair restarts it.")]
    Unreachable(String),
    /// The request or the answer failed a check, or the engine refused the clip.
    #[error(transparent)]
    Speech(#[from] SpeechError),
}

/// Turns a recorded clip into text. Implementations honour one contract: no deadline (the caller shows "Transcribing" with
/// the elapsed time and Cancel, ER-19), dropping the future abandons the request, and the clip is never kept or logged.
pub trait Transcriber: Send + Sync {
    /// Transcribes `clip`.
    fn transcribe<'a>(&'a self, clip: &'a Clip) -> BoxFuture<'a, Result<Transcribed, TranscribeError>>;
}

/// The transcriber while no speech engine exists: every clip is refused with [`TranscribeError::NotInstalled`].
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSpeechEngine;

impl Transcriber for NoSpeechEngine {
    fn transcribe<'a>(&'a self, _clip: &'a Clip) -> BoxFuture<'a, Result<Transcribed, TranscribeError>> {
        Box::pin(async { Err(TranscribeError::NotInstalled) })
    }
}

/// A whisper-server listening on a loopback port.
#[derive(Debug, Clone)]
pub struct WhisperServer {
    client: Client,
    options: InferenceOptions,
    random: fn(&mut [u8]) -> Result<(), String>,
}

fn os_random(b: &mut [u8]) -> Result<(), String> {
    getrandom::fill(b).map_err(|e| e.to_string())
}

impl WhisperServer {
    /// The whisper-server on `port` (it is started without an API key: it listens on 127.0.0.1 at a port Pitwall picked).
    pub fn new(port: LoopbackPort, options: InferenceOptions) -> WhisperServer {
        WhisperServer {
            client: Client::new(port, None),
            options,
            random: os_random,
        }
    }

    /// A fresh multipart boundary of 32 letters and digits.
    fn boundary(&self) -> Result<String, TranscribeError> {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut bytes = [0u8; 32];
        (self.random)(&mut bytes).map_err(|e| SpeechError::Request(format!("no random boundary: {e}")))?;
        Ok(bytes.iter().map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char).collect())
    }

    async fn send(&self, clip: &Clip) -> Result<Transcript, TranscribeError> {
        let mut options = self.options;
        if let Some(sized) = options.audio_ctx {
            // A caller's value is a ceiling; the clip's own size is used when smaller (W4).
            options.audio_ctx = Some(sized.min(whisper::audio_ctx_for(clip)));
        }
        // A boundary that happens to occur in the audio is drawn again (a 32-character token: practically never).
        let mut body = Err(SpeechError::Request("no boundary".into()));
        for _ in 0..3 {
            body = whisper::inference_body(clip, options, &self.boundary()?);
            if body.is_ok() {
                break;
            }
        }
        let body = body?;
        let reply = self
            .client
            .send_no_deadline("POST", "/inference", body.body, &[("content-type", body.content_type.as_str())])
            .await
            .map_err(unreachable)?;
        let status = reply.status;
        let raw = reply.bytes(whisper::MAX_ANSWER_BYTES).await.map_err(|e| match e {
            LoopbackError::Body(_) => SpeechError::Answer(format!("it is over {} KiB", whisper::MAX_ANSWER_BYTES / 1024)).into(),
            other => unreachable(other),
        })?;
        Ok(whisper::parse_inference(status, &raw)?)
    }
}

fn unreachable(e: LoopbackError) -> TranscribeError {
    TranscribeError::Unreachable(e.to_string())
}

impl Transcriber for WhisperServer {
    fn transcribe<'a>(&'a self, clip: &'a Clip) -> BoxFuture<'a, Result<Transcribed, TranscribeError>> {
        Box::pin(async move {
            let speech = clip.trimmed();
            let speech_ms = speech.duration_ms();
            if speech.is_empty() {
                log::info!(target: "pitwall_speech", "clip of {} ms: nothing heard, not sent", clip.duration_ms());
                return Ok(Transcribed {
                    transcript: Transcript::default(),
                    speech_ms,
                    engine_ms: 0,
                });
            }
            let started = Instant::now();
            let result = self.send(&speech).await;
            let engine_ms = started.elapsed().as_millis() as u64;
            match &result {
                Ok(t) => log::info!(
                    target: "pitwall_speech",
                    "clip of {} ms ({} ms of speech, {} bytes) transcribed in {engine_ms} ms: {} bytes of text{}",
                    clip.duration_ms(),
                    speech_ms,
                    speech.samples().len() * 2,
                    t.text().len(),
                    if t.was_cut() { ", cut" } else { "" }
                ),
                Err(e) => log::warn!(
                    target: "pitwall_speech",
                    "clip of {} ms ({speech_ms} ms of speech) failed after {engine_ms} ms: {e}",
                    clip.duration_ms()
                ),
            }
            Ok(Transcribed {
                transcript: result?,
                speech_ms,
                engine_ms,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot whisper-server stand-in: reads one whole request and answers `reply`; returns what it received.
    async fn serve(reply: String) -> (LoopbackPort, tokio::task::JoinHandle<Vec<u8>>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = LoopbackPort::new(l.local_addr().unwrap().port()).unwrap();
        let h = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = s.read(&mut buf).await.unwrap();
                seen.extend_from_slice(&buf[..n]);
                if n == 0 || seen.ends_with(b"--\r\n") {
                    break;
                }
            }
            s.write_all(reply.as_bytes()).await.unwrap();
            s.shutdown().await.unwrap();
            seen
        });
        (port, h)
    }

    fn http(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    /// A second of a loud square wave at 16 kHz.
    fn speech() -> Clip {
        Clip::from_samples((0..16_000).map(|i| if i % 40 < 20 { 8_000 } else { -8_000 }).collect()).unwrap()
    }

    #[tokio::test]
    async fn posts_the_clip_as_multipart_and_reads_the_text() {
        let (port, seen) = serve(http("200 OK", r#"{"text":" Open the dashboard.\n"}"#)).await;
        let w = WhisperServer::new(port, InferenceOptions::default());
        let t = w.transcribe(&speech()).await.unwrap();
        assert_eq!(t.transcript.text(), "Open the dashboard.");
        assert_eq!(t.speech_ms, 1000);
        let seen = seen.await.unwrap();
        let head = String::from_utf8_lossy(&seen);
        assert!(head.starts_with("POST /inference HTTP/1.1"), "{head}");
        assert!(head.contains("multipart/form-data; boundary="), "{head}");
        assert!(head.contains("name=\"language\"\r\n\r\nen"), "{head}");
        assert!(!head.contains("audio_ctx"), "the full window by default");
        assert!(!head.to_ascii_lowercase().contains("authorization"), "no key is sent");
    }

    #[tokio::test]
    async fn a_sized_window_is_a_ceiling_the_clip_lowers() {
        let (port, seen) = serve(http("200 OK", r#"{"text":"ok"}"#)).await;
        let w = WhisperServer::new(port, InferenceOptions { audio_ctx: Some(1500) });
        w.transcribe(&speech()).await.unwrap();
        let seen = seen.await.unwrap();
        assert!(String::from_utf8_lossy(&seen).contains("name=\"audio_ctx\"\r\n\r\n114\r\n"));
    }

    #[tokio::test]
    async fn a_silent_clip_is_never_sent() {
        // This port resets every connection: a request would fail.
        let held = paddock_testkit::net::HeldPort::reset();
        let port = LoopbackPort::new(held.port()).unwrap();
        let w = WhisperServer::new(port, InferenceOptions::default());
        let t = w.transcribe(&Clip::from_samples(vec![0; 32_000]).unwrap()).await.unwrap();
        assert!(t.transcript.is_empty());
        assert_eq!((t.speech_ms, t.engine_ms), (0, 0));
    }

    #[tokio::test]
    async fn an_engine_refusal_and_a_dead_port_say_what_happened() {
        let (port, _) = serve(http("400 Bad Request", r#"{"error":"failed to read audio data"}"#)).await;
        let e = WhisperServer::new(port, InferenceOptions::default())
            .transcribe(&speech())
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("HTTP 400") && e.contains("failed to read audio data"), "{e}");

        let held = paddock_testkit::net::HeldPort::reset();
        let port = LoopbackPort::new(held.port()).unwrap();
        let e = WhisperServer::new(port, InferenceOptions::default())
            .transcribe(&speech())
            .await
            .unwrap_err();
        assert!(matches!(e, TranscribeError::Unreachable(_)), "{e:?}");
        assert!(e.to_string().contains("Repair"), "{e}");
    }

    #[tokio::test]
    async fn an_over_long_answer_is_refused() {
        let big = format!(r#"{{"text":"{}"}}"#, "a".repeat(whisper::MAX_ANSWER_BYTES));
        let (port, _) = serve(http("200 OK", &big)).await;
        let e = WhisperServer::new(port, InferenceOptions::default())
            .transcribe(&speech())
            .await
            .unwrap_err();
        assert!(e.to_string().contains("KiB"), "{e}");
    }

    #[tokio::test]
    async fn without_an_engine_every_clip_is_refused_with_the_next_step() {
        let e = NoSpeechEngine.transcribe(&speech()).await.unwrap_err();
        assert_eq!(e, TranscribeError::NotInstalled);
        assert!(e.to_string().contains("set it up"));
    }

    #[test]
    fn a_failing_random_source_is_a_request_error() {
        let mut w = WhisperServer::new(LoopbackPort::new(9).unwrap(), InferenceOptions::default());
        w.random = |_| Err("no entropy".into());
        assert!(w.boundary().unwrap_err().to_string().contains("no entropy"));
        w.random = |b| {
            b.fill(255);
            Ok(())
        };
        let b = w.boundary().unwrap();
        assert_eq!(b.len(), 32);
        assert!(b.bytes().all(|c| c.is_ascii_alphanumeric()));
    }
}

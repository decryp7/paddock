//! Speech input for the Playground's composer (ADR 0023 Decision 5, WP-7): the pure half of the transcribe path.
//!
//! The app captures the microphone through its `Capture` port and feeds the device's frames to a [`pcm::ClipBuilder`],
//! which downmixes and resamples them to 16 kHz mono PCM16 and stops at [`MAX_CLIP_SECONDS`]. The clip is encoded as a
//! WAV file ([`wav::encode`]) and sent to the speech engine, whisper.cpp's `whisper-server` (the user's choice of
//! 2026-11-11, `docs/research/speech-spike-2026-11-11.md`), as the multipart request [`whisper::inference_body`]
//! builds; [`whisper::parse_inference`] turns its answer into a [`whisper::Transcript`].
//!
//! Nothing here touches a device, a file or the network. Audio is never logged: callers log only a clip's duration and
//! size, the engine's time and the outcome (ADR 0023 Decision 5, ER-27).

pub mod pcm;
pub mod wav;
pub mod whisper;

/// The sample rate every clip is resampled to: what Whisper was trained on.
pub const SAMPLE_RATE: u32 = 16_000;

/// The longest clip, in seconds (decided 2026-11-11: 5 minutes). Recording stops by itself there.
pub const MAX_CLIP_SECONDS: u32 = 300;

/// The most samples a clip holds: 5 minutes at 16 kHz, 9.6 MB of PCM16.
pub const MAX_CLIP_SAMPLES: usize = (SAMPLE_RATE * MAX_CLIP_SECONDS) as usize;

/// A speech input that cannot be used as given; the message says what happened and what to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpeechError {
    /// The device reported a format Pitwall does not take.
    #[error("the microphone reports {0}, which Pitwall cannot record")]
    Format(String),
    /// A clip is over the length limit.
    #[error("a clip is limited to {MAX_CLIP_SECONDS} seconds")]
    TooLong,
    /// The request to the speech engine could not be built.
    #[error("{0}")]
    Request(String),
    /// The speech engine answered with an error.
    #[error("the speech engine could not transcribe the clip: {0}")]
    Engine(String),
    /// The speech engine's answer was not what it documents.
    #[error("the speech engine's answer could not be read: {0}")]
    Answer(String),
}

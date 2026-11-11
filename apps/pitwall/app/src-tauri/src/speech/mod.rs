//! Speech input in the app (ADR 0023 Decision 5, WP-7): the microphone is opened here, in `Pitwall.app` (the process the OS
//! asks the user about, with a window to explain it), never in the service or the webview.
//!
//! - [`capture`]: the [`Capture`] port the recorder opens a device through, and the bounded hand-over of frames from the audio
//!   thread ([`Frames`]).
//! - [`recorder`]: [`Recorder`], the toggle's state (idle, recording, full, failed) and the clip it makes with
//!   `pitwall_core::speech::pcm`.
//! - `fake`: the test double, compiled only for tests and the `test-support` feature (rule 73): nothing in a shipped build can
//!   reach it.
//!
//! No audio is kept after a clip is transcribed and none is logged: lines carry lengths, counts and outcomes only.

pub mod capture;
#[cfg(any(test, feature = "test-support"))]
pub mod fake;
pub mod recorder;

pub use capture::{Capture, CaptureError, Frames, OpenCapture};
pub use recorder::{RecordError, Recorder, Status};

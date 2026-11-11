//! The capture port: how the recorder opens a microphone without knowing which one or how (rule 7: tests never touch a device).
//!
//! An adapter delivers the device's interleaved `f32` frames through [`Frames`], which never blocks the audio thread: the
//! hand-over is a bounded channel, and a chunk that does not fit is dropped and counted (an overrun), never queued without
//! limit.

use pitwall_core::speech::pcm::InputFormat;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};

/// How many chunks wait between the audio thread and the recorder: devices deliver 5 to 100 ms a chunk, so at least a
/// third of a second of slack (judgement).
pub const CHANNEL_CHUNKS: usize = 64;

/// The largest chunk taken, in samples: a second of 8 channels at 384 kHz. A larger callback is cut there (an overrun).
pub const MAX_CHUNK_SAMPLES: usize = 384_000 * 8;

/// Why the microphone could not be used; the message says what to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    /// No input device.
    #[error("no microphone was found. Connect one, or choose an input in the system's sound settings.")]
    NoDevice,
    /// The OS refused access.
    #[error("Pitwall may not use the microphone. Allow it in the system's privacy settings, then try again.")]
    Denied,
    /// The device's format is not one Pitwall takes.
    #[error("the microphone's format is not supported: {0}")]
    Format(String),
    /// The device failed while opening or recording.
    #[error("the microphone stopped working: {0}")]
    Device(String),
    /// Recording is not built for this system yet.
    #[error("speech input is not available on this system yet")]
    Unsupported,
}

/// What travels from the audio thread to the recorder.
#[derive(Debug)]
pub(crate) enum Chunk {
    /// Interleaved samples.
    Samples(Vec<f32>),
    /// Look at the failure slot.
    Wake,
}

/// The audio thread's end of the hand-over. Dropping it (with the stream that owns it) tells the recorder the device closed.
#[derive(Debug)]
pub struct Frames {
    tx: SyncSender<Chunk>,
    overruns: Arc<AtomicU64>,
    failure: Arc<Mutex<Option<CaptureError>>>,
}

/// The recorder's end: the chunks, the overrun count and the failure slot.
#[derive(Debug)]
pub(crate) struct FramesRx {
    pub(crate) rx: Receiver<Chunk>,
    pub(crate) overruns: Arc<AtomicU64>,
    pub(crate) failure: Arc<Mutex<Option<CaptureError>>>,
}

impl Frames {
    /// A hand-over of `capacity` chunks.
    pub(crate) fn channel(capacity: usize) -> (Frames, FramesRx) {
        let (tx, rx) = sync_channel(capacity);
        let overruns = Arc::new(AtomicU64::new(0));
        let failure = Arc::new(Mutex::new(None));
        (
            Frames {
                tx,
                overruns: overruns.clone(),
                failure: failure.clone(),
            },
            FramesRx { rx, overruns, failure },
        )
    }

    /// Hands over a chunk of interleaved samples. Never blocks: a chunk that does not fit is counted as an overrun.
    pub fn push(&self, interleaved: &[f32]) {
        if interleaved.is_empty() {
            return;
        }
        let take = interleaved.len().min(MAX_CHUNK_SAMPLES);
        if take < interleaved.len() {
            self.overruns.fetch_add(1, Ordering::Relaxed);
        }
        match self.tx.try_send(Chunk::Samples(interleaved[..take].to_vec())) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                self.overruns.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Reports that the device failed; the first failure is kept and recording ends.
    pub fn fail(&self, error: CaptureError) {
        let mut slot = self.failure.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some(error);
        }
        drop(slot);
        let _ = self.tx.try_send(Chunk::Wake);
    }
}

/// An open device. [`OpenCapture::stop`] (or dropping it) closes the device and drops its [`Frames`].
pub struct OpenCapture {
    format: InputFormat,
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl std::fmt::Debug for OpenCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenCapture").field("format", &self.format).finish()
    }
}

impl OpenCapture {
    /// An open device delivering `format`; `stop` closes it and must drop the [`Frames`] it was given.
    pub fn new(format: InputFormat, stop: impl FnOnce() + Send + 'static) -> OpenCapture {
        OpenCapture {
            format,
            stop: Some(Box::new(stop)),
        }
    }

    /// The format the device delivers.
    pub fn format(&self) -> InputFormat {
        self.format
    }

    /// Closes the device.
    pub fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

impl Drop for OpenCapture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

/// Opens the microphone. Implementations honour one contract: on success the device is open and delivers through `frames`
/// until [`OpenCapture::stop`]; on failure `frames` is dropped and nothing stays open; a device failure later is reported
/// through [`Frames::fail`].
pub trait Capture: Send + Sync {
    /// Opens the default input device.
    fn open(&self, frames: Frames) -> Result<OpenCapture, CaptureError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_hand_over_drops_and_counts_instead_of_blocking() {
        let (frames, rx) = Frames::channel(2);
        for _ in 0..5 {
            frames.push(&[0.1; 4]);
        }
        assert_eq!(rx.overruns.load(Ordering::Relaxed), 3);
        assert_eq!(rx.rx.try_iter().count(), 2);
        frames.push(&[]);
        assert_eq!(rx.rx.try_iter().count(), 0, "an empty chunk is not sent");
    }

    #[test]
    fn an_over_long_chunk_is_cut_and_counted() {
        let (frames, rx) = Frames::channel(2);
        frames.push(&vec![0.0; MAX_CHUNK_SAMPLES + 1]);
        assert_eq!(rx.overruns.load(Ordering::Relaxed), 1);
        match rx.rx.try_recv().unwrap() {
            Chunk::Samples(v) => assert_eq!(v.len(), MAX_CHUNK_SAMPLES),
            Chunk::Wake => panic!("samples expected"),
        }
    }

    #[test]
    fn the_first_failure_is_kept_even_when_the_hand_over_is_full() {
        let (frames, rx) = Frames::channel(1);
        frames.push(&[0.1]);
        frames.fail(CaptureError::Device("unplugged".into()));
        frames.fail(CaptureError::Denied);
        assert_eq!(*rx.failure.lock().unwrap(), Some(CaptureError::Device("unplugged".into())));
    }

    #[test]
    fn pushing_after_the_recorder_left_is_harmless() {
        let (frames, rx) = Frames::channel(1);
        drop(rx);
        frames.push(&[0.1]);
        frames.fail(CaptureError::Denied);
    }

    #[test]
    fn dropping_an_open_capture_stops_it_once() {
        let n = Arc::new(AtomicU64::new(0));
        let m = n.clone();
        let open = OpenCapture::new(InputFormat::new(16_000, 1).unwrap(), move || {
            m.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(open.format().sample_rate(), 16_000);
        drop(open);
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }
}

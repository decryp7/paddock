//! The capture port: how the recorder opens a microphone without knowing which one or how (rule 7: tests never touch a device).
//!
//! An adapter delivers the device's interleaved `f32` frames through [`Frames`], which never blocks the audio thread: the
//! hand-over is a bounded channel, and a chunk that does not fit is dropped and counted (an overrun), never queued without
//! limit.

use pitwall_core::speech::pcm::InputFormat;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};

/// How many chunks wait between the audio thread and the recorder, and how many buffers the hand-over owns: devices deliver
/// 5 to 100 ms a chunk, so a few hundred milliseconds of slack at the usual 10 ms (judgement).
pub const CHANNEL_CHUNKS: usize = 32;

/// The largest chunk taken, in samples (about 0.7 s of 48 kHz stereo; devices deliver about 10 ms). A larger callback is
/// cut there, at a whole frame, and counted as an overrun.
pub const MAX_CHUNK_SAMPLES: usize = 65_536;

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

/// A device sample as `f32` in ±1: signed integers by their full scale, unsigned ones around their midpoint.
pub trait ToF32: Copy {
    /// The sample as a float in ±1.
    fn to_f32(self) -> f32;
}

impl ToF32 for f32 {
    fn to_f32(self) -> f32 {
        self
    }
}

impl ToF32 for f64 {
    fn to_f32(self) -> f32 {
        self as f32
    }
}

impl ToF32 for i8 {
    fn to_f32(self) -> f32 {
        f32::from(self) / 128.0
    }
}

impl ToF32 for i16 {
    fn to_f32(self) -> f32 {
        f32::from(self) / 32_768.0
    }
}

impl ToF32 for i32 {
    fn to_f32(self) -> f32 {
        (f64::from(self) / 2_147_483_648.0) as f32
    }
}

impl ToF32 for u8 {
    fn to_f32(self) -> f32 {
        (f32::from(self) - 128.0) / 128.0
    }
}

impl ToF32 for u16 {
    fn to_f32(self) -> f32 {
        (f32::from(self) - 32_768.0) / 32_768.0
    }
}

/// What travels from the audio thread to the recorder.
pub(crate) enum Chunk {
    /// Interleaved samples, in one of the hand-over's own buffers (the recorder gives it back).
    Samples(Vec<f32>),
    /// Look at the failure slot.
    Wake,
}

impl std::fmt::Debug for Chunk {
    // Never the samples: they are the user's voice.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Chunk::Samples(v) => f.debug_struct("Samples").field("len", &v.len()).finish(),
            Chunk::Wake => f.write_str("Wake"),
        }
    }
}

/// The recorder's half before the device is open: an adapter turns it into [`Frames`] once it knows the device's format, so
/// a hand-over always knows its channel count.
#[derive(Debug)]
pub struct FrameSink {
    tx: SyncSender<Chunk>,
    pool: Receiver<Vec<f32>>,
    recycle: SyncSender<Vec<f32>>,
    overruns: Arc<AtomicU64>,
    failure: Arc<Mutex<Option<CaptureError>>>,
}

impl FrameSink {
    /// The hand-over for `format`. Its buffers are allocated here, on the opening thread, never on the audio thread.
    pub fn frames(self, format: InputFormat) -> Frames {
        let channels = usize::from(format.channels());
        let max_chunk = MAX_CHUNK_SAMPLES / channels * channels;
        for _ in 0..CHANNEL_CHUNKS {
            let _ = self.recycle.try_send(Vec::with_capacity(max_chunk));
        }
        Frames {
            tx: self.tx,
            pool: self.pool,
            max_chunk,
            overruns: self.overruns,
            failure: self.failure,
        }
    }
}

/// The audio thread's end of the hand-over. Dropping it (with the stream that owns it) tells the recorder the device closed.
///
/// It owns a fixed set of buffers that circulate between the audio thread and the recorder, so a callback neither blocks nor
/// allocates: it fills a free buffer and sends it; with no free buffer, or a full channel, the chunk is dropped and counted
/// as an overrun.
#[derive(Debug)]
pub struct Frames {
    tx: SyncSender<Chunk>,
    pool: Receiver<Vec<f32>>,
    max_chunk: usize,
    overruns: Arc<AtomicU64>,
    failure: Arc<Mutex<Option<CaptureError>>>,
}

/// The recorder's end: the chunks, where to give buffers back, the overrun count and the failure slot.
#[derive(Debug)]
pub(crate) struct FramesRx {
    pub(crate) rx: Receiver<Chunk>,
    pub(crate) recycle: SyncSender<Vec<f32>>,
    pub(crate) overruns: Arc<AtomicU64>,
    pub(crate) failure: Arc<Mutex<Option<CaptureError>>>,
}

impl FramesRx {
    /// Gives a buffer back to the audio thread.
    pub(crate) fn give_back(&self, mut buffer: Vec<f32>) {
        buffer.clear();
        let _ = self.recycle.try_send(buffer);
    }
}

impl Frames {
    /// A hand-over of `capacity` chunks and buffers; the adapter makes the [`Frames`] from the sink.
    pub(crate) fn channel(capacity: usize) -> (FrameSink, FramesRx) {
        let (tx, rx) = sync_channel(capacity);
        let (recycle, pool) = sync_channel(capacity);
        let overruns = Arc::new(AtomicU64::new(0));
        let failure = Arc::new(Mutex::new(None));
        (
            FrameSink {
                tx,
                pool,
                recycle: recycle.clone(),
                overruns: overruns.clone(),
                failure: failure.clone(),
            },
            FramesRx {
                rx,
                recycle,
                overruns,
                failure,
            },
        )
    }

    /// The most samples one chunk carries: [`MAX_CHUNK_SAMPLES`] rounded down to whole frames.
    pub fn max_chunk(&self) -> usize {
        self.max_chunk
    }

    /// Converts a callback's interleaved samples into a free buffer and hands it over. Never blocks and never allocates; a
    /// chunk over [`Frames::max_chunk`] is cut at a whole frame, and one that finds no free buffer or a full channel is dropped;
    /// both count as overruns.
    pub fn push<T: ToF32>(&self, interleaved: &[T]) {
        if interleaved.is_empty() {
            return;
        }
        let take = interleaved.len().min(self.max_chunk);
        if take < interleaved.len() {
            self.overruns.fetch_add(1, Ordering::Relaxed);
        }
        let Ok(mut buffer) = self.pool.try_recv() else {
            self.overruns.fetch_add(1, Ordering::Relaxed);
            return;
        };
        buffer.clear();
        // Within the buffer's capacity (max_chunk), so this does not reallocate.
        buffer.extend(interleaved[..take].iter().map(|s| s.to_f32()));
        match self.tx.try_send(Chunk::Samples(buffer)) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                self.overruns.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Reports that the device failed; the first failure is kept and recording ends.
    pub fn fail(&self, error: CaptureError) {
        self.failer().fail(error);
    }

    /// A handle that only reports failures, for a stream's error callback (the data callback owns the [`Frames`]).
    pub fn failer(&self) -> FrameFailer {
        FrameFailer {
            tx: self.tx.clone(),
            failure: self.failure.clone(),
        }
    }
}

/// Reports a device failure to the recorder (see [`Frames::failer`]).
#[derive(Debug, Clone)]
pub struct FrameFailer {
    tx: SyncSender<Chunk>,
    failure: Arc<Mutex<Option<CaptureError>>>,
}

impl FrameFailer {
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

/// Opens the microphone. Implementations honour one contract: on success the device is open and delivers through the
/// [`Frames`] made from `sink` with its format, until [`OpenCapture::stop`]; on failure the sink is dropped and nothing stays
/// open; a device failure later is reported through [`Frames::fail`].
pub trait Capture: Send + Sync {
    /// Opens the default input device.
    fn open(&self, sink: FrameSink) -> Result<OpenCapture, CaptureError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(capacity: usize, channels: u16) -> (Frames, FramesRx) {
        let (sink, rx) = Frames::channel(capacity);
        (sink.frames(InputFormat::new(16_000, channels).unwrap()), rx)
    }

    fn samples(c: Chunk) -> Vec<f32> {
        match c {
            Chunk::Samples(v) => v,
            Chunk::Wake => panic!("samples expected"),
        }
    }

    #[test]
    fn a_full_hand_over_drops_and_counts_instead_of_blocking() {
        let (frames, rx) = frames(2, 1);
        for _ in 0..5 {
            frames.push(&[0.1_f32; 4]);
        }
        assert_eq!(rx.overruns.load(Ordering::Relaxed), 3);
        assert_eq!(rx.rx.try_iter().count(), 2);
        frames.push::<f32>(&[]);
        assert_eq!(rx.rx.try_iter().count(), 0, "an empty chunk is not sent");
    }

    #[test]
    fn buffers_circulate_so_the_audio_thread_never_allocates() {
        let (frames, rx) = frames(2, 1);
        frames.push(&[0.5_f32; 8]);
        frames.push(&[0.5_f32; 8]);
        let first = samples(rx.rx.try_recv().unwrap());
        let (ptr, cap) = (first.as_ptr(), first.capacity());
        assert_eq!(cap, frames.max_chunk(), "allocated when the device opened, at full size");
        rx.give_back(first);
        let _ = samples(rx.rx.try_recv().unwrap());
        frames.push(&[0.25_f32; 8]);
        let reused = samples(rx.rx.try_recv().unwrap());
        assert_eq!((reused.as_ptr(), reused.capacity()), (ptr, cap), "the same buffer came back");
        assert_eq!(reused, vec![0.25; 8]);
        // No buffer free: the chunk is dropped, not allocated.
        frames.push(&[0.1_f32; 8]);
        assert_eq!(rx.overruns.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn an_over_long_chunk_is_cut_at_a_whole_frame_and_counted() {
        let (frames, rx) = frames(2, 3);
        assert_eq!(frames.max_chunk(), 65_535, "65,536 rounded down to frames of 3");
        frames.push(&vec![0.0_f32; MAX_CHUNK_SAMPLES + 9]);
        assert_eq!(rx.overruns.load(Ordering::Relaxed), 1);
        assert_eq!(samples(rx.rx.try_recv().unwrap()).len(), 65_535);
    }

    #[test]
    fn integer_samples_are_converted_on_the_way() {
        let (frames, rx) = frames(2, 1);
        frames.push(&[i16::MIN, 0, 16_384]);
        assert_eq!(samples(rx.rx.try_recv().unwrap()), vec![-1.0, 0.0, 0.5]);
    }

    #[test]
    fn every_integer_format_maps_to_plus_minus_one() {
        assert_eq!(i16::MIN.to_f32(), -1.0);
        assert_eq!(0_i16.to_f32(), 0.0);
        assert!((i16::MAX.to_f32() - 1.0).abs() < 1e-4);
        assert_eq!(i32::MIN.to_f32(), -1.0);
        assert_eq!(i8::MIN.to_f32(), -1.0);
        assert_eq!(0_u8.to_f32(), -1.0);
        assert_eq!(128_u8.to_f32(), 0.0);
        assert_eq!(0_u16.to_f32(), -1.0);
        assert_eq!(32_768_u16.to_f32(), 0.0);
        assert_eq!(0.5_f64.to_f32(), 0.5);
        assert_eq!((-0.25_f32).to_f32(), -0.25);
    }

    #[test]
    fn a_chunk_never_shows_its_samples_in_debug_output() {
        let d = format!("{:?}", Chunk::Samples(vec![0.123_456; 3]));
        assert_eq!(d, "Samples { len: 3 }");
        assert!(!d.contains("0.123"));
        assert_eq!(format!("{:?}", Chunk::Wake), "Wake");
    }

    #[test]
    fn the_first_failure_is_kept_even_when_the_hand_over_is_full() {
        let (frames, rx) = frames(1, 1);
        frames.push(&[0.1_f32]);
        frames.fail(CaptureError::Device("unplugged".into()));
        frames.fail(CaptureError::Denied);
        assert_eq!(*rx.failure.lock().unwrap(), Some(CaptureError::Device("unplugged".into())));
    }

    #[test]
    fn pushing_after_the_recorder_left_is_harmless() {
        let (frames, rx) = frames(1, 1);
        drop(rx);
        frames.push(&[0.1_f32]);
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

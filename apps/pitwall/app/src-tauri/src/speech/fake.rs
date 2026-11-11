//! The test double of [`Capture`] (compiled only for tests and `test-support`, rule 73): a scripted device that delivers its
//! chunks when opened, can fail to open or fail while open, and tells the test whether it is open. No microphone is touched.

use super::capture::{Capture, CaptureError, FrameSink, OpenCapture, CHANNEL_CHUNKS};
use pitwall_core::speech::pcm::InputFormat;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Debug, Default)]
struct State {
    open: bool,
}

#[derive(Debug)]
struct Inner {
    rate: u32,
    channels: u16,
    chunks: Vec<Vec<f32>>,
    open_error: Option<CaptureError>,
    then_fail: Option<CaptureError>,
    state: Mutex<State>,
    changed: Condvar,
}

/// A scripted microphone (cheap to clone; clones share their state).
#[derive(Debug, Clone)]
pub struct FakeCapture {
    inner: Arc<Inner>,
}

impl FakeCapture {
    /// A device of `rate` Hz and `channels` that delivers `chunks` (at most the hand-over's 32) as soon as it is opened, then
    /// stays open, silent, until stopped.
    pub fn new(rate: u32, channels: u16, chunks: Vec<Vec<f32>>) -> FakeCapture {
        assert!(
            chunks.len() <= CHANNEL_CHUNKS,
            "a script fits the hand-over, so no chunk is dropped"
        );
        FakeCapture::build(rate, channels, chunks, None, None)
    }

    /// A device that cannot be opened.
    pub fn failing(error: CaptureError) -> FakeCapture {
        FakeCapture::build(16_000, 1, vec![], Some(error), None)
    }

    /// The same device, failing with `error` after its chunks.
    pub fn then_fail(self, error: CaptureError) -> FakeCapture {
        let i = &self.inner;
        FakeCapture::build(i.rate, i.channels, i.chunks.clone(), i.open_error.clone(), Some(error))
    }

    fn build(
        rate: u32,
        channels: u16,
        chunks: Vec<Vec<f32>>,
        open_error: Option<CaptureError>,
        then_fail: Option<CaptureError>,
    ) -> FakeCapture {
        FakeCapture {
            inner: Arc::new(Inner {
                rate,
                channels,
                chunks,
                open_error,
                then_fail,
                state: Mutex::new(State::default()),
                changed: Condvar::new(),
            }),
        }
    }

    /// Whether the device is open.
    pub fn is_open(&self) -> bool {
        self.inner.state.lock().map(|s| s.open).unwrap_or(false)
    }

    /// Waits until the device is closed, at most 10 s (ER-32: never an unbounded wait); panics after that.
    pub fn wait_closed(&self) {
        let guard = self.inner.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (s, timeout) = self
            .inner
            .changed
            .wait_timeout_while(guard, Duration::from_secs(10), |s| s.open)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(!timeout.timed_out() && !s.open, "the fake microphone was not closed within 10 s");
    }
}

impl Capture for FakeCapture {
    fn open(&self, sink: FrameSink) -> Result<OpenCapture, CaptureError> {
        if let Some(e) = &self.inner.open_error {
            return Err(e.clone());
        }
        let format = InputFormat::new(self.inner.rate, self.inner.channels).map_err(|e| CaptureError::Format(e.to_string()))?;
        let frames = sink.frames(format);
        if let Ok(mut s) = self.inner.state.lock() {
            s.open = true;
        }
        for c in &self.inner.chunks {
            frames.push(c.as_slice());
        }
        if let Some(e) = &self.inner.then_fail {
            frames.fail(e.clone());
        }
        let inner = self.inner.clone();
        let held = Mutex::new(Some(frames));
        Ok(OpenCapture::new(format, move || {
            // Closing drops the hand-over, as a real stream does with its callback.
            drop(held.lock().ok().and_then(|mut f| f.take()));
            if let Ok(mut s) = inner.state.lock() {
                s.open = false;
            }
            inner.changed.notify_all();
        }))
    }
}

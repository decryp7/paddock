//! The system's microphone through `cpal` (Apache-2.0; CoreAudio here, WASAPI and ALSA in the next slice).
//!
//! A `cpal::Stream` is not `Send` on every platform, so each recording gets its own thread that builds, plays and finally drops
//! the stream; [`OpenCapture::stop`] tells that thread to end and joins it. The data callback only converts the device's
//! samples to `f32` into one of the hand-over's own buffers (`Frames::push`), which never blocks or allocates. Opening uses
//! the default input device and its default configuration, as macOS Dictation and Windows voice typing do; a device picker
//! is not in 1.1.
//!
//! No test opens a device (ER-14): the conversions are tested with the hand-over (`capture.rs`) and the adapter is checked by
//! hand on the user's Mac (ADR 0023 "Testing": a manual row with a verified precondition, rule 71).

use super::capture::Capture;
#[cfg(not(target_os = "macos"))]
use super::capture::{CaptureError, FrameSink, OpenCapture};
use std::sync::Arc;

/// The microphone of this system, or, where no adapter is built yet, a capture that refuses with
/// [`CaptureError::Unsupported`].
pub fn system_capture() -> Arc<dyn Capture> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(cpal_adapter::CpalCapture)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Arc::new(Unsupported)
    }
}

/// The capture of a system without an adapter yet.
#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Copy)]
struct Unsupported;

#[cfg(not(target_os = "macos"))]
impl Capture for Unsupported {
    fn open(&self, _sink: FrameSink) -> Result<OpenCapture, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
mod cpal_adapter {
    use crate::speech::capture::{Capture, CaptureError, FrameSink, Frames, OpenCapture, ToF32};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{ErrorKind, SampleFormat, SizedSample};
    use pitwall_core::speech::pcm::InputFormat;
    use std::sync::mpsc;

    /// The default input device through cpal's default host.
    #[derive(Debug, Clone, Copy)]
    pub(super) struct CpalCapture;

    fn error(e: &cpal::Error) -> CaptureError {
        match e.kind() {
            ErrorKind::PermissionDenied => CaptureError::Denied,
            ErrorKind::DeviceNotAvailable => CaptureError::NoDevice,
            ErrorKind::UnsupportedConfig => CaptureError::Format(e.to_string()),
            _ => CaptureError::Device(e.to_string()),
        }
    }

    fn stream<T: SizedSample + ToF32 + Send + 'static>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        frames: Frames,
    ) -> Result<cpal::Stream, cpal::Error> {
        let on_error = frames.failer();
        device.build_input_stream::<T, _, _>(
            config,
            // Converts into one of the hand-over's own buffers: no lock, no allocation, no wait on the audio thread.
            move |data: &[T], _: &cpal::InputCallbackInfo| frames.push(data),
            move |e: cpal::Error| match e.kind() {
                // A glitch or a reroute to the new default device: recording goes on.
                ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => {}
                _ => on_error.fail(error(&e)),
            },
            None,
        )
    }

    fn build(sink: FrameSink) -> Result<(cpal::Stream, InputFormat), CaptureError> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(CaptureError::NoDevice)?;
        let supported = device.default_input_config().map_err(|e| error(&e))?;
        let format = InputFormat::new(supported.sample_rate(), supported.channels()).map_err(|e| CaptureError::Format(e.to_string()))?;
        let frames = sink.frames(format);
        let config = supported.config();
        let stream = match supported.sample_format() {
            SampleFormat::F32 => stream::<f32>(&device, config, frames),
            SampleFormat::F64 => stream::<f64>(&device, config, frames),
            SampleFormat::I8 => stream::<i8>(&device, config, frames),
            SampleFormat::I16 => stream::<i16>(&device, config, frames),
            SampleFormat::I32 => stream::<i32>(&device, config, frames),
            SampleFormat::U8 => stream::<u8>(&device, config, frames),
            SampleFormat::U16 => stream::<u16>(&device, config, frames),
            other => return Err(CaptureError::Format(format!("{other} samples"))),
        }
        .map_err(|e| error(&e))?;
        stream.play().map_err(|e| error(&e))?;
        Ok((stream, format))
    }

    impl Capture for CpalCapture {
        fn open(&self, sink: FrameSink) -> Result<OpenCapture, CaptureError> {
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let (stop_tx, stop_rx) = mpsc::channel::<()>();
            let thread = std::thread::Builder::new()
                .name("pitwall-speech-device".into())
                .spawn(move || match build(sink) {
                    Ok((stream, format)) => {
                        let _ = ready_tx.send(Ok(format));
                        // Until stopped (or the recorder is gone); dropping the stream closes the device and its Frames.
                        let _ = stop_rx.recv();
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                })
                .map_err(|e| CaptureError::Device(format!("no audio thread: {e}")))?;
            // Opening a device is a short system call, not a job (the thread always answers before it waits).
            let format = match ready_rx.recv() {
                Ok(r) => r?,
                Err(_) => return Err(CaptureError::Device("the audio thread ended while opening".into())),
            };
            Ok(OpenCapture::new(format, move || {
                let _ = stop_tx.send(());
                let _ = thread.join();
            }))
        }
    }
}

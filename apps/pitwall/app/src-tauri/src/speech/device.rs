//! The system's microphone through `cpal` (Apache-2.0; CoreAudio here, WASAPI and ALSA in the next slice).
//!
//! A `cpal::Stream` is not `Send` on every platform, so each recording gets its own thread that builds, plays and finally drops
//! the stream; [`OpenCapture::stop`] tells that thread to end and joins it. The data callback only converts the device's
//! samples to `f32` and hands them to [`Frames`], which never blocks. Opening uses the default input device and its default
//! configuration, as macOS Dictation and Windows voice typing do; a device picker is not in 1.1.
//!
//! No test opens a device (ER-14): the conversions are tested here and the adapter is checked by hand on the user's Mac
//! (ADR 0023 "Testing": a manual row with a verified precondition, rule 71).

use super::capture::{Capture, Frames};
#[cfg(not(target_os = "macos"))]
use super::capture::{CaptureError, OpenCapture};
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
    fn open(&self, _frames: Frames) -> Result<OpenCapture, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

/// Integer samples as `f32` in ±1: signed by their full scale, unsigned around their midpoint.
pub(crate) trait ToF32: Copy {
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

/// Converts one callback's samples into `scratch` (reused, so the audio thread does not allocate for it) and hands them over.
pub(crate) fn hand_over<T: ToF32>(frames: &Frames, data: &[T], scratch: &mut Vec<f32>) {
    scratch.clear();
    scratch.extend(data.iter().map(|s| s.to_f32()));
    frames.push(scratch);
}

#[cfg(target_os = "macos")]
mod cpal_adapter {
    use super::{hand_over, ToF32};
    use crate::speech::capture::{Capture, CaptureError, Frames, OpenCapture};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{ErrorKind, SampleFormat, SizedSample};
    use pitwall_core::speech::pcm::InputFormat;
    use std::sync::mpsc;
    use std::sync::Arc;

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
        frames: Arc<Frames>,
    ) -> Result<cpal::Stream, cpal::Error> {
        let on_error = frames.clone();
        let mut scratch = Vec::new();
        device.build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| hand_over(&frames, data, &mut scratch),
            move |e: cpal::Error| match e.kind() {
                // A glitch or a reroute to the new default device: recording goes on.
                ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => {}
                _ => on_error.fail(error(&e)),
            },
            None,
        )
    }

    fn build(frames: Frames) -> Result<(cpal::Stream, InputFormat), CaptureError> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(CaptureError::NoDevice)?;
        let supported = device.default_input_config().map_err(|e| error(&e))?;
        let format = InputFormat::new(supported.sample_rate(), supported.channels()).map_err(|e| CaptureError::Format(e.to_string()))?;
        let frames = Arc::new(frames);
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
        fn open(&self, frames: Frames) -> Result<OpenCapture, CaptureError> {
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let (stop_tx, stop_rx) = mpsc::channel::<()>();
            let thread = std::thread::Builder::new()
                .name("pitwall-speech-device".into())
                .spawn(move || match build(frames) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_callback_is_converted_and_handed_over() {
        let (frames, rx) = Frames::channel(4);
        let mut scratch = Vec::new();
        hand_over(&frames, &[i16::MIN, 0, 16_384], &mut scratch);
        match rx.rx.try_recv().unwrap() {
            crate::speech::capture::Chunk::Samples(v) => assert_eq!(v, vec![-1.0, 0.0, 0.5]),
            crate::speech::capture::Chunk::Wake => panic!("samples expected"),
        }
    }
}

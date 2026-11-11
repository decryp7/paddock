//! From a microphone's frames to a 16 kHz mono PCM16 clip: downmix, resample, convert, bound.
//!
//! The capture adapter hands over interleaved `f32` samples in the device's own rate and channel count, in chunks of any
//! size (a chunk may even end inside a frame). [`ClipBuilder`] keeps its state across chunks, so the result does not
//! depend on how the device cut the stream.
//!
//! Resampling is an area (box) average: each output sample is the mean of the input it covers, with fractional weights at
//! the edges. For the usual 44.1 and 48 kHz devices that is a decimating low-pass good enough for speech recognition
//! (judgement; Whisper's own front end works on 80 mel bands up to 8 kHz). Below 16 kHz it holds each input sample.

use super::{SpeechError, MAX_CLIP_SAMPLES, SAMPLE_RATE};

/// The lowest device rate accepted.
pub const MIN_INPUT_RATE: u32 = 8_000;
/// The highest device rate accepted.
pub const MAX_INPUT_RATE: u32 = 384_000;
/// The most channels accepted.
pub const MAX_INPUT_CHANNELS: u16 = 32;

/// What the device delivers: its sample rate and channel count, checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputFormat {
    sample_rate: u32,
    channels: u16,
}

impl InputFormat {
    /// A device format, refused outside 8–384 kHz or 1–32 channels.
    pub fn new(sample_rate: u32, channels: u16) -> Result<InputFormat, SpeechError> {
        if !(MIN_INPUT_RATE..=MAX_INPUT_RATE).contains(&sample_rate) {
            return Err(SpeechError::Format(format!("a sample rate of {sample_rate} Hz")));
        }
        if !(1..=MAX_INPUT_CHANNELS).contains(&channels) {
            return Err(SpeechError::Format(format!("{channels} channels")));
        }
        Ok(InputFormat { sample_rate, channels })
    }

    /// The sample rate in Hz.
    pub fn sample_rate(self) -> u32 {
        self.sample_rate
    }

    /// The channel count.
    pub fn channels(self) -> u16 {
        self.channels
    }
}

/// A recorded clip: 16 kHz mono PCM16, at most [`MAX_CLIP_SAMPLES`] samples.
#[derive(Clone, PartialEq, Eq)]
pub struct Clip {
    samples: Vec<i16>,
}

impl std::fmt::Debug for Clip {
    // Never the samples: a clip is the user's voice (no user audio in logs).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clip").field("duration_ms", &self.duration_ms()).finish()
    }
}

impl Clip {
    /// A clip from 16 kHz mono samples; refused over the length limit.
    pub fn from_samples(samples: Vec<i16>) -> Result<Clip, SpeechError> {
        if samples.len() > MAX_CLIP_SAMPLES {
            return Err(SpeechError::TooLong);
        }
        Ok(Clip { samples })
    }

    /// The samples.
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    /// Whether it holds no sample.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Its length in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.samples.len() as u64 * 1000 / u64::from(SAMPLE_RATE)
    }

    /// The clip without its leading and trailing silence, keeping [`SILENCE_PAD_MS`] around the speech; empty when no
    /// 20 ms frame is louder than [`SILENCE_RMS`]. Whisper tends to invent words for silence ("Thank you."), so a silent
    /// clip is never sent (ADR 0023, "Parallelism and cost": trim by a simple energy check first).
    pub fn trimmed(&self) -> Clip {
        let frame = (SAMPLE_RATE / 50) as usize;
        let loud = |chunk: &[i16]| {
            let sum: f64 = chunk.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
            (sum / chunk.len() as f64).sqrt() > f64::from(SILENCE_RMS)
        };
        let frames: Vec<bool> = self.samples.chunks(frame).map(loud).collect();
        let (Some(first), Some(last)) = (frames.iter().position(|&l| l), frames.iter().rposition(|&l| l)) else {
            return Clip { samples: Vec::new() };
        };
        let pad = (SAMPLE_RATE as u64 * SILENCE_PAD_MS / 1000) as usize;
        let start = (first * frame).saturating_sub(pad);
        let end = ((last + 1) * frame + pad).min(self.samples.len());
        Clip {
            samples: self.samples[start..end].to_vec(),
        }
    }
}

/// The RMS level (of 32,767) at or below which a 20 ms frame counts as silence: about -50 dBFS (judgement; a quiet room's
/// noise floor on a laptop microphone is lower, quiet speech is higher).
pub const SILENCE_RMS: i16 = 100;
/// How much silence [`Clip::trimmed`] keeps before and after the speech, so a soft first or last syllable is not cut.
pub const SILENCE_PAD_MS: u64 = 250;

/// Whether a chunk was taken whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    /// Taken; the clip is below its limit.
    Taking,
    /// The clip reached its limit; this chunk was taken up to it and later ones are dropped. Stop recording.
    Full,
}

/// Builds a [`Clip`] from a device's interleaved `f32` frames (see the module documentation).
#[derive(Debug)]
pub struct ClipBuilder {
    channels: usize,
    /// Input samples per output sample.
    ratio: f64,
    /// The channel the next interleaved value belongs to.
    channel: usize,
    /// The sum of the current frame's channels.
    frame_sum: f64,
    /// The weighted sum of the current output window.
    acc: f64,
    /// How much input (in input samples) the current output window holds.
    filled: f64,
    cap: usize,
    out: Vec<i16>,
}

impl ClipBuilder {
    /// A builder for `format`, holding at most `max_samples` output samples (capped at [`MAX_CLIP_SAMPLES`]).
    pub fn new(format: InputFormat, max_samples: usize) -> ClipBuilder {
        ClipBuilder {
            channels: usize::from(format.channels),
            ratio: f64::from(format.sample_rate) / f64::from(SAMPLE_RATE),
            channel: 0,
            frame_sum: 0.0,
            acc: 0.0,
            filled: 0.0,
            cap: max_samples.min(MAX_CLIP_SAMPLES),
            out: Vec::new(),
        }
    }

    /// Takes a chunk of interleaved samples. Values that are not finite count as silence; others are clamped to ±1.
    pub fn push(&mut self, interleaved: &[f32]) -> Push {
        for &v in interleaved {
            if self.out.len() >= self.cap {
                return Push::Full;
            }
            let v = if v.is_finite() { f64::from(v.clamp(-1.0, 1.0)) } else { 0.0 };
            self.frame_sum += v;
            self.channel += 1;
            if self.channel == self.channels {
                let mono = self.frame_sum / self.channels as f64;
                self.channel = 0;
                self.frame_sum = 0.0;
                self.resample(mono);
            }
        }
        if self.out.len() >= self.cap {
            Push::Full
        } else {
            Push::Taking
        }
    }

    fn resample(&mut self, x: f64) {
        let mut left = 1.0_f64;
        while left > 0.0 && self.out.len() < self.cap {
            let take = left.min(self.ratio - self.filled);
            self.acc += x * take;
            self.filled += take;
            left -= take;
            // A tolerance for the rounding of fractional ratios (44.1 kHz), far below one sample.
            if self.filled >= self.ratio - 1e-9 {
                self.out.push(to_i16(self.acc / self.ratio));
                self.acc = 0.0;
                self.filled = 0.0;
            }
        }
    }

    /// Output samples so far.
    pub fn len(&self) -> usize {
        self.out.len()
    }

    /// Whether nothing was recorded yet.
    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    /// Milliseconds recorded so far.
    pub fn duration_ms(&self) -> u64 {
        self.out.len() as u64 * 1000 / u64::from(SAMPLE_RATE)
    }

    /// The clip. A partly filled last output window is dropped (under one sample of 16 kHz).
    pub fn finish(self) -> Clip {
        Clip { samples: self.out }
    }
}

fn to_i16(x: f64) -> i16 {
    // `x` is within ±1, so the product is within i16's range; `as` saturates in any case.
    (x * f64::from(i16::MAX)).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn fmt(rate: u32, channels: u16) -> InputFormat {
        InputFormat::new(rate, channels).unwrap()
    }

    #[test]
    fn refuses_formats_outside_the_bounds() {
        assert!(InputFormat::new(7_999, 1).is_err());
        assert!(InputFormat::new(384_001, 1).is_err());
        assert!(InputFormat::new(48_000, 0).is_err());
        assert!(InputFormat::new(48_000, 33).is_err());
        let e = InputFormat::new(4_000, 2).unwrap_err().to_string();
        assert!(e.contains("4000 Hz"), "{e}");
        assert_eq!(fmt(48_000, 2).channels(), 2);
    }

    #[test]
    fn a_second_of_48_khz_stereo_is_a_second_of_16_khz_mono() {
        let mut b = ClipBuilder::new(fmt(48_000, 2), MAX_CLIP_SAMPLES);
        let second = vec![0.5_f32; 48_000 * 2];
        assert_eq!(b.push(&second), Push::Taking);
        assert_eq!(b.len(), 16_000);
        assert_eq!(b.duration_ms(), 1000);
        let clip = b.finish();
        assert!(
            clip.samples().iter().all(|&s| s == 16_384),
            "0.5 is 16,384 of 32,767 after rounding"
        );
    }

    #[test]
    fn downmix_averages_the_channels() {
        let mut b = ClipBuilder::new(fmt(16_000, 2), MAX_CLIP_SAMPLES);
        b.push(&[1.0, 0.0, -1.0, 0.0, 1.0, 1.0]);
        assert_eq!(b.finish().samples(), &[16_384, -16_384, 32_767]);
    }

    #[test]
    fn a_fractional_rate_keeps_its_length() {
        let mut b = ClipBuilder::new(fmt(44_100, 1), MAX_CLIP_SAMPLES);
        for _ in 0..10 {
            b.push(&vec![0.25_f32; 4_410]);
        }
        // 44,100 input samples are exactly 16,000 output samples, give or take the open last window.
        assert!((15_999..=16_000).contains(&b.len()), "{}", b.len());
    }

    #[test]
    fn an_8_khz_device_is_held_up_to_16_khz() {
        let mut b = ClipBuilder::new(fmt(8_000, 1), MAX_CLIP_SAMPLES);
        b.push(&[0.5, -0.5]);
        assert_eq!(b.finish().samples(), &[16_384, 16_384, -16_384, -16_384]);
    }

    #[test]
    fn chunks_may_split_a_frame() {
        let whole = {
            let mut b = ClipBuilder::new(fmt(48_000, 3), MAX_CLIP_SAMPLES);
            b.push(&(0..900).map(|i| (i as f32 / 900.0) - 0.5).collect::<Vec<_>>());
            b.finish()
        };
        let split = {
            let mut b = ClipBuilder::new(fmt(48_000, 3), MAX_CLIP_SAMPLES);
            let v: Vec<f32> = (0..900).map(|i| (i as f32 / 900.0) - 0.5).collect();
            for c in v.chunks(7) {
                b.push(c);
            }
            b.finish()
        };
        assert_eq!(whole, split);
    }

    #[test]
    fn stops_at_its_cap_and_says_so() {
        let mut b = ClipBuilder::new(fmt(16_000, 1), 10);
        assert_eq!(b.push(&[0.1; 6]), Push::Taking);
        assert_eq!(b.push(&[0.1; 6]), Push::Full);
        assert_eq!(b.push(&[0.1; 6]), Push::Full);
        assert_eq!(b.len(), 10);
    }

    #[test]
    fn the_cap_never_exceeds_five_minutes() {
        let b = ClipBuilder::new(fmt(16_000, 1), usize::MAX);
        assert_eq!(b.cap, MAX_CLIP_SAMPLES);
        assert_eq!(MAX_CLIP_SAMPLES, 4_800_000);
        assert!(Clip::from_samples(vec![0; MAX_CLIP_SAMPLES + 1]).is_err());
    }

    #[test]
    fn values_that_are_not_finite_are_silence_and_loud_ones_are_clamped() {
        let mut b = ClipBuilder::new(fmt(16_000, 1), MAX_CLIP_SAMPLES);
        b.push(&[f32::NAN, f32::INFINITY, 7.0, -7.0]);
        assert_eq!(b.finish().samples(), &[0, 0, 32_767, -32_767]);
    }

    #[test]
    fn a_clip_never_shows_its_samples_in_debug_output() {
        let c = Clip::from_samples(vec![1234; 16_000]).unwrap();
        let d = format!("{c:?}");
        assert_eq!(d, "Clip { duration_ms: 1000 }");
    }

    #[test]
    fn trimming_keeps_the_speech_and_a_pad() {
        let mut s = vec![0_i16; 16_000];
        s.extend(vec![3_000_i16; 8_000]);
        s.extend(vec![0_i16; 16_000]);
        let t = Clip::from_samples(s).unwrap().trimmed();
        // 0.5 s of speech plus 250 ms on each side.
        assert_eq!(t.samples().len(), 8_000 + 2 * 4_000);
        assert_eq!(t.samples()[4_000], 3_000);
    }

    #[test]
    fn a_silent_clip_trims_to_nothing() {
        assert!(Clip::from_samples(vec![50; 32_000]).unwrap().trimmed().is_empty());
        assert!(Clip::from_samples(vec![]).unwrap().trimmed().is_empty());
    }

    proptest! {
        #[test]
        fn never_panics_and_never_passes_the_cap(
            rate in MIN_INPUT_RATE..=MAX_INPUT_RATE,
            channels in 1u16..=MAX_INPUT_CHANNELS,
            cap in 0usize..2_000,
            chunks in proptest::collection::vec(proptest::collection::vec(any::<f32>(), 0..300), 0..20),
        ) {
            let mut b = ClipBuilder::new(fmt(rate, channels), cap);
            for c in &chunks {
                b.push(c);
            }
            prop_assert!(b.len() <= cap);
            let clip = b.finish();
            let _ = clip.trimmed();
        }
    }
}

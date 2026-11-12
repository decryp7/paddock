//! A clip as a WAV file: RIFF, `fmt ` PCM, 16 kHz, mono, 16 bits, little-endian. The only audio format the speech engine is
//! sent (whisper-server reads WAV without `--convert`, which Pitwall never passes: it would run ffmpeg).

use super::pcm::Clip;
use super::SAMPLE_RATE;

/// The size of the header [`encode`] writes.
pub const HEADER_BYTES: usize = 44;

/// The clip as a WAV file.
pub fn encode(clip: &Clip) -> Vec<u8> {
    let samples = clip.samples();
    // At most MAX_CLIP_SAMPLES (4.8 million) samples, so the sizes fit in u32.
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(HEADER_BYTES + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // the fmt chunk's size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    #[test]
    fn writes_a_canonical_16_khz_mono_pcm16_header() {
        let clip = Clip::from_samples(vec![1, -2, 32_767]).unwrap();
        let w = encode(&clip);
        assert_eq!(w.len(), HEADER_BYTES + 6);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32_at(&w, 4), 36 + 6);
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32_at(&w, 16), 16);
        assert_eq!(&w[20..24], &[1, 0, 1, 0], "PCM, one channel");
        assert_eq!(u32_at(&w, 24), 16_000);
        assert_eq!(u32_at(&w, 28), 32_000);
        assert_eq!(&w[32..36], &[2, 0, 16, 0], "2 bytes a frame, 16 bits");
        assert_eq!(&w[36..40], b"data");
        assert_eq!(u32_at(&w, 40), 6);
        assert_eq!(&w[44..], &[1, 0, 0xFE, 0xFF, 0xFF, 0x7F]);
    }

    #[test]
    fn an_empty_clip_is_a_header_only() {
        let w = encode(&Clip::from_samples(vec![]).unwrap());
        assert_eq!(w.len(), HEADER_BYTES);
        assert_eq!(u32_at(&w, 40), 0);
    }

    #[test]
    fn five_minutes_fit_the_request_limit() {
        let w = encode(&Clip::from_samples(vec![0; crate::speech::MAX_CLIP_SAMPLES]).unwrap());
        assert_eq!(w.len(), HEADER_BYTES + 9_600_000);
        assert!((w.len() as u64) < crate::config::ServerSettings::default().max_request_bytes);
    }
}

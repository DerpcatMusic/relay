#![forbid(unsafe_code)]

//! Deterministic, dependency-free fixtures for RELAY tests.
//!
//! Audio buffers use frame-major interleaving: the sample for `(frame, channel)`
//! is stored at `frame * channels + channel`. Channel counts are always nonzero,
//! and every accepted buffer contains an integral number of complete frames.
//! Sample values are preserved exactly; non-finite `f32` values are permitted so
//! callers can test their own numeric validation.

use std::fmt;
use std::time::Duration;

/// A deterministic monotonic clock advanced explicitly by a test.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FakeClock {
    now: Duration,
}

impl FakeClock {
    /// Creates a clock starting at `Duration::ZERO`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            now: Duration::ZERO,
        }
    }

    /// Creates a clock starting at a caller-selected instant.
    #[must_use]
    pub const fn starting_at(now: Duration) -> Self {
        Self { now }
    }

    /// Returns the current fake monotonic time.
    #[must_use]
    pub const fn now(&self) -> Duration {
        self.now
    }

    /// Advances the clock and returns its new time.
    ///
    /// If the requested time cannot be represented, the clock is unchanged.
    pub fn advance(&mut self, elapsed: Duration) -> Result<Duration, ClockError> {
        let Some(next) = self.now.checked_add(elapsed) else {
            return Err(ClockError::Overflow);
        };
        self.now = next;
        Ok(next)
    }
}

/// Failure produced while advancing a [`FakeClock`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockError {
    /// The resulting [`Duration`] would exceed its representable range.
    Overflow,
}

impl fmt::Display for ClockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("fake clock advance would overflow Duration")
    }
}

impl std::error::Error for ClockError {}

/// A deterministic, frame-aligned source of interleaved `f32` samples.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioSource {
    channels: usize,
    samples: Vec<f32>,
    position_frames: usize,
}

impl AudioSource {
    /// Generates exactly `frames * channels` samples in frame-major order.
    ///
    /// `sample_at` is called once for every `(frame, channel)`, with frames and
    /// channels each visited in ascending order. This stable call order makes
    /// stateful deterministic generators reproducible.
    pub fn generate(
        channels: usize,
        frames: usize,
        mut sample_at: impl FnMut(usize, usize) -> f32,
    ) -> Result<Self, AudioError> {
        validate_channels(channels)?;
        let Some(sample_count) = frames.checked_mul(channels) else {
            return Err(AudioError::SampleCountOverflow { frames, channels });
        };

        let mut samples = Vec::new();
        samples
            .try_reserve_exact(sample_count)
            .map_err(|_| AudioError::CapacityExceeded {
                additional_samples: sample_count,
            })?;
        for frame in 0..frames {
            for channel in 0..channels {
                samples.push(sample_at(frame, channel));
            }
        }

        Ok(Self {
            channels,
            samples,
            position_frames: 0,
        })
    }

    /// Creates a source from an already interleaved, complete-frame buffer.
    pub fn from_interleaved(channels: usize, samples: Vec<f32>) -> Result<Self, AudioError> {
        validate_buffer(channels, samples.len())?;
        Ok(Self {
            channels,
            samples,
            position_frames: 0,
        })
    }

    /// Returns the nonzero number of channels in every frame.
    #[must_use]
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// Returns the total number of complete frames in this source.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }

    /// Returns the number of complete frames not yet read.
    #[must_use]
    pub fn remaining_frames(&self) -> usize {
        self.frames() - self.position_frames
    }

    /// Returns all generated samples without changing the read cursor.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Reads complete frames into `output` and returns the number of frames read.
    ///
    /// `output` must itself hold an integral number of frames. At end-of-source,
    /// only the returned prefix (`frames_read * channels` samples) is written;
    /// the remainder of `output` is left unchanged.
    pub fn read_interleaved(&mut self, output: &mut [f32]) -> Result<usize, AudioError> {
        validate_buffer(self.channels, output.len())?;
        let requested_frames = output.len() / self.channels;
        let frames_read = requested_frames.min(self.remaining_frames());
        let sample_count = frames_read * self.channels;
        let start = self.position_frames * self.channels;
        let end = start + sample_count;
        output[..sample_count].copy_from_slice(&self.samples[start..end]);
        self.position_frames += frames_read;
        Ok(frames_read)
    }

    /// Rewinds the read cursor to the first frame.
    pub const fn reset(&mut self) {
        self.position_frames = 0;
    }
}

/// A collector that appends complete frames of interleaved `f32` samples.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioSink {
    channels: usize,
    samples: Vec<f32>,
}

impl AudioSink {
    /// Creates an empty collector with a fixed, nonzero channel count.
    pub fn new(channels: usize) -> Result<Self, AudioError> {
        validate_channels(channels)?;
        Ok(Self {
            channels,
            samples: Vec::new(),
        })
    }

    /// Returns the fixed number of channels in every collected frame.
    #[must_use]
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// Returns the number of collected complete frames.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }

    /// Returns the collected samples in frame-major interleaved order.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Appends complete interleaved frames and returns their frame count.
    ///
    /// On validation failure, no samples are appended.
    pub fn write_interleaved(&mut self, input: &[f32]) -> Result<usize, AudioError> {
        validate_buffer(self.channels, input.len())?;
        self.samples
            .try_reserve(input.len())
            .map_err(|_| AudioError::CapacityExceeded {
                additional_samples: input.len(),
            })?;
        self.samples.extend_from_slice(input);
        Ok(input.len() / self.channels)
    }

    /// Looks up a sample by frame and channel without panicking.
    #[must_use]
    pub fn sample(&self, frame: usize, channel: usize) -> Option<f32> {
        if channel >= self.channels || frame >= self.frames() {
            return None;
        }
        self.samples.get(frame * self.channels + channel).copied()
    }

    /// Removes all samples while retaining the channel configuration.
    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

/// A violation of the interleaved-audio sample/channel/frame invariants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioError {
    /// Audio must contain at least one channel.
    ZeroChannels,
    /// A sample buffer ended in the middle of a frame.
    IncompleteFrame { samples: usize, channels: usize },
    /// The requested frame and channel dimensions cannot fit in `usize`.
    SampleCountOverflow { frames: usize, channels: usize },
    /// The requested sample storage cannot be represented or allocated.
    CapacityExceeded { additional_samples: usize },
}

impl fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroChannels => formatter.write_str("audio channel count must be nonzero"),
            Self::IncompleteFrame { samples, channels } => write!(
                formatter,
                "{samples} interleaved samples do not form complete {channels}-channel frames"
            ),
            Self::SampleCountOverflow { frames, channels } => write!(
                formatter,
                "{frames} frames with {channels} channels overflow the sample count"
            ),
            Self::CapacityExceeded { additional_samples } => write!(
                formatter,
                "cannot reserve storage for {additional_samples} additional audio samples"
            ),
        }
    }
}

impl std::error::Error for AudioError {}

fn validate_channels(channels: usize) -> Result<(), AudioError> {
    if channels == 0 {
        return Err(AudioError::ZeroChannels);
    }
    Ok(())
}

fn validate_buffer(channels: usize, samples: usize) -> Result<(), AudioError> {
    validate_channels(channels)?;
    if !samples.is_multiple_of(channels) {
        return Err(AudioError::IncompleteFrame { samples, channels });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_starts_at_zero_and_advances_deterministically() {
        let mut clock = FakeClock::new();
        assert_eq!(clock.now(), Duration::ZERO);
        assert_eq!(
            clock.advance(Duration::from_millis(5)),
            Ok(Duration::from_millis(5))
        );
        assert_eq!(
            clock.advance(Duration::from_millis(7)),
            Ok(Duration::from_millis(12))
        );
    }

    #[test]
    fn fake_clock_can_start_at_a_selected_time() {
        let clock = FakeClock::starting_at(Duration::from_secs(42));
        assert_eq!(clock.now(), Duration::from_secs(42));
    }

    #[test]
    fn overflowing_clock_advance_is_rejected_without_mutation() {
        let mut clock = FakeClock::starting_at(Duration::MAX);
        assert_eq!(
            clock.advance(Duration::from_nanos(1)),
            Err(ClockError::Overflow)
        );
        assert_eq!(clock.now(), Duration::MAX);
    }

    #[test]
    fn source_generation_is_frame_major_and_repeatable() {
        let first = AudioSource::generate(2, 3, |frame, channel| (frame * 10 + channel) as f32);
        let second = AudioSource::generate(2, 3, |frame, channel| (frame * 10 + channel) as f32);
        let Ok(first) = first else {
            panic!("valid dimensions")
        };
        let Ok(second) = second else {
            panic!("valid dimensions")
        };

        assert_eq!(first, second);
        assert_eq!(first.channels(), 2);
        assert_eq!(first.frames(), 3);
        assert_eq!(first.samples(), &[0.0, 1.0, 10.0, 11.0, 20.0, 21.0]);
    }

    #[test]
    fn source_reads_only_complete_available_frames_and_can_reset() {
        let source = AudioSource::from_interleaved(2, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        let Ok(mut source) = source else {
            panic!("complete stereo frames")
        };
        let mut first = [-1.0; 4];
        assert_eq!(source.read_interleaved(&mut first), Ok(2));
        assert_eq!(first, [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(source.remaining_frames(), 1);

        let mut last = [-1.0; 4];
        assert_eq!(source.read_interleaved(&mut last), Ok(1));
        assert_eq!(last, [4.0, 5.0, -1.0, -1.0]);
        assert_eq!(source.read_interleaved(&mut last), Ok(0));

        source.reset();
        assert_eq!(source.remaining_frames(), 3);
    }

    #[test]
    fn source_rejects_zero_channels_incomplete_frames_and_dimension_overflow() {
        assert_eq!(
            AudioSource::generate(0, 1, |_, _| 0.0),
            Err(AudioError::ZeroChannels)
        );
        assert_eq!(
            AudioSource::from_interleaved(2, vec![0.0, 1.0, 2.0]),
            Err(AudioError::IncompleteFrame {
                samples: 3,
                channels: 2,
            })
        );
        assert_eq!(
            AudioSource::generate(2, usize::MAX, |_, _| 0.0),
            Err(AudioError::SampleCountOverflow {
                frames: usize::MAX,
                channels: 2,
            })
        );
        assert_eq!(
            AudioSource::generate(1, usize::MAX, |_, _| 0.0),
            Err(AudioError::CapacityExceeded {
                additional_samples: usize::MAX,
            })
        );
    }

    #[test]
    fn empty_audio_is_zero_complete_frames() {
        let source = AudioSource::generate(2, 0, |_, _| panic!("no samples expected"));
        let Ok(mut source) = source else {
            panic!("zero complete frames are valid")
        };
        assert_eq!(source.frames(), 0);
        assert_eq!(source.read_interleaved(&mut []), Ok(0));

        let sink = AudioSink::new(2);
        let Ok(mut sink) = sink else {
            panic!("nonzero channels")
        };
        assert_eq!(sink.write_interleaved(&[]), Ok(0));
        assert_eq!(sink.frames(), 0);
    }

    #[test]
    fn source_rejects_a_partial_output_frame_before_changing_its_cursor() {
        let source = AudioSource::generate(2, 1, |frame, channel| (frame + channel) as f32);
        let Ok(mut source) = source else {
            panic!("valid dimensions")
        };
        assert_eq!(
            source.read_interleaved(&mut [0.0; 3]),
            Err(AudioError::IncompleteFrame {
                samples: 3,
                channels: 2,
            })
        );
        assert_eq!(source.remaining_frames(), 1);
    }

    #[test]
    fn sink_collects_frames_in_write_order_and_supports_indexing() {
        let sink = AudioSink::new(2);
        let Ok(mut sink) = sink else {
            panic!("nonzero channels")
        };
        assert_eq!(sink.write_interleaved(&[0.0, 1.0]), Ok(1));
        assert_eq!(sink.write_interleaved(&[2.0, 3.0, 4.0, 5.0]), Ok(2));
        assert_eq!(sink.channels(), 2);
        assert_eq!(sink.frames(), 3);
        assert_eq!(sink.samples(), &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(sink.sample(1, 0), Some(2.0));
        assert_eq!(sink.sample(2, 1), Some(5.0));
        assert_eq!(sink.sample(3, 0), None);
        assert_eq!(sink.sample(0, 2), None);
    }

    #[test]
    fn sink_rejects_invalid_writes_without_mutating_collected_audio() {
        assert_eq!(AudioSink::new(0), Err(AudioError::ZeroChannels));
        let sink = AudioSink::new(2);
        let Ok(mut sink) = sink else {
            panic!("nonzero channels")
        };
        assert_eq!(sink.write_interleaved(&[0.0, 1.0]), Ok(1));
        assert_eq!(
            sink.write_interleaved(&[2.0]),
            Err(AudioError::IncompleteFrame {
                samples: 1,
                channels: 2,
            })
        );
        assert_eq!(sink.samples(), &[0.0, 1.0]);
    }

    #[test]
    fn sink_preserves_all_f32_bit_patterns_and_clear_keeps_its_format() {
        let sink = AudioSink::new(1);
        let Ok(mut sink) = sink else {
            panic!("nonzero channels")
        };
        let samples = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.0];
        assert_eq!(sink.write_interleaved(&samples), Ok(4));
        for (actual, expected) in sink.samples().iter().zip(samples) {
            assert_eq!(actual.to_bits(), expected.to_bits());
        }

        sink.clear();
        assert_eq!(sink.channels(), 1);
        assert_eq!(sink.frames(), 0);
        assert!(sink.samples().is_empty());
    }
}

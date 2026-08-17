//! Worker-thread sample-rate conversion for RELAY.
//!
//! Construction and processing belong on a decode/resample worker, never on a
//! hard-real-time device callback. The [`WorkerResampler`] contract is only for
//! an infinite/live stream: it consumes complete chunks and deliberately keeps
//! filter history for the next call. Fixed-ratio finite media uses
//! [`FiniteFixedRatioConverter`]. A concrete [`AdaptiveClockConverter`] may
//! instead end its existing adaptive stream with
//! [`AdaptiveClockConverter::finish_interleaved`]. Both terminal operations are
//! allocation-free and expose exact leading/trailing trim metadata.
//!
//! All processing uses caller-owned, preallocated interleaved buffers. Rubato
//! objects and their scratch storage are allocated only during construction.

mod adaptive;
mod fixed;

use core::fmt;
use rubato::{Resampler, audioadapter_buffers::direct::InterleavedSlice};

pub use adaptive::{
    AdaptiveClockConfig, AdaptiveClockConverter, AdaptiveFinishReport, AdaptiveFinishRequirements,
    ClockCorrection, OutputInputRatioCorrectionPpm,
};
pub use fixed::{
    FiniteFixedRatioConverter, FiniteFrameRequirements, FiniteProcessReport, FixedRatioConverter,
};

/// Sample rates supported by the Phase-1 48 kHz media boundary.
pub const SUPPORTED_SAMPLE_RATES: [usize; 4] = [44_100, 48_000, 96_000, 192_000];

/// Buffer sizing and startup-delay information for the next processing call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRequirements {
    /// Number of interleaved channels.
    pub channels: usize,
    /// Input frames required by the next call.
    pub input_frames_next: usize,
    /// Largest input frame count this instance can require.
    pub input_frames_max: usize,
    /// Output frames expected from the next call.
    pub output_frames_next: usize,
    /// Output capacity required for every call.
    pub output_frames_max: usize,
    /// Algorithmic delay measured in output frames.
    pub output_delay: usize,
}

/// Frame counts actually consumed and written by a processing call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessReport {
    /// Frames consumed from each input channel.
    pub input_frames: usize,
    /// Frames written to each output channel.
    pub output_frames: usize,
}

/// Common worker-side interface for an infinite/live stream.
///
/// This trait has intentionally no finish or drain method. Ending a stream
/// through this interface abandons the last partial input chunk and retained
/// filter tail. Use [`FiniteFixedRatioConverter`] for fixed-ratio finite media,
/// or the concrete [`AdaptiveClockConverter::finish_interleaved`] operation to
/// finish an already-running adaptive finite stream.
pub trait WorkerResampler {
    /// Return sizing data for the next call and maximum reusable buffers.
    fn requirements(&self) -> FrameRequirements;

    /// Process one interleaved chunk without allocating.
    ///
    /// `input` must contain exactly `input_frames_next * channels` samples.
    /// `output` must have capacity for at least `output_frames_max * channels`
    /// samples. Only the prefix identified by [`ProcessReport::output_frames`]
    /// is initialized by this call.
    fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<ProcessReport, ResampleError>;

    /// Clear streaming history while retaining all allocations.
    fn reset(&mut self);
}

/// Configuration or processing failure.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum ResampleError {
    /// A sample rate was zero or not one of the Phase-1 supported rates.
    UnsupportedSampleRate { input: usize, output: usize },
    /// The Phase-1 seed only converts to or from the 48 kHz media rate.
    UnsupportedRatePair { input: usize, output: usize },
    /// Only mono and stereo are supported.
    UnsupportedChannelCount(usize),
    /// Chunk size must be nonzero.
    InvalidChunkFrames,
    /// Adaptive configuration was outside its safe numeric bounds.
    InvalidAdaptiveConfiguration,
    /// The input slice did not contain exactly the required interleaved samples.
    InvalidInputLength { expected: usize, actual: usize },
    /// The reusable output slice was smaller than the maximum requirement.
    OutputBufferTooSmall { required: usize, actual: usize },
    /// An input sample was NaN or infinite; the backend was not advanced.
    NonFiniteInput { sample_index: usize },
    /// A requested clock correction was NaN or infinite.
    NonFiniteClockCorrection,
    /// A ratio multiplier was non-positive.
    InvalidOutputInputRatioMultiplier,
    /// An interleaved finite buffer did not contain complete frames.
    InvalidInterleavedLength { channels: usize, actual: usize },
    /// Frame or sample count arithmetic exceeded the platform address space.
    FrameCountOverflow,
    /// Rubato rejected construction or processing parameters.
    Backend,
    /// Rubato produced a NaN or infinite sample.
    NonFiniteOutput { sample_index: usize },
    /// Terminal processing requires at least one, and at most one chunk, of valid input.
    InvalidValidInputFrames { valid: usize, maximum: usize },
    /// The adaptive stream has already ended or its terminal boundary is faulted.
    EndOfStream,
}

impl fmt::Display for ResampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ResampleError {}

pub(crate) fn checked_sample_len(frames: usize, channels: usize) -> Result<usize, ResampleError> {
    frames
        .checked_mul(channels)
        .ok_or(ResampleError::FrameCountOverflow)
}

pub(crate) fn validate_finite_samples(input: &[f32]) -> Result<(), ResampleError> {
    if let Some(sample_index) = input.iter().position(|sample| !sample.is_finite()) {
        Err(ResampleError::NonFiniteInput { sample_index })
    } else {
        Ok(())
    }
}

pub(crate) fn validate_configuration(
    input_rate: usize,
    output_rate: usize,
    channels: usize,
    chunk_frames: usize,
) -> Result<(), ResampleError> {
    if !SUPPORTED_SAMPLE_RATES.contains(&input_rate)
        || !SUPPORTED_SAMPLE_RATES.contains(&output_rate)
    {
        return Err(ResampleError::UnsupportedSampleRate {
            input: input_rate,
            output: output_rate,
        });
    }
    if input_rate != 48_000 && output_rate != 48_000 {
        return Err(ResampleError::UnsupportedRatePair {
            input: input_rate,
            output: output_rate,
        });
    }
    if !(1..=2).contains(&channels) {
        return Err(ResampleError::UnsupportedChannelCount(channels));
    }
    if chunk_frames == 0 {
        return Err(ResampleError::InvalidChunkFrames);
    }
    Ok(())
}

pub(crate) fn requirements<R: Resampler<f32>>(backend: &R, channels: usize) -> FrameRequirements {
    FrameRequirements {
        channels,
        input_frames_next: backend.input_frames_next(),
        input_frames_max: backend.input_frames_max(),
        output_frames_next: backend.output_frames_next(),
        output_frames_max: backend.output_frames_max(),
        output_delay: backend.output_delay(),
    }
}

pub(crate) fn validate_io<R: Resampler<f32>>(
    backend: &R,
    channels: usize,
    input: &[f32],
    output: &[f32],
) -> Result<(), ResampleError> {
    let expected_input = backend.input_frames_next() * channels;
    if input.len() != expected_input {
        return Err(ResampleError::InvalidInputLength {
            expected: expected_input,
            actual: input.len(),
        });
    }
    let required_output = backend.output_frames_max() * channels;
    if output.len() < required_output {
        return Err(ResampleError::OutputBufferTooSmall {
            required: required_output,
            actual: output.len(),
        });
    }
    validate_finite_samples(input)
}

pub(crate) fn process_validated<R: Resampler<f32>>(
    backend: &mut R,
    channels: usize,
    input: &[f32],
    output: &mut [f32],
) -> Result<ProcessReport, ResampleError> {
    let input_frames = backend.input_frames_next();
    let output_capacity = backend.output_frames_max();
    let input_adapter =
        InterleavedSlice::new(input, channels, input_frames).map_err(|_| ResampleError::Backend)?;
    let mut output_adapter = InterleavedSlice::new_mut(output, channels, output_capacity)
        .map_err(|_| ResampleError::Backend)?;
    let (input_frames, output_frames) = backend
        .process_into_buffer(&input_adapter, &mut output_adapter, None)
        .map_err(|_| ResampleError::Backend)?;
    let written_samples = output_frames * channels;
    if let Some(sample_index) = output[..written_samples]
        .iter()
        .position(|sample| !sample.is_finite())
    {
        return Err(ResampleError::NonFiniteOutput { sample_index });
    }
    Ok(ProcessReport {
        input_frames,
        output_frames,
    })
}

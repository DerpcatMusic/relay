use core::ops::Range;

use rubato::{Fft, FixedSync, Indexing, Resampler, audioadapter_buffers::direct::InterleavedSlice};

use crate::{
    FrameRequirements, ProcessReport, ResampleError, WorkerResampler, checked_sample_len,
    process_validated, requirements, validate_configuration, validate_finite_samples, validate_io,
};

enum FixedBackend {
    Passthrough { chunk_frames: usize },
    Rubato(Box<Fft<f32>>),
}

impl FixedBackend {
    fn new(
        input_rate: usize,
        output_rate: usize,
        channels: usize,
        chunk_frames: usize,
    ) -> Result<Self, ResampleError> {
        if input_rate == output_rate {
            Ok(Self::Passthrough { chunk_frames })
        } else {
            Fft::new(
                input_rate,
                output_rate,
                chunk_frames,
                channels,
                FixedSync::Input,
            )
            .map(Box::new)
            .map(Self::Rubato)
            .map_err(|_| ResampleError::Backend)
        }
    }
}

/// High-quality synchronous converter for an unbounded, fixed-rate stream.
///
/// This interface deliberately has no end-of-stream operation: every call is
/// one complete streaming chunk, and filter state remains live for the next
/// chunk. Use [`FiniteFixedRatioConverter`] when the source has an end and its
/// leading delay and retained tail must be recovered.
///
/// Non-unity pairs use Rubato's FFT family. The fixed 48 kHz to 48 kHz pair is
/// an exact, zero-delay, allocation-free interleaved copy.
pub struct FixedRatioConverter {
    backend: FixedBackend,
    channels: usize,
    ratio: f64,
}

impl FixedRatioConverter {
    /// Allocate and initialize a converter on the worker/control thread.
    pub fn new(
        input_rate: usize,
        output_rate: usize,
        channels: usize,
        chunk_frames: usize,
    ) -> Result<Self, ResampleError> {
        validate_configuration(input_rate, output_rate, channels, chunk_frames)?;
        Ok(Self {
            backend: FixedBackend::new(input_rate, output_rate, channels, chunk_frames)?,
            channels,
            ratio: output_rate as f64 / input_rate as f64,
        })
    }

    /// Fixed output/input sample-rate ratio.
    #[must_use]
    pub fn ratio(&self) -> f64 {
        self.ratio
    }
}

impl WorkerResampler for FixedRatioConverter {
    fn requirements(&self) -> FrameRequirements {
        match &self.backend {
            FixedBackend::Passthrough { chunk_frames } => FrameRequirements {
                channels: self.channels,
                input_frames_next: *chunk_frames,
                input_frames_max: *chunk_frames,
                output_frames_next: *chunk_frames,
                output_frames_max: *chunk_frames,
                output_delay: 0,
            },
            FixedBackend::Rubato(backend) => requirements(backend.as_ref(), self.channels),
        }
    }

    fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<ProcessReport, ResampleError> {
        match &mut self.backend {
            FixedBackend::Passthrough { chunk_frames } => {
                let samples = checked_sample_len(*chunk_frames, self.channels)?;
                if input.len() != samples {
                    return Err(ResampleError::InvalidInputLength {
                        expected: samples,
                        actual: input.len(),
                    });
                }
                if output.len() < samples {
                    return Err(ResampleError::OutputBufferTooSmall {
                        required: samples,
                        actual: output.len(),
                    });
                }
                validate_finite_samples(input)?;
                output[..samples].copy_from_slice(input);
                Ok(ProcessReport {
                    input_frames: *chunk_frames,
                    output_frames: *chunk_frames,
                })
            }
            FixedBackend::Rubato(backend) => {
                validate_io(backend.as_ref(), self.channels, input, output)?;
                process_validated(backend.as_mut(), self.channels, input, output)
            }
        }
    }

    fn reset(&mut self) {
        if let FixedBackend::Rubato(backend) = &mut self.backend {
            backend.reset();
        }
    }
}

/// Caller-owned buffer requirements for one complete finite stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiniteFrameRequirements {
    /// Number of interleaved channels.
    pub channels: usize,
    /// Exact source length in frames per channel.
    pub input_frames: usize,
    /// Exact useful destination length: `ceil(input * output_rate / input_rate)`.
    pub output_frames: usize,
    /// Frames required in the caller-owned output workspace.
    ///
    /// This includes room for raw leading delay and at most one final backend
    /// block beyond the useful tail. Only the range reported by
    /// [`FiniteProcessReport::valid_output_frame_range`] is useful audio.
    pub output_workspace_frames: usize,
}

/// Exact trimming and count result for one complete finite stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiniteProcessReport {
    /// Valid source frames consumed (zero padding is not included).
    pub input_frames: usize,
    /// Raw frames generated, including delay and final padding.
    pub generated_output_frames: usize,
    /// Exact useful destination frames.
    pub output_frames: usize,
    /// Raw leading frames to remove (the backend's algorithmic delay).
    pub leading_trim_frames: usize,
    /// Raw trailing zero-padding frames to remove.
    pub trailing_trim_frames: usize,
}

impl FiniteProcessReport {
    /// Half-open frame range containing the complete, delay-compensated output.
    #[must_use]
    pub fn valid_output_frame_range(self) -> Range<usize> {
        self.leading_trim_frames..self.generated_output_frames - self.trailing_trim_frames
    }
}

/// Allocation-free, caller-buffered adapter for a complete fixed-rate stream.
///
/// Construction allocates Rubato's filters and scratch storage. Each call to
/// [`Self::process_interleaved`] resets that state, feeds full chunks, feeds a
/// final short chunk with pinned Rubato 4.0's `Indexing::partial_len`, then
/// pumps `partial_len(0)` until the complete tail is present. Processing itself
/// allocates no Rust heap storage.
pub struct FiniteFixedRatioConverter {
    backend: FixedBackend,
    channels: usize,
    input_rate: usize,
    output_rate: usize,
}

impl FiniteFixedRatioConverter {
    /// Allocate and initialize a finite-stream converter on a worker thread.
    pub fn new(
        input_rate: usize,
        output_rate: usize,
        channels: usize,
        chunk_frames: usize,
    ) -> Result<Self, ResampleError> {
        validate_configuration(input_rate, output_rate, channels, chunk_frames)?;
        Ok(Self {
            backend: FixedBackend::new(input_rate, output_rate, channels, chunk_frames)?,
            channels,
            input_rate,
            output_rate,
        })
    }

    /// Compute exact useful length and required caller workspace.
    pub fn requirements(
        &self,
        input_frames: usize,
    ) -> Result<FiniteFrameRequirements, ResampleError> {
        let output_frames = checked_rate_ceil(input_frames, self.input_rate, self.output_rate)?;
        let output_workspace_frames = match &self.backend {
            FixedBackend::Passthrough { .. } => output_frames,
            FixedBackend::Rubato(backend) => backend
                .output_delay()
                .checked_add(output_frames)
                .and_then(|frames| frames.checked_add(backend.output_frames_max()))
                .ok_or(ResampleError::FrameCountOverflow)?,
        };
        Ok(FiniteFrameRequirements {
            channels: self.channels,
            input_frames,
            output_frames,
            output_workspace_frames,
        })
    }

    /// Convert one entire finite interleaved stream without allocating.
    ///
    /// The input length defines the stream length and may be non-chunk-aligned.
    /// `output` must satisfy [`Self::requirements`]. The useful frames remain
    /// in the reported frame range; retaining raw trim regions makes both trim
    /// values observable and avoids an overlapping compaction pass.
    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<FiniteProcessReport, ResampleError> {
        if !input.len().is_multiple_of(self.channels) {
            return Err(ResampleError::InvalidInterleavedLength {
                channels: self.channels,
                actual: input.len(),
            });
        }
        validate_finite_samples(input)?;
        let input_frames = input.len() / self.channels;
        let req = self.requirements(input_frames)?;
        let required_samples = checked_sample_len(req.output_workspace_frames, self.channels)?;
        if output.len() < required_samples {
            return Err(ResampleError::OutputBufferTooSmall {
                required: required_samples,
                actual: output.len(),
            });
        }

        match &mut self.backend {
            FixedBackend::Passthrough { .. } => {
                let samples = checked_sample_len(input_frames, self.channels)?;
                output[..samples].copy_from_slice(input);
                Ok(FiniteProcessReport {
                    input_frames,
                    generated_output_frames: input_frames,
                    output_frames: input_frames,
                    leading_trim_frames: 0,
                    trailing_trim_frames: 0,
                })
            }
            FixedBackend::Rubato(backend) => process_finite_rubato(
                backend.as_mut(),
                self.channels,
                input,
                &mut output[..required_samples],
                req.output_frames,
            ),
        }
    }
}

fn process_finite_rubato(
    backend: &mut Fft<f32>,
    channels: usize,
    input: &[f32],
    output: &mut [f32],
    output_frames: usize,
) -> Result<FiniteProcessReport, ResampleError> {
    backend.reset();
    let input_frames = input.len() / channels;
    if input_frames == 0 {
        return Ok(FiniteProcessReport {
            input_frames: 0,
            generated_output_frames: 0,
            output_frames: 0,
            leading_trim_frames: 0,
            trailing_trim_frames: 0,
        });
    }

    let output_capacity = output.len() / channels;
    let input_adapter =
        InterleavedSlice::new(input, channels, input_frames).map_err(|_| ResampleError::Backend)?;
    let mut output_adapter = InterleavedSlice::new_mut(output, channels, output_capacity)
        .map_err(|_| ResampleError::Backend)?;
    let mut consumed = 0_usize;
    let mut generated = 0_usize;

    while consumed < input_frames {
        let needed = backend.input_frames_next();
        let available = input_frames - consumed;
        let valid = available.min(needed);
        let indexing = Indexing::new()
            .input_offset(consumed)
            .output_offset(generated)
            .partial_len(valid);
        let (_, written) = backend
            .process_into_buffer(&input_adapter, &mut output_adapter, Some(&indexing))
            .map_err(|_| ResampleError::Backend)?;
        consumed += valid;
        generated += written;
    }

    let leading_trim_frames = backend.output_delay();
    let useful_end = leading_trim_frames
        .checked_add(output_frames)
        .ok_or(ResampleError::FrameCountOverflow)?;
    while generated < useful_end {
        let indexing = Indexing::new()
            .input_offset(input_frames)
            .output_offset(generated)
            .partial_len(0);
        let (_, written) = backend
            .process_into_buffer(&input_adapter, &mut output_adapter, Some(&indexing))
            .map_err(|_| ResampleError::Backend)?;
        if written == 0 {
            return Err(ResampleError::Backend);
        }
        generated += written;
    }

    let written_samples = checked_sample_len(generated, channels)?;
    if let Some(sample_index) = output[..written_samples]
        .iter()
        .position(|sample| !sample.is_finite())
    {
        return Err(ResampleError::NonFiniteOutput { sample_index });
    }
    let trailing_trim_frames = generated - useful_end;
    Ok(FiniteProcessReport {
        input_frames,
        generated_output_frames: generated,
        output_frames,
        leading_trim_frames,
        trailing_trim_frames,
    })
}

fn checked_rate_ceil(
    input_frames: usize,
    input_rate: usize,
    output_rate: usize,
) -> Result<usize, ResampleError> {
    let numerator = (input_frames as u128)
        .checked_mul(output_rate as u128)
        .ok_or(ResampleError::FrameCountOverflow)?;
    let output = numerator.div_ceil(input_rate as u128);
    usize::try_from(output).map_err(|_| ResampleError::FrameCountOverflow)
}

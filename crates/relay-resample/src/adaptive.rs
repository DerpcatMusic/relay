use rubato::{
    Adjustable, Async, FixedAsync, Indexing, Resampler, Resizable, SincInterpolationParameters,
    audioadapter_buffers::direct::InterleavedSlice,
};

use crate::{
    FrameRequirements, ProcessReport, ResampleError, WorkerResampler, checked_sample_len,
    validate_configuration, validate_finite_samples,
};

const PPM_SCALE: f64 = 1_000_000.0;
const MAX_ALLOWED_CORRECTION_PPM: f64 = 100_000.0;

/// Clock-recovery limits and smoothing for the adaptive converter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdaptiveClockConfig {
    /// Symmetric clamp around the nominal ratio, in parts per million.
    pub max_correction_ppm: f64,
    /// One-pole smoothing time constant. Must be finite and greater than zero.
    pub smoothing_time_seconds: f64,
}

impl Default for AdaptiveClockConfig {
    fn default() -> Self {
        Self {
            max_correction_ppm: 1_000.0,
            smoothing_time_seconds: 1.0,
        }
    }
}

/// Validated correction to an output-frames/input-frames ratio, in ppm.
///
/// Positive means `nominal_output_per_input * (1 + ppm / 1_000_000)` is
/// larger; negative means it is smaller. This is a resampler command, never a
/// raw remote-clock drift observation. A positive remote drift normally yields
/// a negative value after clock recovery.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputInputRatioCorrectionPpm(f64);

impl OutputInputRatioCorrectionPpm {
    /// Validate an already-controlled output/input correction.
    pub fn new(ppm: f64) -> Result<Self, ResampleError> {
        if ppm.is_finite() {
            Ok(Self(ppm))
        } else {
            Err(ResampleError::NonFiniteClockCorrection)
        }
    }

    /// Recover the correction from a controller's output/input ratio multiplier.
    ///
    /// This is directly compatible with `relay_clock::ClockRecoveryOutput`'s
    /// `ratio_multiplier` field and prevents accidentally passing its raw remote
    /// drift input across this boundary.
    pub fn from_ratio_multiplier(multiplier: f64) -> Result<Self, ResampleError> {
        if !multiplier.is_finite() {
            return Err(ResampleError::NonFiniteClockCorrection);
        }
        if multiplier <= 0.0 {
            return Err(ResampleError::InvalidOutputInputRatioMultiplier);
        }
        Self::new((multiplier - 1.0) * PPM_SCALE)
    }

    /// Correction value in parts per million.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// Corresponding multiplier for a nominal output/input ratio.
    #[must_use]
    pub fn ratio_multiplier(self) -> f64 {
        1.0 + self.0 / PPM_SCALE
    }
}

/// Result of accepting an output/input ratio correction request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockCorrection {
    /// Controller request before the safety clamp.
    pub requested_ppm: f64,
    /// Target retained after clamping.
    pub clamped_ppm: f64,
}

/// Buffer sizing and startup-delay information for terminal adaptive processing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdaptiveFinishRequirements {
    /// Number of interleaved channels.
    pub channels: usize,
    /// Normal full input transaction expected by terminal processing.
    pub final_input_frames: usize,
    /// Output capacity sufficient at every admitted ratio and phase.
    pub output_workspace_frames: usize,
    /// Initial startup delay at the head of previously returned streaming output.
    pub leading_trim_frames: usize,
}

/// Exact accounting for a successful terminal adaptive transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdaptiveFinishReport {
    /// Source-valid frames consumed from the final full transaction.
    pub valid_input_frames: usize,
    /// Raw frames written, including the final backend overshoot.
    pub generated_output_frames: usize,
    /// Valid finish frames in the caller's output prefix.
    pub output_frames: usize,
    /// Initial raw frames to remove from the head of the complete stream.
    pub leading_trim_frames: usize,
    /// Raw zero-pump overshoot omitted from `output_frames`.
    pub trailing_trim_frames: usize,
    /// Always zero after this one-shot low-level success.
    pub pending_output_frames: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdaptiveState {
    Ready,
    Finished,
    Faulted,
}

/// High-quality asynchronous converter steered only by clock-rate recovery.
///
/// Network packet jitter and raw remote-clock drift are not accepted here.
/// [`Self::set_output_input_correction`] takes only the named output/input ratio
/// command produced by a recovery controller.
pub struct AdaptiveClockConverter {
    backend: Async<f32>,
    channels: usize,
    input_rate: usize,
    nominal_ratio: f64,
    max_correction_ppm: f64,
    smoothing_time_seconds: f64,
    target_ppm: f64,
    smoothed_ppm: f64,
    configured_chunk_frames: usize,
    normal_output_workspace_frames: usize,
    initial_output_delay_frames: usize,
    finish_output_workspace_frames: usize,
    finish_zero_pump_blocks: usize,
    saw_input: bool,
    state: AdaptiveState,
}

impl AdaptiveClockConverter {
    /// Allocate filters and internal storage on the worker/control thread.
    ///
    /// Construction reserves a derived private Rubato phase workspace covering
    /// every admitted correction reversal. The public input transaction remains
    /// exactly `chunk_frames`; normal processing and finite finish allocate nothing.
    pub fn new(
        input_rate: usize,
        output_rate: usize,
        channels: usize,
        chunk_frames: usize,
        config: AdaptiveClockConfig,
    ) -> Result<Self, ResampleError> {
        validate_configuration(input_rate, output_rate, channels, chunk_frames)?;
        if !config.max_correction_ppm.is_finite()
            || !(0.0..=MAX_ALLOWED_CORRECTION_PPM).contains(&config.max_correction_ppm)
            || !config.smoothing_time_seconds.is_finite()
            || config.smoothing_time_seconds <= 0.0
        {
            return Err(ResampleError::InvalidAdaptiveConfiguration);
        }

        let nominal_ratio = output_rate as f64 / input_rate as f64;
        let max_fraction = config.max_correction_ppm / PPM_SCALE;
        // Rubato's lower bound is nominal/max_relative. Choosing the reciprocal
        // keeps our symmetric negative clamp strictly inside that bound.
        let max_relative = 1.0 / (1.0 - max_fraction);
        let sinc = SincInterpolationParameters::default();

        // Pinned Rubato 4 (`asynchro.rs`) calculates a fixed-input block's
        // output count as M=floor(y*A), where y=c-L-1-x, c is the current
        // chunk (at most C), L is the sinc length, x is `last_index`, and A is
        // the arithmetic mean of the endpoint ratios. `InnerSinc::process`
        // instead advances x by D, the sum of M linearly interpolated reciprocal
        // ratios. This arithmetic-mean/harmonic-mean mismatch is why opposite
        // ramps can exceed Rubato's nominal C+2L private buffer.
        //
        // The following is a construction proof, not an empirical guard:
        //
        // * x >= -L-1-1/r_min is invariant. Initially it is stronger. If M=0,
        //   floor(y*A)=0 bounds y<1/A. If M>0, M>=y*A-1 and
        //   D=M*H+delta/2 >= y-1/r_start because A*H>=1.
        // * For a block that emits output, y<=C+1/r_min, M<=y*r_max, and every
        //   phase step is <=1/r_min. Its greatest read phase is therefore at
        //   most C-L-1 + (C+1/r_min)*(r_max/r_min-1).
        // * Rubato's cubic direct and stereo-combined paths can reach one tap
        //   beyond floor(phase). A private maximum P=C+ceil(phase_excess) makes
        //   that tap strictly smaller than Rubato's P+2L buffer length.
        //
        // Every floating operation is rounded outward. The backend is then
        // resized back to C, so the public transaction remains exactly C.
        let minimum_ratio = (nominal_ratio * (1.0 - max_fraction)).next_down();
        let maximum_ratio = (nominal_ratio * (1.0 + max_fraction)).next_up();
        let inverse_minimum_ratio = (1.0 / minimum_ratio).next_up();
        let phase_span = (chunk_frames as f64 + inverse_minimum_ratio).next_up();
        let relative_span = (maximum_ratio / minimum_ratio).next_up();
        let phase_excess = (phase_span * (relative_span - 1.0).next_up()).next_up();
        if !phase_excess.is_finite() || phase_excess.ceil() > usize::MAX as f64 {
            return Err(ResampleError::FrameCountOverflow);
        }
        let backend_chunk_frames = chunk_frames
            .checked_add(phase_excess.ceil() as usize)
            .ok_or(ResampleError::FrameCountOverflow)?;
        let mut backend = Async::new_sinc(
            nominal_ratio,
            max_relative,
            &sinc,
            backend_chunk_frames,
            channels,
            FixedAsync::Input,
        )
        .map_err(|_| ResampleError::Backend)?;
        backend
            .set_chunk_size(chunk_frames)
            .map_err(|_| ResampleError::Backend)?;

        // This is pinned Rubato 4's `calculate_max_output_size` for FixedInput,
        // evaluated at the public chunk rather than the larger private allocation.
        let normal_output_bound = chunk_frames as f64 * nominal_ratio * max_relative + 10.0;
        if !normal_output_bound.is_finite() || normal_output_bound > usize::MAX as f64 {
            return Err(ResampleError::FrameCountOverflow);
        }
        let normal_output_workspace_frames = normal_output_bound as usize;
        let initial_output_delay_frames = backend.output_delay();
        // Half the sinc support must move beyond the finite endpoint. The
        // additional full transaction covers fractional phase at every ratio.
        let finish_zero_pump_blocks = (sinc.sinc_len / 2)
            .div_ceil(chunk_frames)
            .checked_add(1)
            .ok_or(ResampleError::FrameCountOverflow)?;
        let finish_output_workspace_frames = finish_zero_pump_blocks
            .checked_add(1)
            .and_then(|blocks| blocks.checked_mul(normal_output_workspace_frames))
            .ok_or(ResampleError::FrameCountOverflow)?;

        Ok(Self {
            backend,
            channels,
            input_rate,
            nominal_ratio,
            max_correction_ppm: config.max_correction_ppm,
            smoothing_time_seconds: config.smoothing_time_seconds,
            target_ppm: 0.0,
            smoothed_ppm: 0.0,
            configured_chunk_frames: chunk_frames,
            normal_output_workspace_frames,
            initial_output_delay_frames,
            finish_output_workspace_frames,
            finish_zero_pump_blocks,
            saw_input: false,
            state: AdaptiveState::Ready,
        })
    }

    /// Set an output/input ratio correction, clamped to configured bounds.
    ///
    /// Positive increases output frames per input frame. Negative decreases it.
    /// This does not accept raw drift, packet arrival error, or jitter-buffer
    /// occupancy; those inputs belong in the clock-recovery controller.
    pub fn set_output_input_correction(
        &mut self,
        correction: OutputInputRatioCorrectionPpm,
    ) -> ClockCorrection {
        let requested_ppm = correction.get();
        let clamped_ppm = requested_ppm.clamp(-self.max_correction_ppm, self.max_correction_ppm);
        self.target_ppm = clamped_ppm;
        ClockCorrection {
            requested_ppm,
            clamped_ppm,
        }
    }

    /// Current controller target after clamping.
    #[must_use]
    pub fn target_correction_ppm(&self) -> f64 {
        self.target_ppm
    }

    /// Current one-pole-smoothed correction applied to Rubato.
    #[must_use]
    pub fn smoothed_correction_ppm(&self) -> f64 {
        self.smoothed_ppm
    }

    /// Current output/input ratio reported by Rubato.
    #[must_use]
    pub fn ratio(&self) -> f64 {
        self.backend.resample_ratio()
    }

    /// Return the caller-owned workspace required by [`Self::finish_interleaved`].
    ///
    /// This query does not change ratio, filter, or lifecycle state.
    pub fn finish_requirements(&self) -> Result<AdaptiveFinishRequirements, ResampleError> {
        checked_sample_len(self.configured_chunk_frames, self.channels)?;
        checked_sample_len(self.finish_output_workspace_frames, self.channels)?;
        Ok(AdaptiveFinishRequirements {
            channels: self.channels,
            final_input_frames: self.configured_chunk_frames,
            output_workspace_frames: self.finish_output_workspace_frames,
            leading_trim_frames: self.initial_output_delay_frames,
        })
    }

    /// Consume a valid prefix of one final full transaction and drain the sinc tail.
    ///
    /// All input and capacity checks happen before the ratio, filter, or lifecycle
    /// is changed. The smoother advances once by `valid_input_frames / input_rate`;
    /// zero pumps freeze that reached ratio. The operation neither allocates nor
    /// retains either caller slice.
    pub fn finish_interleaved(
        &mut self,
        final_input: &[f32],
        valid_input_frames: usize,
        output: &mut [f32],
    ) -> Result<AdaptiveFinishReport, ResampleError> {
        if self.state != AdaptiveState::Ready {
            return Err(ResampleError::EndOfStream);
        }

        let requirements = self.finish_requirements()?;
        let expected_input_samples =
            checked_sample_len(requirements.final_input_frames, self.channels)?;
        if final_input.len() != expected_input_samples {
            return Err(ResampleError::InvalidInputLength {
                expected: expected_input_samples,
                actual: final_input.len(),
            });
        }
        if !(1..=requirements.final_input_frames).contains(&valid_input_frames) {
            return Err(ResampleError::InvalidValidInputFrames {
                valid: valid_input_frames,
                maximum: requirements.final_input_frames,
            });
        }
        let valid_input_samples = checked_sample_len(valid_input_frames, self.channels)?;
        validate_finite_samples(&final_input[..valid_input_samples])?;
        let required_output_samples =
            checked_sample_len(requirements.output_workspace_frames, self.channels)?;
        if output.len() < required_output_samples {
            return Err(ResampleError::OutputBufferTooSmall {
                required: required_output_samples,
                actual: output.len(),
            });
        }

        self.finish_validated(final_input, valid_input_frames, output)
    }

    fn finish_validated(
        &mut self,
        final_input: &[f32],
        valid_input_frames: usize,
        output: &mut [f32],
    ) -> Result<AdaptiveFinishReport, ResampleError> {
        if self.backend.set_chunk_size(valid_input_frames).is_err() {
            self.state = AdaptiveState::Faulted;
            return Err(ResampleError::Backend);
        }
        if self.advance_smoothed_ratio().is_err() {
            self.state = AdaptiveState::Faulted;
            return Err(ResampleError::Backend);
        }

        let input_adapter =
            match InterleavedSlice::new(final_input, self.channels, self.configured_chunk_frames) {
                Ok(adapter) => adapter,
                Err(_) => {
                    self.state = AdaptiveState::Faulted;
                    return Err(ResampleError::Backend);
                }
            };
        let output_capacity_frames = self.finish_output_workspace_frames;
        let (generated_output_frames, pumped_output_frames, final_delay_frames) = {
            let mut output_adapter =
                match InterleavedSlice::new_mut(output, self.channels, output_capacity_frames) {
                    Ok(adapter) => adapter,
                    Err(_) => {
                        self.state = AdaptiveState::Faulted;
                        return Err(ResampleError::Backend);
                    }
                };

            let (_, final_generated_frames) =
                match self
                    .backend
                    .process_into_buffer(&input_adapter, &mut output_adapter, None)
                {
                    Ok(report) => report,
                    Err(_) => {
                        self.state = AdaptiveState::Faulted;
                        return Err(ResampleError::Backend);
                    }
                };
            let final_delay_frames = self.backend.output_delay();
            if self
                .backend
                .set_chunk_size(self.configured_chunk_frames)
                .is_err()
            {
                self.state = AdaptiveState::Faulted;
                return Err(ResampleError::Backend);
            }

            let mut generated_output_frames = final_generated_frames;
            let mut pumped_output_frames = 0_usize;
            let zero_input = Indexing::new().partial_len(0);
            for _ in 0..self.finish_zero_pump_blocks {
                if pumped_output_frames >= final_delay_frames {
                    break;
                }
                let block_output_frames = self.backend.output_frames_next();
                let block_end = match generated_output_frames.checked_add(block_output_frames) {
                    Some(end) if end <= output_capacity_frames => end,
                    _ => {
                        self.state = AdaptiveState::Faulted;
                        return Err(ResampleError::Backend);
                    }
                };
                let indexing = zero_input.clone().output_offset(generated_output_frames);
                let (_, written_frames) = match self.backend.process_into_buffer(
                    &input_adapter,
                    &mut output_adapter,
                    Some(&indexing),
                ) {
                    Ok(report) => report,
                    Err(_) => {
                        self.state = AdaptiveState::Faulted;
                        return Err(ResampleError::Backend);
                    }
                };
                if written_frames != block_output_frames {
                    self.state = AdaptiveState::Faulted;
                    return Err(ResampleError::Backend);
                }
                generated_output_frames = block_end;
                pumped_output_frames = match pumped_output_frames.checked_add(written_frames) {
                    Some(frames) => frames,
                    None => {
                        self.state = AdaptiveState::Faulted;
                        return Err(ResampleError::Backend);
                    }
                };
            }

            (
                generated_output_frames,
                pumped_output_frames,
                final_delay_frames,
            )
        };
        if let Some(sample_index) = output[..generated_output_frames * self.channels]
            .iter()
            .position(|sample| !sample.is_finite())
        {
            self.state = AdaptiveState::Faulted;
            return Err(ResampleError::NonFiniteOutput { sample_index });
        }
        if pumped_output_frames < final_delay_frames {
            self.state = AdaptiveState::Faulted;
            return Err(ResampleError::Backend);
        }
        let trailing_trim_frames = pumped_output_frames - final_delay_frames;
        let output_frames = generated_output_frames - trailing_trim_frames;
        self.saw_input |= valid_input_frames != 0;
        self.state = AdaptiveState::Finished;
        Ok(AdaptiveFinishReport {
            valid_input_frames,
            generated_output_frames,
            output_frames,
            leading_trim_frames: self.initial_output_delay_frames,
            trailing_trim_frames,
            pending_output_frames: 0,
        })
    }

    fn advance_smoothed_ratio(&mut self) -> Result<(), ResampleError> {
        let block_seconds = self.backend.input_frames_next() as f64 / self.input_rate as f64;
        let smoothing = 1.0 - (-block_seconds / self.smoothing_time_seconds).exp();
        self.smoothed_ppm += smoothing * (self.target_ppm - self.smoothed_ppm);
        let ratio = self.nominal_ratio * (1.0 + self.smoothed_ppm / PPM_SCALE);
        self.backend
            .set_resample_ratio(ratio, true)
            .map_err(|_| ResampleError::Backend)
    }
}

impl WorkerResampler for AdaptiveClockConverter {
    fn requirements(&self) -> FrameRequirements {
        FrameRequirements {
            channels: self.channels,
            input_frames_next: self.backend.input_frames_next(),
            input_frames_max: self.configured_chunk_frames,
            output_frames_next: self.backend.output_frames_next(),
            output_frames_max: self.normal_output_workspace_frames,
            output_delay: self.backend.output_delay(),
        }
    }

    fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<ProcessReport, ResampleError> {
        if self.state != AdaptiveState::Ready {
            return Err(ResampleError::EndOfStream);
        }
        // Validate before changing either smoothing or Rubato streaming state.
        let input_frames = self.backend.input_frames_next();
        let expected_input_samples = checked_sample_len(input_frames, self.channels)?;
        if input.len() != expected_input_samples {
            return Err(ResampleError::InvalidInputLength {
                expected: expected_input_samples,
                actual: input.len(),
            });
        }
        let required_output_samples =
            checked_sample_len(self.normal_output_workspace_frames, self.channels)?;
        if output.len() < required_output_samples {
            return Err(ResampleError::OutputBufferTooSmall {
                required: required_output_samples,
                actual: output.len(),
            });
        }
        validate_finite_samples(input)?;

        self.advance_smoothed_ratio()?;
        let input_adapter = InterleavedSlice::new(input, self.channels, input_frames)
            .map_err(|_| ResampleError::Backend)?;
        let mut output_adapter =
            InterleavedSlice::new_mut(output, self.channels, self.normal_output_workspace_frames)
                .map_err(|_| ResampleError::Backend)?;
        let (input_frames, output_frames) = self
            .backend
            .process_into_buffer(&input_adapter, &mut output_adapter, None)
            .map_err(|_| ResampleError::Backend)?;
        let written_samples = checked_sample_len(output_frames, self.channels)?;
        if let Some(sample_index) = output[..written_samples]
            .iter()
            .position(|sample| !sample.is_finite())
        {
            return Err(ResampleError::NonFiniteOutput { sample_index });
        }
        self.saw_input = true;
        Ok(ProcessReport {
            input_frames,
            output_frames,
        })
    }

    fn reset(&mut self) {
        self.backend.reset();
        // Construction retains the derived private Rubato phase workspace.
        let _ = self.backend.set_chunk_size(self.configured_chunk_frames);
        self.target_ppm = 0.0;
        self.smoothed_ppm = 0.0;
        self.saw_input = false;
        self.state = AdaptiveState::Ready;
        // The nominal ratio is always inside the construction bounds.
        let _ = self.backend.set_resample_ratio(self.nominal_ratio, false);
        self.initial_output_delay_frames = self.backend.output_delay();
    }
}

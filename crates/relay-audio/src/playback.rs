//! Scheduled playout, adaptive sample-rate conversion, and callback rendering.
//!
//! [`PlaybackWorker`] runs off the device callback. It binds extended remote
//! media progression to caller-supplied scheduled device-frame positions,
//! steers an adaptive converter, and publishes complete chunks to a bounded
//! SPSC ring. [`PlaybackRenderer`] is the callback-facing endpoint: it performs
//! only bounded zero-fill, sample copies, and the ring's atomic bookkeeping.

use core::fmt;

use relay_clock::{
    ClockError, ClockRecovery, ClockRecoveryOutput, DiscontinuityReason, DriftEstimator,
    DriftEstimatorConfig, DriftEstimatorUpdate, PlayoutClockObservation,
};
use relay_resample::{
    AdaptiveClockConverter, OutputInputRatioCorrectionPpm, ResampleError, WorkerResampler,
};
use relay_rt::{
    AudioConsumer, AudioProducer, AudioRingMetrics, ReadState, RingConfigError, WriteOutcome,
    audio_ring,
};

use crate::{AudioPipelineConfig, ExtendedTimestamp, PcmFrame};

const MEDIA_RATE_HZ: usize = 48_000;

/// Playback-specific policy validated against an [`AudioPipelineConfig`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackConfig {
    /// Desired queued device frames sampled after each worker publication.
    pub target_fill_frames: usize,
    /// Long-window scheduled-playout drift estimator policy.
    pub drift_estimator: DriftEstimatorConfig,
}

impl PlaybackConfig {
    /// Creates the Phase-1 default policy for a validated pipeline.
    ///
    /// The target covers one converter output plus its algorithmic delay,
    /// bounded strictly inside the fixed ring. Callers may replace it.
    #[must_use]
    pub fn for_pipeline(pipeline: AudioPipelineConfig) -> Self {
        let drift_estimator = DriftEstimatorConfig {
            nominal_sample_rate_hz: MEDIA_RATE_HZ as f64,
            local_device_sample_rate_hz: pipeline.playback_rate_hz() as f64,
            ..DriftEstimatorConfig::default()
        };
        let ring_capacity_frames = pipeline.playback_ring_samples() / pipeline.channels();
        let requirements = pipeline.adaptive_resampler_requirements();
        let low_latency_target = requirements
            .output_delay
            .saturating_add(requirements.output_frames_next);
        Self {
            target_fill_frames: low_latency_target.min(ring_capacity_frames.saturating_sub(1)),
            drift_estimator,
        }
    }
}

/// Playback-pair construction failure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlaybackBuildError {
    /// The target was zero or not strictly smaller than ring capacity.
    InvalidTargetFillFrames {
        /// Requested target in device frames.
        target: usize,
        /// Fixed ring capacity in device frames.
        capacity: usize,
    },
    /// The estimator's declared clock domains did not match the pipeline.
    MismatchedEstimatorClockDomains,
    /// The configured ring sample capacity could not be constructed.
    Ring(RingConfigError),
    /// The estimator policy was invalid.
    Clock(ClockError),
    /// The validated pipeline could not recreate its adaptive converter.
    Resampler(ResampleError),
    /// Fixed sample-count arithmetic overflowed the platform address space.
    CapacityOverflow,
    /// A fixed live or finish workspace allocation was rejected.
    AllocationFailure,
}

impl fmt::Display for PlaybackBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PlaybackBuildError {}

/// Worker lifecycle for live and finite playback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackWorkerState {
    /// The worker accepts scheduled decoded frames or one finite end.
    Running,
    /// Adaptive finish completed, but valid output remains ring-blocked.
    Finishing,
    /// Every valid adaptive finish frame was published.
    Finished,
    /// A clock discontinuity or stateful processing error requires an explicit reset.
    Faulted,
}

/// Why a scheduled frame was not published.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlaybackProcessError {
    /// The worker must be reset before accepting another frame.
    Faulted,
    /// Finite finishing has begun, so ordinary input is no longer accepted.
    EndOfStream,
    /// The frame did not match the configured fixed 48 kHz transaction size.
    InvalidFrameSamples {
        /// Required interleaved scalar-sample count.
        expected: usize,
        /// Supplied interleaved scalar-sample count.
        actual: usize,
    },
    /// The scheduled clock observation was invalid.
    Clock(ClockError),
    /// The remote media timeline regressed or stalled.
    ClockDiscontinuity(DiscontinuityReason),
    /// Adaptive conversion failed.
    Resampler(ResampleError),
}

impl fmt::Display for PlaybackProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PlaybackProcessError {}

/// Outcome of one complete all-or-drop worker publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackPublication {
    /// The complete converted chunk entered the playback ring.
    Published,
    /// The complete new chunk was dropped because the ring lacked capacity.
    DroppedFull,
    /// The renderer endpoint had already been destroyed.
    RendererDisconnected,
}

/// Result of processing one resolved RX frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackProcessReport {
    /// Remote 48 kHz frames consumed.
    pub input_frames: usize,
    /// Local device frames produced by adaptive conversion.
    pub output_frames: usize,
    /// Ring outcome for the complete converted chunk.
    pub publication: PlaybackPublication,
    /// Stable-phase ring fill after publication, in device frames.
    pub ring_fill_frames: usize,
    /// Latest long-window remote drift estimate, if warmed up.
    pub estimated_remote_drift_ppm: Option<f64>,
    /// Latest controller output, only when cadence elapsed on this call.
    pub controller: Option<ClockRecoveryOutput>,
    /// Post-publication control failure; publication progress remains explicit.
    pub control_fault: Option<PlaybackControlFault>,
    /// Smoothed output/input correction applied by the converter on this call.
    pub applied_correction_ppm: f64,
    /// Target correction retained for the next converter call.
    pub target_correction_ppm: f64,
}

/// Failure encountered after a converted chunk had an explicit ring outcome.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlaybackControlFault {
    /// The scheduled controller interval was invalid.
    Clock(ClockError),
    /// The typed controller-to-resampler boundary rejected its command.
    Resampler(ResampleError),
}

/// Saturating worker-side counters for the playback worker lifetime.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlaybackMetrics {
    /// Resolved RX frames presented to the adaptive converter.
    pub input_frames: u64,
    /// Raw adaptive SRC output device frames produced.
    pub output_frames: u64,
    /// Complete chunks published to the ring.
    pub published_chunks: u64,
    /// Complete live chunks dropped because the ring was full.
    pub dropped_full_chunks: u64,
    /// Complete chunks dropped because the renderer was gone.
    pub disconnected_chunks: u64,
    /// Controller updates performed at or after configured cadence.
    pub controller_updates: u64,
    /// Estimator discontinuities that faulted the worker.
    pub clock_discontinuities: u64,
    /// Explicit successful state resets while the ring was empty.
    pub resets: u64,
}

/// Why an explicit playback reset could not be applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackResetError {
    /// Queued samples from the previous epoch must be rendered or discarded by
    /// stopping and recreating the pair before state can be reused.
    RingNotEmpty {
        /// Scalar samples still visible to the renderer.
        queued_samples: usize,
    },
    /// Valid finite output is still retained by the worker.
    FinishPending {
        /// Device frames not yet published.
        pending_frames: usize,
    },
}

/// The withheld last decoded frame and its finite manifest facts.
#[derive(Debug)]
pub struct FinitePlaybackInput<'a> {
    /// The final decoded frame returned by [`crate::RxWorker::drain`].
    pub frame: &'a PcmFrame,
    /// Full packet frames, or the final valid media prefix from finite TX.
    pub valid_media_frames: usize,
    /// Extended remote media position of the withheld frame.
    pub remote_media_sample_position: ExtendedTimestamp,
    /// Scheduled local device-frame position of the withheld frame.
    pub scheduled_local_device_frame: u64,
}

/// One explicit finite playback end or retained-tail retry.
#[derive(Debug)]
pub enum FinitePlaybackEnd<'a> {
    /// Consume the withheld final decoded frame exactly once.
    Final(FinitePlaybackInput<'a>),
    /// Complete a genuinely empty stream without manufacturing media.
    Empty,
    /// Retry publication of already-generated valid finish output.
    Continue,
}

/// Finite playback completion state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackFinishStatus {
    /// Valid finish output remains retained because the ring lacked room.
    PendingRing,
    /// Every valid finish frame was published.
    Finished,
}

/// Exact per-call and retained finite playback accounting, in per-channel frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackFinishReport {
    /// Whether valid output remains retained.
    pub status: PlaybackFinishStatus,
    /// Valid final 48 kHz frames accepted on this call; zero on retry.
    pub input_frames_consumed: usize,
    /// Raw adaptive finish frames generated on this call; zero on retry.
    pub generated_output_frames: usize,
    /// Valid adaptive finish frames, repeated on every retry.
    pub valid_output_frames: usize,
    /// Valid device frames published on this call.
    pub published_output_frames: usize,
    /// Initial adaptive delay at the head of the complete collected stream.
    pub leading_trim_frames: usize,
    /// Raw finish overshoot omitted from publication.
    pub trailing_trim_frames: usize,
    /// Valid device frames retained but not yet published.
    pub pending_output_frames: usize,
    /// Previously and newly published frames still queued for rendering.
    pub queued_playback_frames: usize,
}

/// Cause of a finite playback rejection or sticky failure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlaybackFinishErrorCause {
    /// The requested end operation was not valid in the current lifecycle.
    InvalidTransition,
    /// The final manifest count was zero or exceeded one negotiated packet.
    InvalidValidMediaFrames {
        /// Rejected valid prefix.
        valid: usize,
        /// Maximum accepted packet prefix.
        maximum: usize,
    },
    /// The supplied decoded frame did not match the negotiated packet size.
    InvalidFinalFrameSamples {
        /// Required interleaved scalar samples.
        expected: usize,
        /// Supplied interleaved scalar samples.
        actual: usize,
    },
    /// Empty was supplied after adaptive media had already been accepted.
    MissingFinalFrame,
    /// Earlier live playback was lossy, so finite completeness cannot be claimed.
    PriorPlaybackLoss,
    /// The worker is sticky-faulted until reset.
    Faulted,
    /// The final scheduled clock observation was invalid.
    Clock(ClockError),
    /// The final remote media timeline was discontinuous.
    ClockDiscontinuity(DiscontinuityReason),
    /// Adaptive terminal conversion failed.
    Resampler(ResampleError),
    /// The renderer was destroyed before retained output could be published.
    RendererDisconnected,
    /// A preflighted all-or-none ring publication unexpectedly failed.
    PublicationInvariant,
}

/// Finite playback failure with an exact progress snapshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackFinishError {
    /// Rejection or sticky-failure cause.
    pub cause: PlaybackFinishErrorCause,
    /// Generated, trim, publication, pending, and queue facts at failure.
    pub progress: PlaybackFinishReport,
}

impl fmt::Display for PlaybackFinishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}", self.cause)
    }
}

impl std::error::Error for PlaybackFinishError {}

/// Off-callback scheduled-playout worker.
///
/// Construction preallocates both live and worst-case finite-finish workspaces.
/// Processing and finishing never grow them. Destroy this endpoint only after
/// the renderer callback has been stopped and acknowledged by the embedding API.
pub struct PlaybackWorker {
    pipeline: AudioPipelineConfig,
    target_fill_frames: usize,
    ring_capacity_samples: usize,
    producer: AudioProducer,
    estimator: DriftEstimator,
    recovery: ClockRecovery,
    converter: AdaptiveClockConverter,
    output: Vec<f32>,
    finish_output: Vec<f32>,
    finish_generated_frames: usize,
    finish_valid_frames: usize,
    finish_cursor_samples: usize,
    finish_leading_trim_frames: usize,
    finish_trailing_trim_frames: usize,
    finite_saw_input: bool,
    finite_integrity: bool,
    latest_drift_ppm: Option<f64>,
    last_control_device_frame: Option<u64>,
    state: PlaybackWorkerState,
    metrics: PlaybackMetrics,
}

/// Callback-facing bounded ring consumer.
///
/// `render` performs no allocation, lock, wait, logging, I/O, networking, or
/// DSP. Do not destroy this endpoint on the callback; stop/detach the device,
/// wait for its acknowledgement, then drop both endpoints on a control thread.
pub struct PlaybackRenderer {
    channels: usize,
    consumer: AudioConsumer,
}

/// Renderer result after the complete caller buffer has been initialized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderReport {
    /// Scalar samples requested by the device.
    pub requested_samples: usize,
    /// Leading scalar samples copied from the playback ring.
    pub rendered_samples: usize,
    /// Scalar samples intentionally left as zero.
    pub zeroed_samples: usize,
    /// Queue/alignment state observed by this render.
    pub state: RenderState,
}

/// Callback render state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderState {
    /// The ring filled the complete aligned device buffer.
    Complete,
    /// The producer remained connected but the suffix was zero-filled.
    Underrun,
    /// Buffered audio was drained and the producer was observed gone.
    Disconnected,
    /// The device supplied a scalar count that was not a whole audio frame;
    /// the complete output was zeroed and the ring was not consumed.
    Misaligned,
}

/// Constructs the unique worker/renderer pair and shared ring metrics.
///
/// All allocation occurs in this function. The metrics handle is observational;
/// it must also be dropped off the callback after endpoint shutdown.
pub fn playback_pair(
    pipeline: AudioPipelineConfig,
    config: PlaybackConfig,
) -> Result<(PlaybackWorker, PlaybackRenderer, AudioRingMetrics), PlaybackBuildError> {
    let channels = pipeline.channels();
    let ring_capacity_samples = pipeline.playback_ring_samples();
    let ring_capacity_frames = ring_capacity_samples / channels;
    if config.target_fill_frames == 0 || config.target_fill_frames >= ring_capacity_frames {
        return Err(PlaybackBuildError::InvalidTargetFillFrames {
            target: config.target_fill_frames,
            capacity: ring_capacity_frames,
        });
    }
    if config.drift_estimator.nominal_sample_rate_hz != MEDIA_RATE_HZ as f64
        || config.drift_estimator.local_device_sample_rate_hz != pipeline.playback_rate_hz() as f64
    {
        return Err(PlaybackBuildError::MismatchedEstimatorClockDomains);
    }

    let estimator =
        DriftEstimator::new(config.drift_estimator).map_err(PlaybackBuildError::Clock)?;
    let recovery = pipeline
        .create_clock_recovery()
        .map_err(PlaybackBuildError::Clock)?;
    let converter = pipeline
        .create_adaptive_resampler()
        .map_err(PlaybackBuildError::Resampler)?;
    let requirements = converter.requirements();
    let output_samples = requirements
        .output_frames_max
        .checked_mul(channels)
        .ok_or(PlaybackBuildError::CapacityOverflow)?;
    let finish_requirements = converter
        .finish_requirements()
        .map_err(PlaybackBuildError::Resampler)?;
    let finish_output_samples = finish_requirements
        .output_workspace_frames
        .checked_mul(channels)
        .ok_or(PlaybackBuildError::CapacityOverflow)?;
    let output = zeroed_workspace(output_samples)?;
    let finish_output = zeroed_workspace(finish_output_samples)?;
    let (producer, consumer, metrics) =
        audio_ring(ring_capacity_samples).map_err(PlaybackBuildError::Ring)?;

    Ok((
        PlaybackWorker {
            pipeline,
            target_fill_frames: config.target_fill_frames,
            ring_capacity_samples,
            producer,
            estimator,
            recovery,
            converter,
            output,
            finish_output,
            finish_generated_frames: 0,
            finish_valid_frames: 0,
            finish_cursor_samples: 0,
            finish_leading_trim_frames: 0,
            finish_trailing_trim_frames: 0,
            finite_saw_input: false,
            finite_integrity: true,
            latest_drift_ppm: None,
            last_control_device_frame: None,
            state: PlaybackWorkerState::Running,
            metrics: PlaybackMetrics::default(),
        },
        PlaybackRenderer { channels, consumer },
        metrics,
    ))
}

fn zeroed_workspace(samples: usize) -> Result<Vec<f32>, PlaybackBuildError> {
    let mut workspace = Vec::new();
    workspace
        .try_reserve_exact(samples)
        .map_err(|_| PlaybackBuildError::AllocationFailure)?;
    workspace.resize(samples, 0.0);
    Ok(workspace)
}

impl PlaybackWorker {
    /// Returns whether the worker accepts another resolved frame.
    #[must_use]
    pub const fn state(&self) -> PlaybackWorkerState {
        self.state
    }

    /// Returns a coherent worker-owned metrics snapshot.
    #[must_use]
    pub const fn metrics(&self) -> PlaybackMetrics {
        self.metrics
    }

    /// Returns the current stable-phase ring fill in local device frames.
    #[must_use]
    pub fn ring_fill_frames(&self) -> usize {
        (self.ring_capacity_samples - self.producer.available_samples()) / self.pipeline.channels()
    }

    /// Returns the fixed output workspace address and capacity for tests/telemetry.
    #[must_use]
    pub fn output_workspace_identity(&self) -> (*const f32, usize) {
        (self.output.as_ptr(), self.output.capacity())
    }

    /// Returns the fixed finite-finish workspace address and capacity.
    #[must_use]
    pub fn finish_workspace_identity(&self) -> (*const f32, usize) {
        (self.finish_output.as_ptr(), self.finish_output.capacity())
    }

    /// Consumes the withheld final decoded frame or retries retained publication.
    ///
    /// The method is worker/control-thread only. It invokes adaptive terminal
    /// conversion exactly once for [`FinitePlaybackEnd::Final`], retains valid
    /// output across ring backpressure, performs at most one all-or-none ring
    /// write per call, and never allocates.
    pub fn finish_finite(
        &mut self,
        end: FinitePlaybackEnd<'_>,
    ) -> Result<PlaybackFinishReport, PlaybackFinishError> {
        match self.state {
            PlaybackWorkerState::Faulted => {
                return Err(self.finish_error(PlaybackFinishErrorCause::Faulted, 0, 0, 0));
            }
            PlaybackWorkerState::Finished => {
                return match end {
                    FinitePlaybackEnd::Continue => Ok(self.finish_report(0, 0, 0)),
                    FinitePlaybackEnd::Final(_) | FinitePlaybackEnd::Empty => {
                        Err(self.finish_error(PlaybackFinishErrorCause::InvalidTransition, 0, 0, 0))
                    }
                };
            }
            PlaybackWorkerState::Finishing => {
                return match end {
                    FinitePlaybackEnd::Continue => self.publish_finish_output(0, 0),
                    FinitePlaybackEnd::Final(_) | FinitePlaybackEnd::Empty => {
                        Err(self.finish_error(PlaybackFinishErrorCause::InvalidTransition, 0, 0, 0))
                    }
                };
            }
            PlaybackWorkerState::Running => {}
        }

        match end {
            FinitePlaybackEnd::Continue => {
                Err(self.finish_error(PlaybackFinishErrorCause::InvalidTransition, 0, 0, 0))
            }
            FinitePlaybackEnd::Empty => self.finish_empty(),
            FinitePlaybackEnd::Final(input) => self.finish_final(input),
        }
    }

    fn finish_empty(&mut self) -> Result<PlaybackFinishReport, PlaybackFinishError> {
        if self.finite_saw_input {
            return Err(self.finish_error(PlaybackFinishErrorCause::MissingFinalFrame, 0, 0, 0));
        }
        if !self.finite_integrity {
            return Err(self.finish_error(PlaybackFinishErrorCause::PriorPlaybackLoss, 0, 0, 0));
        }
        if self.producer.is_disconnected() {
            self.state = PlaybackWorkerState::Faulted;
            self.finite_integrity = false;
            return Err(self.finish_error(PlaybackFinishErrorCause::RendererDisconnected, 0, 0, 0));
        }
        self.state = PlaybackWorkerState::Finished;
        Ok(self.finish_report(0, 0, 0))
    }

    fn finish_final(
        &mut self,
        input: FinitePlaybackInput<'_>,
    ) -> Result<PlaybackFinishReport, PlaybackFinishError> {
        if !self.finite_integrity {
            return Err(self.finish_error(PlaybackFinishErrorCause::PriorPlaybackLoss, 0, 0, 0));
        }
        let requirements = match self.converter.finish_requirements() {
            Ok(requirements) => requirements,
            Err(error) => {
                self.state = PlaybackWorkerState::Faulted;
                self.finite_integrity = false;
                return Err(self.finish_error(PlaybackFinishErrorCause::Resampler(error), 0, 0, 0));
            }
        };
        let maximum = requirements.final_input_frames;
        if !(1..=maximum).contains(&input.valid_media_frames) {
            return Err(self.finish_error(
                PlaybackFinishErrorCause::InvalidValidMediaFrames {
                    valid: input.valid_media_frames,
                    maximum,
                },
                0,
                0,
                0,
            ));
        }
        let expected_samples = match maximum.checked_mul(self.pipeline.channels()) {
            Some(samples) => samples,
            None => {
                self.state = PlaybackWorkerState::Faulted;
                self.finite_integrity = false;
                return Err(self.finish_error(
                    PlaybackFinishErrorCause::Resampler(ResampleError::FrameCountOverflow),
                    0,
                    0,
                    0,
                ));
            }
        };
        if input.frame.samples().len() != expected_samples {
            return Err(self.finish_error(
                PlaybackFinishErrorCause::InvalidFinalFrameSamples {
                    expected: expected_samples,
                    actual: input.frame.samples().len(),
                },
                0,
                0,
                0,
            ));
        }
        let valid_samples = input.valid_media_frames * self.pipeline.channels();
        if let Some(sample_index) = input.frame.samples()[..valid_samples]
            .iter()
            .position(|sample| !sample.is_finite())
        {
            return Err(self.finish_error(
                PlaybackFinishErrorCause::Resampler(ResampleError::NonFiniteInput { sample_index }),
                0,
                0,
                0,
            ));
        }
        if self.producer.is_disconnected() {
            self.state = PlaybackWorkerState::Faulted;
            self.finite_integrity = false;
            return Err(self.finish_error(PlaybackFinishErrorCause::RendererDisconnected, 0, 0, 0));
        }

        let observation = PlayoutClockObservation::from_scheduled_playout(
            input.remote_media_sample_position.get(),
            input.scheduled_local_device_frame,
        );
        match self.estimator.observe_scheduled_playout(observation) {
            Ok(DriftEstimatorUpdate::WarmingUp) => {}
            Ok(DriftEstimatorUpdate::EstimatePpm(ppm)) => self.latest_drift_ppm = Some(ppm),
            Ok(DriftEstimatorUpdate::Discontinuity(reason)) => {
                self.metrics.clock_discontinuities =
                    self.metrics.clock_discontinuities.saturating_add(1);
                self.state = PlaybackWorkerState::Faulted;
                self.finite_integrity = false;
                return Err(self.finish_error(
                    PlaybackFinishErrorCause::ClockDiscontinuity(reason),
                    0,
                    0,
                    0,
                ));
            }
            Err(error) => {
                return Err(self.finish_error(PlaybackFinishErrorCause::Clock(error), 0, 0, 0));
            }
        }

        let report = match self.converter.finish_interleaved(
            input.frame.samples(),
            input.valid_media_frames,
            &mut self.finish_output,
        ) {
            Ok(report) => report,
            Err(error) => {
                self.state = PlaybackWorkerState::Faulted;
                self.finite_integrity = false;
                return Err(self.finish_error(PlaybackFinishErrorCause::Resampler(error), 0, 0, 0));
            }
        };

        self.finish_generated_frames = report.generated_output_frames;
        self.finish_valid_frames = report.output_frames;
        self.finish_cursor_samples = 0;
        self.finish_leading_trim_frames = report.leading_trim_frames;
        self.finish_trailing_trim_frames = report.trailing_trim_frames;
        self.finite_saw_input = true;
        self.metrics.input_frames = self
            .metrics
            .input_frames
            .saturating_add(report.valid_input_frames as u64);
        self.metrics.output_frames = self
            .metrics
            .output_frames
            .saturating_add(report.generated_output_frames as u64);
        self.state = PlaybackWorkerState::Finishing;
        self.publish_finish_output(report.valid_input_frames, report.generated_output_frames)
    }

    fn publish_finish_output(
        &mut self,
        input_frames_consumed: usize,
        generated_output_frames: usize,
    ) -> Result<PlaybackFinishReport, PlaybackFinishError> {
        let channels = self.pipeline.channels();
        let valid_samples = self.finish_valid_frames * channels;
        let pending_samples = valid_samples - self.finish_cursor_samples;
        if pending_samples == 0 {
            self.state = PlaybackWorkerState::Finished;
            return Ok(self.finish_report(input_frames_consumed, generated_output_frames, 0));
        }
        if self.producer.is_disconnected() {
            self.state = PlaybackWorkerState::Faulted;
            self.finite_integrity = false;
            return Err(self.finish_error(
                PlaybackFinishErrorCause::RendererDisconnected,
                input_frames_consumed,
                generated_output_frames,
                0,
            ));
        }

        let available_samples = self.producer.available_samples();
        let whole_frame_samples = available_samples - available_samples % channels;
        let publish_samples = pending_samples.min(whole_frame_samples);
        if publish_samples == 0 {
            self.state = PlaybackWorkerState::Finishing;
            return Ok(self.finish_report(input_frames_consumed, generated_output_frames, 0));
        }

        let start = self.finish_cursor_samples;
        let end = start + publish_samples;
        match self.producer.write(&self.finish_output[start..end]) {
            WriteOutcome::Written { .. } => {
                self.finish_cursor_samples = end;
                self.metrics.published_chunks = self.metrics.published_chunks.saturating_add(1);
                if end == valid_samples {
                    self.state = PlaybackWorkerState::Finished;
                } else {
                    self.state = PlaybackWorkerState::Finishing;
                }
                Ok(self.finish_report(
                    input_frames_consumed,
                    generated_output_frames,
                    publish_samples / channels,
                ))
            }
            WriteOutcome::DroppedFull { .. } => {
                self.state = PlaybackWorkerState::Faulted;
                self.finite_integrity = false;
                self.metrics.dropped_full_chunks =
                    self.metrics.dropped_full_chunks.saturating_add(1);
                Err(self.finish_error(
                    PlaybackFinishErrorCause::PublicationInvariant,
                    input_frames_consumed,
                    generated_output_frames,
                    0,
                ))
            }
            WriteOutcome::Disconnected { .. } => {
                self.state = PlaybackWorkerState::Faulted;
                self.finite_integrity = false;
                self.metrics.disconnected_chunks =
                    self.metrics.disconnected_chunks.saturating_add(1);
                Err(self.finish_error(
                    PlaybackFinishErrorCause::RendererDisconnected,
                    input_frames_consumed,
                    generated_output_frames,
                    0,
                ))
            }
        }
    }

    fn finish_report(
        &self,
        input_frames_consumed: usize,
        generated_output_frames: usize,
        published_output_frames: usize,
    ) -> PlaybackFinishReport {
        let channels = self.pipeline.channels();
        let valid_samples = self.finish_valid_frames * channels;
        let pending_output_frames =
            valid_samples.saturating_sub(self.finish_cursor_samples) / channels;
        PlaybackFinishReport {
            status: if pending_output_frames == 0 {
                PlaybackFinishStatus::Finished
            } else {
                PlaybackFinishStatus::PendingRing
            },
            input_frames_consumed,
            generated_output_frames,
            valid_output_frames: self.finish_valid_frames,
            published_output_frames,
            leading_trim_frames: self.finish_leading_trim_frames,
            trailing_trim_frames: self.finish_trailing_trim_frames,
            pending_output_frames,
            queued_playback_frames: self.ring_fill_frames(),
        }
    }

    fn finish_error(
        &self,
        cause: PlaybackFinishErrorCause,
        input_frames_consumed: usize,
        generated_output_frames: usize,
        published_output_frames: usize,
    ) -> PlaybackFinishError {
        PlaybackFinishError {
            cause,
            progress: self.finish_report(
                input_frames_consumed,
                generated_output_frames,
                published_output_frames,
            ),
        }
    }

    /// Writes already-converted device-rate PCM straight into the playback ring.
    ///
    /// Used by the LAN path so home-network audio skips Opus, FEC lookahead,
    /// and adaptive resampling delay when the device is already 48 kHz.
    #[must_use]
    pub fn push_direct(&mut self, interleaved: &[f32]) -> WriteOutcome {
        if interleaved.is_empty() {
            return WriteOutcome::Written { samples: 0 };
        }
        self.producer.write(interleaved)
    }

    /// Converts and publishes one resolved RX frame.
    ///
    /// `remote_media_sample_position` is the extended 48 kHz RTP timestamp
    /// chosen by the playout scheduler. `scheduled_local_device_frame` is the
    /// corresponding monotonic device-frame position. Packet arrival or socket
    /// time cannot be supplied through this interface.
    pub fn process_frame(
        &mut self,
        frame: &PcmFrame,
        remote_media_sample_position: ExtendedTimestamp,
        scheduled_local_device_frame: u64,
    ) -> Result<PlaybackProcessReport, PlaybackProcessError> {
        self.process_samples(
            frame.samples(),
            remote_media_sample_position,
            scheduled_local_device_frame,
        )
    }

    fn process_samples(
        &mut self,
        samples: &[f32],
        remote_media_sample_position: ExtendedTimestamp,
        scheduled_local_device_frame: u64,
    ) -> Result<PlaybackProcessReport, PlaybackProcessError> {
        match self.state {
            PlaybackWorkerState::Running => {}
            PlaybackWorkerState::Faulted => return Err(PlaybackProcessError::Faulted),
            PlaybackWorkerState::Finishing | PlaybackWorkerState::Finished => {
                return Err(PlaybackProcessError::EndOfStream);
            }
        }
        let requirements = self.converter.requirements();
        let expected = requirements.input_frames_next * self.pipeline.channels();
        if samples.len() != expected {
            return Err(PlaybackProcessError::InvalidFrameSamples {
                expected,
                actual: samples.len(),
            });
        }
        if let Some(sample_index) = samples.iter().position(|sample| !sample.is_finite()) {
            return Err(PlaybackProcessError::Resampler(
                ResampleError::NonFiniteInput { sample_index },
            ));
        }

        let observation = PlayoutClockObservation::from_scheduled_playout(
            remote_media_sample_position.get(),
            scheduled_local_device_frame,
        );
        match self.estimator.observe_scheduled_playout(observation) {
            Ok(DriftEstimatorUpdate::WarmingUp) => {}
            Ok(DriftEstimatorUpdate::EstimatePpm(ppm)) => self.latest_drift_ppm = Some(ppm),
            Ok(DriftEstimatorUpdate::Discontinuity(reason)) => {
                self.metrics.clock_discontinuities =
                    self.metrics.clock_discontinuities.saturating_add(1);
                self.state = PlaybackWorkerState::Faulted;
                self.finite_integrity = false;
                return Err(PlaybackProcessError::ClockDiscontinuity(reason));
            }
            Err(error) => return Err(PlaybackProcessError::Clock(error)),
        }

        let report = match self
            .converter
            .process_interleaved(samples, &mut self.output)
        {
            Ok(report) => report,
            Err(
                error @ (ResampleError::NonFiniteInput { .. }
                | ResampleError::InvalidInputLength { .. }
                | ResampleError::OutputBufferTooSmall { .. }),
            ) => {
                return Err(PlaybackProcessError::Resampler(error));
            }
            Err(error) => {
                self.state = PlaybackWorkerState::Faulted;
                self.finite_integrity = false;
                return Err(PlaybackProcessError::Resampler(error));
            }
        };
        self.finite_saw_input = true;
        let output_samples = report.output_frames * self.pipeline.channels();
        let applied_correction_ppm = self.converter.smoothed_correction_ppm();
        let publication = match self.producer.write(&self.output[..output_samples]) {
            WriteOutcome::Written { .. } => {
                self.metrics.published_chunks = self.metrics.published_chunks.saturating_add(1);
                PlaybackPublication::Published
            }
            WriteOutcome::DroppedFull { .. } => {
                self.finite_integrity = false;
                self.metrics.dropped_full_chunks =
                    self.metrics.dropped_full_chunks.saturating_add(1);
                PlaybackPublication::DroppedFull
            }
            WriteOutcome::Disconnected { .. } => {
                self.finite_integrity = false;
                self.metrics.disconnected_chunks =
                    self.metrics.disconnected_chunks.saturating_add(1);
                PlaybackPublication::RendererDisconnected
            }
        };
        self.metrics.input_frames = self
            .metrics
            .input_frames
            .saturating_add(report.input_frames as u64);
        self.metrics.output_frames = self
            .metrics
            .output_frames
            .saturating_add(report.output_frames as u64);

        let ring_fill_frames = self.ring_fill_frames();
        let (controller, control_fault) =
            self.update_controller_if_due(scheduled_local_device_frame, ring_fill_frames);
        if self.state == PlaybackWorkerState::Faulted {
            self.finite_integrity = false;
        }

        Ok(PlaybackProcessReport {
            input_frames: report.input_frames,
            output_frames: report.output_frames,
            publication,
            ring_fill_frames,
            estimated_remote_drift_ppm: self.latest_drift_ppm,
            controller,
            control_fault,
            applied_correction_ppm,
            target_correction_ppm: self.converter.target_correction_ppm(),
        })
    }

    fn update_controller_if_due(
        &mut self,
        scheduled_local_device_frame: u64,
        ring_fill_frames: usize,
    ) -> (Option<ClockRecoveryOutput>, Option<PlaybackControlFault>) {
        let Some(last) = self.last_control_device_frame else {
            self.last_control_device_frame = Some(scheduled_local_device_frame);
            return (None, None);
        };
        let Some(elapsed_frames) = scheduled_local_device_frame.checked_sub(last) else {
            self.state = PlaybackWorkerState::Faulted;
            return (
                None,
                Some(PlaybackControlFault::Clock(
                    ClockError::NonPositiveLocalInterval,
                )),
            );
        };
        if elapsed_frames < self.pipeline.controller_cadence_frames() as u64 {
            return (None, None);
        }
        let elapsed_seconds = elapsed_frames as f64 / self.pipeline.playback_rate_hz() as f64;
        let fill_error = ring_fill_frames as f64 - self.target_fill_frames as f64;
        let output = match self.recovery.update(
            self.latest_drift_ppm.unwrap_or(0.0),
            fill_error,
            elapsed_seconds,
        ) {
            Ok(output) => output,
            Err(error) => {
                self.state = PlaybackWorkerState::Faulted;
                return (None, Some(PlaybackControlFault::Clock(error)));
            }
        };
        let correction =
            match OutputInputRatioCorrectionPpm::from_ratio_multiplier(output.ratio_multiplier) {
                Ok(correction) => correction,
                Err(error) => {
                    self.state = PlaybackWorkerState::Faulted;
                    return (None, Some(PlaybackControlFault::Resampler(error)));
                }
            };
        self.converter.set_output_input_correction(correction);
        self.last_control_device_frame = Some(scheduled_local_device_frame);
        self.metrics.controller_updates = self.metrics.controller_updates.saturating_add(1);
        (Some(output), None)
    }

    /// Clears stateful clock/SRC history only after all old-epoch audio drained.
    ///
    /// If the ring is nonempty, stop the device callback and recreate the pair
    /// instead; mixing queued pre-reset audio with a new epoch is prohibited.
    pub fn reset_when_empty(&mut self) -> Result<(), PlaybackResetError> {
        let queued_samples = self.ring_capacity_samples - self.producer.available_samples();
        if queued_samples != 0 {
            return Err(PlaybackResetError::RingNotEmpty { queued_samples });
        }
        let pending_frames = self
            .finish_valid_frames
            .saturating_mul(self.pipeline.channels())
            .saturating_sub(self.finish_cursor_samples)
            / self.pipeline.channels();
        if self.state == PlaybackWorkerState::Finishing && pending_frames != 0 {
            return Err(PlaybackResetError::FinishPending { pending_frames });
        }
        self.estimator.reset();
        self.recovery.reset();
        self.converter.reset();
        self.finish_generated_frames = 0;
        self.finish_valid_frames = 0;
        self.finish_cursor_samples = 0;
        self.finish_leading_trim_frames = 0;
        self.finish_trailing_trim_frames = 0;
        self.finite_saw_input = false;
        self.finite_integrity = true;
        self.latest_drift_ppm = None;
        self.last_control_device_frame = None;
        self.state = PlaybackWorkerState::Running;
        self.metrics.resets = self.metrics.resets.saturating_add(1);
        Ok(())
    }
}

impl PlaybackRenderer {
    /// Initializes the full device output, copying available audio then leaving
    /// silence for every missing sample.
    ///
    /// A misaligned scalar count is entirely zeroed and does not consume the
    /// ring. The embedding device setup must normally guarantee alignment.
    #[must_use]
    pub fn render(&mut self, output: &mut [f32]) -> RenderReport {
        output.fill(0.0);
        let requested_samples = output.len();
        if !requested_samples.is_multiple_of(self.channels) {
            return RenderReport {
                requested_samples,
                rendered_samples: 0,
                zeroed_samples: requested_samples,
                state: RenderState::Misaligned,
            };
        }
        let outcome = self.consumer.read(output);
        let state = match outcome.state {
            ReadState::Complete => RenderState::Complete,
            ReadState::Underrun => RenderState::Underrun,
            ReadState::Disconnected if self.consumer.available_samples() == 0 => {
                RenderState::Disconnected
            }
            ReadState::Disconnected if outcome.read_samples == requested_samples => {
                RenderState::Complete
            }
            ReadState::Disconnected => RenderState::Underrun,
        };
        RenderReport {
            requested_samples,
            rendered_samples: outcome.read_samples,
            zeroed_samples: requested_samples - outcome.read_samples,
            state,
        }
    }

    /// Returns the currently readable scalar-sample count.
    #[must_use]
    pub fn available_samples(&self) -> usize {
        self.consumer.available_samples()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioPipelineConfigInput, FrameDuration, MAX_PACKET_BYTES};
    use relay_clock::ClockRecoveryConfig;
    use relay_resample::AdaptiveClockConfig;

    fn pipeline(rate: usize, duration: FrameDuration) -> AudioPipelineConfig {
        AudioPipelineConfig::new(AudioPipelineConfigInput {
            capture_rate_hz: 48_000,
            playback_rate_hz: rate,
            channels: 2,
            frame_duration: duration,
            capture_src_chunk_frames: 480,
            capture_ring_samples: 100_000,
            playback_ring_samples: 100_000,
            tx_accumulator_samples: 100_000,
            reorder_capacity: 64,
            network_capacity: 8,
            network_due_batch_capacity: 4,
            packet_capacity: MAX_PACKET_BYTES,
            controller_cadence_frames: rate / 100,
            clock_recovery: ClockRecoveryConfig::default(),
            adaptive_clock: AdaptiveClockConfig::default(),
        })
        .expect("valid playback pipeline")
    }

    fn pipeline_with_minimum_ring(rate: usize, duration: FrameDuration) -> AudioPipelineConfig {
        let spacious = pipeline(rate, duration);
        AudioPipelineConfig::new(AudioPipelineConfigInput {
            capture_rate_hz: 48_000,
            playback_rate_hz: rate,
            channels: 2,
            frame_duration: duration,
            capture_src_chunk_frames: 480,
            capture_ring_samples: 100_000,
            playback_ring_samples: spacious.minimum_playback_ring_samples(),
            tx_accumulator_samples: 100_000,
            reorder_capacity: 64,
            network_capacity: 8,
            network_due_batch_capacity: 4,
            packet_capacity: MAX_PACKET_BYTES,
            controller_cadence_frames: rate / 100,
            clock_recovery: ClockRecoveryConfig::default(),
            adaptive_clock: AdaptiveClockConfig::default(),
        })
        .expect("minimum playback ring pipeline")
    }

    fn frame(duration: FrameDuration) -> PcmFrame {
        PcmFrame::from_test_samples(&samples(duration))
    }

    fn final_input<'a>(
        frame: &'a PcmFrame,
        valid_media_frames: usize,
        remote: u64,
        local: u64,
    ) -> FinitePlaybackEnd<'a> {
        FinitePlaybackEnd::Final(FinitePlaybackInput {
            frame,
            valid_media_frames,
            remote_media_sample_position: ExtendedTimestamp::new(remote),
            scheduled_local_device_frame: local,
        })
    }

    fn samples(duration: FrameDuration) -> Vec<f32> {
        (0..duration.interleaved_samples())
            .map(|index| ((index % 97) as f32 - 48.0) / 96.0)
            .collect()
    }

    fn local_position(packet: u64, duration: FrameDuration, rate: usize) -> u64 {
        let remote = packet * duration.samples_per_channel() as u64;
        (remote * rate as u64 + 24_000) / 48_000
    }

    #[test]
    fn construction_rejects_invalid_target_and_clock_domains() {
        let pipeline = pipeline(48_000, FrameDuration::Ms10);
        let mut config = PlaybackConfig::for_pipeline(pipeline);
        config.target_fill_frames = 0;
        assert_eq!(
            playback_pair(pipeline, config)
                .err()
                .expect("invalid config"),
            PlaybackBuildError::InvalidTargetFillFrames {
                target: 0,
                capacity: 50_000,
            }
        );

        let mut config = PlaybackConfig::for_pipeline(pipeline);
        config.drift_estimator.local_device_sample_rate_hz = 44_100.0;
        assert_eq!(
            playback_pair(pipeline, config)
                .err()
                .expect("invalid config"),
            PlaybackBuildError::MismatchedEstimatorClockDomains
        );
    }

    #[test]
    fn every_rate_and_duration_publishes_finite_fixed_workspace_audio() {
        for rate in [44_100, 48_000, 96_000, 192_000] {
            for duration in [FrameDuration::Ms5, FrameDuration::Ms10, FrameDuration::Ms20] {
                let pipeline = pipeline(rate, duration);
                let config = PlaybackConfig::for_pipeline(pipeline);
                let (mut worker, mut renderer, _) =
                    playback_pair(pipeline, config).expect("playback pair");
                let identity = worker.output_workspace_identity();
                let input = samples(duration);
                for packet in 0..4 {
                    let report = worker
                        .process_samples(
                            &input,
                            ExtendedTimestamp::new(packet * duration.samples_per_channel() as u64),
                            local_position(packet, duration, rate),
                        )
                        .expect("scheduled frame");
                    assert_eq!(report.input_frames, duration.samples_per_channel());
                    assert_eq!(report.publication, PlaybackPublication::Published);
                    assert!(
                        report.output_frames
                            <= pipeline.adaptive_resampler_requirements().output_frames_max
                    );
                    let mut output = vec![f32::NAN; report.output_frames * 2];
                    let rendered = renderer.render(&mut output);
                    assert_eq!(rendered.state, RenderState::Complete);
                    assert!(output.iter().all(|sample| sample.is_finite()));
                }
                assert_eq!(worker.output_workspace_identity(), identity);
            }
        }
    }

    #[test]
    fn renderer_zero_fills_starvation_misalignment_and_resumes_without_consuming_odd_call() {
        let pipeline = pipeline(48_000, FrameDuration::Ms10);
        let (mut worker, mut renderer, metrics) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("pair");
        let mut starved = [1.0; 64];
        let report = renderer.render(&mut starved);
        assert_eq!(report.state, RenderState::Underrun);
        assert_eq!(report.zeroed_samples, 64);
        assert!(starved.iter().all(|sample| *sample == 0.0));

        let input = samples(FrameDuration::Ms10);
        let published = worker
            .process_samples(&input, ExtendedTimestamp::new(0), 0)
            .expect("publish");
        let queued = renderer.available_samples();
        let mut odd = [1.0; 3];
        assert_eq!(renderer.render(&mut odd).state, RenderState::Misaligned);
        assert_eq!(renderer.available_samples(), queued);
        assert_eq!(odd, [0.0; 3]);

        let requested = published.output_frames * 2 + 32;
        let mut resumed = vec![f32::NAN; requested];
        let resumed_report = renderer.render(&mut resumed);
        assert_eq!(resumed_report.state, RenderState::Underrun);
        assert_eq!(resumed_report.rendered_samples, published.output_frames * 2);
        assert!(
            resumed[resumed_report.rendered_samples..]
                .iter()
                .all(|sample| *sample == 0.0)
        );
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.underruns, 2);
        assert_eq!(snapshot.underrun_samples, 96);
    }

    #[test]
    fn full_ring_drops_complete_new_chunks_and_metrics_are_explicit() {
        let mut input_config = PlaybackConfig::for_pipeline(pipeline(48_000, FrameDuration::Ms20));
        input_config.target_fill_frames = 1;
        let pipeline = pipeline(48_000, FrameDuration::Ms20);
        let (mut worker, _renderer, metrics) = playback_pair(pipeline, input_config).expect("pair");
        let input = samples(FrameDuration::Ms20);
        let mut saw_drop = false;
        for packet in 0..100 {
            let report = worker
                .process_samples(&input, ExtendedTimestamp::new(packet * 960), packet * 960)
                .expect("bounded publication");
            if report.publication == PlaybackPublication::DroppedFull {
                saw_drop = true;
                break;
            }
        }
        assert!(saw_drop);
        assert_eq!(worker.metrics().dropped_full_chunks, 1);
        assert!(metrics.snapshot().dropped_samples > 0);
    }

    #[test]
    fn scheduled_remote_drift_produces_the_correct_output_input_sign() {
        let mut input = AudioPipelineConfigInput {
            capture_rate_hz: 48_000,
            playback_rate_hz: 48_000,
            channels: 2,
            frame_duration: FrameDuration::Ms10,
            capture_src_chunk_frames: 480,
            capture_ring_samples: 100_000,
            playback_ring_samples: 100_000,
            tx_accumulator_samples: 100_000,
            reorder_capacity: 64,
            network_capacity: 8,
            network_due_batch_capacity: 4,
            packet_capacity: MAX_PACKET_BYTES,
            controller_cadence_frames: 480,
            clock_recovery: ClockRecoveryConfig::default(),
            adaptive_clock: AdaptiveClockConfig::default(),
        };
        input.clock_recovery.proportional_gain_ppm_per_frame = 0.0;
        input.clock_recovery.integral_gain_ppm_per_frame_second = 0.0;
        let pipeline = AudioPipelineConfig::new(input).expect("pipeline");
        let mut config = PlaybackConfig::for_pipeline(pipeline);
        config.drift_estimator.observation_window_seconds = 0.01;
        config.drift_estimator.smoothing_factor = 1.0;
        let (mut worker, mut renderer, _) = playback_pair(pipeline, config).expect("pair");
        let samples = samples(FrameDuration::Ms10);
        let mut last = None;
        for packet in 0..8_u64 {
            let report = worker
                .process_samples(&samples, ExtendedTimestamp::new(packet * 480), packet * 479)
                .expect("scheduled frame");
            let mut drain = vec![0.0; report.output_frames * 2];
            let _ = renderer.render(&mut drain);
            last = Some(report);
        }
        let report = last.expect("report");
        assert!(report.estimated_remote_drift_ppm.expect("estimate") > 0.0);
        assert!(report.target_correction_ppm < 0.0);
    }

    #[test]
    fn zero_mean_scheduled_quantization_does_not_become_remote_drift() {
        let pipeline = pipeline(48_000, FrameDuration::Ms10);
        let mut config = PlaybackConfig::for_pipeline(pipeline);
        config.drift_estimator.observation_window_seconds = 0.02;
        config.drift_estimator.smoothing_factor = 1.0;
        let (mut worker, mut renderer, _) = playback_pair(pipeline, config).expect("pair");
        let input = samples(FrameDuration::Ms10);
        let mut last_estimate = None;
        for packet in 0..20_u64 {
            let local_jitter = if packet.is_multiple_of(2) { 0 } else { 1 };
            let report = worker
                .process_samples(
                    &input,
                    ExtendedTimestamp::new(packet * 480),
                    packet * 480 + local_jitter,
                )
                .expect("quantized scheduled position");
            let mut drain = vec![0.0; report.output_frames * 2];
            let _ = renderer.render(&mut drain);
            if report.estimated_remote_drift_ppm.is_some() {
                last_estimate = report.estimated_remote_drift_ppm;
            }
        }
        assert!(last_estimate.expect("warmed estimate").abs() < f64::EPSILON);
    }

    #[test]
    fn post_publication_control_fault_retains_exact_progress() {
        let pipeline = pipeline(48_000, FrameDuration::Ms10);
        let (mut worker, mut renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("pair");
        let input = samples(FrameDuration::Ms10);
        let first = worker
            .process_samples(&input, ExtendedTimestamp::new(0), 0)
            .expect("first");
        let mut drain = vec![0.0; first.output_frames * 2];
        let _ = renderer.render(&mut drain);
        let report = worker
            .process_samples(&input, ExtendedTimestamp::new(480), 48_000)
            .expect("publication progress remains reportable");
        assert_eq!(report.publication, PlaybackPublication::Published);
        assert_eq!(
            report.control_fault,
            Some(PlaybackControlFault::Clock(
                ClockError::UpdateIntervalTooLong
            ))
        );
        assert_eq!(worker.state(), PlaybackWorkerState::Faulted);
        assert_eq!(worker.metrics().published_chunks, 2);
    }

    #[test]
    fn discontinuity_faults_until_the_old_ring_drains_and_reset_reuses_storage() {
        let pipeline = pipeline(48_000, FrameDuration::Ms10);
        let (mut worker, mut renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("pair");
        let input = samples(FrameDuration::Ms10);
        let identity = worker.output_workspace_identity();
        let first = worker
            .process_samples(&input, ExtendedTimestamp::new(1_000), 0)
            .expect("first");
        assert_eq!(
            worker.process_samples(&input, ExtendedTimestamp::new(1_000), 480),
            Err(PlaybackProcessError::ClockDiscontinuity(
                DiscontinuityReason::RemoteStall
            ))
        );
        assert_eq!(worker.state(), PlaybackWorkerState::Faulted);
        assert!(matches!(
            worker.reset_when_empty(),
            Err(PlaybackResetError::RingNotEmpty { .. })
        ));
        let mut drain = vec![0.0; first.output_frames * 2];
        assert_eq!(renderer.render(&mut drain).state, RenderState::Complete);
        worker.reset_when_empty().expect("empty reset");
        assert_eq!(worker.state(), PlaybackWorkerState::Running);
        assert_eq!(worker.output_workspace_identity(), identity);
        worker
            .process_samples(&input, ExtendedTimestamp::new(2_000), 2_000)
            .expect("new epoch");
    }

    #[test]
    fn nonfinite_input_is_recoverable_and_renderer_reports_disconnect() {
        let pipeline = pipeline(48_000, FrameDuration::Ms5);
        let (mut worker, mut renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("pair");
        let mut invalid = samples(FrameDuration::Ms5);
        invalid[3] = f32::NAN;
        assert!(matches!(
            worker.process_samples(&invalid, ExtendedTimestamp::new(0), 0),
            Err(PlaybackProcessError::Resampler(
                ResampleError::NonFiniteInput { sample_index: 3 }
            ))
        ));
        assert_eq!(worker.state(), PlaybackWorkerState::Running);
        worker.reset_when_empty().expect("clear observation anchor");
        drop(worker);
        let mut output = [1.0; 32];
        let report = renderer.render(&mut output);
        assert_eq!(report.state, RenderState::Disconnected);
        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn finite_finish_covers_all_playback_rates_durations_and_valid_prefixes() {
        for rate in [44_100, 48_000, 96_000, 192_000] {
            for duration in [FrameDuration::Ms5, FrameDuration::Ms10, FrameDuration::Ms20] {
                let packet_frames = duration.samples_per_channel();
                for valid_media_frames in [1, packet_frames - 1, packet_frames] {
                    let pipeline = pipeline(rate, duration);
                    let (mut worker, mut renderer, _) =
                        playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline))
                            .expect("finite playback pair");
                    let frame = frame(duration);
                    let identity = worker.finish_workspace_identity();
                    let first = worker
                        .finish_finite(final_input(&frame, valid_media_frames, 0, 0))
                        .expect("valid finite final");
                    assert_eq!(first.input_frames_consumed, valid_media_frames);
                    assert_eq!(
                        first.valid_output_frames,
                        first.generated_output_frames - first.trailing_trim_frames
                    );
                    assert_eq!(
                        first.valid_output_frames,
                        first.published_output_frames + first.pending_output_frames
                    );
                    assert!(first.leading_trim_frames <= first.generated_output_frames);

                    let mut report = first;
                    let mut published = first.published_output_frames;
                    while report.status == PlaybackFinishStatus::PendingRing {
                        let available = renderer.available_samples();
                        let mut rendered = vec![f32::NAN; available];
                        let rendered_report = renderer.render(&mut rendered);
                        assert_eq!(rendered_report.rendered_samples, available);
                        assert!(rendered.iter().all(|sample| sample.is_finite()));
                        report = worker
                            .finish_finite(FinitePlaybackEnd::Continue)
                            .expect("bounded retained-tail retry");
                        published += report.published_output_frames;
                        assert_eq!(
                            report.valid_output_frames,
                            published + report.pending_output_frames
                        );
                    }
                    assert_eq!(published, first.valid_output_frames);
                    assert_eq!(worker.state(), PlaybackWorkerState::Finished);
                    assert_eq!(worker.finish_workspace_identity(), identity);
                }
            }
        }
    }

    fn collect_finish(
        worker: &mut PlaybackWorker,
        renderer: &mut PlaybackRenderer,
        frame: &PcmFrame,
        valid_media_frames: usize,
    ) -> (PlaybackFinishReport, Vec<f32>) {
        let first = worker
            .finish_finite(final_input(frame, valid_media_frames, 0, 0))
            .expect("finite final");
        let mut final_report = first;
        let mut output = Vec::new();
        loop {
            let available = renderer.available_samples();
            let old_len = output.len();
            output.resize(old_len + available, f32::NAN);
            if available != 0 {
                let rendered = renderer.render(&mut output[old_len..]);
                assert_eq!(rendered.rendered_samples, available);
            }
            if final_report.status == PlaybackFinishStatus::Finished {
                break;
            }
            final_report = worker
                .finish_finite(FinitePlaybackEnd::Continue)
                .expect("retry retained finish output");
        }
        (first, output)
    }

    #[test]
    fn ring_full_retries_match_one_shot_without_drop_loss_or_duplication() {
        let duration = FrameDuration::Ms5;
        let frame = frame(duration);
        let valid = duration.samples_per_channel() - 1;

        let spacious = pipeline(192_000, duration);
        let (mut reference_worker, mut reference_renderer, _) =
            playback_pair(spacious, PlaybackConfig::for_pipeline(spacious)).expect("reference");
        let (reference_report, reference) = collect_finish(
            &mut reference_worker,
            &mut reference_renderer,
            &frame,
            valid,
        );

        let bounded = pipeline_with_minimum_ring(192_000, duration);
        let (mut worker, mut renderer, metrics) =
            playback_pair(bounded, PlaybackConfig::for_pipeline(bounded)).expect("bounded");
        let identity = worker.finish_workspace_identity();
        let (report, actual) = collect_finish(&mut worker, &mut renderer, &frame, valid);
        assert_eq!(report.status, PlaybackFinishStatus::PendingRing);
        assert_eq!(
            report.generated_output_frames,
            reference_report.generated_output_frames
        );
        assert_eq!(
            report.valid_output_frames,
            reference_report.valid_output_frames
        );
        assert_eq!(
            report.leading_trim_frames,
            reference_report.leading_trim_frames
        );
        assert_eq!(
            report.trailing_trim_frames,
            reference_report.trailing_trim_frames
        );
        assert_eq!(actual, reference);
        assert_eq!(actual.len(), report.valid_output_frames * 2);
        assert_eq!(metrics.snapshot().dropped_samples, 0);
        assert_eq!(worker.metrics().dropped_full_chunks, 0);
        assert_eq!(worker.finish_workspace_identity(), identity);
        let repeated = worker
            .finish_finite(FinitePlaybackEnd::Continue)
            .expect("finished retry is idempotent");
        assert_eq!(repeated.status, PlaybackFinishStatus::Finished);
        assert_eq!(repeated.published_output_frames, 0);
        assert_eq!(repeated.pending_output_frames, 0);
    }

    #[test]
    fn empty_and_lifecycle_misuse_are_explicit_and_reset_preserves_pending() {
        let pipeline = pipeline(48_000, FrameDuration::Ms5);
        let (mut empty, _renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("empty pair");
        assert_eq!(
            empty
                .finish_finite(FinitePlaybackEnd::Continue)
                .expect_err("continue before end")
                .cause,
            PlaybackFinishErrorCause::InvalidTransition
        );
        let empty_report = empty
            .finish_finite(FinitePlaybackEnd::Empty)
            .expect("genuinely empty stream");
        assert_eq!(empty_report.status, PlaybackFinishStatus::Finished);
        assert_eq!(empty_report.valid_output_frames, 0);
        assert_eq!(empty_report.queued_playback_frames, 0);
        assert_eq!(
            empty
                .finish_finite(FinitePlaybackEnd::Empty)
                .expect_err("empty only once")
                .cause,
            PlaybackFinishErrorCause::InvalidTransition
        );

        let (mut worker, mut renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("live pair");
        let input = samples(FrameDuration::Ms5);
        let live = worker
            .process_samples(&input, ExtendedTimestamp::new(0), 0)
            .expect("live input");
        assert_eq!(
            worker
                .finish_finite(FinitePlaybackEnd::Empty)
                .expect_err("nonempty stream needs final frame")
                .cause,
            PlaybackFinishErrorCause::MissingFinalFrame
        );
        let mut live_drain = vec![0.0; live.output_frames * 2];
        let _ = renderer.render(&mut live_drain);

        let bounded = pipeline_with_minimum_ring(192_000, FrameDuration::Ms5);
        let (mut pending, mut pending_renderer, _) =
            playback_pair(bounded, PlaybackConfig::for_pipeline(bounded)).expect("pending pair");
        let frame = frame(FrameDuration::Ms5);
        let finish = pending
            .finish_finite(final_input(&frame, 239, 0, 0))
            .expect("pending final");
        assert_eq!(finish.status, PlaybackFinishStatus::PendingRing);
        let queued = pending_renderer.available_samples();
        let mut drain = vec![0.0; queued];
        let _ = pending_renderer.render(&mut drain);
        assert!(matches!(
            pending.reset_when_empty(),
            Err(PlaybackResetError::FinishPending { .. })
        ));
        while pending.state() == PlaybackWorkerState::Finishing {
            let _ = pending
                .finish_finite(FinitePlaybackEnd::Continue)
                .expect("continue");
            let queued = pending_renderer.available_samples();
            let mut drain = vec![0.0; queued];
            let _ = pending_renderer.render(&mut drain);
        }
        pending
            .reset_when_empty()
            .expect("reset completed finite state");
        assert_eq!(pending.state(), PlaybackWorkerState::Running);
    }

    #[test]
    fn invalid_manifest_prior_loss_and_disconnect_faults_are_sticky() {
        let duration = FrameDuration::Ms10;
        let pipeline = pipeline(48_000, duration);
        let frame = frame(duration);
        let (mut invalid, _renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("invalid pair");
        assert_eq!(
            invalid
                .finish_finite(final_input(&frame, 0, 0, 0))
                .expect_err("zero valid prefix")
                .cause,
            PlaybackFinishErrorCause::InvalidValidMediaFrames {
                valid: 0,
                maximum: 480,
            }
        );
        assert_eq!(invalid.state(), PlaybackWorkerState::Running);

        let bounded = pipeline_with_minimum_ring(48_000, duration);
        let (mut lossy, _renderer, _) =
            playback_pair(bounded, PlaybackConfig::for_pipeline(bounded)).expect("loss pair");
        let input = samples(duration);
        let mut packet = 0_u64;
        loop {
            let report = lossy
                .process_samples(&input, ExtendedTimestamp::new(packet * 480), packet * 480)
                .expect("live fill");
            packet += 1;
            if report.publication == PlaybackPublication::DroppedFull {
                break;
            }
        }
        assert_eq!(
            lossy
                .finish_finite(final_input(&frame, 480, packet * 480, packet * 480))
                .expect_err("loss cannot become complete")
                .cause,
            PlaybackFinishErrorCause::PriorPlaybackLoss
        );
        assert_eq!(lossy.state(), PlaybackWorkerState::Running);

        let (mut disconnected, renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("disconnect");
        drop(renderer);
        assert_eq!(
            disconnected
                .finish_finite(final_input(&frame, 480, 0, 0))
                .expect_err("renderer gone")
                .cause,
            PlaybackFinishErrorCause::RendererDisconnected
        );
        assert_eq!(disconnected.state(), PlaybackWorkerState::Faulted);
        assert_eq!(
            disconnected
                .finish_finite(FinitePlaybackEnd::Continue)
                .expect_err("fault sticks")
                .cause,
            PlaybackFinishErrorCause::Faulted
        );
    }

    #[test]
    fn disconnected_is_terminal_only_after_the_post_read_queue_is_empty() {
        let duration = FrameDuration::Ms5;
        let pipeline = pipeline(48_000, duration);
        let final_frame = frame(duration);

        let (mut reference_worker, mut reference_renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("reference");
        let reference_finish = reference_worker
            .finish_finite(final_input(&final_frame, 240, 0, 0))
            .expect("reference finish");
        assert_eq!(reference_finish.status, PlaybackFinishStatus::Finished);
        let reference_samples = reference_renderer.available_samples();
        assert!(reference_samples > 4);
        let mut reference = vec![f32::NAN; reference_samples];
        assert_eq!(
            reference_renderer.render(&mut reference).rendered_samples,
            reference_samples
        );

        let (mut worker, mut renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("subject");
        let finish = worker
            .finish_finite(final_input(&final_frame, 240, 0, 0))
            .expect("subject finish");
        assert_eq!(finish.status, PlaybackFinishStatus::Finished);
        assert_eq!(renderer.available_samples(), reference_samples);
        drop(worker);

        let mut actual = Vec::with_capacity(reference_samples);
        let mut terminal_acknowledgements = 0;
        while terminal_acknowledgements == 0 {
            let available_before = renderer.available_samples();
            assert_ne!(available_before, 0);
            let requested = 4;
            let old_len = actual.len();
            actual.resize(old_len + requested, f32::NAN);
            let report = renderer.render(&mut actual[old_len..]);
            actual.truncate(old_len + report.rendered_samples);
            let available_after = renderer.available_samples();
            if report.state == RenderState::Disconnected {
                terminal_acknowledgements += 1;
                assert_eq!(
                    available_after, 0,
                    "terminal acknowledgement left queued audio"
                );
            } else {
                assert_ne!(available_after, 0, "empty callback return was not terminal");
                assert_eq!(report.state, RenderState::Complete);
            }
        }
        assert_eq!(terminal_acknowledgements, 1);
        assert_eq!(actual.len(), reference_samples);
        assert_eq!(actual, reference);
    }

    #[test]
    fn repeated_final_and_renderer_loss_during_pending_finish_are_explicit() {
        let duration = FrameDuration::Ms5;
        let final_frame = frame(duration);

        let spacious = pipeline(48_000, duration);
        let (mut finished, _renderer, _) =
            playback_pair(spacious, PlaybackConfig::for_pipeline(spacious)).expect("finished pair");
        let first = finished
            .finish_finite(final_input(&final_frame, 239, 0, 0))
            .expect("first final");
        let metrics_after_first = finished.metrics();
        let repeated = finished
            .finish_finite(final_input(&final_frame, 239, 0, 0))
            .expect_err("Final is accepted exactly once");
        assert_eq!(repeated.cause, PlaybackFinishErrorCause::InvalidTransition);
        assert_eq!(
            repeated.progress.valid_output_frames,
            first.valid_output_frames
        );
        assert_eq!(
            repeated.progress.pending_output_frames,
            first.pending_output_frames
        );
        assert_eq!(finished.metrics(), metrics_after_first);

        let bounded = pipeline_with_minimum_ring(192_000, duration);
        let (mut pending, renderer, _) =
            playback_pair(bounded, PlaybackConfig::for_pipeline(bounded)).expect("pending pair");
        let partial = pending
            .finish_finite(final_input(&final_frame, 239, 0, 0))
            .expect("partial publication");
        assert_eq!(partial.status, PlaybackFinishStatus::PendingRing);
        assert!(partial.published_output_frames > 0);
        assert!(partial.pending_output_frames > 0);
        drop(renderer);
        let disconnected = pending
            .finish_finite(FinitePlaybackEnd::Continue)
            .expect_err("renderer loss retains the unpublished tail");
        assert_eq!(
            disconnected.cause,
            PlaybackFinishErrorCause::RendererDisconnected
        );
        assert_eq!(
            disconnected.progress.pending_output_frames,
            partial.pending_output_frames
        );
        assert_eq!(pending.state(), PlaybackWorkerState::Faulted);
        let sticky = pending
            .finish_finite(FinitePlaybackEnd::Continue)
            .expect_err("renderer loss is sticky");
        assert_eq!(sticky.cause, PlaybackFinishErrorCause::Faulted);
        assert_eq!(
            sticky.progress.pending_output_frames,
            partial.pending_output_frames
        );
    }

    #[test]
    fn complete_playback_accounting_includes_prior_streaming_and_terminal_trim() {
        let duration = FrameDuration::Ms5;
        let packet_frames = duration.samples_per_channel();
        let live_frame = frame(duration);
        let final_frame = frame(duration);
        for prior_transactions in [0_u64, 1, 2, 7] {
            let pipeline = pipeline(48_000, duration);
            let (mut worker, mut renderer, _) =
                playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("pair");
            let mut streaming_output_frames = 0;
            for packet in 0..prior_transactions {
                let live = worker
                    .process_frame(
                        &live_frame,
                        ExtendedTimestamp::new(packet * packet_frames as u64),
                        packet * packet_frames as u64,
                    )
                    .expect("prior live frame");
                assert_eq!(live.publication, PlaybackPublication::Published);
                streaming_output_frames += live.output_frames;
            }
            let finish = worker
                .finish_finite(final_input(
                    &final_frame,
                    packet_frames - 1,
                    prior_transactions * packet_frames as u64,
                    prior_transactions * packet_frames as u64,
                ))
                .expect("terminal frame");
            assert_eq!(finish.status, PlaybackFinishStatus::Finished);
            assert_eq!(worker.state(), PlaybackWorkerState::Finished);
            assert_eq!(finish.pending_output_frames, 0);
            assert_eq!(
                finish.valid_output_frames,
                finish.generated_output_frames - finish.trailing_trim_frames
            );

            let collected_frames = renderer.available_samples() / pipeline.channels();
            let mut collected = vec![f32::NAN; collected_frames * pipeline.channels()];
            let render = renderer.render(&mut collected);
            assert_eq!(render.rendered_samples, collected.len());
            assert!(collected.iter().all(|sample| sample.is_finite()));
            assert_eq!(
                collected_frames - finish.leading_trim_frames,
                streaming_output_frames + finish.generated_output_frames
                    - finish.leading_trim_frames
                    - finish.trailing_trim_frames,
                "S + G - L - T accounting for {prior_transactions} live transactions"
            );
        }
    }

    #[test]
    fn final_clock_fault_is_sticky_and_finished_producer_drains_to_disconnect() {
        let duration = FrameDuration::Ms5;
        let pipeline = pipeline(48_000, duration);
        let frame = frame(duration);
        let input = samples(duration);
        let (mut faulted, mut fault_renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("fault pair");
        let live = faulted
            .process_samples(&input, ExtendedTimestamp::new(1_000), 0)
            .expect("live anchor");
        let mut drain = vec![0.0; live.output_frames * 2];
        let _ = fault_renderer.render(&mut drain);
        assert_eq!(
            faulted
                .finish_finite(final_input(&frame, 240, 1_000, 240))
                .expect_err("remote stall")
                .cause,
            PlaybackFinishErrorCause::ClockDiscontinuity(DiscontinuityReason::RemoteStall)
        );
        assert_eq!(faulted.state(), PlaybackWorkerState::Faulted);
        assert_eq!(
            faulted
                .finish_finite(FinitePlaybackEnd::Continue)
                .expect_err("clock fault sticks")
                .cause,
            PlaybackFinishErrorCause::Faulted
        );

        let (mut worker, mut renderer, _) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("finish pair");
        let identity = (
            worker.output_workspace_identity(),
            worker.finish_workspace_identity(),
        );
        let (_finish, rendered) = collect_finish(&mut worker, &mut renderer, &frame, 240);
        assert!(!rendered.is_empty());
        assert_eq!(
            (
                worker.output_workspace_identity(),
                worker.finish_workspace_identity(),
            ),
            identity
        );
        assert_eq!(
            worker.process_samples(&input, ExtendedTimestamp::new(240), 240),
            Err(PlaybackProcessError::EndOfStream)
        );
        drop(worker);
        let mut terminal = [1.0; 32];
        let terminal_report = renderer.render(&mut terminal);
        assert_eq!(terminal_report.state, RenderState::Disconnected);
        assert!(terminal.iter().all(|sample| *sample == 0.0));
    }
}

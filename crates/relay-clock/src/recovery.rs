use crate::ClockError;

const PPM_SCALE: f64 = 1_000_000.0;

/// Configuration for [`ClockRecovery`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockRecoveryConfig {
    /// Symmetric bound on the output/input ASRC ratio correction.
    pub max_abs_correction_ppm: f64,
    /// Maximum change of correction per second.
    pub max_slew_ppm_per_second: f64,
    /// Proportional ring-fill correction in ppm per frame of error.
    pub proportional_gain_ppm_per_frame: f64,
    /// Integral ring-fill correction in ppm per frame-second.
    pub integral_gain_ppm_per_frame_second: f64,
    /// Symmetric anti-windup bound for the integral term.
    pub max_abs_integral_ppm: f64,
    /// Ring-fill input is saturated to this magnitude before filtering.
    pub max_abs_ring_fill_error_frames: f64,
    /// Time constant of the time-based first-order ring-fill low-pass.
    pub ring_fill_filter_time_constant_seconds: f64,
    /// Symmetric deadband applied after ring-fill low-pass filtering.
    pub ring_fill_deadband_frames: f64,
    /// Largest trusted interval between controller updates.
    ///
    /// A longer interval is rejected without state mutation: the newest fill
    /// sample is not assumed to describe the missing history.
    pub max_update_interval_seconds: f64,
}

impl Default for ClockRecoveryConfig {
    fn default() -> Self {
        Self {
            max_abs_correction_ppm: 500.0,
            max_slew_ppm_per_second: 25.0,
            proportional_gain_ppm_per_frame: 0.05,
            integral_gain_ppm_per_frame_second: 0.002,
            max_abs_integral_ppm: 250.0,
            max_abs_ring_fill_error_frames: 4_800.0,
            ring_fill_filter_time_constant_seconds: 1.0,
            ring_fill_deadband_frames: 24.0,
            max_update_interval_seconds: 0.25,
        }
    }
}

impl ClockRecoveryConfig {
    fn validate(self) -> Result<Self, ClockError> {
        finite(self.max_abs_correction_ppm, "max_abs_correction_ppm")?;
        finite(self.max_slew_ppm_per_second, "max_slew_ppm_per_second")?;
        finite(
            self.proportional_gain_ppm_per_frame,
            "proportional_gain_ppm_per_frame",
        )?;
        finite(
            self.integral_gain_ppm_per_frame_second,
            "integral_gain_ppm_per_frame_second",
        )?;
        finite(self.max_abs_integral_ppm, "max_abs_integral_ppm")?;
        finite(
            self.max_abs_ring_fill_error_frames,
            "max_abs_ring_fill_error_frames",
        )?;
        finite(
            self.ring_fill_filter_time_constant_seconds,
            "ring_fill_filter_time_constant_seconds",
        )?;
        finite(self.ring_fill_deadband_frames, "ring_fill_deadband_frames")?;
        finite(
            self.max_update_interval_seconds,
            "max_update_interval_seconds",
        )?;

        if !(0.0 < self.max_abs_correction_ppm && self.max_abs_correction_ppm < PPM_SCALE) {
            return Err(ClockError::OutOfRange("max_abs_correction_ppm"));
        }
        if self.max_slew_ppm_per_second <= 0.0 {
            return Err(ClockError::OutOfRange("max_slew_ppm_per_second"));
        }
        if self.proportional_gain_ppm_per_frame < 0.0 {
            return Err(ClockError::OutOfRange("proportional_gain_ppm_per_frame"));
        }
        if self.integral_gain_ppm_per_frame_second < 0.0 {
            return Err(ClockError::OutOfRange("integral_gain_ppm_per_frame_second"));
        }
        if self.max_abs_integral_ppm < 0.0
            || self.max_abs_integral_ppm > self.max_abs_correction_ppm
        {
            return Err(ClockError::OutOfRange("max_abs_integral_ppm"));
        }
        if self.max_abs_ring_fill_error_frames <= 0.0 {
            return Err(ClockError::OutOfRange("max_abs_ring_fill_error_frames"));
        }
        if self.ring_fill_filter_time_constant_seconds <= 0.0 {
            return Err(ClockError::OutOfRange(
                "ring_fill_filter_time_constant_seconds",
            ));
        }
        if self.ring_fill_deadband_frames < 0.0
            || self.ring_fill_deadband_frames >= self.max_abs_ring_fill_error_frames
        {
            return Err(ClockError::OutOfRange("ring_fill_deadband_frames"));
        }
        if self.max_update_interval_seconds <= 0.0 {
            return Err(ClockError::OutOfRange("max_update_interval_seconds"));
        }
        Ok(self)
    }
}

/// One bounded clock-recovery command and complete limiting telemetry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockRecoveryOutput {
    /// Applied output/input ratio correction, in ratio-space ppm.
    pub correction_ppm: f64,
    /// Exact `1 + correction_ppm / 1_000_000`, ready to multiply by a
    /// nominal output-frames/input-frames resampling ratio.
    pub ratio_multiplier: f64,
    /// Low-pass filtered ring-fill error after the configured deadband.
    pub controlled_ring_fill_error_frames: f64,
    /// Whether the remote drift input was clamped.
    pub drift_input_clamped: bool,
    /// Whether the raw ring-fill input was clamped before filtering.
    pub ring_fill_input_clamped: bool,
    /// Whether the integral accumulator hit its configured bound.
    pub integral_limited: bool,
    /// Whether conditional anti-windup suppressed integration because either
    /// amplitude or slew saturation prevented the requested actuation.
    pub anti_windup_active: bool,
    /// Whether output-amplitude limiting affected the update.
    pub amplitude_limited: bool,
    /// Whether output slew limiting affected the update.
    pub slew_limited: bool,
    /// Aggregate of every input, integral, amplitude, and slew limit above.
    pub saturated: bool,
}

/// Slew-limited exact feed-forward plus filtered PI clock recovery.
///
/// Remote drift supplies an exact reciprocal output/input ratio feed-forward:
/// at zero fill trim, a remote rate factor `1 + d` produces multiplier
/// `1 / (1 + d)`. Ring fill is sampled at one stable scheduling phase on every
/// update (for example, immediately after producing the worker's resampler
/// block), then low-pass filtered by elapsed time and passed through a deadband
/// before PI control. Do not sample sometimes before and sometimes after a
/// packet/block transfer.
///
/// Call at a bounded cadence no slower than
/// [`ClockRecoveryConfig::max_update_interval_seconds`]. A longer gap is
/// rejected without mutation rather than integrating a stale sample over the
/// gap. Packet arrival interval is never an input.
///
/// After construction, [`update`](Self::update) is O(1), allocation-free, and
/// deterministic. It is worker-side control, not hard realtime callback work.
#[derive(Clone, Debug)]
pub struct ClockRecovery {
    config: ClockRecoveryConfig,
    integral_ppm: f64,
    correction_ppm: f64,
    filtered_ring_fill_error_frames: f64,
}

#[derive(Clone, Copy)]
struct Actuation {
    requested: f64,
    applied: f64,
    amplitude_limited: bool,
    slew_limited: bool,
}

impl ClockRecovery {
    /// Constructs a recovery controller after validating numeric bounds.
    pub fn new(config: ClockRecoveryConfig) -> Result<Self, ClockError> {
        Ok(Self {
            config: config.validate()?,
            integral_ppm: 0.0,
            correction_ppm: 0.0,
            filtered_ring_fill_error_frames: 0.0,
        })
    }

    /// Returns the currently published ratio-space correction in ppm.
    #[must_use]
    pub const fn correction_ppm(&self) -> f64 {
        self.correction_ppm
    }

    /// Clears the filter, PI accumulator, and command to nominal ratio.
    pub fn reset(&mut self) {
        self.integral_ppm = 0.0;
        self.correction_ppm = 0.0;
        self.filtered_ring_fill_error_frames = 0.0;
    }

    /// Advances the controller using a stable-phase ring-fill sample.
    ///
    /// `estimated_remote_drift_ppm` is positive when the remote media clock is
    /// faster than the local device. `ring_fill_error_frames` is
    /// `current - target`; positive means overfull. `elapsed_seconds` must be
    /// positive and no larger than the configured maximum. Every input is
    /// validated before any state mutation.
    pub fn update(
        &mut self,
        estimated_remote_drift_ppm: f64,
        ring_fill_error_frames: f64,
        elapsed_seconds: f64,
    ) -> Result<ClockRecoveryOutput, ClockError> {
        finite(estimated_remote_drift_ppm, "estimated_remote_drift_ppm")?;
        finite(ring_fill_error_frames, "ring_fill_error_frames")?;
        finite(elapsed_seconds, "elapsed_seconds")?;
        if elapsed_seconds <= 0.0 {
            return Err(ClockError::NonPositiveLocalInterval);
        }
        if elapsed_seconds > self.config.max_update_interval_seconds {
            return Err(ClockError::UpdateIntervalTooLong);
        }

        let drift_ppm = estimated_remote_drift_ppm.clamp(
            -self.config.max_abs_correction_ppm,
            self.config.max_abs_correction_ppm,
        );
        let fill_input = ring_fill_error_frames.clamp(
            -self.config.max_abs_ring_fill_error_frames,
            self.config.max_abs_ring_fill_error_frames,
        );

        // Backward-Euler discretization makes the filter response depend on
        // elapsed time and remain stable for every accepted variable interval.
        let alpha = elapsed_seconds
            / (self.config.ring_fill_filter_time_constant_seconds + elapsed_seconds);
        let filtered_fill = self.filtered_ring_fill_error_frames
            + alpha * (fill_input - self.filtered_ring_fill_error_frames);
        let controlled_fill = apply_deadband(filtered_fill, self.config.ring_fill_deadband_frames);

        // Exact reciprocal feed-forward in output/input ratio space.
        let remote_rate_factor = 1.0 + drift_ppm / PPM_SCALE;
        let feed_forward_ppm = (remote_rate_factor.recip() - 1.0) * PPM_SCALE;
        let proportional = self.config.proportional_gain_ppm_per_frame * controlled_fill;
        let integral_delta =
            self.config.integral_gain_ppm_per_frame_second * controlled_fill * elapsed_seconds;
        let unclamped_integral = self.integral_ppm + integral_delta;
        let candidate_integral = unclamped_integral.clamp(
            -self.config.max_abs_integral_ppm,
            self.config.max_abs_integral_ppm,
        );
        let integral_limited = candidate_integral != unclamped_integral;

        let candidate = self.actuation(
            feed_forward_ppm - proportional - candidate_integral,
            elapsed_seconds,
        );
        let integration_command_delta = -(candidate_integral - self.integral_ppm);
        let actuator_residual = candidate.requested - candidate.applied;
        let anti_windup_active = (candidate.amplitude_limited || candidate.slew_limited)
            && integration_command_delta != 0.0
            && actuator_residual * integration_command_delta > 0.0;

        let final_actuation = if anti_windup_active {
            self.actuation(
                feed_forward_ppm - proportional - self.integral_ppm,
                elapsed_seconds,
            )
        } else {
            self.integral_ppm = candidate_integral;
            candidate
        };

        self.filtered_ring_fill_error_frames = filtered_fill;
        self.correction_ppm = final_actuation.applied;

        // Report attempted limiting too: anti-windup changing the final request
        // must not hide the saturation that caused integration suppression.
        let amplitude_limited = candidate.amplitude_limited || final_actuation.amplitude_limited;
        let slew_limited = candidate.slew_limited || final_actuation.slew_limited;
        let drift_input_clamped = estimated_remote_drift_ppm != drift_ppm;
        let ring_fill_input_clamped = ring_fill_error_frames != fill_input;
        let saturated = drift_input_clamped
            || ring_fill_input_clamped
            || integral_limited
            || anti_windup_active
            || amplitude_limited
            || slew_limited;

        Ok(ClockRecoveryOutput {
            correction_ppm: self.correction_ppm,
            ratio_multiplier: 1.0 + self.correction_ppm / PPM_SCALE,
            controlled_ring_fill_error_frames: controlled_fill,
            drift_input_clamped,
            ring_fill_input_clamped,
            integral_limited,
            anti_windup_active,
            amplitude_limited,
            slew_limited,
            saturated,
        })
    }

    fn actuation(&self, requested: f64, elapsed_seconds: f64) -> Actuation {
        let target = requested.clamp(
            -self.config.max_abs_correction_ppm,
            self.config.max_abs_correction_ppm,
        );
        let slew_limit = self.config.max_slew_ppm_per_second * elapsed_seconds;
        let desired_delta = target - self.correction_ppm;
        let applied_delta = desired_delta.clamp(-slew_limit, slew_limit);
        Actuation {
            requested,
            applied: (self.correction_ppm + applied_delta).clamp(
                -self.config.max_abs_correction_ppm,
                self.config.max_abs_correction_ppm,
            ),
            amplitude_limited: requested != target,
            slew_limited: desired_delta != applied_delta,
        }
    }
}

fn apply_deadband(value: f64, deadband: f64) -> f64 {
    if value > deadband {
        value - deadband
    } else if value < -deadband {
        value + deadband
    } else {
        0.0
    }
}

fn finite(value: f64, name: &'static str) -> Result<(), ClockError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ClockError::NonFinite(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DRIFTS_PPM: [f64; 7] = [-250.0, -100.0, -20.0, 0.0, 20.0, 100.0, 250.0];

    #[test]
    fn feed_forward_converges_to_exact_reciprocal_ratio() {
        for remote_drift_ppm in TEST_DRIFTS_PPM {
            let mut recovery = ClockRecovery::new(ClockRecoveryConfig::default())
                .expect("default configuration is valid");

            let mut output = recovery
                .update(remote_drift_ppm, 0.0, 0.1)
                .expect("valid update");
            for _ in 0..200 {
                output = recovery
                    .update(remote_drift_ppm, 0.0, 0.1)
                    .expect("valid update");
            }

            let expected = 1.0 / (1.0 + remote_drift_ppm / PPM_SCALE);
            assert_eq!(output.ratio_multiplier, expected);
            assert_eq!(output.correction_ppm, (expected - 1.0) * PPM_SCALE);
        }
    }

    #[test]
    fn exact_rate_ratio_plant_converges_with_variable_dt() {
        let config = ClockRecoveryConfig {
            max_slew_ppm_per_second: 50.0,
            ring_fill_deadband_frames: 0.0,
            ..ClockRecoveryConfig::default()
        };
        let mut recovery = ClockRecovery::new(config).expect("configuration is valid");
        let remote_drift_ppm = 250.0;
        let estimated_drift_ppm = 200.0;
        let remote_rate_factor = 1.0 + remote_drift_ppm / PPM_SCALE;
        let dts = [0.037, 0.113, 0.071, 0.149, 0.053, 0.097];
        let mut fill_error_frames = 0.0;
        let mut max_abs_fill = 0.0_f64;
        let mut output = recovery
            .update(estimated_drift_ppm, fill_error_frames, dts[0])
            .expect("valid initial update");

        for step in 1..240_000 {
            let dt = dts[step % dts.len()];
            output = recovery
                .update(estimated_drift_ppm, fill_error_frames, dt)
                .expect("valid variable-cadence update");
            // Exact output/input plant: input consumed per local output is 1/r.
            fill_error_frames +=
                (remote_rate_factor - output.ratio_multiplier.recip()) * 48_000.0 * dt;
            max_abs_fill = max_abs_fill.max(fill_error_frames.abs());
            assert!(output.correction_ppm.abs() <= config.max_abs_correction_ppm);
            assert!(output.ratio_multiplier.is_finite());
        }

        let exact_required_ratio = remote_rate_factor.recip();
        assert!(
            max_abs_fill < 1_000.0,
            "maximum fill error was {max_abs_fill}"
        );
        assert!(
            fill_error_frames.abs() < 0.05,
            "final fill error was {fill_error_frames}"
        );
        assert!(
            (output.ratio_multiplier - exact_required_ratio).abs() < 2.0e-9,
            "ratio {} did not converge to {exact_required_ratio}",
            output.ratio_multiplier
        );
    }

    #[test]
    fn time_filter_and_deadband_reject_quantized_fill_jitter_at_variable_cadence() {
        let mut recovery = ClockRecovery::new(ClockRecoveryConfig::default())
            .expect("default configuration is valid");
        let dts = [0.021, 0.083, 0.047, 0.119, 0.031, 0.067];
        let mut max_abs_correction = 0.0_f64;
        let mut sum_squares = 0.0;

        for step in 0..20_000 {
            // Packet/block-quantized sawtooth with zero mean and 96-frame span.
            let fill = [-48.0, -24.0, 0.0, 24.0, 48.0][step % 5];
            let output = recovery
                .update(0.0, fill, dts[step % dts.len()])
                .expect("variable cadence is within bound");
            max_abs_correction = max_abs_correction.max(output.correction_ppm.abs());
            sum_squares += output.correction_ppm * output.correction_ppm;
        }

        let rms = (sum_squares / 20_000.0).sqrt();
        assert!(
            max_abs_correction < 0.05,
            "peak correction was {max_abs_correction}"
        );
        assert!(rms < 0.02, "RMS correction was {rms}");
    }

    #[test]
    fn anti_windup_covers_slew_and_both_amplitude_reversals() {
        let config = ClockRecoveryConfig {
            max_abs_correction_ppm: 40.0,
            max_slew_ppm_per_second: 20.0,
            proportional_gain_ppm_per_frame: 0.2,
            integral_gain_ppm_per_frame_second: 0.5,
            max_abs_integral_ppm: 30.0,
            ring_fill_filter_time_constant_seconds: 0.01,
            ring_fill_deadband_frames: 0.0,
            ..ClockRecoveryConfig::default()
        };
        let mut recovery = ClockRecovery::new(config).expect("configuration is valid");

        let negative = recovery.update(0.0, 4_000.0, 0.1).expect("valid update");
        assert!(negative.amplitude_limited);
        assert!(negative.slew_limited);
        assert!(negative.anti_windup_active);
        assert_eq!(recovery.integral_ppm, 0.0, "negative slew must not wind up");

        for _ in 0..30 {
            recovery.update(0.0, 4_000.0, 0.1).expect("drive lower");
        }
        assert!(recovery.correction_ppm() < 0.0);

        let before_up = recovery.correction_ppm();
        let mut saw_upward_reversal = false;
        for _ in 0..60 {
            let reverse_up = recovery.update(0.0, -4_000.0, 0.1).expect("reverse upward");
            saw_upward_reversal |= reverse_up.correction_ppm > before_up;
        }
        assert!(saw_upward_reversal, "negative saturation must reverse");
        assert!(recovery.correction_ppm() > 0.0);
        assert!(
            recovery.integral_ppm <= 0.0,
            "unwinding direction is allowed"
        );

        let before_down = recovery.correction_ppm();
        let mut saw_downward_reversal = false;
        for _ in 0..60 {
            let reverse_down = recovery
                .update(0.0, 4_000.0, 0.1)
                .expect("reverse downward");
            saw_downward_reversal |= reverse_down.correction_ppm < before_down;
        }
        assert!(saw_downward_reversal, "positive saturation must reverse");
        assert!(recovery.correction_ppm() < 0.0);
        assert!(
            recovery.integral_ppm >= 0.0,
            "opposite unwinding direction is allowed"
        );
    }

    #[test]
    fn split_saturation_telemetry_reports_every_limiter() {
        let config = ClockRecoveryConfig {
            max_abs_correction_ppm: 50.0,
            max_slew_ppm_per_second: 10.0,
            proportional_gain_ppm_per_frame: 1.0,
            integral_gain_ppm_per_frame_second: 10.0,
            max_abs_integral_ppm: 0.1,
            max_abs_ring_fill_error_frames: 100.0,
            ring_fill_filter_time_constant_seconds: 0.001,
            ring_fill_deadband_frames: 0.0,
            ..ClockRecoveryConfig::default()
        };
        let mut recovery = ClockRecovery::new(config).expect("configuration is valid");
        let output = recovery
            .update(1_000.0, 1_000.0, 0.1)
            .expect("finite extremes are bounded");

        assert!(output.drift_input_clamped);
        assert!(output.ring_fill_input_clamped);
        assert!(output.integral_limited);
        assert!(output.anti_windup_active);
        assert!(output.amplitude_limited);
        assert!(output.slew_limited);
        assert!(output.saturated);
    }

    #[test]
    fn long_gap_is_rejected_without_any_state_mutation() {
        let mut recovery = ClockRecovery::new(ClockRecoveryConfig::default())
            .expect("default configuration is valid");
        recovery.update(20.0, 100.0, 0.1).expect("valid update");
        let before = recovery.clone();

        assert_eq!(
            recovery.update(-400.0, -4_000.0, 0.251),
            Err(ClockError::UpdateIntervalTooLong)
        );
        assert_eq!(recovery.integral_ppm, before.integral_ppm);
        assert_eq!(recovery.correction_ppm, before.correction_ppm);
        assert_eq!(
            recovery.filtered_ring_fill_error_frames,
            before.filtered_ring_fill_error_frames
        );
    }

    #[test]
    fn reset_clears_filter_integrator_and_command_after_discontinuity() {
        let mut recovery = ClockRecovery::new(ClockRecoveryConfig::default())
            .expect("default configuration is valid");
        for _ in 0..100 {
            recovery.update(100.0, 50.0, 0.1).expect("valid update");
        }
        assert_ne!(recovery.correction_ppm(), 0.0);

        recovery.reset();

        assert_eq!(recovery.correction_ppm(), 0.0);
        assert_eq!(recovery.integral_ppm, 0.0);
        assert_eq!(recovery.filtered_ring_fill_error_frames, 0.0);
        let output = recovery.update(0.0, 0.0, 0.1).expect("valid update");
        assert_eq!(output.correction_ppm, 0.0);
        assert_eq!(output.ratio_multiplier, 1.0);
    }

    #[test]
    fn invalid_inputs_do_not_mutate_controller() {
        let mut recovery = ClockRecovery::new(ClockRecoveryConfig::default())
            .expect("default configuration is valid");
        recovery.update(20.0, 0.0, 0.1).expect("valid update");
        let before = recovery.clone();

        assert_eq!(
            recovery.update(f64::NAN, 0.0, 0.1),
            Err(ClockError::NonFinite("estimated_remote_drift_ppm"))
        );
        assert_eq!(
            recovery.update(0.0, f64::INFINITY, 0.1),
            Err(ClockError::NonFinite("ring_fill_error_frames"))
        );
        assert_eq!(
            recovery.update(0.0, 0.0, 0.0),
            Err(ClockError::NonPositiveLocalInterval)
        );
        assert_eq!(recovery.integral_ppm, before.integral_ppm);
        assert_eq!(recovery.correction_ppm, before.correction_ppm);
        assert_eq!(
            recovery.filtered_ring_fill_error_frames,
            before.filtered_ring_fill_error_frames
        );
    }

    #[test]
    fn non_finite_and_out_of_range_configuration_is_rejected() {
        let config = ClockRecoveryConfig {
            max_abs_correction_ppm: f64::NAN,
            ..ClockRecoveryConfig::default()
        };
        assert_eq!(
            ClockRecovery::new(config).expect_err("NaN must fail"),
            ClockError::NonFinite("max_abs_correction_ppm")
        );

        let config = ClockRecoveryConfig {
            max_update_interval_seconds: 0.0,
            ..ClockRecoveryConfig::default()
        };
        assert_eq!(
            ClockRecovery::new(config).expect_err("zero interval must fail"),
            ClockError::OutOfRange("max_update_interval_seconds")
        );
    }
}

use crate::ClockError;

const PPM_SCALE: f64 = 1_000_000.0;

/// Configuration for [`DriftEstimator`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriftEstimatorConfig {
    /// Nominal remote media sample rate in samples per second.
    pub nominal_sample_rate_hz: f64,
    /// Nominal local audio-device rate used by the monotonic frame timeline.
    pub local_device_sample_rate_hz: f64,
    /// Minimum time covered by one drift measurement.
    pub observation_window_seconds: f64,
    /// EWMA weight given to the newest window measurement, in `(0, 1]`.
    pub smoothing_factor: f64,
    /// Symmetric saturation limit applied to measured remote drift.
    pub max_abs_drift_ppm: f64,
}

impl Default for DriftEstimatorConfig {
    fn default() -> Self {
        Self {
            nominal_sample_rate_hz: 48_000.0,
            local_device_sample_rate_hz: 48_000.0,
            observation_window_seconds: 2.0,
            smoothing_factor: 0.2,
            max_abs_drift_ppm: 500.0,
        }
    }
}

impl DriftEstimatorConfig {
    fn validate(self) -> Result<Self, ClockError> {
        finite(self.nominal_sample_rate_hz, "nominal_sample_rate_hz")?;
        finite(
            self.local_device_sample_rate_hz,
            "local_device_sample_rate_hz",
        )?;
        finite(
            self.observation_window_seconds,
            "observation_window_seconds",
        )?;
        finite(self.smoothing_factor, "smoothing_factor")?;
        finite(self.max_abs_drift_ppm, "max_abs_drift_ppm")?;

        if self.nominal_sample_rate_hz <= 0.0 {
            return Err(ClockError::OutOfRange("nominal_sample_rate_hz"));
        }
        if self.local_device_sample_rate_hz <= 0.0 {
            return Err(ClockError::OutOfRange("local_device_sample_rate_hz"));
        }
        if self.observation_window_seconds <= 0.0 {
            return Err(ClockError::OutOfRange("observation_window_seconds"));
        }
        if !(0.0 < self.smoothing_factor && self.smoothing_factor <= 1.0) {
            return Err(ClockError::OutOfRange("smoothing_factor"));
        }
        if self.max_abs_drift_ppm <= 0.0 {
            return Err(ClockError::OutOfRange("max_abs_drift_ppm"));
        }
        Ok(self)
    }
}

/// A remote media-clock position scheduled on a local audio-device timeline.
///
/// This type deliberately has no public fields and no wall-clock timestamp
/// constructor. Create it only at the media/playout scheduling boundary, after
/// the jitter buffer has selected media for a known device frame. A raw packet
/// arrival timestamp, socket receive timestamp, or network transit time is not
/// a valid observation and must never be substituted for the device frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlayoutClockObservation {
    remote_media_sample_position: u64,
    local_device_frame_position: u64,
}

impl PlayoutClockObservation {
    /// Binds an extended remote media position to its scheduled device frame.
    ///
    /// `local_device_frame_position` is the monotonically increasing frame
    /// counter of the local audio/device timeline, not a packet-arrival clock.
    #[must_use]
    pub const fn from_scheduled_playout(
        remote_media_sample_position: u64,
        local_device_frame_position: u64,
    ) -> Self {
        Self {
            remote_media_sample_position,
            local_device_frame_position,
        }
    }
}

/// Why an observation caused the estimator to establish a new clock epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscontinuityReason {
    /// The extended remote media sample position moved backwards.
    RemoteRegression,
    /// The local device timeline advanced but remote media did not.
    RemoteStall,
}

/// Result of adding one playout-clock observation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DriftEstimatorUpdate {
    /// No complete observation window is available yet.
    WarmingUp,
    /// A new smoothed estimate is available.
    ///
    /// Positive parts per million means the remote sample clock advances
    /// faster than its configured nominal rate relative to the local device.
    EstimatePpm(f64),
    /// The previous epoch was discarded and this observation became the new
    /// anchor. The controller should be reset as part of the same recovery.
    Discontinuity(DiscontinuityReason),
}

/// Long-window estimator of remote media-clock drift against device playout.
///
/// Observations are accepted only through [`PlayoutClockObservation`], whose
/// constructor names the scheduling boundary and local device-frame timeline.
/// The transport adapter unwraps the remote timestamp; the playout scheduler
/// binds that position to a device frame after jitter-buffer scheduling. Raw
/// network arrival time is explicitly outside this estimator's contract.
///
/// Reset on an SSRC change, seek, restart, or known gap that invalidates the
/// clock epoch. Anchored windows smooth media/device clock quantization; they
/// are not a network-delay estimator.
#[derive(Clone, Debug)]
pub struct DriftEstimator {
    config: DriftEstimatorConfig,
    anchor: Option<PlayoutClockObservation>,
    estimate_ppm: Option<f64>,
}

impl DriftEstimator {
    /// Constructs an estimator after validating all numeric configuration.
    pub fn new(config: DriftEstimatorConfig) -> Result<Self, ClockError> {
        Ok(Self {
            config: config.validate()?,
            anchor: None,
            estimate_ppm: None,
        })
    }

    /// Returns the most recent smoothed drift estimate, if one exists.
    #[must_use]
    pub const fn estimate_ppm(&self) -> Option<f64> {
        self.estimate_ppm
    }

    /// Clears the observation epoch and filtered estimate.
    pub fn reset(&mut self) {
        self.anchor = None;
        self.estimate_ppm = None;
    }

    /// Adds scheduled media progression on the local device timeline.
    ///
    /// Local device frames must advance. A remote regression or stall starts a
    /// new epoch. Invalid local progression is rejected without state mutation.
    /// Packet arrival timestamps are not accepted by this API.
    pub fn observe_scheduled_playout(
        &mut self,
        observation: PlayoutClockObservation,
    ) -> Result<DriftEstimatorUpdate, ClockError> {
        let Some(anchor) = self.anchor else {
            self.anchor = Some(observation);
            return Ok(DriftEstimatorUpdate::WarmingUp);
        };

        let Some(local_frame_delta) = observation
            .local_device_frame_position
            .checked_sub(anchor.local_device_frame_position)
        else {
            return Err(ClockError::NonPositiveLocalInterval);
        };
        if local_frame_delta == 0 {
            return Err(ClockError::NonPositiveLocalInterval);
        }

        if observation.remote_media_sample_position < anchor.remote_media_sample_position {
            self.start_new_epoch(observation);
            return Ok(DriftEstimatorUpdate::Discontinuity(
                DiscontinuityReason::RemoteRegression,
            ));
        }
        if observation.remote_media_sample_position == anchor.remote_media_sample_position {
            self.start_new_epoch(observation);
            return Ok(DriftEstimatorUpdate::Discontinuity(
                DiscontinuityReason::RemoteStall,
            ));
        }

        let local_delta_seconds =
            local_frame_delta as f64 / self.config.local_device_sample_rate_hz;
        if local_delta_seconds < self.config.observation_window_seconds {
            return Ok(DriftEstimatorUpdate::WarmingUp);
        }

        let remote_delta =
            (observation.remote_media_sample_position - anchor.remote_media_sample_position) as f64;
        let measured_rate = remote_delta / local_delta_seconds;
        let raw_ppm = (measured_rate / self.config.nominal_sample_rate_hz - 1.0) * PPM_SCALE;
        if !raw_ppm.is_finite() {
            return Err(ClockError::NonFinite("measured_drift_ppm"));
        }
        let saturated_ppm = raw_ppm.clamp(
            -self.config.max_abs_drift_ppm,
            self.config.max_abs_drift_ppm,
        );
        let smoothed_ppm = match self.estimate_ppm {
            Some(previous) => previous + self.config.smoothing_factor * (saturated_ppm - previous),
            None => saturated_ppm,
        };

        self.anchor = Some(observation);
        self.estimate_ppm = Some(smoothed_ppm);
        Ok(DriftEstimatorUpdate::EstimatePpm(smoothed_ppm))
    }

    fn start_new_epoch(&mut self, observation: PlayoutClockObservation) {
        self.anchor = Some(observation);
        self.estimate_ppm = None;
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

    fn playout(remote: u64, device: u64) -> PlayoutClockObservation {
        PlayoutClockObservation::from_scheduled_playout(remote, device)
    }

    #[test]
    fn estimator_converges_for_required_drift_range() {
        for expected_ppm in TEST_DRIFTS_PPM {
            let mut estimator = DriftEstimator::new(DriftEstimatorConfig::default())
                .expect("default configuration is valid");
            let remote_rate = 48_000.0 * (1.0 + expected_ppm / PPM_SCALE);
            estimator
                .observe_scheduled_playout(playout(0, 0))
                .expect("initial observation");

            for second in 1..=300_u64 {
                let position = (remote_rate * second as f64).round() as u64;
                estimator
                    .observe_scheduled_playout(playout(position, second * 48_000))
                    .expect("valid progression");
            }

            let actual = estimator.estimate_ppm().expect("estimate is available");
            assert!(
                (actual - expected_ppm).abs() < 0.3,
                "expected {expected_ppm} ppm, got {actual} ppm"
            );
        }
    }

    #[test]
    fn multi_window_network_jitter_and_delay_steps_are_not_observations() {
        for expected_ppm in [-250.0, 0.0, 250.0] {
            let mut estimator = DriftEstimator::new(DriftEstimatorConfig::default())
                .expect("default configuration is valid");
            let remote_rate = 48_000.0 * (1.0 + expected_ppm / PPM_SCALE);
            let mut estimates = 0;
            let mut arrival_variation_was_exercised = false;

            for packet in 0..=12_000_u64 {
                let device_frame = packet * 480;
                let media_position = (remote_rate * device_frame as f64 / 48_000.0).round() as u64;

                // Model raw socket arrival separately. It includes alternating
                // jitter, two delay steps, and a slow asymmetric ramp. It is
                // intentionally impossible to pass through `playout` because
                // that constructor accepts a device frame, not seconds.
                let nominal_seconds = device_frame as f64 / 48_000.0;
                let jitter = match packet % 5 {
                    0 => -0.004,
                    1 => 0.006,
                    2 => -0.002,
                    3 => 0.009,
                    _ => 0.0,
                };
                let delay_step = if (2_000..7_000).contains(&packet) {
                    0.010
                } else if packet >= 7_000 {
                    0.001
                } else {
                    0.0
                };
                let delay_ramp = if packet >= 9_000 {
                    (packet - 9_000) as f64 * 0.000_001
                } else {
                    0.0
                };
                let raw_network_arrival_seconds =
                    nominal_seconds + jitter + delay_step + delay_ramp;
                arrival_variation_was_exercised |=
                    (raw_network_arrival_seconds - nominal_seconds).abs() >= 0.010;

                let update = estimator
                    .observe_scheduled_playout(playout(media_position, device_frame))
                    .expect("scheduled progression remains valid");
                estimates += usize::from(matches!(update, DriftEstimatorUpdate::EstimatePpm(_)));
            }

            assert!(arrival_variation_was_exercised);
            assert!(estimates >= 50, "test must cross many complete windows");
            let actual = estimator.estimate_ppm().expect("estimate is available");
            assert!(
                (actual - expected_ppm).abs() < 0.3,
                "arrival variation leaked into {expected_ppm} ppm estimate: {actual}"
            );
        }
    }

    #[test]
    fn estimator_saturates_implausible_media_progression() {
        let mut estimator = DriftEstimator::new(DriftEstimatorConfig::default())
            .expect("default configuration is valid");
        estimator
            .observe_scheduled_playout(playout(0, 0))
            .expect("initial observation");
        let update = estimator
            .observe_scheduled_playout(playout(100_000, 96_000))
            .expect("finite measurement is accepted");

        assert_eq!(update, DriftEstimatorUpdate::EstimatePpm(500.0));
    }

    #[test]
    fn regression_resets_epoch_and_filtered_estimate() {
        let mut estimator = DriftEstimator::new(DriftEstimatorConfig::default())
            .expect("default configuration is valid");
        estimator
            .observe_scheduled_playout(playout(1_000, 0))
            .expect("initial observation");
        estimator
            .observe_scheduled_playout(playout(97_000, 96_000))
            .expect("first estimate");

        let update = estimator
            .observe_scheduled_playout(playout(10, 144_000))
            .expect("regression is classified");

        assert_eq!(
            update,
            DriftEstimatorUpdate::Discontinuity(DiscontinuityReason::RemoteRegression)
        );
        assert_eq!(estimator.estimate_ppm(), None);
        assert_eq!(
            estimator
                .observe_scheduled_playout(playout(48_010, 192_000))
                .expect("new epoch warms up"),
            DriftEstimatorUpdate::WarmingUp
        );
    }

    #[test]
    fn explicit_reset_discards_previous_epoch() {
        let mut estimator = DriftEstimator::new(DriftEstimatorConfig::default())
            .expect("default configuration is valid");
        estimator
            .observe_scheduled_playout(playout(0, 0))
            .expect("initial observation");
        estimator
            .observe_scheduled_playout(playout(96_000, 96_000))
            .expect("first estimate");

        estimator.reset();

        assert_eq!(estimator.estimate_ppm(), None);
        assert_eq!(
            estimator
                .observe_scheduled_playout(playout(500, 2_400_000))
                .expect("new anchor"),
            DriftEstimatorUpdate::WarmingUp
        );
    }

    #[test]
    fn non_advancing_device_frames_are_rejected_without_mutation() {
        let mut estimator = DriftEstimator::new(DriftEstimatorConfig::default())
            .expect("default configuration is valid");
        estimator
            .observe_scheduled_playout(playout(0, 48_000))
            .expect("initial observation");

        assert_eq!(
            estimator.observe_scheduled_playout(playout(480, 48_000)),
            Err(ClockError::NonPositiveLocalInterval)
        );
        assert_eq!(
            estimator.observe_scheduled_playout(playout(480, 47_999)),
            Err(ClockError::NonPositiveLocalInterval)
        );
        assert_eq!(
            estimator
                .observe_scheduled_playout(playout(96_000, 144_000))
                .expect("anchor was preserved"),
            DriftEstimatorUpdate::EstimatePpm(0.0)
        );
    }
}

//! Deterministic worker-side clock drift estimation and bounded recovery.
//!
//! This crate deliberately separates two time scales:
//! [`DriftEstimator`] measures long-term remote sample-clock progression, while
//! [`ClockRecovery`] applies a slew-limited PI correction using that estimate
//! and slow output-ring fill error. Estimator observations bind scheduled remote
//! media progression to a monotonic local audio-device frame timeline. Raw
//! packet-arrival timestamps are explicitly not an estimator input; network
//! timing belongs in the jitter buffer.
//!
//! All processing is constant-time and allocation-free after construction. The
//! types contain no async runtime, platform clock, synchronization, or I/O.

#![forbid(unsafe_code)]

mod estimator;
mod recovery;

pub use estimator::{
    DiscontinuityReason, DriftEstimator, DriftEstimatorConfig, DriftEstimatorUpdate,
    PlayoutClockObservation,
};
pub use recovery::{ClockRecovery, ClockRecoveryConfig, ClockRecoveryOutput};

/// Invalid configuration or observation supplied to clock recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockError {
    /// A named value was not finite.
    NonFinite(&'static str),
    /// A named configuration value was outside its allowed range.
    OutOfRange(&'static str),
    /// A local device timeline or controller interval did not advance.
    NonPositiveLocalInterval,
    /// A controller update interval exceeded its configured trusted maximum.
    UpdateIntervalTooLong,
}

impl core::fmt::Display for ClockError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NonFinite(name) => write!(formatter, "{name} must be finite"),
            Self::OutOfRange(name) => write!(formatter, "{name} is outside its allowed range"),
            Self::NonPositiveLocalInterval => {
                formatter.write_str("local monotonic interval must advance")
            }
            Self::UpdateIntervalTooLong => {
                formatter.write_str("controller update interval exceeds the trusted maximum")
            }
        }
    }
}

impl std::error::Error for ClockError {}

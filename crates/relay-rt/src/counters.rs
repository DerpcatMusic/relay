use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(not(target_has_atomic = "64"))]
compile_error!("relay-rt requires native lock-free 64-bit atomics for callback counters");

/// Read-only access to the queue's atomic diagnostic counters.
///
/// Clone and drop metrics handles only off the audio callback. [`Self::snapshot`]
/// performs relaxed atomic loads and is intended for observational telemetry;
/// fields are not a transactionally coherent point-in-time view.
#[derive(Clone, Debug)]
pub struct AudioRingMetrics {
    pub(crate) counters: Arc<Counters>,
}

impl AudioRingMetrics {
    /// Loads a nonblocking observational snapshot of all counters.
    #[must_use]
    pub fn snapshot(&self) -> AudioRingSnapshot {
        AudioRingSnapshot {
            dropped_samples: self.counters.dropped_samples.load(Ordering::Relaxed),
            underruns: self.counters.underruns.load(Ordering::Relaxed),
            underrun_samples: self.counters.underrun_samples.load(Ordering::Relaxed),
        }
    }
}

/// Copyable diagnostic values sampled from an [`AudioRingMetrics`] handle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AudioRingSnapshot {
    /// Scalar input samples rejected by full or disconnected writes.
    pub dropped_samples: u64,
    /// Read operations that returned fewer samples than requested.
    pub underruns: u64,
    /// Scalar output samples absent across all underrun operations.
    pub underrun_samples: u64,
}

#[derive(Debug, Default)]
pub(crate) struct Counters {
    dropped_samples: AtomicU64,
    underruns: AtomicU64,
    underrun_samples: AtomicU64,
}

impl Counters {
    pub(crate) fn record_drop(&self, samples: usize) {
        self.dropped_samples
            .fetch_add(samples as u64, Ordering::Relaxed);
    }

    pub(crate) fn record_underrun(&self, missing_samples: usize) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
        self.underrun_samples
            .fetch_add(missing_samples as u64, Ordering::Relaxed);
    }
}

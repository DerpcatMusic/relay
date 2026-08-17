use std::error::Error;
use std::fmt;
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::counters::{AudioRingMetrics, Counters};

/// Creates a fixed-capacity interleaved-sample SPSC ring.
///
/// This is the only queue operation that allocates. `capacity_samples` counts
/// scalar `f32` values, not audio frames; choose a whole-frame multiple for the
/// stream's channel count.
///
/// # Errors
///
/// Returns [`RingConfigError::ZeroCapacity`] for a zero-sized ring.
pub fn audio_ring(
    capacity_samples: usize,
) -> Result<(AudioProducer, AudioConsumer, AudioRingMetrics), RingConfigError> {
    if capacity_samples == 0 {
        return Err(RingConfigError::ZeroCapacity);
    }

    let (producer, consumer) = RingBuffer::new(capacity_samples);
    let counters = Arc::new(Counters::default());
    let metrics = AudioRingMetrics {
        counters: Arc::clone(&counters),
    };

    Ok((
        AudioProducer {
            inner: producer,
            counters: Arc::clone(&counters),
        },
        AudioConsumer {
            inner: consumer,
            counters,
        },
        metrics,
    ))
}

/// Invalid construction parameters for an audio ring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RingConfigError {
    /// A bounded transport must contain at least one sample slot.
    ZeroCapacity,
}

impl fmt::Display for RingConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => formatter.write_str("audio ring capacity must be non-zero"),
        }
    }
}

impl Error for RingConfigError {}

/// The unique writer endpoint of an audio ring.
///
/// Move this value to exactly one producer thread. It is intentionally not
/// cloneable or shareable. Destruction must happen off the audio callback; see
/// the crate-level lifecycle contract.
pub struct AudioProducer {
    inner: Producer<f32>,
    counters: Arc<Counters>,
}

impl AudioProducer {
    /// Publishes all samples or drops the entire new slice immediately.
    ///
    /// The method performs one bounded copy on success. It does not allocate,
    /// lock, wait, log, or retry. An empty slice always succeeds without
    /// changing counters.
    #[must_use]
    pub fn write(&mut self, samples: &[f32]) -> WriteOutcome {
        if samples.is_empty() {
            return WriteOutcome::Written { samples: 0 };
        }

        if self.inner.is_abandoned() {
            self.counters.record_drop(samples.len());
            return WriteOutcome::Disconnected {
                samples: samples.len(),
            };
        }

        match self.inner.push_entire_slice(samples) {
            Ok(()) => WriteOutcome::Written {
                samples: samples.len(),
            },
            Err(_) => {
                self.counters.record_drop(samples.len());
                WriteOutcome::DroppedFull {
                    samples: samples.len(),
                }
            }
        }
    }

    /// Returns currently available scalar sample slots.
    ///
    /// This is an observation only; the consumer may increase the value at any
    /// time. [`Self::write`] remains the authoritative all-or-drop operation.
    #[must_use]
    pub fn available_samples(&self) -> usize {
        self.inner.slots()
    }

    /// Reports whether the consumer endpoint has been destroyed.
    ///
    /// This is a diagnostic observation and not lifecycle synchronization.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.inner.is_abandoned()
    }
}

/// The result of one all-or-drop write attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteOutcome {
    /// The complete input slice was published.
    Written {
        /// Number of scalar samples published.
        samples: usize,
    },
    /// The complete input slice was rejected because capacity was insufficient.
    DroppedFull {
        /// Number of scalar samples dropped.
        samples: usize,
    },
    /// The consumer was already observed as destroyed, so the slice was dropped.
    Disconnected {
        /// Number of scalar samples dropped.
        samples: usize,
    },
}

/// The unique reader endpoint of an audio ring.
///
/// Move this value to exactly one consumer thread. It is intentionally not
/// cloneable or shareable. Destruction must happen off the audio callback; see
/// the crate-level lifecycle contract.
pub struct AudioConsumer {
    inner: Consumer<f32>,
    counters: Arc<Counters>,
}

impl AudioConsumer {
    /// Copies up to `output.len()` samples without waiting.
    ///
    /// The unread remainder of `output` is left unchanged. A short read records
    /// one underrun plus the missing scalar-sample count. The method performs no
    /// allocation, lock, wait, logging, retry, zero-fill, I/O, or DSP.
    #[must_use]
    pub fn read(&mut self, output: &mut [f32]) -> ReadOutcome {
        let requested = output.len();
        let read_samples = {
            let (read, _) = self.inner.pop_partial_slice(output);
            read.len()
        };
        let disconnected = self.inner.is_abandoned();

        if read_samples < requested {
            self.counters.record_underrun(requested - read_samples);
        }

        let state = if disconnected {
            ReadState::Disconnected
        } else if read_samples < requested {
            ReadState::Underrun
        } else {
            ReadState::Complete
        };

        ReadOutcome {
            read_samples,
            state,
        }
    }

    /// Returns the currently readable scalar sample count.
    ///
    /// This is an observation only; the producer may increase the value at any
    /// time.
    #[must_use]
    pub fn available_samples(&self) -> usize {
        self.inner.slots()
    }

    /// Reports whether the producer endpoint has been destroyed.
    ///
    /// Buffered samples remain readable. This is a diagnostic observation and
    /// not lifecycle synchronization.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.inner.is_abandoned()
    }
}

/// Result of one bounded read into caller-owned memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadOutcome {
    /// Number of leading output slots initialized from the queue.
    pub read_samples: usize,
    /// Queue state observed after the copy.
    pub state: ReadState,
}

/// Queue state observed by a read operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadState {
    /// The requested output slice was filled and the producer remained present.
    Complete,
    /// The producer remained present but not enough samples were available.
    Underrun,
    /// The producer endpoint was observed as destroyed.
    ///
    /// `read_samples` can be nonzero because buffered samples are drained first.
    Disconnected,
}

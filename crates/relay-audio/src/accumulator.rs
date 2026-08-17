use core::fmt;

use relay_opus::CHANNELS;

/// Fixed-capacity interleaved stereo sample ring at the 48 kHz media rate.
///
/// Construction allocates one boxed slice. Push/pop indexing is constant time;
/// sample copying is linear in the transferred sample count. The ring never
/// reallocates or grows after construction.
#[derive(Debug)]
pub struct Interleaved48kAccumulator {
    storage: Box<[f32]>,
    read: usize,
    len: usize,
}

impl Interleaved48kAccumulator {
    /// Allocates a fixed, stereo-frame-aligned scalar-sample capacity.
    ///
    /// # Errors
    ///
    /// Zero, odd, overflowing, or unallocatable capacities are rejected.
    pub fn new(capacity_samples: usize) -> Result<Self, AccumulatorError> {
        if capacity_samples == 0 {
            return Err(AccumulatorError::ZeroCapacity);
        }
        if !capacity_samples.is_multiple_of(usize::from(CHANNELS)) {
            return Err(AccumulatorError::IncompleteInterleavedFrame {
                samples: capacity_samples,
            });
        }
        capacity_samples
            .checked_mul(core::mem::size_of::<f32>())
            .ok_or(AccumulatorError::CapacityOverflow)?;
        let mut storage = Vec::new();
        storage
            .try_reserve_exact(capacity_samples)
            .map_err(|_| AccumulatorError::AllocationFailed)?;
        storage.resize(capacity_samples, 0.0);
        Ok(Self {
            storage: storage.into_boxed_slice(),
            read: 0,
            len: 0,
        })
    }

    /// Returns the immutable scalar-sample capacity.
    #[must_use]
    pub fn capacity_samples(&self) -> usize {
        self.storage.len()
    }

    /// Returns retained scalar samples.
    #[must_use]
    pub const fn len_samples(&self) -> usize {
        self.len
    }

    /// Returns retained stereo frames.
    #[must_use]
    pub const fn len_frames(&self) -> usize {
        self.len / CHANNELS as usize
    }

    /// Reports whether the ring contains no samples.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns remaining scalar-sample capacity.
    #[must_use]
    pub fn remaining_samples(&self) -> usize {
        self.storage.len() - self.len
    }

    /// Appends complete finite stereo frames all-or-nothing.
    ///
    /// # Errors
    ///
    /// An odd-length input, a NaN/infinity, or insufficient fixed storage is
    /// rejected without changing ring state.
    pub fn push_interleaved(&mut self, samples: &[f32]) -> Result<(), AccumulatorError> {
        if !samples.len().is_multiple_of(usize::from(CHANNELS)) {
            return Err(AccumulatorError::IncompleteInterleavedFrame {
                samples: samples.len(),
            });
        }
        if let Some(sample_index) = samples.iter().position(|sample| !sample.is_finite()) {
            return Err(AccumulatorError::NonFiniteInput { sample_index });
        }
        if samples.len() > self.remaining_samples() {
            return Err(AccumulatorError::Full {
                available: self.remaining_samples(),
                requested: samples.len(),
            });
        }
        self.push_validated(samples);
        Ok(())
    }

    /// Removes exactly `output.len()` complete stereo samples.
    ///
    /// # Errors
    ///
    /// An odd output length or more requested samples than retained is rejected
    /// without changing ring state.
    pub fn pop_interleaved(&mut self, output: &mut [f32]) -> Result<(), AccumulatorError> {
        if !output.len().is_multiple_of(usize::from(CHANNELS)) {
            return Err(AccumulatorError::IncompleteInterleavedFrame {
                samples: output.len(),
            });
        }
        if output.len() > self.len {
            return Err(AccumulatorError::InsufficientSamples {
                available: self.len,
                requested: output.len(),
            });
        }
        self.pop_validated(output);
        Ok(())
    }

    /// Discards all retained samples while preserving allocation and capacity.
    pub fn clear(&mut self) {
        self.read = 0;
        self.len = 0;
    }

    pub(crate) fn push_prefix(&mut self, samples: &[f32]) -> usize {
        let count = samples.len().min(self.remaining_samples());
        // Worker-produced buffers are already finite and stereo aligned. Limit
        // to complete frames so the public accumulator invariant is retained.
        let aligned = count - count % usize::from(CHANNELS);
        self.push_validated(&samples[..aligned]);
        aligned
    }

    pub(crate) fn peek_exact(&self, output: &mut [f32]) {
        debug_assert!(output.len() <= self.len);
        debug_assert!(output.len().is_multiple_of(usize::from(CHANNELS)));
        if output.is_empty() {
            return;
        }
        let first = output.len().min(self.storage.len() - self.read);
        output[..first].copy_from_slice(&self.storage[self.read..self.read + first]);
        let remaining = output.len() - first;
        if remaining != 0 {
            output[first..].copy_from_slice(&self.storage[..remaining]);
        }
    }

    pub(crate) fn discard_exact(&mut self, samples: usize) {
        debug_assert!(samples <= self.len);
        debug_assert!(samples.is_multiple_of(usize::from(CHANNELS)));
        if samples == 0 {
            return;
        }
        self.read = (self.read + samples) % self.storage.len();
        self.len -= samples;
        if self.len == 0 {
            self.read = 0;
        }
    }

    #[cfg(test)]
    pub(crate) fn storage_identity(&self) -> (*const f32, usize) {
        (self.storage.as_ptr(), self.storage.len())
    }

    fn push_validated(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let write = (self.read + self.len) % self.storage.len();
        let first = samples.len().min(self.storage.len() - write);
        self.storage[write..write + first].copy_from_slice(&samples[..first]);
        let remaining = samples.len() - first;
        if remaining != 0 {
            self.storage[..remaining].copy_from_slice(&samples[first..]);
        }
        self.len += samples.len();
    }

    fn pop_validated(&mut self, output: &mut [f32]) {
        if output.is_empty() {
            return;
        }
        let first = output.len().min(self.storage.len() - self.read);
        output[..first].copy_from_slice(&self.storage[self.read..self.read + first]);
        let remaining = output.len() - first;
        if remaining != 0 {
            output[first..].copy_from_slice(&self.storage[..remaining]);
        }
        self.read = (self.read + output.len()) % self.storage.len();
        self.len -= output.len();
        if self.len == 0 {
            self.read = 0;
        }
    }
}

/// Why a fixed 48 kHz accumulator operation failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccumulatorError {
    /// A useful ring must retain at least one stereo frame.
    ZeroCapacity,
    /// A capacity or transfer ended midway through a stereo frame.
    IncompleteInterleavedFrame {
        /// Rejected scalar-sample count.
        samples: usize,
    },
    /// Fixed-capacity byte arithmetic exceeded `usize`.
    CapacityOverflow,
    /// Construction-time allocation failed.
    AllocationFailed,
    /// Input contained NaN or infinity.
    NonFiniteInput {
        /// Index of the first rejected scalar sample.
        sample_index: usize,
    },
    /// The all-or-nothing push did not fit.
    Full {
        /// Available scalar samples.
        available: usize,
        /// Requested scalar samples.
        requested: usize,
    },
    /// The exact pop requested more samples than retained.
    InsufficientSamples {
        /// Retained scalar samples.
        available: usize,
        /// Requested scalar samples.
        requested: usize,
    },
}

impl fmt::Display for AccumulatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for AccumulatorError {}

#[cfg(test)]
mod tests {
    use super::Interleaved48kAccumulator;

    #[test]
    fn wraparound_preserves_order_and_storage_identity() {
        let mut ring = Interleaved48kAccumulator::new(8).expect("ring");
        let identity = ring.storage_identity();
        ring.push_interleaved(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0])
            .expect("push");
        let mut first = [0.0; 4];
        ring.pop_interleaved(&mut first).expect("pop");
        assert_eq!(first, [0.0, 1.0, 2.0, 3.0]);
        ring.push_interleaved(&[6.0, 7.0, 8.0, 9.0])
            .expect("wrapped push");
        let mut rest = [0.0; 6];
        ring.pop_interleaved(&mut rest).expect("wrapped pop");
        assert_eq!(rest, [4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        assert_eq!(ring.storage_identity(), identity);
    }

    #[test]
    fn invalid_and_full_pushes_are_transactional() {
        let mut ring = Interleaved48kAccumulator::new(4).expect("ring");
        ring.push_interleaved(&[1.0, 2.0]).expect("push");
        assert!(ring.push_interleaved(&[3.0, f32::NAN]).is_err());
        assert!(ring.push_interleaved(&[3.0, 4.0, 5.0, 6.0]).is_err());
        let mut retained = [0.0; 2];
        ring.pop_interleaved(&mut retained).expect("pop");
        assert_eq!(retained, [1.0, 2.0]);
    }
}

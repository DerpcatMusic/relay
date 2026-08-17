//! Bounded worker-side RTP packet ordering and latency-target policy.
//!
//! This crate deliberately does not parse RTP, depacketize codecs, or perform I/O. The
//! transport/media worker supplies the 16-bit RTP sequence number and owns playout timing.
//! [`ReorderBuffer::pop_at_deadline`] is the explicit point where an absent sequence number
//! becomes a playout gap requiring concealment. It is not a network-loss decision.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use core::fmt;
use std::time::Duration;

const SERIAL_HALF_RANGE: u16 = 1 << 15;
const MAX_CAPACITY: usize = (SERIAL_HALF_RANGE - 1) as usize;

/// Invalid reorder-window configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReorderConfigError {
    /// A zero-sized window cannot retain any packet.
    ZeroCapacity,
    /// Serial-number comparisons are unambiguous only below half the `u16` sequence space.
    CapacityExceedsSerialHalfRange,
}

impl fmt::Display for ReorderConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => formatter.write_str("reorder capacity must be non-zero"),
            Self::CapacityExceedsSerialHalfRange => formatter.write_str(
                "reorder capacity must be smaller than half the RTP sequence-number space",
            ),
        }
    }
}

impl std::error::Error for ReorderConfigError {}

#[derive(Debug)]
struct Slot<T> {
    extended_sequence: u64,
    sequence: u16,
    packet: T,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecentObservation {
    Emitted,
    MissingAtDeadline,
    LateSeen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HistoryEntry {
    extended_sequence: u64,
    observation: RecentObservation,
}

/// How an accepted packet relates to the next playout sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcceptedPacket {
    /// This is the next packet wanted by playout.
    InOrder,
    /// The packet arrived ahead of a gap and was retained for later playout.
    Reordered {
        /// Number of sequence positions between the playout head and this packet.
        depth: u16,
    },
}

/// Why a packet was not stored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectedPacket {
    /// The same sequence number is already buffered, or was recently emitted.
    Duplicate,
    /// The sequence number is behind the playout head and was not recently emitted.
    Late {
        /// Distance behind the current playout head in sequence positions.
        distance: u16,
    },
    /// The packet is ahead of, but cannot fit in, the configured reorder window.
    AheadOfWindow {
        /// Distance ahead of the current playout head in sequence positions.
        distance: u16,
    },
    /// Exactly half the serial space has no well-defined before/after ordering.
    AmbiguousSerialDistance,
}

/// Result of trying to insert a packet.
#[derive(Debug)]
#[must_use]
pub enum PushResult<T> {
    /// The packet was accepted into bounded storage.
    Accepted(AcceptedPacket),
    /// The packet was rejected and ownership is returned to the caller.
    Rejected {
        /// Classification of the rejection.
        reason: RejectedPacket,
        /// Original packet, returned without allocation or cloning.
        packet: T,
    },
}

/// One playout decision made at a caller-selected deadline.
#[derive(Debug, Eq, PartialEq)]
#[must_use]
pub enum Playout<T> {
    /// No sequence has been observed, so there is no playout head yet.
    Empty,
    /// The expected packet was present.
    Packet {
        /// Original 16-bit RTP sequence number.
        sequence: u16,
        /// Stored packet value.
        packet: T,
    },
    /// The expected packet was absent at its playout deadline and requires concealment.
    ///
    /// This is a playout-timing fact, not proof of network loss. Consumers must not use it
    /// directly as RTCP network-loss truth; a source-statistics layer must reconcile later
    /// arrivals over its own reporting horizon.
    MissingAtDeadline {
        /// Missing 16-bit RTP sequence number.
        sequence: u16,
        /// Length of the current consecutive-missing burst, saturating at `u32::MAX`.
        burst_length: u32,
    },
}

/// Fixed-capacity reorder storage for one RTP sequence-number stream.
///
/// Construction allocates two fixed boxed slices. After construction, [`Self::push`] and
/// [`Self::pop_at_deadline`] do not allocate, resize, scan, or panic for any remote sequence
/// number. Each call is O(1). Capacity must be in `1..=32_767`, keeping RFC 3550-style serial
/// comparisons below the ambiguous half-range.
#[derive(Debug)]
pub struct ReorderBuffer<T> {
    slots: Box<[Option<Slot<T>>]>,
    recent_history: Box<[Option<HistoryEntry>]>,
    expected_sequence: Option<u16>,
    expected_extended: u64,
    consecutive_missing: u32,
}

impl<T> ReorderBuffer<T> {
    /// Creates a fixed-capacity reorder window.
    ///
    /// # Errors
    ///
    /// Returns [`ReorderConfigError`] when `capacity` is zero or is not below half of the
    /// 16-bit sequence-number space.
    pub fn new(capacity: usize) -> Result<Self, ReorderConfigError> {
        if capacity == 0 {
            return Err(ReorderConfigError::ZeroCapacity);
        }
        if capacity > MAX_CAPACITY {
            return Err(ReorderConfigError::CapacityExceedsSerialHalfRange);
        }

        let slots = std::iter::repeat_with(|| None)
            .take(capacity)
            .collect::<Box<[_]>>();
        let recent_history = vec![None; capacity].into_boxed_slice();
        Ok(Self {
            slots,
            recent_history,
            expected_sequence: None,
            expected_extended: 0,
            consecutive_missing: 0,
        })
    }

    /// Returns the immutable packet capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Returns the sequence number that the next deadline will decide, if initialized.
    #[must_use]
    pub fn expected_sequence(&self) -> Option<u16> {
        self.expected_sequence
    }

    /// Drops retained packets and all classification history, then rebases the playout head.
    ///
    /// This is an explicitly trusted, local recovery seam for a transport/control worker after it
    /// has validated a source restart or discontinuity. It is O(capacity), may drop packet values,
    /// and must not run on a real-time audio hot path. Never call it merely because remote input
    /// has a surprising sequence number; [`Self::push`] deliberately cannot trigger a rebase.
    pub fn reset_and_rebase(&mut self, next_sequence: u16) {
        for slot in &mut self.slots {
            *slot = None;
        }
        self.recent_history.fill(None);
        self.expected_sequence = Some(next_sequence);
        self.expected_extended = 0;
        self.consecutive_missing = 0;
    }

    /// Attempts to retain one packet without growing storage.
    ///
    /// A caller may use an [`RejectedPacket::AheadOfWindow`] result as an overload/discontinuity
    /// signal, but this core never advances playout merely because a remote packet jumps ahead.
    /// Only [`Self::pop_at_deadline`] may decide a playout gap and move the playout head.
    pub fn push(&mut self, sequence: u16, packet: T) -> PushResult<T> {
        let expected = match self.expected_sequence {
            Some(expected) => expected,
            None => {
                self.expected_sequence = Some(sequence);
                self.expected_extended = 0;
                return self.store(sequence, 0, packet, AcceptedPacket::InOrder);
            }
        };

        let forward = sequence.wrapping_sub(expected);
        if forward == SERIAL_HALF_RANGE {
            return PushResult::Rejected {
                reason: RejectedPacket::AmbiguousSerialDistance,
                packet,
            };
        }

        if forward < SERIAL_HALF_RANGE {
            let distance = forward;
            if usize::from(distance) >= self.capacity() {
                return PushResult::Rejected {
                    reason: RejectedPacket::AheadOfWindow { distance },
                    packet,
                };
            }

            let extended = self.expected_extended.wrapping_add(u64::from(distance));
            let index = self.slot_index(extended);
            if self.slots[index]
                .as_ref()
                .is_some_and(|slot| slot.extended_sequence == extended)
            {
                return PushResult::Rejected {
                    reason: RejectedPacket::Duplicate,
                    packet,
                };
            }

            let accepted = if distance == 0 {
                AcceptedPacket::InOrder
            } else {
                AcceptedPacket::Reordered { depth: distance }
            };
            return self.store(sequence, extended, packet, accepted);
        }

        let behind = expected.wrapping_sub(sequence);
        let reason = if u64::from(behind) <= self.expected_extended {
            let extended = self.expected_extended.wrapping_sub(u64::from(behind));
            let index = self.slot_index(extended);
            match self.recent_history[index] {
                Some(HistoryEntry {
                    extended_sequence,
                    observation: RecentObservation::Emitted | RecentObservation::LateSeen,
                }) if extended_sequence == extended => RejectedPacket::Duplicate,
                Some(HistoryEntry {
                    extended_sequence,
                    observation: RecentObservation::MissingAtDeadline,
                }) if extended_sequence == extended => {
                    self.recent_history[index] = Some(HistoryEntry {
                        extended_sequence: extended,
                        observation: RecentObservation::LateSeen,
                    });
                    RejectedPacket::Late { distance: behind }
                }
                _ => RejectedPacket::Late { distance: behind },
            }
        } else {
            RejectedPacket::Late { distance: behind }
        };
        PushResult::Rejected { reason, packet }
    }

    /// Decides the next sequence position at the caller-selected playout deadline.
    ///
    /// Calling this method too early creates a playout gap; timing policy remains the media
    /// worker's responsibility. A gap is not proof of network loss and must not be copied into
    /// RTCP network-loss accounting. Within the most recent `capacity()` decided positions, the
    /// first arrival after a missing deadline is [`RejectedPacket::Late`] and repeated copies are
    /// [`RejectedPacket::Duplicate`]. Older observations intentionally fall out of history.
    pub fn pop_at_deadline(&mut self) -> Playout<T> {
        let sequence = match self.expected_sequence {
            Some(sequence) => sequence,
            None => return Playout::Empty,
        };
        let extended = self.expected_extended;
        let index = self.slot_index(extended);
        let packet = match self.slots[index].take() {
            Some(slot) if slot.extended_sequence == extended => {
                self.recent_history[index] = Some(HistoryEntry {
                    extended_sequence: extended,
                    observation: RecentObservation::Emitted,
                });
                self.consecutive_missing = 0;
                Playout::Packet {
                    sequence: slot.sequence,
                    packet: slot.packet,
                }
            }
            Some(slot) => {
                // This cannot occur for a valid window, but retaining the unrelated slot is safer
                // than losing it if internal state is ever extended incorrectly.
                self.slots[index] = Some(slot);
                self.recent_history[index] = Some(HistoryEntry {
                    extended_sequence: extended,
                    observation: RecentObservation::MissingAtDeadline,
                });
                self.consecutive_missing = self.consecutive_missing.saturating_add(1);
                Playout::MissingAtDeadline {
                    sequence,
                    burst_length: self.consecutive_missing,
                }
            }
            None => {
                self.recent_history[index] = Some(HistoryEntry {
                    extended_sequence: extended,
                    observation: RecentObservation::MissingAtDeadline,
                });
                self.consecutive_missing = self.consecutive_missing.saturating_add(1);
                Playout::MissingAtDeadline {
                    sequence,
                    burst_length: self.consecutive_missing,
                }
            }
        };

        self.expected_sequence = Some(sequence.wrapping_add(1));
        self.expected_extended = self.expected_extended.wrapping_add(1);
        packet
    }

    fn store(
        &mut self,
        sequence: u16,
        extended_sequence: u64,
        packet: T,
        accepted: AcceptedPacket,
    ) -> PushResult<T> {
        let index = self.slot_index(extended_sequence);
        // A valid sub-half-range window maps every retained extended sequence to a unique slot.
        // Replace rather than assert so malformed remote input can never trigger a panic.
        self.slots[index] = Some(Slot {
            extended_sequence,
            sequence,
            packet,
        });
        PushResult::Accepted(accepted)
    }

    fn slot_index(&self, extended_sequence: u64) -> usize {
        (extended_sequence % self.capacity() as u64) as usize
    }
}

/// Invalid target-delay policy configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetDelayConfigError {
    /// Minimum target exceeded maximum target.
    InvertedBounds,
    /// Initial target was outside the inclusive bounds.
    InitialOutsideBounds,
    /// Increase and decrease steps must both be non-zero.
    ZeroStep,
    /// At least one stable observation is required before a decrease.
    ZeroStableObservations,
    /// The observation cadence must have a non-zero interval.
    ZeroObservationInterval,
}

impl fmt::Display for TargetDelayConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvertedBounds => formatter.write_str("minimum delay exceeds maximum delay"),
            Self::InitialOutsideBounds => {
                formatter.write_str("initial delay is outside the configured bounds")
            }
            Self::ZeroStep => formatter.write_str("delay adjustment steps must be non-zero"),
            Self::ZeroStableObservations => {
                formatter.write_str("stable observation count must be non-zero")
            }
            Self::ZeroObservationInterval => {
                formatter.write_str("target-delay observation interval must be non-zero")
            }
        }
    }
}

impl std::error::Error for TargetDelayConfigError {}

/// Validated configuration for [`TargetDelayPolicy`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetDelayConfig {
    /// Inclusive lower bound for the target.
    pub min_delay: Duration,
    /// Inclusive upper bound for the target.
    pub max_delay: Duration,
    /// Initial target, within the inclusive bounds.
    pub initial_delay: Duration,
    /// Minimum increase applied immediately for each pressure observation.
    pub increase_step: Duration,
    /// Decrease applied only after a complete stable hysteresis interval.
    pub decrease_step: Duration,
    /// Consecutive stable observations required before one decrease.
    pub stable_observations_before_decrease: u32,
    /// Fixed cadence at which the adapter must submit one coalesced observation.
    ///
    /// Packet events within an interval must be aggregated into one [`DelaySignal`]; callers must
    /// not call [`TargetDelayPolicy::observe`] once per packet.
    pub observation_interval: Duration,
}

/// A bounded input to the target-delay controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DelaySignal {
    /// No late/missing/reorder pressure occurred in this configured observation interval.
    Stable,
    /// Coalesced late/missing/reorder pressure for one configured observation interval.
    Pressure {
        /// Observed delay requirement. It is clamped to the configured bounds.
        required_delay: Duration,
    },
}

/// Result of a target-delay observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct TargetDelayUpdate {
    /// Target after applying the observation.
    pub target: Duration,
    /// Signed direction of the update without lossy numeric duration conversion.
    pub change: TargetDelayChange,
}

/// Direction of a target-delay update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetDelayChange {
    /// Target did not move.
    Held,
    /// Target increased immediately.
    Increased,
    /// Target decreased after the stable hysteresis interval.
    Decreased,
}

/// Bounded asymmetric target-delay policy.
///
/// Pressure immediately raises the target by at least `increase_step` (or to the reported
/// requirement), while decreases occur by `decrease_step` only after a configured run of stable
/// observations. The caller submits exactly one coalesced signal per
/// [`TargetDelayConfig::observation_interval`]; packet arrival batching must not change this
/// cadence. The state is fixed-size; [`Self::observe`] is O(1), allocation-free, and clamps all
/// untrusted durations rather than panicking or overflowing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetDelayPolicy {
    config: TargetDelayConfig,
    target: Duration,
    consecutive_stable: u32,
}

impl TargetDelayPolicy {
    /// Creates a validated bounded policy.
    ///
    /// # Errors
    ///
    /// Returns [`TargetDelayConfigError`] for inverted bounds, an out-of-range initial value,
    /// zero adjustment steps, a zero stable-observation count, or a zero observation cadence.
    pub fn new(config: TargetDelayConfig) -> Result<Self, TargetDelayConfigError> {
        if config.min_delay > config.max_delay {
            return Err(TargetDelayConfigError::InvertedBounds);
        }
        if config.initial_delay < config.min_delay || config.initial_delay > config.max_delay {
            return Err(TargetDelayConfigError::InitialOutsideBounds);
        }
        if config.increase_step.is_zero() || config.decrease_step.is_zero() {
            return Err(TargetDelayConfigError::ZeroStep);
        }
        if config.stable_observations_before_decrease == 0 {
            return Err(TargetDelayConfigError::ZeroStableObservations);
        }
        if config.observation_interval.is_zero() {
            return Err(TargetDelayConfigError::ZeroObservationInterval);
        }
        Ok(Self {
            config,
            target: config.initial_delay,
            consecutive_stable: 0,
        })
    }

    /// Returns the current bounded target delay.
    #[must_use]
    pub fn target(&self) -> Duration {
        self.target
    }

    /// Returns the required fixed cadence for coalesced observations.
    #[must_use]
    pub fn observation_interval(&self) -> Duration {
        self.config.observation_interval
    }

    /// Applies one deterministic pressure or stability observation.
    ///
    /// Call exactly once per [`Self::observation_interval`]. The caller aggregates all packet
    /// events in that interval: submit [`DelaySignal::Pressure`] with the interval's required
    /// delay when any pressure occurred, otherwise submit [`DelaySignal::Stable`].
    pub fn observe(&mut self, signal: DelaySignal) -> TargetDelayUpdate {
        let previous = self.target;
        match signal {
            DelaySignal::Pressure { required_delay } => {
                self.consecutive_stable = 0;
                let stepped = self.target.saturating_add(self.config.increase_step);
                self.target = stepped.max(required_delay).min(self.config.max_delay);
            }
            DelaySignal::Stable => {
                self.consecutive_stable = self.consecutive_stable.saturating_add(1);
                if self.consecutive_stable >= self.config.stable_observations_before_decrease {
                    self.consecutive_stable = 0;
                    self.target = self
                        .target
                        .saturating_sub(self.config.decrease_step)
                        .max(self.config.min_delay);
                }
            }
        }
        let change = match self.target.cmp(&previous) {
            core::cmp::Ordering::Less => TargetDelayChange::Decreased,
            core::cmp::Ordering::Equal => TargetDelayChange::Held,
            core::cmp::Ordering::Greater => TargetDelayChange::Increased,
        };
        TargetDelayUpdate {
            target: self.target,
            change,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepted<T>(result: PushResult<T>) -> AcceptedPacket {
        match result {
            PushResult::Accepted(value) => value,
            PushResult::Rejected { reason, .. } => panic!("unexpected rejection: {reason:?}"),
        }
    }

    fn rejected<T>(result: PushResult<T>) -> RejectedPacket {
        match result {
            PushResult::Rejected { reason, .. } => reason,
            PushResult::Accepted(value) => panic!("unexpected acceptance: {value:?}"),
        }
    }

    #[test]
    fn wraparound_keeps_packets_in_sequence() {
        let mut buffer = ReorderBuffer::new(10).expect("valid test capacity");
        assert_eq!(
            accepted(buffer.push(65_535, "last")),
            AcceptedPacket::InOrder
        );
        assert_eq!(
            accepted(buffer.push(0, "wrapped")),
            AcceptedPacket::Reordered { depth: 1 }
        );
        assert_eq!(
            buffer.pop_at_deadline(),
            Playout::Packet {
                sequence: 65_535,
                packet: "last"
            }
        );
        assert_eq!(
            buffer.pop_at_deadline(),
            Playout::Packet {
                sequence: 0,
                packet: "wrapped"
            }
        );
    }

    #[test]
    fn reordered_packets_wait_behind_the_gap() {
        let mut buffer = ReorderBuffer::new(8).expect("valid test capacity");
        assert_eq!(accepted(buffer.push(10, "ten")), AcceptedPacket::InOrder);
        assert_eq!(
            accepted(buffer.push(12, "twelve")),
            AcceptedPacket::Reordered { depth: 2 }
        );
        assert_eq!(
            accepted(buffer.push(11, "eleven")),
            AcceptedPacket::Reordered { depth: 1 }
        );
        for (sequence, packet) in [(10, "ten"), (11, "eleven"), (12, "twelve")] {
            assert_eq!(
                buffer.pop_at_deadline(),
                Playout::Packet { sequence, packet }
            );
        }
    }

    #[test]
    fn duplicates_are_rejected_before_and_after_playout() {
        let mut buffer = ReorderBuffer::new(4).expect("valid test capacity");
        assert_eq!(accepted(buffer.push(40, "first")), AcceptedPacket::InOrder);
        assert_eq!(
            rejected(buffer.push(40, "buffered duplicate")),
            RejectedPacket::Duplicate
        );
        let _ = buffer.pop_at_deadline();
        assert_eq!(
            rejected(buffer.push(40, "played duplicate")),
            RejectedPacket::Duplicate
        );
    }

    #[test]
    fn missing_bursts_and_late_arrivals_do_not_reopen_decisions() {
        let mut buffer = ReorderBuffer::new(8).expect("valid test capacity");
        let _ = buffer.push(100, "start");
        assert!(matches!(buffer.pop_at_deadline(), Playout::Packet { .. }));
        assert_eq!(
            buffer.pop_at_deadline(),
            Playout::MissingAtDeadline {
                sequence: 101,
                burst_length: 1
            }
        );
        assert_eq!(
            buffer.pop_at_deadline(),
            Playout::MissingAtDeadline {
                sequence: 102,
                burst_length: 2
            }
        );
        assert_eq!(
            rejected(buffer.push(101, "too late")),
            RejectedPacket::Late { distance: 2 }
        );
        assert_eq!(
            accepted(buffer.push(103, "recovered")),
            AcceptedPacket::InOrder
        );
        assert!(matches!(
            buffer.pop_at_deadline(),
            Playout::Packet {
                sequence: 103,
                packet: "recovered"
            }
        ));
        assert_eq!(
            buffer.pop_at_deadline(),
            Playout::MissingAtDeadline {
                sequence: 104,
                burst_length: 1
            }
        );
    }

    #[test]
    fn outside_window_and_ambiguous_input_are_bounded_rejections() {
        let mut buffer = ReorderBuffer::new(4).expect("valid test capacity");
        let _ = buffer.push(1_000, ());
        assert_eq!(
            rejected(buffer.push(1_004, ())),
            RejectedPacket::AheadOfWindow { distance: 4 }
        );
        assert_eq!(
            rejected(buffer.push(1_000u16.wrapping_add(32_768), ())),
            RejectedPacket::AmbiguousSerialDistance
        );
        assert_eq!(buffer.capacity(), 4);
    }

    #[test]
    fn first_post_deadline_arrival_is_late_then_repeated_copy_is_duplicate() {
        let mut buffer = ReorderBuffer::new(4).expect("valid test capacity");
        let _ = buffer.push(100, "start");
        let _ = buffer.pop_at_deadline();
        assert!(matches!(
            buffer.pop_at_deadline(),
            Playout::MissingAtDeadline { sequence: 101, .. }
        ));
        assert_eq!(
            rejected(buffer.push(101, "first late copy")),
            RejectedPacket::Late { distance: 1 }
        );
        assert_eq!(
            rejected(buffer.push(101, "repeated late copy")),
            RejectedPacket::Duplicate
        );
    }

    #[test]
    fn half_range_boundaries_are_classified_without_aliasing() {
        let mut buffer = ReorderBuffer::new(MAX_CAPACITY).expect("maximum capacity is valid");
        let start = 1_000u16;
        let _ = buffer.push(start, ());
        assert_eq!(
            accepted(buffer.push(start.wrapping_add(32_766), ())),
            AcceptedPacket::Reordered { depth: 32_766 }
        );
        assert_eq!(
            rejected(buffer.push(start.wrapping_add(32_767), ())),
            RejectedPacket::AheadOfWindow { distance: 32_767 }
        );
        assert_eq!(
            rejected(buffer.push(start.wrapping_add(32_768), ())),
            RejectedPacket::AmbiguousSerialDistance
        );
        assert_eq!(
            rejected(buffer.push(start.wrapping_add(32_769), ())),
            RejectedPacket::Late { distance: 32_767 }
        );
    }

    #[test]
    fn minimum_capacity_and_repeated_sequence_wraps_preserve_order() {
        let mut buffer = ReorderBuffer::new(1).expect("minimum capacity is valid");
        let total_positions = usize::from(u16::MAX) * 2 + 3;
        for position in 0..total_positions {
            let sequence = position as u16;
            assert_eq!(
                accepted(buffer.push(sequence, position)),
                AcceptedPacket::InOrder
            );
            assert_eq!(
                buffer.pop_at_deadline(),
                Playout::Packet {
                    sequence,
                    packet: position
                }
            );
        }
        assert_eq!(buffer.capacity(), 1);
    }

    #[test]
    fn trusted_reset_drops_packets_clears_history_and_rebases() {
        let mut buffer = ReorderBuffer::new(4).expect("valid test capacity");
        let _ = buffer.push(10, "emitted before reset");
        let _ = buffer.push(12, "buffered then dropped");
        let _ = buffer.pop_at_deadline();
        assert_eq!(
            rejected(buffer.push(10, "duplicate before reset")),
            RejectedPacket::Duplicate
        );

        buffer.reset_and_rebase(500);

        assert_eq!(buffer.expected_sequence(), Some(500));
        assert_eq!(
            rejected(buffer.push(10, "history was cleared")),
            RejectedPacket::Late { distance: 490 }
        );
        assert_eq!(
            accepted(buffer.push(500, "new source position")),
            AcceptedPacket::InOrder
        );
        assert_eq!(
            buffer.pop_at_deadline(),
            Playout::Packet {
                sequence: 500,
                packet: "new source position"
            }
        );
        assert_eq!(
            buffer.pop_at_deadline(),
            Playout::MissingAtDeadline {
                sequence: 501,
                burst_length: 1
            }
        );
    }

    fn delay_config() -> TargetDelayConfig {
        TargetDelayConfig {
            min_delay: Duration::from_millis(20),
            max_delay: Duration::from_millis(80),
            initial_delay: Duration::from_millis(30),
            increase_step: Duration::from_millis(15),
            decrease_step: Duration::from_millis(5),
            stable_observations_before_decrease: 4,
            observation_interval: Duration::from_millis(20),
        }
    }

    #[test]
    fn target_grows_quickly_and_never_exceeds_maximum() {
        let mut policy = TargetDelayPolicy::new(delay_config()).expect("valid test policy");
        let first = policy.observe(DelaySignal::Pressure {
            required_delay: Duration::from_millis(55),
        });
        assert_eq!(first.target, Duration::from_millis(55));
        assert_eq!(first.change, TargetDelayChange::Increased);
        for _ in 0..10 {
            let _ = policy.observe(DelaySignal::Pressure {
                required_delay: Duration::MAX,
            });
        }
        assert_eq!(policy.target(), Duration::from_millis(80));
    }

    #[test]
    fn target_shrinks_slowly_after_stability() {
        let mut policy = TargetDelayPolicy::new(delay_config()).expect("valid test policy");
        let _ = policy.observe(DelaySignal::Pressure {
            required_delay: Duration::from_millis(60),
        });
        for _ in 0..3 {
            assert_eq!(
                policy.observe(DelaySignal::Stable).change,
                TargetDelayChange::Held
            );
        }
        assert_eq!(
            policy.observe(DelaySignal::Stable),
            TargetDelayUpdate {
                target: Duration::from_millis(55),
                change: TargetDelayChange::Decreased
            }
        );
        for _ in 0..100 {
            let _ = policy.observe(DelaySignal::Stable);
        }
        assert_eq!(policy.target(), Duration::from_millis(20));
    }

    #[test]
    fn intermittent_pressure_resets_shrink_hysteresis_without_oscillation() {
        let mut policy = TargetDelayPolicy::new(delay_config()).expect("valid test policy");
        for _ in 0..3 {
            let _ = policy.observe(DelaySignal::Stable);
        }
        let pressure = policy.observe(DelaySignal::Pressure {
            required_delay: Duration::from_millis(35),
        });
        assert_eq!(pressure.target, Duration::from_millis(45));
        for _ in 0..3 {
            assert_eq!(
                policy.observe(DelaySignal::Stable).target,
                Duration::from_millis(45)
            );
        }
        assert_eq!(policy.target(), Duration::from_millis(45));
    }

    #[test]
    fn observation_cadence_is_explicit_and_drives_interval_hysteresis() {
        let mut policy = TargetDelayPolicy::new(delay_config()).expect("valid test policy");
        assert_eq!(policy.observation_interval(), Duration::from_millis(20));
        let _ = policy.observe(DelaySignal::Pressure {
            required_delay: Duration::from_millis(60),
        });
        for elapsed_intervals in 1..=4 {
            let update = policy.observe(DelaySignal::Stable);
            if elapsed_intervals < 4 {
                assert_eq!(update.change, TargetDelayChange::Held);
            } else {
                assert_eq!(update.change, TargetDelayChange::Decreased);
            }
        }
    }

    #[test]
    fn zero_observation_cadence_is_rejected() {
        let mut config = delay_config();
        config.observation_interval = Duration::ZERO;
        assert_eq!(
            TargetDelayPolicy::new(config),
            Err(TargetDelayConfigError::ZeroObservationInterval)
        );
    }

    #[test]
    fn invalid_configurations_return_errors_instead_of_panicking() {
        assert_eq!(
            ReorderBuffer::<()>::new(0).map(|_| ()),
            Err(ReorderConfigError::ZeroCapacity)
        );
        let mut config = delay_config();
        config.min_delay = Duration::from_millis(90);
        assert_eq!(
            TargetDelayPolicy::new(config),
            Err(TargetDelayConfigError::InvertedBounds)
        );
    }
}

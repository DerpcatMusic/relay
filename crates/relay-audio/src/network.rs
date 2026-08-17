use core::fmt;
use std::time::Duration;

use crate::MediaPacket;

/// Monotonic virtual network time measured in integer microseconds.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NetworkTime(u64);

impl NetworkTime {
    /// Virtual time zero.
    pub const ZERO: Self = Self(0);

    /// Creates an exact virtual instant.
    #[must_use]
    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    /// Returns the integer virtual microsecond value.
    #[must_use]
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    fn checked_add(self, delay: Duration) -> Option<Self> {
        let micros = u64::try_from(delay.as_micros()).ok()?;
        self.0.checked_add(micros).map(Self)
    }
}

/// An explicit deterministic disposition for one submitted packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkAction {
    /// Schedule one copy at the current virtual time.
    Deliver,
    /// Consume the packet without scheduling it and record simulated loss.
    Drop,
    /// Schedule the original now and another owned copy after `duplicate_delay`.
    Duplicate {
        /// Delay applied only to the second copy.
        duplicate_delay: Duration,
    },
    /// Schedule one copy after `delay`.
    Delay {
        /// Deterministic virtual delay.
        delay: Duration,
    },
}

/// Why a packet submission returned ownership without scheduling anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleRejection {
    /// No scheduled slot was free.
    Full,
    /// Adding the requested delay exceeded [`NetworkTime`].
    TimeOverflow,
    /// Stable insertion ordinals could no longer advance.
    InsertionOrdinalOverflow,
}

/// Small disposition value for one packet submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduleStatus {
    /// One or two copies were retained in scheduled storage.
    Scheduled {
        /// Number of retained copies. A duplicate may retain only its original
        /// when the second slot is unavailable.
        copies: usize,
    },
    /// The explicit drop action consumed the packet.
    Dropped,
    /// Nothing was retained.
    Rejected(ScheduleRejection),
}

/// Allocation-free result of applying a [`NetworkAction`].
///
/// The fixed-inline packet field is populated only for a primary rejection.
/// Keeping status separate from returned ownership avoids both heap indirection
/// and a size-imbalanced result enum.
#[derive(Debug)]
#[must_use]
pub struct ScheduleOutcome {
    status: ScheduleStatus,
    returned_packet: Option<MediaPacket>,
}

impl ScheduleOutcome {
    /// Returns the small copyable disposition.
    #[must_use]
    pub const fn status(&self) -> ScheduleStatus {
        self.status
    }

    /// Returns a borrowed rejected packet, if primary ownership was returned.
    #[must_use]
    pub const fn returned_packet(&self) -> Option<&MediaPacket> {
        self.returned_packet.as_ref()
    }

    /// Moves out a rejected packet, if primary ownership was returned.
    #[must_use]
    pub fn into_returned_packet(self) -> Option<MediaPacket> {
        self.returned_packet
    }

    fn scheduled(copies: usize) -> Self {
        Self {
            status: ScheduleStatus::Scheduled { copies },
            returned_packet: None,
        }
    }

    fn dropped() -> Self {
        Self {
            status: ScheduleStatus::Dropped,
            returned_packet: None,
        }
    }

    fn rejected(reason: ScheduleRejection, packet: MediaPacket) -> Self {
        Self {
            status: ScheduleStatus::Rejected(reason),
            returned_packet: Some(packet),
        }
    }
}

/// Immutable fake-network truth counters.
///
/// These are model facts, not receiver observations. In particular,
/// `simulated_drops` must never be inferred from playout gaps.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NetworkMetrics {
    /// Packet actions submitted by the caller.
    pub submitted: u64,
    /// Copies retained in scheduled slots.
    pub scheduled_copies: u64,
    /// Copies moved into due batches.
    pub delivered_copies: u64,
    /// Explicit simulated drop actions.
    pub simulated_drops: u64,
    /// Explicit duplicate actions.
    pub duplicate_requests: u64,
    /// Second copies successfully retained.
    pub duplicate_copies_scheduled: u64,
    /// Second copies rejected because only the original slot was free.
    pub duplicate_capacity_rejections: u64,
    /// Primary packets returned because storage was full.
    pub capacity_rejections: u64,
    /// Actions returned because delivery-time arithmetic overflowed.
    pub time_overflow_rejections: u64,
    /// Actions returned because insertion ordinals overflowed.
    pub ordinal_overflow_rejections: u64,
}

/// Why fixed fake-network storage could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkConfigError {
    /// Scheduled capacity was zero.
    ZeroCapacity,
    /// Maximum due-batch capacity was zero.
    ZeroDueBatchCapacity,
    /// A due batch cannot be larger than scheduled storage.
    DueBatchExceedsCapacity,
    /// Capacity byte arithmetic exceeded `usize`.
    CapacityOverflow,
    /// The allocator rejected the fixed construction-time request.
    AllocationFailed,
}

impl fmt::Display for NetworkConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for NetworkConfigError {}

#[derive(Debug)]
struct ScheduledPacket {
    deliver_at: NetworkTime,
    insertion_ordinal: u64,
    packet: MediaPacket,
}

/// Fixed-capacity output storage reused across advances or drains.
///
/// Construction allocates exactly once. Consume packets with [`Self::take_next`]
/// before reusing the batch, or explicitly [`Self::clear`] them on a worker.
#[derive(Debug)]
pub struct DueBatch {
    slots: Box<[Option<MediaPacket>]>,
    cursor: usize,
    len: usize,
}

impl DueBatch {
    /// Allocates fixed output slots.
    ///
    /// # Errors
    ///
    /// Zero, overflowing, or unallocatable capacities are rejected.
    pub fn new(capacity: usize) -> Result<Self, DueBatchError> {
        if capacity == 0 {
            return Err(DueBatchError::ZeroCapacity);
        }
        capacity
            .checked_mul(core::mem::size_of::<Option<MediaPacket>>())
            .ok_or(DueBatchError::CapacityOverflow)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity)
            .map_err(|_| DueBatchError::AllocationFailed)?;
        slots.resize_with(capacity, || None);
        Ok(Self {
            slots: slots.into_boxed_slice(),
            cursor: 0,
            len: 0,
        })
    }

    /// Returns the immutable packet capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Returns the number of packets not yet taken.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len.saturating_sub(self.cursor)
    }

    /// Reports whether no packets remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Moves the next packet in stable delivery order out of this batch.
    pub fn take_next(&mut self) -> Option<MediaPacket> {
        if self.cursor == self.len {
            self.prepare_for_fill();
            return None;
        }
        let packet = self.slots[self.cursor].take();
        self.cursor += 1;
        if self.cursor == self.len {
            self.prepare_for_fill();
        }
        packet
    }

    /// Drops all unconsumed packets and makes the fixed storage reusable.
    ///
    /// Packet destruction belongs on a worker/control path, not an audio callback.
    pub fn clear(&mut self) {
        for slot in &mut self.slots[..self.len] {
            *slot = None;
        }
        self.cursor = 0;
        self.len = 0;
    }

    fn prepare_for_fill(&mut self) {
        self.cursor = 0;
        self.len = 0;
    }

    fn push(&mut self, packet: MediaPacket) {
        self.slots[self.len] = Some(packet);
        self.len += 1;
    }
}

/// Why a [`DueBatch`] could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DueBatchError {
    /// A zero-capacity batch cannot return a packet.
    ZeroCapacity,
    /// Capacity byte arithmetic exceeded `usize`.
    CapacityOverflow,
    /// The allocator rejected the fixed construction-time request.
    AllocationFailed,
}

impl fmt::Display for DueBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for DueBatchError {}

/// Why virtual time could not be advanced into a due batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdvanceError {
    /// Virtual time cannot move backward.
    TimeMovedBackward {
        /// Current network time.
        current: NetworkTime,
        /// Rejected requested time.
        requested: NetworkTime,
    },
    /// Unconsumed packets must not be overwritten.
    BatchNotEmpty,
    /// The supplied batch exceeds the network's configured per-call bound.
    BatchExceedsConfiguredMaximum {
        /// Supplied fixed capacity.
        actual: usize,
        /// Configured per-call maximum.
        maximum: usize,
    },
}

impl fmt::Display for AdvanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for AdvanceError {}

/// Bounded result of advancing virtual time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdvanceReport {
    /// Packets moved into the batch during this call.
    pub delivered: usize,
    /// Scheduled packets already due but left queued by the batch bound.
    pub due_remaining: usize,
    /// All copies still scheduled, due or future.
    pub queued: usize,
}

/// Bounded result of draining without regard to delivery time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrainReport {
    /// Packets moved into the batch during this call.
    pub drained: usize,
    /// All copies still scheduled.
    pub queued: usize,
}

/// Preallocated, caller-driven deterministic fake network.
///
/// Slot indices are not delivery order. Every extraction selects the smallest
/// `(delivery_time, insertion_ordinal)` pair, giving stable ties and allowing
/// delay to create reproducible reordering. All steady-state operations are
/// bounded by construction capacities and perform no heap growth.
#[derive(Debug)]
pub struct DeterministicNetwork {
    slots: Box<[Option<ScheduledPacket>]>,
    now: NetworkTime,
    next_ordinal: u64,
    max_due_batch: usize,
    queued: usize,
    metrics: NetworkMetrics,
}

impl DeterministicNetwork {
    /// Allocates a fixed number of scheduled-copy slots.
    ///
    /// # Errors
    ///
    /// Both capacities must be nonzero, the batch bound cannot exceed scheduled
    /// capacity, and capacity arithmetic/allocation must succeed.
    pub fn new(capacity: usize, max_due_batch: usize) -> Result<Self, NetworkConfigError> {
        if capacity == 0 {
            return Err(NetworkConfigError::ZeroCapacity);
        }
        if max_due_batch == 0 {
            return Err(NetworkConfigError::ZeroDueBatchCapacity);
        }
        if max_due_batch > capacity {
            return Err(NetworkConfigError::DueBatchExceedsCapacity);
        }
        capacity
            .checked_mul(core::mem::size_of::<Option<ScheduledPacket>>())
            .ok_or(NetworkConfigError::CapacityOverflow)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity)
            .map_err(|_| NetworkConfigError::AllocationFailed)?;
        slots.resize_with(capacity, || None);
        Ok(Self {
            slots: slots.into_boxed_slice(),
            now: NetworkTime::ZERO,
            next_ordinal: 0,
            max_due_batch,
            queued: 0,
            metrics: NetworkMetrics::default(),
        })
    }

    /// Returns scheduled-copy capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Returns the configured per-call due bound.
    #[must_use]
    pub const fn max_due_batch(&self) -> usize {
        self.max_due_batch
    }

    /// Returns current virtual time.
    #[must_use]
    pub const fn now(&self) -> NetworkTime {
        self.now
    }

    /// Returns the number of retained scheduled copies.
    #[must_use]
    pub const fn queued(&self) -> usize {
        self.queued
    }

    /// Returns a coherent caller-thread snapshot of model truth.
    #[must_use]
    pub const fn metrics(&self) -> NetworkMetrics {
        self.metrics
    }

    /// Applies an explicit action at current virtual time.
    ///
    /// If the primary copy cannot fit, ownership is returned in
    /// [`ScheduleStatus::Rejected`]. A duplicate with exactly one free slot
    /// schedules its original, rejects only the second copy, and records that
    /// fact in truth metrics.
    pub fn schedule(&mut self, packet: MediaPacket, action: NetworkAction) -> ScheduleOutcome {
        self.metrics.submitted = self.metrics.submitted.saturating_add(1);
        let (primary_time, duplicate_time) = match action {
            NetworkAction::Deliver => (self.now, None),
            NetworkAction::Drop => {
                self.metrics.simulated_drops = self.metrics.simulated_drops.saturating_add(1);
                return ScheduleOutcome::dropped();
            }
            NetworkAction::Delay { delay } => {
                let Some(deliver_at) = self.now.checked_add(delay) else {
                    self.metrics.time_overflow_rejections =
                        self.metrics.time_overflow_rejections.saturating_add(1);
                    return rejected(packet, ScheduleRejection::TimeOverflow);
                };
                (deliver_at, None)
            }
            NetworkAction::Duplicate { duplicate_delay } => {
                self.metrics.duplicate_requests = self.metrics.duplicate_requests.saturating_add(1);
                let Some(deliver_at) = self.now.checked_add(duplicate_delay) else {
                    self.metrics.time_overflow_rejections =
                        self.metrics.time_overflow_rejections.saturating_add(1);
                    return rejected(packet, ScheduleRejection::TimeOverflow);
                };
                (self.now, Some(deliver_at))
            }
        };

        let available = self.capacity() - self.queued;
        if available == 0 {
            self.metrics.capacity_rejections = self.metrics.capacity_rejections.saturating_add(1);
            return rejected(packet, ScheduleRejection::Full);
        }
        let copies = if duplicate_time.is_some() && available >= 2 {
            2
        } else {
            1
        };
        let Some(next_ordinal) = self.next_ordinal.checked_add(copies as u64) else {
            self.metrics.ordinal_overflow_rejections =
                self.metrics.ordinal_overflow_rejections.saturating_add(1);
            return rejected(packet, ScheduleRejection::InsertionOrdinalOverflow);
        };

        let duplicate = (copies == 2).then(|| packet.clone());
        self.insert(ScheduledPacket {
            deliver_at: primary_time,
            insertion_ordinal: self.next_ordinal,
            packet,
        });
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        if let (Some(deliver_at), Some(packet)) = (duplicate_time, duplicate) {
            self.insert(ScheduledPacket {
                deliver_at,
                insertion_ordinal: self.next_ordinal,
                packet,
            });
            self.next_ordinal = next_ordinal;
            self.metrics.duplicate_copies_scheduled =
                self.metrics.duplicate_copies_scheduled.saturating_add(1);
        } else if duplicate_time.is_some() {
            self.metrics.duplicate_capacity_rejections =
                self.metrics.duplicate_capacity_rejections.saturating_add(1);
        }
        self.metrics.scheduled_copies = self.metrics.scheduled_copies.saturating_add(copies as u64);
        ScheduleOutcome::scheduled(copies)
    }

    /// Advances monotonically and moves a bounded ordered prefix of due copies.
    ///
    /// # Errors
    ///
    /// Time cannot move backward, the batch must be empty, and its capacity
    /// cannot exceed this network's configured maximum.
    pub fn advance_to(
        &mut self,
        requested: NetworkTime,
        batch: &mut DueBatch,
    ) -> Result<AdvanceReport, AdvanceError> {
        self.validate_batch(batch)?;
        if requested < self.now {
            return Err(AdvanceError::TimeMovedBackward {
                current: self.now,
                requested,
            });
        }
        self.now = requested;
        let delivered = self.extract(batch, Some(requested));
        let due_remaining = self
            .slots
            .iter()
            .flatten()
            .filter(|scheduled| scheduled.deliver_at <= requested)
            .count();
        self.metrics.delivered_copies = self
            .metrics
            .delivered_copies
            .saturating_add(delivered as u64);
        Ok(AdvanceReport {
            delivered,
            due_remaining,
            queued: self.queued,
        })
    }

    /// Moves a bounded ordered prefix of all scheduled copies regardless of time.
    ///
    /// # Errors
    ///
    /// The same batch reuse and configured-capacity rules as [`Self::advance_to`]
    /// apply.
    pub fn drain(&mut self, batch: &mut DueBatch) -> Result<DrainReport, AdvanceError> {
        self.validate_batch(batch)?;
        let drained = self.extract(batch, None);
        self.metrics.delivered_copies =
            self.metrics.delivered_copies.saturating_add(drained as u64);
        Ok(DrainReport {
            drained,
            queued: self.queued,
        })
    }

    /// Drops every scheduled packet and restores initial deterministic state.
    ///
    /// Returns the number of discarded scheduled copies. Virtual time, insertion
    /// ordinals, and all truth counters return to zero; fixed allocations remain.
    pub fn reset(&mut self) -> usize {
        let discarded = self.queued;
        for slot in &mut self.slots {
            *slot = None;
        }
        self.now = NetworkTime::ZERO;
        self.next_ordinal = 0;
        self.queued = 0;
        self.metrics = NetworkMetrics::default();
        discarded
    }

    fn validate_batch(&self, batch: &DueBatch) -> Result<(), AdvanceError> {
        if !batch.is_empty() {
            return Err(AdvanceError::BatchNotEmpty);
        }
        if batch.capacity() > self.max_due_batch {
            return Err(AdvanceError::BatchExceedsConfiguredMaximum {
                actual: batch.capacity(),
                maximum: self.max_due_batch,
            });
        }
        Ok(())
    }

    fn insert(&mut self, scheduled: ScheduledPacket) {
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(scheduled);
            self.queued += 1;
        }
    }

    fn extract(&mut self, batch: &mut DueBatch, due_by: Option<NetworkTime>) -> usize {
        while batch.len < batch.capacity() {
            let Some(index) = self.earliest_index(due_by) else {
                break;
            };
            if let Some(scheduled) = self.slots[index].take() {
                batch.push(scheduled.packet);
                self.queued -= 1;
            }
        }
        batch.len
    }

    fn earliest_index(&self, due_by: Option<NetworkTime>) -> Option<usize> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_ref().map(|scheduled| (index, scheduled)))
            .filter(|(_, scheduled)| due_by.is_none_or(|deadline| scheduled.deliver_at <= deadline))
            .min_by_key(|(_, scheduled)| (scheduled.deliver_at, scheduled.insertion_ordinal))
            .map(|(index, _)| index)
    }
}

fn rejected(packet: MediaPacket, reason: ScheduleRejection) -> ScheduleOutcome {
    ScheduleOutcome::rejected(reason, packet)
}

use core::fmt;

const SEQUENCE_HALF_RANGE: u16 = 1 << 15;
const SEQUENCE_MODULUS: u64 = 1 << 16;
const TIMESTAMP_HALF_RANGE: u32 = 1 << 31;
const TIMESTAMP_MODULUS: u64 = 1 << 32;

/// Synchronization-source identity for one validated RTP-like stream epoch.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Ssrc(u32);

impl Ssrc {
    /// Wraps the complete 32-bit wire identity without reserving sentinel values.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl From<u32> for Ssrc {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

/// A wrapping 16-bit RTP sequence number on the wire.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SequenceNumber(u16);

impl SequenceNumber {
    /// Creates a wire sequence value.
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the wire value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// Advances by one with defined wire wrapping.
    #[must_use]
    pub const fn wrapping_next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

impl From<u16> for SequenceNumber {
    fn from(value: u16) -> Self {
        Self::new(value)
    }
}

/// A wrapping 32-bit RTP media timestamp on the wire.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RtpTimestamp(u32);

impl RtpTimestamp {
    /// Creates a wire timestamp value.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Advances by an exact media-frame count with defined wire wrapping.
    #[must_use]
    pub const fn wrapping_add(self, frames: u32) -> Self {
        Self(self.0.wrapping_add(frames))
    }
}

impl From<u32> for RtpTimestamp {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

/// Epoch-relative, non-wrapping sequence position.
///
/// Its low 16 bits are the corresponding [`SequenceNumber`]. A trusted local
/// epoch reset chooses a new starting value; remote input is only extended
/// relative to an existing value and can never reset it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtendedSequence(u64);

impl ExtendedSequence {
    /// Creates a validated-local epoch position.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Starts an epoch at the first observed wire sequence.
    #[must_use]
    pub const fn starting_at(wire: SequenceNumber) -> Self {
        Self(wire.0 as u64)
    }

    /// Returns the non-wrapping position.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the low wire bits represented by this position.
    #[must_use]
    pub const fn wire(self) -> SequenceNumber {
        SequenceNumber(self.0 as u16)
    }

    /// Extends `wire` to the nearest position in this epoch.
    ///
    /// # Errors
    ///
    /// Returns [`ExtensionError::AmbiguousHalfRange`] at exactly 32,768
    /// sequence positions, and reports underflow/overflow rather than selecting
    /// a non-nearest epoch.
    pub const fn extend(self, wire: SequenceNumber) -> Result<Self, ExtensionError> {
        extend_sequence(self, wire)
    }
}

/// Epoch-relative, non-wrapping RTP media-frame position.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtendedTimestamp(u64);

impl ExtendedTimestamp {
    /// Creates a validated-local epoch position.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Starts an epoch at the first observed wire timestamp.
    #[must_use]
    pub const fn starting_at(wire: RtpTimestamp) -> Self {
        Self(wire.0 as u64)
    }

    /// Returns the non-wrapping position.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the low wire bits represented by this position.
    #[must_use]
    pub const fn wire(self) -> RtpTimestamp {
        RtpTimestamp(self.0 as u32)
    }

    /// Extends `wire` to the nearest position in this epoch.
    ///
    /// # Errors
    ///
    /// Returns [`ExtensionError::AmbiguousHalfRange`] at exactly 2^31 media
    /// frames, and reports underflow/overflow rather than selecting another epoch.
    pub const fn extend(self, wire: RtpTimestamp) -> Result<Self, ExtensionError> {
        extend_timestamp(self, wire)
    }
}

/// Why a wrapping wire value has no accepted nearest extended position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionError {
    /// Exactly half the serial space has no defined before/after direction.
    AmbiguousHalfRange,
    /// The nearest mathematical position lies before this local epoch's zero.
    BeforeEpoch,
    /// The nearest mathematical position exceeds the `u64` extended timeline.
    ExtendedOverflow,
}

impl fmt::Display for ExtensionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AmbiguousHalfRange => {
                formatter.write_str("wire value is exactly half a serial range away")
            }
            Self::BeforeEpoch => formatter.write_str("nearest position lies before epoch zero"),
            Self::ExtendedOverflow => formatter.write_str("extended timeline overflow"),
        }
    }
}

impl std::error::Error for ExtensionError {}

/// Extends a wire sequence to its unique nearest position around `reference`.
///
/// # Errors
///
/// See [`ExtendedSequence::extend`].
pub const fn extend_sequence(
    reference: ExtendedSequence,
    wire: SequenceNumber,
) -> Result<ExtendedSequence, ExtensionError> {
    let forward = wire.0.wrapping_sub(reference.wire().0);
    if forward == SEQUENCE_HALF_RANGE {
        return Err(ExtensionError::AmbiguousHalfRange);
    }
    if forward < SEQUENCE_HALF_RANGE {
        match reference.0.checked_add(forward as u64) {
            Some(value) => Ok(ExtendedSequence(value)),
            None => Err(ExtensionError::ExtendedOverflow),
        }
    } else {
        let backward = SEQUENCE_MODULUS - forward as u64;
        match reference.0.checked_sub(backward) {
            Some(value) => Ok(ExtendedSequence(value)),
            None => Err(ExtensionError::BeforeEpoch),
        }
    }
}

/// Extends a wire timestamp to its unique nearest position around `reference`.
///
/// # Errors
///
/// See [`ExtendedTimestamp::extend`].
pub const fn extend_timestamp(
    reference: ExtendedTimestamp,
    wire: RtpTimestamp,
) -> Result<ExtendedTimestamp, ExtensionError> {
    let forward = wire.0.wrapping_sub(reference.wire().0);
    if forward == TIMESTAMP_HALF_RANGE {
        return Err(ExtensionError::AmbiguousHalfRange);
    }
    if forward < TIMESTAMP_HALF_RANGE {
        match reference.0.checked_add(forward as u64) {
            Some(value) => Ok(ExtendedTimestamp(value)),
            None => Err(ExtensionError::ExtendedOverflow),
        }
    } else {
        let backward = TIMESTAMP_MODULUS - forward as u64;
        match reference.0.checked_sub(backward) {
            Some(value) => Ok(ExtendedTimestamp(value)),
            None => Err(ExtensionError::BeforeEpoch),
        }
    }
}

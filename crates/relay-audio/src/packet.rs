use core::fmt;

use crate::{RtpTimestamp, SequenceNumber, Ssrc};

/// Maximum encoded bytes retained inline by one [`MediaPacket`].
pub const MAX_PACKET_BYTES: usize = relay_opus::MAX_PACKET_BYTES;

/// A validated seven-bit RTP payload type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PayloadType(u8);

impl PayloadType {
    /// Parses the seven payload-type bits independently of the RTP marker bit.
    ///
    /// # Errors
    ///
    /// Values above 127 are not representable by the RTP payload-type field.
    pub const fn new(value: u8) -> Result<Self, PayloadTypeError> {
        if value <= 0x7f {
            Ok(Self(value))
        } else {
            Err(PayloadTypeError(value))
        }
    }

    /// Returns the seven-bit wire value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for PayloadType {
    type Error = PayloadTypeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// A value did not fit the RTP seven-bit payload-type field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayloadTypeError(pub u8);

impl fmt::Display for PayloadTypeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "payload type {} exceeds 127", self.0)
    }
}

impl std::error::Error for PayloadTypeError {}

/// One owned RTP-like media packet with fixed inline payload storage.
///
/// Cloning is explicit and copies the fixed 4,000-byte storage; the deterministic
/// network uses it only for a requested duplicate. Packet creation performs no
/// heap allocation and unused payload bytes remain zeroed.
#[derive(Clone, Eq, PartialEq)]
pub struct MediaPacket {
    ssrc: Ssrc,
    sequence: SequenceNumber,
    timestamp: RtpTimestamp,
    payload_type: PayloadType,
    payload_len: u16,
    payload: [u8; MAX_PACKET_BYTES],
}

impl MediaPacket {
    /// Copies one non-empty encoded payload into fixed inline storage.
    ///
    /// # Errors
    ///
    /// Returns [`PacketError::EmptyPayload`] for no encoded bytes or
    /// [`PacketError::PayloadTooLarge`] above [`MAX_PACKET_BYTES`].
    pub fn new(
        ssrc: Ssrc,
        sequence: SequenceNumber,
        timestamp: RtpTimestamp,
        payload_type: PayloadType,
        payload: &[u8],
    ) -> Result<Self, PacketError> {
        Self::new_with_max_payload(
            ssrc,
            sequence,
            timestamp,
            payload_type,
            payload,
            MAX_PACKET_BYTES,
        )
    }

    pub(crate) fn new_with_max_payload(
        ssrc: Ssrc,
        sequence: SequenceNumber,
        timestamp: RtpTimestamp,
        payload_type: PayloadType,
        payload: &[u8],
        maximum_payload: usize,
    ) -> Result<Self, PacketError> {
        validate_payload_len(payload.len(), maximum_payload)?;
        let mut storage = [0; MAX_PACKET_BYTES];
        storage[..payload.len()].copy_from_slice(payload);
        Ok(Self {
            ssrc,
            sequence,
            timestamp,
            payload_type,
            payload_len: payload.len() as u16,
            payload: storage,
        })
    }

    /// Validates a raw payload type and encoded payload in one boundary call.
    ///
    /// # Errors
    ///
    /// Returns [`PacketError::InvalidPayloadType`] above payload type 127, plus
    /// the length failures documented by [`Self::new`].
    pub fn try_new(
        ssrc: u32,
        sequence: u16,
        timestamp: u32,
        payload_type: u8,
        payload: &[u8],
    ) -> Result<Self, PacketError> {
        let payload_type = PayloadType::new(payload_type)
            .map_err(|error| PacketError::InvalidPayloadType(error.0))?;
        Self::new(
            Ssrc::new(ssrc),
            SequenceNumber::new(sequence),
            RtpTimestamp::new(timestamp),
            payload_type,
            payload,
        )
    }

    /// Returns the synchronization source.
    #[must_use]
    pub const fn ssrc(&self) -> Ssrc {
        self.ssrc
    }

    /// Returns the wrapping wire sequence.
    #[must_use]
    pub const fn sequence(&self) -> SequenceNumber {
        self.sequence
    }

    /// Returns the wrapping 48 kHz media timestamp.
    #[must_use]
    pub const fn timestamp(&self) -> RtpTimestamp {
        self.timestamp
    }

    /// Returns the validated payload type.
    #[must_use]
    pub const fn payload_type(&self) -> PayloadType {
        self.payload_type
    }

    /// Returns the initialized encoded payload length.
    #[must_use]
    pub const fn payload_len(&self) -> usize {
        self.payload_len as usize
    }

    /// Borrows only initialized encoded payload bytes.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len()]
    }
}

impl fmt::Debug for MediaPacket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MediaPacket")
            .field("ssrc", &self.ssrc)
            .field("sequence", &self.sequence)
            .field("timestamp", &self.timestamp)
            .field("payload_type", &self.payload_type)
            .field("payload", &self.payload())
            .finish()
    }
}

/// Why a raw packet could not become a [`MediaPacket`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketError {
    /// RTP payload types contain only seven bits.
    InvalidPayloadType(u8),
    /// An Opus media packet must contain encoded bytes.
    EmptyPayload,
    /// The encoded payload did not fit fixed inline storage.
    PayloadTooLarge {
        /// Fixed storage maximum.
        maximum: usize,
        /// Rejected payload length.
        actual: usize,
    },
}

impl fmt::Display for PacketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PacketError {}

fn validate_payload_len(len: usize, maximum: usize) -> Result<(), PacketError> {
    if len == 0 {
        Err(PacketError::EmptyPayload)
    } else if len > maximum {
        Err(PacketError::PayloadTooLarge {
            maximum,
            actual: len,
        })
    } else {
        Ok(())
    }
}

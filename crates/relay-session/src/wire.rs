//! Compact native datagram framing for RELAY media and join control.

use relay_audio::{MAX_PACKET_BYTES, MediaPacket};

/// Four-byte magic identifying a RELAY native datagram.
pub const MAGIC: [u8; 4] = *b"RELY";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 4 + 1 + 1;
const MEDIA_PREFIX_LEN: usize = HEADER_LEN + 4 + 2 + 4 + 1 + 2;

/// Kind of a native datagram.
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum WirePacket<'a> {
    /// Peer announcement / Connect hello.
    Hello {
        /// Sender SSRC.
        ssrc: u32,
        /// SHA-256 of the session password, or zeros if the room is open.
        token: [u8; 32],
    },
    /// Hello acknowledgement.
    HelloAck {
        /// Responder SSRC.
        ssrc: u32,
    },
    /// Listener asking a Stream hub for media.
    Subscribe {
        /// Listener SSRC.
        ssrc: u32,
        /// SHA-256 of the session password, or zeros if the room is open.
        token: [u8; 32],
    },
    /// Producer registering with a Stream hub.
    Publish {
        /// Producer SSRC.
        ssrc: u32,
        /// SHA-256 of the session password, or zeros if the room is open.
        token: [u8; 32],
    },
    /// Explicit leave.
    Goodbye {
        /// Departing SSRC.
        ssrc: u32,
    },
    /// One Opus media packet.
    Media {
        /// Encoded media.
        packet: MediaPacket,
    },
    /// LAN name query. Hosts on the well-known UDP port reply with [`Self::Announce`].
    Who {
        /// Querier SSRC.
        ssrc: u32,
        /// Requested session slug.
        name: String,
        /// SHA-256 of the session password, or zeros if the room is open.
        token: [u8; 32],
    },
    /// LAN name advertisement (unicast reply or periodic beacon).
    Announce {
        /// Host SSRC.
        ssrc: u32,
        /// UDP port the host actually bound.
        port: u16,
        /// Session slug.
        name: String,
    },
    /// 16-bit stereo FLAC frame at 48 kHz.
    Flac {
        /// Sender SSRC.
        ssrc: u32,
        /// Packet sequence.
        sequence: u16,
        /// RTP-style 48 kHz timestamp.
        timestamp: u32,
        /// Complete 16-bit FLAC stream for one media frame.
        payload: Vec<u8>,
    },
    /// Uncompressed interleaved stereo i16le PCM at 48 kHz (LAN path).
    Pcm {
        /// Sender SSRC.
        ssrc: u32,
        /// Packet sequence.
        sequence: u16,
        /// RTP-style 48 kHz timestamp.
        timestamp: u32,
        /// Interleaved little-endian i16 samples.
        samples: Vec<u8>,
    },
    /// Marker used only to keep the lifetime in unused parse paths.
    _Reserved(&'a [u8]),
}

/// Why a datagram could not be parsed or encoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireError {
    /// Buffer too small for the declared layout.
    Truncated,
    /// Magic or version did not match.
    Unrecognized,
    /// Kind byte was unknown.
    UnknownKind(u8),
    /// Media payload failed [`MediaPacket`] validation.
    Media,
}

impl core::fmt::Display for WireError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for WireError {}

/// Encodes `packet` into `out`. Returns the written length.
pub fn encode(packet: &WirePacket<'_>, out: &mut [u8]) -> Result<usize, WireError> {
    if out.len() < HEADER_LEN + 4 {
        return Err(WireError::Truncated);
    }
    out[..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    match packet {
        WirePacket::Hello { ssrc, token } => write_authed(out, 1, *ssrc, token),
        WirePacket::HelloAck { ssrc } => write_control(out, 2, *ssrc),
        WirePacket::Subscribe { ssrc, token } => write_authed(out, 3, *ssrc, token),
        WirePacket::Publish { ssrc, token } => write_authed(out, 4, *ssrc, token),
        WirePacket::Goodbye { ssrc } => write_control(out, 5, *ssrc),
        WirePacket::Media { packet } => write_media(out, packet),
        WirePacket::Who { ssrc, name, token } => write_named(out, 7, *ssrc, Some(0), name, token),
        WirePacket::Announce { ssrc, port, name } => {
            write_named(out, 8, *ssrc, Some(*port), name, &[0; 32])
        }
        WirePacket::Flac {
            ssrc,
            sequence,
            timestamp,
            payload,
        } => write_blob(out, 10, *ssrc, *sequence, *timestamp, payload),
        WirePacket::Pcm {
            ssrc,
            sequence,
            timestamp,
            samples,
        } => write_blob(out, 9, *ssrc, *sequence, *timestamp, samples),
        WirePacket::_Reserved(_) => Err(WireError::Unrecognized),
    }
}

/// Parses one datagram.
pub fn decode(bytes: &[u8]) -> Result<WirePacket<'static>, WireError> {
    if bytes.len() < HEADER_LEN {
        return Err(WireError::Truncated);
    }
    if bytes[..4] != MAGIC || bytes[4] != VERSION {
        return Err(WireError::Unrecognized);
    }
    match bytes[5] {
        1 => {
            let (ssrc, token) = read_authed(bytes)?;
            Ok(WirePacket::Hello { ssrc, token })
        }
        2 => Ok(WirePacket::HelloAck {
            ssrc: read_ssrc(bytes)?,
        }),
        3 => {
            let (ssrc, token) = read_authed(bytes)?;
            Ok(WirePacket::Subscribe { ssrc, token })
        }
        4 => {
            let (ssrc, token) = read_authed(bytes)?;
            Ok(WirePacket::Publish { ssrc, token })
        }
        5 => Ok(WirePacket::Goodbye {
            ssrc: read_ssrc(bytes)?,
        }),
        6 => decode_media(bytes),
        7 => {
            decode_named(bytes).map(|(ssrc, _, name, token)| WirePacket::Who { ssrc, name, token })
        }
        8 => decode_named(bytes).map(|(ssrc, port, name, _)| WirePacket::Announce {
            ssrc,
            port,
            name,
        }),
        9 => decode_blob(bytes).map(|(ssrc, sequence, timestamp, samples)| WirePacket::Pcm {
            ssrc,
            sequence,
            timestamp,
            samples,
        }),
        10 => decode_blob(bytes).map(|(ssrc, sequence, timestamp, payload)| WirePacket::Flac {
            ssrc,
            sequence,
            timestamp,
            payload,
        }),
        kind => Err(WireError::UnknownKind(kind)),
    }
}

fn write_control(out: &mut [u8], kind: u8, ssrc: u32) -> Result<usize, WireError> {
    out[5] = kind;
    let end = HEADER_LEN + 4;
    if out.len() < end {
        return Err(WireError::Truncated);
    }
    out[HEADER_LEN..end].copy_from_slice(&ssrc.to_be_bytes());
    Ok(end)
}

fn write_authed(out: &mut [u8], kind: u8, ssrc: u32, token: &[u8; 32]) -> Result<usize, WireError> {
    let end = HEADER_LEN + 4 + 32;
    if out.len() < end {
        return Err(WireError::Truncated);
    }
    out[5] = kind;
    out[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&ssrc.to_be_bytes());
    out[HEADER_LEN + 4..end].copy_from_slice(token);
    Ok(end)
}

fn read_authed(bytes: &[u8]) -> Result<(u32, [u8; 32]), WireError> {
    let ssrc = read_ssrc(bytes)?;
    let mut token = [0_u8; 32];
    if let Some(slice) = bytes.get(HEADER_LEN + 4..HEADER_LEN + 36) {
        token.copy_from_slice(slice);
    }
    Ok((ssrc, token))
}

fn write_media(out: &mut [u8], packet: &MediaPacket) -> Result<usize, WireError> {
    let payload = packet.payload();
    let end = MEDIA_PREFIX_LEN
        .checked_add(payload.len())
        .ok_or(WireError::Truncated)?;
    if out.len() < end || payload.len() > MAX_PACKET_BYTES {
        return Err(WireError::Truncated);
    }
    out[5] = 6;
    let mut cursor = HEADER_LEN;
    out[cursor..cursor + 4].copy_from_slice(&packet.ssrc().get().to_be_bytes());
    cursor += 4;
    out[cursor..cursor + 2].copy_from_slice(&packet.sequence().get().to_be_bytes());
    cursor += 2;
    out[cursor..cursor + 4].copy_from_slice(&packet.timestamp().get().to_be_bytes());
    cursor += 4;
    out[cursor] = packet.payload_type().get();
    cursor += 1;
    let len = u16::try_from(payload.len()).map_err(|_| WireError::Truncated)?;
    out[cursor..cursor + 2].copy_from_slice(&len.to_be_bytes());
    cursor += 2;
    out[cursor..end].copy_from_slice(payload);
    Ok(end)
}

fn read_ssrc(bytes: &[u8]) -> Result<u32, WireError> {
    let end = HEADER_LEN + 4;
    let slice = bytes.get(HEADER_LEN..end).ok_or(WireError::Truncated)?;
    let mut raw = [0_u8; 4];
    raw.copy_from_slice(slice);
    Ok(u32::from_be_bytes(raw))
}

fn write_named(
    out: &mut [u8],
    kind: u8,
    ssrc: u32,
    port: Option<u16>,
    name: &str,
    token: &[u8; 32],
) -> Result<usize, WireError> {
    let slug = name.as_bytes();
    if slug.len() > 48 {
        return Err(WireError::Truncated);
    }
    let name_end = HEADER_LEN + 4 + 2 + 1 + slug.len();
    let end = name_end + 32;
    if out.len() < end {
        return Err(WireError::Truncated);
    }
    out[5] = kind;
    let mut cursor = HEADER_LEN;
    out[cursor..cursor + 4].copy_from_slice(&ssrc.to_be_bytes());
    cursor += 4;
    out[cursor..cursor + 2].copy_from_slice(&port.unwrap_or(0).to_be_bytes());
    cursor += 2;
    out[cursor] = u8::try_from(slug.len()).map_err(|_| WireError::Truncated)?;
    cursor += 1;
    out[cursor..name_end].copy_from_slice(slug);
    out[name_end..end].copy_from_slice(token);
    Ok(end)
}

fn decode_named(bytes: &[u8]) -> Result<(u32, u16, String, [u8; 32]), WireError> {
    let min = HEADER_LEN + 4 + 2 + 1;
    if bytes.len() < min {
        return Err(WireError::Truncated);
    }
    let mut cursor = HEADER_LEN;
    let ssrc = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap_or([0; 4]));
    cursor += 4;
    let port = u16::from_be_bytes(bytes[cursor..cursor + 2].try_into().unwrap_or([0; 2]));
    cursor += 2;
    let len = usize::from(bytes[cursor]);
    cursor += 1;
    let raw = bytes
        .get(cursor..cursor + len)
        .ok_or(WireError::Truncated)?;
    let name = raw
        .iter()
        .copied()
        .map(char::from)
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .take(48)
        .collect();
    cursor += len;
    let mut token = [0_u8; 32];
    if let Some(slice) = bytes.get(cursor..cursor + 32) {
        token.copy_from_slice(slice);
    }
    Ok((ssrc, port, name, token))
}

fn write_blob(
    out: &mut [u8],
    kind: u8,
    ssrc: u32,
    sequence: u16,
    timestamp: u32,
    payload: &[u8],
) -> Result<usize, WireError> {
    let end = HEADER_LEN
        .checked_add(4 + 2 + 4 + 2)
        .and_then(|prefix| prefix.checked_add(payload.len()))
        .ok_or(WireError::Truncated)?;
    if out.len() < end || payload.len() > MAX_PACKET_BYTES {
        return Err(WireError::Truncated);
    }
    out[5] = kind;
    let mut cursor = HEADER_LEN;
    out[cursor..cursor + 4].copy_from_slice(&ssrc.to_be_bytes());
    cursor += 4;
    out[cursor..cursor + 2].copy_from_slice(&sequence.to_be_bytes());
    cursor += 2;
    out[cursor..cursor + 4].copy_from_slice(&timestamp.to_be_bytes());
    cursor += 4;
    let len = u16::try_from(payload.len()).map_err(|_| WireError::Truncated)?;
    out[cursor..cursor + 2].copy_from_slice(&len.to_be_bytes());
    cursor += 2;
    out[cursor..end].copy_from_slice(payload);
    Ok(end)
}

fn decode_blob(bytes: &[u8]) -> Result<(u32, u16, u32, Vec<u8>), WireError> {
    let min = HEADER_LEN + 4 + 2 + 4 + 2;
    if bytes.len() < min {
        return Err(WireError::Truncated);
    }
    let mut cursor = HEADER_LEN;
    let ssrc = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap_or([0; 4]));
    cursor += 4;
    let sequence = u16::from_be_bytes(bytes[cursor..cursor + 2].try_into().unwrap_or([0; 2]));
    cursor += 2;
    let timestamp = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap_or([0; 4]));
    cursor += 4;
    let len = u16::from_be_bytes(bytes[cursor..cursor + 2].try_into().unwrap_or([0; 2])) as usize;
    cursor += 2;
    let payload = bytes
        .get(cursor..cursor + len)
        .ok_or(WireError::Truncated)?
        .to_vec();
    if payload.len() < 4 {
        return Err(WireError::Media);
    }
    Ok((ssrc, sequence, timestamp, payload))
}

fn decode_media(bytes: &[u8]) -> Result<WirePacket<'static>, WireError> {
    if bytes.len() < MEDIA_PREFIX_LEN {
        return Err(WireError::Truncated);
    }
    let mut cursor = HEADER_LEN;
    let ssrc = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().expect("4"));
    cursor += 4;
    let sequence = u16::from_be_bytes(bytes[cursor..cursor + 2].try_into().expect("2"));
    cursor += 2;
    let timestamp = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().expect("4"));
    cursor += 4;
    let payload_type = bytes[cursor];
    cursor += 1;
    let len = u16::from_be_bytes(bytes[cursor..cursor + 2].try_into().expect("2")) as usize;
    cursor += 2;
    let payload = bytes
        .get(cursor..cursor + len)
        .ok_or(WireError::Truncated)?;
    let packet = MediaPacket::try_new(ssrc, sequence, timestamp, payload_type, payload)
        .map_err(|_| WireError::Media)?;
    Ok(WirePacket::Media { packet })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_round_trips() {
        let packet = MediaPacket::try_new(7, 9, 48, 111, &[1, 2, 3, 4]).expect("packet");
        let mut buffer = [0_u8; 64];
        let written = encode(
            &WirePacket::Media {
                packet: packet.clone(),
            },
            &mut buffer,
        )
        .expect("encode");
        match decode(&buffer[..written]).expect("decode") {
            WirePacket::Media { packet: decoded } => {
                assert_eq!(decoded.ssrc(), packet.ssrc());
                assert_eq!(decoded.sequence(), packet.sequence());
                assert_eq!(decoded.timestamp(), packet.timestamp());
                assert_eq!(decoded.payload(), packet.payload());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn hello_carries_password_token() {
        let mut buffer = [0_u8; 64];
        let token = [9_u8; 32];
        let written = encode(&WirePacket::Hello { ssrc: 3, token }, &mut buffer).expect("hello");
        match decode(&buffer[..written]).expect("decode hello") {
            WirePacket::Hello {
                ssrc,
                token: decoded,
            } => {
                assert_eq!(ssrc, 3);
                assert_eq!(decoded, token);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn pcm_and_who_round_trip() {
        let mut buffer = [0_u8; 64];
        let written = encode(
            &WirePacket::Who {
                ssrc: 9,
                name: "late-night-mix".into(),
                token: [7; 32],
            },
            &mut buffer,
        )
        .expect("who");
        match decode(&buffer[..written]).expect("decode who") {
            WirePacket::Who { ssrc, name, token } => {
                assert_eq!(ssrc, 9);
                assert_eq!(name, "late-night-mix");
                assert_eq!(token, [7; 32]);
            }
            other => panic!("{other:?}"),
        }
        let written = encode(
            &WirePacket::Pcm {
                ssrc: 1,
                sequence: 2,
                timestamp: 240,
                samples: vec![0, 1, 2, 3],
            },
            &mut buffer,
        )
        .expect("pcm");
        match decode(&buffer[..written]).expect("decode pcm") {
            WirePacket::Pcm {
                ssrc,
                sequence,
                timestamp,
                samples,
            } => {
                assert_eq!((ssrc, sequence, timestamp), (1, 2, 240));
                assert_eq!(samples, vec![0, 1, 2, 3]);
            }
            other => panic!("{other:?}"),
        }
    }
}

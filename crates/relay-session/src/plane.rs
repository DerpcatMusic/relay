//! Non-blocking UDP plane for Connect peers and a local Stream hub.

use std::collections::BTreeSet;
use std::io::{self, ErrorKind};
use std::net::{SocketAddr, UdpSocket};

use relay_audio::MediaPacket;

use crate::wire::{self, WireError, WirePacket};

/// Role of the local UDP socket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketRole {
    /// Idle; no socket is bound.
    Idle,
    /// Bidirectional Connect peer.
    Connect,
    /// One-to-many Stream hub (local, unpaid).
    StreamHub,
    /// Stream producer sending to a hub.
    StreamProducer,
    /// Stream listener receiving from a hub.
    StreamListener,
}

/// Plane I/O or protocol failure.
#[derive(Debug)]
pub enum PlaneError {
    /// Socket bind/connect failed.
    Io(io::Error),
    /// Framing failed.
    Wire(WireError),
    /// Operation is invalid for the current role.
    InvalidRole,
    /// No socket is bound.
    NotBound,
}

impl core::fmt::Display for PlaneError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PlaneError {}

impl From<io::Error> for PlaneError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// One inbound datagram after decode.
#[derive(Debug)]
pub struct Inbound {
    /// Sender address.
    pub from: SocketAddr,
    /// Decoded packet.
    pub packet: WirePacket<'static>,
}

/// Bound UDP socket plus the current peer/listener set.
#[derive(Debug)]
pub struct NativePlane {
    socket: Option<UdpSocket>,
    role: SocketRole,
    local_ssrc: u32,
    auth: [u8; 32],
    peers: BTreeSet<SocketAddr>,
    encode_buf: [u8; 4_096],
    recv_buf: [u8; 4_096],
}

impl NativePlane {
    /// Creates an unbound plane.
    #[must_use]
    pub fn new(local_ssrc: u32) -> Self {
        Self {
            socket: None,
            role: SocketRole::Idle,
            local_ssrc,
            auth: [0; 32],
            peers: BTreeSet::new(),
            encode_buf: [0; 4_096],
            recv_buf: [0; 4_096],
        }
    }

    /// Sets the password token sent with Hello / Who / Subscribe / Publish.
    pub fn set_auth(&mut self, token: [u8; 32]) {
        self.auth = token;
    }

    /// Returns the current role.
    #[must_use]
    pub const fn role(&self) -> SocketRole {
        self.role
    }

    /// Returns the bound local address, if any.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.socket
            .as_ref()
            .and_then(|socket| socket.local_addr().ok())
    }

    /// Known destinations that receive media.
    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Binds a non-blocking UDP socket.
    pub fn bind(&mut self, addr: SocketAddr, role: SocketRole) -> Result<SocketAddr, PlaneError> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        let _ = socket.set_broadcast(true);
        let local = socket.local_addr()?;
        self.socket = Some(socket);
        self.role = role;
        self.peers.clear();
        Ok(local)
    }

    /// Adds a Connect or Stream destination and announces this peer.
    pub fn add_peer(&mut self, peer: SocketAddr) -> Result<(), PlaneError> {
        if self.socket.is_none() {
            return Err(PlaneError::NotBound);
        }
        self.peers.insert(peer);
        let hello = match self.role {
            SocketRole::Connect => WirePacket::Hello {
                ssrc: self.local_ssrc,
                token: self.auth,
            },
            SocketRole::StreamProducer => WirePacket::Publish {
                ssrc: self.local_ssrc,
                token: self.auth,
            },
            SocketRole::StreamListener => WirePacket::Subscribe {
                ssrc: self.local_ssrc,
                token: self.auth,
            },
            SocketRole::StreamHub | SocketRole::Idle => return Err(PlaneError::InvalidRole),
        };
        self.send_to(peer, &hello)
    }

    /// Records a remote without sending (used by the hub).
    pub fn remember(&mut self, peer: SocketAddr) {
        self.peers.insert(peer);
    }

    /// Forgets a remote.
    pub fn forget(&mut self, peer: SocketAddr) {
        self.peers.remove(&peer);
    }

    /// Sends one media packet to every current destination.
    pub fn send_media(&mut self, packet: &MediaPacket) -> Result<usize, PlaneError> {
        self.forward_media(packet, None)
    }

    /// Forwards media to every destination except `except`.
    pub fn forward_media(
        &mut self,
        packet: &MediaPacket,
        except: Option<SocketAddr>,
    ) -> Result<usize, PlaneError> {
        let framed = WirePacket::Media {
            packet: packet.clone(),
        };
        let mut sent = 0;
        let destinations: Vec<SocketAddr> = self.peers.iter().copied().collect();
        for dest in destinations {
            if Some(dest) == except {
                continue;
            }
            self.send_to(dest, &framed)?;
            sent += 1;
        }
        Ok(sent)
    }

    /// Forwards an already-framed packet to every destination except `except`.
    pub fn forward_wire(
        &mut self,
        packet: &WirePacket<'_>,
        except: Option<SocketAddr>,
    ) -> Result<usize, PlaneError> {
        let mut sent = 0;
        let destinations: Vec<SocketAddr> = self.peers.iter().copied().collect();
        for dest in destinations {
            if Some(dest) == except {
                continue;
            }
            self.send_to(dest, packet)?;
            sent += 1;
        }
        Ok(sent)
    }

    /// Sends `packet` to every well-known LAN discovery address.
    pub fn send_discovery(
        &mut self,
        packet: &WirePacket<'_>,
        extra: Option<u16>,
    ) -> Result<(), PlaneError> {
        for dest in discovery_addrs(extra) {
            let _ = self.send_to(dest, packet);
        }
        Ok(())
    }

    /// Sends a control or media packet to one destination.
    pub fn send_to(&mut self, dest: SocketAddr, packet: &WirePacket<'_>) -> Result<(), PlaneError> {
        let socket = self.socket.as_ref().ok_or(PlaneError::NotBound)?;
        let written = wire::encode(packet, &mut self.encode_buf).map_err(PlaneError::Wire)?;
        socket.send_to(&self.encode_buf[..written], dest)?;
        Ok(())
    }

    /// Receives one pending datagram, if any.
    pub fn recv(&mut self) -> Result<Option<Inbound>, PlaneError> {
        let socket = self.socket.as_ref().ok_or(PlaneError::NotBound)?;
        match socket.recv_from(&mut self.recv_buf) {
            Ok((len, from)) => {
                let packet = wire::decode(&self.recv_buf[..len]).map_err(PlaneError::Wire)?;
                Ok(Some(Inbound { from, packet }))
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(PlaneError::Io(error)),
        }
    }
}

fn discovery_addrs(extra: Option<u16>) -> Vec<SocketAddr> {
    let port = crate::DEFAULT_CONNECT_PORT;
    let mut addrs = vec![
        SocketAddr::from(([255, 255, 255, 255], port)),
        SocketAddr::from(([127, 0, 0, 1], port)),
    ];
    if let Some(extra) = extra
        && extra != 0
        && extra != port
    {
        addrs.push(SocketAddr::from(([255, 255, 255, 255], extra)));
        addrs.push(SocketAddr::from(([127, 0, 0, 1], extra)));
    }
    addrs
}

//! Plugin → browser WebRTC. Cloudflare only sees signaling.
//!
//! Off-LAN listeners get a sendonly Opus audio track (native
//! `RTCPeerConnection` → `MediaStream` → `<audio>`). The native stack is
//! libdatachannel behind [`relay_transport`]. STUN-only; juice has no TURN/TLS.

use std::collections::HashMap;
use std::task::{Context, Poll, Waker};

use relay_opus::{
    Bitrate, Encoder, EncoderConfigV1, EncoderPolicyV1, FrameDuration, InbandFec, MAX_PACKET_BYTES,
    PacketLossPercent,
};
use relay_transport::{
    BinaryPayload, ChannelId, Command, Event, IceCandidate, NativeTransportProvider,
    NegotiationEpoch, OperationId, PeerDriver, PeerState, SessionDescription, TransportError,
};
use relay_transport_libdatachannel::{LibdatachannelProvider, drain_ready, listen_offerer_config};

pub const MAX_PEERS: usize = 10;
const CHANNEL: ChannelId = ChannelId(0);
const EPOCH: NegotiationEpoch = NegotiationEpoch(1);

pub struct Hub {
    provider: LibdatachannelProvider,
    peers: HashMap<String, Peer>,
    encoder: Option<Encoder>,
    bitrate_kbps: u32,
    packet: Vec<u8>,
    leftover: Vec<f32>,
    frames_sent: u64,
    last_peak: f32,
}

/// 10 ms of 48 kHz stereo, interleaved.
const FRAME_SAMPLES: usize = 960;

struct Peer {
    driver: Box<dyn PeerDriver>,
    next_op: u64,
    ready: bool,
    dead: bool,
    answered: bool,
    offer_sdp: Option<String>,
    pending_ice: Vec<(String, Option<String>)>,
}

impl Hub {
    pub fn new() -> Self {
        Self {
            provider: LibdatachannelProvider::new(),
            peers: HashMap::new(),
            encoder: None,
            bitrate_kbps: 192,
            packet: vec![0; MAX_PACKET_BYTES],
            leftover: Vec::new(),
            frames_sent: 0,
            last_peak: 0.0,
        }
    }

    pub fn peer_count(&self) -> u32 {
        u32::try_from(self.peers.len()).unwrap_or(u32::MAX)
    }

    pub fn ready_count(&self) -> u32 {
        u32::try_from(
            self.peers
                .values()
                .filter(|peer| peer.ready && !peer.dead)
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    pub fn frames_sent(&self) -> u64 {
        self.frames_sent
    }

    pub fn last_peak(&self) -> f32 {
        self.last_peak
    }

    pub fn clear(&mut self) {
        for peer in self.peers.values_mut() {
            peer.shutdown();
        }
        self.peers.clear();
        self.leftover.clear();
        self.encoder = None;
        self.frames_sent = 0;
        self.last_peak = 0.0;
    }

    pub fn apply_signal(&mut self, raw: &str, outgoing: &mut Vec<String>) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
            return;
        };
        let t = value.get("t").and_then(|v| v.as_str()).unwrap_or("");
        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        if id.is_empty() {
            return;
        }
        match t {
            "want" => self.want(&id, outgoing),
            "answer" => {
                if let Some(sdp) = value.get("sdp").and_then(|v| v.as_str()) {
                    self.answer(&id, sdp);
                }
            }
            "ice" => {
                if let Some(cand) = value.get("cand").and_then(|v| v.as_str()) {
                    let mid = value
                        .get("mid")
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned);
                    self.remote_ice(&id, cand, mid);
                }
            }
            "bye" => self.drop_peer(&id),
            _ => {}
        }
    }

    /// Drop byes first so a reconnect `bye` + `want` for the same id
    /// creates a fresh peer instead of creating then immediately deleting it.
    pub fn apply_all(&mut self, raws: &[String], outgoing: &mut Vec<String>) {
        for raw in raws {
            if raw.contains("\"t\":\"bye\"") {
                self.apply_signal(raw, outgoing);
            }
        }
        for raw in raws {
            if !raw.contains("\"t\":\"bye\"") {
                self.apply_signal(raw, outgoing);
            }
        }
    }

    pub fn push_pcm(&mut self, pcm: &[f32], bitrate_kbps: u32) {
        let mut peak = 0.0_f32;
        for sample in pcm {
            peak = peak.max(sample.abs());
        }
        self.last_peak = peak;
        if self.peers.is_empty() {
            self.leftover.clear();
            return;
        }
        if self.encoder.is_none() || self.bitrate_kbps != bitrate_kbps {
            self.encoder = make_encoder(bitrate_kbps);
            self.bitrate_kbps = bitrate_kbps;
        }
        if self.encoder.is_none() {
            return;
        }
        self.leftover.extend_from_slice(pcm);
        while self.leftover.len() >= FRAME_SAMPLES {
            let frame: Vec<f32> = self.leftover.drain(..FRAME_SAMPLES).collect();
            self.send_frame(&frame);
        }
    }

    pub fn drive(&mut self, outgoing: &mut Vec<String>) {
        let ids: Vec<String> = self.peers.keys().cloned().collect();
        for id in ids {
            let Some(peer) = self.peers.get_mut(&id) else {
                continue;
            };
            for event in drain_ready(peer.driver.as_mut()) {
                match event {
                    Event::LocalDescription { description } => {
                        peer.offer_sdp = Some(description.sdp().to_owned());
                        outgoing.push(signal_sdp("offer", &id, description.sdp()));
                    }
                    Event::LocalCandidate { candidate } => {
                        outgoing.push(signal_ice(&id, &candidate));
                    }
                    Event::DataChannelOpened { .. }
                    | Event::StateChanged {
                        state: PeerState::Connected,
                    } => {
                        peer.ready = true;
                    }
                    Event::DataChannelClosed { .. }
                    | Event::FatalError { .. }
                    | Event::ShutdownComplete => {
                        peer.dead = true;
                    }
                    Event::StateChanged {
                        state: PeerState::Failed | PeerState::Closed,
                    } => {
                        peer.dead = true;
                    }
                    _ => {}
                }
            }
            if peer.dead {
                peer.shutdown();
                self.peers.remove(&id);
            }
        }
    }

    fn drop_peer(&mut self, id: &str) {
        if let Some(mut peer) = self.peers.remove(id) {
            peer.shutdown();
        }
    }

    fn want(&mut self, id: &str, outgoing: &mut Vec<String>) {
        if let Some(peer) = self.peers.get(id)
            && !peer.dead
        {
            if let Some(sdp) = &peer.offer_sdp {
                outgoing.push(signal_sdp("offer", id, sdp));
            }
            return;
        }
        self.drop_peer(id);
        if self.peers.len() >= MAX_PEERS {
            let spare = self
                .peers
                .iter()
                .find(|(_, peer)| !peer.ready || peer.dead)
                .map(|(key, _)| key.clone());
            if let Some(old) = spare {
                self.drop_peer(&old);
            }
        }
        if self.peers.len() >= MAX_PEERS {
            outgoing.push(signal_bye(id));
            return;
        }
        let Ok(config) = listen_offerer_config() else {
            return;
        };
        let Ok(validated) = config.validate_for(self.provider.capabilities()) else {
            return;
        };
        let Ok(driver) = self.provider.create_peer(validated) else {
            return;
        };
        let mut peer = Peer {
            driver,
            next_op: 0,
            ready: false,
            dead: false,
            answered: false,
            offer_sdp: None,
            pending_ice: Vec::new(),
        };
        if peer
            .submit(|operation_id| Command::OpenDataChannel {
                operation_id,
                channel_id: CHANNEL,
            })
            .is_err()
        {
            return;
        }
        if peer
            .submit(|operation_id| Command::CreateOffer {
                operation_id,
                epoch: EPOCH,
            })
            .is_err()
        {
            return;
        }
        self.peers.insert(id.to_owned(), peer);
    }

    fn send_frame(&mut self, frame: &[f32]) {
        let Some(encoder) = self.encoder.as_mut() else {
            return;
        };
        let Ok(n) = encoder.encode(frame, &mut self.packet) else {
            return;
        };
        let packet = self.packet[..n].to_vec();
        let ids: Vec<String> = self.peers.keys().cloned().collect();
        for id in ids {
            let Some(peer) = self.peers.get_mut(&id) else {
                continue;
            };
            if peer.dead || !peer.ready {
                continue;
            }
            let Ok(payload) = BinaryPayload::new(packet.clone()) else {
                continue;
            };
            match peer.submit(|operation_id| Command::Send {
                operation_id,
                channel_id: CHANNEL,
                payload,
            }) {
                Ok(()) => self.frames_sent = self.frames_sent.saturating_add(1),
                Err(TransportError::WouldBlock | TransportError::InvalidState) => {}
                Err(_) => peer.dead = true,
            }
        }
    }

    fn answer(&mut self, id: &str, sdp: &str) {
        let Some(peer) = self.peers.get_mut(id) else {
            return;
        };
        let Ok(description) = SessionDescription::new(
            EPOCH,
            relay_transport::DescriptionKind::Answer,
            sdp.to_owned(),
        ) else {
            peer.dead = true;
            return;
        };
        if peer
            .submit(|operation_id| Command::SetRemoteDescription {
                operation_id,
                description,
            })
            .is_err()
        {
            peer.dead = true;
            return;
        }
        peer.answered = true;
        let queued = std::mem::take(&mut peer.pending_ice);
        for (cand, mid) in queued {
            if !usable_ice(&cand) {
                continue;
            }
            let Ok(candidate) = IceCandidate::new(EPOCH, cand, mid, Some(0), None) else {
                continue;
            };
            let _ = peer.submit(|operation_id| Command::AddRemoteCandidate {
                operation_id,
                candidate,
            });
        }
    }

    fn remote_ice(&mut self, id: &str, cand: &str, mid: Option<String>) {
        if !usable_ice(cand) {
            return;
        }
        let Some(peer) = self.peers.get_mut(id) else {
            return;
        };
        if !peer.answered {
            peer.pending_ice.push((cand.to_owned(), mid));
            return;
        }
        let Ok(candidate) = IceCandidate::new(EPOCH, cand.to_owned(), mid, Some(0), None) else {
            return;
        };
        let _ = peer.submit(|operation_id| Command::AddRemoteCandidate {
            operation_id,
            candidate,
        });
    }
}

impl Peer {
    fn submit(&mut self, make: impl FnOnce(OperationId) -> Command) -> Result<(), TransportError> {
        self.next_op = self.next_op.saturating_add(1);
        let command = make(OperationId(self.next_op));
        self.driver.submit(command).map_err(|error| error.error())
    }

    fn shutdown(&mut self) {
        if self.dead && self.next_op == u64::MAX {
            return;
        }
        self.next_op = self.next_op.saturating_add(1);
        let _ = self.driver.submit(Command::Shutdown {
            operation_id: OperationId(self.next_op),
        });
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        while let Poll::Ready(Some(event)) = self.driver.poll_event(&mut context) {
            if matches!(event, Event::ShutdownComplete) {
                break;
            }
        }
    }
}

fn make_encoder(bitrate_kbps: u32) -> Option<Encoder> {
    let bps = i32::try_from(bitrate_kbps.clamp(64, 256).saturating_mul(1_000)).ok()?;
    let bitrate = Bitrate::try_new(bps).ok()?;
    let policy = EncoderPolicyV1::new(bitrate, InbandFec::Enabled, PacketLossPercent::ZERO);
    Encoder::new(EncoderConfigV1::stereo_48k(FrameDuration::Ms10, policy)).ok()
}

fn signal_sdp(kind: &str, id: &str, sdp: &str) -> String {
    serde_json::json!({ "t": kind, "id": id, "sdp": sdp }).to_string()
}

fn signal_ice(id: &str, candidate: &IceCandidate) -> String {
    serde_json::json!({
        "t": "ice",
        "id": id,
        "cand": candidate.candidate(),
        "mid": candidate.sdp_mid(),
    })
    .to_string()
}

fn signal_bye(id: &str) -> String {
    serde_json::json!({ "t": "bye", "id": id }).to_string()
}

fn usable_ice(cand: &str) -> bool {
    let text = cand.trim();
    !text.is_empty() && text != "candidate:" && text != "a=candidate:"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_caps_at_ten() {
        assert_eq!(MAX_PEERS, 10);
        assert_eq!(FRAME_SAMPLES, 960);
    }

    #[test]
    fn signal_json_is_object() {
        let json = signal_sdp("offer", "ab", "v=0");
        assert!(json.contains("\"t\":\"offer\""));
        assert!(json.contains("\"id\":\"ab\""));
        assert!(json.contains("v=0"));
    }

    #[test]
    fn ice_before_answer_is_held() {
        let mut hub = Hub::new();
        let mut outgoing = Vec::new();
        hub.apply_signal(
            r#"{"t":"ice","id":"ab","cand":"candidate:1 1 UDP 1 127.0.0.1 9 typ host"}"#,
            &mut outgoing,
        );
        assert!(outgoing.is_empty());
        assert!(hub.peers.is_empty());
    }

    #[test]
    fn empty_end_of_candidates_is_not_usable() {
        assert!(!usable_ice(""));
        assert!(!usable_ice("   "));
        assert!(!usable_ice("candidate:"));
        assert!(usable_ice("candidate:1 1 UDP 1 127.0.0.1 9 typ host"));
    }

    fn wait_offer(hub: &mut Hub) -> Vec<String> {
        let start = std::time::Instant::now();
        let mut outgoing = Vec::new();
        while start.elapsed() < std::time::Duration::from_secs(5) {
            hub.drive(&mut outgoing);
            if outgoing.iter().any(|msg| msg.contains("\"t\":\"offer\"")) {
                return outgoing;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        outgoing
    }

    fn offer_sdp(msgs: &[String]) -> Option<String> {
        for msg in msgs {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(msg) else {
                continue;
            };
            if value.get("t").and_then(|v| v.as_str()) == Some("offer") {
                return value.get("sdp").and_then(|v| v.as_str()).map(str::to_owned);
            }
        }
        None
    }

    #[test]
    fn want_on_existing_id_keeps_peer() {
        let mut hub = Hub::new();
        let mut outgoing = Vec::new();
        hub.apply_signal(r#"{"t":"want","id":"ab"}"#, &mut outgoing);
        assert_eq!(
            hub.peer_count(),
            1,
            "libdatachannel must create a peer from want"
        );
        let first = wait_offer(&mut hub);
        let sdp = offer_sdp(&first).expect("first want must emit an offer");
        outgoing.clear();
        hub.apply_signal(r#"{"t":"want","id":"ab"}"#, &mut outgoing);
        assert_eq!(hub.peer_count(), 1);
        assert_eq!(
            offer_sdp(&outgoing).as_deref(),
            Some(sdp.as_str()),
            "duplicate want must re-send the same offer, not a new peer: {outgoing:?}"
        );
    }

    #[test]
    fn bye_then_want_creates_a_fresh_peer() {
        let mut hub = Hub::new();
        let mut outgoing = Vec::new();
        hub.apply_signal(r#"{"t":"want","id":"ab"}"#, &mut outgoing);
        let first = wait_offer(&mut hub);
        let first_sdp = offer_sdp(&first).expect("first offer");
        outgoing.clear();
        hub.apply_all(
            &[
                r#"{"t":"bye","id":"ab"}"#.to_owned(),
                r#"{"t":"want","id":"ab"}"#.to_owned(),
            ],
            &mut outgoing,
        );
        assert_eq!(hub.peer_count(), 1);
        let second = wait_offer(&mut hub);
        let second_sdp = offer_sdp(&second).expect("bye+want must emit a new offer");
        assert_ne!(
            first_sdp, second_sdp,
            "bye must drop the old ICE credentials"
        );
    }

    #[test]
    fn apply_all_processes_bye_before_want() {
        let mut hub = Hub::new();
        let mut outgoing = Vec::new();
        hub.apply_all(
            &[
                r#"{"t":"want","id":"ab"}"#.to_owned(),
                r#"{"t":"bye","id":"ab"}"#.to_owned(),
            ],
            &mut outgoing,
        );
        assert_eq!(
            hub.peer_count(),
            1,
            "bye then want on the same id must leave a peer"
        );
    }
}

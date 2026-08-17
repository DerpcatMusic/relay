//! Off-callback session engine shared by standalone Connect, Stream, and the plugin.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use relay_audio::{
    AdaptiveClockConfig, AudioPipelineConfig, AudioPipelineConfigInput, Bitrate,
    ClockRecoveryConfig, EncoderPolicyV1, ExtendedSequence, ExtendedTimestamp, FrameDuration,
    InbandFec, IngressStatus, MAX_PACKET_BYTES, PacketBatch, PacketLossPercent, PayloadType,
    PcmFrame, PlaybackConfig, PlaybackProcessError, PlaybackRenderer, PlaybackWorker, RenderReport,
    RtpTimestamp, RxStreamConfig, RxWorker, SequenceNumber, Ssrc, TxProcessOutcome, TxStreamConfig,
    TxWorker, playback_pair,
};
use relay_domain::{ConnectionState, MediaRoute, SessionMode};
use relay_rt::{AudioConsumer, AudioProducer, WriteOutcome, audio_ring};

use crate::plane::{NativePlane, PlaneError, SocketRole};
use crate::wire::WirePacket;

const PAYLOAD_TYPE: u8 = 111;
const CHANNELS: usize = 2;

/// How the host callback mixes local and remote audio.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonitorMode {
    /// Host input is copied to output; remote is ignored in the callback.
    Dry,
    /// Only remote playback is written to the output.
    Remote,
    /// Dry plus remote, clipped to finite samples.
    Mix,
}

/// Construction settings for [`SessionEngine`].
#[derive(Clone, Copy, Debug)]
pub struct SessionConfig {
    /// Product surface.
    pub mode: SessionMode,
    /// Capture / playback device rate.
    pub device_rate_hz: usize,
    /// Negotiated Opus duration.
    pub frame_duration: FrameDuration,
    /// Local SSRC.
    pub ssrc: u32,
    /// Callback mix policy.
    pub monitor: MonitorMode,
    /// Home-LAN path: 5 ms uncompressed PCM, no Opus/FEC lookahead.
    pub lan: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            mode: SessionMode::Connect,
            device_rate_hz: 48_000,
            frame_duration: FrameDuration::Ms5,
            ssrc: 0x5245_4c59,
            monitor: MonitorMode::Dry,
            lan: true,
        }
    }
}

/// Off-thread command processed by [`SessionEngine::drive`].
#[derive(Clone, Copy, Debug)]
pub enum EngineCommand {
    /// Bind a Connect socket.
    Listen(SocketAddr),
    /// Join a Connect peer.
    Join(SocketAddr),
    /// Bind a local unpaid Stream hub.
    HostStream(SocketAddr),
    /// Send to a Stream hub as the producer.
    PublishStream(SocketAddr),
    /// Receive from a Stream hub.
    ListenStream(SocketAddr),
    /// Send to and receive from this process over localhost UDP.
    Loopback,
    /// Store the shareable LAN session slug used for Who/Announce.
    SetSlug {
        /// UTF-8 slug bytes.
        bytes: [u8; 48],
        /// Occupied prefix of `bytes`.
        len: u8,
    },
    /// Bind an ephemeral socket and discover a LAN host by slug.
    JoinLan {
        /// UTF-8 slug bytes.
        bytes: [u8; 48],
        /// Occupied prefix of `bytes`.
        len: u8,
    },
    /// Drop peers and close the socket role.
    Disconnect,
}

/// Immutable view published off the callback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionSnapshot {
    /// Product mode.
    pub mode: SessionMode,
    /// Lifecycle.
    pub state: ConnectionState,
    /// Media path currently in use.
    pub route: MediaRoute,
    /// Known destinations.
    pub peers: usize,
    /// Bound UDP port, if any.
    pub local_port: Option<u16>,
}

/// One worker-side drive result.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DriveReport {
    /// Datagrams parsed this drive.
    pub received: u64,
    /// Media packets sent this drive.
    pub sent: u64,
    /// RX frames published to playback.
    pub decoded_frames: u64,
}

/// Why a session engine could not be constructed.
#[derive(Debug)]
pub enum EngineBuildError {
    /// Pipeline validation failed.
    Config,
    /// TX construction failed.
    Tx,
    /// RX construction failed.
    Rx,
    /// Playback construction failed.
    Playback,
    /// Ring construction failed.
    Ring,
}

impl core::fmt::Display for EngineBuildError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for EngineBuildError {}

/// Audio-thread face: rings and renderer only.
pub struct CallbackFace {
    capture_tx: AudioProducer,
    renderer: PlaybackRenderer,
    staging: Vec<f32>,
    staging_filled: usize,
    monitor: MonitorMode,
}

/// Worker-thread owner of sockets, codec, and playback publication.
pub struct SessionWorker {
    config: SessionConfig,
    pipeline: AudioPipelineConfig,
    state: ConnectionState,
    route: MediaRoute,
    plane: NativePlane,
    capture_rx: AudioConsumer,
    playback: PlaybackWorker,
    tx: TxWorker,
    rx: RxWorker,
    batch: PacketBatch,
    capture_chunk: Box<[f32]>,
    scheduled_local: u64,
    remote_ssrc: Option<u32>,
    slug: [u8; 48],
    slug_len: u8,
    want_slug: [u8; 48],
    want_slug_len: u8,
    codec: crate::WireCodec,
    flac_level: u8,
    password_hash: [u8; 32],
    pcm_seq: u16,
    pcm_ts: u32,
    pcm_bytes: Vec<u8>,
    pcm_decode: Vec<f32>,
    web_pcm: Vec<f32>,
    web_pcm_seq: u64,
    lan_frame: PcmFrame,
    last_who: Instant,
}

/// Connect / Stream engine. Callback methods never touch the socket.
pub struct SessionEngine {
    callback: CallbackFace,
    worker: SessionWorker,
}

impl SessionEngine {
    /// Preallocates codec, rings, and workers. Does not bind a socket.
    pub fn prepare(mut config: SessionConfig) -> Result<Self, EngineBuildError> {
        if config.lan {
            config.frame_duration = FrameDuration::Ms5;
        }
        let pipeline = pipeline_for(config.device_rate_hz, config.frame_duration)
            .map_err(|_| EngineBuildError::Config)?;
        let policy = EncoderPolicyV1::new(
            Bitrate::try_new(192_000).map_err(|_| EngineBuildError::Tx)?,
            InbandFec::Enabled,
            PacketLossPercent::try_new(5).map_err(|_| EngineBuildError::Tx)?,
        );
        let tx = TxWorker::new(
            pipeline,
            TxStreamConfig {
                ssrc: Ssrc::new(config.ssrc),
                payload_type: PayloadType::new(PAYLOAD_TYPE).map_err(|_| EngineBuildError::Tx)?,
                initial_sequence: SequenceNumber::new(1),
                initial_timestamp: RtpTimestamp::new(0),
                encoding_policy: policy,
            },
        )
        .map_err(|_| EngineBuildError::Tx)?;
        let rx = RxWorker::new(
            pipeline,
            RxStreamConfig {
                ssrc: Ssrc::new(0),
                payload_type: PayloadType::new(PAYLOAD_TYPE).map_err(|_| EngineBuildError::Rx)?,
                initial_sequence: ExtendedSequence::new(0),
                initial_timestamp: RtpTimestamp::new(0),
            },
        )
        .map_err(|_| EngineBuildError::Rx)?;
        let (playback, renderer, _metrics) =
            playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline))
                .map_err(|_| EngineBuildError::Playback)?;
        let chunk_samples = tx.capture_chunk_samples();
        let media_pcm_samples = tx.media_pcm_frame_samples();
        let (capture_tx, capture_rx, _capture_metrics) =
            audio_ring(pipeline.capture_ring_samples()).map_err(|_| EngineBuildError::Ring)?;
        let batch = PacketBatch::new(8).map_err(|_| EngineBuildError::Tx)?;
        let mut capture_chunk = Vec::new();
        capture_chunk
            .try_reserve_exact(chunk_samples)
            .map_err(|_| EngineBuildError::Ring)?;
        capture_chunk.resize(chunk_samples, 0.0);
        Ok(Self {
            callback: CallbackFace {
                capture_tx,
                renderer,
                staging: vec![0.0; chunk_samples],
                staging_filled: 0,
                monitor: config.monitor,
            },
            worker: SessionWorker {
                config,
                pipeline,
                state: ConnectionState::Idle,
                route: MediaRoute::Direct,
                plane: NativePlane::new(config.ssrc),
                capture_rx,
                playback,
                tx,
                rx,
                batch,
                capture_chunk: capture_chunk.into_boxed_slice(),
                scheduled_local: 0,
                remote_ssrc: None,
                slug: [0; 48],
                slug_len: 0,
                want_slug: [0; 48],
                want_slug_len: 0,
                codec: if config.lan {
                    crate::WireCodec::Pcm
                } else {
                    crate::WireCodec::Opus
                },
                flac_level: 5,
                password_hash: [0; 32],
                pcm_seq: 1,
                pcm_ts: 0,
                pcm_bytes: vec![0; media_pcm_samples.saturating_mul(2)],
                pcm_decode: vec![0.0; media_pcm_samples],
                web_pcm: Vec::new(),
                web_pcm_seq: 0,
                lan_frame: PcmFrame::empty(),
                last_who: Instant::now()
                    .checked_sub(Duration::from_secs(1))
                    .unwrap_or_else(Instant::now),
            },
        })
    }

    /// Splits the audio-thread face from the worker for a plugin host.
    #[must_use]
    pub fn into_parts(self) -> (CallbackFace, SessionWorker) {
        (self.callback, self.worker)
    }

    /// Current off-callback snapshot.
    #[must_use]
    pub fn snapshot(&self) -> SessionSnapshot {
        self.worker.snapshot()
    }

    /// Applies a control command. Never call this from the audio callback.
    pub fn apply(&mut self, command: EngineCommand) -> Result<(), PlaneError> {
        self.worker.apply(command)
    }

    /// Applies the selected codec and its settings. Off-callback only.
    pub fn apply_codec(&mut self, settings: crate::CodecSettings) {
        self.worker.apply_codec(settings);
    }

    /// Host-callback capture tap.
    #[must_use]
    pub fn process_capture(&mut self, interleaved: &[f32]) -> WriteOutcome {
        self.callback.process_capture(interleaved)
    }

    /// Host-callback render.
    #[must_use]
    pub fn render(&mut self, output: &mut [f32], dry: &[f32]) -> RenderReport {
        self.callback.render(output, dry)
    }

    /// Worker-side pump. Not callback-safe.
    pub fn drive(&mut self) -> Result<DriveReport, PlaneError> {
        self.worker.drive()
    }

    /// Latest 48 kHz stereo samples produced for media / web fan-out.
    #[must_use]
    pub fn last_web_pcm(&self) -> Option<(&[f32], u64)> {
        self.worker.last_web_pcm()
    }

    /// Drains every unpublished 48 kHz web frame.
    #[must_use]
    pub fn take_web_pcm(&mut self) -> Option<(Vec<f32>, u64)> {
        self.worker.take_web_pcm()
    }
}

impl CallbackFace {
    /// Host-callback capture tap. Copies into a preallocated staging buffer and
    /// publishes whole TX chunks through the lock-free ring.
    #[must_use]
    pub fn process_capture(&mut self, interleaved: &[f32]) -> WriteOutcome {
        if interleaved.is_empty() {
            return WriteOutcome::Written { samples: 0 };
        }
        let mut written = 0;
        let mut source = interleaved;
        while !source.is_empty() {
            let room = self.staging.len().saturating_sub(self.staging_filled);
            let take = source.len().min(room);
            self.staging[self.staging_filled..self.staging_filled + take]
                .copy_from_slice(&source[..take]);
            self.staging_filled += take;
            source = &source[take..];
            written += take;
            if self.staging_filled == self.staging.len() {
                match self.capture_tx.write(&self.staging) {
                    WriteOutcome::Written { .. } => {}
                    other => return other,
                }
                self.staging_filled = 0;
            }
        }
        WriteOutcome::Written { samples: written }
    }

    /// Changes the callback mix policy. Safe on the audio thread.
    pub fn set_monitor(&mut self, monitor: MonitorMode) {
        self.monitor = monitor;
    }

    /// Host-callback render. Zero-fills, then applies the monitor policy.
    #[must_use]
    pub fn render(&mut self, output: &mut [f32], dry: &[f32]) -> RenderReport {
        let remote = self.renderer.render(output);
        match self.monitor {
            MonitorMode::Remote => remote,
            MonitorMode::Dry => {
                copy_dry(output, dry);
                remote
            }
            MonitorMode::Mix => {
                mix_dry(output, dry);
                remote
            }
        }
    }
}

impl SessionWorker {
    /// Current off-callback snapshot.
    #[must_use]
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            mode: self.config.mode,
            state: self.state,
            route: self.route,
            peers: self.plane.peer_count(),
            local_port: self.plane.local_addr().map(|addr| addr.port()),
        }
    }

    /// Applies the selected codec and its settings. Off-callback only.
    pub fn apply_codec(&mut self, settings: crate::CodecSettings) {
        self.codec = settings.codec();
        self.flac_level = settings
            .flac_level()
            .unwrap_or(crate::codec::FLAC_LEVEL_DEFAULT);
        self.config.lan = settings.codec().is_pcm();
        let _ = self.tx.set_bitrate(settings.opus_bitrate_bps());
    }

    /// Applies the session password token. Off-callback only.
    pub fn apply_password(&mut self, token: [u8; 32]) {
        self.password_hash = token;
        self.plane.set_auth(token);
    }

    /// Latest 48 kHz stereo media frame, if the worker has produced one.
    ///
    /// This is what the web listen path should upload. Device-rate capture
    /// chunks stay inside TX ingest.
    #[must_use]
    pub fn last_web_pcm(&self) -> Option<(&[f32], u64)> {
        if self.web_pcm_seq == 0 || self.web_pcm.is_empty() {
            None
        } else {
            Some((self.web_pcm.as_slice(), self.web_pcm_seq))
        }
    }

    /// Drains unpublished 48 kHz stereo samples for the listen-page upload.
    #[must_use]
    pub fn take_web_pcm(&mut self) -> Option<(Vec<f32>, u64)> {
        if self.web_pcm.is_empty() {
            return None;
        }
        let seq = self.web_pcm_seq;
        Some((core::mem::take(&mut self.web_pcm), seq))
    }

    /// Last device-rate capture chunk the worker encoded or sent.
    #[must_use]
    pub fn last_capture(&self) -> &[f32] {
        &self.capture_chunk
    }

    /// Applies a control command. Never call this from the audio callback.
    pub fn apply(&mut self, command: EngineCommand) -> Result<(), PlaneError> {
        match command {
            EngineCommand::Listen(addr) => {
                self.config.mode = SessionMode::Connect;
                self.route = MediaRoute::Direct;
                self.plane.bind(addr, SocketRole::Connect)?;
                self.state = ConnectionState::Connecting;
            }
            EngineCommand::Join(peer) => {
                self.config.mode = SessionMode::Connect;
                self.route = MediaRoute::Direct;
                if self.plane.role() == SocketRole::Idle {
                    self.plane
                        .bind(SocketAddr::from(([0, 0, 0, 0], 0)), SocketRole::Connect)?;
                }
                self.plane.add_peer(peer)?;
                self.state = ConnectionState::Connecting;
            }
            EngineCommand::HostStream(addr) => {
                self.config.mode = SessionMode::Stream;
                self.route = MediaRoute::Sfu;
                self.plane.bind(addr, SocketRole::StreamHub)?;
                self.state = ConnectionState::Connected;
            }
            EngineCommand::PublishStream(hub) => {
                self.config.mode = SessionMode::Stream;
                self.route = MediaRoute::Sfu;
                self.plane.bind(
                    SocketAddr::from(([0, 0, 0, 0], 0)),
                    SocketRole::StreamProducer,
                )?;
                self.plane.add_peer(hub)?;
                self.state = ConnectionState::Connecting;
            }
            EngineCommand::ListenStream(hub) => {
                self.config.mode = SessionMode::Stream;
                self.route = MediaRoute::Sfu;
                self.plane.bind(
                    SocketAddr::from(([0, 0, 0, 0], 0)),
                    SocketRole::StreamListener,
                )?;
                self.plane.add_peer(hub)?;
                self.state = ConnectionState::Connecting;
            }
            EngineCommand::Loopback => {
                self.config.mode = SessionMode::Connect;
                self.route = MediaRoute::Direct;
                let local = self
                    .plane
                    .bind(SocketAddr::from(([127, 0, 0, 1], 0)), SocketRole::Connect)?;
                self.plane.add_peer(local)?;
                self.state = ConnectionState::Connecting;
            }
            EngineCommand::SetSlug { bytes, len } => {
                self.slug = bytes;
                self.slug_len = len;
            }
            EngineCommand::JoinLan { bytes, len } => {
                self.config.mode = SessionMode::Connect;
                self.route = MediaRoute::Direct;
                self.want_slug = bytes;
                self.want_slug_len = len;
                if self.plane.role() == SocketRole::Idle {
                    self.plane
                        .bind(SocketAddr::from(([0, 0, 0, 0], 0)), SocketRole::Connect)?;
                }
                self.state = ConnectionState::Signaling;
                self.broadcast_who()?;
            }
            EngineCommand::Disconnect => {
                self.state = ConnectionState::Closing;
                self.plane = NativePlane::new(self.config.ssrc);
                self.state = ConnectionState::Closed;
            }
        }
        Ok(())
    }

    /// Worker-side pump: socket, decode, encode, send. Not callback-safe.
    pub fn drive(&mut self) -> Result<DriveReport, PlaneError> {
        let mut report = DriveReport::default();
        let mut media_accepted = 0_u32;
        if self.plane.role() != SocketRole::Idle {
            while let Some(inbound) = self.plane.recv()? {
                report.received += 1;
                if self.handle_inbound(inbound.from, inbound.packet)? {
                    media_accepted = media_accepted.saturating_add(1);
                }
            }
        }
        report.decoded_frames = self.pump_rx(media_accepted);
        if self.want_slug_len > 0
            && self.plane.peer_count() == 0
            && self.last_who.elapsed() >= Duration::from_millis(250)
        {
            let _ = self.broadcast_who();
        }
        if self.slug_len > 0
            && matches!(
                self.plane.role(),
                SocketRole::Connect | SocketRole::StreamHub | SocketRole::StreamProducer
            )
            && self.last_who.elapsed() >= Duration::from_millis(500)
        {
            let _ = self.broadcast_announce();
        }
        // Listen-only roles must not consume the capture ring. A hub still
        // sends its own insert audio (and forwards peers separately).
        if self.plane.role() != SocketRole::StreamListener {
            report.sent = self.pump_tx()?;
        }
        if self.plane.role() == SocketRole::StreamHub {
            report.sent = report.sent.max(self.plane.peer_count() as u64);
        }
        Ok(report)
    }

    fn handle_inbound(
        &mut self,
        from: SocketAddr,
        packet: WirePacket<'static>,
    ) -> Result<bool, PlaneError> {
        match packet {
            WirePacket::Hello { token, .. } => {
                if !self.allows(&token) {
                    return Ok(false);
                }
                self.plane.remember(from);
                self.plane.send_to(
                    from,
                    &WirePacket::HelloAck {
                        ssrc: self.config.ssrc,
                    },
                )?;
                self.state = ConnectionState::Connected;
            }
            WirePacket::HelloAck { .. } => {
                self.plane.remember(from);
                self.state = ConnectionState::Connected;
            }
            WirePacket::Publish { token, .. } => {
                if !self.allows(&token) {
                    return Ok(false);
                }
                self.plane.remember(from);
                self.state = ConnectionState::Connected;
            }
            WirePacket::Subscribe { token, .. } => {
                if self.plane.role() == SocketRole::StreamHub && self.allows(&token) {
                    self.plane.remember(from);
                }
            }
            WirePacket::Goodbye { .. } => {
                self.plane.forget(from);
                if self.plane.peer_count() == 0 {
                    self.state = ConnectionState::Closing;
                }
            }
            WirePacket::Media { packet } => {
                if self.plane.role() == SocketRole::StreamHub {
                    self.plane.remember(from);
                    let _ = self.plane.forward_media(&packet, Some(from));
                    return Ok(false);
                }
                self.adopt_remote(
                    packet.ssrc().get(),
                    Some(packet.sequence().get()),
                    Some(packet.timestamp().get()),
                );
                let accepted = matches!(
                    self.rx.ingress(packet).status(),
                    IngressStatus::AcceptedInOrder | IngressStatus::AcceptedReordered { .. }
                );
                self.state = ConnectionState::Connected;
                return Ok(accepted);
            }
            WirePacket::Who { name, token, .. } => {
                if self.slug_matches(&name) && self.allows(&token) {
                    self.plane.remember(from);
                    self.send_announce_to(from)?;
                }
            }
            WirePacket::Announce { name, port, .. } => {
                if self.want_matches(&name) {
                    let dest =
                        SocketAddr::new(from.ip(), if port == 0 { from.port() } else { port });
                    self.plane.add_peer(dest)?;
                    self.state = ConnectionState::Connected;
                }
            }
            WirePacket::Flac {
                ssrc,
                sequence,
                timestamp,
                payload,
            } => {
                if self.plane.role() == SocketRole::StreamHub {
                    self.plane.remember(from);
                    let framed = WirePacket::Flac {
                        ssrc,
                        sequence,
                        timestamp,
                        payload,
                    };
                    let _ = self.plane.forward_wire(&framed, Some(from));
                    return Ok(false);
                }
                let Ok(pcm16) = crate::flac::decode_s16le(&payload) else {
                    return Ok(false);
                };
                let mut bytes = Vec::with_capacity(pcm16.len() * 2);
                for sample in pcm16 {
                    bytes.extend_from_slice(&sample.to_le_bytes());
                }
                self.plane.remember(from);
                self.adopt_remote(ssrc, Some(sequence), Some(timestamp));
                self.push_lan_pcm(&bytes, timestamp);
                self.state = ConnectionState::Connected;
                return Ok(false);
            }
            WirePacket::Pcm {
                ssrc,
                sequence,
                timestamp,
                samples,
            } => {
                if self.plane.role() == SocketRole::StreamHub {
                    self.plane.remember(from);
                    let framed = WirePacket::Pcm {
                        ssrc,
                        sequence,
                        timestamp,
                        samples,
                    };
                    let _ = self.plane.forward_wire(&framed, Some(from));
                    return Ok(false);
                }
                self.plane.remember(from);
                self.adopt_remote(ssrc, Some(sequence), Some(timestamp));
                self.push_lan_pcm(&samples, timestamp);
                self.state = ConnectionState::Connected;
                return Ok(false);
            }
            WirePacket::_Reserved(_) => {}
        }
        Ok(false)
    }

    fn allows(&self, token: &[u8; 32]) -> bool {
        crate::password_allows(&self.password_hash, token)
    }

    fn slug_text(bytes: &[u8; 48], len: u8) -> &str {
        let end = usize::from(len).min(48);
        core::str::from_utf8(&bytes[..end]).unwrap_or("")
    }

    fn slug_matches(&self, name: &str) -> bool {
        self.slug_len > 0 && Self::slug_text(&self.slug, self.slug_len) == name
    }

    fn want_matches(&self, name: &str) -> bool {
        self.want_slug_len > 0 && Self::slug_text(&self.want_slug, self.want_slug_len) == name
    }

    fn broadcast_who(&mut self) -> Result<(), PlaneError> {
        let name = Self::slug_text(&self.want_slug, self.want_slug_len).to_string();
        if name.is_empty() {
            return Ok(());
        }
        self.last_who = Instant::now();
        self.plane.send_discovery(
            &WirePacket::Who {
                ssrc: self.config.ssrc,
                name,
                token: self.password_hash,
            },
            Some(self.plane.local_addr().map(|addr| addr.port()).unwrap_or(0)),
        )
    }

    fn broadcast_announce(&mut self) -> Result<(), PlaneError> {
        self.last_who = Instant::now();
        let packet = self.announce_packet();
        self.plane.send_discovery(
            &packet,
            Some(self.plane.local_addr().map(|addr| addr.port()).unwrap_or(0)),
        )
    }

    fn send_announce_to(&mut self, dest: SocketAddr) -> Result<(), PlaneError> {
        let packet = self.announce_packet();
        self.plane.send_to(dest, &packet)
    }

    fn announce_packet(&self) -> WirePacket<'static> {
        WirePacket::Announce {
            ssrc: self.config.ssrc,
            port: self.plane.local_addr().map(|addr| addr.port()).unwrap_or(0),
            name: Self::slug_text(&self.slug, self.slug_len).to_string(),
        }
    }

    fn push_lan_pcm(&mut self, samples: &[u8], timestamp: u32) {
        let frame_samples = self.tx.media_pcm_frame_samples().max(2);
        if self.pcm_decode.len() < frame_samples {
            self.pcm_decode.resize(frame_samples, 0.0);
        }
        let mut ts = timestamp;
        for packet_chunk in samples.chunks_exact(frame_samples.saturating_mul(2)) {
            for (index, pair) in packet_chunk.chunks_exact(2).enumerate() {
                let quant = i16::from_le_bytes([pair[0], pair[1]]);
                self.pcm_decode[index] = f32::from(quant) / 32_767.0;
            }
            if !self
                .lan_frame
                .copy_from_interleaved(&self.pcm_decode[..frame_samples])
            {
                break;
            }
            let remote = ExtendedTimestamp::starting_at(RtpTimestamp::new(ts));
            let local = self.scheduled_local;
            match self.playback.process_frame(&self.lan_frame, remote, local) {
                Ok(_) | Err(PlaybackProcessError::EndOfStream) => {}
                Err(_) => break,
            }
            let frame_frames = (frame_samples / CHANNELS) as u64;
            self.scheduled_local = self.scheduled_local.saturating_add(
                frame_frames.saturating_mul(self.pipeline.playback_rate_hz() as u64) / 48_000,
            );
            ts = ts.wrapping_add(frame_frames as u32);
        }
    }

    fn adopt_remote(&mut self, ssrc: u32, sequence: Option<u16>, timestamp: Option<u32>) {
        if self.remote_ssrc == Some(ssrc) {
            return;
        }
        self.remote_ssrc = Some(ssrc);
        let Ok(payload_type) = PayloadType::new(PAYLOAD_TYPE) else {
            return;
        };
        let initial = match sequence {
            Some(wire) => ExtendedSequence::starting_at(SequenceNumber::new(wire)),
            None => ExtendedSequence::new(0),
        };
        let _ = self.rx.reset(RxStreamConfig {
            ssrc: Ssrc::new(ssrc),
            payload_type,
            initial_sequence: initial,
            initial_timestamp: RtpTimestamp::new(timestamp.unwrap_or(0)),
        });
    }

    fn pump_rx(&mut self, media_received: u32) -> u64 {
        let mut decoded = 0;
        for _ in 0..media_received {
            let Some(outcome) = self.rx.tick() else {
                continue;
            };
            let remote = ExtendedTimestamp::starting_at(outcome.timestamp());
            let local = self.scheduled_local;
            let frame_frames = outcome.frame().samples_per_channel() as u64;
            match self.playback.process_frame(outcome.frame(), remote, local) {
                Ok(_) | Err(PlaybackProcessError::EndOfStream) => {}
                Err(_) => break,
            }
            self.scheduled_local = self.scheduled_local.saturating_add(
                frame_frames.saturating_mul(self.pipeline.playback_rate_hz() as u64) / 48_000,
            );
            decoded += 1;
        }
        decoded
    }

    fn pump_tx(&mut self) -> Result<u64, PlaneError> {
        let mut sent = 0;
        loop {
            let needed = self.capture_chunk.len();
            if self.capture_rx.available_samples() < needed {
                break;
            }
            let outcome = self.capture_rx.read(&mut self.capture_chunk);
            if outcome.read_samples != needed {
                break;
            }
            match self.codec {
                crate::WireCodec::Pcm => {
                    sent += self.send_lan_pcm()?;
                    continue;
                }
                crate::WireCodec::Flac => {
                    sent += self.send_flac()?;
                    continue;
                }
                crate::WireCodec::Opus => {
                    sent += self.send_opus()?;
                    continue;
                }
            }
        }
        Ok(sent)
    }

    fn send_lan_pcm(&mut self) -> Result<u64, PlaneError> {
        if self.tx.ingest_capture(&self.capture_chunk).is_err() {
            return Ok(0);
        }
        let frame_samples = self.tx.media_pcm_frame_samples();
        if self.pcm_decode.len() < frame_samples {
            self.pcm_decode.resize(frame_samples, 0.0);
        }
        if self.pcm_bytes.len() < frame_samples.saturating_mul(2) {
            self.pcm_bytes.resize(frame_samples.saturating_mul(2), 0);
        }
        let mut sent = 0_u64;
        while let Some((sequence, timestamp)) = self.tx.take_media_pcm(&mut self.pcm_decode) {
            crate::codec::write_s16le(
                &self.pcm_decode[..frame_samples],
                &mut self.pcm_bytes[..frame_samples * 2],
            );
            let packet = WirePacket::Pcm {
                ssrc: self.config.ssrc,
                sequence: sequence.get(),
                timestamp: timestamp.get(),
                samples: self.pcm_bytes[..frame_samples * 2].to_vec(),
            };
            sent += self.plane.forward_wire(&packet, None)? as u64;
            self.pcm_seq = sequence.get();
            self.pcm_ts = timestamp.get();
            self.store_web_pcm(frame_samples);
        }
        Ok(sent)
    }

    fn send_flac(&mut self) -> Result<u64, PlaneError> {
        if self.tx.ingest_capture(&self.capture_chunk).is_err() {
            return Ok(0);
        }
        let frame_samples = self.tx.media_pcm_frame_samples();
        if self.pcm_decode.len() < frame_samples {
            self.pcm_decode.resize(frame_samples, 0.0);
        }
        let mut sent = 0_u64;
        while let Some((sequence, timestamp)) = self.tx.take_media_pcm(&mut self.pcm_decode) {
            self.store_web_pcm(frame_samples);
            let mut pcm16 = Vec::with_capacity(frame_samples);
            for sample in &self.pcm_decode[..frame_samples] {
                pcm16.push(crate::codec::quantize_s16(*sample));
            }
            let Ok(payload) = crate::flac::encode_s16le(&pcm16, self.flac_level) else {
                continue;
            };
            let packet = WirePacket::Flac {
                ssrc: self.config.ssrc,
                sequence: sequence.get(),
                timestamp: timestamp.get(),
                payload,
            };
            sent += self.plane.forward_wire(&packet, None)? as u64;
            self.pcm_seq = sequence.get();
            self.pcm_ts = timestamp.get();
        }
        Ok(sent)
    }

    fn send_opus(&mut self) -> Result<u64, PlaneError> {
        if self.tx.ingest_capture(&self.capture_chunk).is_err() {
            return Ok(0);
        }
        self.append_ready_web_pcm();
        match self.tx.encode_ready(&mut self.batch) {
            TxProcessOutcome::Complete(_) | TxProcessOutcome::BatchFull(_) => {}
            TxProcessOutcome::Disconnected(_) | TxProcessOutcome::Error(_) => return Ok(0),
        }
        let mut sent = 0_u64;
        while let Some(packet) = self.batch.take_next() {
            sent += self.plane.send_media(&packet)? as u64;
        }
        Ok(sent)
    }

    fn append_ready_web_pcm(&mut self) {
        let frame_samples = self.tx.media_pcm_frame_samples();
        if frame_samples == 0 {
            return;
        }
        let ready = self
            .tx
            .accumulated_media_frames()
            .saturating_mul(frame_samples);
        if ready == 0 {
            return;
        }
        if self.pcm_decode.len() < ready {
            self.pcm_decode.resize(ready, 0.0);
        }
        let copied = self.tx.copy_ready_pcm_all(&mut self.pcm_decode[..ready]);
        if copied == 0 {
            return;
        }
        self.web_pcm.extend_from_slice(&self.pcm_decode[..copied]);
        self.web_pcm_seq = self.web_pcm_seq.saturating_add(1);
        const MAX_WEB: usize = 48_000 * 2 * 2;
        if self.web_pcm.len() > MAX_WEB {
            let overflow = self.web_pcm.len() - MAX_WEB;
            self.web_pcm.drain(..overflow);
        }
    }

    fn store_web_pcm(&mut self, frame_samples: usize) {
        let n = frame_samples.min(self.pcm_decode.len());
        self.web_pcm.extend_from_slice(&self.pcm_decode[..n]);
        self.web_pcm_seq = self.web_pcm_seq.saturating_add(1);
        const MAX_WEB: usize = 48_000 * 2 * 2;
        if self.web_pcm.len() > MAX_WEB {
            let overflow = self.web_pcm.len() - MAX_WEB;
            self.web_pcm.drain(..overflow);
        }
    }
}

fn pipeline_for(
    rate_hz: usize,
    duration: FrameDuration,
) -> Result<AudioPipelineConfig, EngineBuildError> {
    let chunk = (rate_hz / 100).max(1);
    AudioPipelineConfig::new(AudioPipelineConfigInput {
        capture_rate_hz: rate_hz,
        playback_rate_hz: rate_hz,
        channels: CHANNELS,
        frame_duration: duration,
        capture_src_chunk_frames: chunk,
        capture_ring_samples: 100_000,
        playback_ring_samples: 100_000,
        tx_accumulator_samples: 100_000,
        reorder_capacity: 64,
        network_capacity: 64,
        network_due_batch_capacity: 16,
        packet_capacity: MAX_PACKET_BYTES,
        controller_cadence_frames: chunk,
        clock_recovery: ClockRecoveryConfig::default(),
        adaptive_clock: AdaptiveClockConfig::default(),
    })
    .map_err(|_| EngineBuildError::Config)
}

fn copy_dry(output: &mut [f32], dry: &[f32]) {
    let n = output.len().min(dry.len());
    output[..n].copy_from_slice(&dry[..n]);
    for sample in &mut output[n..] {
        *sample = 0.0;
    }
}

fn mix_dry(output: &mut [f32], dry: &[f32]) {
    let n = output.len().min(dry.len());
    for index in 0..n {
        let mixed = output[index] + dry[index];
        output[index] = mixed.clamp(-1.0, 1.0);
    }
}

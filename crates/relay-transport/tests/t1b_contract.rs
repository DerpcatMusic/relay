use core::task::{Context, Poll};
use std::task::Waker;

use relay_transport::{
    BinaryPayload, CapabilityLimitedFakeProvider, ChannelId, Command, DescriptionKind, Event,
    FakeNativeTransportProvider, IceServer, IceTransport, NativeTransportProvider,
    NegotiationEpoch, OperationId, PeerConfig, PeerDriver, PeerState, ProviderCapabilities,
    SessionDescription, TlsTrust, TransportError, TurnCredentials, TurnTlsConfig,
};

fn next(peer: &mut impl PeerDriver) -> Event {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match peer.poll_event(&mut context) {
        Poll::Ready(Some(event)) => event,
        other => panic!("expected event, got {other:?}"),
    }
}

fn connected_peer(mut config: PeerConfig) -> relay_transport::FakePeer {
    config.required_capabilities.reliable_ordered_data_channel = true;
    let mut peer = FakeNativeTransportProvider
        .create_fake_peer(config.validate().expect("configuration is valid"))
        .expect("fake construction succeeds");
    let epoch = NegotiationEpoch(1);
    peer.submit(Command::CreateOffer {
        operation_id: OperationId(1),
        epoch,
    })
    .expect("offer is admitted");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Negotiating,
        }
    );
    let local = match next(&mut peer) {
        Event::LocalDescription { description } => description,
        other => panic!("expected description, got {other:?}"),
    };
    assert!(matches!(next(&mut peer), Event::LocalCandidate { .. }));
    assert!(matches!(
        next(&mut peer),
        Event::LocalCandidatesEnded { .. }
    ));
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(1),
        }
    );
    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(2),
        description: local,
    })
    .expect("local description is admitted");
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(2),
        }
    );
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(3),
        description: SessionDescription::new(epoch, DescriptionKind::Answer, "bounded answer")
            .expect("bounded SDP"),
    })
    .expect("answer is admitted");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Connecting,
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Connected,
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(3),
        }
    );
    peer
}

fn rejected(result: Result<(), relay_transport::SubmitError>, error: TransportError) -> Command {
    let rejection = result.expect_err("command must be returned");
    assert_eq!(rejection.error(), error);
    let (actual, command) = rejection.into_parts();
    assert_eq!(actual, error);
    command
}

#[test]
fn ice_turn_tls_validation_is_fail_closed_and_capability_checked() {
    assert_eq!(
        IceServer::stun("stun.example", 3478, IceTransport::Tls),
        Err(TransportError::InvalidIceServer),
    );
    let credentials = TurnCredentials::new("user", "secret").expect("bounded credentials");
    assert!(!format!("{credentials:?}").contains("secret"));
    assert_eq!(
        IceServer::turn(
            "turn.example",
            5349,
            IceTransport::Tls,
            credentials.clone(),
            None,
        ),
        Err(TransportError::InvalidIceServer),
    );
    assert_eq!(
        TurnTlsConfig::new("bad name", TlsTrust::Platform),
        Err(TransportError::InvalidTlsServerName),
    );
    assert_eq!(
        TurnTlsConfig::new("turn.example", TlsTrust::Custom(Vec::new())),
        Err(TransportError::InvalidTlsTrust),
    );

    let tls = TurnTlsConfig::new(
        "turn.example",
        TlsTrust::Custom(vec![
            include_bytes!("fixtures/minimal-ed25519-cert.der").to_vec(),
        ]),
    )
    .expect("custom trust is bounded");
    let server = IceServer::turn(
        "turn.example",
        5349,
        IceTransport::Tls,
        credentials,
        Some(tls),
    )
    .expect("secure TURN/TLS is valid");
    let mut config = PeerConfig::offerer();
    config.ice_servers.push(server.clone());
    config.required_capabilities.ice_restart = true;
    let mut duplicate = config.clone();
    duplicate.ice_servers.push(server);
    assert_eq!(duplicate.validate(), Err(TransportError::InvalidIceServer),);
    let validated = config
        .validate()
        .expect("complete fake supports requirements");

    let mut missing = ProviderCapabilities::ALL;
    missing.custom_tls_trust = false;
    let provider = CapabilityLimitedFakeProvider::new(missing);
    assert_eq!(
        provider.create_peer(validated).err(),
        Some(TransportError::UnsupportedCapability),
    );
    assert_eq!(
        config.validate_for(missing),
        Err(TransportError::UnsupportedCapability),
    );
}

#[test]
fn send_is_atomic_owned_and_retries_only_after_deterministic_capacity() {
    let mut config = PeerConfig::offerer();
    config.max_message_bytes = 6;
    config.send_buffer_bytes = 6;
    config.send_buffer_messages = 1;
    config.send_low_water_bytes = 0;
    let mut invalid_low_water = config.clone();
    invalid_low_water.send_low_water_bytes = 1;
    assert_eq!(
        invalid_low_water.validate(),
        Err(TransportError::InvalidLowWaterMark),
        "the threshold must guarantee a future retry edge for the largest message",
    );
    let mut peer = connected_peer(config);
    let channel_id = ChannelId(9);
    peer.submit(Command::OpenDataChannel {
        operation_id: OperationId(4),
        channel_id,
    })
    .expect("open is admitted");
    assert_eq!(next(&mut peer), Event::DataChannelOpened { channel_id });
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(4),
        }
    );

    peer.submit(Command::Send {
        operation_id: OperationId(5),
        channel_id,
        payload: BinaryPayload::new(vec![1, 2, 3, 4]).expect("bounded"),
    })
    .expect("complete message fits");
    assert_eq!(peer.buffered_send(), (4, 1));
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(5),
        },
        "completion means the provider queued the whole message",
    );

    let retry = Command::Send {
        operation_id: OperationId(6),
        channel_id,
        payload: BinaryPayload::new(vec![7, 8, 9]).expect("bounded"),
    };
    let returned = rejected(peer.submit(retry.clone()), TransportError::WouldBlock);
    assert_eq!(returned, retry);
    assert_eq!(peer.buffered_send(), (4, 1), "rejection transfers nothing");

    let (drained_channel, drained) = peer
        .drain_provider_send()
        .expect("the provider owns one complete send");
    assert_eq!(drained_channel, channel_id);
    assert_eq!(drained.as_bytes(), &[1, 2, 3, 4]);
    assert_eq!(
        next(&mut peer),
        Event::SendCapacity {
            channel_id,
            available_bytes: 6,
            available_messages: 1,
        }
    );
    peer.submit(returned).expect("same owned command retries");
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(6),
        }
    );
    assert_eq!(peer.buffered_send(), (3, 1));

    let oversize = Command::Send {
        operation_id: OperationId(7),
        channel_id,
        payload: BinaryPayload::new(vec![0; 7]).expect("below absolute cap"),
    };
    assert_eq!(
        rejected(
            peer.submit(oversize.clone()),
            TransportError::MessageTooLarge
        ),
        oversize,
    );
    assert_eq!(peer.buffered_send(), (3, 1));
}

#[test]
fn inbound_stats_and_channel_close_are_bounded_ordered_and_idempotent() {
    let mut config = PeerConfig::offerer();
    config.max_message_bytes = 4;
    let mut peer = connected_peer(config);
    let channel_id = ChannelId(1);
    peer.submit(Command::OpenDataChannel {
        operation_id: OperationId(4),
        channel_id,
    })
    .expect("open admitted");
    assert_eq!(next(&mut peer), Event::DataChannelOpened { channel_id });
    let _ = next(&mut peer);

    assert_eq!(
        peer.inject_message(
            channel_id,
            BinaryPayload::new(vec![1, 2, 3, 4, 5]).expect("absolute bound")
        ),
        Err(TransportError::MessageTooLarge),
    );
    peer.inject_message(
        channel_id,
        BinaryPayload::new(vec![1, 2, 3]).expect("bounded"),
    )
    .expect("inbound message fits");
    assert_eq!(
        next(&mut peer),
        Event::Message {
            channel_id,
            payload: BinaryPayload::new(vec![1, 2, 3]).expect("bounded"),
        }
    );

    peer.submit(Command::RequestStats {
        operation_id: OperationId(5),
    })
    .expect("stats admitted");
    let report = match next(&mut peer) {
        Event::Stats {
            operation_id: OperationId(5),
            report,
        } => report,
        other => panic!("expected stats, got {other:?}"),
    };
    assert_eq!(report.sequence, 1);
    assert_eq!(report.messages_received, 1);
    assert_eq!(report.bytes_received, 3);
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(5),
        }
    );

    peer.submit(Command::CloseDataChannel {
        operation_id: OperationId(6),
        channel_id,
    })
    .expect("close admitted");
    assert_eq!(next(&mut peer), Event::DataChannelClosed { channel_id });
    let _ = next(&mut peer);
    peer.submit(Command::CloseDataChannel {
        operation_id: OperationId(7),
        channel_id,
    })
    .expect("repeat close is idempotent");
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(7),
        }
    );
    let send = Command::Send {
        operation_id: OperationId(8),
        channel_id,
        payload: BinaryPayload::new(vec![1]).expect("bounded"),
    };
    assert_eq!(
        rejected(peer.submit(send.clone()), TransportError::InvalidState),
        send,
    );
}

#[test]
fn explicit_restart_has_a_new_epoch_and_one_exact_terminal() {
    let mut peer = connected_peer(PeerConfig::offerer());
    peer.submit(Command::RestartIce {
        operation_id: OperationId(4),
        epoch: NegotiationEpoch(2),
    })
    .expect("restart admitted");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Restarting,
        }
    );
    let description = match next(&mut peer) {
        Event::LocalDescription { description } => description,
        other => panic!("expected restart description, got {other:?}"),
    };
    assert_eq!(description.epoch(), NegotiationEpoch(2));
    assert!(matches!(next(&mut peer), Event::LocalCandidate { .. }));
    assert!(matches!(
        next(&mut peer),
        Event::LocalCandidatesEnded { .. }
    ));
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(4),
        }
    );
    peer.submit(Command::RestartIce {
        operation_id: OperationId(5),
        epoch: NegotiationEpoch(2),
    })
    .expect("stale restart is a correlated failure");
    assert_eq!(
        next(&mut peer),
        Event::OperationFailed {
            operation_id: OperationId(5),
            error: TransportError::StaleEpoch,
        }
    );
}

#[test]
fn callback_overflow_fatal_drop_and_timeout_paths_are_deterministic() {
    let mut config = PeerConfig::offerer();
    config.event_capacity = 5;
    config.max_message_bytes = 1;
    let mut peer = connected_peer(config);
    let channel_id = ChannelId(7);
    peer.submit(Command::OpenDataChannel {
        operation_id: OperationId(4),
        channel_id,
    })
    .expect("open accepted");
    assert_eq!(next(&mut peer), Event::DataChannelOpened { channel_id });
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(4),
        }
    );
    peer.submit(Command::RequestStats {
        operation_id: OperationId(5),
    })
    .expect("accepted operation");
    for byte in 0..5 {
        peer.inject_message(
            channel_id,
            BinaryPayload::new(vec![byte]).expect("bounded message"),
        )
        .expect("legal provider callback fits");
    }
    assert_eq!(
        peer.inject_message(
            channel_id,
            BinaryPayload::new(vec![9]).expect("bounded message"),
        ),
        Err(TransportError::EventQueueOverflow),
    );
    for _ in 0..5 {
        assert!(matches!(next(&mut peer), Event::Message { .. }));
    }
    assert_eq!(
        next(&mut peer),
        Event::OperationFailed {
            operation_id: OperationId(5),
            error: TransportError::EventQueueOverflow,
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Failed,
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::FatalError {
            error: TransportError::EventQueueOverflow,
        }
    );
    assert_eq!(
        peer.inject_message(
            channel_id,
            BinaryPayload::new(vec![10]).expect("bounded message"),
        ),
        Err(TransportError::ProviderFailure),
    );
    peer.submit(Command::Shutdown {
        operation_id: OperationId(6),
    })
    .expect("explicit shutdown remains available after fatal");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Closing,
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(6),
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Closed,
        }
    );
    assert_eq!(next(&mut peer), Event::ShutdownComplete);
    assert_eq!(
        peer.inject_provider_drop(),
        Err(TransportError::Shutdown),
        "no provider callback is admitted after the terminal marker",
    );
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(peer.poll_event(&mut context), Poll::Ready(None));

    let mut timeout_config = PeerConfig::offerer();
    timeout_config.shutdown_timeout_ms = 7;
    let mut timeout = FakeNativeTransportProvider
        .create_fake_peer(timeout_config.validate().expect("valid"))
        .expect("fake");
    let teardown = timeout.teardown_probe();
    timeout
        .inject_shutdown_timeout()
        .expect("fault installed before shutdown");
    timeout
        .submit(Command::Shutdown {
            operation_id: OperationId(1),
        })
        .expect("shutdown admitted");
    assert_eq!(timeout.poll_event(&mut context), Poll::Pending);
    timeout.advance_time(6);
    assert_eq!(timeout.poll_event(&mut context), Poll::Pending);
    timeout.advance_time(1);
    assert_eq!(
        next(&mut timeout),
        Event::StateChanged {
            state: PeerState::Closing,
        }
    );
    assert_eq!(
        next(&mut timeout),
        Event::OperationFailed {
            operation_id: OperationId(1),
            error: TransportError::ShutdownTimeout,
        }
    );
    assert!(
        teardown.was_torn_down(),
        "forced resources are gone before the timeout terminal"
    );
    assert_eq!(
        next(&mut timeout),
        Event::StateChanged {
            state: PeerState::Closed,
        }
    );
    assert_eq!(next(&mut timeout), Event::ShutdownComplete);
}

#[test]
fn provider_drop_preserves_every_accepted_terminal_before_shutdown() {
    let mut peer = FakeNativeTransportProvider
        .create_fake_peer(PeerConfig::offerer().validate().expect("valid"))
        .expect("fake");
    peer.submit(Command::RequestStats {
        operation_id: OperationId(1),
    })
    .expect("work accepted before provider loss");
    peer.inject_provider_drop()
        .expect("provider loss fault is installed");
    peer.submit(Command::Shutdown {
        operation_id: OperationId(2),
    })
    .expect("shutdown can be queued during fatal drain");

    assert_eq!(
        next(&mut peer),
        Event::OperationFailed {
            operation_id: OperationId(1),
            error: TransportError::ProviderFailure,
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Failed,
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::FatalError {
            error: TransportError::ProviderFailure,
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Closing,
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(2),
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Closed,
        }
    );
    assert_eq!(next(&mut peer), Event::ShutdownComplete);
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(peer.poll_event(&mut context), Poll::Ready(None));
}

#[test]
fn transport_and_required_capability_matrix_is_fail_closed() {
    let credentials = TurnCredentials::new("user", "secret").expect("credentials");
    let tls = TurnTlsConfig::new("turn.example", TlsTrust::Platform).expect("TLS policy");
    type CapabilityCase = (IceServer, fn(&mut ProviderCapabilities));
    let cases: [CapabilityCase; 5] = [
        (
            IceServer::stun("stun.example", 3478, IceTransport::Udp).expect("STUN UDP"),
            |caps: &mut ProviderCapabilities| caps.stun_udp = false,
        ),
        (
            IceServer::stun("stun.example", 3478, IceTransport::Tcp).expect("STUN TCP"),
            |caps: &mut ProviderCapabilities| caps.stun_tcp = false,
        ),
        (
            IceServer::turn(
                "turn.example",
                3478,
                IceTransport::Udp,
                credentials.clone(),
                None,
            )
            .expect("TURN UDP"),
            |caps: &mut ProviderCapabilities| caps.turn_udp = false,
        ),
        (
            IceServer::turn(
                "turn.example",
                3478,
                IceTransport::Tcp,
                credentials.clone(),
                None,
            )
            .expect("TURN TCP"),
            |caps: &mut ProviderCapabilities| caps.turn_tcp = false,
        ),
        (
            IceServer::turn(
                "turn.example",
                5349,
                IceTransport::Tls,
                credentials,
                Some(tls),
            )
            .expect("TURN TLS"),
            |caps: &mut ProviderCapabilities| caps.turn_tls = false,
        ),
    ];
    for (server, remove_capability) in cases {
        let mut config = PeerConfig::offerer();
        config.ice_servers.push(server);
        assert!(config.validate_for(ProviderCapabilities::ALL).is_ok());
        let mut missing = ProviderCapabilities::ALL;
        remove_capability(&mut missing);
        assert_eq!(
            config.validate_for(missing),
            Err(TransportError::UnsupportedCapability)
        );
    }

    for feature in 0..3 {
        let mut config = PeerConfig::offerer();
        let mut missing = ProviderCapabilities::ALL;
        match feature {
            0 => {
                config.required_capabilities.ice_restart = true;
                missing.ice_restart = false;
            }
            1 => {
                config.required_capabilities.reliable_ordered_data_channel = true;
                missing.reliable_ordered_data_channel = false;
            }
            _ => {
                config.required_capabilities.stats = true;
                missing.stats = false;
            }
        }
        assert_eq!(
            config.validate_for(missing),
            Err(TransportError::UnsupportedCapability)
        );
    }
}

#[test]
fn send_rejections_preserve_exact_allocations_and_distinguish_bounds() {
    fn payload_pointer(command: &Command) -> *const u8 {
        let Command::Send { payload, .. } = command else {
            panic!("expected send command");
        };
        payload.as_bytes().as_ptr()
    }

    let mut config = PeerConfig::offerer();
    config.command_capacity = 1;
    config.max_message_bytes = 4;
    config.send_buffer_bytes = 4;
    config.send_buffer_messages = 2;
    config.send_low_water_bytes = 0;
    let mut peer = connected_peer(config);
    let channel_id = ChannelId(11);
    peer.submit(Command::OpenDataChannel {
        operation_id: OperationId(4),
        channel_id,
    })
    .expect("open admitted");
    let _ = next(&mut peer);
    let _ = next(&mut peer);

    peer.submit(Command::Send {
        operation_id: OperationId(5),
        channel_id,
        payload: BinaryPayload::new(vec![1]).expect("bounded"),
    })
    .expect("first send occupies the command queue");
    let queued_out = Command::Send {
        operation_id: OperationId(6),
        channel_id,
        payload: BinaryPayload::new(vec![2]).expect("bounded"),
    };
    let queued_pointer = payload_pointer(&queued_out);
    let returned = rejected(peer.submit(queued_out), TransportError::QueueFull);
    assert_eq!(payload_pointer(&returned), queued_pointer);
    let _ = next(&mut peer);
    let _ = peer.drain_provider_send();
    assert!(matches!(next(&mut peer), Event::SendCapacity { .. }));

    let oversized = Command::Send {
        operation_id: OperationId(6),
        channel_id,
        payload: BinaryPayload::new(vec![3; 5]).expect("absolute bound"),
    };
    let oversized_pointer = payload_pointer(&oversized);
    let returned = rejected(peer.submit(oversized), TransportError::MessageTooLarge);
    assert_eq!(payload_pointer(&returned), oversized_pointer);

    peer.submit(Command::Send {
        operation_id: OperationId(6),
        channel_id,
        payload: BinaryPayload::new(vec![4; 4]).expect("bounded"),
    })
    .expect("byte budget filled");
    let _ = next(&mut peer);
    let blocked = Command::Send {
        operation_id: OperationId(7),
        channel_id,
        payload: BinaryPayload::new(vec![5]).expect("bounded"),
    };
    let blocked_pointer = payload_pointer(&blocked);
    let returned = rejected(peer.submit(blocked), TransportError::WouldBlock);
    assert_eq!(payload_pointer(&returned), blocked_pointer);

    let mut message_config = PeerConfig::offerer();
    message_config.max_message_bytes = 4;
    message_config.send_buffer_bytes = 8;
    message_config.send_buffer_messages = 1;
    message_config.send_low_water_bytes = 0;
    let mut message_peer = connected_peer(message_config);
    message_peer
        .submit(Command::OpenDataChannel {
            operation_id: OperationId(4),
            channel_id,
        })
        .expect("open admitted");
    let _ = next(&mut message_peer);
    let _ = next(&mut message_peer);
    message_peer
        .submit(Command::Send {
            operation_id: OperationId(5),
            channel_id,
            payload: BinaryPayload::new(vec![1]).expect("bounded"),
        })
        .expect("message slot filled");
    let _ = next(&mut message_peer);
    assert_eq!(
        rejected(
            message_peer.submit(Command::Send {
                operation_id: OperationId(6),
                channel_id,
                payload: BinaryPayload::new(vec![2]).expect("bounded"),
            }),
            TransportError::WouldBlock,
        )
        .operation_id(),
        OperationId(6),
    );
}

#[test]
fn fifo_low_water_stats_and_inbound_matrix_is_truthful() {
    let mut config = PeerConfig::offerer();
    config.max_message_bytes = 4;
    config.send_buffer_bytes = 8;
    config.send_buffer_messages = 4;
    config.send_low_water_bytes = 4;
    let mut peer = connected_peer(config);
    let channel_id = ChannelId(12);
    peer.submit(Command::OpenDataChannel {
        operation_id: OperationId(4),
        channel_id,
    })
    .expect("open admitted");
    let _ = next(&mut peer);
    let _ = next(&mut peer);

    for (operation, bytes) in [(5, vec![1, 2, 3]), (6, vec![4, 5, 6])] {
        peer.submit(Command::Send {
            operation_id: OperationId(operation),
            channel_id,
            payload: BinaryPayload::new(bytes).expect("bounded"),
        })
        .expect("send admitted");
        let _ = next(&mut peer);
    }
    for bytes in [vec![7], vec![8, 9]] {
        peer.inject_message(
            channel_id,
            BinaryPayload::new(bytes).expect("bounded inbound"),
        )
        .expect("inbound admitted");
    }
    assert_eq!(
        next(&mut peer),
        Event::Message {
            channel_id,
            payload: BinaryPayload::new(vec![7]).expect("bounded"),
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::Message {
            channel_id,
            payload: BinaryPayload::new(vec![8, 9]).expect("bounded"),
        }
    );

    peer.submit(Command::RequestStats {
        operation_id: OperationId(7),
    })
    .expect("stats admitted");
    let first = match next(&mut peer) {
        Event::Stats { report, .. } => report,
        other => panic!("expected stats, got {other:?}"),
    };
    assert_eq!(
        (
            first.messages_sent,
            first.bytes_sent,
            first.messages_received,
            first.bytes_received,
            first.buffered_send_bytes,
            first.buffered_send_messages,
        ),
        (2, 6, 2, 3, 6, 2),
    );
    let _ = next(&mut peer);

    let (_, first_send) = peer.drain_provider_send().expect("first FIFO message");
    assert_eq!(first_send.as_bytes(), &[1, 2, 3]);
    assert!(matches!(next(&mut peer), Event::SendCapacity { .. }));
    let (_, second_send) = peer.drain_provider_send().expect("second FIFO message");
    assert_eq!(second_send.as_bytes(), &[4, 5, 6]);

    peer.submit(Command::RequestStats {
        operation_id: OperationId(8),
    })
    .expect("second stats admitted");
    let second = match next(&mut peer) {
        Event::Stats { report, .. } => report,
        other => panic!("expected stats, got {other:?}"),
    };
    assert_eq!(second.sequence, first.sequence + 1);
    assert_eq!(second.buffered_send_bytes, 0);
    assert_eq!(second.buffered_send_messages, 0);
    assert_eq!(second.messages_sent, first.messages_sent);
    assert_eq!(second.messages_received, first.messages_received);
}

#[test]
fn open_idempotence_and_nonzero_low_water_rearm_are_explicit() {
    let mut config = PeerConfig::offerer();
    config.max_message_bytes = 4;
    config.send_buffer_bytes = 8;
    config.send_buffer_messages = 4;
    config.send_low_water_bytes = 4;
    let mut peer = connected_peer(config);
    let channel_id = ChannelId(21);
    peer.submit(Command::OpenDataChannel {
        operation_id: OperationId(4),
        channel_id,
    })
    .expect("open admitted");
    assert_eq!(next(&mut peer), Event::DataChannelOpened { channel_id });
    let _ = next(&mut peer);
    peer.submit(Command::OpenDataChannel {
        operation_id: OperationId(5),
        channel_id,
    })
    .expect("same channel open is idempotent");
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(5),
        }
    );
    peer.submit(Command::OpenDataChannel {
        operation_id: OperationId(6),
        channel_id: ChannelId(22),
    })
    .expect("different open is correlated");
    assert_eq!(
        next(&mut peer),
        Event::OperationFailed {
            operation_id: OperationId(6),
            error: TransportError::InvalidState,
        }
    );

    for operation_id in [7, 8] {
        peer.submit(Command::Send {
            operation_id: OperationId(operation_id),
            channel_id,
            payload: BinaryPayload::new(vec![operation_id as u8; 3]).expect("bounded"),
        })
        .expect("send admitted");
        let _ = next(&mut peer);
    }
    let _ = peer.drain_provider_send().expect("first FIFO send");
    assert!(matches!(next(&mut peer), Event::SendCapacity { .. }));
    peer.submit(Command::Send {
        operation_id: OperationId(9),
        channel_id,
        payload: BinaryPayload::new(vec![9; 3]).expect("bounded"),
    })
    .expect("capacity edge permits retry");
    let _ = next(&mut peer);
    let _ = peer.drain_provider_send().expect("second FIFO send");
    assert!(matches!(next(&mut peer), Event::SendCapacity { .. }));
}

#[test]
fn answerer_restart_generates_fresh_bounded_answer_credentials() {
    let mut peer = FakeNativeTransportProvider
        .create_fake_peer(PeerConfig::answerer().validate().expect("valid"))
        .expect("fake");
    let baseline = NegotiationEpoch(1);
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(1),
        description: SessionDescription::new(baseline, DescriptionKind::Offer, "baseline offer")
            .expect("bounded"),
    })
    .expect("offer admitted");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Negotiating,
        }
    );
    let _ = next(&mut peer);
    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(2),
        description: SessionDescription::new(baseline, DescriptionKind::Answer, "baseline answer")
            .expect("bounded"),
    })
    .expect("answer admitted");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Connecting,
        }
    );
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Connected,
        }
    );
    let _ = next(&mut peer);

    let restart = NegotiationEpoch(2);
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(3),
        description: SessionDescription::new(restart, DescriptionKind::Offer, "restart offer")
            .expect("bounded"),
    })
    .expect("restart offer admitted");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Restarting,
        }
    );
    let _ = next(&mut peer);
    peer.submit(Command::RestartIce {
        operation_id: OperationId(4),
        epoch: restart,
    })
    .expect("answerer restart admitted");
    let answer = match next(&mut peer) {
        Event::LocalDescription { description } => description,
        other => panic!("expected restart answer, got {other:?}"),
    };
    assert_eq!(answer.kind(), DescriptionKind::Answer);
    assert!(answer.sdp().contains("native-restart-v2"));
    let candidate = match next(&mut peer) {
        Event::LocalCandidate { candidate } => candidate,
        other => panic!("expected restart candidate, got {other:?}"),
    };
    assert_eq!(candidate.username_fragment(), Some("native-restart-v2"));
    assert!(matches!(
        next(&mut peer),
        Event::LocalCandidatesEnded { .. }
    ));
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(4),
        }
    );
}

#[test]
fn ice_count_credential_and_redaction_boundaries_are_checked() {
    let maximum_credential = "x".repeat(relay_transport::MAX_ICE_TEXT_BYTES);
    assert!(TurnCredentials::new(&maximum_credential, &maximum_credential).is_ok());
    assert_eq!(
        TurnCredentials::new(
            "x".repeat(relay_transport::MAX_ICE_TEXT_BYTES + 1),
            "secret"
        ),
        Err(TransportError::InvalidIceServer),
    );

    let mut config = PeerConfig::offerer();
    for index in 0..relay_transport::MAX_ICE_SERVERS {
        config.ice_servers.push(
            IceServer::stun(format!("s{index}.example"), 3478, IceTransport::Udp)
                .expect("unique bounded server"),
        );
    }
    assert!(config.validate().is_ok());
    config.ice_servers.push(
        IceServer::stun("overflow.example", 3478, IceTransport::Udp).expect("bounded server"),
    );
    assert_eq!(config.validate(), Err(TransportError::InvalidIceServer));

    let credentials =
        TurnCredentials::new("visible-user", "never-print-this-secret").expect("credentials");
    let server = IceServer::turn("turn.example", 3478, IceTransport::Udp, credentials, None)
        .expect("TURN server");
    let mut redacted = PeerConfig::offerer();
    redacted.ice_servers.push(server);
    let validated = redacted.validate().expect("valid");
    for debug in [format!("{redacted:?}"), format!("{validated:?}")] {
        assert!(!debug.contains("visible-user"));
        assert!(!debug.contains("never-print-this-secret"));
        assert!(debug.contains("<redacted>"));
    }
}

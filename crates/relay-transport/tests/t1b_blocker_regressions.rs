//! Regressions for the T1b adversarial-review blockers.

use core::task::{Context, Poll};
use std::task::Waker;

use relay_transport::{
    BinaryPayload, ChannelId, Command, DescriptionKind, Event, FakeNativeTransportProvider,
    IceCandidate, IceServer, IceTransport, NegotiationEpoch, OperationId, PeerConfig, PeerDriver,
    PeerState, SessionDescription, TlsTrust, TransportError, TurnTlsConfig,
};

fn next(peer: &mut impl PeerDriver) -> Event {
    let mut context = Context::from_waker(Waker::noop());
    match peer.poll_event(&mut context) {
        Poll::Ready(Some(event)) => event,
        other => panic!("expected event, got {other:?}"),
    }
}

fn connected_peer() -> relay_transport::FakePeer {
    let mut peer = FakeNativeTransportProvider
        .create_fake_peer(PeerConfig::offerer().validate().expect("valid config"))
        .expect("fake peer");
    let epoch = NegotiationEpoch(1);
    peer.submit(Command::CreateOffer {
        operation_id: OperationId(1),
        epoch,
    })
    .expect("offer admitted");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Negotiating,
        }
    );
    let local = match next(&mut peer) {
        Event::LocalDescription { description } => description,
        other => panic!("expected local description, got {other:?}"),
    };
    let _ = next(&mut peer);
    let _ = next(&mut peer);
    let _ = next(&mut peer);
    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(2),
        description: local,
    })
    .expect("local description admitted");
    let _ = next(&mut peer);
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(3),
        description: SessionDescription::new(epoch, DescriptionKind::Answer, "remote answer")
            .expect("bounded answer"),
    })
    .expect("remote description admitted");
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
    peer
}

#[test]
fn public_injection_cannot_forge_terminals_shutdown_or_post_fatal_progress() {
    let mut peer = FakeNativeTransportProvider
        .create_fake_peer(PeerConfig::offerer().validate().expect("valid config"))
        .expect("fake peer");
    peer.submit(Command::RequestStats {
        operation_id: OperationId(1),
    })
    .expect("operation admitted");
    assert_eq!(
        peer.inject_provider_event(Event::OperationCompleted {
            operation_id: OperationId(1),
        }),
        Err(TransportError::InvalidState),
    );
    assert_eq!(
        peer.inject_provider_event(Event::StateChanged {
            state: PeerState::Closed,
        }),
        Err(TransportError::InvalidState),
    );
    assert!(matches!(next(&mut peer), Event::Stats { .. }));
    assert_eq!(
        next(&mut peer),
        Event::OperationCompleted {
            operation_id: OperationId(1),
        }
    );
    peer.inject_provider_drop().expect("fatal fact accepted");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Failed,
        }
    );
    assert!(matches!(next(&mut peer), Event::FatalError { .. }));
    assert_eq!(
        peer.inject_provider_event(Event::StateChanged {
            state: PeerState::Connected,
        }),
        Err(TransportError::ProviderFailure),
    );
}

#[test]
fn restart_ice_changes_credentials_and_rejects_stale_generation_data() {
    let mut peer = connected_peer();
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
        other => panic!("expected description, got {other:?}"),
    };
    let candidate = match next(&mut peer) {
        Event::LocalCandidate { candidate } => candidate,
        other => panic!("expected candidate, got {other:?}"),
    };
    let end_ufrag = match next(&mut peer) {
        Event::LocalCandidatesEnded { end } => end.username_fragment().map(str::to_owned),
        other => panic!("expected end marker, got {other:?}"),
    };
    assert_ne!(
        description.sdp(),
        "v=0\r\no=- 20002 1 IN IP4 127.0.0.1\r\ns=RELAY native offer fixture\r\nt=0 0\r\na=ice-options:trickle\r\na=ice-ufrag:native-base-v1\r\na=setup:actpass\r\n"
    );
    assert_ne!(candidate.username_fragment(), Some("native-base-v1"));
    assert_eq!(
        end_ufrag.as_deref(),
        candidate.username_fragment(),
        "all restart carriers share the new ICE credential"
    );
    let _ = next(&mut peer);
    peer.submit(Command::AddRemoteCandidate {
        operation_id: OperationId(5),
        candidate: IceCandidate::new(
            NegotiationEpoch(1),
            "candidate:1 1 UDP 1 127.0.0.1 9 typ host",
            None,
            None,
            Some("native-base-v1".to_owned()),
        )
        .expect("bounded candidate"),
    })
    .expect("stale input is correlated");
    assert_eq!(
        next(&mut peer),
        Event::OperationFailed {
            operation_id: OperationId(5),
            error: TransportError::StaleEpoch,
        }
    );
}

#[test]
fn lifecycle_includes_disconnect_recovery_and_close_states() {
    let mut peer = connected_peer();
    peer.inject_disconnect().expect("disconnect accepted");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Disconnected,
        }
    );
    peer.inject_recovery().expect("recovery accepted");
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
    peer.submit(Command::Shutdown {
        operation_id: OperationId(4),
    })
    .expect("shutdown admitted");
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Closing,
        }
    );
    let _ = next(&mut peer);
    assert_eq!(
        next(&mut peer),
        Event::StateChanged {
            state: PeerState::Closed,
        }
    );
    assert_eq!(next(&mut peer), Event::ShutdownComplete);
}

#[test]
fn operation_timeout_and_active_drop_are_bounded_and_terminal() {
    let mut config = PeerConfig::offerer();
    config.operation_timeout_ms = 5;
    let mut peer = FakeNativeTransportProvider
        .create_fake_peer(config.validate().expect("valid config"))
        .expect("fake peer");
    peer.inject_operation_stall().expect("stall installed");
    peer.submit(Command::RequestStats {
        operation_id: OperationId(1),
    })
    .expect("operation admitted");
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(peer.poll_event(&mut context), Poll::Pending);
    peer.advance_time(5);
    assert_eq!(
        next(&mut peer),
        Event::OperationFailed {
            operation_id: OperationId(1),
            error: TransportError::OperationTimeout,
        }
    );
    assert_eq!(peer.poll_event(&mut context), Poll::Pending);

    let drop_probe = peer.drop_probe();
    let teardown_probe = peer.teardown_probe();
    drop(peer);
    assert!(drop_probe.was_dropped());
    assert!(teardown_probe.was_torn_down());
}

#[test]
fn dns_host_rejects_nul_backslash_and_ambiguous_text() {
    for host in [
        "bad\0host",
        r"bad\host",
        "bad_name",
        "bad host",
        "-bad.example",
    ] {
        assert_eq!(
            IceServer::stun(host, 3478, IceTransport::Udp),
            Err(TransportError::InvalidIceServer),
            "host {host:?} must fail closed",
        );
    }
    assert!(IceServer::stun("127.0.0.1", 3478, IceTransport::Udp).is_ok());
    assert!(IceServer::stun("stun.example", 3478, IceTransport::Tcp).is_ok());
}

#[test]
fn custom_trust_anchor_rejects_openssl_rejected_der_and_accepts_a_certificate() {
    let rejected: [&[u8]; 7] = [
        &[1, 2, 3],
        &[0x30, 0x01, 0],
        include_bytes!("fixtures/openssl-rejected-empty-oid.der"),
        include_bytes!("fixtures/openssl-rejected-malformed-length.der"),
        include_bytes!("fixtures/openssl-rejected-indefinite-length.der"),
        include_bytes!("fixtures/openssl-rejected-trailing-data.der"),
        // Valid outer lengths, but the TBS value is not a TBSCertificate.
        &[
            0x30, 0x0e, 0x30, 0x03, 0x02, 0x01, 0x01, 0x30, 0x03, 0x06, 0x01, 0x2a, 0x03, 0x02,
            0x00, 0x00,
        ],
    ];
    for bogus in rejected {
        assert_eq!(
            TurnTlsConfig::new("turn.example", TlsTrust::Custom(vec![bogus.to_vec()])),
            Err(TransportError::InvalidTlsTrust),
            "OpenSSL-rejected input must fail closed: {bogus:02x?}",
        );
    }

    let certificate = include_bytes!("fixtures/minimal-ed25519-cert.der").to_vec();
    assert!(
        TurnTlsConfig::new("turn.example", TlsTrust::Custom(vec![certificate.clone()])).is_ok()
    );

    // Regression: the issuer commonName AttributeValue at exact DER offset 52 is a
    // UTF8String. Retagging it as INTEGER must not pass the custom-anchor parser.
    let mut integer_attribute_value = certificate;
    assert_eq!(integer_attribute_value[52], 0x0c);
    integer_attribute_value[52] = 0x02;
    assert_eq!(
        TurnTlsConfig::new(
            "turn.example",
            TlsTrust::Custom(vec![integer_attribute_value]),
        ),
        Err(TransportError::InvalidTlsTrust),
    );
}

#[test]
fn legal_message_callback_stays_bounded() {
    let mut peer = connected_peer();
    let channel_id = ChannelId(2);
    peer.submit(Command::OpenDataChannel {
        operation_id: OperationId(4),
        channel_id,
    })
    .expect("open admitted");
    let _ = next(&mut peer);
    let _ = next(&mut peer);
    peer.inject_provider_event(Event::Message {
        channel_id,
        payload: BinaryPayload::new(vec![1, 2, 3]).expect("bounded"),
    })
    .expect("legal callback admitted");
    assert!(matches!(next(&mut peer), Event::Message { .. }));
}

#[test]
fn queued_operation_times_out_at_its_absolute_deadline_without_executing() {
    let mut config = PeerConfig::offerer();
    config.operation_timeout_ms = 5;
    let mut peer = FakeNativeTransportProvider
        .create_fake_peer(config.validate().expect("valid config"))
        .expect("fake peer");
    peer.inject_operation_stall().expect("stall installed");
    for operation_id in [OperationId(1), OperationId(2)] {
        peer.submit(Command::RequestStats { operation_id })
            .expect("operation admitted");
    }

    peer.advance_time(5);
    for operation_id in [OperationId(1), OperationId(2)] {
        assert_eq!(
            next(&mut peer),
            Event::OperationFailed {
                operation_id,
                error: TransportError::OperationTimeout,
            }
        );
    }
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(peer.poll_event(&mut context), Poll::Pending);
}

#[test]
fn elapsed_operation_timeout_precedes_later_provider_loss() {
    let mut config = PeerConfig::offerer();
    config.operation_timeout_ms = 5;
    let mut peer = FakeNativeTransportProvider
        .create_fake_peer(config.validate().expect("valid config"))
        .expect("fake peer");
    peer.inject_operation_stall().expect("stall installed");
    peer.submit(Command::RequestStats {
        operation_id: OperationId(1),
    })
    .expect("operation admitted");

    peer.advance_time(5);
    peer.inject_provider_drop()
        .expect("later provider loss accepted");
    assert_eq!(
        next(&mut peer),
        Event::OperationFailed {
            operation_id: OperationId(1),
            error: TransportError::OperationTimeout,
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
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(peer.poll_event(&mut context), Poll::Pending);
}

#[test]
fn one_clock_advance_expires_all_due_operations_once_in_acceptance_order() {
    let mut config = PeerConfig::offerer();
    config.operation_timeout_ms = 5;
    config.event_capacity = 5;
    let mut peer = FakeNativeTransportProvider
        .create_fake_peer(config.validate().expect("valid config"))
        .expect("fake peer");
    peer.inject_operation_stall().expect("stall installed");
    peer.submit(Command::RequestStats {
        operation_id: OperationId(1),
    })
    .expect("first operation admitted");
    peer.advance_time(1);
    for operation_id in [OperationId(2), OperationId(3), OperationId(4)] {
        peer.submit(Command::RequestStats { operation_id })
            .expect("queued operation admitted");
    }

    peer.advance_time(5);
    for operation_id in [
        OperationId(1),
        OperationId(2),
        OperationId(3),
        OperationId(4),
    ] {
        assert_eq!(
            next(&mut peer),
            Event::OperationFailed {
                operation_id,
                error: TransportError::OperationTimeout,
            }
        );
    }
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(peer.poll_event(&mut context), Poll::Pending);
}

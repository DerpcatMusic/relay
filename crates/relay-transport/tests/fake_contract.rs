use core::task::{Context, Poll};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Wake, Waker};

use prost::Message;
use relay_protocol::v1::{Envelope, IceCandidate as WireIceCandidate, envelope};
use relay_transport::{
    Command, DescriptionKind, EndOfCandidates, Event, FakeNativeTransportProvider, IceCandidate,
    NativeTransportProvider, NegotiationEpoch, OperationId, PeerConfig, PeerDriver, PeerState,
    SessionDescription, SubmitError, TransportError,
};

const BROWSER_OFFER_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/browser-offer-v1.bin");
const BROWSER_ANSWER_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/browser-answer-v1.bin");
const NATIVE_OFFER_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/native-offer-v1.bin");
const NATIVE_ANSWER_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/native-answer-v1.bin");
const BROWSER_RESTART_OFFER_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/browser-ice-restart-offer-v1.bin");
const BROWSER_RESTART_ANSWER_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/browser-ice-restart-answer-v1.bin");
const NATIVE_RESTART_OFFER_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/native-ice-restart-offer-v1.bin");
const NATIVE_RESTART_ANSWER_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/native-ice-restart-answer-v1.bin");
const BROWSER_TRICKLE_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/browser-trickle-candidate-v1.bin");
const NATIVE_TRICKLE_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/native-trickle-candidate-v1.bin");
const BROWSER_END_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/browser-end-of-candidates-v1.bin");
const NATIVE_END_V1: &[u8] =
    include_bytes!("../../../tests/fixtures/transport/v1/native-end-of-candidates-v1.bin");

#[derive(Default)]
struct CountingWake {
    wakes: AtomicUsize,
}

impl CountingWake {
    fn count(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }
}

impl Wake for CountingWake {
    fn wake(self: Arc<Self>) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
    }
}

fn create_peer(config: PeerConfig) -> Box<dyn PeerDriver> {
    FakeNativeTransportProvider
        .create_peer(config.validate().expect("test configuration is valid"))
        .expect("fake construction is infallible")
}

fn next_event(peer: &mut dyn PeerDriver) -> Event {
    let waker = std::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    match peer.poll_event(&mut context) {
        Poll::Ready(Some(event)) => event,
        other => panic!("expected an event, got {other:?}"),
    }
}

fn assert_completed(event: Event, operation_id: u64) {
    assert_eq!(
        event,
        Event::OperationCompleted {
            operation_id: OperationId(operation_id),
        }
    );
}

fn assert_failed(event: Event, operation_id: u64, error: TransportError) {
    assert_eq!(
        event,
        Event::OperationFailed {
            operation_id: OperationId(operation_id),
            error,
        }
    );
}

fn rejected_command(result: Result<(), SubmitError>, expected_error: TransportError) -> Command {
    let rejection = result.expect_err("command should be rejected before admission");
    assert_eq!(rejection.error(), expected_error);
    let (error, command) = rejection.into_parts();
    assert_eq!(error, expected_error);
    command
}

fn decode_fixture(bytes: &[u8]) -> Envelope {
    let envelope = Envelope::decode(bytes).expect("SHA-frozen V1 fixture must decode");
    assert_eq!(
        envelope.version.as_ref().map(|version| version.major),
        Some(1),
    );
    envelope
}

fn fixture_description(
    bytes: &[u8],
    epoch: NegotiationEpoch,
    expected_kind: DescriptionKind,
) -> SessionDescription {
    let envelope = decode_fixture(bytes);
    let (kind, target_peer_id, sdp) = match envelope.payload {
        Some(envelope::Payload::Offer(offer)) => {
            (DescriptionKind::Offer, offer.target_peer_id, offer.sdp)
        }
        Some(envelope::Payload::Answer(answer)) => {
            (DescriptionKind::Answer, answer.target_peer_id, answer.sdp)
        }
        other => panic!("expected offer/answer fixture, got {other:?}"),
    };
    assert_eq!(kind, expected_kind);
    assert!(
        !target_peer_id.is_empty(),
        "V1 routing identity stays outside the transport value",
    );
    let description =
        SessionDescription::new(epoch, kind, sdp.clone()).expect("fixture SDP is bounded");
    assert_eq!(description.epoch(), epoch);
    assert_eq!(description.kind(), kind);
    assert_eq!(description.sdp(), sdp);
    description
}

fn wire_candidate(bytes: &[u8]) -> WireIceCandidate {
    let envelope = decode_fixture(bytes);
    let Some(envelope::Payload::IceCandidate(candidate)) = envelope.payload else {
        panic!("expected ICE-candidate fixture");
    };
    assert!(
        !candidate.target_peer_id.is_empty(),
        "V1 routing identity stays outside the transport value",
    );
    candidate
}

fn fixture_candidate(bytes: &[u8], epoch: NegotiationEpoch) -> IceCandidate {
    let wire = wire_candidate(bytes);
    assert!(!wire.end_of_candidates);
    assert!(!wire.candidate.is_empty());
    let mline = wire
        .sdp_mline_index
        .map(|index| u16::try_from(index).expect("fixture m-line index fits transport bound"));
    let candidate = IceCandidate::new(
        epoch,
        wire.candidate.clone(),
        wire.sdp_mid.clone(),
        mline,
        wire.username_fragment.clone(),
    )
    .expect("fixture candidate is bounded");
    assert_candidate_identity(&candidate, &wire, epoch);
    candidate
}

fn fixture_end(bytes: &[u8], epoch: NegotiationEpoch) -> EndOfCandidates {
    let wire = wire_candidate(bytes);
    assert!(wire.end_of_candidates);
    assert!(wire.candidate.is_empty());
    let mline = wire
        .sdp_mline_index
        .map(|index| u16::try_from(index).expect("fixture m-line index fits transport bound"));
    let end = EndOfCandidates::new(
        epoch,
        wire.sdp_mid.clone(),
        mline,
        wire.username_fragment.clone(),
    )
    .expect("fixture end marker is bounded");
    assert_end_identity(&end, &wire, epoch);
    end
}

fn assert_candidate_identity(
    candidate: &IceCandidate,
    wire: &WireIceCandidate,
    epoch: NegotiationEpoch,
) {
    assert!(!wire.end_of_candidates);
    assert_eq!(candidate.epoch(), epoch);
    assert_eq!(candidate.candidate(), wire.candidate);
    assert_eq!(candidate.sdp_mid(), wire.sdp_mid.as_deref());
    assert_eq!(
        candidate.sdp_mline_index().map(u32::from),
        wire.sdp_mline_index,
    );
    assert_eq!(
        candidate.username_fragment(),
        wire.username_fragment.as_deref(),
    );
}

fn assert_end_identity(end: &EndOfCandidates, wire: &WireIceCandidate, epoch: NegotiationEpoch) {
    assert!(wire.end_of_candidates);
    assert!(wire.candidate.is_empty());
    assert_eq!(end.epoch(), epoch);
    assert_eq!(end.sdp_mid(), wire.sdp_mid.as_deref());
    assert_eq!(end.sdp_mline_index().map(u32::from), wire.sdp_mline_index,);
    assert_eq!(end.username_fragment(), wire.username_fragment.as_deref());
}

fn local_generation(
    peer: &mut dyn PeerDriver,
    kind: DescriptionKind,
    epoch: NegotiationEpoch,
    operation_id: u64,
    expected_state: PeerState,
    frozen_identity: bool,
) {
    assert_eq!(
        next_event(peer),
        Event::StateChanged {
            state: expected_state,
        }
    );
    let description = match next_event(peer) {
        Event::LocalDescription { description } => description,
        other => panic!("expected local description, got {other:?}"),
    };
    assert_eq!(description.kind(), kind);
    assert_eq!(description.epoch(), epoch);

    let candidate = match next_event(peer) {
        Event::LocalCandidate { candidate } => candidate,
        other => panic!("expected local candidate, got {other:?}"),
    };
    if frozen_identity {
        assert_candidate_identity(&candidate, &wire_candidate(NATIVE_TRICKLE_V1), epoch);
    } else {
        assert_eq!(candidate.epoch(), epoch);
        assert!(candidate.candidate().contains(&epoch.0.to_string()));
        assert!(
            candidate
                .username_fragment()
                .is_some_and(|fragment| fragment.contains(&epoch.0.to_string()))
        );
    }
    let end = match next_event(peer) {
        Event::LocalCandidatesEnded { end } => end,
        other => panic!("expected local end marker, got {other:?}"),
    };
    if frozen_identity {
        assert_end_identity(&end, &wire_candidate(NATIVE_END_V1), epoch);
    } else {
        assert_eq!(end.epoch(), epoch);
        assert_eq!(end.username_fragment(), candidate.username_fragment());
    }
    assert_completed(next_event(peer), operation_id);
}

#[test]
fn frozen_v1_payloads_map_every_transport_field_into_bounded_commands() {
    let descriptions = [
        (BROWSER_OFFER_V1, DescriptionKind::Offer),
        (NATIVE_ANSWER_V1, DescriptionKind::Answer),
        (NATIVE_OFFER_V1, DescriptionKind::Offer),
        (BROWSER_ANSWER_V1, DescriptionKind::Answer),
        (BROWSER_RESTART_OFFER_V1, DescriptionKind::Offer),
        (NATIVE_RESTART_ANSWER_V1, DescriptionKind::Answer),
        (NATIVE_RESTART_OFFER_V1, DescriptionKind::Offer),
        (BROWSER_RESTART_ANSWER_V1, DescriptionKind::Answer),
    ];
    for (index, (bytes, kind)) in descriptions.into_iter().enumerate() {
        let epoch = NegotiationEpoch(u64::try_from(index + 1).expect("small fixture index"));
        let description = fixture_description(bytes, epoch, kind);
        let command = if index % 2 == 0 {
            Command::SetRemoteDescription {
                operation_id: OperationId(epoch.0),
                description,
            }
        } else {
            Command::SetLocalDescription {
                operation_id: OperationId(epoch.0),
                description,
            }
        };
        let mapped = match command {
            Command::SetLocalDescription { description, .. }
            | Command::SetRemoteDescription { description, .. } => description,
            other => panic!("expected mapped description command, got {other:?}"),
        };
        assert_eq!(mapped, fixture_description(bytes, epoch, kind));
    }

    for (index, bytes) in [BROWSER_TRICKLE_V1, NATIVE_TRICKLE_V1]
        .into_iter()
        .enumerate()
    {
        let epoch = NegotiationEpoch(u64::try_from(index + 20).expect("small fixture index"));
        let command = Command::AddRemoteCandidate {
            operation_id: OperationId(epoch.0),
            candidate: fixture_candidate(bytes, epoch),
        };
        let Command::AddRemoteCandidate { candidate, .. } = command else {
            unreachable!("constructed candidate command")
        };
        assert_candidate_identity(&candidate, &wire_candidate(bytes), epoch);
    }

    for (index, bytes) in [BROWSER_END_V1, NATIVE_END_V1].into_iter().enumerate() {
        let epoch = NegotiationEpoch(u64::try_from(index + 30).expect("small fixture index"));
        let command = Command::EndRemoteCandidates {
            operation_id: OperationId(epoch.0),
            end: fixture_end(bytes, epoch),
        };
        let Command::EndRemoteCandidates { end, .. } = command else {
            unreachable!("constructed end-marker command")
        };
        assert_end_identity(&end, &wire_candidate(bytes), epoch);
    }
}

#[test]
fn offerer_baseline_drives_frozen_v1_fields_and_exact_operation_terminals() {
    let mut peer = create_peer(PeerConfig::offerer());
    let epoch = NegotiationEpoch(1);
    peer.submit(Command::CreateOffer {
        operation_id: OperationId(1),
        epoch,
    })
    .expect("create offer is accepted");
    local_generation(
        &mut *peer,
        DescriptionKind::Offer,
        epoch,
        1,
        PeerState::Negotiating,
        true,
    );

    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(2),
        description: fixture_description(NATIVE_OFFER_V1, epoch, DescriptionKind::Offer),
    })
    .expect("fixture-mapped local offer is accepted");
    assert_completed(next_event(&mut *peer), 2);

    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(3),
        description: fixture_description(BROWSER_ANSWER_V1, epoch, DescriptionKind::Answer),
    })
    .expect("fixture-mapped remote answer is accepted");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connecting,
        }
    );
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connected,
        }
    );
    assert_completed(next_event(&mut *peer), 3);

    peer.submit(Command::AddRemoteCandidate {
        operation_id: OperationId(4),
        candidate: fixture_candidate(BROWSER_TRICKLE_V1, epoch),
    })
    .expect("fixture-mapped candidate is accepted");
    assert_completed(next_event(&mut *peer), 4);

    peer.submit(Command::EndRemoteCandidates {
        operation_id: OperationId(5),
        end: fixture_end(BROWSER_END_V1, epoch),
    })
    .expect("fixture-mapped end marker is accepted");
    assert_completed(next_event(&mut *peer), 5);
}

#[test]
fn answerer_baseline_drives_frozen_v1_fields_before_creating_answer() {
    let mut peer = create_peer(PeerConfig::answerer());
    let epoch = NegotiationEpoch(1);
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(1),
        description: fixture_description(BROWSER_OFFER_V1, epoch, DescriptionKind::Offer),
    })
    .expect("fixture-mapped remote offer is accepted");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Negotiating,
        }
    );
    assert_completed(next_event(&mut *peer), 1);

    peer.submit(Command::AddRemoteCandidate {
        operation_id: OperationId(2),
        candidate: fixture_candidate(BROWSER_TRICKLE_V1, epoch),
    })
    .expect("fixture-mapped candidate is accepted");
    assert_completed(next_event(&mut *peer), 2);
    peer.submit(Command::EndRemoteCandidates {
        operation_id: OperationId(3),
        end: fixture_end(BROWSER_END_V1, epoch),
    })
    .expect("fixture-mapped end marker is accepted");
    assert_completed(next_event(&mut *peer), 3);

    peer.submit(Command::CreateAnswer {
        operation_id: OperationId(4),
        epoch,
    })
    .expect("create answer is accepted");
    let description = match next_event(&mut *peer) {
        Event::LocalDescription { description } => description,
        other => panic!("expected local description, got {other:?}"),
    };
    assert_eq!(description.kind(), DescriptionKind::Answer);
    assert_eq!(description.epoch(), epoch);
    let candidate = match next_event(&mut *peer) {
        Event::LocalCandidate { candidate } => candidate,
        other => panic!("expected local candidate, got {other:?}"),
    };
    assert_candidate_identity(&candidate, &wire_candidate(NATIVE_TRICKLE_V1), epoch);
    let end = match next_event(&mut *peer) {
        Event::LocalCandidatesEnded { end } => end,
        other => panic!("expected local end marker, got {other:?}"),
    };
    assert_end_identity(&end, &wire_candidate(NATIVE_END_V1), epoch);
    assert_completed(next_event(&mut *peer), 4);

    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(5),
        description: fixture_description(NATIVE_ANSWER_V1, epoch, DescriptionKind::Answer),
    })
    .expect("fixture-mapped local answer is accepted");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connecting,
        }
    );
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connected,
        }
    );
    assert_completed(next_event(&mut *peer), 5);
}

#[test]
fn command_queue_full_transfers_nothing_and_allows_retry() {
    let mut config = PeerConfig::offerer();
    config.command_capacity = 1;
    let mut peer = create_peer(config);
    peer.submit(Command::CreateOffer {
        operation_id: OperationId(1),
        epoch: NegotiationEpoch(1),
    })
    .expect("first command fits");
    let second = Command::CreateOffer {
        operation_id: OperationId(2),
        epoch: NegotiationEpoch(2),
    };
    let rejection = peer.submit(second).expect_err("the command queue is full");
    assert_eq!(rejection.error(), TransportError::QueueFull);
    assert_eq!(
        rejection.command().operation_id(),
        OperationId(2),
        "the rejected command remains available by reference",
    );
    let (error, second) = rejection.into_parts();
    assert_eq!(error, TransportError::QueueFull);

    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Negotiating,
        }
    );
    peer.submit(second)
        .expect("the returned command retries without cloning");

    let drained: Vec<_> = (0..8).map(|_| next_event(&mut *peer)).collect();
    assert!(matches!(drained[0], Event::LocalDescription { .. }));
    assert!(matches!(drained[1], Event::LocalCandidate { .. }));
    assert!(matches!(drained[2], Event::LocalCandidatesEnded { .. }));
    assert_eq!(
        drained[3],
        Event::OperationCompleted {
            operation_id: OperationId(1),
        }
    );
    assert!(matches!(drained[4], Event::LocalDescription { .. }));
    assert!(matches!(drained[5], Event::LocalCandidate { .. }));
    assert!(matches!(drained[6], Event::LocalCandidatesEnded { .. }));
    assert_eq!(
        drained[7],
        Event::OperationCompleted {
            operation_id: OperationId(2),
        },
        "the exact returned command drains through its own terminal",
    );
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert_eq!(peer.poll_event(&mut context), Poll::Pending);
}

#[test]
fn duplicate_operation_is_rejected_without_a_second_terminal() {
    let mut peer = create_peer(PeerConfig::offerer());
    let command = Command::CreateOffer {
        operation_id: OperationId(7),
        epoch: NegotiationEpoch(1),
    };
    peer.submit(command.clone()).expect("first use is accepted");
    let rejected = rejected_command(
        peer.submit(command.clone()),
        TransportError::DuplicateOperation,
    );
    assert_eq!(rejected, command);
    for _ in 0..5 {
        let _ = next_event(&mut *peer);
    }
    let rejected = rejected_command(
        peer.submit(command.clone()),
        TransportError::DuplicateOperation,
    );
    assert_eq!(rejected, command);

    let waker = std::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    assert_eq!(peer.poll_event(&mut context), Poll::Pending);
}

#[test]
fn stale_remote_epoch_is_an_operation_failure_not_silent_input() {
    let mut peer = create_peer(PeerConfig::answerer());
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(1),
        description: fixture_description(
            BROWSER_RESTART_OFFER_V1,
            NegotiationEpoch(2),
            DescriptionKind::Offer,
        ),
    })
    .expect("new epoch is accepted");
    let _ = next_event(&mut *peer);
    assert_completed(next_event(&mut *peer), 1);

    peer.submit(Command::AddRemoteCandidate {
        operation_id: OperationId(2),
        candidate: fixture_candidate(BROWSER_TRICKLE_V1, NegotiationEpoch(1)),
    })
    .expect("well-sized command is accepted for correlated failure");
    assert_eq!(
        next_event(&mut *peer),
        Event::OperationFailed {
            operation_id: OperationId(2),
            error: TransportError::StaleEpoch,
        }
    );
}

#[test]
fn configured_text_caps_fail_accepted_operations_with_stable_errors() {
    let mut config = PeerConfig::offerer();
    config.max_sdp_bytes = 8;
    let mut peer = create_peer(config);
    peer.submit(Command::CreateOffer {
        operation_id: OperationId(1),
        epoch: NegotiationEpoch(1),
    })
    .expect("operation is accepted");
    assert_eq!(
        next_event(&mut *peer),
        Event::OperationFailed {
            operation_id: OperationId(1),
            error: TransportError::SdpTooLarge,
        }
    );
}

#[test]
fn configured_input_caps_return_commands_without_admission_or_terminal_events() {
    let mut sdp_config = PeerConfig::offerer();
    sdp_config.max_sdp_bytes = 8;
    let mut sdp_peer = create_peer(sdp_config);
    let sdp_command = Command::SetLocalDescription {
        operation_id: OperationId(1),
        description: SessionDescription::new(
            NegotiationEpoch(1),
            DescriptionKind::Offer,
            "123456789",
        )
        .expect("the value is within the absolute cap"),
    };
    let returned = rejected_command(sdp_peer.submit(sdp_command), TransportError::SdpTooLarge);
    match returned {
        Command::SetLocalDescription {
            operation_id,
            description,
        } => {
            assert_eq!(operation_id, OperationId(1));
            assert_eq!(description.sdp(), "123456789");
        }
        other => panic!("expected the original SDP command, got {other:?}"),
    }

    let waker = std::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    assert_eq!(sdp_peer.poll_event(&mut context), Poll::Pending);

    let mut candidate_config = PeerConfig::offerer();
    candidate_config.max_candidate_bytes = 8;
    let mut candidate_peer = create_peer(candidate_config);
    let candidate_command = Command::AddRemoteCandidate {
        operation_id: OperationId(2),
        candidate: IceCandidate::new(NegotiationEpoch(1), "123456789", None, None, None)
            .expect("the value is within the absolute cap"),
    };
    let returned = rejected_command(
        candidate_peer.submit(candidate_command),
        TransportError::CandidateTooLarge,
    );
    match returned {
        Command::AddRemoteCandidate {
            operation_id,
            candidate,
        } => {
            assert_eq!(operation_id, OperationId(2));
            assert_eq!(candidate.candidate(), "123456789");
        }
        other => panic!("expected the original candidate command, got {other:?}"),
    }
    assert_eq!(candidate_peer.poll_event(&mut context), Poll::Pending);
}

#[test]
fn validated_configuration_rejects_unbounded_or_unserviceable_capacities() {
    let mut config = PeerConfig::offerer();
    config.event_capacity = 4;
    assert_eq!(config.validate(), Err(TransportError::InvalidEventCapacity));
    config.event_capacity = 5;
    config.command_capacity = 0;
    assert_eq!(
        config.validate(),
        Err(TransportError::InvalidCommandCapacity)
    );
}

#[test]
fn maximum_five_event_batch_fits_exact_event_capacity_without_overflow() {
    let mut config = PeerConfig::offerer();
    config.event_capacity = 5;
    let mut peer = create_peer(config);
    let epoch = NegotiationEpoch(1);
    peer.submit(Command::CreateOffer {
        operation_id: OperationId(1),
        epoch,
    })
    .expect("maximum batch is admitted at exact capacity");

    let events: Vec<_> = (0..5).map(|_| next_event(&mut *peer)).collect();
    assert_eq!(
        events[0],
        Event::StateChanged {
            state: PeerState::Negotiating,
        }
    );
    let Event::LocalDescription { description } = &events[1] else {
        panic!("expected local description, got {:?}", events[1]);
    };
    assert_eq!(description.epoch(), epoch);
    assert_eq!(description.kind(), DescriptionKind::Offer);
    let Event::LocalCandidate { candidate } = &events[2] else {
        panic!("expected local candidate, got {:?}", events[2]);
    };
    assert_candidate_identity(candidate, &wire_candidate(NATIVE_TRICKLE_V1), epoch);
    let Event::LocalCandidatesEnded { end } = &events[3] else {
        panic!("expected local end marker, got {:?}", events[3]);
    };
    assert_end_identity(end, &wire_candidate(NATIVE_END_V1), epoch);
    assert_eq!(
        events[4],
        Event::OperationCompleted {
            operation_id: OperationId(1),
        }
    );

    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert_eq!(peer.poll_event(&mut context), Poll::Pending);
}

#[test]
fn shutdown_is_terminal_idempotent_and_preserves_prior_operation_terminals() {
    let mut peer = create_peer(PeerConfig::offerer());
    peer.submit(Command::CreateOffer {
        operation_id: OperationId(1),
        epoch: NegotiationEpoch(1),
    })
    .expect("work is accepted");
    peer.submit(Command::Shutdown {
        operation_id: OperationId(2),
    })
    .expect("shutdown is accepted");
    let rejected = rejected_command(
        peer.submit(Command::Shutdown {
            operation_id: OperationId(3),
        }),
        TransportError::Shutdown,
    );
    assert_eq!(rejected.operation_id(), OperationId(3));

    let mut events = Vec::new();
    loop {
        let waker = std::task::Waker::noop();
        let mut context = Context::from_waker(waker);
        match peer.poll_event(&mut context) {
            Poll::Ready(Some(event)) => events.push(event),
            Poll::Ready(None) => break,
            Poll::Pending => panic!("accepted shutdown must make deterministic progress"),
        }
    }
    let terminal_ids: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::OperationCompleted { operation_id }
            | Event::OperationFailed { operation_id, .. } => Some(*operation_id),
            _ => None,
        })
        .collect();
    assert_eq!(terminal_ids, vec![OperationId(1), OperationId(2)]);
    assert_eq!(
        &events[events.len() - 4..],
        &[
            Event::StateChanged {
                state: PeerState::Closing,
            },
            Event::OperationCompleted {
                operation_id: OperationId(2),
            },
            Event::StateChanged {
                state: PeerState::Closed,
            },
            Event::ShutdownComplete,
        ],
        "shutdown ordering is exact and ShutdownComplete is terminal",
    );

    let waker = std::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    assert_eq!(peer.poll_event(&mut context), Poll::Ready(None));
    let rejected = rejected_command(
        peer.submit(Command::Shutdown {
            operation_id: OperationId(4),
        }),
        TransportError::Shutdown,
    );
    assert_eq!(rejected.operation_id(), OperationId(4));
    assert_eq!(peer.poll_event(&mut context), Poll::Ready(None));
}

#[test]
fn pending_replaces_the_registered_waker_and_submit_wakes_only_the_latest() {
    let mut peer = create_peer(PeerConfig::offerer());
    let stale_counter = Arc::new(CountingWake::default());
    let latest_counter = Arc::new(CountingWake::default());
    let stale_waker = Waker::from(Arc::clone(&stale_counter));
    let latest_waker = Waker::from(Arc::clone(&latest_counter));

    let mut stale_context = Context::from_waker(&stale_waker);
    assert_eq!(peer.poll_event(&mut stale_context), Poll::Pending);
    let mut latest_context = Context::from_waker(&latest_waker);
    assert_eq!(peer.poll_event(&mut latest_context), Poll::Pending);

    peer.submit(Command::Shutdown {
        operation_id: OperationId(1),
    })
    .expect("shutdown creates progress");
    assert_eq!(stale_counter.count(), 0, "the replaced waker is stale");
    assert_eq!(latest_counter.count(), 1, "the latest waker is woken once");

    assert_eq!(
        peer.poll_event(&mut latest_context),
        Poll::Ready(Some(Event::StateChanged {
            state: PeerState::Closing,
        }))
    );
    assert_eq!(
        peer.poll_event(&mut latest_context),
        Poll::Ready(Some(Event::OperationCompleted {
            operation_id: OperationId(1),
        }))
    );
    assert_eq!(
        peer.poll_event(&mut latest_context),
        Poll::Ready(Some(Event::StateChanged {
            state: PeerState::Closed,
        }))
    );
    assert_eq!(
        peer.poll_event(&mut latest_context),
        Poll::Ready(Some(Event::ShutdownComplete))
    );
    assert_eq!(peer.poll_event(&mut latest_context), Poll::Ready(None));
    assert_eq!(peer.poll_event(&mut latest_context), Poll::Ready(None));
    assert_eq!(stale_counter.count(), 0);
    assert_eq!(latest_counter.count(), 1);
}

#[test]
fn same_epoch_description_replay_is_idempotent_but_conflicts_are_stable() {
    let mut peer = create_peer(PeerConfig::answerer());
    let epoch = NegotiationEpoch(1);
    let remote_offer = fixture_description(BROWSER_OFFER_V1, epoch, DescriptionKind::Offer);
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(1),
        description: remote_offer.clone(),
    })
    .expect("initial remote offer is accepted");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Negotiating,
        }
    );
    assert_completed(next_event(&mut *peer), 1);

    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(2),
        description: remote_offer,
    })
    .expect("exact replay is admitted");
    assert_completed(next_event(&mut *peer), 2);

    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(3),
        description: SessionDescription::new(epoch, DescriptionKind::Offer, "conflicting offer")
            .expect("bounded conflicting offer"),
    })
    .expect("conflict is a correlated operation failure");
    assert_failed(
        next_event(&mut *peer),
        3,
        TransportError::ConflictingDescription,
    );

    let local_answer =
        SessionDescription::new(epoch, DescriptionKind::Answer, "stable local answer")
            .expect("bounded local answer");
    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(4),
        description: local_answer.clone(),
    })
    .expect("local answer is accepted");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connecting,
        }
    );
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connected,
        }
    );
    assert_completed(next_event(&mut *peer), 4);

    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(5),
        description: local_answer,
    })
    .expect("exact connected-state replay is admitted");
    assert_completed(next_event(&mut *peer), 5);
    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(6),
        description: SessionDescription::new(epoch, DescriptionKind::Answer, "different answer")
            .expect("bounded conflicting answer"),
    })
    .expect("connected-state conflict is correlated");
    assert_failed(
        next_event(&mut *peer),
        6,
        TransportError::ConflictingDescription,
    );
}

#[test]
fn remote_description_kind_follows_role_and_offerer_rejects_glare() {
    let epoch = NegotiationEpoch(1);
    let mut answerer = create_peer(PeerConfig::answerer());
    answerer
        .submit(Command::SetRemoteDescription {
            operation_id: OperationId(1),
            description: fixture_description(BROWSER_ANSWER_V1, epoch, DescriptionKind::Answer),
        })
        .expect("wrong kind is a correlated failure");
    assert_failed(next_event(&mut *answerer), 1, TransportError::InvalidState);

    let mut offerer = create_peer(PeerConfig::offerer());
    offerer
        .submit(Command::CreateOffer {
            operation_id: OperationId(1),
            epoch,
        })
        .expect("offer generation is accepted");
    local_generation(
        &mut *offerer,
        DescriptionKind::Offer,
        epoch,
        1,
        PeerState::Negotiating,
        true,
    );
    offerer
        .submit(Command::SetRemoteDescription {
            operation_id: OperationId(2),
            description: fixture_description(BROWSER_OFFER_V1, epoch, DescriptionKind::Offer),
        })
        .expect("glare is a correlated failure");
    assert_failed(next_event(&mut *offerer), 2, TransportError::InvalidState);
    offerer
        .submit(Command::SetLocalDescription {
            operation_id: OperationId(3),
            description: fixture_description(NATIVE_OFFER_V1, epoch, DescriptionKind::Offer),
        })
        .expect("local offer is accepted");
    assert_completed(next_event(&mut *offerer), 3);
    let remote_answer = fixture_description(BROWSER_ANSWER_V1, epoch, DescriptionKind::Answer);
    offerer
        .submit(Command::SetRemoteDescription {
            operation_id: OperationId(4),
            description: remote_answer.clone(),
        })
        .expect("remote answer is accepted");
    assert_eq!(
        next_event(&mut *offerer),
        Event::StateChanged {
            state: PeerState::Connecting,
        }
    );
    assert_eq!(
        next_event(&mut *offerer),
        Event::StateChanged {
            state: PeerState::Connected,
        }
    );
    assert_completed(next_event(&mut *offerer), 4);
    offerer
        .submit(Command::SetRemoteDescription {
            operation_id: OperationId(5),
            description: remote_answer,
        })
        .expect("exact remote answer replay is accepted");
    assert_completed(next_event(&mut *offerer), 5);
    offerer
        .submit(Command::SetRemoteDescription {
            operation_id: OperationId(6),
            description: SessionDescription::new(
                epoch,
                DescriptionKind::Answer,
                "conflicting remote answer",
            )
            .expect("bounded conflicting answer"),
        })
        .expect("remote answer conflict is correlated");
    assert_failed(
        next_event(&mut *offerer),
        6,
        TransportError::ConflictingDescription,
    );
}

#[test]
fn newer_answerer_epoch_drives_restart_fixtures_and_rejects_every_stale_input() {
    let mut peer = create_peer(PeerConfig::answerer());
    let old_epoch = NegotiationEpoch(1);
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(1),
        description: fixture_description(BROWSER_OFFER_V1, old_epoch, DescriptionKind::Offer),
    })
    .expect("baseline fixture offer is accepted");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Negotiating,
        }
    );
    assert_completed(next_event(&mut *peer), 1);
    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(2),
        description: fixture_description(NATIVE_ANSWER_V1, old_epoch, DescriptionKind::Answer),
    })
    .expect("baseline fixture answer is accepted");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connecting,
        }
    );
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connected,
        }
    );
    assert_completed(next_event(&mut *peer), 2);

    let new_epoch = NegotiationEpoch(2);
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(3),
        description: fixture_description(
            BROWSER_RESTART_OFFER_V1,
            new_epoch,
            DescriptionKind::Offer,
        ),
    })
    .expect("newer fixture offer restarts the answerer");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Restarting,
        }
    );
    assert_completed(next_event(&mut *peer), 3);

    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(4),
        description: fixture_description(BROWSER_OFFER_V1, old_epoch, DescriptionKind::Offer),
    })
    .expect("stale fixture offer is correlated");
    assert_failed(next_event(&mut *peer), 4, TransportError::StaleEpoch);
    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(5),
        description: fixture_description(NATIVE_ANSWER_V1, old_epoch, DescriptionKind::Answer),
    })
    .expect("stale fixture answer is correlated");
    assert_failed(next_event(&mut *peer), 5, TransportError::StaleEpoch);
    peer.submit(Command::AddRemoteCandidate {
        operation_id: OperationId(6),
        candidate: fixture_candidate(BROWSER_TRICKLE_V1, old_epoch),
    })
    .expect("stale fixture candidate is correlated");
    assert_failed(next_event(&mut *peer), 6, TransportError::StaleEpoch);
    peer.submit(Command::EndRemoteCandidates {
        operation_id: OperationId(7),
        end: fixture_end(BROWSER_END_V1, old_epoch),
    })
    .expect("stale fixture end marker is correlated");
    assert_failed(next_event(&mut *peer), 7, TransportError::StaleEpoch);

    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(8),
        description: fixture_description(
            NATIVE_RESTART_ANSWER_V1,
            new_epoch,
            DescriptionKind::Answer,
        ),
    })
    .expect("restart fixture answer is accepted after reset");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connecting,
        }
    );
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connected,
        }
    );
    assert_completed(next_event(&mut *peer), 8);
}

#[test]
fn newer_offerer_epoch_drives_restart_fixtures_and_rejects_every_stale_input() {
    let mut peer = create_peer(PeerConfig::offerer());
    let old_epoch = NegotiationEpoch(1);
    peer.submit(Command::CreateOffer {
        operation_id: OperationId(1),
        epoch: old_epoch,
    })
    .expect("baseline offer generation is accepted");
    local_generation(
        &mut *peer,
        DescriptionKind::Offer,
        old_epoch,
        1,
        PeerState::Negotiating,
        true,
    );
    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(2),
        description: fixture_description(NATIVE_OFFER_V1, old_epoch, DescriptionKind::Offer),
    })
    .expect("baseline fixture local offer is accepted");
    assert_completed(next_event(&mut *peer), 2);
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(3),
        description: fixture_description(BROWSER_ANSWER_V1, old_epoch, DescriptionKind::Answer),
    })
    .expect("baseline fixture remote answer is accepted");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connecting,
        }
    );
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connected,
        }
    );
    assert_completed(next_event(&mut *peer), 3);

    let new_epoch = NegotiationEpoch(2);
    peer.submit(Command::CreateOffer {
        operation_id: OperationId(4),
        epoch: new_epoch,
    })
    .expect("newer local offer restarts the offerer");
    local_generation(
        &mut *peer,
        DescriptionKind::Offer,
        new_epoch,
        4,
        PeerState::Restarting,
        false,
    );

    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(5),
        description: fixture_description(NATIVE_OFFER_V1, old_epoch, DescriptionKind::Offer),
    })
    .expect("stale fixture local offer is correlated");
    assert_failed(next_event(&mut *peer), 5, TransportError::StaleEpoch);
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(6),
        description: fixture_description(BROWSER_ANSWER_V1, old_epoch, DescriptionKind::Answer),
    })
    .expect("stale fixture remote answer is correlated");
    assert_failed(next_event(&mut *peer), 6, TransportError::StaleEpoch);
    peer.submit(Command::AddRemoteCandidate {
        operation_id: OperationId(7),
        candidate: fixture_candidate(BROWSER_TRICKLE_V1, old_epoch),
    })
    .expect("stale fixture candidate is correlated");
    assert_failed(next_event(&mut *peer), 7, TransportError::StaleEpoch);
    peer.submit(Command::EndRemoteCandidates {
        operation_id: OperationId(8),
        end: fixture_end(BROWSER_END_V1, old_epoch),
    })
    .expect("stale fixture end marker is correlated");
    assert_failed(next_event(&mut *peer), 8, TransportError::StaleEpoch);

    peer.submit(Command::SetLocalDescription {
        operation_id: OperationId(9),
        description: fixture_description(
            NATIVE_RESTART_OFFER_V1,
            new_epoch,
            DescriptionKind::Offer,
        ),
    })
    .expect("restart fixture local offer is accepted");
    assert_completed(next_event(&mut *peer), 9);
    peer.submit(Command::SetRemoteDescription {
        operation_id: OperationId(10),
        description: fixture_description(
            BROWSER_RESTART_ANSWER_V1,
            new_epoch,
            DescriptionKind::Answer,
        ),
    })
    .expect("restart fixture remote answer is accepted");
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connecting,
        }
    );
    assert_eq!(
        next_event(&mut *peer),
        Event::StateChanged {
            state: PeerState::Connected,
        }
    );
    assert_completed(next_event(&mut *peer), 10);
}

#[test]
fn end_marker_is_bounded_and_preadmission_return_preserves_every_field() {
    let absolute_oversize = "x".repeat(relay_transport::MAX_CANDIDATE_BYTES + 1);
    assert_eq!(
        EndOfCandidates::new(NegotiationEpoch(1), Some(absolute_oversize), None, None),
        Err(TransportError::CandidateTooLarge),
    );

    let mut config = PeerConfig::answerer();
    config.max_candidate_bytes = 8;
    let mut peer = create_peer(config);
    let command = Command::EndRemoteCandidates {
        operation_id: OperationId(11),
        end: EndOfCandidates::new(
            NegotiationEpoch(7),
            Some("data-mid".to_owned()),
            Some(23),
            Some("ufrag".to_owned()),
        )
        .expect("marker is within the absolute cap"),
    };
    let returned = rejected_command(peer.submit(command), TransportError::CandidateTooLarge);
    let Command::EndRemoteCandidates { operation_id, end } = returned else {
        panic!("expected the original end-marker command");
    };
    assert_eq!(operation_id, OperationId(11));
    assert_eq!(end.epoch(), NegotiationEpoch(7));
    assert_eq!(end.sdp_mid(), Some("data-mid"));
    assert_eq!(end.sdp_mline_index(), Some(23));
    assert_eq!(end.username_fragment(), Some("ufrag"));

    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert_eq!(peer.poll_event(&mut context), Poll::Pending);
}

#[test]
fn maximum_operation_id_is_reserved_for_orderly_shutdown() {
    let mut peer = create_peer(PeerConfig::offerer());
    peer.submit(Command::CreateOffer {
        operation_id: OperationId(u64::MAX - 1),
        epoch: NegotiationEpoch(1),
    })
    .expect("maximum nonterminal operation ID is accepted");
    let exhausted = Command::CreateOffer {
        operation_id: OperationId(u64::MAX),
        epoch: NegotiationEpoch(2),
    };
    assert_eq!(
        rejected_command(
            peer.submit(exhausted.clone()),
            TransportError::OperationIdExhausted
        ),
        exhausted,
    );
    peer.submit(Command::Shutdown {
        operation_id: OperationId(u64::MAX),
    })
    .expect("reserved maximum ID remains available for shutdown");

    let mut events = Vec::new();
    loop {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        match peer.poll_event(&mut context) {
            Poll::Ready(Some(event)) => events.push(event),
            Poll::Ready(None) => break,
            Poll::Pending => panic!("accepted shutdown must terminate"),
        }
    }
    let terminal_ids: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::OperationCompleted { operation_id }
            | Event::OperationFailed { operation_id, .. } => Some(*operation_id),
            _ => None,
        })
        .collect();
    assert_eq!(
        terminal_ids,
        vec![OperationId(u64::MAX - 1), OperationId(u64::MAX)]
    );
    assert_eq!(
        &events[events.len() - 4..],
        &[
            Event::StateChanged {
                state: PeerState::Closing,
            },
            Event::OperationCompleted {
                operation_id: OperationId(u64::MAX),
            },
            Event::StateChanged {
                state: PeerState::Closed,
            },
            Event::ShutdownComplete,
        ]
    );
}

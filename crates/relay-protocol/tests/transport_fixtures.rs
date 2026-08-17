use prost::Message;
use relay_protocol::v1::{Envelope, PeerUpdateKind, envelope, hello, welcome};

#[derive(Clone, Copy)]
enum FixtureKind {
    Offer,
    Answer,
    TrickleCandidate,
    EndOfCandidates,
    PeerLeft,
    ResumeRequest,
    ResumeAccepted,
}

const FIXTURES: &[(&str, &[u8], FixtureKind)] = &[
    (
        "browser-offer-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/browser-offer-v1.bin"),
        FixtureKind::Offer,
    ),
    (
        "native-answer-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/native-answer-v1.bin"),
        FixtureKind::Answer,
    ),
    (
        "native-offer-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/native-offer-v1.bin"),
        FixtureKind::Offer,
    ),
    (
        "browser-answer-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/browser-answer-v1.bin"),
        FixtureKind::Answer,
    ),
    (
        "browser-trickle-candidate-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/browser-trickle-candidate-v1.bin"),
        FixtureKind::TrickleCandidate,
    ),
    (
        "native-trickle-candidate-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/native-trickle-candidate-v1.bin"),
        FixtureKind::TrickleCandidate,
    ),
    (
        "browser-end-of-candidates-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/browser-end-of-candidates-v1.bin"),
        FixtureKind::EndOfCandidates,
    ),
    (
        "native-end-of-candidates-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/native-end-of-candidates-v1.bin"),
        FixtureKind::EndOfCandidates,
    ),
    (
        "peer-left-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/peer-left-v1.bin"),
        FixtureKind::PeerLeft,
    ),
    (
        "resume-request-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/resume-request-v1.bin"),
        FixtureKind::ResumeRequest,
    ),
    (
        "resume-accepted-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/resume-accepted-v1.bin"),
        FixtureKind::ResumeAccepted,
    ),
    (
        "browser-ice-restart-offer-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/browser-ice-restart-offer-v1.bin"),
        FixtureKind::Offer,
    ),
    (
        "native-ice-restart-answer-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/native-ice-restart-answer-v1.bin"),
        FixtureKind::Answer,
    ),
    (
        "native-ice-restart-offer-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/native-ice-restart-offer-v1.bin"),
        FixtureKind::Offer,
    ),
    (
        "browser-ice-restart-answer-v1.bin",
        include_bytes!("../../../tests/fixtures/transport/v1/browser-ice-restart-answer-v1.bin"),
        FixtureKind::Answer,
    ),
];

#[test]
fn rust_decodes_and_reencodes_all_transport_v1_fixtures() {
    for (name, golden, expected_kind) in FIXTURES {
        let envelope =
            Envelope::decode(*golden).unwrap_or_else(|error| panic!("{name} must decode: {error}"));

        assert_eq!(
            envelope.version.as_ref().map(|version| version.major),
            Some(1),
            "{name}"
        );
        assert_eq!(
            envelope.session_id, "transport-fixture-session-v1",
            "{name}"
        );
        assert_payload(name, &envelope, *expected_kind);
        assert_eq!(envelope.encode_to_vec(), *golden, "{name}");
    }
}

#[test]
fn ice_restart_fixtures_change_the_opaque_sdp_ice_generation() {
    let browser_baseline = offer_sdp(FIXTURES[0].1);
    let browser_restart = offer_sdp(FIXTURES[11].1);
    let native_baseline = offer_sdp(FIXTURES[2].1);
    let native_restart = offer_sdp(FIXTURES[13].1);

    assert!(browser_baseline.contains("a=ice-ufrag:browser-base-v1"));
    assert!(browser_restart.contains("a=ice-ufrag:browser-restart-v1"));
    assert!(native_baseline.contains("a=ice-ufrag:native-base-v1"));
    assert!(native_restart.contains("a=ice-ufrag:native-restart-v1"));
}

fn offer_sdp(bytes: &[u8]) -> String {
    let envelope = Envelope::decode(bytes).expect("offer fixture must decode");
    let Some(envelope::Payload::Offer(offer)) = envelope.payload else {
        panic!("fixture must contain an offer");
    };
    offer.sdp
}

fn assert_payload(name: &str, envelope: &Envelope, expected_kind: FixtureKind) {
    match (expected_kind, envelope.payload.as_ref()) {
        (FixtureKind::Offer, Some(envelope::Payload::Offer(offer))) => {
            assert!(!offer.target_peer_id.is_empty(), "{name}");
            assert!(offer.sdp.contains("m=application"), "{name}");
        }
        (FixtureKind::Answer, Some(envelope::Payload::Answer(answer))) => {
            assert!(!answer.target_peer_id.is_empty(), "{name}");
            assert!(answer.sdp.contains("m=application"), "{name}");
        }
        (FixtureKind::TrickleCandidate, Some(envelope::Payload::IceCandidate(candidate))) => {
            assert!(!candidate.candidate.is_empty(), "{name}");
            assert!(!candidate.end_of_candidates, "{name}");
            assert!(candidate.sdp_mid.is_some(), "{name}");
            assert!(candidate.username_fragment.is_some(), "{name}");
        }
        (FixtureKind::EndOfCandidates, Some(envelope::Payload::IceCandidate(candidate))) => {
            assert!(candidate.candidate.is_empty(), "{name}");
            assert!(candidate.end_of_candidates, "{name}");
        }
        (FixtureKind::PeerLeft, Some(envelope::Payload::PeerUpdate(update))) => {
            assert_eq!(update.kind(), PeerUpdateKind::Left, "{name}");
        }
        (FixtureKind::ResumeRequest, Some(envelope::Payload::Hello(hello_message))) => {
            assert!(
                matches!(hello_message.entry, Some(hello::Entry::Resume(_))),
                "{name}"
            );
        }
        (FixtureKind::ResumeAccepted, Some(envelope::Payload::Welcome(welcome_message))) => {
            assert!(
                matches!(
                    welcome_message.recovery,
                    Some(welcome::Recovery::ResumeAccepted(_))
                ),
                "{name}"
            );
        }
        _ => panic!("{name} has the wrong payload kind"),
    }
}

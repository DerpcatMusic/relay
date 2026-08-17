use std::{fs, path::PathBuf};

use prost::Message;
use relay_protocol::v1::{
    Answer, Capabilities, Envelope, Hello, IceCandidate, Offer, PeerUpdate, PeerUpdateKind,
    ProtocolVersion, ResumeAccepted, ResumeRequest, Welcome, envelope, hello, welcome,
};

const SESSION_ID: &str = "transport-fixture-session-v1";
const BROWSER_PEER_ID: &str = "browser-v1";
const NATIVE_PEER_ID: &str = "native-v1";

fn version() -> Option<ProtocolVersion> {
    Some(ProtocolVersion { major: 1, minor: 1 })
}

fn capabilities() -> Option<Capabilities> {
    Some(Capabilities {
        opus_frame_durations_us: vec![5_000, 10_000, 20_000],
        max_opus_bitrate_bps: 256_000,
        inband_fec: true,
        dtx: false,
        turn_tls: true,
        ice_restart: true,
        max_audio_tracks: 1,
        opus_channel_counts: vec![2],
    })
}

fn envelope(
    message_id: &str,
    peer_id: &str,
    revision: u64,
    payload: envelope::Payload,
) -> Envelope {
    Envelope {
        version: version(),
        message_id: message_id.into(),
        session_id: SESSION_ID.into(),
        peer_id: peer_id.into(),
        revision,
        payload: Some(payload),
    }
}

fn sdp(origin_id: u64, session_version: u64, role: &str, ice_generation: &str) -> String {
    let setup = if role.contains("offer") {
        "actpass"
    } else {
        "active"
    };
    format!(
        "v=0\r\no=- {origin_id} {session_version} IN IP4 127.0.0.1\r\ns=RELAY {role} fixture\r\nt=0 0\r\na=group:BUNDLE data\r\na=msid-semantic: WMS\r\na=ice-options:trickle\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\nc=IN IP4 0.0.0.0\r\na=mid:data\r\na=ice-ufrag:{ice_generation}\r\na=ice-pwd:{ice_generation}-password\r\na=fingerprint:sha-256 00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF\r\na=setup:{setup}\r\na=sctp-port:5000\r\na=max-message-size:262144\r\n"
    )
}

fn candidate(address: &str, port: u16, username_fragment: &str) -> String {
    format!(
        "candidate:1 1 UDP 2122260223 {address} {port} typ host generation 0 ufrag {username_fragment}"
    )
}

fn fixtures() -> Vec<(&'static str, Envelope)> {
    vec![
        (
            "browser-offer-v1.bin",
            envelope(
                "transport-browser-offer-v1",
                BROWSER_PEER_ID,
                1,
                envelope::Payload::Offer(Offer {
                    target_peer_id: NATIVE_PEER_ID.into(),
                    sdp: sdp(10_001, 1, "browser offer", "browser-base-v1"),
                }),
            ),
        ),
        (
            "native-answer-v1.bin",
            envelope(
                "transport-native-answer-v1",
                NATIVE_PEER_ID,
                2,
                envelope::Payload::Answer(Answer {
                    target_peer_id: BROWSER_PEER_ID.into(),
                    sdp: sdp(20_001, 1, "native answer", "native-base-v1"),
                }),
            ),
        ),
        (
            "native-offer-v1.bin",
            envelope(
                "transport-native-offer-v1",
                NATIVE_PEER_ID,
                3,
                envelope::Payload::Offer(Offer {
                    target_peer_id: BROWSER_PEER_ID.into(),
                    sdp: sdp(20_002, 1, "native offer", "native-base-v1"),
                }),
            ),
        ),
        (
            "browser-answer-v1.bin",
            envelope(
                "transport-browser-answer-v1",
                BROWSER_PEER_ID,
                4,
                envelope::Payload::Answer(Answer {
                    target_peer_id: NATIVE_PEER_ID.into(),
                    sdp: sdp(10_002, 1, "browser answer", "browser-base-v1"),
                }),
            ),
        ),
        (
            "browser-trickle-candidate-v1.bin",
            envelope(
                "transport-browser-trickle-v1",
                BROWSER_PEER_ID,
                5,
                envelope::Payload::IceCandidate(IceCandidate {
                    target_peer_id: NATIVE_PEER_ID.into(),
                    candidate: candidate("192.0.2.10", 50_000, "browser-base-v1"),
                    sdp_mid: Some("data".into()),
                    sdp_mline_index: Some(0),
                    username_fragment: Some("browser-base-v1".into()),
                    end_of_candidates: false,
                }),
            ),
        ),
        (
            "native-trickle-candidate-v1.bin",
            envelope(
                "transport-native-trickle-v1",
                NATIVE_PEER_ID,
                6,
                envelope::Payload::IceCandidate(IceCandidate {
                    target_peer_id: BROWSER_PEER_ID.into(),
                    candidate: candidate("198.51.100.20", 50_002, "native-base-v1"),
                    sdp_mid: Some("data".into()),
                    sdp_mline_index: Some(0),
                    username_fragment: Some("native-base-v1".into()),
                    end_of_candidates: false,
                }),
            ),
        ),
        (
            "browser-end-of-candidates-v1.bin",
            envelope(
                "transport-browser-end-of-candidates-v1",
                BROWSER_PEER_ID,
                7,
                envelope::Payload::IceCandidate(IceCandidate {
                    target_peer_id: NATIVE_PEER_ID.into(),
                    candidate: String::new(),
                    sdp_mid: Some("data".into()),
                    sdp_mline_index: Some(0),
                    username_fragment: Some("browser-base-v1".into()),
                    end_of_candidates: true,
                }),
            ),
        ),
        (
            "native-end-of-candidates-v1.bin",
            envelope(
                "transport-native-end-of-candidates-v1",
                NATIVE_PEER_ID,
                8,
                envelope::Payload::IceCandidate(IceCandidate {
                    target_peer_id: BROWSER_PEER_ID.into(),
                    candidate: String::new(),
                    sdp_mid: Some("data".into()),
                    sdp_mline_index: Some(0),
                    username_fragment: Some("native-base-v1".into()),
                    end_of_candidates: true,
                }),
            ),
        ),
        (
            "peer-left-v1.bin",
            envelope(
                "transport-peer-left-v1",
                "signaling-server-v1",
                9,
                envelope::Payload::PeerUpdate(PeerUpdate {
                    kind: PeerUpdateKind::Left.into(),
                    subject_peer_id: BROWSER_PEER_ID.into(),
                    capabilities: None,
                }),
            ),
        ),
        (
            "resume-request-v1.bin",
            envelope(
                "transport-resume-request-v1",
                BROWSER_PEER_ID,
                9,
                envelope::Payload::Hello(Hello {
                    supported_versions: vec![ProtocolVersion { major: 1, minor: 1 }],
                    capabilities: capabilities(),
                    client_name: "relay-browser-fixture".into(),
                    client_version: "1.0.0-fixture".into(),
                    entry: Some(hello::Entry::Resume(ResumeRequest {
                        resume_token: "fixture-resume-token-not-a-secret".into(),
                        last_seen_revision: 9,
                    })),
                }),
            ),
        ),
        (
            "resume-accepted-v1.bin",
            envelope(
                "transport-resume-accepted-v1",
                "signaling-server-v1",
                10,
                envelope::Payload::Welcome(Welcome {
                    selected_version: version(),
                    assigned_session_id: SESSION_ID.into(),
                    assigned_peer_id: BROWSER_PEER_ID.into(),
                    capabilities: capabilities(),
                    resume_token: "fixture-rotated-resume-token-not-a-secret".into(),
                    current_revision: 10,
                    recovery: Some(welcome::Recovery::ResumeAccepted(ResumeAccepted {
                        missing_events: Vec::new(),
                    })),
                }),
            ),
        ),
        (
            "browser-ice-restart-offer-v1.bin",
            envelope(
                "transport-browser-ice-restart-offer-v1",
                BROWSER_PEER_ID,
                11,
                envelope::Payload::Offer(Offer {
                    target_peer_id: NATIVE_PEER_ID.into(),
                    sdp: sdp(10_001, 2, "browser ICE restart offer", "browser-restart-v1"),
                }),
            ),
        ),
        (
            "native-ice-restart-answer-v1.bin",
            envelope(
                "transport-native-ice-restart-answer-v1",
                NATIVE_PEER_ID,
                12,
                envelope::Payload::Answer(Answer {
                    target_peer_id: BROWSER_PEER_ID.into(),
                    sdp: sdp(20_001, 2, "native ICE restart answer", "native-restart-v1"),
                }),
            ),
        ),
        (
            "native-ice-restart-offer-v1.bin",
            envelope(
                "transport-native-ice-restart-offer-v1",
                NATIVE_PEER_ID,
                13,
                envelope::Payload::Offer(Offer {
                    target_peer_id: BROWSER_PEER_ID.into(),
                    sdp: sdp(20_002, 2, "native ICE restart offer", "native-restart-v1"),
                }),
            ),
        ),
        (
            "browser-ice-restart-answer-v1.bin",
            envelope(
                "transport-browser-ice-restart-answer-v1",
                BROWSER_PEER_ID,
                14,
                envelope::Payload::Answer(Answer {
                    target_peer_id: NATIVE_PEER_ID.into(),
                    sdp: sdp(
                        10_002,
                        2,
                        "browser ICE restart answer",
                        "browser-restart-v1",
                    ),
                }),
            ),
        ),
    ]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/transport/v1");
    fs::create_dir_all(&output)?;

    for (name, fixture) in fixtures() {
        let path = output.join(name);
        fs::write(&path, fixture.encode_to_vec())?;
        println!("wrote {}", path.display());
    }

    Ok(())
}

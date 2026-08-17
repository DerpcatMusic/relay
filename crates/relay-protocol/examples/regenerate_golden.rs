use std::{fs, path::PathBuf};

use prost::Message;
use relay_protocol::v1::{
    Capabilities, Envelope, Hello, ProtocolVersion, ResumeRequest, envelope, hello,
};

fn fixture() -> Envelope {
    Envelope {
        version: Some(ProtocolVersion { major: 1, minor: 1 }),
        message_id: "golden-hello-resume-0001".into(),
        session_id: "session-golden".into(),
        peer_id: "peer-golden".into(),
        revision: 42,
        payload: Some(envelope::Payload::Hello(Hello {
            supported_versions: vec![
                ProtocolVersion { major: 1, minor: 1 },
                ProtocolVersion { major: 1, minor: 0 },
            ],
            capabilities: Some(Capabilities {
                opus_frame_durations_us: vec![5_000, 10_000, 20_000],
                max_opus_bitrate_bps: 256_000,
                inband_fec: true,
                dtx: false,
                turn_tls: true,
                ice_restart: true,
                max_audio_tracks: 2,
                opus_channel_counts: vec![1, 2],
            }),
            client_name: "relay-golden".into(),
            client_version: "0.1.0".into(),
            entry: Some(hello::Entry::Resume(ResumeRequest {
                resume_token: "fixture-resume-token-not-a-secret".into(),
                last_seen_revision: 41,
            })),
        })),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/protocol/hello-resume-v1.bin");
    let encoded = fixture().encode_to_vec();
    fs::write(&output, encoded)?;
    println!("wrote {}", output.display());
    Ok(())
}

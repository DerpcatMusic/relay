use std::process::{Command, Output};

use serde_json::Value;

const RATES: [&str; 4] = ["44100", "48000", "96000", "192000"];
const PACKET_DURATIONS_MS: [&str; 3] = ["5", "10", "20"];

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_relay-audio-lab"))
        .args(args)
        .output()
        .expect("audio-lab executable should launch")
}

fn json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("output must be valid JSON")
}

fn unsigned(value: &Value, name: &str) -> u64 {
    value[name]
        .as_u64()
        .unwrap_or_else(|| panic!("missing unsigned field {name}"))
}

fn signed(value: &Value, name: &str) -> i64 {
    value[name]
        .as_i64()
        .unwrap_or_else(|| panic!("missing signed field {name}"))
}

#[test]
fn deterministic_real_path_json_is_repeatable_and_bounded() {
    let args = [
        "--json",
        "--duration-ms",
        "100",
        "--packet-ms",
        "10",
        "--capture-rate",
        "44100",
        "--playback-rate",
        "96000",
        "--seed",
        "9",
    ];
    let first = run(&args);
    let second = run(&args);
    assert_eq!(first.stdout, second.stdout);
    let report = json(&first);
    assert_eq!(unsigned(&report, "input_frames"), 4_410);
    assert_eq!(unsigned(&report, "encoded_packets"), 10);
    assert_eq!(unsigned(&report, "emitted_frames"), 10);
    assert_eq!(unsigned(&report, "drained_lookahead_frames"), 1);
    assert_eq!(unsigned(&report, "ingress_accepted_packets"), 10);
    assert_eq!(unsigned(&report, "network_drops"), 0);
    assert_eq!(unsigned(&report, "ring_dropped_frames"), 0);
    assert_eq!(unsigned(&report, "ring_underrun_frames"), 0);
    assert_eq!(unsigned(&report, "configured_capture_rate_hz"), 44_100);
    assert_eq!(unsigned(&report, "configured_playback_rate_hz"), 96_000);
    assert!(unsigned(&report, "rendered_checksum") > 0);
}

#[test]
fn impaired_profile_distinguishes_requests_scheduled_copies_and_rx_observations() {
    let output = run(&[
        "--json",
        "--duration-ms",
        "500",
        "--packet-ms",
        "20",
        "--profile",
        "impaired",
        "--seed",
        "7",
    ]);
    let report = json(&output);
    assert_eq!(unsigned(&report, "network_drops"), 2);
    assert_eq!(unsigned(&report, "network_duplicate_requests"), 1);
    assert_eq!(unsigned(&report, "network_duplicate_copies_scheduled"), 1);
    assert_eq!(unsigned(&report, "rx_duplicate_rejections"), 1);
    assert_eq!(unsigned(&report, "ingress_accepted_packets"), 23);
    assert_eq!(unsigned(&report, "emitted_frames"), 25);
    assert_eq!(unsigned(&report, "drained_lookahead_frames"), 1);
    assert_eq!(unsigned(&report, "fec_or_plc_frames"), 2);
    assert_eq!(unsigned(&report, "plc_frames"), 0);
    assert_eq!(unsigned(&report, "ring_dropped_frames"), 0);
}

#[test]
fn full_supported_rate_and_packet_duration_matrix_has_exact_path_identities() {
    for capture in RATES {
        for playback in RATES {
            for packet_ms in PACKET_DURATIONS_MS {
                let output = run(&[
                    "--json",
                    "--duration-ms",
                    "100",
                    "--capture-rate",
                    capture,
                    "--playback-rate",
                    playback,
                    "--packet-ms",
                    packet_ms,
                ]);
                let report = json(&output);
                let capture_rate = capture.parse::<u64>().expect("rate literal");
                let playback_rate = playback.parse::<u64>().expect("rate literal");
                let packet_ms = packet_ms.parse::<u64>().expect("duration literal");
                let expected_input_frames = capture_rate / 10;
                let nominal_packet_count = 100 / packet_ms;
                let expected_encoded = if capture_rate == 192_000 {
                    nominal_packet_count - 1
                } else {
                    nominal_packet_count
                };
                let expected_error = -i64::try_from((playback_rate / 24_000).max(2))
                    .expect("supported rate fits i64");
                let expected_rendered = expected_encoded * packet_ms * playback_rate / 1_000;
                let expected_rendered = expected_rendered
                    .checked_add_signed(expected_error)
                    .expect("expected render count is positive");
                let encoded = unsigned(&report, "encoded_packets");
                assert_eq!(unsigned(&report, "input_frames"), expected_input_frames);
                assert_eq!(encoded, expected_encoded);
                assert_eq!(unsigned(&report, "rendered_frames"), expected_rendered);
                assert_eq!(signed(&report, "playback_error_frames"), expected_error);
                assert_eq!(unsigned(&report, "emitted_frames"), encoded);
                assert_eq!(unsigned(&report, "ingress_accepted_packets"), encoded);
                assert_eq!(unsigned(&report, "drained_lookahead_frames"), 1);
                assert_eq!(unsigned(&report, "network_drops"), 0);
                assert_eq!(unsigned(&report, "ring_dropped_frames"), 0);
                assert_eq!(unsigned(&report, "ring_underrun_frames"), 0);
                assert!(unsigned(&report, "published_chunks") > 0);
            }
        }
    }
}

#[test]
fn minimum_duration_supports_every_matrix_case_and_maximum_is_bounded() {
    for capture in RATES {
        for playback in RATES {
            for packet_ms in PACKET_DURATIONS_MS {
                let output = run(&[
                    "--json",
                    "--duration-ms",
                    "50",
                    "--capture-rate",
                    capture,
                    "--playback-rate",
                    playback,
                    "--packet-ms",
                    packet_ms,
                ]);
                let report = json(&output);
                assert!(unsigned(&report, "encoded_packets") >= 2);
                assert_eq!(unsigned(&report, "drained_lookahead_frames"), 1);
                assert_eq!(
                    unsigned(&report, "encoded_packets"),
                    unsigned(&report, "emitted_frames")
                );
                assert_eq!(unsigned(&report, "ring_dropped_frames"), 0);
            }
        }
    }
    let output = run(&["--json", "--duration-ms", "10000", "--packet-ms", "20"]);
    let report = json(&output);
    assert_eq!(unsigned(&report, "drained_lookahead_frames"), 1);
    assert_eq!(unsigned(&report, "encoded_packets"), 500);
    assert_eq!(unsigned(&report, "emitted_frames"), 500);
    assert_eq!(unsigned(&report, "ring_dropped_frames"), 0);
}

#[test]
fn human_summary_uses_nominal_labels_and_is_emitted_after_the_run() {
    let output = run(&["--duration-ms", "100"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(stdout.starts_with("audio-lab diagnostics\n"));
    assert!(stdout.contains("configured nominal rates: capture=48000 playback=48000"));
    assert!(stdout.contains("drained lookahead frames: 1"));
    assert!(!stdout.contains("effective rates"));
    assert!(!stdout.contains("clean shutdown"));
    assert!(output.stderr.is_empty());
}

#[test]
fn invalid_configuration_and_device_mode_fail_without_panicking() {
    for args in [
        vec!["--duration-ms", "20"],
        vec!["--duration-ms", "40"],
        vec!["--duration-ms", "51"],
        vec!["--duration-ms", "10010"],
        vec!["--packet-ms", "7"],
        vec!["--capture-rate", "32000"],
        vec!["--playback-rate"],
        vec!["--device"],
        vec!["--not-an-option"],
    ] {
        let output = run(&args);
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.starts_with("audio-lab:"));
        assert!(!stderr.contains("panicked"));
    }
}

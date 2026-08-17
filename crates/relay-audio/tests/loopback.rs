//! Short, deterministic, real-codec end-to-end audio-loop tests.

use std::{f32::consts::TAU, time::Duration};

use relay_audio::{
    AdaptiveClockConfig, AudioPipelineConfig, AudioPipelineConfigInput, Bitrate, CaptureInput,
    ClockRecoveryConfig, EncoderPolicyV1, ExtendedSequence, ExtendedTimestamp, FrameDuration,
    FrameSource, InbandFec, IngressMismatch, IngressStatus, MAX_PACKET_BYTES, NetworkAction,
    NetworkTime, PacketBatch, PacketLossPercent, PayloadType, PlaybackConfig, PlaybackPublication,
    RenderState, RtpTimestamp, RxStreamConfig, RxWorker, ScheduleStatus, SequenceNumber, Ssrc,
    TxProcessOutcome, TxStreamConfig, TxWorker, playback_pair,
};

const SSRC: u32 = 0x51a7_0bad;
const PAYLOAD_TYPE: u8 = 111;
const INITIAL_SEQUENCE: u64 = (7_u64 << 16) | 65_532;
const INITIAL_TIMESTAMP: u32 = u32::MAX - 700;

#[derive(Clone, Copy)]
enum PathShape {
    Nominal,
    LossReorderDuplicate,
}

#[derive(Debug, Eq, PartialEq)]
struct LoopResult {
    encoded_packets: usize,
    rendered_bits: Vec<u32>,
    frames: Vec<(u64, u32, FrameSource)>,
    rx_metrics: relay_audio::RxMetrics,
    network_metrics: relay_audio::NetworkMetrics,
    playback_metrics: relay_audio::PlaybackMetrics,
    ring_dropped_samples: u64,
    maximum_ring_samples: usize,
    estimated_remote_drift_ppm_bits: Option<u64>,
    target_correction_ppm_bits: u64,
}

fn pipeline(
    capture_rate_hz: usize,
    playback_rate_hz: usize,
    duration: FrameDuration,
) -> AudioPipelineConfig {
    AudioPipelineConfig::new(AudioPipelineConfigInput {
        capture_rate_hz,
        playback_rate_hz,
        channels: 2,
        frame_duration: duration,
        capture_src_chunk_frames: capture_rate_hz / 100,
        capture_ring_samples: 100_000,
        playback_ring_samples: 100_000,
        tx_accumulator_samples: 100_000,
        reorder_capacity: 64,
        network_capacity: 64,
        network_due_batch_capacity: 64,
        packet_capacity: MAX_PACKET_BYTES,
        controller_cadence_frames: playback_rate_hz / 100,
        clock_recovery: ClockRecoveryConfig {
            max_slew_ppm_per_second: 100_000.0,
            proportional_gain_ppm_per_frame: 0.0,
            integral_gain_ppm_per_frame_second: 0.0,
            ..ClockRecoveryConfig::default()
        },
        adaptive_clock: AdaptiveClockConfig::default(),
    })
    .expect("loopback pipeline shape")
}

fn stream_policy(shape: PathShape) -> EncoderPolicyV1 {
    let (fec, loss) = match shape {
        PathShape::Nominal => (InbandFec::Disabled, PacketLossPercent::ZERO),
        PathShape::LossReorderDuplicate => (
            InbandFec::Enabled,
            PacketLossPercent::try_new(20).expect("valid loss hint"),
        ),
    };
    EncoderPolicyV1::new(Bitrate::try_new(96_000).expect("valid bitrate"), fec, loss)
}

fn capture_pcm(start_frame: usize, frames: usize, rate_hz: usize) -> Vec<f32> {
    let mut pcm = vec![0.0; frames * 2];
    for frame in 0..frames {
        let position = (start_frame + frame) as f32 / rate_hz as f32;
        pcm[frame * 2] = (TAU * 311.0 * position).sin() * 0.22;
        pcm[frame * 2 + 1] = (TAU * 617.0 * position).sin() * 0.17;
    }
    pcm
}

fn action_for(index: usize, packet_micros: u64, shape: PathShape) -> NetworkAction {
    if matches!(shape, PathShape::LossReorderDuplicate) {
        match index {
            0 => NetworkAction::Duplicate {
                duplicate_delay: Duration::from_micros(1),
            },
            // Packet 3 is delivered before packet 2, while both remain inside
            // the scheduled playout lookahead.
            2 => NetworkAction::Delay {
                delay: Duration::from_micros(3 * packet_micros),
            },
            3 => NetworkAction::Delay {
                delay: Duration::from_micros(2 * packet_micros),
            },
            // One FEC request, followed later by a two-packet hole whose first
            // position has no following packet and therefore uses explicit PLC.
            4 | 7 | 8 => NetworkAction::Drop,
            _ => NetworkAction::Delay {
                delay: Duration::from_micros(index as u64 * packet_micros),
            },
        }
    } else {
        NetworkAction::Delay {
            delay: Duration::from_micros(index as u64 * packet_micros),
        }
    }
}

fn scheduled_position(
    sequence: ExtendedSequence,
    timestamp: RtpTimestamp,
    duration: FrameDuration,
    playback_rate_hz: usize,
    scheduled_drift_ppm: i32,
) -> (ExtendedTimestamp, u64) {
    let offset = sequence
        .get()
        .checked_sub(INITIAL_SEQUENCE)
        .expect("RX sequence remains in the scheduled epoch");
    let media_delta = offset * duration.samples_per_channel() as u64;
    assert_eq!(
        timestamp,
        RtpTimestamp::new(INITIAL_TIMESTAMP.wrapping_add(media_delta as u32))
    );
    let extended_media = ExtendedTimestamp::new(u64::from(INITIAL_TIMESTAMP) + media_delta);
    let drift_scale = u64::try_from(1_000_000_i64 + i64::from(scheduled_drift_ppm))
        .expect("test drift keeps a positive rate");
    let numerator = media_delta * playback_rate_hz as u64 * 1_000_000;
    let denominator = 48_000 * drift_scale;
    let local_device_frame = (numerator + denominator / 2) / denominator;
    (extended_media, local_device_frame)
}

fn run_loop(
    capture_rate_hz: usize,
    playback_rate_hz: usize,
    duration: FrameDuration,
    shape: PathShape,
    scheduled_drift_ppm: i32,
) -> LoopResult {
    let config = pipeline(capture_rate_hz, playback_rate_hz, duration);
    let tx_stream = TxStreamConfig {
        ssrc: Ssrc::new(SSRC),
        payload_type: PayloadType::new(PAYLOAD_TYPE).expect("payload type"),
        initial_sequence: SequenceNumber::new(INITIAL_SEQUENCE as u16),
        initial_timestamp: RtpTimestamp::new(INITIAL_TIMESTAMP),
        encoding_policy: stream_policy(shape),
    };
    let mut tx = TxWorker::new(config, tx_stream).expect("TX worker");
    let mut tx_batch = PacketBatch::new(64).expect("TX batch");
    let capture_frames = tx.capture_chunk_samples() / config.channels();
    let capture_chunks = if scheduled_drift_ppm != 0 {
        64
    } else if matches!(shape, PathShape::Nominal) {
        12
    } else {
        30
    };
    let mut packets = Vec::new();
    for chunk in 0..capture_chunks {
        let pcm = capture_pcm(chunk * capture_frames, capture_frames, capture_rate_hz);
        match tx.process_capture(CaptureInput::Chunk(&pcm), &mut tx_batch) {
            TxProcessOutcome::Complete(_) | TxProcessOutcome::BatchFull(_) => {}
            other => panic!("unexpected TX outcome: {other:?}"),
        }
        while let Some(packet) = tx_batch.take_next() {
            packets.push(packet);
        }
    }
    assert!(packets.len() >= 5, "the short run must reach every stage");

    let encoded_packets = packets.len();
    let packet_micros = duration.samples_per_channel() as u64 * 1_000_000 / 48_000;
    let mut network = config
        .create_deterministic_network()
        .expect("deterministic network");
    let mut due = config.create_due_batch().expect("due batch");
    for (index, packet) in packets.into_iter().enumerate() {
        let outcome = network.schedule(packet, action_for(index, packet_micros, shape));
        assert!(
            matches!(
                outcome.status(),
                ScheduleStatus::Scheduled { .. } | ScheduleStatus::Dropped
            ),
            "fixed network unexpectedly rejected packet {index}: {:?}",
            outcome.status()
        );
    }

    let rx_stream = RxStreamConfig {
        ssrc: Ssrc::new(SSRC),
        payload_type: PayloadType::new(PAYLOAD_TYPE).expect("payload type"),
        initial_sequence: ExtendedSequence::new(INITIAL_SEQUENCE),
        initial_timestamp: RtpTimestamp::new(INITIAL_TIMESTAMP),
    };
    let mut rx = RxWorker::new(config, rx_stream).expect("RX worker");
    let mut playback_config = PlaybackConfig::for_pipeline(config);
    playback_config.drift_estimator.observation_window_seconds = 0.1;
    let (mut playback, mut renderer, ring_metrics) =
        playback_pair(config, playback_config).expect("playback pair");
    let mut rendered_bits = Vec::new();
    let mut frames = Vec::new();
    let mut maximum_ring_samples = 0;
    let mut estimated_remote_drift_ppm = None;
    let mut target_correction_ppm = 0.0_f64;

    let mut consume_frame = |outcome: relay_audio::FrameOutcome<'_>| {
        let (extended_media, scheduled_local) = scheduled_position(
            outcome.sequence(),
            outcome.timestamp(),
            duration,
            playback_rate_hz,
            scheduled_drift_ppm,
        );
        frames.push((
            outcome.sequence().get(),
            outcome.timestamp().get(),
            outcome.source(),
        ));
        let report = playback
            .process_frame(outcome.frame(), extended_media, scheduled_local)
            .expect("scheduled playback processing");
        assert_eq!(report.publication, PlaybackPublication::Published);
        estimated_remote_drift_ppm = report.estimated_remote_drift_ppm;
        target_correction_ppm = report.target_correction_ppm;
        maximum_ring_samples = maximum_ring_samples.max(renderer.available_samples());
        assert!(renderer.available_samples() <= config.playback_ring_samples());
        let mut output = vec![f32::NAN; report.output_frames * config.channels()];
        let render = renderer.render(&mut output);
        assert_eq!(render.state, RenderState::Complete);
        assert_eq!(render.rendered_samples, output.len());
        assert!(output.iter().all(|sample| sample.is_finite()));
        rendered_bits.extend(output.into_iter().map(f32::to_bits));
    };

    // Arrival is driven only by virtual NetworkTime. Playout position below is
    // independently derived from the scheduled RTP sequence/timestamp epoch.
    for slot in 0..encoded_packets {
        let deadline = (slot as u64 + 4) * packet_micros;
        network
            .advance_to(NetworkTime::from_micros(deadline), &mut due)
            .expect("monotonic virtual delivery");
        while let Some(packet) = due.take_next() {
            match rx.ingress(packet).status() {
                IngressStatus::AcceptedInOrder | IngressStatus::AcceptedReordered { .. } => {}
                IngressStatus::Rejected(IngressMismatch::Duplicate)
                    if matches!(shape, PathShape::LossReorderDuplicate) => {}
                status => panic!("unexpected rejection of a valid loop packet: {status:?}"),
            }
        }
        if let Some(outcome) = rx.tick() {
            consume_frame(outcome);
        }
    }
    let final_outcome = rx.drain().expect("final staged RX position must drain");
    consume_frame(final_outcome);

    assert_eq!(
        frames.len(),
        encoded_packets,
        "one output per scheduled position"
    );
    let last_index = encoded_packets as u64 - 1;
    let expected_final_timestamp =
        INITIAL_TIMESTAMP.wrapping_add((last_index * duration.samples_per_channel() as u64) as u32);
    assert_eq!(
        frames.last(),
        Some(&(
            INITIAL_SEQUENCE + last_index,
            expected_final_timestamp,
            frames.last().expect("final frame").2,
        )),
        "drain must emit the final sequence and timestamp",
    );
    assert_eq!(renderer.available_samples(), 0, "renderer drained the ring");
    let snapshot = ring_metrics.snapshot();
    let rx_metrics = rx.metrics();
    assert_eq!(rx_metrics.emitted_frames as usize, encoded_packets);
    assert_eq!(rx_metrics.identity_mismatches, 0);
    assert_eq!(rx_metrics.duration_timestamp_mismatches, 0);
    assert_eq!(rx_metrics.malformed_packets, 0);
    assert_eq!(rx_metrics.oversized_packets, 0);
    assert_eq!(rx_metrics.extension_rejections, 0);
    assert_eq!(rx_metrics.late, 0);
    assert_eq!(rx_metrics.ahead_of_window, 0);
    assert_eq!(rx_metrics.codec_errors, 0);
    match shape {
        PathShape::Nominal => {
            assert_eq!(rx_metrics.ingress_packets as usize, encoded_packets);
            assert_eq!(rx_metrics.duplicates, 0);
            assert_eq!(
                (rx_metrics.accepted_in_order + rx_metrics.accepted_reordered) as usize,
                encoded_packets,
            );
        }
        PathShape::LossReorderDuplicate => {
            assert_eq!(rx_metrics.ingress_packets as usize, encoded_packets - 2);
            assert_eq!(rx_metrics.duplicates, 1);
            assert_eq!(
                (rx_metrics.accepted_in_order + rx_metrics.accepted_reordered) as usize,
                encoded_packets - 3,
            );
        }
    }
    LoopResult {
        encoded_packets,
        rendered_bits,
        frames,
        rx_metrics,
        network_metrics: network.metrics(),
        playback_metrics: playback.metrics(),
        ring_dropped_samples: snapshot.dropped_samples,
        maximum_ring_samples,
        estimated_remote_drift_ppm_bits: estimated_remote_drift_ppm.map(f64::to_bits),
        target_correction_ppm_bits: target_correction_ppm.to_bits(),
    }
}

fn assert_finite_nontrivial_stereo(result: &LoopResult) {
    assert!(!result.rendered_bits.is_empty());
    let samples: Vec<f32> = result
        .rendered_bits
        .iter()
        .copied()
        .map(f32::from_bits)
        .collect();
    let left_energy: f64 = samples
        .chunks_exact(2)
        .map(|frame| f64::from(frame[0]).powi(2))
        .sum();
    let right_energy: f64 = samples
        .chunks_exact(2)
        .map(|frame| f64::from(frame[1]).powi(2))
        .sum();
    let stereo_difference: f64 = samples
        .chunks_exact(2)
        .map(|frame| f64::from(frame[0] - frame[1]).abs())
        .sum();
    assert!(left_energy > 0.01, "left output is trivial");
    assert!(right_energy > 0.01, "right output is trivial");
    assert!(stereo_difference > 0.1, "stereo channels collapsed");
    assert_eq!(result.ring_dropped_samples, 0);
    assert!(result.maximum_ring_samples > 0);
}

#[test]
fn nominal_loop_is_repeatable_and_covers_rates_and_packet_durations() {
    // Pairwise coverage avoids a 4 x 4 x 3 Cartesian matrix while exercising
    // every supported capture/playback rate and every negotiated duration.
    let cases = [
        (44_100, 192_000, FrameDuration::Ms5),
        (48_000, 96_000, FrameDuration::Ms10),
        (96_000, 48_000, FrameDuration::Ms20),
        (192_000, 44_100, FrameDuration::Ms10),
    ];
    for (capture_rate, playback_rate, duration) in cases {
        let result = run_loop(capture_rate, playback_rate, duration, PathShape::Nominal, 0);
        assert_finite_nontrivial_stereo(&result);
        assert_eq!(result.rx_metrics.plc_frames, 0);
        assert_eq!(result.playback_metrics.dropped_full_chunks, 0);
    }

    let first = run_loop(44_100, 48_000, FrameDuration::Ms5, PathShape::Nominal, 0);
    let second = run_loop(44_100, 48_000, FrameDuration::Ms5, PathShape::Nominal, 0);
    assert_eq!(first, second, "the complete real-codec loop changed");
}

#[test]
fn real_fec_plc_duplicate_delay_reorder_and_wrap_are_observable() {
    let result = run_loop(
        48_000,
        44_100,
        FrameDuration::Ms20,
        PathShape::LossReorderDuplicate,
        0,
    );
    assert_finite_nontrivial_stereo(&result);
    assert!(
        result
            .frames
            .iter()
            .any(|frame| frame.2 == FrameSource::InbandFecOrPlc),
        "the dropped packet must request the following real Opus packet's FEC path"
    );
    assert!(
        result
            .frames
            .iter()
            .any(|frame| frame.2 == FrameSource::PacketLossConcealment),
        "the two-packet hole must expose explicit PLC"
    );
    let source_at = |packet_index: u64| {
        result
            .frames
            .iter()
            .find(|frame| frame.0 == INITIAL_SEQUENCE + packet_index)
            .map(|frame| frame.2)
            .expect("scheduled gap position was emitted")
    };
    assert_eq!(source_at(4), FrameSource::InbandFecOrPlc);
    assert_eq!(source_at(7), FrameSource::PacketLossConcealment);
    assert_eq!(source_at(8), FrameSource::InbandFecOrPlc);
    assert_eq!(result.rx_metrics.fec_attempts, 2);
    assert_eq!(result.rx_metrics.plc_frames, 1);
    assert_eq!(result.rx_metrics.duplicates, 1);
    assert!(result.rx_metrics.accepted_reordered >= 1);
    assert_eq!(result.network_metrics.simulated_drops, 3);
    assert_eq!(result.network_metrics.duplicate_requests, 1);
    assert!(result.encoded_packets >= 10);
    // The chosen epoch crosses both 16-bit sequence and 32-bit timestamp wrap.
    assert!(INITIAL_SEQUENCE as u16 > (INITIAL_SEQUENCE + result.encoded_packets as u64) as u16);
    let timestamp_end = INITIAL_TIMESTAMP.wrapping_add(
        result.encoded_packets as u32 * FrameDuration::Ms20.samples_per_channel() as u32,
    );
    assert!(timestamp_end < INITIAL_TIMESTAMP);
}

#[test]
fn valid_rx_timeline_drives_both_scheduled_drift_correction_signs() {
    let positive = run_loop(48_000, 48_000, FrameDuration::Ms10, PathShape::Nominal, 400);
    let zero = run_loop(48_000, 48_000, FrameDuration::Ms10, PathShape::Nominal, 0);
    let negative = run_loop(
        48_000,
        48_000,
        FrameDuration::Ms10,
        PathShape::Nominal,
        -400,
    );

    let positive_drift = f64::from_bits(
        positive
            .estimated_remote_drift_ppm_bits
            .expect("positive scheduled drift estimate"),
    );
    let negative_drift = f64::from_bits(
        negative
            .estimated_remote_drift_ppm_bits
            .expect("negative scheduled drift estimate"),
    );
    assert!(
        positive_drift > 0.0,
        "positive remote clock: {positive_drift}"
    );
    assert!(
        negative_drift < 0.0,
        "negative remote clock: {negative_drift}"
    );
    assert!(f64::from_bits(positive.target_correction_ppm_bits) < 0.0);
    assert!(f64::from_bits(negative.target_correction_ppm_bits) > 0.0);
    assert!(f64::from_bits(zero.target_correction_ppm_bits).abs() < 1.0e-9);
}

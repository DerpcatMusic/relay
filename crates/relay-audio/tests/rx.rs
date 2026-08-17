use std::f32::consts::TAU;

use relay_audio::{
    AdaptiveClockConfig, AudioPipelineConfig, AudioPipelineConfigInput, Bitrate,
    ClockRecoveryConfig, EncoderPolicyV1, ExtendedSequence, FrameDuration, FrameSource,
    FrameStatus, InbandFec, IngressMismatch, IngressStatus, MAX_PACKET_BYTES, MediaPacket,
    PacketLossPercent, PayloadType, RtpTimestamp, RxMetrics, RxStreamConfig, RxWorker, Ssrc,
};
use relay_opus::{Encoder, EncoderConfigV1};

const SSRC: u32 = 0x1234_5678;
const PAYLOAD_TYPE: u8 = 111;
const INITIAL_SEQUENCE: u64 = 1_000;
const INITIAL_TIMESTAMP: u32 = 90_000;

fn pipeline(duration: FrameDuration) -> AudioPipelineConfig {
    pipeline_with_bounds(duration, 64, MAX_PACKET_BYTES)
}

fn pipeline_with_bounds(
    duration: FrameDuration,
    reorder_capacity: usize,
    packet_capacity: usize,
) -> AudioPipelineConfig {
    AudioPipelineConfig::new(AudioPipelineConfigInput {
        capture_rate_hz: 48_000,
        playback_rate_hz: 48_000,
        channels: 2,
        frame_duration: duration,
        capture_src_chunk_frames: 480,
        capture_ring_samples: 100_000,
        playback_ring_samples: 100_000,
        tx_accumulator_samples: 100_000,
        reorder_capacity,
        network_capacity: 8,
        network_due_batch_capacity: 8,
        packet_capacity,
        controller_cadence_frames: 480,
        clock_recovery: ClockRecoveryConfig::default(),
        adaptive_clock: AdaptiveClockConfig::default(),
    })
    .expect("valid RX test pipeline")
}

fn stream(
    initial_sequence: u64,
    initial_timestamp: u32,
    ssrc: u32,
    payload_type: u8,
) -> RxStreamConfig {
    RxStreamConfig {
        ssrc: Ssrc::new(ssrc),
        payload_type: PayloadType::new(payload_type).expect("valid payload type"),
        initial_sequence: ExtendedSequence::new(initial_sequence),
        initial_timestamp: RtpTimestamp::new(initial_timestamp),
    }
}

fn test_stream() -> RxStreamConfig {
    stream(INITIAL_SEQUENCE, INITIAL_TIMESTAMP, SSRC, PAYLOAD_TYPE)
}

fn encoder(duration: FrameDuration, fec: InbandFec, loss_percent: i32) -> Encoder {
    let policy = EncoderPolicyV1::new(
        Bitrate::try_new(64_000).expect("valid test bitrate"),
        fec,
        PacketLossPercent::try_new(loss_percent).expect("valid packet-loss hint"),
    );
    Encoder::new(EncoderConfigV1::stereo_48k(duration, policy)).expect("test encoder")
}

fn encoded_frames(
    duration: FrameDuration,
    count: usize,
    fec: InbandFec,
    loss_percent: i32,
) -> Vec<Vec<u8>> {
    let mut encoder = encoder(duration, fec, loss_percent);
    let frames = duration.samples_per_channel();
    let mut encoded = Vec::with_capacity(count);
    for packet_index in 0..count {
        let mut pcm = vec![0.0_f32; duration.interleaved_samples()];
        for frame in 0..frames {
            let absolute_frame = packet_index * frames + frame;
            pcm[frame * 2] = (TAU * 233.0 * absolute_frame as f32 / 48_000.0).sin() * 0.2;
            pcm[frame * 2 + 1] = (TAU * 377.0 * absolute_frame as f32 / 48_000.0).sin() * 0.15;
        }
        let mut storage = [0_u8; MAX_PACKET_BYTES];
        let len = encoder
            .encode(&pcm, &mut storage)
            .expect("encode deterministic frame");
        encoded.push(storage[..len].to_vec());
    }
    encoded
}

fn encoded_silence(duration: FrameDuration) -> Vec<u8> {
    let mut encoder = encoder(duration, InbandFec::Disabled, 0);
    let pcm = vec![0.0_f32; duration.interleaved_samples()];
    let mut storage = [0_u8; MAX_PACKET_BYTES];
    let len = encoder.encode(&pcm, &mut storage).expect("encode silence");
    storage[..len].to_vec()
}

fn timestamp_at(duration: FrameDuration, initial_timestamp: u32, offset: u64) -> u32 {
    initial_timestamp
        .wrapping_add(offset.wrapping_mul(duration.samples_per_channel() as u64) as u32)
}

fn packet_with(
    payload: &[u8],
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    payload_type: u8,
) -> MediaPacket {
    MediaPacket::try_new(ssrc, sequence, timestamp, payload_type, payload)
        .expect("test packet fits fixed storage")
}

fn packet_at(
    duration: FrameDuration,
    payload: &[u8],
    initial_sequence: u64,
    initial_timestamp: u32,
    offset: u64,
) -> MediaPacket {
    packet_with(
        payload,
        initial_sequence.wrapping_add(offset) as u16,
        timestamp_at(duration, initial_timestamp, offset),
        SSRC,
        PAYLOAD_TYPE,
    )
}

fn assert_frame_shape(
    duration: FrameDuration,
    outcome: &relay_audio::FrameOutcome<'_>,
    expected_sequence: u64,
    expected_timestamp: u32,
    expected_source: FrameSource,
) {
    assert_eq!(outcome.sequence(), ExtendedSequence::new(expected_sequence));
    assert_eq!(outcome.timestamp(), RtpTimestamp::new(expected_timestamp));
    assert_eq!(outcome.source(), expected_source);
    assert_eq!(outcome.status(), FrameStatus::Produced);
    assert_eq!(
        outcome.frame().samples_per_channel(),
        duration.samples_per_channel()
    );
    assert_eq!(
        outcome.frame().samples().len(),
        duration.interleaved_samples()
    );
    assert!(
        outcome
            .frame()
            .samples()
            .iter()
            .all(|sample| sample.is_finite())
    );
}

#[test]
fn all_durations_use_one_packet_lookahead_and_explicit_drain() {
    for duration in [FrameDuration::Ms5, FrameDuration::Ms10, FrameDuration::Ms20] {
        let config = pipeline(duration);
        let mut worker = RxWorker::new(config, test_stream()).expect("RX worker");
        let payloads = encoded_frames(duration, 2, InbandFec::Disabled, 0);

        assert_eq!(
            worker
                .ingress(packet_at(
                    duration,
                    &payloads[0],
                    INITIAL_SEQUENCE,
                    INITIAL_TIMESTAMP,
                    0,
                ))
                .status(),
            IngressStatus::AcceptedInOrder
        );
        assert_eq!(
            worker
                .ingress(packet_at(
                    duration,
                    &payloads[1],
                    INITIAL_SEQUENCE,
                    INITIAL_TIMESTAMP,
                    1,
                ))
                .status(),
            IngressStatus::AcceptedReordered { depth: 1 }
        );

        assert!(
            worker.tick().is_none(),
            "first {duration:?} tick is lookahead"
        );
        let first_pointer = {
            let first = worker.tick().expect("first frame after lookahead");
            assert_frame_shape(
                duration,
                &first,
                INITIAL_SEQUENCE,
                INITIAL_TIMESTAMP,
                FrameSource::Packet,
            );
            assert_eq!(
                first.frame().capacity(),
                FrameDuration::Ms20.interleaved_samples()
            );
            first.frame().samples().as_ptr() as usize
        };
        let second_pointer = {
            let second = worker.drain().expect("drain final staged packet");
            assert_frame_shape(
                duration,
                &second,
                INITIAL_SEQUENCE + 1,
                timestamp_at(duration, INITIAL_TIMESTAMP, 1),
                FrameSource::Packet,
            );
            second.frame().samples().as_ptr() as usize
        };
        assert_eq!(first_pointer, second_pointer, "fixed output storage moved");
        assert!(worker.drain().is_none());
    }
}

#[test]
fn reordered_duplicate_and_late_packets_have_distinct_owned_dispositions() {
    let duration = FrameDuration::Ms10;
    let mut worker = RxWorker::new(pipeline(duration), test_stream()).expect("RX worker");
    let payloads = encoded_frames(duration, 3, InbandFec::Disabled, 0);
    let first = packet_at(
        duration,
        &payloads[0],
        INITIAL_SEQUENCE,
        INITIAL_TIMESTAMP,
        0,
    );
    let third = packet_at(
        duration,
        &payloads[2],
        INITIAL_SEQUENCE,
        INITIAL_TIMESTAMP,
        2,
    );
    let second = packet_at(
        duration,
        &payloads[1],
        INITIAL_SEQUENCE,
        INITIAL_TIMESTAMP,
        1,
    );

    assert_eq!(
        worker.ingress(first).status(),
        IngressStatus::AcceptedInOrder
    );
    assert_eq!(
        worker.ingress(third).status(),
        IngressStatus::AcceptedReordered { depth: 2 }
    );
    assert_eq!(
        worker.ingress(second.clone()).status(),
        IngressStatus::AcceptedReordered { depth: 1 }
    );
    let duplicate = worker.ingress(second.clone());
    assert_eq!(
        duplicate.status(),
        IngressStatus::Rejected(IngressMismatch::Duplicate)
    );
    assert_eq!(duplicate.into_returned_packet(), Some(second));

    assert!(worker.tick().is_none());
    for expected_offset in 0..2 {
        let frame = worker.tick().expect("ordered buffered frame");
        assert_frame_shape(
            duration,
            &frame,
            INITIAL_SEQUENCE + expected_offset,
            timestamp_at(duration, INITIAL_TIMESTAMP, expected_offset),
            FrameSource::Packet,
        );
    }
    let third = worker.drain().expect("third reordered frame");
    assert_frame_shape(
        duration,
        &third,
        INITIAL_SEQUENCE + 2,
        timestamp_at(duration, INITIAL_TIMESTAMP, 2),
        FrameSource::Packet,
    );

    assert!(worker.tick().is_none());
    let late_packet = packet_at(
        duration,
        &payloads[0],
        INITIAL_SEQUENCE,
        INITIAL_TIMESTAMP,
        3,
    );
    let late = worker.ingress(late_packet.clone());
    assert_eq!(
        late.status(),
        IngressStatus::Rejected(IngressMismatch::Late { distance: 1 })
    );
    assert_eq!(late.into_returned_packet(), Some(late_packet));

    let metrics = worker.metrics();
    assert_eq!(metrics.accepted_in_order, 1);
    assert_eq!(metrics.accepted_reordered, 2);
    assert_eq!(metrics.duplicates, 1);
    assert_eq!(metrics.late, 1);
}

fn assert_rejection_preserves_owned_packet_and_epoch(
    duration: FrameDuration,
    bad_packet: MediaPacket,
    expected: IngressMismatch,
    valid_payload: &[u8],
) {
    let mut worker = RxWorker::new(pipeline(duration), test_stream()).expect("RX worker");
    let returned_copy = bad_packet.clone();
    let rejected = worker.ingress(bad_packet);
    assert_eq!(rejected.status(), IngressStatus::Rejected(expected));
    assert_eq!(rejected.into_returned_packet(), Some(returned_copy));
    let mut expected_metrics = RxMetrics {
        ingress_packets: 1,
        ..RxMetrics::default()
    };
    match expected {
        IngressMismatch::Ssrc { .. } | IngressMismatch::PayloadType { .. } => {
            expected_metrics.identity_mismatches = 1;
        }
        IngressMismatch::Timestamp { .. } | IngressMismatch::Duration { .. } => {
            expected_metrics.duration_timestamp_mismatches = 1;
        }
        IngressMismatch::MalformedPacket => expected_metrics.malformed_packets = 1,
        _ => panic!("unexpected rejection in metadata helper"),
    }
    assert_eq!(worker.metrics(), expected_metrics);

    let valid = packet_at(
        duration,
        valid_payload,
        INITIAL_SEQUENCE,
        INITIAL_TIMESTAMP,
        0,
    );
    assert_eq!(
        worker.ingress(valid).status(),
        IngressStatus::AcceptedInOrder
    );
    assert!(worker.tick().is_none());
    let initial = worker.drain().expect("rejection did not advance epoch");
    assert_frame_shape(
        duration,
        &initial,
        INITIAL_SEQUENCE,
        INITIAL_TIMESTAMP,
        FrameSource::Packet,
    );
    expected_metrics.ingress_packets = 2;
    expected_metrics.accepted_in_order = 1;
    expected_metrics.deadline_decisions = 1;
    expected_metrics.emitted_frames = 1;
    expected_metrics.packet_frames = 1;
    assert_eq!(worker.metrics(), expected_metrics);
}

#[test]
fn metadata_duration_and_malformed_rejections_return_ownership_without_epoch_mutation() {
    let duration = FrameDuration::Ms20;
    let valid_payload = encoded_silence(duration);
    let initial_wire = INITIAL_SEQUENCE as u16;

    assert_rejection_preserves_owned_packet_and_epoch(
        duration,
        packet_with(
            &valid_payload,
            initial_wire,
            INITIAL_TIMESTAMP,
            SSRC ^ 1,
            PAYLOAD_TYPE,
        ),
        IngressMismatch::Ssrc {
            expected: Ssrc::new(SSRC),
            actual: Ssrc::new(SSRC ^ 1),
        },
        &valid_payload,
    );
    assert_rejection_preserves_owned_packet_and_epoch(
        duration,
        packet_with(
            &valid_payload,
            initial_wire,
            INITIAL_TIMESTAMP,
            SSRC,
            PAYLOAD_TYPE - 1,
        ),
        IngressMismatch::PayloadType {
            expected: PayloadType::new(PAYLOAD_TYPE).expect("payload type"),
            actual: PayloadType::new(PAYLOAD_TYPE - 1).expect("payload type"),
        },
        &valid_payload,
    );
    assert_rejection_preserves_owned_packet_and_epoch(
        duration,
        packet_with(
            &valid_payload,
            initial_wire,
            INITIAL_TIMESTAMP.wrapping_add(1),
            SSRC,
            PAYLOAD_TYPE,
        ),
        IngressMismatch::Timestamp {
            expected: RtpTimestamp::new(INITIAL_TIMESTAMP),
            actual: RtpTimestamp::new(INITIAL_TIMESTAMP.wrapping_add(1)),
        },
        &valid_payload,
    );

    let wrong_duration = encoded_silence(FrameDuration::Ms10);
    assert_rejection_preserves_owned_packet_and_epoch(
        duration,
        packet_with(
            &wrong_duration,
            initial_wire,
            INITIAL_TIMESTAMP,
            SSRC,
            PAYLOAD_TYPE,
        ),
        IngressMismatch::Duration {
            expected_samples_per_channel: FrameDuration::Ms20.samples_per_channel(),
            actual_samples_per_channel: FrameDuration::Ms10.samples_per_channel(),
        },
        &valid_payload,
    );
    assert_rejection_preserves_owned_packet_and_epoch(
        duration,
        packet_with(&[0xff], initial_wire, INITIAL_TIMESTAMP, SSRC, PAYLOAD_TYPE),
        IngressMismatch::MalformedPacket,
        &valid_payload,
    );
}

#[test]
fn packet_capacity_rejection_has_an_oversized_snapshot_and_returns_ownership() {
    let duration = FrameDuration::Ms20;
    let payload = encoded_silence(duration);
    assert!(payload.len() > 1);
    let config = pipeline_with_bounds(duration, 4, payload.len() - 1);
    let mut worker = RxWorker::new(config, test_stream()).expect("RX worker");
    let packet = packet_at(duration, &payload, INITIAL_SEQUENCE, INITIAL_TIMESTAMP, 0);
    let returned = packet.clone();
    let outcome = worker.ingress(packet);
    assert_eq!(
        outcome.status(),
        IngressStatus::Rejected(IngressMismatch::PacketTooLarge {
            maximum: payload.len() - 1,
            actual: payload.len(),
        })
    );
    assert_eq!(outcome.into_returned_packet(), Some(returned));
    assert_eq!(
        worker.metrics(),
        RxMetrics {
            ingress_packets: 1,
            oversized_packets: 1,
            ..RxMetrics::default()
        }
    );
}

#[test]
fn reorder_capacity_wrap_half_range_and_before_epoch_have_full_snapshots() {
    let duration = FrameDuration::Ms10;
    let payload = encoded_silence(duration);
    let initial_sequence = (3_u64 << 16) | u64::from(u16::MAX - 1);
    let initial_timestamp = u32::MAX - 100;
    let mut capacity_worker = RxWorker::new(
        pipeline_with_bounds(duration, 4, MAX_PACKET_BYTES),
        stream(initial_sequence, initial_timestamp, SSRC, PAYLOAD_TYPE),
    )
    .expect("capacity worker");
    assert_eq!(
        capacity_worker
            .ingress(packet_at(
                duration,
                &payload,
                initial_sequence,
                initial_timestamp,
                3,
            ))
            .status(),
        IngressStatus::AcceptedReordered { depth: 3 }
    );
    assert_eq!((initial_sequence + 3) as u16, 1, "accepted edge wraps");
    assert_eq!(
        capacity_worker
            .ingress(packet_at(
                duration,
                &payload,
                initial_sequence,
                initial_timestamp,
                4,
            ))
            .status(),
        IngressStatus::Rejected(IngressMismatch::AheadOfWindow { distance: 4 })
    );
    assert_eq!((initial_sequence + 4) as u16, 2, "rejected edge wraps");
    assert_eq!(
        capacity_worker.metrics(),
        RxMetrics {
            ingress_packets: 2,
            accepted_reordered: 1,
            ahead_of_window: 1,
            ..RxMetrics::default()
        }
    );

    let mut ambiguous_worker = RxWorker::new(pipeline(duration), test_stream()).expect("RX worker");
    let ambiguous = packet_with(
        &payload,
        (INITIAL_SEQUENCE as u16).wrapping_add(0x8000),
        INITIAL_TIMESTAMP,
        SSRC,
        PAYLOAD_TYPE,
    );
    assert_eq!(
        ambiguous_worker.ingress(ambiguous).status(),
        IngressStatus::Rejected(IngressMismatch::AmbiguousSequence)
    );
    assert_eq!(
        ambiguous_worker.metrics(),
        RxMetrics {
            ingress_packets: 1,
            extension_rejections: 1,
            ..RxMetrics::default()
        }
    );

    let mut before_epoch_worker = RxWorker::new(
        pipeline(duration),
        stream(0, INITIAL_TIMESTAMP, SSRC, PAYLOAD_TYPE),
    )
    .expect("RX worker");
    let before_epoch = packet_with(&payload, u16::MAX, INITIAL_TIMESTAMP, SSRC, PAYLOAD_TYPE);
    assert_eq!(
        before_epoch_worker.ingress(before_epoch).status(),
        IngressStatus::Rejected(IngressMismatch::SequenceBeforeEpoch)
    );
    assert_eq!(
        before_epoch_worker.metrics(),
        RxMetrics {
            ingress_packets: 1,
            extension_rejections: 1,
            ..RxMetrics::default()
        }
    );
}

#[test]
fn u64_overflow_exhaustion_and_reset_are_exact_and_fully_snapshotted() {
    let duration = FrameDuration::Ms10;
    let payloads = encoded_frames(duration, 3, InbandFec::Disabled, 0);
    let initial_sequence = u64::MAX - 1;
    let initial_timestamp = u32::MAX - 100;
    let mut worker = RxWorker::new(
        pipeline(duration),
        stream(initial_sequence, initial_timestamp, SSRC, PAYLOAD_TYPE),
    )
    .expect("RX worker");

    let overflow = packet_with(
        &payloads[2],
        initial_sequence.wrapping_add(2) as u16,
        timestamp_at(duration, initial_timestamp, 2),
        SSRC,
        PAYLOAD_TYPE,
    );
    assert_eq!(
        worker.ingress(overflow).status(),
        IngressStatus::Rejected(IngressMismatch::SequenceOverflow)
    );
    for (offset, payload) in payloads[..2].iter().enumerate() {
        let expected = if offset == 0 {
            IngressStatus::AcceptedInOrder
        } else {
            IngressStatus::AcceptedReordered { depth: 1 }
        };
        assert_eq!(
            worker
                .ingress(packet_at(
                    duration,
                    payload,
                    initial_sequence,
                    initial_timestamp,
                    offset as u64,
                ))
                .status(),
            expected
        );
    }
    assert!(worker.tick().is_none());
    assert_eq!(
        worker
            .tick()
            .expect("penultimate decision")
            .sequence()
            .get(),
        u64::MAX - 1
    );
    assert_eq!(
        worker.tick().expect("final decision").sequence().get(),
        u64::MAX
    );
    assert!(worker.tick().is_none());
    assert!(worker.drain().is_none());
    assert_eq!(
        worker.metrics(),
        RxMetrics {
            ingress_packets: 3,
            accepted_in_order: 1,
            accepted_reordered: 1,
            extension_rejections: 1,
            deadline_decisions: 2,
            emitted_frames: 2,
            packet_frames: 2,
            ..RxMetrics::default()
        }
    );

    let reset_stream = stream(42, 77, SSRC, PAYLOAD_TYPE);
    worker.reset(reset_stream).expect("reset after exhaustion");
    assert_eq!(worker.metrics(), RxMetrics::default());
    assert_eq!(
        worker
            .ingress(packet_with(&payloads[2], 42, 77, SSRC, PAYLOAD_TYPE,))
            .status(),
        IngressStatus::AcceptedInOrder
    );
    assert!(worker.tick().is_none());
    assert_eq!(worker.drain().expect("reset recovery").sequence().get(), 42);
    assert_eq!(
        worker.metrics(),
        RxMetrics {
            ingress_packets: 1,
            accepted_in_order: 1,
            deadline_decisions: 1,
            emitted_frames: 1,
            packet_frames: 1,
            ..RxMetrics::default()
        }
    );
}

#[test]
fn sequence_and_timestamp_wrap_preserve_extended_order() {
    let duration = FrameDuration::Ms20;
    let initial_sequence = (7_u64 << 16) | u64::from(u16::MAX);
    let initial_timestamp = u32::MAX - 400;
    let mut worker = RxWorker::new(
        pipeline(duration),
        stream(initial_sequence, initial_timestamp, SSRC, PAYLOAD_TYPE),
    )
    .expect("RX worker");
    let payloads = encoded_frames(duration, 3, InbandFec::Disabled, 0);

    for (offset, payload) in payloads.iter().enumerate() {
        let outcome = worker.ingress(packet_at(
            duration,
            payload,
            initial_sequence,
            initial_timestamp,
            offset as u64,
        ));
        let expected = if offset == 0 {
            IngressStatus::AcceptedInOrder
        } else {
            IngressStatus::AcceptedReordered {
                depth: offset as u16,
            }
        };
        assert_eq!(outcome.status(), expected);
    }

    assert!(worker.tick().is_none());
    for offset in 0..2 {
        let frame = worker.tick().expect("wrapped frame");
        assert_frame_shape(
            duration,
            &frame,
            initial_sequence + offset,
            timestamp_at(duration, initial_timestamp, offset),
            FrameSource::Packet,
        );
    }
    let final_frame = worker.drain().expect("final wrapped frame");
    assert_frame_shape(
        duration,
        &final_frame,
        initial_sequence + 2,
        timestamp_at(duration, initial_timestamp, 2),
        FrameSource::Packet,
    );
    assert_eq!((initial_sequence + 1) as u16, 0);
    assert!(timestamp_at(duration, initial_timestamp, 1) < initial_timestamp);

    assert!(
        worker.tick().is_none(),
        "stage one wrapped missing deadline"
    );
    let late = worker.ingress(packet_at(
        duration,
        &payloads[0],
        initial_sequence,
        initial_timestamp,
        3,
    ));
    assert_eq!(
        late.status(),
        IngressStatus::Rejected(IngressMismatch::Late { distance: 1 })
    );
    let far_offset = 68;
    assert!(timestamp_at(duration, initial_timestamp, far_offset) < initial_timestamp);
    let far_ahead = worker.ingress(packet_at(
        duration,
        &payloads[1],
        initial_sequence,
        initial_timestamp,
        far_offset,
    ));
    assert_eq!(
        far_ahead.status(),
        IngressStatus::Rejected(IngressMismatch::AheadOfWindow { distance: 64 })
    );
    assert_eq!(
        worker.metrics(),
        RxMetrics {
            ingress_packets: 5,
            accepted_in_order: 1,
            accepted_reordered: 2,
            late: 1,
            ahead_of_window: 1,
            deadline_decisions: 4,
            emitted_frames: 3,
            packet_frames: 3,
            ..RxMetrics::default()
        }
    );
}

#[test]
fn consecutive_deadline_gaps_produce_exact_finite_plc_frames() {
    let duration = FrameDuration::Ms5;
    let mut worker = RxWorker::new(pipeline(duration), test_stream()).expect("RX worker");

    assert!(worker.tick().is_none());
    for offset in 0..2 {
        let frame = worker.tick().expect("consecutive PLC frame");
        assert_frame_shape(
            duration,
            &frame,
            INITIAL_SEQUENCE + offset,
            timestamp_at(duration, INITIAL_TIMESTAMP, offset),
            FrameSource::PacketLossConcealment,
        );
    }
    let final_gap = worker.drain().expect("drain final gap");
    assert_frame_shape(
        duration,
        &final_gap,
        INITIAL_SEQUENCE + 2,
        timestamp_at(duration, INITIAL_TIMESTAMP, 2),
        FrameSource::PacketLossConcealment,
    );
    assert_eq!(worker.metrics().plc_frames, 3);
}

fn assert_following_packet_uses_honest_fec_or_plc_source(fec: InbandFec, loss_percent: i32) {
    let duration = FrameDuration::Ms20;
    let payloads = encoded_frames(duration, 2, fec, loss_percent);
    let mut worker = RxWorker::new(pipeline(duration), test_stream()).expect("RX worker");

    // The first consecutive frame was encoded by the same encoder but is deliberately dropped.
    let following = packet_at(
        duration,
        &payloads[1],
        INITIAL_SEQUENCE,
        INITIAL_TIMESTAMP,
        1,
    );
    assert_eq!(
        worker.ingress(following).status(),
        IngressStatus::AcceptedReordered { depth: 1 }
    );
    assert!(worker.tick().is_none());
    let recovered_or_concealed = worker.tick().expect("FEC request result");
    assert_frame_shape(
        duration,
        &recovered_or_concealed,
        INITIAL_SEQUENCE,
        INITIAL_TIMESTAMP,
        FrameSource::InbandFecOrPlc,
    );
    assert_eq!(worker.metrics().fec_attempts, 1);

    let following = worker.drain().expect("following packet normal decode");
    assert_frame_shape(
        duration,
        &following,
        INITIAL_SEQUENCE + 1,
        timestamp_at(duration, INITIAL_TIMESTAMP, 1),
        FrameSource::Packet,
    );
}

#[test]
fn fec_enabled_consecutive_non_silent_frames_request_fec_without_claiming_lbrr() {
    assert_following_packet_uses_honest_fec_or_plc_source(InbandFec::Enabled, 20);
}

#[test]
fn no_fec_packet_uses_the_same_honest_fec_or_plc_fallback_source() {
    assert_following_packet_uses_honest_fec_or_plc_source(InbandFec::Disabled, 0);
}

#[test]
fn trusted_reset_clears_pending_buffered_history_metrics_and_rebases_epoch() {
    let duration = FrameDuration::Ms10;
    let config = pipeline(duration);
    let mut worker = RxWorker::new(config, test_stream()).expect("RX worker");
    let payloads = encoded_frames(duration, 3, InbandFec::Disabled, 0);
    assert_eq!(
        worker
            .ingress(packet_at(
                duration,
                &payloads[0],
                INITIAL_SEQUENCE,
                INITIAL_TIMESTAMP,
                0,
            ))
            .status(),
        IngressStatus::AcceptedInOrder
    );
    assert_eq!(
        worker
            .ingress(packet_at(
                duration,
                &payloads[2],
                INITIAL_SEQUENCE,
                INITIAL_TIMESTAMP,
                2,
            ))
            .status(),
        IngressStatus::AcceptedReordered { depth: 2 }
    );
    assert!(worker.tick().is_none());
    assert_ne!(worker.metrics(), relay_audio::RxMetrics::default());

    let new_sequence = 70_000_u64;
    let new_timestamp = u32::MAX - 100;
    let new_ssrc = SSRC ^ 0xffff_0000;
    let new_payload_type = PAYLOAD_TYPE - 1;
    let new_stream = stream(new_sequence, new_timestamp, new_ssrc, new_payload_type);
    worker.reset(new_stream).expect("trusted reset");

    assert_eq!(worker.metrics(), relay_audio::RxMetrics::default());
    assert_eq!(worker.reorder_capacity(), config.reorder_capacity());
    assert!(worker.drain().is_none());

    let old_epoch_packet = packet_at(
        duration,
        &payloads[0],
        INITIAL_SEQUENCE,
        INITIAL_TIMESTAMP,
        0,
    );
    assert!(matches!(
        worker.ingress(old_epoch_packet).status(),
        IngressStatus::Rejected(IngressMismatch::Ssrc { .. })
    ));

    let new_packet = packet_with(
        &payloads[1],
        new_sequence as u16,
        new_timestamp,
        new_ssrc,
        new_payload_type,
    );
    assert_eq!(
        worker.ingress(new_packet).status(),
        IngressStatus::AcceptedInOrder
    );
    assert!(worker.tick().is_none());
    let reset_frame = worker.drain().expect("new epoch frame");
    assert_frame_shape(
        duration,
        &reset_frame,
        new_sequence,
        new_timestamp,
        FrameSource::Packet,
    );
}

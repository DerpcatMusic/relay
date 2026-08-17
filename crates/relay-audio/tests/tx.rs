use std::f32::consts::TAU;

use relay_audio::{
    AdaptiveClockConfig, AudioPipelineConfig, AudioPipelineConfigInput, Bitrate, CaptureInput,
    ClockRecoveryConfig, EncoderPolicyV1, FinalFramePolicy, FiniteTxWorker, FrameDuration,
    InbandFec, MAX_PACKET_BYTES, PacketBatch, PacketLossPercent, PayloadType, RtpTimestamp,
    SequenceNumber, Ssrc, TxProcessOutcome, TxStreamConfig, TxWorker,
};
use relay_opus::{Decoder, DecoderConfig};

fn pipeline(rate: usize, duration: FrameDuration) -> AudioPipelineConfig {
    AudioPipelineConfig::new(AudioPipelineConfigInput {
        capture_rate_hz: rate,
        playback_rate_hz: 48_000,
        channels: 2,
        frame_duration: duration,
        capture_src_chunk_frames: rate / 100,
        capture_ring_samples: rate / 50 * 2,
        playback_ring_samples: 16_000,
        tx_accumulator_samples: 16_000,
        reorder_capacity: 64,
        network_capacity: 32,
        network_due_batch_capacity: 32,
        packet_capacity: MAX_PACKET_BYTES,
        controller_cadence_frames: 480,
        clock_recovery: ClockRecoveryConfig::default(),
        adaptive_clock: AdaptiveClockConfig::default(),
    })
    .expect("valid TX shape")
}

fn stream() -> TxStreamConfig {
    TxStreamConfig {
        ssrc: Ssrc::new(0x1234_5678),
        payload_type: PayloadType::new(111).expect("payload type"),
        initial_sequence: SequenceNumber::new(65_530),
        initial_timestamp: RtpTimestamp::new(u32::MAX - 1_000),
        encoding_policy: EncoderPolicyV1::new(
            Bitrate::try_new(128_000).expect("bitrate"),
            InbandFec::Disabled,
            PacketLossPercent::ZERO,
        ),
    }
}

fn deterministic_pcm(start: usize, frames: usize, rate: usize, right: bool) -> Vec<f32> {
    let mut pcm = vec![0.0; frames * 2];
    for frame in 0..frames {
        pcm[frame * 2] = (TAU * 440.0 * (start + frame) as f32 / rate as f32).sin() * 0.2;
        if right {
            pcm[frame * 2 + 1] = (TAU * 733.0 * (start + frame) as f32 / rate as f32).sin() * 0.15;
        }
    }
    pcm
}

fn run_live(
    rate: usize,
    duration: FrameDuration,
    chunks: usize,
    right: bool,
) -> Vec<relay_audio::MediaPacket> {
    let config = pipeline(rate, duration);
    let mut tx = TxWorker::new(config, stream()).expect("worker");
    let mut batch = PacketBatch::new(32).expect("batch");
    let frames = tx.capture_chunk_samples() / 2;
    let mut packets = Vec::new();
    for chunk in 0..chunks {
        let pcm = deterministic_pcm(chunk * frames, frames, rate, right);
        match tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch) {
            TxProcessOutcome::Complete(_) | TxProcessOutcome::BatchFull(_) => {}
            other => panic!("unexpected process outcome: {other:?}"),
        }
        while let Some(packet) = batch.take_next() {
            packets.push(packet);
        }
    }
    loop {
        match tx.process_capture(CaptureInput::Disconnected, &mut batch) {
            TxProcessOutcome::Disconnected(_) => {
                while let Some(packet) = batch.take_next() {
                    packets.push(packet);
                }
                break;
            }
            TxProcessOutcome::BatchFull(_) => {
                while let Some(packet) = batch.take_next() {
                    packets.push(packet);
                }
            }
            other => panic!("unexpected disconnect outcome: {other:?}"),
        }
    }
    packets
}

#[test]
fn every_capture_rate_and_frame_duration_is_same_run_deterministic() {
    for rate in [44_100, 48_000, 96_000, 192_000] {
        for duration in [FrameDuration::Ms5, FrameDuration::Ms10, FrameDuration::Ms20] {
            let first = run_live(rate, duration, 24, true);
            let second = run_live(rate, duration, 24, true);
            let nominal_packets = 24 * 480 / duration.samples_per_channel();
            assert!(
                (nominal_packets.saturating_sub(1)..=nominal_packets).contains(&first.len()),
                "unexpected bounded streaming count for {rate} {duration:?}: {}",
                first.len()
            );
            assert_eq!(
                first, second,
                "same-run determinism for {rate} {duration:?}"
            );
            for pair in first.windows(2) {
                assert_eq!(
                    pair[1].sequence().get(),
                    pair[0].sequence().get().wrapping_add(1)
                );
                assert_eq!(
                    pair[1].timestamp().get(),
                    pair[0]
                        .timestamp()
                        .get()
                        .wrapping_add(duration.samples_per_channel() as u32)
                );
                assert!(!pair[0].payload().is_empty());
            }
        }
    }
}

#[test]
fn forty_four_one_five_ms_accumulates_two_packets_per_ten_ms_after_priming() {
    let packets = run_live(44_100, FrameDuration::Ms5, 40, false);
    // FFT streaming retains bounded startup/tail state, but 400 ms produces a
    // stable complete-frame count and never a short negotiated packet.
    assert!(
        packets.len() >= 70 && packets.len() <= 80,
        "{}",
        packets.len()
    );
    let mut decoder = Decoder::new(DecoderConfig::stereo_48k(FrameDuration::Ms5)).expect("decoder");
    let mut pcm = vec![0.0; FrameDuration::Ms5.interleaved_samples()];
    for packet in &packets {
        let decoded = decoder
            .decode(packet.payload(), &mut pcm)
            .expect("fixed duration");
        assert_eq!(decoded.samples_per_channel(), 240);
    }
}

#[test]
fn sequence_timestamp_wrap_and_batch_backpressure_are_exact() {
    let config = pipeline(48_000, FrameDuration::Ms5);
    let mut tx = TxWorker::new(config, stream()).expect("worker");
    let pcm = vec![0.0; tx.capture_chunk_samples()];
    let mut batch = PacketBatch::new(1).expect("batch");
    assert!(matches!(
        tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch),
        TxProcessOutcome::BatchFull(_)
    ));
    let first = batch.take_next().expect("first");
    assert_eq!(first.sequence().get(), 65_530);
    assert_eq!(first.timestamp().get(), u32::MAX - 1_000);
    assert!(matches!(
        tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch),
        TxProcessOutcome::BatchFull(_)
    ));
    let second = batch.take_next().expect("second");
    assert_eq!(second.sequence().get(), 65_531);
    assert_eq!(
        second.timestamp().get(),
        (u32::MAX - 1_000).wrapping_add(240)
    );
}

#[test]
fn stereo_channels_remain_isolated_through_encode_decode() {
    let packets = run_live(48_000, FrameDuration::Ms10, 8, false);
    let mut decoder =
        Decoder::new(DecoderConfig::stereo_48k(FrameDuration::Ms10)).expect("decoder");
    let mut pcm = vec![0.0; FrameDuration::Ms10.interleaved_samples()];
    let mut left_energy = 0.0_f64;
    let mut right_peak = 0.0_f32;
    for packet in packets {
        decoder.decode(packet.payload(), &mut pcm).expect("decode");
        for frame in 0..480 {
            left_energy += f64::from(pcm[frame * 2]).powi(2);
            right_peak = right_peak.max(pcm[frame * 2 + 1].abs());
        }
    }
    assert!(left_energy > 1.0);
    assert!(right_peak < 1.0e-5, "right channel leaked: {right_peak}");
}

#[test]
fn nonfinite_is_recoverable_without_timeline_advance() {
    let config = pipeline(48_000, FrameDuration::Ms10);
    let mut tx = TxWorker::new(config, stream()).expect("worker");
    let mut batch = PacketBatch::new(4).expect("batch");
    let mut bad = vec![0.0; tx.capture_chunk_samples()];
    bad[17] = f32::NAN;
    assert!(matches!(
        tx.process_capture(CaptureInput::Chunk(&bad), &mut batch),
        TxProcessOutcome::Error(_)
    ));
    let good = vec![0.0; tx.capture_chunk_samples()];
    assert!(matches!(
        tx.process_capture(CaptureInput::Chunk(&good), &mut batch),
        TxProcessOutcome::Complete(_)
    ));
    let packet = batch.take_next().expect("packet after reset");
    assert_eq!(
        (packet.sequence().get(), packet.timestamp().get()),
        (65_530, u32::MAX - 1_000)
    );
}

#[test]
fn live_disconnect_discards_only_partial_frame_and_all_terminal_states_are_explicit() {
    let config = pipeline(48_000, FrameDuration::Ms20);
    let mut tx = TxWorker::new(config, stream()).expect("worker");
    let mut batch = PacketBatch::new(2).expect("batch");
    let wrong = vec![0.0; tx.capture_chunk_samples() - 2];
    assert!(matches!(
        tx.process_capture(CaptureInput::Chunk(&wrong), &mut batch),
        TxProcessOutcome::Error(relay_audio::TxProcessFailure {
            cause: relay_audio::TxError::InvalidCaptureLength { .. },
            ..
        })
    ));

    let half_packet = vec![0.0; tx.capture_chunk_samples()];
    assert!(matches!(
        tx.process_capture(CaptureInput::Chunk(&half_packet), &mut batch),
        TxProcessOutcome::Complete(_)
    ));
    assert!(batch.is_empty());
    let disconnect = tx.process_capture(CaptureInput::Disconnected, &mut batch);
    let TxProcessOutcome::Disconnected(report) = disconnect else {
        panic!("unexpected disconnect: {disconnect:?}");
    };
    assert_eq!(report.discarded_partial_media_frames, 480);
    assert_eq!(report.abandoned_converter_tail_frames, 0);
    assert!(batch.is_empty());
    assert!(matches!(
        tx.process_capture(CaptureInput::Disconnected, &mut batch),
        TxProcessOutcome::Error(relay_audio::TxProcessFailure {
            cause: relay_audio::TxError::AlreadyDisconnected,
            ..
        })
    ));
}

#[test]
fn per_pipeline_packet_capacity_is_enforced_at_encode_and_packet_creation() {
    let input = AudioPipelineConfigInput {
        capture_rate_hz: 48_000,
        playback_rate_hz: 48_000,
        channels: 2,
        frame_duration: FrameDuration::Ms5,
        capture_src_chunk_frames: 240,
        capture_ring_samples: 480,
        playback_ring_samples: 16_000,
        tx_accumulator_samples: 16_000,
        reorder_capacity: 64,
        network_capacity: 32,
        network_due_batch_capacity: 32,
        packet_capacity: 1,
        controller_cadence_frames: 480,
        clock_recovery: ClockRecoveryConfig::default(),
        adaptive_clock: AdaptiveClockConfig::default(),
    };
    let config = AudioPipelineConfig::new(input).expect("one-byte bounded pipeline");
    let mut tx = TxWorker::new(config, stream()).expect("worker");
    let pcm: Vec<f32> = (0..tx.capture_chunk_samples())
        .map(|index| (index as f32 * 0.137).sin() * 0.8)
        .collect();
    let mut batch = PacketBatch::new(1).expect("batch");
    assert!(matches!(
        tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch),
        TxProcessOutcome::Complete(_)
    ));
    let packet = batch.take_next().expect("bounded packet");
    assert_eq!(packet.payload_len(), 1);
    assert_eq!(packet.sequence().get(), 65_530);
}

#[test]
fn finite_path_reports_trim_and_explicit_zero_padding() {
    let config = pipeline(44_100, FrameDuration::Ms20);
    let stream = stream();
    let frames = 1_000;
    let input = deterministic_pcm(0, frames, 44_100, true);
    let mut finite = FiniteTxWorker::new(config, stream, frames).expect("finite worker");
    let mut batch = PacketBatch::new(8).expect("batch");
    let report = finite
        .process_finite(&input, FinalFramePolicy::ZeroPad, &mut batch)
        .expect("finite process");
    assert_eq!(report.resampler.input_frames, frames);
    assert_eq!(
        report.resampler.output_frames,
        (frames * 48_000).div_ceil(44_100)
    );
    assert_eq!(
        report.resampler.generated_output_frames,
        report.resampler.leading_trim_frames
            + report.resampler.output_frames
            + report.resampler.trailing_trim_frames
    );
    assert!(report.zero_padded_media_frames > 0);
    assert_eq!(
        report.final_valid_media_frames + report.zero_padded_media_frames,
        960
    );
}

#[test]
fn nonunity_reports_direct_src_accounting_and_actual_pending_frames() {
    let config = pipeline(44_100, FrameDuration::Ms5);
    let mut tx = TxWorker::new(config, stream()).expect("worker");
    let frames = tx.capture_chunk_samples() / 2;
    let mut batch = PacketBatch::new(8).expect("batch");
    let mut consumed = 0;
    let mut produced = 0;
    let mut emitted = 0;
    for chunk in 0..12 {
        let pcm = deterministic_pcm(chunk * frames, frames, 44_100, true);
        let outcome = tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch);
        let report = match outcome {
            TxProcessOutcome::Complete(report) | TxProcessOutcome::BatchFull(report) => report,
            other => panic!("unexpected accounting outcome: {other:?}"),
        };
        assert!(!report.input_pending);
        consumed += report.capture_frames_consumed;
        produced += report.media_frames_produced;
        emitted += report.packets_emitted;
        batch.clear();
        assert_eq!(
            produced,
            emitted * FrameDuration::Ms5.samples_per_channel() + tx.accumulated_media_frames()
        );
    }
    assert_eq!(consumed, 12 * frames);
}

#[test]
fn repeated_one_slot_backpressure_preserves_input_ownership() {
    let config = AudioPipelineConfig::new(AudioPipelineConfigInput {
        capture_rate_hz: 48_000,
        playback_rate_hz: 48_000,
        channels: 2,
        frame_duration: FrameDuration::Ms5,
        capture_src_chunk_frames: 960,
        capture_ring_samples: 1_920,
        playback_ring_samples: 16_000,
        tx_accumulator_samples: 16_000,
        reorder_capacity: 64,
        network_capacity: 32,
        network_due_batch_capacity: 32,
        packet_capacity: MAX_PACKET_BYTES,
        controller_cadence_frames: 480,
        clock_recovery: ClockRecoveryConfig::default(),
        adaptive_clock: AdaptiveClockConfig::default(),
    })
    .expect("config");
    let mut tx = TxWorker::new(config, stream()).expect("worker");
    let pcm = vec![0.0; tx.capture_chunk_samples()];
    let mut batch = PacketBatch::new(1).expect("batch");
    let first = tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch);
    let TxProcessOutcome::BatchFull(first) = first else {
        panic!("{first:?}")
    };
    assert_eq!(first.capture_frames_consumed, 960);
    assert_eq!(first.media_frames_produced, 960);
    assert_eq!(first.packets_emitted, 1);
    assert!(!first.input_pending);
    batch.clear();

    for _ in 0..2 {
        let blocked = tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch);
        let TxProcessOutcome::BatchFull(blocked) = blocked else {
            panic!("{blocked:?}")
        };
        assert_eq!(blocked.capture_frames_consumed, 0);
        assert_eq!(blocked.media_frames_produced, 0);
        assert_eq!(blocked.packets_emitted, 1);
        assert!(blocked.input_pending);
        batch.clear();
    }

    let retried = tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch);
    let TxProcessOutcome::BatchFull(retried) = retried else {
        panic!("{retried:?}")
    };
    assert_eq!(retried.capture_frames_consumed, 960);
    assert_eq!(retried.media_frames_produced, 960);
    assert!(!retried.input_pending);
}

#[test]
fn disconnect_rejects_chunks_while_draining() {
    let config = AudioPipelineConfig::new(AudioPipelineConfigInput {
        capture_rate_hz: 48_000,
        playback_rate_hz: 48_000,
        channels: 2,
        frame_duration: FrameDuration::Ms5,
        capture_src_chunk_frames: 960,
        capture_ring_samples: 1_920,
        playback_ring_samples: 16_000,
        tx_accumulator_samples: 16_000,
        reorder_capacity: 64,
        network_capacity: 32,
        network_due_batch_capacity: 32,
        packet_capacity: MAX_PACKET_BYTES,
        controller_cadence_frames: 480,
        clock_recovery: ClockRecoveryConfig::default(),
        adaptive_clock: AdaptiveClockConfig::default(),
    })
    .expect("config");
    let mut tx = TxWorker::new(config, stream()).expect("worker");
    let pcm = vec![0.0; tx.capture_chunk_samples()];
    let mut batch = PacketBatch::new(1).expect("batch");
    assert!(matches!(
        tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch),
        TxProcessOutcome::BatchFull(_)
    ));
    batch.clear();
    assert!(matches!(
        tx.process_capture(CaptureInput::Disconnected, &mut batch),
        TxProcessOutcome::BatchFull(_)
    ));
    batch.clear();
    assert!(matches!(
        tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch),
        TxProcessOutcome::Error(relay_audio::TxProcessFailure {
            cause: relay_audio::TxError::AlreadyDisconnected,
            ..
        })
    ));
    assert!(batch.is_empty());
    loop {
        match tx.process_capture(CaptureInput::Disconnected, &mut batch) {
            TxProcessOutcome::BatchFull(_) => batch.clear(),
            TxProcessOutcome::Disconnected(_) => break,
            other => panic!("unexpected drain outcome: {other:?}"),
        }
    }
}

#[test]
fn nonunity_disconnect_distinguishes_configured_delay_from_abandoned_tail() {
    let config = pipeline(44_100, FrameDuration::Ms10);
    let mut immediate = TxWorker::new(config, stream()).expect("worker");
    let mut batch = PacketBatch::new(8).expect("batch");
    let TxProcessOutcome::Disconnected(empty) =
        immediate.process_capture(CaptureInput::Disconnected, &mut batch)
    else {
        panic!("disconnect")
    };
    assert!(empty.configured_converter_delay_frames > 0);
    assert_eq!(empty.abandoned_converter_tail_frames, 0);

    let mut active = TxWorker::new(config, stream()).expect("worker");
    let pcm = vec![0.0; active.capture_chunk_samples()];
    assert!(matches!(
        active.process_capture(CaptureInput::Chunk(&pcm), &mut batch),
        TxProcessOutcome::Complete(_)
    ));
    batch.clear();
    let TxProcessOutcome::Disconnected(used) =
        active.process_capture(CaptureInput::Disconnected, &mut batch)
    else {
        panic!("disconnect")
    };
    assert_eq!(
        used.abandoned_converter_tail_frames,
        used.configured_converter_delay_frames
    );
    assert!(used.abandoned_converter_tail_frames > 0);
}

#[test]
fn finite_capacity_preflight_consumes_zero_and_reports_required_capacity() {
    let config = pipeline(44_100, FrameDuration::Ms20);
    let frames = 1_000;
    let input = deterministic_pcm(0, frames, 44_100, true);
    let mut finite = FiniteTxWorker::new(config, stream(), frames).expect("finite");
    let mut small = PacketBatch::new(1).expect("batch");
    let blocked = finite
        .process_finite(&input, FinalFramePolicy::ZeroPad, &mut small)
        .expect("preflight");
    assert!(blocked.batch_full);
    assert_eq!(blocked.resampler.input_frames, 0);
    assert_eq!(blocked.packets_emitted, 0);
    assert_eq!(blocked.required_batch_capacity, 2);
    assert!(small.is_empty());

    let mut enough = PacketBatch::new(blocked.required_batch_capacity).expect("batch");
    let done = finite
        .process_finite(&input, FinalFramePolicy::ZeroPad, &mut enough)
        .expect("retry");
    assert!(!done.batch_full);
    assert_eq!(done.resampler.input_frames, frames);
}

#[test]
fn finite_require_complete_is_zero_progress_retryable_and_empty_is_explicit() {
    let config = pipeline(44_100, FrameDuration::Ms20);
    let frames = 1_000;
    let input = deterministic_pcm(0, frames, 44_100, true);
    let mut finite = FiniteTxWorker::new(config, stream(), frames).expect("finite");
    let mut batch = PacketBatch::new(8).expect("batch");
    let rejected = finite
        .process_finite(&input, FinalFramePolicy::RequireComplete, &mut batch)
        .expect_err("incomplete output must be rejected");
    assert_eq!(
        rejected.cause,
        relay_audio::TxError::IncompleteFinalOpusFrame {
            valid_media_frames: 129,
            packet_frames: 960,
        }
    );
    assert_eq!(rejected.input_frames_consumed, 0);
    assert_eq!(rejected.packets_emitted, 0);
    assert!(batch.is_empty());

    let report = finite
        .process_finite(&input, FinalFramePolicy::ZeroPad, &mut batch)
        .expect("same worker remains retryable");
    assert_eq!(report.packets_emitted, 2);
    assert_eq!(report.final_valid_media_frames, 129);
    assert_eq!(report.zero_padded_media_frames, 831);

    batch.clear();
    let repeated = finite
        .process_finite(&input, FinalFramePolicy::ZeroPad, &mut batch)
        .expect_err("successful operation is one shot");
    assert_eq!(repeated.cause, relay_audio::TxError::FiniteAlreadyProcessed);
    assert_eq!(repeated.input_frames_consumed, 0);
    assert_eq!(repeated.packets_emitted, 0);

    let mut empty = FiniteTxWorker::new(config, stream(), frames).expect("finite");
    let empty_report = empty
        .process_finite(&[], FinalFramePolicy::RequireComplete, &mut batch)
        .expect("empty");
    assert_eq!(empty_report.resampler.input_frames, 0);
    assert_eq!(empty_report.packets_emitted, 0);

    let mut recoverable = FiniteTxWorker::new(config, stream(), frames).expect("finite");
    let mut bad = input.clone();
    bad[3] = f32::INFINITY;
    let failure = recoverable
        .process_finite(&bad, FinalFramePolicy::ZeroPad, &mut batch)
        .expect_err("nonfinite");
    assert_eq!(failure.input_frames_consumed, 0);
    assert_eq!(failure.packets_emitted, 0);
    recoverable
        .process_finite(&input, FinalFramePolicy::ZeroPad, &mut batch)
        .expect("usable after validation");
}

#[test]
fn finite_require_complete_rejects_partial_frames_for_all_durations() {
    for duration in [FrameDuration::Ms5, FrameDuration::Ms10, FrameDuration::Ms20] {
        let packet_frames = duration.samples_per_channel();
        let source_frames = packet_frames + 1;
        let input = deterministic_pcm(0, source_frames, 48_000, true);
        let mut finite = FiniteTxWorker::new(pipeline(48_000, duration), stream(), source_frames)
            .expect("finite");
        let mut batch = PacketBatch::new(8).expect("batch");
        let rejected = finite
            .process_finite(&input, FinalFramePolicy::RequireComplete, &mut batch)
            .expect_err("one-frame remainder");
        assert_eq!(
            rejected.cause,
            relay_audio::TxError::IncompleteFinalOpusFrame {
                valid_media_frames: 1,
                packet_frames,
            }
        );
        assert_eq!(
            (rejected.input_frames_consumed, rejected.packets_emitted),
            (0, 0)
        );
        assert!(batch.is_empty());
    }
}

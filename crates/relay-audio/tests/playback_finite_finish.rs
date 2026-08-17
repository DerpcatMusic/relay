//! Real finite TX -> RX lookahead/drain -> playback completion regressions.

use std::f32::consts::TAU;

use relay_audio::{
    AdaptiveClockConfig, AudioPipelineConfig, AudioPipelineConfigInput, Bitrate,
    ClockRecoveryConfig, EncoderPolicyV1, ExtendedSequence, ExtendedTimestamp, FinalFramePolicy,
    FinitePlaybackEnd, FinitePlaybackInput, FiniteTxWorker, FrameDuration, InbandFec,
    IngressStatus, MAX_PACKET_BYTES, PacketBatch, PacketLossPercent, PayloadType, PlaybackConfig,
    PlaybackFinishStatus, PlaybackPublication, RenderState, RtpTimestamp, RxStreamConfig, RxWorker,
    SequenceNumber, Ssrc, TxStreamConfig, playback_pair,
};

const SOURCE_RATE_HZ: usize = 44_100;
const PLAYBACK_RATE_HZ: usize = 44_100;
const SOURCE_FRAMES: usize = 1_000;
const PACKET_FRAMES: usize = 960;

fn pipeline() -> AudioPipelineConfig {
    AudioPipelineConfig::new(AudioPipelineConfigInput {
        capture_rate_hz: SOURCE_RATE_HZ,
        playback_rate_hz: PLAYBACK_RATE_HZ,
        channels: 2,
        frame_duration: FrameDuration::Ms20,
        capture_src_chunk_frames: 441,
        capture_ring_samples: 100_000,
        playback_ring_samples: 100_000,
        tx_accumulator_samples: 100_000,
        reorder_capacity: 8,
        network_capacity: 2,
        network_due_batch_capacity: 2,
        packet_capacity: MAX_PACKET_BYTES,
        controller_cadence_frames: 441,
        clock_recovery: ClockRecoveryConfig::default(),
        adaptive_clock: AdaptiveClockConfig::default(),
    })
    .expect("valid real finite pipeline")
}

fn stream() -> TxStreamConfig {
    TxStreamConfig {
        ssrc: Ssrc::new(0x0f1a_17e0),
        payload_type: PayloadType::new(111).expect("payload type"),
        initial_sequence: SequenceNumber::new(7),
        initial_timestamp: RtpTimestamp::new(0),
        encoding_policy: EncoderPolicyV1::new(
            Bitrate::try_new(96_000).expect("bitrate"),
            InbandFec::Disabled,
            PacketLossPercent::ZERO,
        ),
    }
}

fn source() -> Vec<f32> {
    let mut samples = vec![0.0; SOURCE_FRAMES * 2];
    for frame in 0..SOURCE_FRAMES {
        samples[frame * 2] = (TAU * 311.0 * frame as f32 / SOURCE_RATE_HZ as f32).sin() * 0.2;
        samples[frame * 2 + 1] = (TAU * 617.0 * frame as f32 / SOURCE_RATE_HZ as f32).sin() * 0.15;
    }
    samples
}

#[test]
fn zero_pad_manifest_reaches_the_withheld_rx_frame_once_and_playback_drains_completely() {
    let pipeline = pipeline();
    let stream = stream();
    let mut tx = FiniteTxWorker::new(pipeline, stream, SOURCE_FRAMES).expect("finite TX");
    let mut packets = PacketBatch::new(2).expect("two-packet batch");
    let tx_report = tx
        .process_finite(&source(), FinalFramePolicy::ZeroPad, &mut packets)
        .expect("finite TX completion");
    assert_eq!(tx_report.packets_emitted, 2);
    assert_eq!(tx_report.final_valid_media_frames, 129);
    assert_eq!(tx_report.zero_padded_media_frames, 831);
    assert_eq!(
        tx_report.final_valid_media_frames + tx_report.zero_padded_media_frames,
        PACKET_FRAMES
    );

    let mut rx = RxWorker::new(
        pipeline,
        RxStreamConfig {
            ssrc: stream.ssrc,
            payload_type: stream.payload_type,
            initial_sequence: ExtendedSequence::new(u64::from(stream.initial_sequence.get())),
            initial_timestamp: stream.initial_timestamp,
        },
    )
    .expect("RX");
    assert_eq!(
        rx.ingress(packets.take_next().expect("first packet"))
            .status(),
        IngressStatus::AcceptedInOrder
    );
    assert_eq!(
        rx.ingress(packets.take_next().expect("last packet"))
            .status(),
        IngressStatus::AcceptedReordered { depth: 1 }
    );
    assert!(packets.is_empty());

    let (mut playback, mut renderer, _) =
        playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("playback");

    // The first deadline establishes lookahead. Only the first packet is ever
    // ordinary live playback; the last packet remains staged for explicit drain.
    assert!(rx.tick().is_none());
    let streaming_output_frames = {
        let first = rx.tick().expect("first deadline output");
        let report = playback
            .process_frame(
                first.frame(),
                ExtendedTimestamp::new(u64::from(first.timestamp().get())),
                0,
            )
            .expect("ordinary first packet playback");
        assert_eq!(report.publication, PlaybackPublication::Published);
        report.output_frames
    };

    let finish = {
        let withheld = rx.drain().expect("one withheld final RX frame");
        assert_eq!(withheld.sequence().get(), 8);
        assert_eq!(
            withheld.timestamp(),
            RtpTimestamp::new(PACKET_FRAMES as u32)
        );
        playback
            .finish_finite(FinitePlaybackEnd::Final(FinitePlaybackInput {
                frame: withheld.frame(),
                valid_media_frames: tx_report.final_valid_media_frames,
                remote_media_sample_position: ExtendedTimestamp::new(u64::from(
                    withheld.timestamp().get(),
                )),
                scheduled_local_device_frame: 882,
            }))
            .expect("finish the one withheld frame")
    };
    assert!(
        rx.drain().is_none(),
        "the final RX frame was consumed exactly once"
    );
    assert_eq!(finish.status, PlaybackFinishStatus::Finished);
    assert_eq!(
        finish.input_frames_consumed,
        tx_report.final_valid_media_frames
    );
    assert_eq!(finish.pending_output_frames, 0);
    assert_eq!(
        finish.valid_output_frames,
        finish.generated_output_frames - finish.trailing_trim_frames
    );

    let queued_frames = renderer.available_samples() / pipeline.channels();
    assert_eq!(
        queued_frames - finish.leading_trim_frames,
        streaming_output_frames + finish.generated_output_frames
            - finish.leading_trim_frames
            - finish.trailing_trim_frames,
        "collected - L must equal S + G - L - T"
    );

    drop(playback);
    let mut collected = Vec::with_capacity(queued_frames * pipeline.channels());
    let mut terminal_acknowledgements = 0;
    while terminal_acknowledgements == 0 {
        let old_len = collected.len();
        collected.resize(old_len + 128, f32::NAN);
        let render = renderer.render(&mut collected[old_len..]);
        collected.truncate(old_len + render.rendered_samples);
        if render.state == RenderState::Disconnected {
            terminal_acknowledgements += 1;
            assert_eq!(renderer.available_samples(), 0);
        } else {
            assert_eq!(render.state, RenderState::Complete);
            assert!(renderer.available_samples() > 0);
        }
    }
    assert_eq!(terminal_acknowledgements, 1);
    assert_eq!(collected.len(), queued_frames * pipeline.channels());
    assert!(collected.iter().all(|sample| sample.is_finite()));
}

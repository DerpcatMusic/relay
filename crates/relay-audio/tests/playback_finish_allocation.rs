use relay_audio::{
    AdaptiveClockConfig, AudioPipelineConfig, AudioPipelineConfigInput, Bitrate,
    ClockRecoveryConfig, EncoderPolicyV1, ExtendedSequence, ExtendedTimestamp, FinalFramePolicy,
    FinitePlaybackEnd, FinitePlaybackInput, FiniteTxWorker, FrameDuration, InbandFec,
    IngressStatus, MAX_PACKET_BYTES, PacketBatch, PacketLossPercent, PayloadType, PlaybackConfig,
    PlaybackFinishStatus, RtpTimestamp, RxStreamConfig, RxWorker, SequenceNumber, Ssrc,
    TxStreamConfig, playback_pair,
};
use relay_resample_test_allocator::CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

fn pipeline() -> AudioPipelineConfig {
    AudioPipelineConfig::new(AudioPipelineConfigInput {
        capture_rate_hz: 44_100,
        playback_rate_hz: 44_100,
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
    .expect("valid allocation-test pipeline")
}

#[test]
fn live_finish_continue_and_render_allocate_nothing_after_construction() {
    let pipeline = pipeline();
    let ssrc = Ssrc::new(7);
    let payload_type = PayloadType::new(111).expect("payload type");
    let policy = EncoderPolicyV1::new(
        Bitrate::try_new(96_000).expect("bitrate"),
        InbandFec::Disabled,
        PacketLossPercent::ZERO,
    );
    let tx_stream = TxStreamConfig {
        ssrc,
        payload_type,
        initial_sequence: SequenceNumber::new(0),
        initial_timestamp: RtpTimestamp::new(0),
        encoding_policy: policy,
    };
    let mut tx = FiniteTxWorker::new(pipeline, tx_stream, 1_000).expect("finite TX");
    let mut packets = PacketBatch::new(2).expect("packet batch");
    let source = vec![0.125; 2_000];
    let tx_report = tx
        .process_finite(&source, FinalFramePolicy::ZeroPad, &mut packets)
        .expect("two-packet finite source");
    assert_eq!(tx_report.packets_emitted, 2);
    assert_eq!(tx_report.final_valid_media_frames, 129);
    assert_eq!(tx_report.zero_padded_media_frames, 831);

    let rx_stream = RxStreamConfig {
        ssrc,
        payload_type,
        initial_sequence: ExtendedSequence::new(0),
        initial_timestamp: RtpTimestamp::new(0),
    };
    let mut rx = RxWorker::new(pipeline, rx_stream).expect("RX");
    assert_eq!(
        rx.ingress(packets.take_next().expect("first encoded packet"))
            .status(),
        IngressStatus::AcceptedInOrder
    );
    assert_eq!(
        rx.ingress(packets.take_next().expect("last encoded packet"))
            .status(),
        IngressStatus::AcceptedReordered { depth: 1 }
    );
    assert!(rx.tick().is_none());
    let live = rx.tick().expect("ordinary first RX frame").frame().clone();
    let withheld = rx.drain().expect("withheld final RX frame").frame().clone();
    assert!(rx.drain().is_none());

    let (mut worker, mut renderer, _) =
        playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline)).expect("playback pair");
    let live_identity = worker.output_workspace_identity();
    let finish_identity = worker.finish_workspace_identity();
    let mut render_workspace = vec![0.0; 100_000];

    // Prewarm the exact paths with the ordinary frame and the distinct withheld
    // final frame, then drain and reset without moving fixed storage.
    worker
        .process_frame(&live, ExtendedTimestamp::new(0), 0)
        .expect("prewarm live");
    let prewarm = worker
        .finish_finite(FinitePlaybackEnd::Final(FinitePlaybackInput {
            frame: &withheld,
            valid_media_frames: tx_report.final_valid_media_frames,
            remote_media_sample_position: ExtendedTimestamp::new(960),
            scheduled_local_device_frame: 882,
        }))
        .expect("prewarm finish");
    assert_eq!(prewarm.status, PlaybackFinishStatus::Finished);
    let queued = renderer.available_samples();
    let _ = renderer.render(&mut render_workspace[..queued]);
    worker.reset_when_empty().expect("prewarm reset");

    ALLOCATOR.reset();
    worker
        .process_frame(&live, ExtendedTimestamp::new(0), 0)
        .expect("measured live");
    let finish = worker
        .finish_finite(FinitePlaybackEnd::Final(FinitePlaybackInput {
            frame: &withheld,
            valid_media_frames: tx_report.final_valid_media_frames,
            remote_media_sample_position: ExtendedTimestamp::new(960),
            scheduled_local_device_frame: 882,
        }))
        .expect("measured finish");
    assert_eq!(finish.status, PlaybackFinishStatus::Finished);
    let repeated = worker
        .finish_finite(FinitePlaybackEnd::Continue)
        .expect("measured idempotent continue");
    assert_eq!(repeated.status, PlaybackFinishStatus::Finished);
    let queued = renderer.available_samples();
    let _ = renderer.render(&mut render_workspace[..queued]);
    let allocations = ALLOCATOR.allocations();

    assert_eq!(allocations, 0);
    assert_eq!(worker.output_workspace_identity(), live_identity);
    assert_eq!(worker.finish_workspace_identity(), finish_identity);
}

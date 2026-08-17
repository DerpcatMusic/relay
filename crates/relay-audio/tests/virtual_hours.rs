//! Deterministic virtual-time endurance gate for receive ordering and clock recovery.
//!
//! This test ends at a synthetic decode/sample-count boundary. It exercises the production
//! reorder and clock-control primitives for twelve virtual hours, but does not run libopus
//! or the sample-value resampler kernel.

use relay_audio::{
    AdaptiveClockConfig, AudioPipelineConfig, AudioPipelineConfigInput, ClockRecoveryConfig,
    FrameDuration, MAX_PACKET_BYTES, PlaybackConfig,
};
use relay_clock::{
    ClockRecovery, DriftEstimator, DriftEstimatorConfig, DriftEstimatorUpdate,
    PlayoutClockObservation,
};
use relay_jitter::{AcceptedPacket, Playout, PushResult, RejectedPacket, ReorderBuffer};
use relay_resample::{OutputInputRatioCorrectionPpm, SUPPORTED_SAMPLE_RATES};

const MEDIA_RATE_HZ: u64 = 48_000;
const VIRTUAL_HOURS: u64 = 12;
const START_SEQUENCE: u16 = u16::MAX - 5;
const PLAYOUT_DELAY_PACKETS: u64 = 4;
const NETWORK_SLOTS: usize = 8;
const MAX_PACKETS_PER_SLOT: usize = 8;
const RING_SAMPLES: usize = 100_000;
const CHANNELS: usize = 2;
const WARMUP_MINUTES: u64 = 20;
const NOMINAL_PEAK_CORRECTION_PPM: f64 = 0.05;
const NOMINAL_RMS_CORRECTION_PPM: f64 = 0.02;
const PACKET_DURATIONS_MS: [u32; 3] = [5, 10, 20];

/// Approved remote-drift set. Stage 0 is longer so warmup plus a nominal jitter window fit.
const STAGES: [Stage; 7] = [
    Stage {
        minutes: 120,
        drift_ppm: 0,
        impairment: Impairment::WarmupThenNominalJitter,
    },
    Stage {
        minutes: 100,
        drift_ppm: -250,
        impairment: Impairment::Clean,
    },
    Stage {
        minutes: 100,
        drift_ppm: -100,
        impairment: Impairment::ExactLossPercent { percent: 1 },
    },
    Stage {
        minutes: 100,
        drift_ppm: -20,
        impairment: Impairment::ExactLossPercent { percent: 5 },
    },
    Stage {
        minutes: 100,
        drift_ppm: 20,
        impairment: Impairment::DelaySteps,
    },
    Stage {
        minutes: 100,
        drift_ppm: 100,
        impairment: Impairment::LossBursts,
    },
    Stage {
        minutes: 100,
        drift_ppm: 250,
        impairment: Impairment::DuplicatesAndReorder,
    },
];

#[derive(Clone, Copy, Debug)]
struct Stage {
    minutes: u64,
    drift_ppm: i32,
    impairment: Impairment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Impairment {
    WarmupThenNominalJitter,
    Clean,
    ExactLossPercent { percent: u8 },
    DelaySteps,
    LossBursts,
    DuplicatesAndReorder,
}

#[derive(Clone, Copy, Debug)]
struct Packet {
    sequence: u16,
    timestamp: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct NetworkTruth {
    generated: u64,
    network_lost: u64,
    delivered: u64,
    accepted: u64,
    duplicates: u64,
    reordered_accepts: u64,
    arrival_inversions: u64,
    delayed: u64,
    on_time: u64,
    late: u64,
    delay_buckets_ms: [u64; 11],
    burst_lost: u64,
    primary: u64,
    fec_or_plc: u64,
    plc: u64,
    synthetic_media_frames: u64,
}

impl NetworkTruth {
    fn increment(counter: &mut u64) {
        *counter = counter.checked_add(1).expect("metric counter overflow");
    }

    fn add_frames(counter: &mut u64, frames: u64) {
        *counter = counter
            .checked_add(frames)
            .expect("synthetic sample-count overflow");
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct PlantTotals {
    produced_frames: u64,
    consumed_frames: u64,
    warmup_observations: u64,
    estimate_updates: u64,
    first_estimate_deadline: Option<u64>,
    saturated_updates: u64,
    drift_input_clamped: u64,
    ring_fill_input_clamped: u64,
    integral_limited: u64,
    anti_windup_active: u64,
    amplitude_limited: u64,
    slew_limited: u64,
    min_fill_frames: f64,
    max_fill_frames: f64,
    sequence_wraps: u64,
    timestamp_wraps: u64,
    terminal_remote_media_frames: u64,
    terminal_local_device_frames: u64,
}

#[derive(Clone, Copy, Debug)]
struct StageSnapshot {
    visited: bool,
    drift_ppm: f64,
    correction_ppm: f64,
    end_fill_error_frames: f64,
    min_fill_frames: f64,
    max_fill_frames: f64,
    correction_peak_ppm: f64,
    correction_sum_squares: f64,
    correction_samples: u64,
}

impl Default for StageSnapshot {
    fn default() -> Self {
        Self {
            visited: false,
            drift_ppm: f64::NAN,
            correction_ppm: f64::NAN,
            end_fill_error_frames: f64::NAN,
            min_fill_frames: f64::INFINITY,
            max_fill_frames: f64::NEG_INFINITY,
            correction_peak_ppm: 0.0,
            correction_sum_squares: 0.0,
            correction_samples: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum PendingDecision {
    Packet { timestamp: u32 },
    Missing,
}

#[derive(Clone, Copy, Debug)]
enum SyntheticDecode {
    Primary,
    InbandFecOrPlc,
    Plc,
}

fn synthetic_decode_sample_count(kind: SyntheticDecode, media_frames: u64) -> u64 {
    match kind {
        SyntheticDecode::Primary | SyntheticDecode::InbandFecOrPlc | SyntheticDecode::Plc => {
            media_frames
        }
    }
}

fn frame_duration(packet_duration_ms: u32) -> FrameDuration {
    FrameDuration::try_from(u16::try_from(packet_duration_ms).expect("duration fits u16"))
        .expect("supported packet duration")
}

fn packets_for_minutes(minutes: u64, packet_duration_ms: u32) -> u64 {
    minutes
        .checked_mul(60_000)
        .expect("minute-to-ms overflow")
        .checked_div(u64::from(packet_duration_ms))
        .expect("packet duration is non-zero")
}

fn packet_count(packet_duration_ms: u32) -> u64 {
    VIRTUAL_HOURS
        .checked_mul(3_600_000)
        .expect("hour-to-ms overflow")
        .checked_div(u64::from(packet_duration_ms))
        .expect("packet duration is non-zero")
}

fn exact_sequence_wraps(generated: u64) -> u64 {
    let first_wrap_tick = u64::from(u16::MAX)
        .checked_sub(u64::from(START_SEQUENCE))
        .and_then(|value| value.checked_add(1))
        .expect("sequence wrap origin");
    if generated <= first_wrap_tick {
        0
    } else {
        generated
            .checked_sub(first_wrap_tick)
            .and_then(|span| span.checked_sub(1))
            .expect("sequence wrap span")
            / u64::from(u16::MAX)
            + 1
    }
}

fn exact_timestamp_wraps(packet_count: u64, media_frames: u64, timestamp_start: u32) -> u64 {
    let start = u64::from(timestamp_start);
    let last_index = packet_count.checked_sub(1).expect("non-empty stream");
    let advance = last_index
        .checked_mul(media_frames)
        .expect("timestamp advance overflow");
    let end = start
        .checked_add(advance)
        .expect("extended timestamp overflow");
    end / (u64::from(u32::MAX) + 1) - start / (u64::from(u32::MAX) + 1)
}

fn wire_timestamp(timestamp_start: u32, packet_index: u64, media_frames: u64) -> u32 {
    let extended = u64::from(timestamp_start)
        .checked_add(
            packet_index
                .checked_mul(media_frames)
                .expect("timestamp product overflow"),
        )
        .expect("extended timestamp overflow");
    u32::try_from(extended & u64::from(u32::MAX)).expect("wire timestamp fits")
}

fn media_to_device_frames(media_frames: u64, device_rate_hz: u64, drift_ppm: i32) -> u64 {
    let numerator = i128::from(media_frames)
        .checked_mul(i128::from(device_rate_hz))
        .and_then(|value| value.checked_mul(1_000_000))
        .expect("media-to-device numerator overflow");
    let denominator = i128::from(MEDIA_RATE_HZ)
        .checked_mul(1_000_000 + i128::from(drift_ppm))
        .expect("media-to-device denominator overflow");
    assert!(denominator > 0, "drift must keep the rate factor positive");
    let rounded = (numerator + denominator / 2) / denominator;
    u64::try_from(rounded).expect("device frames fit u64")
}

fn stage_packet_starts(packet_duration_ms: u32) -> [u64; 8] {
    let mut starts = [0_u64; 8];
    for (index, stage) in STAGES.iter().enumerate() {
        starts[index + 1] = starts[index]
            .checked_add(packets_for_minutes(stage.minutes, packet_duration_ms))
            .expect("stage packet start overflow");
    }
    starts
}

fn stage_local_offsets(
    media_frames: u64,
    device_rate_hz: u64,
    packet_duration_ms: u32,
) -> [u64; 8] {
    let mut offsets = [0_u64; 8];
    for (index, stage) in STAGES.iter().enumerate() {
        let stage_media = packets_for_minutes(stage.minutes, packet_duration_ms)
            .checked_mul(media_frames)
            .expect("stage media overflow");
        offsets[index + 1] = offsets[index]
            .checked_add(media_to_device_frames(
                stage_media,
                device_rate_hz,
                stage.drift_ppm,
            ))
            .expect("stage local offset overflow");
    }
    offsets
}

fn scheduled_local_position(
    remote_packet_boundary: u64,
    media_frames: u64,
    device_rate_hz: u64,
    packet_duration_ms: u32,
) -> u64 {
    let starts = stage_packet_starts(packet_duration_ms);
    let offsets = stage_local_offsets(media_frames, device_rate_hz, packet_duration_ms);
    scheduled_local_from_tables(
        remote_packet_boundary,
        media_frames,
        device_rate_hz,
        starts,
        offsets,
    )
}

fn scheduled_local_from_tables(
    remote_packet_boundary: u64,
    media_frames: u64,
    device_rate_hz: u64,
    starts: [u64; 8],
    offsets: [u64; 8],
) -> u64 {
    if remote_packet_boundary == starts[STAGES.len()] {
        return offsets[STAGES.len()];
    }
    let mut index = 0;
    while index + 1 < STAGES.len() && remote_packet_boundary >= starts[index + 1] {
        index += 1;
    }
    let within = remote_packet_boundary
        .checked_sub(starts[index])
        .expect("boundary precedes stage");
    offsets[index]
        .checked_add(media_to_device_frames(
            within
                .checked_mul(media_frames)
                .expect("partial stage media overflow"),
            device_rate_hz,
            STAGES[index].drift_ppm,
        ))
        .expect("scheduled local overflow")
}

fn stage_index_for_packet(packet_index: u64, packet_duration_ms: u32) -> usize {
    let mut remaining = packet_index;
    for (index, stage) in STAGES.iter().enumerate() {
        let stage_packets = packets_for_minutes(stage.minutes, packet_duration_ms);
        if remaining < stage_packets {
            return index;
        }
        remaining -= stage_packets;
    }
    STAGES.len() - 1
}

fn within_stage_packet(packet_index: u64, packet_duration_ms: u32) -> u64 {
    let mut remaining = packet_index;
    for stage in STAGES {
        let stage_packets = packets_for_minutes(stage.minutes, packet_duration_ms);
        if remaining < stage_packets {
            return remaining;
        }
        remaining -= stage_packets;
    }
    remaining
}

fn delay_ms_for(packet_index: u64, packet_duration_ms: u32) -> u64 {
    let stage = STAGES[stage_index_for_packet(packet_index, packet_duration_ms)];
    let within = within_stage_packet(packet_index, packet_duration_ms);
    match stage.impairment {
        Impairment::WarmupThenNominalJitter => {
            let warmup_packets = packets_for_minutes(WARMUP_MINUTES, packet_duration_ms);
            if within < warmup_packets {
                0
            } else {
                nominal_jitter_delay_ms(within - warmup_packets)
            }
        }
        Impairment::DelaySteps => 1 + (packet_index % 10),
        Impairment::Clean
        | Impairment::ExactLossPercent { .. }
        | Impairment::LossBursts
        | Impairment::DuplicatesAndReorder => 0,
    }
}

fn nominal_jitter_delay_ms(index: u64) -> u64 {
    let ramp = 1 + (index % 1_000) * 9 / 999;
    let jitter = i64::try_from(index % 11).expect("mod 11 fits") - 5;
    (i64::try_from(ramp).expect("ramp fits") + jitter).clamp(0, 10) as u64
}

fn is_lost(packet_index: u64, packet_duration_ms: u32) -> bool {
    let stage = STAGES[stage_index_for_packet(packet_index, packet_duration_ms)];
    match stage.impairment {
        Impairment::ExactLossPercent { percent: 1 } => packet_index % 100 == 99,
        Impairment::ExactLossPercent { percent: 5 } => packet_index % 20 == 19,
        Impairment::ExactLossPercent { percent } => {
            panic!("unsupported exact loss percent {percent}")
        }
        Impairment::LossBursts => packet_index % 200 < 3,
        Impairment::WarmupThenNominalJitter
        | Impairment::Clean
        | Impairment::DelaySteps
        | Impairment::DuplicatesAndReorder => false,
    }
}

fn is_duplicate(packet_index: u64, packet_duration_ms: u32) -> bool {
    matches!(
        STAGES[stage_index_for_packet(packet_index, packet_duration_ms)].impairment,
        Impairment::DuplicatesAndReorder
    ) && packet_index.is_multiple_of(256)
}

fn is_reorder_pair_start(packet_index: u64, packet_duration_ms: u32) -> bool {
    matches!(
        STAGES[stage_index_for_packet(packet_index, packet_duration_ms)].impairment,
        Impairment::DuplicatesAndReorder
    ) && packet_index.is_multiple_of(2)
}

fn pipeline_for(
    device_rate_hz: u32,
    packet_duration_ms: u32,
) -> (AudioPipelineConfig, PlaybackConfig) {
    let duration = frame_duration(packet_duration_ms);
    let input = AudioPipelineConfigInput {
        capture_rate_hz: device_rate_hz as usize,
        playback_rate_hz: device_rate_hz as usize,
        channels: CHANNELS,
        frame_duration: duration,
        capture_src_chunk_frames: 480,
        capture_ring_samples: RING_SAMPLES,
        playback_ring_samples: RING_SAMPLES,
        tx_accumulator_samples: RING_SAMPLES,
        reorder_capacity: 64,
        network_capacity: 8,
        network_due_batch_capacity: 4,
        packet_capacity: MAX_PACKET_BYTES,
        controller_cadence_frames: 480,
        clock_recovery: ClockRecoveryConfig::default(),
        adaptive_clock: AdaptiveClockConfig::default(),
    };
    let pipeline = AudioPipelineConfig::new(input).expect("supported pipeline shape");
    let playback = PlaybackConfig::for_pipeline(pipeline);
    (pipeline, playback)
}

fn resolve_pending(
    previous: PendingDecision,
    current: Option<PendingDecision>,
    media_frames: u64,
    metrics: &mut NetworkTruth,
) {
    let kind = match (previous, current) {
        (PendingDecision::Packet { timestamp }, _) => {
            let _synthetic_payload_identity = timestamp;
            NetworkTruth::increment(&mut metrics.primary);
            SyntheticDecode::Primary
        }
        (PendingDecision::Missing, Some(PendingDecision::Packet { .. })) => {
            NetworkTruth::increment(&mut metrics.fec_or_plc);
            SyntheticDecode::InbandFecOrPlc
        }
        (PendingDecision::Missing, Some(PendingDecision::Missing) | None) => {
            NetworkTruth::increment(&mut metrics.plc);
            SyntheticDecode::Plc
        }
    };
    NetworkTruth::add_frames(
        &mut metrics.synthetic_media_frames,
        synthetic_decode_sample_count(kind, media_frames),
    );
}

fn schedule_copy(
    slots: &mut [[Option<Packet>; MAX_PACKETS_PER_SLOT]; NETWORK_SLOTS],
    due: u64,
    packet: Packet,
) {
    let slot = &mut slots[due as usize % NETWORK_SLOTS];
    let vacant = slot
        .iter_mut()
        .find(|entry| entry.is_none())
        .expect("bounded network slot overflow");
    *vacant = Some(packet);
}

fn record_delay(metrics: &mut NetworkTruth, delay_ms: u64, late: bool) {
    let bucket = usize::try_from(delay_ms.min(10)).expect("delay bucket");
    NetworkTruth::increment(&mut metrics.delay_buckets_ms[bucket]);
    if delay_ms == 0 {
        NetworkTruth::increment(&mut metrics.on_time);
    } else {
        NetworkTruth::increment(&mut metrics.delayed);
    }
    if late {
        NetworkTruth::increment(&mut metrics.late);
    }
}

#[derive(Clone, Copy, Debug)]
struct CaseResult {
    truth: NetworkTruth,
    plant: PlantTotals,
    stages: [StageSnapshot; 7],
    final_fill_error_frames: f64,
    target_fill_frames: f64,
    ring_capacity_frames: f64,
    negotiated_device_frames: f64,
}

fn run_case(device_rate_hz: u32, packet_duration_ms: u32) -> CaseResult {
    let media_frames = MEDIA_RATE_HZ
        .checked_mul(u64::from(packet_duration_ms))
        .expect("media frames overflow")
        / 1_000;
    let packets = packet_count(packet_duration_ms);
    assert_eq!(
        packets,
        STAGES
            .iter()
            .map(|stage| packets_for_minutes(stage.minutes, packet_duration_ms))
            .sum::<u64>()
    );
    assert!(packets > u64::from(u16::MAX));

    let (pipeline, playback) = pipeline_for(device_rate_hz, packet_duration_ms);
    let recovery_config = pipeline.clock_recovery_config();
    let target_fill_frames = playback.target_fill_frames as f64;
    let ring_capacity_frames = (pipeline.playback_ring_samples() / pipeline.channels()) as f64;
    let safe_margin_frames = ring_capacity_frames - target_fill_frames;
    let negotiated_device_frames =
        f64::from(device_rate_hz) * f64::from(packet_duration_ms) / 1_000.0;

    let timestamp_start = u32::MAX
        .checked_sub(
            u32::try_from(
                3_u64
                    .checked_mul(media_frames)
                    .expect("timestamp origin offset"),
            )
            .expect("timestamp origin fits u32"),
        )
        .expect("timestamp origin underflow");

    let mut reorder =
        ReorderBuffer::<u32>::new(pipeline.reorder_capacity()).expect("valid reorder");
    reorder.reset_and_rebase(START_SEQUENCE);
    let mut network_slots = [[None::<Packet>; MAX_PACKETS_PER_SLOT]; NETWORK_SLOTS];
    let mut held_reorder: Option<(u64, Packet)> = None;
    let mut truth = NetworkTruth::default();
    let mut pending = None;

    let estimator_config = DriftEstimatorConfig {
        nominal_sample_rate_hz: MEDIA_RATE_HZ as f64,
        local_device_sample_rate_hz: f64::from(device_rate_hz),
        ..playback.drift_estimator
    };
    let mut estimator = DriftEstimator::new(estimator_config).expect("valid estimator config");
    let mut recovery = ClockRecovery::new(recovery_config).expect("valid recovery config");
    let mut correction =
        OutputInputRatioCorrectionPpm::new(0.0).expect("zero is a finite correction");

    let mut plant = PlantTotals {
        min_fill_frames: target_fill_frames,
        max_fill_frames: target_fill_frames,
        ..PlantTotals::default()
    };
    let mut stages = [StageSnapshot::default(); 7];
    let mut ring_fill_frames = target_fill_frames;
    let mut fractional_output_frames = 0.0_f64;
    let mut previous_scheduled_local = 0_u64;
    let mut latest_drift_ppm = 0.0;
    let mut previous_wire_sequence = START_SEQUENCE;
    let mut previous_wire_timestamp = timestamp_start;
    let packet_starts = stage_packet_starts(packet_duration_ms);
    let local_offsets =
        stage_local_offsets(media_frames, u64::from(device_rate_hz), packet_duration_ms);

    let observe_boundary = |boundary: u64,
                            estimator: &mut DriftEstimator,
                            recovery: &mut ClockRecovery,
                            correction: &mut OutputInputRatioCorrectionPpm,
                            ring_fill_frames: &mut f64,
                            fractional_output_frames: &mut f64,
                            previous_scheduled_local: &mut u64,
                            latest_drift_ppm: &mut f64,
                            plant: &mut PlantTotals,
                            stages: &mut [StageSnapshot; 7]| {
        let remote_media_position = boundary
            .checked_mul(media_frames)
            .expect("extended remote media position overflow");
        let scheduled_local = scheduled_local_from_tables(
            boundary,
            media_frames,
            u64::from(device_rate_hz),
            packet_starts,
            local_offsets,
        );
        let observation =
            PlayoutClockObservation::from_scheduled_playout(remote_media_position, scheduled_local);
        match estimator
            .observe_scheduled_playout(observation)
            .expect("monotonic scheduled playout observation")
        {
            DriftEstimatorUpdate::WarmingUp => {
                NetworkTruth::increment(&mut plant.warmup_observations);
            }
            DriftEstimatorUpdate::EstimatePpm(ppm) => {
                *latest_drift_ppm = ppm;
                NetworkTruth::increment(&mut plant.estimate_updates);
                if plant.first_estimate_deadline.is_none() {
                    plant.first_estimate_deadline = Some(boundary);
                }
            }
            DriftEstimatorUpdate::Discontinuity(reason) => {
                panic!("continuous virtual clock became discontinuous: {reason:?}")
            }
        }

        if boundary > 0 {
            let consumed = scheduled_local
                .checked_sub(*previous_scheduled_local)
                .expect("scheduled local timeline regression");
            plant.consumed_frames = plant
                .consumed_frames
                .checked_add(consumed)
                .expect("consumed overflow");

            let nominal = media_frames as f64 * f64::from(device_rate_hz) / MEDIA_RATE_HZ as f64
                * correction.ratio_multiplier();
            *fractional_output_frames += nominal;
            assert!(
                fractional_output_frames.is_finite(),
                "fractional output became non-finite"
            );
            let produced = fractional_output_frames.floor();
            *fractional_output_frames -= produced;
            assert!(
                (0.0..1.0).contains(&*fractional_output_frames),
                "fractional remainder left [0, 1): {fractional_output_frames}"
            );
            let produced_frames = produced as u64;
            plant.produced_frames = plant
                .produced_frames
                .checked_add(produced_frames)
                .expect("produced overflow");
            *ring_fill_frames =
                target_fill_frames + plant.produced_frames as f64 - plant.consumed_frames as f64;
            assert!(
                (0.0..=ring_capacity_frames).contains(&*ring_fill_frames),
                "ring fill left the configured capacity: {ring_fill_frames}"
            );
            assert!(
                *ring_fill_frames <= target_fill_frames + safe_margin_frames,
                "ring fill left the configured safe margin: {ring_fill_frames}"
            );
            plant.min_fill_frames = plant.min_fill_frames.min(*ring_fill_frames);
            plant.max_fill_frames = plant.max_fill_frames.max(*ring_fill_frames);

            let output = recovery
                .update(
                    *latest_drift_ppm,
                    *ring_fill_frames - target_fill_frames,
                    f64::from(packet_duration_ms) / 1_000.0,
                )
                .expect("packet cadence is a valid recovery interval");
            assert!(output.correction_ppm.abs() <= recovery_config.max_abs_correction_ppm);
            if output.saturated {
                NetworkTruth::increment(&mut plant.saturated_updates);
            }
            if output.drift_input_clamped {
                NetworkTruth::increment(&mut plant.drift_input_clamped);
            }
            if output.ring_fill_input_clamped {
                NetworkTruth::increment(&mut plant.ring_fill_input_clamped);
            }
            if output.integral_limited {
                NetworkTruth::increment(&mut plant.integral_limited);
            }
            if output.anti_windup_active {
                NetworkTruth::increment(&mut plant.anti_windup_active);
            }
            if output.amplitude_limited {
                NetworkTruth::increment(&mut plant.amplitude_limited);
            }
            if output.slew_limited {
                NetworkTruth::increment(&mut plant.slew_limited);
            }
            *correction =
                OutputInputRatioCorrectionPpm::from_ratio_multiplier(output.ratio_multiplier)
                    .expect("recovery produced a finite positive ratio");

            let completed_packet = boundary - 1;
            let stage_idx = stage_index_for_packet(completed_packet, packet_duration_ms);
            let snapshot = &mut stages[stage_idx];
            snapshot.visited = true;
            snapshot.min_fill_frames = snapshot.min_fill_frames.min(*ring_fill_frames);
            snapshot.max_fill_frames = snapshot.max_fill_frames.max(*ring_fill_frames);
            snapshot.correction_peak_ppm = snapshot
                .correction_peak_ppm
                .max(output.correction_ppm.abs());
            snapshot.correction_sum_squares += output.correction_ppm * output.correction_ppm;
            snapshot.correction_samples = snapshot
                .correction_samples
                .checked_add(1)
                .expect("stage sample overflow");

            if boundary == packets
                || stage_index_for_packet(boundary, packet_duration_ms) != stage_idx
            {
                snapshot.drift_ppm = *latest_drift_ppm;
                snapshot.correction_ppm = correction.get();
                snapshot.end_fill_error_frames = *ring_fill_frames - target_fill_frames;
            }
        }
        *previous_scheduled_local = scheduled_local;
        if boundary == packets {
            plant.terminal_remote_media_frames = remote_media_position;
            plant.terminal_local_device_frames = scheduled_local;
        }
    };

    observe_boundary(
        0,
        &mut estimator,
        &mut recovery,
        &mut correction,
        &mut ring_fill_frames,
        &mut fractional_output_frames,
        &mut previous_scheduled_local,
        &mut latest_drift_ppm,
        &mut plant,
        &mut stages,
    );

    let generate_and_maybe_hold =
        |index: u64,
         held: &mut Option<(u64, Packet)>,
         slots: &mut [[Option<Packet>; MAX_PACKETS_PER_SLOT]; NETWORK_SLOTS],
         metrics: &mut NetworkTruth,
         sequence_wraps: &mut u64,
         timestamp_wraps: &mut u64,
         previous_sequence: &mut u16,
         previous_timestamp: &mut u32| {
            NetworkTruth::increment(&mut metrics.generated);
            let sequence = START_SEQUENCE.wrapping_add(index as u16);
            let timestamp = wire_timestamp(timestamp_start, index, media_frames);
            if index > 0 {
                if sequence < *previous_sequence {
                    NetworkTruth::increment(sequence_wraps);
                }
                if timestamp < *previous_timestamp {
                    NetworkTruth::increment(timestamp_wraps);
                }
            }
            *previous_sequence = sequence;
            *previous_timestamp = timestamp;
            let packet = Packet {
                sequence,
                timestamp,
            };

            if is_lost(index, packet_duration_ms) {
                NetworkTruth::increment(&mut metrics.network_lost);
                if matches!(
                    STAGES[stage_index_for_packet(index, packet_duration_ms)].impairment,
                    Impairment::LossBursts
                ) {
                    NetworkTruth::increment(&mut metrics.burst_lost);
                }
                return;
            }

            let delay_ms = delay_ms_for(index, packet_duration_ms);
            let arrival_tick = index
                + delay_ms.checked_mul(1_000).expect("delay us overflow")
                    / (u64::from(packet_duration_ms) * 1_000);
            let deadline_tick = index
                .checked_add(PLAYOUT_DELAY_PACKETS)
                .expect("deadline overflow");
            record_delay(metrics, delay_ms, arrival_tick > deadline_tick);

            let emit = |due: u64,
                    packet: Packet,
                    source_index: u64,
                    slots: &mut [[Option<Packet>; MAX_PACKETS_PER_SLOT]; NETWORK_SLOTS]| {
            schedule_copy(slots, due, packet);
            if is_duplicate(source_index, packet_duration_ms) {
                schedule_copy(slots, due, packet);
            }
        };

            if is_reorder_pair_start(index, packet_duration_ms) {
                assert!(held.is_none(), "reorder hold already occupied");
                *held = Some((arrival_tick, packet));
            } else if let Some((held_due, held_packet)) = held.take() {
                NetworkTruth::increment(&mut metrics.arrival_inversions);
                // Both copies must land on a not-yet-drained slot. The even packet is
                // generated first and held; flush the pair on the later odd tick.
                let due = arrival_tick.max(held_due).max(index);
                emit(due, packet, index, slots);
                emit(due, held_packet, index - 1, slots);
            } else {
                emit(arrival_tick, packet, index, slots);
            }
        };

    for tick in 0..packets + PLAYOUT_DELAY_PACKETS {
        if tick < packets {
            generate_and_maybe_hold(
                tick,
                &mut held_reorder,
                &mut network_slots,
                &mut truth,
                &mut plant.sequence_wraps,
                &mut plant.timestamp_wraps,
                &mut previous_wire_sequence,
                &mut previous_wire_timestamp,
            );
        }

        let slot = &mut network_slots[tick as usize % NETWORK_SLOTS];
        for packet in slot.iter_mut().filter_map(Option::take) {
            NetworkTruth::increment(&mut truth.delivered);
            match reorder.push(packet.sequence, packet.timestamp) {
                PushResult::Accepted(AcceptedPacket::InOrder) => {
                    NetworkTruth::increment(&mut truth.accepted);
                }
                PushResult::Accepted(AcceptedPacket::Reordered { .. }) => {
                    NetworkTruth::increment(&mut truth.accepted);
                    NetworkTruth::increment(&mut truth.reordered_accepts);
                }
                PushResult::Rejected {
                    reason: RejectedPacket::Duplicate,
                    ..
                } => NetworkTruth::increment(&mut truth.duplicates),
                PushResult::Rejected { reason, .. } => {
                    panic!("unexpected bounded-network rejection: {reason:?}")
                }
            }
        }

        if tick >= PLAYOUT_DELAY_PACKETS && tick - PLAYOUT_DELAY_PACKETS < packets {
            let deadline_index = tick - PLAYOUT_DELAY_PACKETS;
            let expected_sequence = START_SEQUENCE.wrapping_add(deadline_index as u16);
            let decision = match reorder.pop_at_deadline() {
                Playout::Packet {
                    sequence,
                    packet: timestamp,
                } => {
                    assert_eq!(sequence, expected_sequence);
                    assert_eq!(
                        timestamp,
                        wire_timestamp(timestamp_start, deadline_index, media_frames)
                    );
                    PendingDecision::Packet { timestamp }
                }
                Playout::MissingAtDeadline { sequence, .. } => {
                    assert_eq!(sequence, expected_sequence);
                    PendingDecision::Missing
                }
                Playout::Empty => panic!("rebased reorder buffer became empty"),
            };
            if let Some(previous) = pending.replace(decision) {
                resolve_pending(previous, Some(decision), media_frames, &mut truth);
            }
            observe_boundary(
                deadline_index
                    .checked_add(1)
                    .expect("terminal plant boundary"),
                &mut estimator,
                &mut recovery,
                &mut correction,
                &mut ring_fill_frames,
                &mut fractional_output_frames,
                &mut previous_scheduled_local,
                &mut latest_drift_ppm,
                &mut plant,
                &mut stages,
            );
        }
    }
    assert!(held_reorder.is_none(), "unterminated reorder pair");
    resolve_pending(
        pending.expect("at least one playout decision"),
        None,
        media_frames,
        &mut truth,
    );

    assert_eq!(truth.generated, packets);
    assert_eq!(
        truth
            .accepted
            .checked_add(truth.network_lost)
            .expect("accepted+lost"),
        packets
    );
    assert_eq!(
        truth.delivered,
        truth
            .accepted
            .checked_add(truth.duplicates)
            .expect("delivered identity")
    );
    assert_eq!(
        truth
            .primary
            .checked_add(truth.fec_or_plc)
            .and_then(|value| value.checked_add(truth.plc))
            .expect("classification identity"),
        packets
    );
    assert_eq!(
        truth.synthetic_media_frames,
        packets.checked_mul(media_frames).expect("media total")
    );
    assert_eq!(
        plant.terminal_remote_media_frames,
        packets.checked_mul(media_frames).expect("terminal remote")
    );
    assert_eq!(plant.sequence_wraps, exact_sequence_wraps(packets));
    assert_eq!(
        plant.timestamp_wraps,
        exact_timestamp_wraps(packets, media_frames, timestamp_start)
    );

    let reconstructed_fill =
        target_fill_frames + plant.produced_frames as f64 - plant.consumed_frames as f64;
    assert!(
        (ring_fill_frames - reconstructed_fill).abs() < 1.0e-9,
        "initial + produced - consumed != final: fill={ring_fill_frames} reconstructed={reconstructed_fill}"
    );
    assert!(
        (0.0..1.0).contains(&fractional_output_frames),
        "terminal SRC remainder left [0, 1): {fractional_output_frames}"
    );

    CaseResult {
        truth,
        plant,
        stages,
        final_fill_error_frames: ring_fill_frames - target_fill_frames,
        target_fill_frames,
        ring_capacity_frames,
        negotiated_device_frames,
    }
}

#[test]
fn deterministic_virtual_twelve_hour_gate() {
    assert_eq!(
        STAGES.iter().map(|stage| stage.minutes).sum::<u64>(),
        VIRTUAL_HOURS * 60
    );
    assert_eq!(SUPPORTED_SAMPLE_RATES, [44_100, 48_000, 96_000, 192_000]);

    for device_rate_hz in SUPPORTED_SAMPLE_RATES {
        let device_rate_hz = u32::try_from(device_rate_hz).expect("supported rate fits u32");
        for packet_duration_ms in PACKET_DURATIONS_MS {
            let result = run_case(device_rate_hz, packet_duration_ms);
            assert_case(device_rate_hz, packet_duration_ms, result);
        }
    }
}

fn assert_case(device_rate_hz: u32, packet_duration_ms: u32, result: CaseResult) {
    let packets = packet_count(packet_duration_ms);
    let media_frames = MEDIA_RATE_HZ * u64::from(packet_duration_ms) / 1_000;
    let label = format!("{device_rate_hz} Hz/{packet_duration_ms} ms");

    for (index, snapshot) in result.stages.iter().enumerate() {
        assert!(snapshot.visited, "{label} stage {index} was never visited");
        let expected_drift = f64::from(STAGES[index].drift_ppm);
        assert!(
            (snapshot.drift_ppm - expected_drift).abs() < 3.0,
            "{label} stage {index}: drift {}",
            snapshot.drift_ppm
        );
        let expected_correction =
            ((1.0 + expected_drift / 1_000_000.0).recip() - 1.0) * 1_000_000.0;
        if expected_drift > 0.0 {
            assert!(
                snapshot.correction_ppm < 0.0,
                "{label} stage {index}: correction {}",
                snapshot.correction_ppm
            );
        } else if expected_drift < 0.0 {
            assert!(
                snapshot.correction_ppm > 0.0,
                "{label} stage {index}: correction {}",
                snapshot.correction_ppm
            );
        } else {
            assert!(
                snapshot.correction_ppm.abs() < 1.0,
                "{label} stage {index}: correction {}",
                snapshot.correction_ppm
            );
        }
        assert!(
            (snapshot.correction_ppm - expected_correction).abs() < 2.0,
            "{label} stage {index}: correction {}",
            snapshot.correction_ppm
        );
        assert!(
            snapshot.end_fill_error_frames.abs() < result.negotiated_device_frames,
            "{label} stage {index}: end fill error {}",
            snapshot.end_fill_error_frames
        );
        assert!(
            snapshot.max_fill_frames <= result.ring_capacity_frames,
            "{label} stage {index}: max fill {}",
            snapshot.max_fill_frames
        );
        assert!(
            snapshot.min_fill_frames >= 0.0,
            "{label} stage {index}: min fill {}",
            snapshot.min_fill_frames
        );
    }

    assert!(
        result.final_fill_error_frames.abs() < result.negotiated_device_frames,
        "{label}: final fill error {} around target {}",
        result.final_fill_error_frames,
        result.target_fill_frames
    );
    assert!(
        result.plant.max_fill_frames <= result.ring_capacity_frames,
        "{label}: max fill {}",
        result.plant.max_fill_frames
    );
    assert_eq!(
        result.plant.terminal_remote_media_frames,
        packets * media_frames
    );
    assert_eq!(
        result.plant.terminal_local_device_frames,
        scheduled_local_position(
            packets,
            media_frames,
            u64::from(device_rate_hz),
            packet_duration_ms
        )
    );
    assert_eq!(
        result.plant.consumed_frames,
        result.plant.terminal_local_device_frames
    );
    assert!(
        result.plant.warmup_observations > 0,
        "{label}: missing warmup"
    );
    let first_estimate = result.plant.first_estimate_deadline.unwrap_or_else(|| {
        panic!("{label}: estimator never left warmup");
    });
    let packets_in_three_seconds = 3_000 / u64::from(packet_duration_ms) + 2;
    assert!(
        first_estimate <= packets_in_three_seconds,
        "{label}: first estimate at packet {first_estimate}"
    );
    assert_eq!(result.plant.drift_input_clamped, 0);
    assert_eq!(result.plant.ring_fill_input_clamped, 0);

    let nominal = result.stages[0];
    let rms = (nominal.correction_sum_squares / nominal.correction_samples as f64).sqrt();
    assert!(
        nominal.correction_peak_ppm < NOMINAL_PEAK_CORRECTION_PPM + 1.0,
        "{label}: nominal peak correction {}",
        nominal.correction_peak_ppm
    );
    assert!(
        rms < NOMINAL_RMS_CORRECTION_PPM + 1.0,
        "{label}: nominal RMS correction {rms}"
    );

    assert!(
        result.truth.delay_buckets_ms[0] > 0,
        "{label}: missing on-time"
    );
    for delay_ms in 1..=10 {
        assert!(
            result.truth.delay_buckets_ms[delay_ms] > 0,
            "{label}: missing {delay_ms} ms delay bucket"
        );
    }
    assert!(result.truth.delayed > 0, "{label}: delay unasserted");
    assert!(
        result.truth.arrival_inversions > 0,
        "{label}: reorder unasserted"
    );
    assert!(
        result.truth.reordered_accepts > 0,
        "{label}: reorder accept unasserted"
    );
    assert!(
        result.truth.duplicates > 0,
        "{label}: duplicates unasserted"
    );
    assert!(result.truth.network_lost > 0, "{label}: loss unasserted");
    assert!(result.truth.burst_lost > 0, "{label}: bursts unasserted");
    assert!(result.truth.fec_or_plc > 0, "{label}: FEC/PLC unasserted");
    assert!(result.truth.plc > 0, "{label}: PLC unasserted");
    assert_eq!(
        result.truth.late, 0,
        "{label}: 1-10 ms delay should stay inside target"
    );

    let expected_loss_1 = packets_for_minutes(100, packet_duration_ms) / 100;
    let expected_loss_5 = packets_for_minutes(100, packet_duration_ms) / 20;
    let expected_bursts = packets_for_minutes(100, packet_duration_ms) / 200 * 3;
    assert_eq!(
        result.truth.network_lost,
        expected_loss_1 + expected_loss_5 + expected_bursts,
        "{label}: loss total"
    );
    assert_eq!(
        result.truth.burst_lost, expected_bursts,
        "{label}: burst total"
    );

    let duplicate_stage_start: u64 = STAGES[..6]
        .iter()
        .map(|stage| packets_for_minutes(stage.minutes, packet_duration_ms))
        .sum();
    let duplicate_stage_end = packets;
    let expected_duplicates = (duplicate_stage_start..duplicate_stage_end)
        .filter(|index| index.is_multiple_of(256))
        .count() as u64;
    assert_eq!(
        result.truth.duplicates, expected_duplicates,
        "{label}: duplicates"
    );

    let expected_inversions = packets_for_minutes(100, packet_duration_ms) / 2;
    assert_eq!(
        result.truth.arrival_inversions, expected_inversions,
        "{label}: inversions"
    );

    let step_stage_start: u64 = STAGES[..4]
        .iter()
        .map(|stage| packets_for_minutes(stage.minutes, packet_duration_ms))
        .sum();
    let step_stage_end = step_stage_start + packets_for_minutes(100, packet_duration_ms);
    for delay_ms in 1..=10_u64 {
        let expected_steps = (step_stage_start..step_stage_end)
            .filter(|index| 1 + index % 10 == delay_ms)
            .count() as u64;
        assert!(
            result.truth.delay_buckets_ms[usize::try_from(delay_ms).expect("bucket")]
                >= expected_steps,
            "{label}: delay bucket {delay_ms} missing step-stage mass"
        );
    }
}

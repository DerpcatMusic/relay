// SPDX-License-Identifier: MPL-2.0
mod diagnostics;

use std::{env, f32::consts::TAU, process::ExitCode, time::Duration};

use diagnostics::Diagnostics;
use relay_audio::{
    AdaptiveClockConfig, AudioPipelineConfig, AudioPipelineConfigInput, Bitrate, CaptureInput,
    ClockRecoveryConfig, EncoderPolicyV1, ExtendedSequence, ExtendedTimestamp, FrameDuration,
    InbandFec, IngressMismatch, IngressStatus, MAX_PACKET_BYTES, NetworkAction, NetworkTime,
    PacketBatch, PacketLossPercent, PayloadType, PlaybackConfig, PlaybackPublication, RenderState,
    RtpTimestamp, RxStreamConfig, RxWorker, ScheduleStatus, SequenceNumber, Ssrc, TxProcessOutcome,
    TxStreamConfig, TxWorker, playback_pair,
};

const CHANNELS: usize = 2;
const MEDIA_RATE_HZ: u64 = 48_000;
const INITIAL_SEQUENCE: u64 = 10_000;
const INITIAL_TIMESTAMP: u32 = 77_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Profile {
    Clean,
    Impaired,
}

#[derive(Clone, Copy, Debug)]
struct Config {
    capture_rate_hz: usize,
    playback_rate_hz: usize,
    frame_duration: FrameDuration,
    duration_ms: usize,
    profile: Profile,
    seed: u64,
    json: bool,
    device: bool,
}

fn take_value(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_rate(value: String, name: &str) -> Result<usize, String> {
    let rate = value
        .parse::<usize>()
        .map_err(|_| format!("invalid {name}"))?;
    if ![44_100, 48_000, 96_000, 192_000].contains(&rate) {
        return Err(format!("{name} must be one of 44100, 48000, 96000, 192000"));
    }
    Ok(rate)
}

fn parse_args() -> Result<Config, String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut config = Config {
        capture_rate_hz: 48_000,
        playback_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        duration_ms: 500,
        profile: Profile::Clean,
        seed: 1,
        json: false,
        device: false,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--capture-rate" => {
                config.capture_rate_hz = parse_rate(
                    take_value(&args, &mut index, "--capture-rate")?,
                    "capture rate",
                )?;
            }
            "--playback-rate" => {
                config.playback_rate_hz = parse_rate(
                    take_value(&args, &mut index, "--playback-rate")?,
                    "playback rate",
                )?;
            }
            "--packet-ms" => {
                config.frame_duration = match take_value(&args, &mut index, "--packet-ms")?
                    .parse::<u8>()
                    .map_err(|_| "invalid packet duration".to_owned())?
                {
                    5 => FrameDuration::Ms5,
                    10 => FrameDuration::Ms10,
                    20 => FrameDuration::Ms20,
                    _ => return Err("packet-ms must be one of 5, 10, 20".to_owned()),
                };
            }
            "--duration-ms" => {
                config.duration_ms = take_value(&args, &mut index, "--duration-ms")?
                    .parse()
                    .map_err(|_| "invalid duration".to_owned())?;
            }
            "--profile" => {
                config.profile = match take_value(&args, &mut index, "--profile")?.as_str() {
                    "clean" => Profile::Clean,
                    "impaired" => Profile::Impaired,
                    _ => return Err("profile must be clean or impaired".to_owned()),
                };
            }
            "--seed" => {
                config.seed = take_value(&args, &mut index, "--seed")?
                    .parse()
                    .map_err(|_| "invalid seed".to_owned())?;
            }
            "--json" => config.json = true,
            "--device" => config.device = true,
            "--help" | "-h" => {
                return Err("usage: relay-audio-lab [--capture-rate HZ] [--playback-rate HZ] [--packet-ms 5|10|20] [--duration-ms 50..10000] [--profile clean|impaired] [--seed N] [--json] [--device]".to_owned());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        index += 1;
    }
    if !(50..=10_000).contains(&config.duration_ms) || !config.duration_ms.is_multiple_of(10) {
        return Err("duration-ms must be a multiple of 10 in 50..=10000".to_owned());
    }
    Ok(config)
}

fn pipeline(config: Config, packet_budget: usize) -> Result<AudioPipelineConfig, String> {
    AudioPipelineConfig::new(AudioPipelineConfigInput {
        capture_rate_hz: config.capture_rate_hz,
        playback_rate_hz: config.playback_rate_hz,
        channels: CHANNELS,
        frame_duration: config.frame_duration,
        capture_src_chunk_frames: config.capture_rate_hz / 100,
        capture_ring_samples: config.capture_rate_hz * CHANNELS,
        playback_ring_samples: config.playback_rate_hz * CHANNELS,
        tx_accumulator_samples: 48_000 * CHANNELS,
        reorder_capacity: 64,
        network_capacity: packet_budget * 2 + 16,
        network_due_batch_capacity: packet_budget * 2 + 16,
        packet_capacity: MAX_PACKET_BYTES,
        controller_cadence_frames: config.playback_rate_hz / 100,
        clock_recovery: ClockRecoveryConfig::default(),
        adaptive_clock: AdaptiveClockConfig::default(),
    })
    .map_err(|error| format!("invalid audio pipeline: {error:?}"))
}

fn synthetic_pcm(start_frame: usize, frames: usize, rate_hz: usize) -> Vec<f32> {
    let mut pcm = vec![0.0; frames * CHANNELS];
    for frame in 0..frames {
        let position = (start_frame + frame) as f32 / rate_hz as f32;
        pcm[frame * CHANNELS] = (TAU * 311.0 * position).sin() * 0.22;
        pcm[frame * CHANNELS + 1] = (TAU * 617.0 * position).sin() * 0.17;
    }
    pcm
}

fn mix64(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn network_action(index: usize, packet_micros: u64, config: Config) -> NetworkAction {
    let base = index as u64 * packet_micros;
    if config.profile == Profile::Clean {
        return NetworkAction::Delay {
            delay: Duration::from_micros(base),
        };
    }
    if index == 0 {
        return NetworkAction::Duplicate {
            duplicate_delay: Duration::from_micros(1),
        };
    }
    let random = mix64(config.seed ^ index as u64);
    if random.is_multiple_of(31) {
        NetworkAction::Drop
    } else {
        NetworkAction::Delay {
            delay: Duration::from_micros(base + ((random >> 8) % 3) * packet_micros),
        }
    }
}

fn run(config: Config) -> Result<Diagnostics, String> {
    let packet_budget = config.duration_ms / 5 + 8;
    let pipeline = pipeline(config, packet_budget)?;
    let policy = EncoderPolicyV1::new(
        Bitrate::try_new(96_000).map_err(|error| format!("bitrate: {error:?}"))?,
        if config.profile == Profile::Impaired {
            InbandFec::Enabled
        } else {
            InbandFec::Disabled
        },
        PacketLossPercent::try_new(if config.profile == Profile::Impaired {
            5
        } else {
            0
        })
        .map_err(|error| format!("loss hint: {error:?}"))?,
    );
    let stream = TxStreamConfig {
        ssrc: Ssrc::new(0x51a7_1ab0),
        payload_type: PayloadType::new(111).map_err(|error| format!("payload: {error:?}"))?,
        initial_sequence: SequenceNumber::new(INITIAL_SEQUENCE as u16),
        initial_timestamp: RtpTimestamp::new(INITIAL_TIMESTAMP),
        encoding_policy: policy,
    };
    let mut tx = TxWorker::new(pipeline, stream).map_err(|error| format!("TX: {error:?}"))?;
    let mut batch = PacketBatch::new(packet_budget).map_err(|error| format!("batch: {error:?}"))?;
    let chunk_frames = tx.capture_chunk_samples() / CHANNELS;
    let chunks = config.duration_ms / 10;
    let mut packets = Vec::with_capacity(packet_budget);
    for chunk in 0..chunks {
        let pcm = synthetic_pcm(chunk * chunk_frames, chunk_frames, config.capture_rate_hz);
        match tx.process_capture(CaptureInput::Chunk(&pcm), &mut batch) {
            TxProcessOutcome::Complete(_) | TxProcessOutcome::BatchFull(_) => {}
            other => return Err(format!("unexpected TX outcome: {other:?}")),
        }
        while let Some(packet) = batch.take_next() {
            packets.push(packet);
        }
    }
    if packets.is_empty() {
        return Err("bounded run did not produce a packet".to_owned());
    }
    let encoded_packets = packets.len();
    let packet_micros =
        config.frame_duration.samples_per_channel() as u64 * 1_000_000 / MEDIA_RATE_HZ;
    let mut network = pipeline
        .create_deterministic_network()
        .map_err(|error| format!("network: {error:?}"))?;
    let mut due = pipeline
        .create_due_batch()
        .map_err(|error| format!("due batch: {error:?}"))?;
    for (index, packet) in packets.into_iter().enumerate() {
        let outcome = network.schedule(packet, network_action(index, packet_micros, config));
        if !matches!(
            outcome.status(),
            ScheduleStatus::Scheduled { .. } | ScheduleStatus::Dropped
        ) {
            return Err(format!("network schedule: {:?}", outcome.status()));
        }
    }

    let rx_stream = RxStreamConfig {
        ssrc: stream.ssrc,
        payload_type: stream.payload_type,
        initial_sequence: ExtendedSequence::new(INITIAL_SEQUENCE),
        initial_timestamp: stream.initial_timestamp,
    };
    let mut rx = RxWorker::new(pipeline, rx_stream).map_err(|error| format!("RX: {error:?}"))?;
    let (mut playback, mut renderer, ring_metrics) =
        playback_pair(pipeline, PlaybackConfig::for_pipeline(pipeline))
            .map_err(|error| format!("playback: {error:?}"))?;
    let mut rendered_frames = 0_u64;
    let mut rendered_checksum = 0_u64;
    let mut ring_high_water_frames = 0_u64;
    let mut ingress_accepted_packets = 0_u64;
    let mut rx_duplicate_rejections = 0_u64;

    let mut consume = |outcome: relay_audio::FrameOutcome<'_>| -> Result<(), String> {
        let offset = outcome
            .sequence()
            .get()
            .checked_sub(INITIAL_SEQUENCE)
            .ok_or_else(|| "RX timeline preceded epoch".to_owned())?;
        let media_delta = offset * config.frame_duration.samples_per_channel() as u64;
        let expected_timestamp = INITIAL_TIMESTAMP.wrapping_add(media_delta as u32);
        if outcome.timestamp() != RtpTimestamp::new(expected_timestamp) {
            return Err("RX timestamp departed scheduled timeline".to_owned());
        }
        let scheduled_local =
            (media_delta * config.playback_rate_hz as u64 + 24_000) / MEDIA_RATE_HZ;
        let report = playback
            .process_frame(
                outcome.frame(),
                ExtendedTimestamp::new(u64::from(INITIAL_TIMESTAMP) + media_delta),
                scheduled_local,
            )
            .map_err(|error| format!("playback worker: {error:?}"))?;
        if report.publication != PlaybackPublication::Published || report.control_fault.is_some() {
            return Err(format!("playback publication/control: {report:?}"));
        }
        ring_high_water_frames = ring_high_water_frames.max(
            u64::try_from(renderer.available_samples() / CHANNELS)
                .map_err(|_| "ring high-water conversion".to_owned())?,
        );
        let mut output = vec![f32::NAN; report.output_frames * CHANNELS];
        let rendered = renderer.render(&mut output);
        if rendered.state != RenderState::Complete || rendered.rendered_samples != output.len() {
            return Err(format!("renderer: {rendered:?}"));
        }
        for sample in output {
            if !sample.is_finite() {
                return Err("renderer produced nonfinite audio".to_owned());
            }
            rendered_checksum = rendered_checksum
                .rotate_left(5)
                .wrapping_add(u64::from(sample.to_bits()));
        }
        rendered_frames = rendered_frames
            .checked_add(report.output_frames as u64)
            .ok_or_else(|| "rendered frame counter overflow".to_owned())?;
        Ok(())
    };

    for slot in 0..encoded_packets {
        let deadline = (slot as u64 + 4) * packet_micros;
        network
            .advance_to(NetworkTime::from_micros(deadline), &mut due)
            .map_err(|error| format!("virtual delivery: {error:?}"))?;
        while let Some(packet) = due.take_next() {
            match rx.ingress(packet).status() {
                IngressStatus::AcceptedInOrder | IngressStatus::AcceptedReordered { .. } => {
                    ingress_accepted_packets += 1;
                }
                IngressStatus::Rejected(IngressMismatch::Duplicate)
                    if config.profile == Profile::Impaired =>
                {
                    rx_duplicate_rejections += 1;
                }
                status => return Err(format!("RX ingress: {status:?}")),
            }
        }
        if let Some(outcome) = rx.tick() {
            consume(outcome)?;
        }
    }
    let drained = rx
        .drain()
        .ok_or_else(|| "final RX lookahead was not drainable".to_owned())?;
    let drained_lookahead_frames = 1_u64;
    consume(drained)?;
    if renderer.available_samples() != 0 {
        return Err("renderer did not drain the bounded ring".to_owned());
    }

    let rx_metrics = rx.metrics();
    let playback_metrics = playback.metrics();
    let network_metrics = network.metrics();
    let ring = ring_metrics.snapshot();
    let expected_rendered = rx_metrics.emitted_frames
        * config.frame_duration.samples_per_channel() as u64
        * config.playback_rate_hz as u64
        / MEDIA_RATE_HZ;
    Ok(Diagnostics {
        input_frames: (chunks * chunk_frames) as u64,
        rendered_frames,
        encoded_packets: encoded_packets as u64,
        emitted_frames: rx_metrics.emitted_frames,
        drained_lookahead_frames,
        ingress_accepted_packets,
        network_drops: network_metrics.simulated_drops,
        network_duplicate_requests: network_metrics.duplicate_requests,
        network_duplicate_copies_scheduled: network_metrics.duplicate_copies_scheduled,
        rx_duplicate_rejections,
        fec_or_plc_frames: rx_metrics.fec_attempts,
        plc_frames: rx_metrics.plc_frames,
        ring_dropped_frames: ring.dropped_samples / CHANNELS as u64,
        ring_underrun_frames: ring.underrun_samples / CHANNELS as u64,
        ring_high_water_frames,
        playback_error_frames: rendered_frames as i64 - expected_rendered as i64,
        configured_capture_rate_hz: config.capture_rate_hz as u64,
        configured_playback_rate_hz: config.playback_rate_hz as u64,
        rendered_checksum,
        published_chunks: playback_metrics.published_chunks,
    })
}

fn main() -> ExitCode {
    let config = match parse_args() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("audio-lab: {error}");
            return ExitCode::from(2);
        }
    };
    if config.device {
        eprintln!("audio-lab: native device mode is unavailable in this headless build");
        return ExitCode::from(3);
    }
    match run(config) {
        Ok(diagnostics) => {
            println!(
                "{}",
                if config.json {
                    diagnostics.json()
                } else {
                    diagnostics.human()
                }
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("audio-lab: {error}");
            ExitCode::FAILURE
        }
    }
}

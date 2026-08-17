use std::time::Duration;

use relay_audio::{
    AdaptiveClockConfig, AdvanceError, AudioPipelineConfig, AudioPipelineConfigInput,
    ClockRecoveryConfig, ConfigError, DeterministicNetwork, DueBatch, ExtendedSequence,
    ExtendedTimestamp, ExtensionError, FrameDuration, MAX_PACKET_BYTES, MediaPacket, NetworkAction,
    NetworkConfigError, NetworkMetrics, NetworkTime, PacketError, PayloadType, RtpTimestamp,
    ScheduleRejection, ScheduleStatus, SequenceNumber, Ssrc, extend_sequence, extend_timestamp,
};

use relay_resample::WorkerResampler;

fn valid_input() -> AudioPipelineConfigInput {
    AudioPipelineConfigInput {
        capture_rate_hz: 48_000,
        playback_rate_hz: 48_000,
        channels: 2,
        frame_duration: FrameDuration::Ms20,
        capture_src_chunk_frames: 480,
        capture_ring_samples: 100_000,
        playback_ring_samples: 100_000,
        tx_accumulator_samples: 100_000,
        reorder_capacity: 64,
        network_capacity: 8,
        network_due_batch_capacity: 4,
        packet_capacity: MAX_PACKET_BYTES,
        controller_cadence_frames: 480,
        clock_recovery: ClockRecoveryConfig::default(),
        adaptive_clock: AdaptiveClockConfig::default(),
    }
}

fn packet(sequence: u16) -> MediaPacket {
    MediaPacket::try_new(
        7,
        sequence,
        u32::from(sequence) * 480,
        111,
        &[sequence as u8, 9],
    )
    .expect("test packet is valid")
}

#[test]
fn config_accepts_all_supported_rates_and_durations() {
    let rates = [44_100, 48_000, 96_000, 192_000];
    let durations = [FrameDuration::Ms5, FrameDuration::Ms10, FrameDuration::Ms20];
    for rate in rates {
        for duration in durations {
            let mut input = valid_input();
            input.capture_rate_hz = rate;
            input.playback_rate_hz = rate;
            input.frame_duration = duration;
            let config = AudioPipelineConfig::new(input).expect("supported shape");
            assert_eq!(config.opus_packet_samples(), duration.interleaved_samples());
            assert!(
                config.capture_ring_samples() >= config.minimum_capture_ring_samples(),
                "capture minimum at {rate} Hz / {duration:?}"
            );
            assert!(
                config.tx_accumulator_samples() >= config.minimum_tx_accumulator_samples(),
                "TX minimum at {rate} Hz / {duration:?}"
            );
            assert!(
                config.playback_ring_samples() >= config.minimum_playback_ring_samples(),
                "playback minimum at {rate} Hz / {duration:?}"
            );
        }
    }
}

#[test]
fn config_rejects_rates_channels_alignment_and_invalid_clock_policy() {
    let mut input = valid_input();
    input.capture_rate_hz = 32_000;
    assert_eq!(
        AudioPipelineConfig::new(input),
        Err(ConfigError::UnsupportedSampleRate {
            name: "capture_rate_hz",
            rate_hz: 32_000,
        })
    );

    let mut input = valid_input();
    input.channels = 1;
    assert_eq!(
        AudioPipelineConfig::new(input),
        Err(ConfigError::UnsupportedChannelCount(1))
    );

    let mut input = valid_input();
    input.capture_ring_samples = 3;
    assert_eq!(
        AudioPipelineConfig::new(input),
        Err(ConfigError::IncompleteInterleavedFrame {
            name: "capture_ring_samples",
            samples: 3,
            channels: 2,
        })
    );

    let mut input = valid_input();
    input.clock_recovery.max_update_interval_seconds = 0.0;
    assert!(matches!(
        AudioPipelineConfig::new(input),
        Err(ConfigError::InvalidClockRecoveryConfiguration(_))
    ));

    let mut input = valid_input();
    input.adaptive_clock.smoothing_time_seconds = 0.0;
    assert!(matches!(
        AudioPipelineConfig::new(input),
        Err(ConfigError::InvalidAdaptiveResamplerConfiguration(_))
    ));

    let mut input = valid_input();
    input.adaptive_clock.max_correction_ppm = input.clock_recovery.max_abs_correction_ppm;
    AudioPipelineConfig::new(input).expect("an equal adaptive/recovery range is contained");

    let mut input = valid_input();
    input.clock_recovery.max_abs_correction_ppm = 1_001.0;
    assert_eq!(
        AudioPipelineConfig::new(input),
        Err(ConfigError::AdaptiveCorrectionRangeTooSmall {
            adaptive_ppm: 1_000.0,
            recovery_ppm: 1_001.0,
        })
    );
}

#[test]
fn cadence_boundaries_are_exact_for_every_rate_and_duration() {
    let rates_and_maximum_frames = [
        (44_100, 11_025),
        (48_000, 12_000),
        (96_000, 24_000),
        (192_000, 48_000),
    ];
    let durations = [FrameDuration::Ms5, FrameDuration::Ms10, FrameDuration::Ms20];

    for (rate, maximum_frames) in rates_and_maximum_frames {
        for duration in durations {
            for cadence in [maximum_frames - 1, maximum_frames] {
                let mut input = valid_input();
                input.playback_rate_hz = rate;
                input.frame_duration = duration;
                input.controller_cadence_frames = cadence;
                AudioPipelineConfig::new(input).unwrap_or_else(|error| {
                    panic!("{rate} Hz / {duration:?} cadence {cadence} failed: {error}")
                });
            }

            let mut input = valid_input();
            input.playback_rate_hz = rate;
            input.frame_duration = duration;
            input.controller_cadence_frames = maximum_frames + 1;
            assert_eq!(
                AudioPipelineConfig::new(input),
                Err(ConfigError::ControllerCadenceExceedsRecoveryMaximum {
                    cadence_frames: maximum_frames + 1,
                    playback_rate_hz: rate,
                    maximum_seconds: 0.25,
                }),
                "{rate} Hz / {duration:?}"
            );
        }
    }
}

#[test]
fn transaction_capacity_boundaries_cover_every_rate_and_duration() {
    let rates = [44_100, 48_000, 96_000, 192_000];
    let durations = [FrameDuration::Ms5, FrameDuration::Ms10, FrameDuration::Ms20];

    for rate in rates {
        for duration in durations {
            let mut seed = valid_input();
            seed.capture_rate_hz = rate;
            seed.playback_rate_hz = rate;
            seed.frame_duration = duration;
            let shape = AudioPipelineConfig::new(seed).expect("oversized seed is valid");
            let cases = [
                ("capture_ring_samples", shape.minimum_capture_ring_samples()),
                (
                    "tx_accumulator_samples",
                    shape.minimum_tx_accumulator_samples(),
                ),
                (
                    "playback_ring_samples",
                    shape.minimum_playback_ring_samples(),
                ),
            ];

            for (name, minimum) in cases {
                for capacity in [minimum, minimum + 2] {
                    let mut input = seed;
                    match name {
                        "capture_ring_samples" => input.capture_ring_samples = capacity,
                        "tx_accumulator_samples" => input.tx_accumulator_samples = capacity,
                        "playback_ring_samples" => input.playback_ring_samples = capacity,
                        _ => unreachable!("closed capacity list"),
                    }
                    AudioPipelineConfig::new(input).unwrap_or_else(|error| {
                        panic!("{rate} Hz / {duration:?} {name}={capacity} should fit: {error}")
                    });
                }

                let below = minimum - 2;
                let mut input = seed;
                match name {
                    "capture_ring_samples" => input.capture_ring_samples = below,
                    "tx_accumulator_samples" => input.tx_accumulator_samples = below,
                    "playback_ring_samples" => input.playback_ring_samples = below,
                    _ => unreachable!("closed capacity list"),
                }
                assert_eq!(
                    AudioPipelineConfig::new(input),
                    Err(ConfigError::CapacityTooSmall {
                        name,
                        minimum,
                        actual: below,
                    }),
                    "{rate} Hz / {duration:?}"
                );
            }
        }
    }
}

#[test]
fn packet_capacity_boundary_is_enforced_for_every_rate_and_duration() {
    let rates = [44_100, 48_000, 96_000, 192_000];
    let durations = [FrameDuration::Ms5, FrameDuration::Ms10, FrameDuration::Ms20];

    for rate in rates {
        for duration in durations {
            let mut input = valid_input();
            input.capture_rate_hz = rate;
            input.playback_rate_hz = rate;
            input.frame_duration = duration;
            for capacity in [MAX_PACKET_BYTES - 1, MAX_PACKET_BYTES] {
                input.packet_capacity = capacity;
                AudioPipelineConfig::new(input).unwrap_or_else(|error| {
                    panic!("{rate} Hz / {duration:?} packet capacity {capacity} failed: {error}")
                });
            }
            input.packet_capacity = MAX_PACKET_BYTES + 1;
            assert_eq!(
                AudioPipelineConfig::new(input),
                Err(ConfigError::PacketCapacityTooLarge {
                    maximum: MAX_PACKET_BYTES,
                    actual: MAX_PACKET_BYTES + 1,
                }),
                "{rate} Hz / {duration:?}"
            );

            input.packet_capacity = 2;
            let config = AudioPipelineConfig::new(input).expect("pipeline");

            for payload in [&[0xa5][..], &[0xa5, 0x5a][..]] {
                let packet = config
                    .try_create_media_packet(1, 2, 3, 111, payload)
                    .unwrap_or_else(|error| {
                        panic!("{rate} Hz / {duration:?} payload failed: {error}")
                    });
                assert_eq!(packet.payload(), payload);
            }
            assert_eq!(
                config.try_create_media_packet(1, 2, 3, 111, &[1, 2, 3]),
                Err(PacketError::PayloadTooLarge {
                    maximum: 2,
                    actual: 3,
                }),
                "{rate} Hz / {duration:?}"
            );

            let globally_valid =
                MediaPacket::try_new(1, 2, 3, 111, &[1, 2, 3]).expect("fits inline storage");
            assert_eq!(
                config.validate_media_packet(&globally_valid),
                Err(PacketError::PayloadTooLarge {
                    maximum: 2,
                    actual: 3,
                })
            );
        }
    }
}

#[test]
fn config_factories_preserve_exact_fixed_layouts() {
    let config = AudioPipelineConfig::new(valid_input()).expect("pipeline");
    let network = config
        .create_deterministic_network()
        .expect("configured network");
    let batch = config.create_due_batch().expect("configured batch");
    assert_eq!(network.capacity(), config.network_capacity());
    assert_eq!(network.max_due_batch(), config.network_due_batch_capacity());
    assert_eq!(batch.capacity(), config.network_due_batch_capacity());
    let fixed = config.create_fixed_resampler().expect("fixed resampler");
    let adaptive = config
        .create_adaptive_resampler()
        .expect("adaptive resampler");
    let _recovery = config.create_clock_recovery().expect("clock recovery");
    assert_eq!(fixed.requirements(), config.fixed_resampler_requirements());
    assert_eq!(
        adaptive.requirements(),
        config.adaptive_resampler_requirements()
    );

    let mut impossible = valid_input();
    impossible.network_capacity = usize::MAX;
    impossible.network_due_batch_capacity = 1;
    let config = AudioPipelineConfig::new(impossible)
        .expect("config does not approximate private network layout");
    assert!(matches!(
        config.create_deterministic_network(),
        Err(NetworkConfigError::CapacityOverflow)
    ));
}

#[test]
fn config_rejects_every_invalid_capacity_class_and_overflow() {
    for field in [
        "capture_src_chunk_frames",
        "capture_ring_samples",
        "playback_ring_samples",
        "tx_accumulator_samples",
        "reorder_capacity",
        "network_capacity",
        "network_due_batch_capacity",
        "packet_capacity",
        "controller_cadence_frames",
    ] {
        let mut input = valid_input();
        match field {
            "capture_src_chunk_frames" => input.capture_src_chunk_frames = 0,
            "capture_ring_samples" => input.capture_ring_samples = 0,
            "playback_ring_samples" => input.playback_ring_samples = 0,
            "tx_accumulator_samples" => input.tx_accumulator_samples = 0,
            "reorder_capacity" => input.reorder_capacity = 0,
            "network_capacity" => input.network_capacity = 0,
            "network_due_batch_capacity" => input.network_due_batch_capacity = 0,
            "packet_capacity" => input.packet_capacity = 0,
            "controller_cadence_frames" => input.controller_cadence_frames = 0,
            _ => unreachable!("closed test field list"),
        }
        assert!(
            matches!(
                AudioPipelineConfig::new(input),
                Err(ConfigError::ZeroValue(name)) if name == field
            ) || matches!(
                AudioPipelineConfig::new(input),
                Err(ConfigError::InvalidFixedResamplerConfiguration(_))
            ),
            "unexpected zero result for {field}: {:?}",
            AudioPipelineConfig::new(input)
        );
    }

    let mut input = valid_input();
    input.reorder_capacity = 32_768;
    assert_eq!(
        AudioPipelineConfig::new(input),
        Err(ConfigError::ReorderCapacityAtOrAboveHalfRange(32_768))
    );

    let mut input = valid_input();
    input.network_due_batch_capacity = 9;
    assert_eq!(
        AudioPipelineConfig::new(input),
        Err(ConfigError::DueBatchExceedsNetworkCapacity {
            batch: 9,
            network: 8,
        })
    );

    let mut input = valid_input();
    input.packet_capacity = MAX_PACKET_BYTES + 1;
    assert_eq!(
        AudioPipelineConfig::new(input),
        Err(ConfigError::PacketCapacityTooLarge {
            maximum: MAX_PACKET_BYTES,
            actual: MAX_PACKET_BYTES + 1,
        })
    );

    let mut input = valid_input();
    input.capture_ring_samples = usize::MAX - 1;
    assert_eq!(
        AudioPipelineConfig::new(input),
        Err(ConfigError::CapacityOverflow("capture_ring_samples"))
    );
}

#[test]
fn typed_wire_values_wrap_only_when_explicitly_requested() {
    assert_eq!(SequenceNumber::new(u16::MAX).wrapping_next().get(), 0);
    assert_eq!(RtpTimestamp::new(u32::MAX - 3).wrapping_add(4).get(), 0);
    assert_eq!(Ssrc::new(0xfeed_beef).get(), 0xfeed_beef);
}

#[test]
fn sequence_extension_handles_both_wrap_directions_and_half_range() {
    assert_eq!(
        extend_sequence(ExtendedSequence::new(65_534), SequenceNumber::new(1)),
        Ok(ExtendedSequence::new(65_537))
    );
    assert_eq!(
        extend_sequence(ExtendedSequence::new(65_537), SequenceNumber::new(65_534)),
        Ok(ExtendedSequence::new(65_534))
    );
    assert_eq!(
        extend_sequence(ExtendedSequence::new(10), SequenceNumber::new(32_778)),
        Err(ExtensionError::AmbiguousHalfRange)
    );
    assert_eq!(
        extend_sequence(ExtendedSequence::new(1), SequenceNumber::new(u16::MAX)),
        Err(ExtensionError::BeforeEpoch)
    );
    assert_eq!(
        extend_sequence(ExtendedSequence::new(u64::MAX), SequenceNumber::new(0)),
        Err(ExtensionError::ExtendedOverflow)
    );
}

#[test]
fn timestamp_extension_handles_both_wrap_directions_and_half_range() {
    let reference = ExtendedTimestamp::new(u64::from(u32::MAX) - 1);
    assert_eq!(
        extend_timestamp(reference, RtpTimestamp::new(2)),
        Ok(ExtendedTimestamp::new(u64::from(u32::MAX) + 3))
    );
    assert_eq!(
        extend_timestamp(
            ExtendedTimestamp::new(u64::from(u32::MAX) + 3),
            RtpTimestamp::new(u32::MAX - 1),
        ),
        Ok(reference)
    );
    assert_eq!(
        extend_timestamp(
            ExtendedTimestamp::new(5),
            RtpTimestamp::new(5_u32.wrapping_add(1 << 31)),
        ),
        Err(ExtensionError::AmbiguousHalfRange)
    );
    assert_eq!(
        extend_timestamp(ExtendedTimestamp::new(1), RtpTimestamp::new(u32::MAX)),
        Err(ExtensionError::BeforeEpoch)
    );
    assert_eq!(
        extend_timestamp(ExtendedTimestamp::new(u64::MAX), RtpTimestamp::new(0)),
        Err(ExtensionError::ExtendedOverflow)
    );
}

#[test]
fn media_packet_validates_type_and_bounded_inline_payload() {
    assert_eq!(PayloadType::new(127).expect("valid").get(), 127);
    assert!(PayloadType::new(128).is_err());
    assert_eq!(
        MediaPacket::try_new(1, 2, 3, 128, &[1]),
        Err(PacketError::InvalidPayloadType(128))
    );
    assert_eq!(
        MediaPacket::try_new(1, 2, 3, 111, &[]),
        Err(PacketError::EmptyPayload)
    );
    let too_large = vec![0; MAX_PACKET_BYTES + 1];
    assert_eq!(
        MediaPacket::try_new(1, 2, 3, 111, &too_large),
        Err(PacketError::PayloadTooLarge {
            maximum: MAX_PACKET_BYTES,
            actual: MAX_PACKET_BYTES + 1,
        })
    );
    let maximum = vec![0xa5; MAX_PACKET_BYTES];
    let packet = MediaPacket::try_new(1, 2, 3, 111, &maximum).expect("exact maximum fits");
    assert_eq!(packet.payload(), maximum);
    assert_eq!(packet.payload_len(), MAX_PACKET_BYTES);
}

#[test]
fn network_orders_by_delivery_time_then_insertion_ordinal() {
    let mut network = DeterministicNetwork::new(6, 6).expect("network");
    let mut batch = DueBatch::new(6).expect("batch");
    assert!(matches!(
        network
            .schedule(
                packet(1),
                NetworkAction::Delay {
                    delay: Duration::from_micros(10),
                },
            )
            .status(),
        ScheduleStatus::Scheduled { copies: 1 }
    ));
    let _ = network.schedule(packet(2), NetworkAction::Deliver);
    let _ = network.schedule(
        packet(3),
        NetworkAction::Duplicate {
            duplicate_delay: Duration::ZERO,
        },
    );
    let report = network
        .advance_to(NetworkTime::from_micros(10), &mut batch)
        .expect("advance");
    assert_eq!(report.delivered, 4);
    let sequences: Vec<_> = std::iter::from_fn(|| batch.take_next())
        .map(|packet| packet.sequence().get())
        .collect();
    assert_eq!(sequences, [2, 3, 3, 1]);
}

#[test]
fn network_returns_owned_primary_on_full_and_counts_partial_duplicate() {
    let mut network = DeterministicNetwork::new(1, 1).expect("network");
    assert!(matches!(
        network
            .schedule(
                packet(10),
                NetworkAction::Duplicate {
                    duplicate_delay: Duration::ZERO,
                },
            )
            .status(),
        ScheduleStatus::Scheduled { copies: 1 }
    ));
    assert_eq!(network.metrics().duplicate_capacity_rejections, 1);

    let rejected = packet(11);
    let outcome = network.schedule(rejected.clone(), NetworkAction::Deliver);
    assert_eq!(
        outcome.status(),
        ScheduleStatus::Rejected(ScheduleRejection::Full)
    );
    assert_eq!(outcome.into_returned_packet(), Some(rejected));
    assert_eq!(network.metrics().capacity_rejections, 1);
}

#[test]
fn network_batches_due_packets_and_preserves_remainder_order() {
    let mut network = DeterministicNetwork::new(3, 2).expect("network");
    let mut batch = DueBatch::new(2).expect("batch");
    for sequence in 0..3 {
        let _ = network.schedule(packet(sequence), NetworkAction::Deliver);
    }
    let first = network
        .advance_to(NetworkTime::ZERO, &mut batch)
        .expect("first batch");
    assert_eq!(first.delivered, 2);
    assert_eq!(first.due_remaining, 1);
    assert_eq!(batch.take_next().expect("first").sequence().get(), 0);
    assert_eq!(batch.take_next().expect("second").sequence().get(), 1);

    let second = network
        .advance_to(NetworkTime::ZERO, &mut batch)
        .expect("second batch");
    assert_eq!(second.delivered, 1);
    assert_eq!(batch.take_next().expect("third").sequence().get(), 2);
}

#[test]
fn network_drop_delay_overflow_and_time_regression_are_explicit() {
    let mut network = DeterministicNetwork::new(2, 2).expect("network");
    assert!(matches!(
        network.schedule(packet(1), NetworkAction::Drop).status(),
        ScheduleStatus::Dropped
    ));
    assert_eq!(network.metrics().simulated_drops, 1);

    let mut batch = DueBatch::new(2).expect("batch");
    network
        .advance_to(NetworkTime::from_micros(u64::MAX), &mut batch)
        .expect("advance to maximum");
    assert!(matches!(
        network
            .schedule(
                packet(2),
                NetworkAction::Delay {
                    delay: Duration::from_micros(1),
                },
            )
            .status(),
        ScheduleStatus::Rejected(ScheduleRejection::TimeOverflow)
    ));
    assert_eq!(network.metrics().time_overflow_rejections, 1);
    assert!(matches!(
        network.advance_to(NetworkTime::ZERO, &mut batch),
        Err(AdvanceError::TimeMovedBackward { .. })
    ));
}

#[test]
fn network_requires_consumed_batches_and_enforces_configured_batch_bound() {
    let mut network = DeterministicNetwork::new(2, 1).expect("network");
    let _ = network.schedule(packet(1), NetworkAction::Deliver);
    let mut batch = DueBatch::new(1).expect("batch");
    network
        .advance_to(NetworkTime::ZERO, &mut batch)
        .expect("fills batch");
    assert_eq!(
        network.advance_to(NetworkTime::ZERO, &mut batch),
        Err(AdvanceError::BatchNotEmpty)
    );

    let mut oversized = DueBatch::new(2).expect("batch");
    assert_eq!(
        network.advance_to(NetworkTime::ZERO, &mut oversized),
        Err(AdvanceError::BatchExceedsConfiguredMaximum {
            actual: 2,
            maximum: 1,
        })
    );
}

#[test]
fn network_drain_is_ordered_and_reset_reuses_fixed_storage() {
    let mut network = DeterministicNetwork::new(3, 2).expect("network");
    let mut batch = DueBatch::new(2).expect("batch");
    let _ = network.schedule(
        packet(1),
        NetworkAction::Delay {
            delay: Duration::from_micros(20),
        },
    );
    let _ = network.schedule(
        packet(2),
        NetworkAction::Delay {
            delay: Duration::from_micros(10),
        },
    );
    let report = network.drain(&mut batch).expect("drain");
    assert_eq!(report.drained, 2);
    assert_eq!(batch.take_next().expect("earliest").sequence().get(), 2);
    assert_eq!(batch.take_next().expect("later").sequence().get(), 1);

    let _ = network.schedule(packet(3), NetworkAction::Deliver);
    assert_eq!(network.reset(), 1);
    assert_eq!(network.now(), NetworkTime::ZERO);
    assert_eq!(network.queued(), 0);
    assert_eq!(network.metrics(), NetworkMetrics::default());
    assert!(matches!(
        network.schedule(packet(4), NetworkAction::Deliver).status(),
        ScheduleStatus::Scheduled { copies: 1 }
    ));
}

#[test]
fn fixed_network_construction_rejects_invalid_capacities() {
    assert!(matches!(
        DeterministicNetwork::new(0, 1),
        Err(NetworkConfigError::ZeroCapacity)
    ));
    assert!(matches!(
        DeterministicNetwork::new(1, 0),
        Err(NetworkConfigError::ZeroDueBatchCapacity)
    ));
    assert!(matches!(
        DeterministicNetwork::new(1, 2),
        Err(NetworkConfigError::DueBatchExceedsCapacity)
    ));
}

use relay_rt::{ReadOutcome, ReadState, RingConfigError, WriteOutcome, audio_ring};

#[test]
fn rejects_zero_capacity() {
    assert!(matches!(audio_ring(0), Err(RingConfigError::ZeroCapacity)));
}

#[test]
fn uses_every_capacity_slot_and_drops_new_data_when_full() {
    let (mut producer, mut consumer, metrics) = audio_ring(4).expect("valid capacity");

    assert_eq!(producer.available_samples(), 4);
    assert_eq!(
        producer.write(&[1.0, 2.0, 3.0, 4.0]),
        WriteOutcome::Written { samples: 4 }
    );
    assert_eq!(
        producer.write(&[9.0]),
        WriteOutcome::DroppedFull { samples: 1 }
    );

    let mut output = [0.0; 4];
    assert_eq!(
        consumer.read(&mut output),
        ReadOutcome {
            read_samples: 4,
            state: ReadState::Complete,
        }
    );
    assert_eq!(output, [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(metrics.snapshot().dropped_samples, 1);
}

#[test]
fn insufficient_capacity_drops_the_whole_new_slice() {
    let (mut producer, mut consumer, metrics) = audio_ring(4).expect("valid capacity");

    assert_eq!(
        producer.write(&[1.0, 2.0, 3.0]),
        WriteOutcome::Written { samples: 3 }
    );
    assert_eq!(
        producer.write(&[4.0, 5.0]),
        WriteOutcome::DroppedFull { samples: 2 }
    );

    let mut output = [-1.0; 4];
    assert_eq!(
        consumer.read(&mut output),
        ReadOutcome {
            read_samples: 3,
            state: ReadState::Underrun,
        }
    );
    assert_eq!(output, [1.0, 2.0, 3.0, -1.0]);
    assert_eq!(metrics.snapshot().dropped_samples, 2);
}

#[test]
fn preserves_fifo_order_across_physical_wrap() {
    let (mut producer, mut consumer, _) = audio_ring(5).expect("valid capacity");

    assert_eq!(
        producer.write(&[0.0, 1.0, 2.0, 3.0]),
        WriteOutcome::Written { samples: 4 }
    );
    let mut prefix = [-1.0; 3];
    assert_eq!(consumer.read(&mut prefix).read_samples, 3);
    assert_eq!(prefix, [0.0, 1.0, 2.0]);

    assert_eq!(
        producer.write(&[4.0, 5.0, 6.0, 7.0]),
        WriteOutcome::Written { samples: 4 }
    );
    let mut wrapped = [-1.0; 5];
    assert_eq!(consumer.read(&mut wrapped).read_samples, 5);
    assert_eq!(wrapped, [3.0, 4.0, 5.0, 6.0, 7.0]);
}

#[test]
fn partial_read_leaves_remainder_untouched_and_counts_underrun() {
    let (mut producer, mut consumer, metrics) = audio_ring(8).expect("valid capacity");
    assert_eq!(
        producer.write(&[0.25, -0.5]),
        WriteOutcome::Written { samples: 2 }
    );

    let mut output = [99.0; 5];
    assert_eq!(
        consumer.read(&mut output),
        ReadOutcome {
            read_samples: 2,
            state: ReadState::Underrun,
        }
    );
    assert_eq!(output, [0.25, -0.5, 99.0, 99.0, 99.0]);
    assert_eq!(metrics.snapshot().underruns, 1);
    assert_eq!(metrics.snapshot().underrun_samples, 3);
}

#[test]
fn producer_reports_disconnected_consumer_and_counts_drop() {
    let (mut producer, consumer, metrics) = audio_ring(4).expect("valid capacity");
    drop(consumer);

    assert!(producer.is_disconnected());
    assert_eq!(
        producer.write(&[1.0, 2.0]),
        WriteOutcome::Disconnected { samples: 2 }
    );
    assert_eq!(metrics.snapshot().dropped_samples, 2);
}

#[test]
fn consumer_drains_buffer_then_reports_disconnected_producer() {
    let (mut producer, mut consumer, metrics) = audio_ring(4).expect("valid capacity");
    assert_eq!(
        producer.write(&[1.0, 2.0]),
        WriteOutcome::Written { samples: 2 }
    );
    drop(producer);

    assert!(consumer.is_disconnected());
    let mut output = [-1.0; 4];
    assert_eq!(
        consumer.read(&mut output),
        ReadOutcome {
            read_samples: 2,
            state: ReadState::Disconnected,
        }
    );
    assert_eq!(output, [1.0, 2.0, -1.0, -1.0]);
    assert_eq!(metrics.snapshot().underruns, 1);
    assert_eq!(metrics.snapshot().underrun_samples, 2);

    let mut next = [-2.0; 1];
    assert_eq!(
        consumer.read(&mut next),
        ReadOutcome {
            read_samples: 0,
            state: ReadState::Disconnected,
        }
    );
    assert_eq!(next, [-2.0]);
}

#[test]
fn empty_operations_do_not_create_false_drop_or_underrun_counts() {
    let (mut producer, mut consumer, metrics) = audio_ring(2).expect("valid capacity");

    assert_eq!(producer.write(&[]), WriteOutcome::Written { samples: 0 });
    assert_eq!(
        consumer.read(&mut []),
        ReadOutcome {
            read_samples: 0,
            state: ReadState::Complete,
        }
    );
    assert_eq!(metrics.snapshot(), Default::default());
}

#[test]
fn concurrent_odd_capacity_wraps_under_full_and_empty_pressure() {
    use std::thread;

    const CAPACITY: usize = 31;
    const TOTAL: usize = 200_000;

    let (mut producer, mut consumer, metrics) = audio_ring(CAPACITY).expect("valid odd capacity");

    // Deterministically begin near full and prove all-or-drop pressure before
    // the two endpoints race across thousands of physical wraps.
    let initial: Vec<f32> = (0..28).map(|value| value as f32).collect();
    assert_eq!(
        producer.write(&initial),
        WriteOutcome::Written { samples: 28 }
    );
    let initially_dropped: Vec<f32> = (28..35).map(|value| value as f32).collect();
    assert_eq!(
        producer.write(&initially_dropped),
        WriteOutcome::DroppedFull { samples: 7 }
    );

    let producer_thread = thread::spawn(move || {
        let mut next = 28;
        let chunk_lengths = [7, 3, 11, 5, 2];
        let mut attempt = 0;
        while next < TOTAL {
            let length = chunk_lengths[attempt % chunk_lengths.len()].min(TOTAL - next);
            let mut chunk = [0.0_f32; 11];
            for (offset, sample) in chunk[..length].iter_mut().enumerate() {
                *sample = (next + offset) as f32;
            }
            match producer.write(&chunk[..length]) {
                WriteOutcome::Written { .. } => next += length,
                WriteOutcome::DroppedFull { .. } => thread::yield_now(),
                WriteOutcome::Disconnected { .. } => {
                    panic!("consumer disconnected before sequence completed")
                }
            }
            attempt += 1;
        }
        // Endpoint destruction races with the consumer's final drain. It is
        // deliberately outside callback lifetime but validates wrapper state.
        drop(producer);
    });

    let consumer_thread = thread::spawn(move || {
        let request_lengths = [5, 13, 1, 9, 4];
        let mut request = 0;
        let mut expected = 0;
        loop {
            let length = request_lengths[request % request_lengths.len()];
            let mut output = [-1.0_f32; 13];
            let outcome = consumer.read(&mut output[..length]);
            for sample in &output[..outcome.read_samples] {
                assert_eq!(*sample, expected as f32, "FIFO mismatch at {expected}");
                expected += 1;
            }
            request += 1;
            if outcome.state == ReadState::Disconnected && outcome.read_samples == 0 {
                break;
            }
            if outcome.read_samples < length {
                thread::yield_now();
            }
        }
        expected
    });

    producer_thread.join().expect("producer thread completed");
    let received = consumer_thread.join().expect("consumer thread completed");
    assert_eq!(received, TOTAL);

    let snapshot = metrics.snapshot();
    assert!(
        snapshot.dropped_samples >= 7,
        "full-pressure writes must be observed"
    );
    assert!(
        snapshot.underruns > 0,
        "empty-pressure reads must be observed"
    );
}

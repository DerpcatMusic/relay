use relay_resample::{
    AdaptiveClockConfig, AdaptiveClockConverter, OutputInputRatioCorrectionPpm, ResampleError,
    WorkerResampler,
};

const RATE_PAIRS: [(usize, usize); 7] = [
    (44_100, 48_000),
    (48_000, 44_100),
    (48_000, 48_000),
    (96_000, 48_000),
    (48_000, 96_000),
    (192_000, 48_000),
    (48_000, 192_000),
];

fn converter(input_rate: usize, output_rate: usize, chunk: usize) -> AdaptiveClockConverter {
    AdaptiveClockConverter::new(
        input_rate,
        output_rate,
        1,
        chunk,
        AdaptiveClockConfig {
            max_correction_ppm: 100_000.0,
            smoothing_time_seconds: 0.01,
        },
    )
    .expect("supported adaptive converter")
}

#[test]
fn finish_covers_every_rate_duration_prefix_and_ratio_extreme() {
    for (input_rate, output_rate) in RATE_PAIRS {
        for chunk in [240, 480, 960] {
            for correction in [-100_000.0, 100_000.0] {
                for valid in [1, chunk - 1, chunk] {
                    let mut converter = converter(input_rate, output_rate, chunk);
                    converter.set_output_input_correction(
                        OutputInputRatioCorrectionPpm::new(correction).expect("finite correction"),
                    );
                    let live = converter.requirements();
                    let input = vec![0.0; live.input_frames_next];
                    let mut live_output = vec![0.0; live.output_frames_max];
                    for _ in 0..3 {
                        converter
                            .process_interleaved(&input, &mut live_output)
                            .expect("normal prefix");
                    }

                    let requirements = converter.finish_requirements().expect("finite bounds");
                    assert_eq!(requirements.final_input_frames, chunk);
                    assert_eq!(requirements.channels, 1);
                    let mut final_input = vec![0.0; chunk];
                    final_input[valid - 1] = 0.25;
                    let mut output = vec![f32::NAN; requirements.output_workspace_frames];
                    let input_ptr = final_input.as_ptr();
                    let input_capacity = final_input.capacity();
                    let output_ptr = output.as_ptr();
                    let output_capacity = output.capacity();
                    let report = converter
                        .finish_interleaved(&final_input, valid, &mut output)
                        .expect("bounded terminal drain");

                    assert_eq!(report.valid_input_frames, valid);
                    assert_eq!(report.leading_trim_frames, requirements.leading_trim_frames);
                    assert_eq!(report.pending_output_frames, 0);
                    assert_eq!(
                        report.generated_output_frames,
                        report.output_frames + report.trailing_trim_frames
                    );
                    assert!(report.generated_output_frames <= requirements.output_workspace_frames);
                    assert!(
                        output[..report.generated_output_frames]
                            .iter()
                            .all(|sample| sample.is_finite())
                    );
                    assert_eq!(final_input.as_ptr(), input_ptr);
                    assert_eq!(final_input.capacity(), input_capacity);
                    assert_eq!(output.as_ptr(), output_ptr);
                    assert_eq!(output.capacity(), output_capacity);
                }
            }
        }
    }
}

#[test]
fn one_frame_chunks_prove_the_multi_pump_bound() {
    for (input_rate, output_rate) in RATE_PAIRS {
        for correction in [-100_000.0, 100_000.0] {
            let mut converter = converter(input_rate, output_rate, 1);
            converter.set_output_input_correction(
                OutputInputRatioCorrectionPpm::new(correction).expect("finite correction"),
            );
            let requirements = converter.finish_requirements().expect("finite bounds");
            let mut output = vec![0.0; requirements.output_workspace_frames];
            let report = converter
                .finish_interleaved(&[1.0], 1, &mut output)
                .expect("multi-block zero pump");
            assert_eq!(
                report.generated_output_frames,
                report.output_frames + report.trailing_trim_frames
            );
            assert!(report.generated_output_frames <= requirements.output_workspace_frames);
        }
    }
}

#[test]
fn validation_is_transactional_and_only_checks_the_valid_prefix() {
    let chunk = 480;
    let mut converter = converter(48_000, 44_100, chunk);
    converter.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(1_000.0).expect("finite correction"),
    );
    let ratio = converter.ratio();
    let smoothed = converter.smoothed_correction_ppm();
    let requirements = converter.finish_requirements().expect("requirements");
    let mut output = vec![7.0; requirements.output_workspace_frames];

    assert_eq!(
        converter.finish_interleaved(&vec![0.0; chunk - 1], chunk - 1, &mut output),
        Err(ResampleError::InvalidInputLength {
            expected: chunk,
            actual: chunk - 1,
        })
    );
    assert_eq!(
        converter.finish_interleaved(&vec![0.0; chunk], 0, &mut output),
        Err(ResampleError::InvalidValidInputFrames {
            valid: 0,
            maximum: chunk,
        })
    );
    assert_eq!(
        converter.finish_interleaved(&vec![0.0; chunk], chunk + 1, &mut output),
        Err(ResampleError::InvalidValidInputFrames {
            valid: chunk + 1,
            maximum: chunk,
        })
    );
    let mut nonfinite = vec![0.0; chunk];
    nonfinite[17] = f32::NAN;
    assert_eq!(
        converter.finish_interleaved(&nonfinite, 18, &mut output),
        Err(ResampleError::NonFiniteInput { sample_index: 17 })
    );
    let mut short = vec![0.0; requirements.output_workspace_frames - 1];
    assert_eq!(
        converter.finish_interleaved(&vec![0.0; chunk], chunk, &mut short),
        Err(ResampleError::OutputBufferTooSmall {
            required: requirements.output_workspace_frames,
            actual: requirements.output_workspace_frames - 1,
        })
    );
    assert_eq!(converter.ratio(), ratio);
    assert_eq!(converter.smoothed_correction_ppm(), smoothed);
    assert!(output.iter().all(|sample| *sample == 7.0));

    // Padding beyond the declared prefix is not media and is never inspected.
    nonfinite.fill(f32::NAN);
    nonfinite[0] = 1.0;
    converter
        .finish_interleaved(&nonfinite, 1, &mut output)
        .expect("nonfinite padding suffix is ignored");
}

fn deterministic_run(converter: &mut AdaptiveClockConverter) -> (Vec<f32>, Vec<f32>) {
    let live = converter.requirements();
    let mut input = vec![0.0; live.input_frames_next];
    input[0] = 1.0;
    let mut live_output = vec![0.0; live.output_frames_max];
    let live_report = converter
        .process_interleaved(&input, &mut live_output)
        .expect("live prefix");
    live_output.truncate(live_report.output_frames);

    let requirements = converter.finish_requirements().expect("requirements");
    let mut final_input = vec![0.0; requirements.final_input_frames];
    final_input[requirements.final_input_frames - 1] = -0.5;
    let mut finish_output = vec![0.0; requirements.output_workspace_frames];
    let finish_report = converter
        .finish_interleaved(
            &final_input,
            requirements.final_input_frames,
            &mut finish_output,
        )
        .expect("finish");
    finish_output.truncate(finish_report.output_frames);
    (live_output, finish_output)
}

#[test]
fn finish_is_terminal_and_reset_is_byte_deterministic() {
    let mut converter = converter(48_000, 96_000, 480);
    let first = deterministic_run(&mut converter);
    let requirements = converter
        .finish_requirements()
        .expect("requirements remain queryable");
    let input = vec![0.0; requirements.final_input_frames];
    let mut output = vec![0.0; requirements.output_workspace_frames];
    assert_eq!(
        converter.process_interleaved(&input, &mut output),
        Err(ResampleError::EndOfStream)
    );
    assert_eq!(
        converter.finish_interleaved(&input, requirements.final_input_frames, &mut output),
        Err(ResampleError::EndOfStream)
    );

    converter.reset();
    assert_eq!(converter.target_correction_ppm(), 0.0);
    assert_eq!(converter.smoothed_correction_ppm(), 0.0);
    assert_eq!(converter.ratio(), 2.0);
    let second = deterministic_run(&mut converter);
    assert_eq!(first, second);
}

#[test]
fn first_and_last_valid_impulses_survive_reported_trims_at_every_rate() {
    let chunk = 480;
    for (input_rate, output_rate) in RATE_PAIRS {
        for impulse_at_start in [true, false] {
            let mut converter = converter(input_rate, output_rate, chunk);
            let live = converter.requirements();
            let mut first = vec![0.0; live.input_frames_next];
            if impulse_at_start {
                first[0] = 1.0;
            }
            let mut live_output = vec![0.0; live.output_frames_max];
            let live_report = converter
                .process_interleaved(&first, &mut live_output)
                .expect("live block");

            let requirements = converter.finish_requirements().expect("requirements");
            let mut final_input = vec![0.0; chunk];
            if !impulse_at_start {
                final_input[chunk - 1] = 1.0;
            }
            let mut finish_output = vec![0.0; requirements.output_workspace_frames];
            let finish_report = converter
                .finish_interleaved(&final_input, chunk, &mut finish_output)
                .expect("finish");
            let mut complete = live_output[..live_report.output_frames].to_vec();
            complete.extend_from_slice(&finish_output[..finish_report.output_frames]);
            let useful = &complete[finish_report.leading_trim_frames..];
            let edge_frames = (output_rate / 200).max(16).min(useful.len());
            let edge = if impulse_at_start {
                &useful[..edge_frames]
            } else {
                &useful[useful.len() - edge_frames..]
            };
            let peak = edge
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
            assert!(
                peak > 0.005,
                "{input_rate}->{output_rate}, start={impulse_at_start}, peak={peak}"
            );
        }
    }
}

#[test]
fn opposite_ratio_finish_is_panic_free_for_valid_admitted_history() {
    let n = 240;
    let mut r = AdaptiveClockConverter::new(
        48_000,
        48_000,
        2,
        n,
        AdaptiveClockConfig {
            max_correction_ppm: 100_000.0,
            smoothing_time_seconds: 0.000_001,
        },
    )
    .expect("valid test operation");

    let q = r.requirements();
    let input = vec![0.0; q.input_frames_next * 2];
    let mut live_output = vec![0.0; q.output_frames_max * 2];
    r.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(100_000.0).expect("valid test operation"),
    );
    r.process_interleaved(&input, &mut live_output)
        .expect("valid test operation");

    r.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(-100_000.0).expect("valid test operation"),
    );
    let fq = r.finish_requirements().expect("valid test operation");
    let final_input = vec![0.0; n * 2];
    let mut finish_output = vec![0.0; fq.output_workspace_frames * 2];
    r.finish_interleaved(&final_input, n, &mut finish_output)
        .expect("valid test operation");
}

#[test]
fn stereo_alternating_and_opposite_ratio_matrix_is_panic_free() {
    for (input_rate, output_rate) in RATE_PAIRS {
        for n in [240, 480, 960] {
            for first_ppm in [-100_000.0, 100_000.0] {
                for history_blocks in [1, 2, 7] {
                    for valid in [1, n - 1, n] {
                        let mut r = AdaptiveClockConverter::new(
                            input_rate,
                            output_rate,
                            2,
                            n,
                            AdaptiveClockConfig {
                                max_correction_ppm: 100_000.0,
                                smoothing_time_seconds: 0.000_001,
                            },
                        )
                        .expect("valid test operation");
                        let mut most_recent_ppm = 0.0;
                        for block in 0..history_blocks {
                            most_recent_ppm = if block % 2 == 0 {
                                first_ppm
                            } else {
                                -first_ppm
                            };
                            r.set_output_input_correction(
                                OutputInputRatioCorrectionPpm::new(most_recent_ppm)
                                    .expect("valid test operation"),
                            );
                            let q = r.requirements();
                            assert_eq!(q.input_frames_next, n);
                            assert_eq!(q.input_frames_max, n);
                            let mut input = vec![0.0; q.input_frames_next * 2];
                            for (frame, samples) in input.chunks_exact_mut(2).enumerate() {
                                samples[0] = frame as f32 * 0.000_1;
                                samples[1] = -(frame as f32) * 0.000_1;
                            }
                            let mut output = vec![0.0; q.output_frames_max * 2];
                            r.process_interleaved(&input, &mut output)
                                .expect("valid test operation");
                        }
                        r.set_output_input_correction(
                            OutputInputRatioCorrectionPpm::new(-most_recent_ppm)
                                .expect("valid test operation"),
                        );
                        let fq = r.finish_requirements().expect("valid test operation");
                        let final_input = vec![0.0; n * 2];
                        let mut finish_output = vec![f32::NAN; fq.output_workspace_frames * 2];
                        let report = r
                            .finish_interleaved(&final_input, valid, &mut finish_output)
                            .expect("valid test operation");
                        assert!(
                            finish_output[..report.generated_output_frames * 2]
                                .iter()
                                .all(|sample| sample.is_finite()),
                            "{input_rate}->{output_rate}, n={n}, first={first_ppm}, history={history_blocks}, valid={valid}"
                        );
                        assert!(
                            finish_output[report.generated_output_frames * 2..]
                                .iter()
                                .all(|sample| sample.is_nan()),
                            "finish wrote beyond its reported prefix"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn finish_advances_smoothing_by_valid_duration_once_then_freezes_it() {
    let n = 480;
    let valid = 137;
    let tau = 0.031;
    let mut r = AdaptiveClockConverter::new(
        48_000,
        44_100,
        2,
        n,
        AdaptiveClockConfig {
            max_correction_ppm: 100_000.0,
            smoothing_time_seconds: tau,
        },
    )
    .expect("valid test operation");
    r.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(80_000.0).expect("valid test operation"),
    );
    let q = r.requirements();
    let input = vec![0.0; q.input_frames_next * 2];
    let mut live_output = vec![0.0; q.output_frames_max * 2];
    r.process_interleaved(&input, &mut live_output)
        .expect("valid test operation");

    r.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(-90_000.0).expect("valid test operation"),
    );
    let before = r.smoothed_correction_ppm();
    let smoothing = 1.0 - (-(valid as f64 / 48_000.0) / tau).exp();
    let expected = before + smoothing * (-90_000.0 - before);
    let fq = r.finish_requirements().expect("valid test operation");
    let final_input = vec![0.0; n * 2];
    let mut output = vec![0.0; fq.output_workspace_frames * 2];
    r.finish_interleaved(&final_input, valid, &mut output)
        .expect("valid test operation");
    assert!((r.smoothed_correction_ppm() - expected).abs() <= f64::EPSILON * expected.abs() * 4.0);
    assert!((r.ratio() - (44_100.0 / 48_000.0) * (1.0 + expected / 1_000_000.0)).abs() < 1.0e-12);
}

#[test]
fn collected_stream_obeys_strong_trim_accounting_across_prior_phases() {
    for history_blocks in [0, 1, 2, 7, 31] {
        let n = 240;
        let mut r = AdaptiveClockConverter::new(
            48_000,
            48_000,
            2,
            n,
            AdaptiveClockConfig {
                max_correction_ppm: 100_000.0,
                smoothing_time_seconds: 0.000_001,
            },
        )
        .expect("valid test operation");
        let mut raw = Vec::new();
        let mut streaming_generated_frames = 0;
        for block in 0..history_blocks {
            let ppm = if block % 2 == 0 {
                -100_000.0
            } else {
                100_000.0
            };
            r.set_output_input_correction(
                OutputInputRatioCorrectionPpm::new(ppm).expect("valid test operation"),
            );
            let q = r.requirements();
            let input = vec![0.125; q.input_frames_next * 2];
            let mut output = vec![0.0; q.output_frames_max * 2];
            let report = r
                .process_interleaved(&input, &mut output)
                .expect("valid test operation");
            streaming_generated_frames += report.output_frames;
            raw.extend_from_slice(&output[..report.output_frames * 2]);
        }
        r.set_output_input_correction(
            OutputInputRatioCorrectionPpm::new(if history_blocks % 2 == 0 {
                100_000.0
            } else {
                -100_000.0
            })
            .expect("valid test operation"),
        );
        let fq = r.finish_requirements().expect("valid test operation");
        let final_input = vec![0.125; n * 2];
        let mut output = vec![0.0; fq.output_workspace_frames * 2];
        let report = r
            .finish_interleaved(&final_input, n - 1, &mut output)
            .expect("valid test operation");
        raw.extend_from_slice(&output[..report.generated_output_frames * 2]);

        assert_eq!(
            raw.len() / 2,
            streaming_generated_frames + report.generated_output_frames
        );
        let useful_start = report.leading_trim_frames;
        let useful_end = raw.len() / 2 - report.trailing_trim_frames;
        assert!(useful_start <= useful_end);
        assert_eq!(
            useful_end - useful_start,
            streaming_generated_frames + report.generated_output_frames
                - report.leading_trim_frames
                - report.trailing_trim_frames
        );
        assert_eq!(
            report.output_frames,
            report.generated_output_frames - report.trailing_trim_frames
        );
    }
}

#[test]
fn stereo_dc_gain_and_channel_isolation_survive_adaptive_trim() {
    let n = 480;
    let mut r = AdaptiveClockConverter::new(
        48_000,
        48_000,
        2,
        n,
        AdaptiveClockConfig {
            max_correction_ppm: 100_000.0,
            smoothing_time_seconds: 0.01,
        },
    )
    .expect("valid test operation");
    let mut raw = Vec::new();
    for _ in 0..6 {
        let q = r.requirements();
        let mut input = vec![0.0; q.input_frames_next * 2];
        for samples in input.chunks_exact_mut(2) {
            samples[0] = 0.25;
        }
        let mut output = vec![0.0; q.output_frames_max * 2];
        let report = r
            .process_interleaved(&input, &mut output)
            .expect("valid test operation");
        raw.extend_from_slice(&output[..report.output_frames * 2]);
    }
    let fq = r.finish_requirements().expect("valid test operation");
    let mut final_input = vec![0.0; n * 2];
    for samples in final_input.chunks_exact_mut(2) {
        samples[0] = 0.25;
    }
    let mut output = vec![0.0; fq.output_workspace_frames * 2];
    let report = r
        .finish_interleaved(&final_input, n, &mut output)
        .expect("valid test operation");
    raw.extend_from_slice(&output[..report.generated_output_frames * 2]);
    let frames = raw.chunks_exact(2).collect::<Vec<_>>();
    let useful = &frames[report.leading_trim_frames..frames.len() - report.trailing_trim_frames];
    let guard = 256.min(useful.len() / 4);
    let steady = &useful[guard..useful.len() - guard];
    let left_mean = steady.iter().map(|frame| frame[0] as f64).sum::<f64>() / steady.len() as f64;
    let right_peak = steady
        .iter()
        .map(|frame| frame[1].abs())
        .fold(0.0_f32, f32::max);
    assert!((left_mean - 0.25).abs() < 1.0e-4, "left_mean={left_mean}");
    assert!(right_peak < 1.0e-7, "right_peak={right_peak}");
}

#[test]
fn finish_nonfinite_backend_fault_is_sticky_and_reset_recovers() {
    let n = 240;
    let mut r = converter(48_000, 192_000, n);
    let fq = r.finish_requirements().expect("valid test operation");
    let explosive = vec![f32::MAX; n];
    let mut output = vec![0.0; fq.output_workspace_frames];
    assert!(matches!(
        r.finish_interleaved(&explosive, n, &mut output),
        Err(ResampleError::NonFiniteOutput { .. })
    ));
    assert_eq!(
        r.finish_interleaved(&vec![0.0; n], n, &mut output),
        Err(ResampleError::EndOfStream)
    );
    assert_eq!(
        r.process_interleaved(&vec![0.0; n], &mut output),
        Err(ResampleError::EndOfStream)
    );
    r.reset();
    let q = r.requirements();
    let mut live_output = vec![0.0; q.output_frames_max];
    r.process_interleaved(&vec![0.0; q.input_frames_next], &mut live_output)
        .expect("valid test operation");
}

use std::f32::consts::TAU;

use relay_resample::{
    AdaptiveClockConfig, AdaptiveClockConverter, FiniteFixedRatioConverter, FixedRatioConverter,
    OutputInputRatioCorrectionPpm, ResampleError, WorkerResampler,
};

const CHUNK: usize = 480;

fn buffers_for<R: WorkerResampler>(converter: &R) -> (Vec<f32>, Vec<f32>) {
    let req = converter.requirements();
    (
        vec![0.0; req.input_frames_next * req.channels],
        vec![0.0; req.output_frames_max * req.channels],
    )
}

#[test]
fn supported_rate_matrix_processes_mono_and_stereo_silence() {
    let pairs = [
        (44_100, 48_000),
        (48_000, 44_100),
        (48_000, 48_000),
        (96_000, 48_000),
        (48_000, 96_000),
        (192_000, 48_000),
        (48_000, 192_000),
    ];
    for &(input_rate, output_rate) in &pairs {
        for channels in [1, 2] {
            let mut fixed = FixedRatioConverter::new(input_rate, output_rate, channels, CHUNK)
                .expect("supported fixed converter must construct");
            let (input, mut output) = buffers_for(&fixed);
            let report = fixed
                .process_interleaved(&input, &mut output)
                .expect("fixed silence must process");
            assert!(
                output[..report.output_frames * channels]
                    .iter()
                    .all(|sample| *sample == 0.0)
            );

            let mut adaptive = AdaptiveClockConverter::new(
                input_rate,
                output_rate,
                channels,
                CHUNK,
                AdaptiveClockConfig::default(),
            )
            .expect("supported adaptive converter must construct");
            let (input, mut output) = buffers_for(&adaptive);
            let report = adaptive
                .process_interleaved(&input, &mut output)
                .expect("adaptive silence must process");
            assert!(
                output[..report.output_frames * channels]
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
        }
    }
}

#[test]
fn impulse_is_delayed_but_preserved_and_finite() {
    let mut converter =
        FixedRatioConverter::new(44_100, 48_000, 1, CHUNK).expect("converter must construct");
    assert!(converter.requirements().output_delay > 0);
    let (mut input, mut output) = buffers_for(&converter);
    input[0] = 1.0;

    let mut peak = 0.0_f32;
    for block in 0..6 {
        let report = converter
            .process_interleaved(&input, &mut output)
            .expect("impulse stream must process");
        for sample in &output[..report.output_frames] {
            assert!(sample.is_finite());
            peak = peak.max(sample.abs());
        }
        if block == 0 {
            input[0] = 0.0;
        }
    }
    assert!(peak > 0.1, "impulse response peak was {peak}");
}

#[test]
fn sine_stays_finite_nontrivial_and_channel_isolated() {
    let mut converter =
        FixedRatioConverter::new(48_000, 44_100, 2, CHUNK).expect("converter must construct");
    let (mut input, mut output) = buffers_for(&converter);
    let mut phase_frame = 0_usize;
    let mut left_energy = 0.0_f64;
    let mut right_peak = 0.0_f32;

    for _ in 0..12 {
        for frame in 0..converter.requirements().input_frames_next {
            input[frame * 2] = (TAU * 1_000.0 * phase_frame as f32 / 48_000.0).sin() * 0.5;
            input[frame * 2 + 1] = 0.0;
            phase_frame += 1;
        }
        let report = converter
            .process_interleaved(&input, &mut output)
            .expect("sine stream must process");
        for frame in 0..report.output_frames {
            let left = output[frame * 2];
            let right = output[frame * 2 + 1];
            assert!(left.is_finite() && right.is_finite());
            left_energy += f64::from(left) * f64::from(left);
            right_peak = right_peak.max(right.abs());
        }
    }
    assert!(left_energy > 1.0);
    assert!(right_peak < 1.0e-6, "silent channel leaked: {right_peak}");
}

#[test]
fn fixed_ratio_frame_count_tracks_the_exact_rate_ratio() {
    let mut converter =
        FixedRatioConverter::new(44_100, 48_000, 1, CHUNK).expect("converter must construct");
    let (input, mut output) = buffers_for(&converter);
    let delay = converter.requirements().output_delay;
    let calls = 100_usize;
    let mut consumed = 0_usize;
    let mut produced = 0_usize;
    for _ in 0..calls {
        let report = converter
            .process_interleaved(&input, &mut output)
            .expect("silence stream must process");
        consumed += report.input_frames;
        produced += report.output_frames;
    }
    let expected = consumed as f64 * 48_000.0 / 44_100.0;
    // Fixed-input FFT streaming may retain a bounded tail internally. The
    // returned count plus that tail must track the exact long-term ratio.
    let retained_tail = expected - produced as f64;
    assert!(retained_tail >= 0.0);
    assert!(retained_tail <= converter.requirements().output_frames_max as f64);
    assert!(retained_tail >= delay as f64);
}

#[test]
fn nonfinite_input_and_clock_requests_are_rejected() {
    let mut fixed =
        FixedRatioConverter::new(48_000, 48_000, 1, CHUNK).expect("converter must construct");
    let (mut input, mut output) = buffers_for(&fixed);
    input[17] = f32::NAN;
    assert_eq!(
        fixed.process_interleaved(&input, &mut output),
        Err(ResampleError::NonFiniteInput { sample_index: 17 })
    );

    assert_eq!(
        OutputInputRatioCorrectionPpm::new(f64::INFINITY),
        Err(ResampleError::NonFiniteClockCorrection)
    );
}

#[test]
fn adaptive_correction_is_clamped_and_smoothed_within_ratio_bounds() {
    let config = AdaptiveClockConfig {
        max_correction_ppm: 200.0,
        smoothing_time_seconds: 0.1,
    };
    let mut converter = AdaptiveClockConverter::new(48_000, 48_000, 1, CHUNK, config)
        .expect("converter must construct");
    let accepted = converter.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(50_000.0).expect("finite command"),
    );
    assert_eq!(accepted.clamped_ppm, 200.0);

    let (input, mut output) = buffers_for(&converter);
    converter
        .process_interleaved(&input, &mut output)
        .expect("adaptive chunk must process");
    assert!(converter.smoothed_correction_ppm() > 0.0);
    assert!(converter.smoothed_correction_ppm() < 200.0);
    assert!((1.0..=1.000_2).contains(&converter.ratio()));

    let accepted = converter.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(-50_000.0).expect("finite command"),
    );
    assert_eq!(accepted.clamped_ppm, -200.0);
    for _ in 0..200 {
        converter
            .process_interleaved(&input, &mut output)
            .expect("adaptive chunk must process");
        assert!((0.999_8..=1.000_2).contains(&converter.ratio()));
    }
    assert!(converter.smoothed_correction_ppm() < 0.0);
    assert!(converter.smoothed_correction_ppm() >= -200.0);
}

#[test]
fn processing_requires_preallocated_maximum_output_capacity() {
    let mut converter =
        FixedRatioConverter::new(44_100, 48_000, 1, CHUNK).expect("converter must construct");
    let req = converter.requirements();
    let input = vec![0.0; req.input_frames_next];
    let mut output = vec![0.0; req.output_frames_max - 1];
    assert_eq!(
        converter.process_interleaved(&input, &mut output),
        Err(ResampleError::OutputBufferTooSmall {
            required: req.output_frames_max,
            actual: req.output_frames_max - 1,
        })
    );
}

const RATE_PAIRS: [(usize, usize); 7] = [
    (44_100, 48_000),
    (48_000, 44_100),
    (48_000, 48_000),
    (96_000, 48_000),
    (48_000, 96_000),
    (192_000, 48_000),
    (48_000, 192_000),
];

fn finite_resample(
    input_rate: usize,
    output_rate: usize,
    channels: usize,
    input: &[f32],
) -> (Vec<f32>, relay_resample::FiniteProcessReport) {
    let mut converter = FiniteFixedRatioConverter::new(input_rate, output_rate, channels, CHUNK)
        .expect("finite converter must construct");
    let req = converter
        .requirements(input.len() / channels)
        .expect("finite size must fit");
    let mut workspace = vec![0.0; req.output_workspace_frames * channels];
    let report = converter
        .process_interleaved(input, &mut workspace)
        .expect("finite stream must process");
    let range = report.valid_output_frame_range();
    let samples = range.start * channels..range.end * channels;
    (workspace[samples].to_vec(), report)
}

#[test]
fn fixed_unity_streaming_is_exact_zero_delay_passthrough() {
    for channels in [1, 2] {
        let mut converter =
            FixedRatioConverter::new(48_000, 48_000, channels, CHUNK).expect("unity converter");
        let req = converter.requirements();
        assert_eq!(req.output_delay, 0);
        assert_eq!(req.input_frames_next, CHUNK);
        assert_eq!(req.output_frames_next, CHUNK);
        let input: Vec<f32> = (0..CHUNK * channels)
            .map(|index| index as f32 * 0.000_1 - 0.25)
            .collect();
        let input_ptr = input.as_ptr();
        let mut output = vec![f32::NAN; CHUNK * channels];
        let output_ptr = output.as_ptr();
        let report = converter
            .process_interleaved(&input, &mut output)
            .expect("passthrough");
        assert_eq!(report.input_frames, CHUNK);
        assert_eq!(report.output_frames, CHUNK);
        assert_eq!(output, input);
        assert_eq!(input.as_ptr(), input_ptr);
        assert_eq!(output.as_ptr(), output_ptr);
    }
}

#[test]
fn finite_non_aligned_streams_recover_both_boundaries_for_every_rate_pair() {
    let input_frames = CHUNK * 3 + 137;
    for (input_rate, output_rate) in RATE_PAIRS {
        for impulse_frame in [0, input_frames - 1] {
            let mut input = vec![0.0; input_frames];
            input[impulse_frame] = 1.0;
            let (output, report) = finite_resample(input_rate, output_rate, 1, &input);
            let expected_frames =
                (input_frames as u128 * output_rate as u128).div_ceil(input_rate as u128) as usize;
            assert_eq!(report.input_frames, input_frames);
            assert_eq!(report.output_frames, expected_frames);
            assert_eq!(output.len(), expected_frames);
            assert_eq!(
                report.generated_output_frames,
                report.leading_trim_frames + report.output_frames + report.trailing_trim_frames
            );
            assert!(output.iter().all(|sample| sample.is_finite()));
            let peak = output
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
            assert!(
                peak > 0.05,
                "{input_rate}->{output_rate}, impulse {impulse_frame}, peak {peak}"
            );
            let edge_window = (output_rate / 100).max(1).min(output.len());
            let edge_peak = if impulse_frame == 0 {
                output[..edge_window]
                    .iter()
                    .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
            } else {
                output[output.len() - edge_window..]
                    .iter()
                    .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
            };
            assert!(
                edge_peak > 0.05,
                "{input_rate}->{output_rate}, missing boundary impulse: {edge_peak}"
            );
        }
    }
}

#[test]
fn finite_stereo_dc_gain_and_channel_isolation_are_bounded() {
    let frames = 48_000 + 137;
    for (input_rate, output_rate) in RATE_PAIRS {
        let mut input = vec![0.0; frames * 2];
        for frame in 0..frames {
            input[frame * 2] = 0.25;
        }
        let (output, _) = finite_resample(input_rate, output_rate, 2, &input);
        let margin = (output_rate / 50).min(output.len() / 4 / 2);
        let mut left_error = 0.0_f32;
        let mut right_peak = 0.0_f32;
        for frame in margin..output.len() / 2 - margin {
            left_error = left_error.max((output[frame * 2] - 0.25).abs());
            right_peak = right_peak.max(output[frame * 2 + 1].abs());
        }
        // FFT passband ripple is far below this 0.2% deterministic guard band;
        // the margin excludes finite-clip boundary ringing.
        assert!(
            left_error < 5.0e-4,
            "{input_rate}->{output_rate}: {left_error}"
        );
        assert!(
            right_peak < 1.0e-7,
            "{input_rate}->{output_rate}: {right_peak}"
        );
    }
}

#[test]
fn finite_impulse_area_and_passband_sine_gain_are_stable() {
    let frames = 48_000 + 137;
    for (input_rate, output_rate) in RATE_PAIRS {
        let ratio = output_rate as f64 / input_rate as f64;
        let mut impulse = vec![0.0; frames];
        impulse[frames / 2] = 1.0;
        let (impulse_output, _) = finite_resample(input_rate, output_rate, 1, &impulse);
        let area: f64 = impulse_output.iter().map(|sample| f64::from(*sample)).sum();
        // Unity-DC interpolation makes impulse area equal to the output/input
        // density ratio. 0.5% catches gain normalization regressions while
        // allowing finite f32 FFT roundoff.
        assert!(
            (area - ratio).abs() < ratio * 0.005 + 1.0e-6,
            "{input_rate}->{output_rate}: area {area}, expected {ratio}"
        );

        let frequency = 1_000.0_f32;
        let input: Vec<f32> = (0..frames)
            .map(|frame| (TAU * frequency * frame as f32 / input_rate as f32).sin() * 0.5)
            .collect();
        let (output, _) = finite_resample(input_rate, output_rate, 1, &input);
        let margin = (output_rate / 50).min(output.len() / 4);
        let rms = (output[margin..output.len() - margin]
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum::<f64>()
            / (output.len() - 2 * margin) as f64)
            .sqrt();
        let expected_rms = 0.5 / 2.0_f64.sqrt();
        // 0.2% is much wider than the pinned FFT backend's 1 kHz ripple and
        // narrow enough to detect a meaningful passband gain regression.
        assert!(
            (rms - expected_rms).abs() < expected_rms * 0.002,
            "{input_rate}->{output_rate}: rms {rms}"
        );
    }
}

#[test]
fn downsampling_rejects_a_deterministic_alias_tone() {
    for (input_rate, output_rate) in [(96_000, 48_000), (192_000, 48_000), (48_000, 44_100)] {
        let frames = input_rate + 137;
        let frequency = (input_rate + output_rate) as f32 * 0.25;
        let input: Vec<f32> = (0..frames)
            .map(|frame| (TAU * frequency * frame as f32 / input_rate as f32).sin() * 0.5)
            .collect();
        let (output, _) = finite_resample(input_rate, output_rate, 1, &input);
        let margin = output_rate / 50;
        let rms = (output[margin..output.len() - margin]
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum::<f64>()
            / (output.len() - 2 * margin) as f64)
            .sqrt();
        // The tone is above output Nyquist. -40 dB relative to its 0.354 RMS
        // input is a conservative guard for the pinned default FFT filter.
        assert!(
            rms < 0.003_54,
            "{input_rate}->{output_rate}: alias rms {rms}"
        );
    }
}

#[test]
fn finite_long_run_counts_are_exact_and_buffers_remain_preallocated() {
    for (input_rate, output_rate) in RATE_PAIRS {
        let frames = input_rate * 30 + 137;
        let input = vec![0.0; frames];
        let mut converter =
            FiniteFixedRatioConverter::new(input_rate, output_rate, 1, CHUNK).expect("converter");
        let req = converter.requirements(frames).expect("requirements");
        let expected = (frames as u128 * output_rate as u128).div_ceil(input_rate as u128) as usize;
        assert_eq!(req.output_frames, expected);
        let mut output = vec![0.0; req.output_workspace_frames];
        let input_ptr = input.as_ptr();
        let output_ptr = output.as_ptr();
        let input_capacity = input.capacity();
        let output_capacity = output.capacity();
        let report = converter
            .process_interleaved(&input, &mut output)
            .expect("long finite stream");
        assert_eq!(report.output_frames, expected);
        assert_eq!(report.valid_output_frame_range().len(), expected);
        // Pointer/capacity stability guards the caller-buffer contract. Rubato
        // 4.0's process_into_buffer implementation is separately source-audited
        // because this unsafe-code-forbidden crate cannot install an allocator.
        assert_eq!(input.as_ptr(), input_ptr);
        assert_eq!(output.as_ptr(), output_ptr);
        assert_eq!(input.capacity(), input_capacity);
        assert_eq!(output.capacity(), output_capacity);
    }
}

#[test]
fn finite_validation_rejects_partial_frames_nonfinite_and_small_workspace() {
    let mut stereo = FiniteFixedRatioConverter::new(44_100, 48_000, 2, CHUNK).expect("converter");
    let mut output = vec![0.0; 10_000];
    assert_eq!(
        stereo.process_interleaved(&[0.0; 3], &mut output),
        Err(ResampleError::InvalidInterleavedLength {
            channels: 2,
            actual: 3
        })
    );
    assert_eq!(
        stereo.process_interleaved(&[0.0, f32::NAN], &mut output),
        Err(ResampleError::NonFiniteInput { sample_index: 1 })
    );

    let mut mono = FiniteFixedRatioConverter::new(44_100, 48_000, 1, CHUNK).expect("converter");
    let input = vec![0.0; CHUNK + 1];
    let req = mono.requirements(input.len()).expect("requirements");
    let mut short = vec![0.0; req.output_workspace_frames - 1];
    assert_eq!(
        mono.process_interleaved(&input, &mut short),
        Err(ResampleError::OutputBufferTooSmall {
            required: req.output_workspace_frames,
            actual: req.output_workspace_frames - 1
        })
    );
}

#[test]
fn clock_recovery_multiplier_sign_matches_output_input_correction_boundary() {
    // relay-clock publishes ratio_multiplier = 1 + correction_ppm / 1e6;
    // positive remote drift produces a multiplier below one. This assertion is
    // intentionally expressed only through that cross-crate wire-compatible
    // value, never by passing the positive raw drift to the resampler.
    let relay_clock_fast_remote_multiplier = 0.999_8;
    let command =
        OutputInputRatioCorrectionPpm::from_ratio_multiplier(relay_clock_fast_remote_multiplier)
            .expect("positive multiplier");
    assert!((command.get() + 200.0).abs() < 1.0e-7);
    assert!((command.ratio_multiplier() - relay_clock_fast_remote_multiplier).abs() < 1.0e-15);

    let config = AdaptiveClockConfig {
        max_correction_ppm: 500.0,
        smoothing_time_seconds: 0.01,
    };
    let mut converter =
        AdaptiveClockConverter::new(48_000, 48_000, 1, CHUNK, config).expect("converter");
    let accepted = converter.set_output_input_correction(command);
    assert!(accepted.clamped_ppm < 0.0);
    let (input, mut output) = buffers_for(&converter);
    converter
        .process_interleaved(&input, &mut output)
        .expect("adaptive process");
    assert!(converter.ratio() < 1.0);

    assert_eq!(
        OutputInputRatioCorrectionPpm::from_ratio_multiplier(0.0),
        Err(ResampleError::InvalidOutputInputRatioMultiplier)
    );
}

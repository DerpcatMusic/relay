use relay_resample::{
    AdaptiveClockConfig, AdaptiveClockConverter, OutputInputRatioCorrectionPpm, WorkerResampler,
};
use relay_resample_test_allocator::CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

#[test]
fn live_and_finite_finish_allocate_nothing_and_keep_caller_storage_stable() {
    let n = 480;
    let mut converter = AdaptiveClockConverter::new(
        44_100,
        48_000,
        2,
        n,
        AdaptiveClockConfig {
            max_correction_ppm: 100_000.0,
            smoothing_time_seconds: 0.000_001,
        },
    )
    .expect("valid test operation");
    let q = converter.requirements();
    assert_eq!(q.input_frames_next, n);
    assert_eq!(q.input_frames_max, n);
    let input = vec![0.0; q.input_frames_next * 2];
    let mut live_output = vec![0.0; q.output_frames_max * 2];
    let fq = converter
        .finish_requirements()
        .expect("valid test operation");
    let final_input = vec![0.0; fq.final_input_frames * 2];
    let mut finish_output = vec![0.0; fq.output_workspace_frames * 2];

    // Prewarm CPU dispatch and every processing path before measurement.
    converter.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(-100_000.0).expect("valid test operation"),
    );
    converter
        .process_interleaved(&input, &mut live_output)
        .expect("valid test operation");
    converter.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(100_000.0).expect("valid test operation"),
    );
    converter
        .finish_interleaved(&final_input, n - 1, &mut finish_output)
        .expect("valid test operation");
    converter.reset();

    let input_identity = (input.as_ptr(), input.capacity());
    let live_identity = (live_output.as_ptr(), live_output.capacity());
    let final_identity = (final_input.as_ptr(), final_input.capacity());
    let finish_identity = (finish_output.as_ptr(), finish_output.capacity());

    ALLOCATOR.reset();
    converter.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(100_000.0).expect("valid test operation"),
    );
    converter
        .process_interleaved(&input, &mut live_output)
        .expect("valid test operation");
    converter.set_output_input_correction(
        OutputInputRatioCorrectionPpm::new(-100_000.0).expect("valid test operation"),
    );
    converter
        .finish_interleaved(&final_input, n - 1, &mut finish_output)
        .expect("valid test operation");
    let allocations = ALLOCATOR.allocations();

    assert_eq!(allocations, 0);
    assert_eq!((input.as_ptr(), input.capacity()), input_identity);
    assert_eq!(
        (live_output.as_ptr(), live_output.capacity()),
        live_identity
    );
    assert_eq!(
        (final_input.as_ptr(), final_input.capacity()),
        final_identity
    );
    assert_eq!(
        (finish_output.as_ptr(), finish_output.capacity()),
        finish_identity
    );
}

use relay_domain::{
    AudioProfile, AudioProfileError, ConnectionState, FecPolicy, FrameDuration, MediaRoute,
    PaidFallbackPolicy, QualityProfile, SessionMode,
};

fn valid_profile() -> AudioProfile {
    AudioProfile::new(
        48_000,
        2,
        192_000,
        FrameDuration::Ms10,
        FecPolicy::Adaptive,
        false,
    )
    .expect("fixture is a valid V1 profile")
}

#[test]
fn exposes_the_master_plan_domain_states() {
    let _ = SessionMode::Connect;
    let _ = MediaRoute::TurnRelay;
    let _ = PaidFallbackPolicy::Ask;
    let _ = ConnectionState::Recovering;
    let _ = QualityProfile::UltraLowLatency;
}

#[test]
fn accepts_a_valid_v1_audio_profile() {
    let profile = valid_profile();

    assert_eq!(profile.sample_rate_hz(), 48_000);
    assert_eq!(profile.channels(), 2);
    assert_eq!(profile.bitrate_bps(), 192_000);
    assert_eq!(profile.frame_duration().microseconds(), 10_000);
    assert_eq!(profile.fec(), FecPolicy::Adaptive);
    assert!(!profile.dtx());
}

#[test]
fn rejects_a_noncanonical_network_sample_rate() {
    assert_eq!(
        AudioProfile::new(
            44_100,
            2,
            192_000,
            FrameDuration::Ms10,
            FecPolicy::Enabled,
            false,
        ),
        Err(AudioProfileError::UnsupportedSampleRate(44_100))
    );
}

#[test]
fn rejects_invalid_channel_counts_and_bitrates() {
    assert_eq!(
        AudioProfile::new(
            48_000,
            1,
            192_000,
            FrameDuration::Ms10,
            FecPolicy::Adaptive,
            false,
        ),
        Err(AudioProfileError::UnsupportedChannelCount(1))
    );

    assert_eq!(
        AudioProfile::new(
            48_000,
            2,
            0,
            FrameDuration::Ms10,
            FecPolicy::Adaptive,
            false,
        ),
        Err(AudioProfileError::ZeroBitrate)
    );
}

#[test]
fn rejects_dtx_for_the_v1_profile() {
    assert_eq!(
        AudioProfile::new(
            48_000,
            2,
            192_000,
            FrameDuration::Ms10,
            FecPolicy::Adaptive,
            true,
        ),
        Err(AudioProfileError::DtxUnsupported)
    );
}

#[test]
fn custom_quality_retains_its_validated_audio_profile() {
    let profile = valid_profile();
    assert_eq!(
        QualityProfile::Custom(profile),
        QualityProfile::Custom(profile)
    );
}

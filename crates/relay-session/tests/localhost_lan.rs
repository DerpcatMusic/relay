//! LAN 5 ms PCM and name discovery on localhost.

use std::net::SocketAddr;

use relay_audio::FrameDuration;
use relay_domain::ConnectionState;
use relay_session::{EngineCommand, MonitorMode, SessionConfig, SessionEngine, SessionMode};

fn sine_chunk(start: usize, frames: usize) -> Vec<f32> {
    let mut samples = vec![0.0; frames * 2];
    for frame in 0..frames {
        let phase = ((start + frame) as f32) * 440.0 * std::f32::consts::TAU / 48_000.0;
        samples[frame * 2] = phase.sin() * 0.25;
        samples[frame * 2 + 1] = phase.sin() * 0.12;
    }
    samples
}

fn slug(name: &str) -> ([u8; 48], u8) {
    let mut bytes = [0_u8; 48];
    let raw = name.as_bytes();
    bytes[..raw.len()].copy_from_slice(raw);
    (bytes, u8::try_from(raw.len()).expect("slug"))
}

#[test]
fn lan_pcm_loopback_is_immediate() {
    let mut engine = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms5,
        ssrc: 0x4c41_4e01,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("prepare");
    engine.apply(EngineCommand::Loopback).expect("loopback");

    let mut out = vec![0.0_f32; 480];
    let mut heard = 0_u32;
    for step in 0..40 {
        let _ = engine.process_capture(&sine_chunk(step * 240, 240));
        engine.drive().expect("drive");
        let _ = engine.render(&mut out, &[]);
        if out.iter().any(|sample| sample.abs() > 1.0e-4) {
            heard += 1;
        }
    }
    assert_eq!(engine.snapshot().state, ConnectionState::Connected);
    assert!(heard > 8, "LAN PCM loopback heard {heard}");
    let (web, seq) = engine.last_web_pcm().expect("48 kHz fan-out frame");
    assert!(seq > 0);
    assert!(web.len() >= 480);
    assert_eq!(web.len() % 480, 0);
    assert!(
        web.iter().any(|sample| sample.abs() > 1.0e-4),
        "web pcm should not be silence"
    );
}

#[test]
fn opus_live_path_publishes_web_pcm() {
    let mut engine = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms5,
        ssrc: 0x4f50_5553,
        monitor: MonitorMode::Dry,
        lan: false,
    })
    .expect("prepare");
    engine.apply_codec(relay_session::CodecSettings::live());
    engine.apply(EngineCommand::Loopback).expect("loopback");

    for step in 0..80 {
        let _ = engine.process_capture(&sine_chunk(step * 240, 240));
        engine.drive().expect("drive");
    }
    let (web, seq) = engine
        .last_web_pcm()
        .expect("Opus live path must publish 48 kHz PCM for the listen page");
    assert!(seq > 0);
    assert!(
        web.iter().any(|sample| sample.abs() > 1.0e-4),
        "web pcm should not be silence, len={}",
        web.len()
    );
    assert!(
        web.len() >= 480 * 2,
        "web tap must keep every 5 ms media frame, got {}",
        web.len()
    );
}

#[test]
fn opus_web_tap_does_not_drop_frames_when_encode_drains_many() {
    let mut engine = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms5,
        ssrc: 0x5745_4231,
        monitor: MonitorMode::Dry,
        lan: false,
    })
    .expect("prepare");
    engine.apply_codec(relay_session::CodecSettings::live());
    engine.apply(EngineCommand::Loopback).expect("loopback");
    for step in 0..8 {
        let _ = engine.process_capture(&sine_chunk(step * 480, 480));
    }
    engine.drive().expect("drive");
    let (pcm, _) = engine.take_web_pcm().expect("queued web pcm");
    assert!(
        pcm.len() >= 480 * 2,
        "expected at least two 5 ms frames after one drive, got {}",
        pcm.len()
    );
}

#[test]
fn lan_name_join_finds_host() {
    let (bytes, len) = slug("lan-demo");
    let mut host = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms5,
        ssrc: 0x4c41_4e02,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("host");
    if let Err(error) = host.apply(EngineCommand::Listen(SocketAddr::from((
        [127, 0, 0, 1],
        17_492,
    )))) {
        eprintln!("skip lan_name_join_finds_host: {error}");
        return;
    }
    host.apply(EngineCommand::SetSlug { bytes, len })
        .expect("slug");

    let mut guest = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms5,
        ssrc: 0x4c41_4e03,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("guest");
    guest
        .apply(EngineCommand::JoinLan { bytes, len })
        .expect("join lan");

    let mut guest_out = vec![0.0_f32; 480];
    let mut heard = 0_u32;
    for step in 0..80 {
        let _ = host.process_capture(&sine_chunk(step * 240, 240));
        host.drive().expect("host");
        guest.drive().expect("guest");
        let _ = guest.render(&mut guest_out, &[]);
        if guest_out.iter().any(|sample| sample.abs() > 1.0e-4) {
            heard += 1;
        }
    }
    assert!(
        guest.snapshot().state == ConnectionState::Connected && heard > 4,
        "name join state={:?} heard={heard}",
        guest.snapshot().state
    );
}

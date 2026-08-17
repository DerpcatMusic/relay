//! One engine hears its own capture after a localhost UDP round-trip.

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

#[test]
fn loopback_hears_own_opus_round_trip() {
    let mut engine = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x1010_0001,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("prepare");
    engine.apply(EngineCommand::Loopback).expect("loopback");

    let mut out = vec![0.0_f32; 960];
    let mut heard = 0_u32;
    for step in 0..80 {
        let _ = engine.process_capture(&sine_chunk(step * 480, 480));
        engine.drive().expect("drive");
        let _ = engine.render(&mut out, &[]);
        if out.iter().any(|sample| sample.abs() > 1.0e-4) {
            heard += 1;
        }
    }
    assert_eq!(engine.snapshot().state, ConnectionState::Connected);
    assert!(heard > 4, "loopback heard {heard} blocks");
}

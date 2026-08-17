//! Local unpaid Stream hub fans one producer out to two listeners.

use std::net::SocketAddr;

use relay_audio::FrameDuration;
use relay_domain::SessionMode;
use relay_session::{EngineCommand, MonitorMode, SessionConfig, SessionEngine};

fn tone(start: usize, frames: usize) -> Vec<f32> {
    let mut samples = vec![0.0; frames * 2];
    for frame in 0..frames {
        let value = ((start + frame) as f32 * 0.02).sin() * 0.3;
        samples[frame * 2] = value;
        samples[frame * 2 + 1] = -value;
    }
    samples
}

#[test]
fn stream_hub_fans_producer_to_two_listeners() {
    let mut hub = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Stream,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x3333_0003,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("hub");
    let mut producer = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Stream,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x4444_0004,
        monitor: MonitorMode::Dry,
        lan: true,
    })
    .expect("producer");
    let mut a = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Stream,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x5555_0005,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("listener a");
    let mut b = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Stream,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x6666_0006,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("listener b");

    hub.apply(EngineCommand::HostStream(SocketAddr::from((
        [127, 0, 0, 1],
        0,
    ))))
    .expect("hub bind");
    let hub_addr = hub
        .snapshot()
        .local_port
        .map(|port| SocketAddr::from(([127, 0, 0, 1], port)))
        .expect("hub port");
    producer
        .apply(EngineCommand::PublishStream(hub_addr))
        .expect("publish");
    a.apply(EngineCommand::ListenStream(hub_addr))
        .expect("listen a");
    b.apply(EngineCommand::ListenStream(hub_addr))
        .expect("listen b");

    let mut a_out = vec![0.0_f32; 960];
    let mut b_out = vec![0.0_f32; 960];
    let mut a_heard = 0;
    let mut b_heard = 0;

    for step in 0..80 {
        let _ = producer.process_capture(&tone(step * 480, 480));
        producer.drive().expect("producer");
        hub.drive().expect("hub");
        a.drive().expect("a");
        b.drive().expect("b");
        let _ = a.render(&mut a_out, &[]);
        let _ = b.render(&mut b_out, &[]);
        if a_out.iter().any(|sample| sample.abs() > 1.0e-4) {
            a_heard += 1;
        }
        if b_out.iter().any(|sample| sample.abs() > 1.0e-4) {
            b_heard += 1;
        }
    }

    assert!(
        a_heard > 4 && b_heard > 4,
        "listener A heard {a_heard}, listener B heard {b_heard}"
    );
}

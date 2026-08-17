//! Two session engines exchange real Opus over localhost UDP.

use std::net::SocketAddr;

use relay_audio::FrameDuration;
use relay_domain::{ConnectionState, SessionMode};
use relay_session::{EngineCommand, MonitorMode, SessionConfig, SessionEngine};

fn sine_chunk(start: usize, frames: usize) -> Vec<f32> {
    let mut samples = vec![0.0; frames * 2];
    for frame in 0..frames {
        let phase = ((start + frame) as f32) * 440.0 * std::f32::consts::TAU / 48_000.0;
        let value = phase.sin() * 0.25;
        samples[frame * 2] = value;
        samples[frame * 2 + 1] = value * 0.5;
    }
    samples
}

#[test]
fn two_connect_peers_exchange_stereo_opus_on_localhost() {
    let mut host = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x1111_0001,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("host");
    let mut guest = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x2222_0002,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("guest");

    host.apply(EngineCommand::Listen(SocketAddr::from(([127, 0, 0, 1], 0))))
        .expect("listen");
    let host_addr = host
        .snapshot()
        .local_port
        .map(|port| SocketAddr::from(([127, 0, 0, 1], port)))
        .expect("bound");
    guest.apply(EngineCommand::Join(host_addr)).expect("join");

    let mut host_out = vec![0.0_f32; 960];
    let mut guest_out = vec![0.0_f32; 960];
    let mut guest_heard = 0_u32;
    let mut host_heard = 0_u32;

    for step in 0..80 {
        let host_pcm = sine_chunk(step * 480, 480);
        let guest_pcm = sine_chunk(step * 480 + 17, 480);
        let _ = host.process_capture(&host_pcm);
        let _ = guest.process_capture(&guest_pcm);
        host.drive().expect("host drive");
        guest.drive().expect("guest drive");
        let _ = host.render(&mut host_out, &[]);
        let _ = guest.render(&mut guest_out, &[]);
        if guest_out.iter().any(|sample| sample.abs() > 1.0e-4) {
            guest_heard += 1;
        }
        if host_out.iter().any(|sample| sample.abs() > 1.0e-4) {
            host_heard += 1;
        }
    }

    assert_eq!(host.snapshot().state, ConnectionState::Connected);
    assert_eq!(guest.snapshot().state, ConnectionState::Connected);
    assert!(
        guest_heard > 4 && host_heard > 4,
        "host heard {host_heard} remote blocks, guest heard {guest_heard}"
    );
}

#[test]
fn listen_only_guest_hears_host_capture() {
    let mut host = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x1111_0003,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("host");
    let mut guest = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x2222_0004,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("guest");
    host.apply(EngineCommand::Listen(SocketAddr::from(([127, 0, 0, 1], 0))))
        .expect("listen");
    let host_addr = host
        .snapshot()
        .local_port
        .map(|port| SocketAddr::from(([127, 0, 0, 1], port)))
        .expect("bound");
    guest.apply(EngineCommand::Join(host_addr)).expect("join");

    let mut guest_out = vec![0.0_f32; 960];
    let mut heard = 0_u32;
    for step in 0..80 {
        let _ = host.process_capture(&sine_chunk(step * 480, 480));
        host.drive().expect("host");
        guest.drive().expect("guest");
        let _ = guest.render(&mut guest_out, &[]);
        if guest_out.iter().any(|sample| sample.abs() > 1.0e-4) {
            heard += 1;
        }
    }
    assert!(heard > 4, "listen-only guest heard {heard}");
}

/// Web Link / late CLAP join: host has already advanced its RTP clock.
#[test]
fn late_join_guest_hears_live_host() {
    let mut host = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x1111_0005,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("host");
    host.apply(EngineCommand::Listen(SocketAddr::from(([127, 0, 0, 1], 0))))
        .expect("listen");
    let host_addr = host
        .snapshot()
        .local_port
        .map(|port| SocketAddr::from(([127, 0, 0, 1], port)))
        .expect("bound");

    for step in 0..40 {
        let _ = host.process_capture(&sine_chunk(step * 480, 480));
        host.drive().expect("host warmup");
    }

    let mut guest = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0x2222_0006,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("guest");
    guest.apply(EngineCommand::Join(host_addr)).expect("join");

    let mut guest_out = vec![0.0_f32; 960];
    let mut heard = 0_u32;
    for step in 40..120 {
        let _ = host.process_capture(&sine_chunk(step * 480, 480));
        host.drive().expect("host");
        guest.drive().expect("guest");
        let _ = guest.render(&mut guest_out, &[]);
        if guest_out.iter().any(|sample| sample.abs() > 1.0e-4) {
            heard += 1;
        }
    }
    assert_eq!(guest.snapshot().state, ConnectionState::Connected);
    assert!(heard > 4, "late join guest heard {heard}");
}

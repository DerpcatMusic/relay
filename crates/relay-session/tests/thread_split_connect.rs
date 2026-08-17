//! Plugin threading model: callback rings plus a worker thread.

use std::thread;
use std::time::{Duration, Instant};

use relay_audio::FrameDuration;
use relay_domain::ConnectionState;
use relay_session::{
    MonitorMode, SessionConfig, SessionEngine, SessionMode, SessionRole, SessionRuntime,
};

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

fn wait_port(runtime: &SessionRuntime) -> u16 {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(port) = runtime.snapshot().local_port {
            return port;
        }
        assert!(
            Instant::now() < deadline,
            "worker did not publish a bind port"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn split_runtime_exchanges_stereo_on_localhost() {
    let host = SessionRuntime::start(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0xaaaa_0001,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("host runtime");
    let guest = SessionRuntime::start(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0xbbbb_0002,
        monitor: MonitorMode::Remote,
        lan: true,
    })
    .expect("guest runtime");

    host.control().set_role(SessionRole::ConnectListen);
    host.control().set_bind_port(0);
    host.control().set_linked(true);
    let port = wait_port(&host);
    guest.control().set_role(SessionRole::ConnectJoin);
    guest
        .control()
        .set_peer(format!("127.0.0.1:{port}"))
        .expect("peer");
    guest.control().set_linked(true);

    let mut host_rt = host;
    let mut guest_rt = guest;
    let mut host_out = vec![0.0_f32; 960];
    let mut guest_out = vec![0.0_f32; 960];
    let mut host_heard = 0_u32;
    let mut guest_heard = 0_u32;

    for step in 0..80 {
        let _ = host_rt.process_capture(&sine_chunk(step * 480, 480));
        let _ = guest_rt.process_capture(&sine_chunk(step * 480 + 17, 480));
        thread::sleep(Duration::from_millis(8));
        let _ = host_rt.render(&mut host_out, &[]);
        let _ = guest_rt.render(&mut guest_out, &[]);
        if guest_out.iter().any(|sample| sample.abs() > 1.0e-4) {
            guest_heard += 1;
        }
        if host_out.iter().any(|sample| sample.abs() > 1.0e-4) {
            host_heard += 1;
        }
    }

    assert_eq!(host_rt.snapshot().state, ConnectionState::Connected);
    assert_eq!(guest_rt.snapshot().state, ConnectionState::Connected);
    assert!(
        guest_heard > 4 && host_heard > 4,
        "host heard {host_heard}, guest heard {guest_heard}"
    );
}

#[test]
fn into_parts_keeps_callback_independent_of_drive() {
    let engine = SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Connect,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: 0xcccc_0003,
        monitor: MonitorMode::Dry,
        lan: true,
    })
    .expect("prepare");
    let (mut face, mut worker) = engine.into_parts();
    let pcm = sine_chunk(0, 480);
    let _ = face.process_capture(&pcm);
    let mut out = vec![0.0_f32; 960];
    let _ = face.render(&mut out, &pcm);
    assert!(out.iter().any(|sample| sample.abs() > 1.0e-4));
    let _ = worker.drive();
}

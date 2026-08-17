//! Local Stream without billing: hub, publish, or listen.

use std::env;
use std::net::{SocketAddr, ToSocketAddrs};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use relay_audio::FrameDuration;
use relay_session::{
    DEFAULT_STREAM_PORT, EngineCommand, MonitorMode, SessionConfig, SessionEngine, SessionMode,
};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("usage: relay-stream hub [port] | publish <hub> | listen <hub>");
        return ExitCode::from(2);
    };
    let mut engine = match SessionEngine::prepare(SessionConfig {
        mode: SessionMode::Stream,
        device_rate_hz: 48_000,
        frame_duration: FrameDuration::Ms20,
        ssrc: std::process::id() ^ 0x5354_524d,
        monitor: MonitorMode::Remote,
        lan: true,
    }) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("prepare failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = match command.as_str() {
        "hub" => {
            let port = args
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_STREAM_PORT);
            engine.apply(EngineCommand::HostStream(SocketAddr::from((
                [0, 0, 0, 0],
                port,
            ))))
        }
        "publish" => match args.next().and_then(|value| parse_addr(&value)) {
            Some(hub) => engine.apply(EngineCommand::PublishStream(hub)),
            None => {
                eprintln!("publish requires hub host:port");
                return ExitCode::from(2);
            }
        },
        "listen" => match args.next().and_then(|value| parse_addr(&value)) {
            Some(hub) => engine.apply(EngineCommand::ListenStream(hub)),
            None => {
                eprintln!("listen requires hub host:port");
                return ExitCode::from(2);
            }
        },
        other => {
            eprintln!("unknown command {other}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = result {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    println!("relay-stream {:?}", engine.snapshot());
    let started = Instant::now();
    let mut block = 0_usize;
    let mut output = vec![0.0_f32; 960];
    while started.elapsed() < Duration::from_secs(3600) {
        if command == "publish" {
            let _ = engine.process_capture(&demo_tone(block, 480));
        }
        if let Err(error) = engine.drive() {
            eprintln!("drive: {error}");
            return ExitCode::FAILURE;
        }
        let _ = engine.render(&mut output, &[]);
        if block.is_multiple_of(50) {
            println!("t={}s {:?}", started.elapsed().as_secs(), engine.snapshot());
        }
        block += 1;
        thread::sleep(Duration::from_millis(20));
    }
    ExitCode::SUCCESS
}

fn parse_addr(value: &str) -> Option<SocketAddr> {
    value.to_socket_addrs().ok()?.next()
}

fn demo_tone(block: usize, frames: usize) -> Vec<f32> {
    let mut samples = vec![0.0; frames * 2];
    for frame in 0..frames {
        let phase = ((block * frames + frame) as f32) * 330.0 * std::f32::consts::TAU / 48_000.0;
        let value = phase.sin() * 0.2;
        samples[frame * 2] = value;
        samples[frame * 2 + 1] = value;
    }
    samples
}

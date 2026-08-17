//! Truce adapter for RELAY Connect and unpaid local Stream.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

use relay_audio::FrameDuration;
mod editor;
mod fanout;
mod local_listen;

use relay_session::{
    DEFAULT_CONNECT_PORT, MonitorMode, SessionConfig, SessionControl, SessionRole, SessionRuntime,
    WireCodec, normalize_slug,
};
use truce::prelude::*;
use truce_egui::EguiEditor;
use truce_font::JETBRAINS_MONO;

use editor::{RelayUi, buffr_visuals};

use RelayParamsParamId as P;

pub(crate) const WINDOW_W: u32 = 428;
pub(crate) const WINDOW_H: u32 = 420;
pub(crate) const MIN_WINDOW_W: u32 = 380;
pub(crate) const MIN_WINDOW_H: u32 = 320;
pub(crate) const MAX_WINDOW_W: u32 = 560;
pub(crate) const MAX_WINDOW_H: u32 = 720;
pub(crate) const METER_FLOOR_DB: f32 = -60.0;
const MAX_INTERLEAVED: usize = 16_384;

static NEXT_SSRC: AtomicU32 = AtomicU32::new(0x5245_0001);

#[derive(ParamEnum)]
pub enum Product {
    #[name = "Link"]
    Link,
    #[name = "Connect"]
    Connect,
}

impl Product {
    const fn role(self) -> SessionRole {
        match self {
            Self::Link => SessionRole::ConnectListen,
            Self::Connect => SessionRole::ConnectJoin,
        }
    }

    const fn is_link(self) -> bool {
        matches!(self, Self::Link)
    }
}

#[derive(ParamEnum)]
pub enum Monitor {
    Dry,
    Mix,
    Remote,
}

impl Monitor {
    const fn mode(self) -> MonitorMode {
        match self {
            Self::Mix => MonitorMode::Mix,
            Self::Remote => MonitorMode::Remote,
            Self::Dry => MonitorMode::Dry,
        }
    }
}

#[derive(ParamEnum)]
pub enum Codec {
    #[name = "Opus · 192 kbps"]
    Opus,
    #[name = "FLAC · 16-bit"]
    Flac,
    #[name = "PCM · 16-bit LAN"]
    Pcm,
}

impl Codec {
    const fn wire(self) -> WireCodec {
        match self {
            Self::Opus => WireCodec::Opus,
            Self::Flac => WireCodec::Flac,
            Self::Pcm => WireCodec::Pcm,
        }
    }
}

#[derive(State, Clone)]
pub struct SessionPersist {
    /// `host:port` for Connect.
    pub peer: String,
    /// Shareable slug for `/<name>`.
    pub name: String,
    /// Optional room password. Empty means anyone with the link can listen.
    pub password: String,
}

impl Default for SessionPersist {
    fn default() -> Self {
        Self {
            peer: format!("127.0.0.1:{DEFAULT_CONNECT_PORT}"),
            name: new_slug(),
            password: String::new(),
        }
    }
}

pub(crate) fn new_slug() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(1);
    format!("room-{:x}", (nanos % 0x0000_ffff_ffff) as u32)
}

#[derive(Params)]
pub struct RelayParams {
    #[param(name = "Product")]
    pub product: EnumParam<Product>,

    #[param(name = "Monitor")]
    pub monitor: EnumParam<Monitor>,

    #[param(name = "Codec")]
    pub codec: EnumParam<Codec>,

    #[param(
        name = "Bitrate",
        range = "discrete(64, 256)",
        default = 192,
        unit = "custom:kbps"
    )]
    pub bitrate: IntParam,

    #[param(name = "Comp", range = "discrete(0, 8)", default = 5)]
    pub flac_level: IntParam,

    #[param(name = "Live", default = 1)]
    pub link: BoolParam,

    #[param(name = "Web", default = 0)]
    pub web: BoolParam,

    #[param(name = "Port", range = "discrete(1, 65535)", default = 17_492)]
    pub port: IntParam,

    #[param(
        name = "Send",
        range = "linear(-24, 12)",
        unit = "dB",
        default = 0.0,
        smooth = "exp(5)"
    )]
    pub input_gain: FloatParam,

    #[param(
        name = "Hear",
        range = "linear(-24, 12)",
        unit = "dB",
        default = 0.0,
        smooth = "exp(5)"
    )]
    pub output_gain: FloatParam,

    #[meter]
    pub meter_left: MeterSlot,

    #[meter]
    pub meter_right: MeterSlot,

    #[persist = "session"]
    pub session: RwLock<SessionPersist>,

    #[skip]
    pub control: Arc<SessionControl>,
}

/// Stateless descriptor. DSP / session live in [`RelayDsp`].
pub struct RelayPlugin;

/// Audio-thread face plus preallocated interleaved staging.
pub struct RelayDsp {
    runtime: Option<SessionRuntime>,
    fanout: Option<std::thread::JoinHandle<()>>,
    fanout_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    interleaved: Vec<f32>,
    dry: Vec<f32>,
    output: Vec<f32>,
}

impl Default for RelayDsp {
    fn default() -> Self {
        Self {
            runtime: None,
            fanout: None,
            fanout_stop: None,
            interleaved: Vec::new(),
            dry: Vec::new(),
            output: Vec::new(),
        }
    }
}

impl PluginLogic for RelayPlugin {
    type Params = RelayParams;
    type DspState = RelayDsp;

    const PRESERVE_DSP_STATE: bool = false;

    fn bus_layouts() -> Vec<BusLayout> {
        BusLayout::stereo_and_mono()
    }

    fn reset(state: &mut RelayDsp, params: &RelayParams, config: &AudioConfig) {
        if params.product.value().is_link() {
            params.link.set_value(true);
        }
        publish_control(params);
        let samples = config
            .max_block_size
            .saturating_mul(2)
            .max(2)
            .min(MAX_INTERLEAVED);
        state.interleaved.resize(samples, 0.0);
        state.dry.resize(samples, 0.0);
        state.output.resize(samples, 0.0);
        state.runtime = None;
        if let Some(stop) = state.fanout_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
        state.fanout = None;
        let rate = config.sample_rate.round().clamp(8_000.0, 192_000.0) as usize;
        params.control.set_device_rate_hz(rate as u32);
        params
            .control
            .set_block_frames(config.max_block_size.min(u32::MAX as usize) as u32);
        let prepared = SessionRuntime::start_with(
            SessionConfig {
                mode: params.product.value().role().session_mode(),
                device_rate_hz: rate,
                frame_duration: frame_from_host(rate, config.max_block_size),
                lan: params.codec.value().wire().is_pcm(),
                ssrc: NEXT_SSRC.fetch_add(1, Ordering::Relaxed),
                monitor: params.monitor.value().mode(),
            },
            Arc::clone(&params.control),
        );
        if let Ok(runtime) = prepared {
            state.runtime = Some(runtime);
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            state.fanout_stop = Some(Arc::clone(&stop));
            state.fanout = Some(fanout::spawn(Arc::clone(&params.control), stop));
        }
    }

    fn latency(_state: &RelayDsp) -> u32 {
        0
    }

    fn process(
        state: &mut RelayDsp,
        params: &RelayParams,
        buffer: &mut AudioBuffer,
        _events: &EventList,
        context: &mut ProcessContext,
    ) -> ProcessStatus {
        params.control.set_linked(params.link.value());
        params.control.set_web_wanted(params.web.value());
        params.control.set_role(params.product.value().role());
        params.control.set_codec(params.codec.value().wire());
        params.bitrate.set_value(192);
        params.control.set_bitrate_kbps(192);
        params
            .control
            .set_flac_level(params.flac_level.value() as u8);
        let port = params.port.value().clamp(1, 65_535) as u16;
        params.control.set_bind_port(port);
        params.control.set_block_frames(buffer.num_samples() as u32);

        let frames = buffer.num_samples();
        let needed = frames.saturating_mul(2);
        if needed == 0 || needed > state.interleaved.len() {
            return ProcessStatus::Normal;
        }

        copy_inputs(buffer, frames, &mut state.interleaved[..needed]);
        state.dry[..needed].copy_from_slice(&state.interleaved[..needed]);

        let input_gain = db_to_linear(params.input_gain.read());
        for sample in &mut state.interleaved[..needed] {
            *sample *= input_gain;
        }

        let linking = params.product.value().is_link();
        let monitor = if linking {
            MonitorMode::Dry
        } else {
            MonitorMode::Remote
        };
        if let Some(runtime) = state.runtime.as_mut() {
            runtime.set_monitor(monitor);
            let _ = runtime.process_capture(&state.interleaved[..needed]);
            let _ = runtime.render(&mut state.output[..needed], &state.dry[..needed]);
        } else {
            state.output[..needed].copy_from_slice(&state.dry[..needed]);
        }

        let hear = db_to_linear(params.output_gain.read());
        let mixed = if linking {
            &mut state.dry[..needed]
        } else {
            &mut state.output[..needed]
        };
        for sample in mixed.iter_mut() {
            *sample *= hear;
        }
        write_outputs(buffer, frames, mixed);

        let (send_l, send_r) = stereo_peaks(&state.interleaved[..needed]);
        context.set_meter(P::MeterLeft, send_l);
        context.set_meter(P::MeterRight, send_r);
        ProcessStatus::Normal
    }

    fn editor(params: Arc<RelayParams>) -> Box<dyn Editor> {
        Box::new(
            EguiEditor::with_ui(params, (WINDOW_W, WINDOW_H), RelayUi::new(WINDOW_H))
                .resizable(true)
                .min_size((MIN_WINDOW_W, MIN_WINDOW_H))
                .max_size((MAX_WINDOW_W, MAX_WINDOW_H))
                .with_visuals(buffr_visuals())
                .with_font(JETBRAINS_MONO),
        )
    }
}

fn stereo_peaks(interleaved: &[f32]) -> (f32, f32) {
    let mut left = 0.0f32;
    let mut right = 0.0f32;
    for pair in interleaved.chunks_exact(2) {
        left = left.max(pair[0].abs());
        right = right.max(pair[1].abs());
    }
    (left, right)
}

#[cfg(test)]
fn peak_to_db(peak: f32) -> f32 {
    if !peak.is_finite() || peak <= 1.0e-6 {
        return METER_FLOOR_DB;
    }
    (20.0 * peak.log10()).clamp(METER_FLOOR_DB, 6.0)
}

fn frame_from_host(rate_hz: usize, max_block: usize) -> FrameDuration {
    let ms = max_block.saturating_mul(1_000) / rate_hz.max(1);
    if ms <= 7 {
        FrameDuration::Ms5
    } else if ms <= 15 {
        FrameDuration::Ms10
    } else {
        FrameDuration::Ms20
    }
}

#[cfg(test)]
fn db_to_pos(db: f32) -> f32 {
    ((db.clamp(METER_FLOOR_DB, 0.0) - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0)
}

pub(crate) fn publish_control(params: &RelayParams) {
    params.control.set_linked(params.link.value());
    params.control.set_role(params.product.value().role());
    params.control.set_codec(params.codec.value().wire());
    params.bitrate.set_value(192);
    params.control.set_bitrate_kbps(192);
    params
        .control
        .set_flac_level(params.flac_level.value() as u8);
    let port = params.port.value().clamp(1, 65_535) as u16;
    params.control.set_bind_port(port);
    if let Ok(session) = params.session.read() {
        let peer = if session.peer.trim().is_empty() {
            format!("127.0.0.1:{DEFAULT_CONNECT_PORT}")
        } else {
            session.peer.clone()
        };
        let _ = params.control.set_peer(peer);
        let name = if session.name.trim().is_empty() {
            new_slug()
        } else {
            normalize_slug(&session.name)
        };
        let _ = params.control.set_session_name(name);
        let _ = params.control.set_password(session.password.clone());
    }
}

fn copy_inputs(buffer: &AudioBuffer, frames: usize, interleaved: &mut [f32]) {
    let inputs = buffer.num_input_channels();
    let left = if inputs >= 1 { buffer.input(0) } else { &[] };
    let right = if inputs >= 2 { buffer.input(1) } else { left };
    for frame in 0..frames {
        let l = left.get(frame).copied().unwrap_or(0.0);
        let r = right.get(frame).copied().unwrap_or(l);
        interleaved[frame * 2] = l;
        interleaved[frame * 2 + 1] = r;
    }
}

fn write_outputs(buffer: &mut AudioBuffer, frames: usize, interleaved: &[f32]) {
    let outputs = buffer.num_output_channels();
    if outputs >= 1 {
        let left = buffer.output(0);
        for frame in 0..frames.min(left.len()) {
            left[frame] = interleaved[frame * 2];
        }
    }
    if outputs >= 2 {
        let right = buffer.output(1);
        for frame in 0..frames.min(right.len()) {
            right[frame] = interleaved[frame * 2 + 1];
        }
    }
}

truce::plugin! {
    logic: RelayPlugin,
    params: RelayParams,
}

truce::enable_rt_paranoid!();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_is_valid() {
        truce_test::assert_valid_info::<Plugin>();
    }

    #[test]
    fn bus_config_effect() {
        truce_test::assert_bus_config_effect::<Plugin>();
    }

    #[test]
    fn has_editor() {
        truce_test::assert_has_editor::<Plugin>();
    }

    #[test]
    fn dry_unlinked_passthrough() {
        use std::time::Duration;
        use truce_test::{InputSource, assertions, driver};

        let result = driver!(Plugin)
            .duration(Duration::from_millis(80))
            .input(InputSource::Constant(0.25))
            .run();
        assertions::assert_no_nans(&result);
        assertions::assert_nonzero(&result);
        assertions::assert_peak_below(&result, 1.0);
    }

    #[test]
    fn process_is_allocation_free() {
        use std::time::Duration;
        use truce_test::{InputSource, assert_no_audio_alloc, driver};
        assert_no_audio_alloc(|| {
            driver!(Plugin)
                .duration(Duration::from_millis(40))
                .input(InputSource::Constant(0.25))
                .run()
        });
    }

    #[test]
    fn state_round_trips() {
        truce_test::assert_state_round_trip::<Plugin>();
    }

    #[test]
    fn peer_persist_round_trips() {
        let params = RelayParams::new();
        params.session.write().expect("lock").peer = "127.0.0.1:17492".into();
        let blob = params.serialize_persist();
        let fresh = RelayParams::new();
        fresh.load_persist(&blob);
        assert_eq!(fresh.session.read().expect("lock").peer, "127.0.0.1:17492");
    }

    #[test]
    fn session_name_persist_round_trips() {
        let params = RelayParams::new();
        params.session.write().expect("lock").name = "late-night-mix".into();
        let blob = params.serialize_persist();
        let fresh = RelayParams::new();
        fresh.load_persist(&blob);
        assert_eq!(fresh.session.read().expect("lock").name, "late-night-mix");
    }

    #[test]
    fn password_persist_round_trips() {
        let params = RelayParams::new();
        params.session.write().expect("lock").password = "mix-secret".into();
        let blob = params.serialize_persist();
        let fresh = RelayParams::new();
        fresh.load_persist(&blob);
        assert_eq!(fresh.session.read().expect("lock").password, "mix-secret");
    }

    #[test]
    fn defaults_to_live_link() {
        let params = RelayParams::new();
        assert!(params.product.value().is_link());
        assert!(params.link.value());
        assert!(matches!(params.codec.value(), Codec::Opus));
        assert_eq!(params.bitrate.value(), 192);
        assert!(!params.session.read().expect("lock").name.is_empty());
    }

    #[test]
    fn meter_scale_is_log_dbfs() {
        assert!((peak_to_db(1.0) - 0.0).abs() < 0.01);
        assert!((peak_to_db(0.5) + 6.02).abs() < 0.05);
        assert_eq!(peak_to_db(0.0), METER_FLOOR_DB);
        assert!(db_to_pos(-6.0) > db_to_pos(-18.0));
        assert!(db_to_pos(-18.0) > 0.5);
        assert!((db_to_pos(0.0) - 1.0).abs() < f32::EPSILON);
        assert!((db_to_pos(-60.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn stereo_peaks_read_both_channels() {
        let (l, r) = stereo_peaks(&[0.1, 0.8, 0.2, 0.4]);
        assert!((l - 0.2).abs() < 1e-6);
        assert!((r - 0.8).abs() < 1e-6);
    }

    #[test]
    fn frame_from_host_follows_block_period() {
        assert_eq!(frame_from_host(48_000, 64), FrameDuration::Ms5);
        assert_eq!(frame_from_host(48_000, 512), FrameDuration::Ms10);
        assert_eq!(frame_from_host(48_000, 1024), FrameDuration::Ms20);
        assert_eq!(frame_from_host(44_100, 128), FrameDuration::Ms5);
    }
}

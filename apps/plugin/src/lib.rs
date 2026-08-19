//! Truce adapter for RELAY Connect and unpaid local Stream.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

use relay_audio::FrameDuration;
mod editor;
mod fanout;
mod local_listen;
mod p2p;

use relay_session::{
    DEFAULT_CONNECT_PORT, MonitorMode, SessionConfig, SessionControl, SessionRole, SessionRuntime,
    WireCodec, normalize_slug,
};
use truce::prelude::*;
use truce_egui::EguiEditor;

use editor::{RelayUi, buffr_visuals, install_chrome};

use RelayParamsParamId as P;

pub(crate) const WINDOW_W: u32 = 440;
pub(crate) const WINDOW_H: u32 = 400;
pub(crate) const MIN_WINDOW_W: u32 = 400;
pub(crate) const MIN_WINDOW_H: u32 = 380;
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
    #[name = "Opus"]
    Opus,
    #[name = "FLAC"]
    Flac,
    #[name = "PCM · LAN"]
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

const SLUG_ADJECTIVES: &[&str] = &[
    "big", "filthy", "quiet", "late", "warm", "cold", "loud", "soft", "bright", "dark", "wild",
    "rusty", "dusty", "sweet", "heavy", "light", "sharp", "empty", "slow", "fast", "deep", "thin",
    "wide", "tiny", "pale", "dry", "calm", "raw", "odd", "bold", "hot", "cool", "flat", "polar",
    "lunar", "storm", "still", "vivid", "mute", "gold",
];

const SLUG_NOUNS: &[&str] = &[
    "papaya", "mango", "cedar", "maple", "river", "stone", "fox", "wolf", "moth", "ember", "comet",
    "harbor", "attic", "kettle", "drum", "piano", "socket", "buffer", "fader", "meter", "booth",
    "desk", "lamp", "tape", "reel", "stem", "gate", "plate", "spring", "orchid", "canyon",
    "glacier", "meadow", "velvet", "copper", "quartz",
];

pub(crate) fn new_slug() -> String {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(1)
        ^ u64::from(std::process::id()).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let n_adj = SLUG_ADJECTIVES.len() as u64;
    let n_noun = SLUG_NOUNS.len() as u64;
    let a = (z % n_adj) as usize;
    let mut b = ((z / n_adj) % n_adj) as usize;
    let c = ((z / n_adj / n_adj) % n_noun) as usize;
    if b == a {
        b = (b + 1) % SLUG_ADJECTIVES.len();
    }
    format!(
        "{}-{}-{}",
        SLUG_ADJECTIVES[a], SLUG_ADJECTIVES[b], SLUG_NOUNS[c]
    )
}

#[derive(Params)]
pub struct RelayParams {
    #[param(name = "Product")]
    pub product: EnumParam<Product>,

    #[param(name = "Monitor", default = 1)]
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

    #[param(name = "Web", default = 1)]
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
    hear_latency: u32,
    device_rate: u32,
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
            hear_latency: 0,
            device_rate: 0,
        }
    }
}

impl PluginLogic for RelayPlugin {
    type Params = RelayParams;
    type DspState = RelayDsp;

    const PRESERVE_DSP_STATE: bool = true;

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
        if state.interleaved.len() < samples {
            state.interleaved.resize(samples, 0.0);
            state.dry.resize(samples, 0.0);
            state.output.resize(samples, 0.0);
        }
        let rate = config.sample_rate.round().clamp(8_000.0, 192_000.0) as usize;
        params.control.set_device_rate_hz(rate as u32);
        params
            .control
            .set_block_frames(config.max_block_size.min(u32::MAX as usize) as u32);
        if state.runtime.is_some() && state.device_rate == rate as u32 {
            ensure_fanout(state, params);
            return;
        }
        state.runtime = None;
        if let Some(stop) = state.fanout_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
        state.fanout = None;
        state.device_rate = rate as u32;
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
            params.control.clear_last_error();
            state.hear_latency = if params.product.value().is_link() {
                0
            } else {
                runtime.playback_target_frames()
            };
            state.runtime = Some(runtime);
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            state.fanout_stop = Some(Arc::clone(&stop));
            state.fanout = Some(fanout::spawn(Arc::clone(&params.control), stop));
        } else {
            state.hear_latency = 0;
            params
                .control
                .set_last_error("session engine failed to start");
        }
    }

    fn latency(state: &RelayDsp) -> u32 {
        state.hear_latency
    }

    fn process(
        state: &mut RelayDsp,
        params: &RelayParams,
        buffer: &mut AudioBuffer,
        _events: &EventList,
        context: &mut ProcessContext,
    ) -> ProcessStatus {
        params.control.set_linked(params.link.value());
        params.control.set_web_wanted(true);
        if let Ok(session) = params.session.read() {
            let name = normalize_slug(&session.name);
            if !name.is_empty() {
                let _ = params.control.set_session_name(name);
            }
        }
        params.control.set_role(params.product.value().role());
        params.control.set_codec(params.codec.value().wire());
        params
            .control
            .set_bitrate_kbps(params.bitrate.value().clamp(64, 256) as u32);
        params
            .control
            .set_flac_level(params.flac_level.value() as u8);
        let port = params.port.value().clamp(1, 65_535) as u16;
        params.control.set_bind_port(port);
        params.control.set_block_frames(buffer.num_samples() as u32);

        let frames = buffer.num_samples();
        let needed = frames.saturating_mul(2);
        if needed == 0 {
            return ProcessStatus::Normal;
        }
        if needed > state.interleaved.len() {
            host_passthrough(buffer, frames);
            return ProcessStatus::Normal;
        }

        copy_inputs(buffer, frames, &mut state.interleaved[..needed]);
        state.dry[..needed].copy_from_slice(&state.interleaved[..needed]);

        let input_gain = db_to_linear(params.input_gain.read_after(frames));
        for sample in &mut state.interleaved[..needed] {
            *sample *= input_gain;
        }

        let linking = params.product.value().is_link();
        let rendered = if let Some(runtime) = state.runtime.as_mut() {
            runtime.set_monitor(MonitorMode::Remote);
            let _ = runtime.process_capture(&state.interleaved[..needed]);
            runtime
                .render(&mut state.output[..needed], &state.dry[..needed])
                .rendered_samples
        } else {
            state.output[..needed].fill(0.0);
            0
        };

        if linking {
            write_outputs(buffer, frames, &state.dry[..needed]);
        } else {
            let hear = db_to_linear(params.output_gain.read_after(frames));
            for sample in &mut state.output[..needed] {
                *sample *= hear;
            }
            match params.monitor.value() {
                Monitor::Dry => write_outputs(buffer, frames, &state.dry[..needed]),
                Monitor::Remote => {
                    if rendered < needed {
                        splice_dry(&mut state.output[..needed], &state.dry[..needed], rendered);
                    }
                    write_outputs(buffer, frames, &state.output[..needed]);
                }
                Monitor::Mix => {
                    mix_into(&mut state.output[..needed], &state.dry[..needed]);
                    write_outputs(buffer, frames, &state.output[..needed]);
                }
            }
        }

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
                .with_context_setup(install_chrome)
                .with_visuals(buffr_visuals()),
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

fn ensure_fanout(state: &mut RelayDsp, params: &RelayParams) {
    let dead = state
        .fanout
        .as_ref()
        .is_none_or(|handle| handle.is_finished());
    if !dead {
        return;
    }
    if let Some(stop) = state.fanout_stop.take() {
        stop.store(true, std::sync::atomic::Ordering::Release);
    }
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    state.fanout_stop = Some(Arc::clone(&stop));
    state.fanout = Some(fanout::spawn(Arc::clone(&params.control), stop));
}

pub(crate) fn publish_control(params: &RelayParams) {
    params.control.set_linked(params.link.value());
    params.web.set_value(true);
    params.control.set_web_wanted(true);
    params.control.set_role(params.product.value().role());
    params.control.set_codec(params.codec.value().wire());
    params
        .control
        .set_bitrate_kbps(params.bitrate.value().clamp(64, 256) as u32);
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

fn mix_into(dst: &mut [f32], dry: &[f32]) {
    for (sample, dry_sample) in dst.iter_mut().zip(dry) {
        *sample += *dry_sample;
    }
}

/// Fill a Remote-only hole with dry so an underrun is the local track, not silence.
fn splice_dry(out: &mut [f32], dry: &[f32], rendered: usize) {
    let rendered = rendered.min(out.len()).min(dry.len()) & !1;
    if rendered == 0 {
        let fade = 64.min(out.len() / 2);
        for i in 0..fade {
            let t = (i + 1) as f32 / fade as f32;
            let g = 0.5 - 0.5 * (core::f32::consts::PI * t).cos();
            let o = i * 2;
            if o + 1 >= out.len() {
                break;
            }
            out[o] = dry[o] * g;
            out[o + 1] = dry[o + 1] * g;
        }
        if fade * 2 < out.len() {
            out[fade * 2..].copy_from_slice(&dry[fade * 2..]);
        }
        return;
    }
    if rendered >= out.len() {
        return;
    }
    let fade_frames = 32.min(rendered / 2).min((out.len() - rendered) / 2).max(1);
    let fade_start = rendered.saturating_sub(fade_frames * 2);
    for i in 0..fade_frames {
        let t = (i + 1) as f32 / fade_frames as f32;
        let g = 0.5 - 0.5 * (core::f32::consts::PI * t).cos();
        let o = fade_start + i * 2;
        out[o] = out[o] * (1.0 - g) + dry[o] * g;
        out[o + 1] = out[o + 1] * (1.0 - g) + dry[o + 1] * g;
    }
    out[rendered..].copy_from_slice(&dry[rendered..]);
}

fn host_passthrough(buffer: &mut AudioBuffer, frames: usize) {
    let inputs = buffer.num_input_channels();
    let outputs = buffer.num_output_channels();
    if outputs == 0 {
        return;
    }
    if inputs == 0 {
        for channel in 0..outputs {
            let out = buffer.output(channel);
            let n = frames.min(out.len());
            out[..n].fill(0.0);
        }
        return;
    }
    for channel in 0..outputs {
        let in_ch = channel.min(inputs - 1);
        if buffer.is_in_place(channel) && in_ch == channel {
            continue;
        }
        let (input, output) = buffer.io_pair(in_ch, channel);
        let n = frames.min(input.len()).min(output.len());
        output[..n].copy_from_slice(&input[..n]);
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
    fn link_passthrough_ignores_send_and_hear() {
        use std::time::Duration;
        use truce_test::{InputSource, assertions, driver};

        let result = driver!(Plugin)
            .set_param(P::InputGain, 1.0)
            .set_param(P::OutputGain, 1.0)
            .duration(Duration::from_millis(80))
            .input(InputSource::Constant(0.25))
            .run();
        assertions::assert_no_nans(&result);
        assertions::assert_nonzero(&result);
        assertions::assert_peak_below(&result, 0.26);
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

    /// LV2 wrapper glue must stay allocation-free on the audio thread.
    #[cfg(feature = "lv2")]
    #[test]
    fn lv2_wrapper_glue_is_allocation_free() {
        assert_eq!(
            truce_lv2::rt_paranoid_smoke::<Plugin>(),
            0,
            "the LV2 wrapper's per-block glue must not allocate on the audio thread"
        );
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
        assert!(
            params.web.value(),
            "Share always offers the listen page; idle with nobody connected is a no-op"
        );
        assert!(matches!(params.codec.value(), Codec::Opus));
        assert_eq!(params.bitrate.value(), 192);
        assert!(matches!(params.monitor.value(), Monitor::Mix));
        assert!(!params.session.read().expect("lock").name.is_empty());
    }

    #[test]
    fn publish_control_keeps_bitrate() {
        let params = RelayParams::new();
        params.bitrate.set_value(96);
        publish_control(&params);
        assert_eq!(params.bitrate.value(), 96);
        assert_eq!(params.control.bitrate_kbps(), 96);
    }

    #[test]
    fn connect_mix_keeps_dry_when_remote_is_silent() {
        use std::time::Duration;
        use truce_test::{InputSource, assertions, driver};

        let result = driver!(Plugin)
            .set_param(P::Product, 1.0)
            .duration(Duration::from_millis(80))
            .input(InputSource::Constant(0.25))
            .run();
        assertions::assert_no_nans(&result);
        assertions::assert_nonzero(&result);
        assertions::assert_peak_below(&result, 0.26);
    }

    #[test]
    fn connect_remote_underrun_falls_back_to_dry() {
        use std::time::Duration;
        use truce_test::{InputSource, assertions, driver};

        let result = driver!(Plugin)
            .set_param(P::Product, 1.0)
            .set_param(P::Monitor, 2.0)
            .duration(Duration::from_millis(80))
            .input(InputSource::Constant(0.25))
            .run();
        assertions::assert_no_nans(&result);
        assertions::assert_nonzero(&result);
        assertions::assert_peak_below(&result, 0.26);
    }

    #[test]
    fn splice_dry_fills_a_silent_suffix() {
        let mut out = vec![0.8; 192];
        for sample in &mut out[128..] {
            *sample = 0.0;
        }
        let dry = vec![0.1; 192];
        splice_dry(&mut out, &dry, 128);
        assert!((out[0] - 0.8).abs() < 1e-5);
        assert!((out[190] - 0.1).abs() < 1e-5);
        assert!((out[191] - 0.1).abs() < 1e-5);
    }

    #[test]
    fn splice_dry_empty_remote_becomes_dry() {
        let mut out = [0.0; 8];
        let dry = [0.2; 8];
        splice_dry(&mut out, &dry, 0);
        assert!(out.iter().any(|s| *s > 0.05));
        assert!((out[6] - 0.2).abs() < 1e-5);
    }

    #[test]
    fn publish_control_arms_web() {
        let params = RelayParams::new();
        params.web.set_value(true);
        publish_control(&params);
        assert!(
            params.control.web_wanted(),
            "web listen is always armed without waiting for process()"
        );
    }

    #[test]
    fn new_slug_is_three_real_words() {
        let slug = new_slug();
        let parts: Vec<_> = slug.split('-').collect();
        assert_eq!(parts.len(), 3, "{slug}");
        assert!(
            SLUG_ADJECTIVES.contains(&parts[0]),
            "unknown adjective in {slug}"
        );
        assert!(
            SLUG_ADJECTIVES.contains(&parts[1]),
            "unknown adjective in {slug}"
        );
        assert!(SLUG_NOUNS.contains(&parts[2]), "unknown noun in {slug}");
        assert_ne!(parts[0], parts[1], "{slug}");
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

    #[test]
    fn same_rate_reset_keeps_runtime() {
        let params = RelayParams::new();
        let mut state = RelayDsp::default();
        let config = AudioConfig::new(48_000.0, 128);
        RelayPlugin::reset(&mut state, &params, &config);
        assert!(state.runtime.is_some());
        let first = state
            .runtime
            .as_ref()
            .map(|runtime| core::ptr::from_ref(runtime) as usize);
        RelayPlugin::reset(&mut state, &params, &config);
        let second = state
            .runtime
            .as_ref()
            .map(|runtime| core::ptr::from_ref(runtime) as usize);
        assert_eq!(first, second);
        assert_eq!(state.device_rate, 48_000);
    }

    #[test]
    fn rate_change_rebuilds_runtime() {
        let params = RelayParams::new();
        let mut state = RelayDsp::default();
        RelayPlugin::reset(&mut state, &params, &AudioConfig::new(48_000.0, 128));
        let before = NEXT_SSRC.load(Ordering::Relaxed);
        RelayPlugin::reset(&mut state, &params, &AudioConfig::new(44_100.0, 128));
        assert!(
            NEXT_SSRC.load(Ordering::Relaxed) > before,
            "a new session engine must take a fresh SSRC"
        );
        assert!(state.runtime.is_some());
        assert_eq!(state.device_rate, 44_100);
    }
}

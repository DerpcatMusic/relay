//! RELAY editor: Polar Night chrome, Share/Join, full-height L/R meters.

use std::time::{Duration, Instant};

use relay_session::{
    DEFAULT_CONNECT_PORT, PUBLIC_LINK_ORIGIN, SessionPill, SessionView, classify_session,
    format_session_status, normalize_slug,
};
use truce_core::editor::{PluginContext, PluginContextReadF32};
use truce_egui::EditorUi;

use crate::{
    Codec, MAX_WINDOW_H, MAX_WINDOW_W, METER_FLOOR_DB, MIN_WINDOW_H, MIN_WINDOW_W, Monitor,
    Product, RelayParams, RelayParamsParamId as P, WINDOW_W, new_slug, publish_control,
};

const MATARI_URL: &str = "https://matari-audio.com";
const RELAY_URL: &str = "https://matari-audio.com";
const GAIN_DEFAULT_01: f32 = 24.0 / 36.0;
const METER_COL: f32 = 28.0;

const ICON_DICE: &[u8] = include_bytes!("../assets/icons/dice-five.svg");
const ICON_COPY: &[u8] = include_bytes!("../assets/icons/copy.svg");
const ICON_OPEN: &[u8] = include_bytes!("../assets/icons/arrow-square-out.svg");
const ICON_LOCK: &[u8] = include_bytes!("../assets/icons/lock-simple.svg");
const ICON_CHECK: &[u8] = include_bytes!("../assets/icons/check.svg");

/// BUFFR Studio Blue — same tokens as drop-recorder `themes.json`.
const BG: egui::Color32 = egui::Color32::from_rgb(25, 25, 25);
const LANE: egui::Color32 = egui::Color32::from_rgb(37, 37, 37);
const SURFACE: egui::Color32 = egui::Color32::from_rgb(53, 53, 53);
const TEXT: egui::Color32 = egui::Color32::WHITE;
const MUTED: egui::Color32 = egui::Color32::from_rgb(184, 184, 184);
const PRIMARY: egui::Color32 = egui::Color32::from_rgb(0, 170, 255);
const OK: egui::Color32 = egui::Color32::from_rgb(91, 232, 179);
const WARN: egui::Color32 = egui::Color32::from_rgb(255, 199, 92);
const HOT: egui::Color32 = egui::Color32::from_rgb(255, 112, 136);
const SUNKEN: egui::Color32 = egui::Color32::from_rgb(16, 16, 16);
const BORDER: egui::Color32 = egui::Color32::from_rgb(26, 94, 128);
const GYR_FLOOR: egui::Color32 = egui::Color32::from_rgb(61, 143, 106);

const FX_KNOB_DIAMETER: f32 = 62.0;
const FX_ARC_START: f32 = std::f32::consts::PI * 0.75;
const FX_ARC_SWEEP: f32 = std::f32::consts::PI * 1.5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    About,
}

pub struct RelayUi {
    pub peer_buf: String,
    pub name_buf: String,
    pub pass_buf: String,
    copied: Option<(String, Instant)>,
    hold_l: f32,
    hold_r: f32,
    hold_age_l: f32,
    hold_age_r: f32,
    last_h: u32,
    overlay: Overlay,
}

impl RelayUi {
    pub fn new(window_h: u32) -> Self {
        Self {
            peer_buf: String::new(),
            name_buf: String::new(),
            pass_buf: String::new(),
            copied: None,
            hold_l: 0.0,
            hold_r: 0.0,
            hold_age_l: 0.0,
            hold_age_r: 0.0,
            last_h: window_h,
            overlay: Overlay::None,
        }
    }
}

impl EditorUi<RelayParams> for RelayUi {
    fn opened(&mut self, ctx: &PluginContext<RelayParams>) {
        if let Ok(session) = ctx.params().session.read() {
            self.peer_buf.clone_from(&session.peer);
            self.name_buf.clone_from(&session.name);
            self.pass_buf.clone_from(&session.password);
        }
        if self.peer_buf.is_empty() {
            self.peer_buf = format!("127.0.0.1:{DEFAULT_CONNECT_PORT}");
        }
        if self.name_buf.is_empty() {
            self.name_buf = new_slug();
            if let Ok(mut session) = ctx.params().session.write() {
                session.name.clone_from(&self.name_buf);
            }
        }
        if ctx.params().product.value().is_link() {
            ctx.params().link.set_value(true);
        }
        ctx.params().web.set_value(true);
        let _ = ctx.params().control.set_peer(self.peer_buf.clone());
        let _ = ctx.params().control.set_session_name(self.name_buf.clone());
        let _ = ctx.params().control.set_password(self.pass_buf.clone());
        publish_control(ctx.params());
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &PluginContext<RelayParams>) {
        apply_buffr_spacing(ui);
        let snap = ctx.params().control.snapshot();
        let linked = ctx.params().link.value();
        let product = ctx.params().product.value();
        let web_ok = ctx.params().control.web_ok();
        let web_silent = ctx.params().control.web_silent();
        egui::Panel::top("header")
            .exact_size(40.0)
            .frame(egui::Frame::NONE.fill(BG))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(10.0);
                    if relay_logo(ui).clicked() {
                        self.overlay = if self.overlay == Overlay::About {
                            Overlay::None
                        } else {
                            Overlay::About
                        };
                    }
                    ui.add_space(10.0);
                    mode_nav(ui, ctx, product);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(10.0);
                        live_pill(ui, ctx, linked, snap, web_ok, web_silent);
                    });
                });
            });

        let peak_l = ctx.get_meter(P::MeterLeft);
        let peak_r = ctx.get_meter(P::MeterRight);
        update_hold(&mut self.hold_l, &mut self.hold_age_l, peak_l);
        update_hold(&mut self.hold_r, &mut self.hold_age_r, peak_r);

        egui::Panel::left("meter-l")
            .exact_size(METER_COL)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(7, 10)),
            )
            .show(ui, |ui| {
                meter_column(ui, peak_l, self.hold_l, "L");
            });

        egui::Panel::right("meter-r")
            .exact_size(METER_COL)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(7, 10)),
            )
            .show(ui, |ui| {
                meter_column(ui, peak_r, self.hold_r, "R");
            });

        let mut content_bottom = 0.0;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(ui.style())
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                if product.is_link() {
                    session_row(ui, self, ctx);
                } else {
                    labeled_field(
                        ui,
                        "Peer",
                        &mut self.peer_buf,
                        false,
                        "host:port or session name",
                        |value| {
                            if let Ok(mut session) = ctx.params().session.write() {
                                session.peer = value.to_owned();
                            }
                            let _ = ctx.params().control.set_peer(value.to_owned());
                            if !value.trim().is_empty() {
                                ctx.params().link.set_value(true);
                                publish_control(ctx.params());
                            }
                        },
                    );
                }
                password_row(ui, self, ctx);
                codec_row(ui, ctx);
                if !product.is_link() {
                    let current = ctx.params().monitor.value();
                    if let Some(next) = buffr_segmented(
                        ui,
                        "relay-monitor",
                        &[
                            (Monitor::Dry, "Dry"),
                            (Monitor::Mix, "Mix"),
                            (Monitor::Remote, "Hear"),
                        ],
                        current,
                    ) {
                        ctx.params().monitor.set_value(next);
                    }
                }

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    gain_knob(ui, ctx, P::InputGain, "Send");
                    if !product.is_link() {
                        gain_knob(ui, ctx, P::OutputGain, "Hear");
                    }
                });

                ui.label(
                    egui::RichText::new(editor_status(ctx, linked, snap, web_ok, web_silent))
                        .size(12.0)
                        .color(MUTED),
                );
                content_bottom = ui.cursor().min.y;
            });

        match self.overlay {
            Overlay::None => {}
            Overlay::About => about_window(ui, &mut self.overlay),
        }

        let needed = (content_bottom + 16.0)
            .ceil()
            .clamp(MIN_WINDOW_H as f32, MAX_WINDOW_H as f32) as u32;
        if needed.abs_diff(self.last_h) >= 8 {
            let _ = ctx.request_resize(WINDOW_W, needed);
            self.last_h = needed;
        }
        let _ = (MIN_WINDOW_W, MAX_WINDOW_W);
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

pub fn buffr_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = SUNKEN;
    visuals.faint_bg_color = LANE;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = PRIMARY;
    visuals.selection.stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.inactive.bg_fill = SUNKEN;
    visuals.widgets.inactive.weak_bg_fill = SUNKEN;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, MUTED);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.hovered.bg_fill = SURFACE;
    visuals.widgets.hovered.weak_bg_fill = SURFACE;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.active.bg_fill = SURFACE;
    visuals.widgets.active.weak_bg_fill = SURFACE;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.open.bg_fill = SURFACE;
    visuals.widgets.open.weak_bg_fill = SURFACE;
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.open.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, MUTED);
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 48, 48));
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 3],
        blur: 16,
        spread: 0,
        color: egui::Color32::from_black_alpha(48),
    };
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 28,
        spread: 0,
        color: egui::Color32::from_black_alpha(56),
    };
    visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 48, 48));
    let radius = egui::CornerRadius::same(4);
    visuals.menu_corner_radius = radius;
    visuals.window_corner_radius = radius;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = radius;
    }
    visuals
}

fn apply_buffr_spacing(ui: &mut egui::Ui) {
    let spacing = &mut ui.style_mut().spacing;
    spacing.item_spacing = egui::vec2(8.0, 6.0);
    spacing.button_padding = egui::vec2(8.0, 5.0);
    spacing.combo_width = 160.0;
    spacing.interact_size.y = 26.0;
}

fn relay_logo(ui: &mut egui::Ui) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        paint_matari_mark(ui, egui::vec2(26.0, 16.0));
        ui.label(
            egui::RichText::new("RELAY")
                .size(13.0)
                .color(TEXT)
                .strong()
                .extra_letter_spacing(1.6),
        );
    })
    .response
    .interact(egui::Sense::click())
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .on_hover_text("About RELAY")
}

fn mode_nav(ui: &mut egui::Ui, ctx: &PluginContext<RelayParams>, product: Product) {
    let options = [(true, "Share"), (false, "Join")];
    let current = product.is_link();
    if let Some(link) = buffr_segmented(ui, "relay-mode", &options, current) {
        let next = if link {
            Product::Link
        } else {
            Product::Connect
        };
        ctx.params().product.set_value(next);
        if link {
            ctx.params().link.set_value(true);
        }
        publish_control(ctx.params());
    }
}

fn buffr_segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    options: &[(T, &str)],
    current: T,
) -> Option<T> {
    let font = egui::FontId::proportional(12.0);
    let pad = 12.0;
    let inset = 2.0;
    let height = 26.0;
    let radius = 4.0;
    let seg_w = options
        .iter()
        .map(|(_, label)| {
            ui.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap((*label).to_owned(), font.clone(), TEXT)
                    .size()
                    .x
            }) + pad * 2.0
        })
        .fold(56.0_f32, f32::max);
    let total_w = seg_w * options.len() as f32 + inset * 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, radius, SUNKEN);
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 48, 48)),
        egui::StrokeKind::Inside,
    );
    let selected = options
        .iter()
        .position(|(value, _)| *value == current)
        .unwrap_or(0);
    let eased = ui.ctx().animate_value_with_time(
        ui.id().with((id, "indicator")),
        selected as f32,
        ui.style().animation_time,
    );
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(rect.left() + inset + eased * seg_w, rect.top() + inset),
            egui::vec2(seg_w, height - inset * 2.0),
        ),
        3.0,
        PRIMARY,
    );
    let mut chosen = None;
    let mut x = rect.left() + inset;
    for (index, (value, label)) in options.iter().enumerate() {
        let seg = egui::Rect::from_min_size(
            egui::pos2(x, rect.top() + inset),
            egui::vec2(seg_w, height - inset * 2.0),
        );
        let response = ui.interact(seg, ui.id().with((id, index)), egui::Sense::click());
        let selected = *value == current;
        if !selected && response.hovered() {
            ui.painter().rect_filled(seg, 3.0, SURFACE);
        }
        ui.painter().text(
            seg.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            font.clone(),
            if selected { BG } else { TEXT },
        );
        if response.clicked() && !selected {
            chosen = Some(*value);
        }
        x += seg_w;
    }
    chosen
}

fn gain_knob(ui: &mut egui::Ui, ctx: &PluginContext<RelayParams>, id: P, label: &str) {
    let diameter = FX_KNOB_DIAMETER;
    let width = (diameter * (4.7 / 4.15)).max(diameter);
    let (_, rect) = ui.allocate_space(egui::vec2(width, diameter));
    let response = ui.interact(
        rect,
        ui.id().with(("fx-knob", label)),
        egui::Sense::click_and_drag(),
    );
    let mut value = ctx.get_param(id);
    let reset = response.double_clicked();
    if reset {
        ctx.automate(id, f64::from(GAIN_DEFAULT_01));
        value = GAIN_DEFAULT_01;
    } else if response.dragged() {
        let drag_id = response.id.with("normalized-drag-value");
        if response.drag_started() {
            ctx.begin_edit(id);
            ui.ctx().data_mut(|data| data.insert_temp(drag_id, value));
        }
        let mut drag = ui
            .ctx()
            .data(|data| data.get_temp::<f32>(drag_id))
            .unwrap_or(value);
        let fine = ui.input(|input| input.modifiers.shift);
        let sensitivity = if fine { 0.0007 } else { 0.005 };
        drag = (drag - ui.input(|input| input.pointer.delta().y) * sensitivity).clamp(0.0, 1.0);
        ui.ctx().data_mut(|data| data.insert_temp(drag_id, drag));
        value = drag;
        ctx.set_param(id, f64::from(value));
    }
    if response.drag_stopped() {
        ui.ctx()
            .data_mut(|data| data.remove::<f32>(response.id.with("normalized-drag-value")));
        ctx.end_edit(id);
    }

    let interactive = response.hovered() || response.dragged() || response.has_focus();
    let interactive_t =
        ui.ctx()
            .animate_bool_with_time(response.id.with("outer-arc"), interactive, 0.18);
    let center = rect.center();
    let radius = diameter * 0.43;
    let value_angle = FX_ARC_START + value * FX_ARC_SWEEP;
    let fill_start = FX_ARC_START + GAIN_DEFAULT_01 * FX_ARC_SWEEP;
    let track = mix(SUNKEN, TEXT, 0.24);
    let rim = mix(SUNKEN, TEXT, 0.18);
    ui.painter().circle_filled(
        center + egui::vec2(0.0, 2.0),
        radius + 1.0,
        egui::Color32::from_black_alpha(if interactive { 92 } else { 68 }),
    );
    ui.painter().circle_filled(center, radius, rim);
    ui.painter().circle_filled(center, radius - 2.0, SUNKEN);
    let arc_radius = radius + 3.5;
    paint_arc(
        ui.painter(),
        center,
        arc_radius,
        FX_ARC_START,
        FX_ARC_START + FX_ARC_SWEEP,
        egui::Stroke::new(1.2 + interactive_t * 0.3, track),
    );
    if (value_angle - fill_start).abs() > 0.001 {
        let value_width = 1.35 + interactive_t * 1.35;
        paint_arc(
            ui.painter(),
            center,
            arc_radius + (value_width - 1.35) * 0.5,
            fill_start,
            value_angle,
            egui::Stroke::new(value_width, PRIMARY),
        );
    }
    let direction = egui::emath::Rot2::from_angle(value_angle) * egui::vec2(1.0, 0.0);
    ui.painter().line_segment(
        [
            center + direction * radius * 0.62,
            center + direction * radius * 0.86,
        ],
        egui::Stroke::new((radius * 0.075).clamp(1.75, 2.75), TEXT),
    );
    ui.painter().text(
        egui::pos2(center.x, center.y - 4.8),
        egui::Align2::CENTER_CENTER,
        ctx.format_param(id),
        egui::FontId::monospace(11.0),
        TEXT,
    );
    ui.painter().text(
        egui::pos2(center.x, center.y + 8.8),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(9.5),
        MUTED,
    );
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    response
        .on_hover_text("Double-click to reset")
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
}

fn mix(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    egui::Color32::from_rgba_unmultiplied(
        (f32::from(a.r()) * inv + f32::from(b.r()) * t).round() as u8,
        (f32::from(a.g()) * inv + f32::from(b.g()) * t).round() as u8,
        (f32::from(a.b()) * inv + f32::from(b.b()) * t).round() as u8,
        (f32::from(a.a()) * inv + f32::from(b.a()) * t).round() as u8,
    )
}

fn paint_arc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    start: f32,
    end: f32,
    stroke: egui::Stroke,
) {
    let steps = (((end - start).abs() / std::f32::consts::PI) * 32.0)
        .ceil()
        .max(4.0) as usize;
    for step in 0..steps {
        let t0 = step as f32 / steps as f32;
        let t1 = (step + 1) as f32 / steps as f32;
        let a0 = start + (end - start) * t0;
        let a1 = start + (end - start) * t1;
        painter.line_segment(
            [
                center + egui::emath::Rot2::from_angle(a0) * egui::vec2(radius, 0.0),
                center + egui::emath::Rot2::from_angle(a1) * egui::vec2(radius, 0.0),
            ],
            stroke,
        );
    }
}

fn live_pill(
    ui: &mut egui::Ui,
    ctx: &PluginContext<RelayParams>,
    linked: bool,
    snap: relay_session::SessionSnapshot,
    web_ok: bool,
    web_silent: bool,
) {
    let view = editor_view(ctx, linked, snap, web_ok, web_silent);
    let pill = classify_session(view);
    let (fill, tip) = match pill {
        SessionPill::Off => (SURFACE, "Start sending"),
        SessionPill::Failed => (HOT, "Bind failed — click to retry"),
        SessionPill::Asleep => (WARN, "Silent — click to wake"),
        SessionPill::Hosting | SessionPill::Streaming => {
            (PRIMARY, "Ready — waiting for a listener")
        }
        SessionPill::Joining => (PRIMARY, "Joining"),
        SessionPill::Live => (OK, "Pause or resume"),
    };
    let button = egui::Button::new(
        egui::RichText::new(pill.as_str())
            .size(11.0)
            .color(BG)
            .strong(),
    )
    .fill(fill)
    .corner_radius(4.0)
    .min_size(egui::vec2(56.0, 22.0));
    if ui.add(button).on_hover_text(tip).clicked() {
        if pill == SessionPill::Asleep {
            ctx.params().control.request_web_wake();
        } else {
            ctx.params().link.set_value(!linked);
            publish_control(ctx.params());
        }
    }
}

fn editor_view(
    ctx: &PluginContext<RelayParams>,
    linked: bool,
    snap: relay_session::SessionSnapshot,
    web_ok: bool,
    web_silent: bool,
) -> SessionView {
    SessionView {
        linked,
        role: ctx.params().control.role(),
        state: snap.state,
        peers: snap.peers,
        lan_browsers: ctx.params().control.lan_listeners(),
        web_listeners: ctx.params().control.web_listeners(),
        web_ok,
        web_silent,
        web_wanted: true,
        bound: snap.bound,
    }
}

fn editor_status(
    ctx: &PluginContext<RelayParams>,
    linked: bool,
    snap: relay_session::SessionSnapshot,
    web_ok: bool,
    web_silent: bool,
) -> String {
    let view = editor_view(ctx, linked, snap, web_ok, web_silent);
    let error = ctx.params().control.last_error().unwrap_or_default();
    let who = ctx.params().control.who().unwrap_or_default();
    format_session_status(view, snap.local_port, 0, snap.dropouts, &who, &error)
}

fn session_row(ui: &mut egui::Ui, state: &mut RelayUi, ctx: &PluginContext<RelayParams>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let width = (ui.available_width() - 100.0).max(80.0);
        let name = ui.add(
            egui::TextEdit::singleline(&mut state.name_buf)
                .desired_width(width)
                .hint_text("session name")
                .margin(egui::Margin::symmetric(10, 6)),
        );
        if name.lost_focus()
            || (name.changed() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
        {
            commit_name(state, ctx);
        }
        if icon_btn(ui, "bytes://phosphor/dice-five.svg", ICON_DICE, "New name").clicked() {
            state.name_buf = new_slug();
            commit_name(state, ctx);
        }
        let copied = state
            .copied
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(2));
        let (copy_uri, copy_bytes, copy_tip) = if copied {
            (
                "bytes://phosphor/check.svg",
                ICON_CHECK,
                "Copied listen link",
            )
        } else {
            ("bytes://phosphor/copy.svg", ICON_COPY, "Copy listen link")
        };
        if icon_btn(ui, copy_uri, copy_bytes, copy_tip).clicked() {
            commit_name(state, ctx);
            let url = public_url(&state.name_buf);
            copy_link(&url);
            ui.ctx().copy_text(url.clone());
            state.copied = Some((url, Instant::now()));
        }
        if icon_btn(
            ui,
            "bytes://phosphor/arrow-square-out.svg",
            ICON_OPEN,
            "Open listen page",
        )
        .clicked()
        {
            commit_name(state, ctx);
            let url = public_url(&state.name_buf);
            let _ = open::that_detached(&url);
        }
    });
}

fn password_row(ui: &mut egui::Ui, state: &mut RelayUi, ctx: &PluginContext<RelayParams>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add(
            egui::Image::from_bytes("bytes://phosphor/lock-simple.svg", ICON_LOCK)
                .fit_to_exact_size(egui::vec2(16.0, 16.0))
                .tint(MUTED)
                .alt_text("Password"),
        );
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.pass_buf)
                .password(true)
                .hint_text("optional")
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(10, 6)),
        );
        if response.changed() || response.lost_focus() {
            commit_password(ctx, &state.pass_buf);
        }
    });
}

fn labeled_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    password: bool,
    hint: &str,
    mut on_change: impl FnMut(&str),
) {
    ui.label(egui::RichText::new(label).size(11.0).color(MUTED));
    let mut edit = egui::TextEdit::singleline(value)
        .desired_width(f32::INFINITY)
        .hint_text(hint)
        .margin(egui::Margin::symmetric(10, 6));
    if password {
        edit = edit.password(true);
    }
    let response = ui.add(edit);
    if response.changed() || response.lost_focus() {
        on_change(value);
    }
}

fn codec_row(ui: &mut egui::Ui, ctx: &PluginContext<RelayParams>) {
    let current = ctx.params().codec.value();
    let selected = codec_summary(ctx, current);
    egui::ComboBox::from_id_salt("relay-codec")
        .selected_text(selected)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for value in [Codec::Opus, Codec::Flac, Codec::Pcm] {
                if ui
                    .selectable_label(current == value, codec_name(value))
                    .clicked()
                {
                    ctx.params().codec.set_value(value);
                    publish_control(ctx.params());
                }
            }
        });
    match current {
        Codec::Opus => {
            let mut kbps = ctx.params().bitrate.value();
            let drag = ui.add(
                egui::DragValue::new(&mut kbps)
                    .range(64..=256)
                    .suffix(" kbps")
                    .speed(1.0),
            );
            if drag.changed() {
                ctx.params().bitrate.set_value(kbps);
                publish_control(ctx.params());
            }
        }
        Codec::Flac => {
            let mut level = ctx.params().flac_level.value();
            let drag = ui.add(egui::DragValue::new(&mut level).range(0..=8).speed(1.0));
            if drag.changed() {
                ctx.params().flac_level.set_value(level);
                publish_control(ctx.params());
            }
        }
        Codec::Pcm => {}
    }
}

fn codec_name(codec: Codec) -> &'static str {
    match codec {
        Codec::Opus => "Opus",
        Codec::Flac => "FLAC",
        Codec::Pcm => "PCM · LAN",
    }
}

fn codec_summary(ctx: &PluginContext<RelayParams>, codec: Codec) -> String {
    match codec {
        Codec::Opus => format!("Opus · {} kbps", ctx.params().bitrate.value()),
        Codec::Flac => format!("FLAC · {}", ctx.params().flac_level.value()),
        Codec::Pcm => "PCM · 16-bit LAN".into(),
    }
}

fn icon_btn(
    ui: &mut egui::Ui,
    uri: &'static str,
    bytes: &'static [u8],
    tip: &str,
) -> egui::Response {
    let image = egui::Image::from_bytes(uri, bytes)
        .fit_to_exact_size(egui::vec2(16.0, 16.0))
        .tint(TEXT);
    ui.add(
        egui::Button::image(image)
            .fill(LANE)
            .corner_radius(4.0)
            .min_size(egui::vec2(28.0, 28.0)),
    )
    .on_hover_text(tip)
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn about_window(ui: &mut egui::Ui, overlay: &mut Overlay) {
    let mut open = true;
    egui::Window::new("About RELAY")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .frame(egui::Frame::window(ui.style()).fill(SURFACE))
        .show(ui.ctx(), |ui| {
            ui.set_width(280.0);
            ui.vertical_centered(|ui| {
                let _ = relay_logo(ui);
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Send the track. Hear it next door.")
                        .size(13.0)
                        .color(TEXT),
                );
                ui.label(
                    egui::RichText::new("CLAP + VST3 · LAN and web")
                        .size(11.0)
                        .color(MUTED),
                );
            });
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                paint_matari_mark(ui, egui::vec2(31.0, 19.0));
                ui.vertical(|ui| {
                    ui.hyperlink_to(
                        egui::RichText::new("Matari Audio")
                            .size(13.0)
                            .color(TEXT)
                            .strong(),
                        MATARI_URL,
                    );
                    ui.label(
                        egui::RichText::new("Share copies the listen link.")
                            .size(11.0)
                            .color(MUTED),
                    );
                });
            });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to("matari-audio.com", MATARI_URL);
                ui.label(egui::RichText::new("·").color(MUTED));
                ui.hyperlink_to("RELAY", RELAY_URL);
            });
        });
    if !open {
        *overlay = Overlay::None;
    }
}

fn paint_matari_mark(ui: &mut egui::Ui, size: egui::Vec2) {
    ui.add(
        egui::Image::from_bytes(
            "bytes://matari-mark.svg",
            &include_bytes!("../assets/matari-mark.svg")[..],
        )
        .fit_to_exact_size(size)
        .tint(TEXT)
        .alt_text("Matari Audio"),
    );
}

fn meter_column(ui: &mut egui::Ui, peak: f32, hold: f32, label: &str) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        let db = peak_to_db(peak);
        let clip = db >= -0.2;
        let (lamp, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(
            lamp.center(),
            3.5,
            if clip {
                HOT
            } else {
                egui::Color32::from_rgb(42, 32, 32)
            },
        );
        ui.add_space(4.0);
        let rail_h = (ui.available_height() - 18.0).max(48.0);
        let (rail, _) = ui.allocate_exact_size(egui::vec2(10.0, rail_h), egui::Sense::hover());
        paint_gyr_rail(ui.painter(), rail, db, peak_to_db(hold));
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .color(MUTED)
                .strong()
                .extra_letter_spacing(0.8),
        );
    });
}

fn paint_gyr_rail(painter: &egui::Painter, rect: egui::Rect, db: f32, hold: f32) {
    painter.rect_filled(rect, 2.0, SUNKEN);
    let rail = rect.shrink(1.0);
    if rail.height() < 2.0 {
        return;
    }
    let stops = [(0.00, GYR_FLOOR), (0.42, OK), (0.78, WARN), (1.00, HOT)];
    let mut mesh = egui::Mesh::default();
    for (t, color) in stops {
        let y = rail.bottom() - rail.height() * t;
        mesh.colored_vertex(egui::pos2(rail.left(), y), color);
        mesh.colored_vertex(egui::pos2(rail.right(), y), color);
    }
    for i in 0_u32..3 {
        let n = i * 2;
        mesh.add_triangle(n, n + 1, n + 2);
        mesh.add_triangle(n + 2, n + 1, n + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
    let pos = db_to_pos(db);
    if pos < 1.0 {
        painter.rect_filled(
            egui::Rect::from_min_max(
                rail.min,
                egui::pos2(rail.right(), rail.bottom() - rail.height() * pos),
            ),
            0.0,
            SUNKEN,
        );
    }
    let hold_pos = db_to_pos(hold);
    if hold_pos > 0.02 {
        let y = rail.bottom() - rail.height() * hold_pos;
        painter.hline(rail.x_range(), y, egui::Stroke::new(1.0, TEXT));
    }
}

fn peak_to_db(peak: f32) -> f32 {
    if !peak.is_finite() || peak <= 1.0e-6 {
        return METER_FLOOR_DB;
    }
    (20.0 * peak.log10()).clamp(METER_FLOOR_DB, 6.0)
}

fn db_to_pos(db: f32) -> f32 {
    ((db.clamp(METER_FLOOR_DB, 0.0) - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0)
}

fn update_hold(hold: &mut f32, age: &mut f32, peak: f32) {
    if peak >= *hold {
        *hold = peak;
        *age = 0.0;
    } else {
        *age += 0.033;
        if *age > 0.9 {
            *hold *= 0.82;
            if *hold < peak {
                *hold = peak;
            }
        }
    }
}

fn public_url(name: &str) -> String {
    format!("{PUBLIC_LINK_ORIGIN}/{}", normalize_slug(name))
}

fn commit_name(ui_state: &mut RelayUi, ctx: &PluginContext<RelayParams>) {
    let slug = normalize_slug(&ui_state.name_buf);
    if slug.is_empty() {
        return;
    }
    ui_state.name_buf.clone_from(&slug);
    if let Ok(mut session) = ctx.params().session.write() {
        session.name.clone_from(&slug);
    }
    let _ = ctx.params().control.set_session_name(slug);
}

fn commit_password(ctx: &PluginContext<RelayParams>, value: &str) {
    if let Ok(mut session) = ctx.params().session.write() {
        session.password = value.to_owned();
    }
    let _ = ctx.params().control.set_password(value.to_owned());
}

fn copy_link(value: &str) {
    if arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(value.to_owned()))
        .is_ok()
    {
        return;
    }
    for command in [
        ("wl-copy", vec![]),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ] {
        if pipe_copy(command.0, &command.1, value) {
            return;
        }
    }
}

fn pipe_copy(bin: &str, args: &[&str], value: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let wrote = child
        .stdin
        .as_mut()
        .is_some_and(|stdin| stdin.write_all(value.as_bytes()).is_ok());
    wrote && child.wait().map(|status| status.success()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use relay_session::{
        ConnectionState, SessionRole, SessionView, classify_session, format_session_status,
    };

    fn view(silent: bool, web: u32) -> SessionView {
        SessionView {
            linked: true,
            role: SessionRole::ConnectListen,
            state: ConnectionState::Connecting,
            peers: 0,
            lan_browsers: 0,
            web_listeners: web,
            web_ok: true,
            web_silent: silent,
            web_wanted: true,
            bound: true,
        }
    }

    #[test]
    fn status_line_names_asleep_when_silent() {
        assert_eq!(classify_session(view(true, 0)).as_str(), "asleep");
        assert!(
            format_session_status(view(true, 2), Some(17_492), 8787, 0, "", "")
                .starts_with("Asleep")
        );
        assert_eq!(classify_session(view(false, 0)).as_str(), "ready");
        let ready = format_session_status(view(false, 0), Some(17_492), 8787, 0, "", "");
        assert!(ready.starts_with("Ready"), "{ready}");
        assert!(!ready.contains("UDP"), "{ready}");
        assert!(!ready.contains("Web"), "{ready}");
    }
}

//! RELAY editor: BUFFR Studio Blue chrome, Plugcat knobs, labeled fields.

use std::time::{Duration, Instant};

use plugcat::{
    tactile_knob_with_tokens, WidgetColors, WidgetRadius, WidgetSpacing, WidgetStroke, WidgetTokens,
};
use relay_session::{
    lan_listen_url, normalize_slug, ConnectionState, DEFAULT_CONNECT_PORT, PUBLIC_LINK_ORIGIN,
};
use truce_core::editor::{PluginContext, PluginContextReadF32};
use truce_egui::EditorUi;

use crate::{
    new_slug, publish_control, Codec, Product, RelayParams, RelayParamsParamId as P, MAX_WINDOW_H,
    MAX_WINDOW_W, METER_FLOOR_DB, MIN_WINDOW_H, MIN_WINDOW_W, WINDOW_W,
};

const MATARI_URL: &str = "https://matari-audio.com";
const RELAY_URL: &str = "https://matari-audio.com";
const GAIN_DEFAULT_01: f32 = 24.0 / 36.0;

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

const STUDIO_TOKENS: WidgetTokens = WidgetTokens {
    name: "Studio Blue",
    light_visuals: false,
    colors: WidgetColors {
        background: BG,
        surface: SURFACE,
        surface_low: LANE,
        surface_high: SURFACE,
        surface_dark: SUNKEN,
        border: BORDER,
        text: TEXT,
        text_on_dark: TEXT,
        muted: MUTED,
        muted_on_dark: MUTED,
        accent: PRIMARY,
        accent_hover: egui::Color32::from_rgb(37, 231, 255),
        selected: SURFACE,
        track: LANE,
        success: OK,
        warning: WARN,
        error: HOT,
        disabled: LANE,
        disabled_text: MUTED,
        shadow: egui::Color32::from_black_alpha(64),
        transparent: egui::Color32::TRANSPARENT,
        white: egui::Color32::WHITE,
        knob_cap: egui::Color32::from_rgb(37, 43, 48),
        knob_cap_highlight: egui::Color32::from_rgb(48, 54, 59),
        knob_arc_track: LANE,
        knob_arc_value: TEXT,
        knob_marker: TEXT,
    },
    radius: WidgetRadius {
        panel: 6,
        control: 6,
        tile: 6,
    },
    spacing: WidgetSpacing {
        xs: 4.0,
        sm: 8.0,
        md: 8.0,
        lg: 12.0,
    },
    stroke: WidgetStroke { control: 1.0 },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    About,
    Settings,
}

pub struct RelayUi {
    pub peer_buf: String,
    pub name_buf: String,
    pub pass_buf: String,
    copied: Option<(String, Instant)>,
    hold: f32,
    hold_age: f32,
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
            hold: 0.0,
            hold_age: 0.0,
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
        let web_on = ctx.params().web.value();
        let web_ok = ctx.params().control.web_ok();
        let web_silent = ctx.params().control.web_silent();
        let listeners = ctx.params().control.web_listeners();
        egui::Panel::top("header")
            .exact_size(40.0)
            .frame(egui::Frame::NONE.fill(BG))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    if relay_logo(ui).clicked() {
                        self.overlay = if self.overlay == Overlay::About {
                            Overlay::None
                        } else {
                            Overlay::About
                        };
                    }
                    ui.add_space(12.0);
                    mode_nav(ui, ctx, product);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        if chip(ui, "Settings").clicked() {
                            self.overlay = if self.overlay == Overlay::Settings {
                                Overlay::None
                            } else {
                                Overlay::Settings
                            };
                        }
                        ui.add_space(6.0);
                        live_pill(ui, ctx, linked, snap, web_ok, web_silent);
                    });
                });
            });

        let mut content_bottom = 0.0;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(ui.style())
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(14, 12)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                ui.label(
                    egui::RichText::new(if product.is_link() {
                        self.name_buf.as_str()
                    } else {
                        "Connect"
                    })
                    .size(20.0)
                    .color(TEXT)
                    .strong(),
                );
                if product.is_link() {
                    session_row(ui, self, ctx);
                } else {
                    labeled_field(ui, "Peer", &mut self.peer_buf, false, |value| {
                        if let Ok(mut session) = ctx.params().session.write() {
                            session.peer = value.to_owned();
                        }
                        let _ = ctx.params().control.set_peer(value.to_owned());
                        if !value.trim().is_empty() {
                            ctx.params().link.set_value(true);
                            publish_control(ctx.params());
                        }
                    });
                }
                labeled_field(ui, "Password", &mut self.pass_buf, true, |value| {
                    commit_password(ctx, value);
                });
                codec_row(ui, ctx);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    gain_knob(ui, ctx, P::InputGain, "Send");
                    gain_knob(ui, ctx, P::OutputGain, "Hear");
                });
                self.level_strip(ui, ctx);

                ui.label(
                    egui::RichText::new(status_line(
                        linked,
                        web_on,
                        web_ok,
                        web_silent,
                        listeners,
                        snap.peers,
                        ctx.params().control.lan_listeners(),
                    ))
                    .size(12.0)
                    .color(MUTED),
                );
                content_bottom = ui.cursor().min.y;
            });

        match self.overlay {
            Overlay::None => {}
            Overlay::About => about_window(ui, &mut self.overlay),
            Overlay::Settings => settings_window(ui, ctx, self),
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
    let radius = egui::CornerRadius::same(6);
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
    spacing.button_padding = egui::vec2(11.0, 5.0);
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
    .on_hover_text("About RELAY · Matari Audio")
}

fn mode_nav(ui: &mut egui::Ui, ctx: &PluginContext<RelayParams>, product: Product) {
    let options = [(false, "CONNECT"), (true, "LINK")];
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
    let pad = 14.0;
    let inset = 2.0;
    let height = 26.0;
    let radius = 8.0;
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
        .fold(64.0_f32, f32::max);
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
        6.0,
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
            ui.painter().rect_filled(seg, 6.0, SURFACE);
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
    ui.vertical(|ui| {
        ui.set_width(72.0);
        let mut value = ctx.get_param(id);
        let response = tactile_knob_with_tokens(ui, &mut value, 46.0, true, &STUDIO_TOKENS);
        if response.double_clicked() {
            ctx.automate(id, f64::from(GAIN_DEFAULT_01));
        } else {
            if response.drag_started() {
                ctx.begin_edit(id);
            }
            if response.changed() {
                ctx.set_param(id, f64::from(value));
            }
            if response.drag_stopped() {
                ctx.end_edit(id);
            }
        }
        ui.label(
            egui::RichText::new(ctx.format_param(id))
                .size(11.0)
                .color(TEXT),
        );
        ui.label(egui::RichText::new(label).size(11.0).color(MUTED));
    })
    .response
    .on_hover_text("Double-click to reset");
}

fn live_pill(
    ui: &mut egui::Ui,
    ctx: &PluginContext<RelayParams>,
    linked: bool,
    snap: relay_session::SessionSnapshot,
    web_ok: bool,
    web_silent: bool,
) {
    let (label, fill) = if !linked {
        ("off", SURFACE)
    } else if snap.state == ConnectionState::Failed {
        ("failed", HOT)
    } else if web_ok || snap.peers > 0 || snap.state == ConnectionState::Connected {
        if web_silent && snap.peers == 0 {
            ("live", PRIMARY)
        } else {
            ("live", OK)
        }
    } else {
        ("ready", PRIMARY)
    };
    let button = egui::Button::new(egui::RichText::new(label).size(11.0).color(BG).strong())
        .fill(fill)
        .corner_radius(8.0)
        .min_size(egui::vec2(54.0, 22.0));
    if ui.add(button).on_hover_text("Pause or resume").clicked() {
        ctx.params().link.set_value(!linked);
        publish_control(ctx.params());
    }
}

fn session_row(ui: &mut egui::Ui, state: &mut RelayUi, ctx: &PluginContext<RelayParams>) {
    ui.label(egui::RichText::new("Session").size(11.0).color(MUTED));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let width = (ui.available_width() - 168.0).max(80.0);
        let name = ui.add(
            egui::TextEdit::singleline(&mut state.name_buf)
                .desired_width(width)
                .margin(egui::Margin::symmetric(10, 6)),
        );
        if name.lost_focus()
            || (name.changed() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
        {
            commit_name(state, ctx);
        }
        if chip(ui, "New").clicked() {
            state.name_buf = new_slug();
            commit_name(state, ctx);
        }
        let copied = state
            .copied
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(2));
        if chip(ui, if copied { "Copied" } else { "Copy" }).clicked() {
            commit_name(state, ctx);
            let url = listen_url(ctx, &state.name_buf);
            copy_link(&url);
            ui.ctx().copy_text(url.clone());
            state.copied = Some((url, Instant::now()));
        }
        if chip(ui, "Open").clicked() {
            commit_name(state, ctx);
            let url = listen_url(ctx, &state.name_buf);
            let _ = open::that_detached(&url);
        }
    });
}

fn labeled_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    password: bool,
    mut on_change: impl FnMut(&str),
) {
    ui.label(egui::RichText::new(label).size(11.0).color(MUTED));
    let mut edit = egui::TextEdit::singleline(value)
        .desired_width(f32::INFINITY)
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
    ui.label(egui::RichText::new("Codec").size(11.0).color(MUTED));
    let current = ctx.params().codec.value();
    let label = codec_label(current);
    egui::ComboBox::from_id_salt("relay-codec")
        .selected_text(label)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for value in [Codec::Opus, Codec::Flac, Codec::Pcm] {
                if ui
                    .selectable_label(current == value, codec_label(value))
                    .clicked()
                {
                    ctx.params().codec.set_value(value);
                    publish_control(ctx.params());
                }
            }
        });
}

fn codec_label(codec: Codec) -> &'static str {
    match codec {
        Codec::Opus => "Opus · 192 kbps",
        Codec::Flac => "FLAC · 16-bit",
        Codec::Pcm => "PCM · 16-bit LAN",
    }
}

fn chip(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(11.0).color(TEXT).strong())
            .fill(LANE)
            .corner_radius(6.0)
            .min_size(egui::vec2(44.0, 24.0)),
    )
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
                    egui::RichText::new("Low-latency listen")
                        .size(13.0)
                        .color(TEXT),
                );
                ui.label(
                    egui::RichText::new("CLAP + VST3 · LAN first")
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
                        egui::RichText::new("Made for producers who would rather keep creating.")
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

fn settings_window(ui: &mut egui::Ui, ctx: &PluginContext<RelayParams>, state: &mut RelayUi) {
    let mut open = true;
    egui::Window::new("Settings")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 8.0])
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .frame(egui::Frame::window(ui.style()).fill(SURFACE))
        .show(ui.ctx(), |ui| {
            ui.set_width(260.0);
            ui.label(egui::RichText::new("Listen").size(11.0).color(MUTED));
            let web_on = ctx.params().web.value();
            if let Some(web) =
                buffr_segmented(ui, "relay-web", &[(false, "LAN"), (true, "Web")], web_on)
            {
                ctx.params().web.set_value(web);
                publish_control(ctx.params());
            }
            ui.add_space(12.0);
            ui.label(egui::RichText::new("Defaults").size(11.0).color(MUTED));
            if chip(ui, "Reset all values")
                .on_hover_text("Send, Hear, codec, web, port")
                .clicked()
            {
                reset_defaults(ctx, state);
            }
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Double-click a knob to reset it.")
                    .size(11.0)
                    .color(MUTED),
            );
        });
    if !open {
        state.overlay = Overlay::None;
    }
}

fn reset_defaults(ctx: &PluginContext<RelayParams>, state: &mut RelayUi) {
    ctx.automate(P::InputGain, f64::from(GAIN_DEFAULT_01));
    ctx.automate(P::OutputGain, f64::from(GAIN_DEFAULT_01));
    ctx.params().codec.set_value(Codec::Opus);
    ctx.params().web.set_value(false);
    ctx.params().bitrate.set_value(192);
    ctx.params().flac_level.set_value(5);
    ctx.params().port.set_value(17_492);
    ctx.params().link.set_value(true);
    state.pass_buf.clear();
    if let Ok(mut session) = ctx.params().session.write() {
        session.password.clear();
    }
    let _ = ctx.params().control.set_password(String::new());
    publish_control(ctx.params());
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

impl RelayUi {
    fn level_strip(&mut self, ui: &mut egui::Ui, ctx: &PluginContext<RelayParams>) {
        let peak = ctx
            .get_meter(P::MeterLeft)
            .max(ctx.get_meter(P::MeterRight));
        update_hold(&mut self.hold, &mut self.hold_age, peak);
        let desired = egui::vec2(ui.available_width(), 22.0);
        let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
        paint_gyr_strip(ui.painter(), rect, peak_to_db(peak), peak_to_db(self.hold));
    }
}

fn paint_gyr_strip(painter: &egui::Painter, rect: egui::Rect, db: f32, hold: f32) {
    painter.rect_filled(rect, 3.0, LANE);
    let rail = rect.shrink(1.0);
    let stops = [
        (0.00, egui::Color32::from_rgb(61, 143, 106)),
        (0.42, OK),
        (0.78, WARN),
        (1.00, HOT),
    ];
    let mut mesh = egui::Mesh::default();
    for (t, color) in stops {
        let x = rail.left() + rail.width() * t;
        mesh.colored_vertex(egui::pos2(x, rail.top()), color);
        mesh.colored_vertex(egui::pos2(x, rail.bottom()), color);
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
                egui::pos2(rail.left() + rail.width() * pos, rail.top()),
                rail.max,
            ),
            0.0,
            LANE,
        );
    }
    painter.vline(
        rail.left() + rail.width() * db_to_pos(hold),
        rail.y_range(),
        egui::Stroke::new(1.4, TEXT),
    );
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

fn status_line(
    linked: bool,
    web_on: bool,
    web_ok: bool,
    silent: bool,
    web: u32,
    lan: usize,
    lan_browsers: u32,
) -> String {
    if !linked {
        return "Off".into();
    }
    let n = web as usize + lan + lan_browsers as usize;
    if !web_on {
        return if n == 0 {
            "LAN only".into()
        } else {
            format!("LAN · {n} listening")
        };
    }
    if !web_ok {
        return "Web offline".into();
    }
    if silent && n == 0 {
        return "Silent".into();
    }
    if n == 0 {
        "Live".into()
    } else {
        format!("Live · {n} listening")
    }
}

fn public_url(name: &str) -> String {
    format!("{PUBLIC_LINK_ORIGIN}/{}", normalize_slug(name))
}

fn lan_url_for(ctx: &PluginContext<RelayParams>) -> Option<String> {
    let name = ctx.params().control.session_name().ok()?;
    lan_listen_url(&name, ctx.params().control.lan_http_port())
}

fn listen_url(ctx: &PluginContext<RelayParams>, name: &str) -> String {
    if ctx.params().web.value() {
        public_url(name)
    } else {
        lan_url_for(ctx).unwrap_or_else(|| public_url(name))
    }
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

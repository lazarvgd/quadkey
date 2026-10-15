//! The user interface (egui, immediate mode: the whole UI is described again every frame).

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, StrokeKind,
    Vec2,
};

use crate::backend::{self, Environment};
use crate::device::DockEvent;
use crate::model::{
    Action, Config, DEVICE_NAME, Media, key_name_from_egui, presets, shortcut_keys,
};

const BG: Color32 = Color32::from_rgb(0x0F, 0x11, 0x15);
const PANEL: Color32 = Color32::from_rgb(0x17, 0x1A, 0x21);
const CARD: Color32 = Color32::from_rgb(0x1F, 0x23, 0x2C);
const CARD_HOVER: Color32 = Color32::from_rgb(0x26, 0x2B, 0x36);
const BORDER: Color32 = Color32::from_rgb(0x2C, 0x31, 0x3D);
const ACCENT: Color32 = Color32::from_rgb(0x5B, 0x8C, 0xFF);
const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEE);
const MUTED: Color32 = Color32::from_rgb(0x8A, 0x90, 0xA0);
const OK: Color32 = Color32::from_rgb(0x3E, 0xCF, 0x8E);
const WARN: Color32 = Color32::from_rgb(0xF5, 0xA5, 0x24);
const ERR: Color32 = Color32::from_rgb(0xF2, 0x5F, 0x5C);

/// How long a pressed tile stays lit, in seconds.
const GLOW_SECONDS: f64 = 0.9;
const LOG_LINES: usize = 60;
const POSITIONS: [&str; 4] = ["Top left", "Top right", "Bottom left", "Bottom right"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Original,
    Disabled,
    Media,
    Shortcut,
    OpenApp,
    Command,
}

impl ActionKind {
    pub const ALL: [ActionKind; 6] = [
        ActionKind::Original,
        ActionKind::Disabled,
        ActionKind::Media,
        ActionKind::Shortcut,
        ActionKind::OpenApp,
        ActionKind::Command,
    ];

    pub fn of(action: &Action) -> Self {
        match action {
            Action::Original => ActionKind::Original,
            Action::Disabled => ActionKind::Disabled,
            Action::Media { .. } => ActionKind::Media,
            Action::Shortcut { .. } => ActionKind::Shortcut,
            Action::OpenApp { .. } => ActionKind::OpenApp,
            Action::Command { .. } => ActionKind::Command,
        }
    }

    pub fn default_action(self) -> Action {
        match self {
            ActionKind::Original => Action::Original,
            ActionKind::Disabled => Action::Disabled,
            ActionKind::Media => Action::Media {
                key: Media::PlayPause,
            },
            ActionKind::Shortcut => Action::Shortcut {
                ctrl: false,
                alt: false,
                shift: true,
                cmd: true,
                key: "5".into(),
            },
            ActionKind::OpenApp => Action::OpenApp { app: String::new() },
            ActionKind::Command => Action::Command {
                command: String::new(),
            },
        }
    }

    pub fn label(self) -> &'static str {
        self.default_action().kind_label()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Connection {
    Waiting,
    Connected { keyboard_ok: bool },
    Disconnected,
}

pub struct DockStudio {
    pub config: Config,
    pub selected: usize,
    pub learning: Option<usize>,
    pub connection: Connection,
    pub status: Option<(bool, String)>,
    pub log: VecDeque<String>,
    pub dirty: bool,
    /// Waiting for a key combination on the computer's keyboard.
    pub recording: bool,
    lit_at: [Option<f64>; 4],
    env: Environment,
    events: Receiver<DockEvent>,
    picked_app: Arc<Mutex<Option<String>>>,
    vendor_packets: usize,
}

impl DockStudio {
    pub fn new(env: Environment, events: Receiver<DockEvent>) -> Self {
        let config = Config::load_from(&env.config_file);
        DockStudio {
            config,
            selected: 0,
            learning: None,
            connection: Connection::Waiting,
            status: None,
            log: VecDeque::new(),
            dirty: false,
            recording: false,
            lit_at: [None; 4],
            env,
            events,
            picked_app: Arc::new(Mutex::new(None)),
            vendor_packets: 0,
        }
    }

    /// For tests and the real app alike: an event channel the HID threads write into.
    pub fn channel() -> (Sender<DockEvent>, Receiver<DockEvent>) {
        std::sync::mpsc::channel()
    }

    pub fn is_lit(&self, index: usize, now: f64) -> bool {
        self.lit_at[index].is_some_and(|t| now - t < GLOW_SECONDS)
    }

    fn push_log(&mut self, line: String) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    fn save(&mut self) {
        self.dirty = true;
        if let Err(err) = self.config.save_to(&self.env.config_file) {
            self.status = Some((false, format!("Could not save settings: {err}")));
        }
    }

    /// Drain the events coming from the HID and UDP threads.
    pub fn process_events(&mut self, now: f64) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                DockEvent::Connected {
                    interfaces,
                    keyboard_ok,
                } => {
                    self.connection = Connection::Connected { keyboard_ok };
                    self.push_log(format!("Plugged in - {interfaces} HID interfaces opened"));
                }
                DockEvent::Disconnected => {
                    self.connection = Connection::Disconnected;
                    self.push_log("Unplugged".into());
                }
                DockEvent::Pressed(trigger) => {
                    if let Some(index) = self.learning.take() {
                        self.config.buttons[index].trigger = trigger;
                        self.save();
                        self.selected = index;
                        self.lit_at[index] = Some(now);
                        self.push_log(format!("Button {} learned: {}", index + 1, trigger.label()));
                        continue;
                    }
                    match self.config.button_for(&trigger) {
                        Some(index) => {
                            self.lit_at[index] = Some(now);
                            backend::run_local_action(&self.config.buttons[index].action);
                            self.push_log(format!("Button {}  {}", index + 1, trigger.label()));
                        }
                        None => self.push_log(format!("Unknown input  {}", trigger.label())),
                    }
                }
                DockEvent::PressedButton(index) => {
                    if index < 4 {
                        self.lit_at[index] = Some(now);
                        self.push_log(format!("Button {}  (via Karabiner)", index + 1));
                    }
                }
                DockEvent::Released => {}
                DockEvent::Vendor(_) => self.vendor_packets += 1,
            }
        }
        if let Some(app) = self.picked_app.lock().ok().and_then(|mut p| p.take()) {
            self.config.buttons[self.selected].action = Action::OpenApp { app };
            self.save();
        }
    }

    pub fn apply(&mut self) {
        self.status = Some(match self.env.apply(&self.config) {
            Ok(message) => {
                self.dirty = false;
                (true, message)
            }
            Err(err) => (false, err.to_string()),
        });
    }

    pub fn remove_rule(&mut self) {
        self.status = Some(match self.env.remove() {
            Ok(message) => (true, message),
            Err(err) => (false, err.to_string()),
        });
    }

    // ------------------------------------------------------------------ drawing

    /// While recording, the next key combination typed on the keyboard becomes the action.
    fn record_shortcut(&mut self, ui: &egui::Ui) {
        if !self.recording {
            return;
        }
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, mods)) = pressed else { return };
        self.recording = false;
        if key == egui::Key::Escape && !mods.any() {
            return; // cancelled
        }
        if let Some(name) = key_name_from_egui(key) {
            self.config.buttons[self.selected].action = Action::Shortcut {
                ctrl: mods.ctrl,
                alt: mods.alt,
                shift: mods.shift,
                cmd: mods.mac_cmd,
                key: name,
            };
            self.save();
        }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        self.process_events(now);
        self.record_shortcut(ui);
        if self.lit_at.iter().flatten().any(|t| now - t < GLOW_SECONDS) {
            ui.ctx().request_repaint(); // keep animating the glow
        }

        egui::Panel::top("header")
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(20, 14)),
            )
            .show(ui, |ui| self.header(ui));
        egui::Panel::bottom("log")
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(20, 10)),
            )
            .resizable(false)
            .show(ui, |ui| {
                self.action_bar(ui);
                ui.add_space(8.0);
                ui.separator();
                self.activity(ui);
            });
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG).inner_margin(Margin::same(20)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            self.cube(ui, now);
                            ui.add_space(20.0);
                            self.editor(ui);
                        });
                    });
            });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("QuadKey").size(22.0).strong().color(TEXT));
            ui.add_space(8.0);
            ui.label(RichText::new(DEVICE_NAME).size(13.0).color(MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (color, text) = match &self.connection {
                    Connection::Connected { keyboard_ok: true } => {
                        (OK, "Dock connected".to_string())
                    }
                    Connection::Connected { keyboard_ok: false } => (
                        WARN,
                        "Connected - allow Input Monitoring to see all buttons".to_string(),
                    ),
                    Connection::Disconnected => (ERR, "Dock not connected".to_string()),
                    Connection::Waiting => (MUTED, "Looking for the dock...".to_string()),
                };
                chip(ui, color, &text);
                if cfg!(target_os = "macos") {
                    let (color, text) = if !self.env.karabiner_installed {
                        (WARN, "Karabiner-Elements not installed")
                    } else if self.env.rule_active() {
                        (OK, "Karabiner rule active")
                    } else {
                        (MUTED, "Karabiner rule not applied")
                    };
                    chip(ui, color, text);
                }
            });
        });
    }

    fn cube(&mut self, ui: &mut egui::Ui, now: f64) {
        let tile = Vec2::new(176.0, 150.0);
        Frame::new()
            .fill(PANEL)
            .corner_radius(CornerRadius::same(26))
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(10.0);
                ui.vertical(|ui| {
                    for row in 0..2 {
                        ui.horizontal(|ui| {
                            for col in 0..2 {
                                let index = row * 2 + col;
                                self.tile(ui, index, tile, now);
                            }
                        });
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Press a button on the dock - it lights up here.")
                            .size(12.0)
                            .color(MUTED),
                    );
                });
            });
    }

    fn tile(&mut self, ui: &mut egui::Ui, index: usize, size: Vec2, now: f64) {
        let button = &self.config.buttons[index];
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let label = format!("Button {}", index + 1);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));

        let glow = self.lit_at[index].map_or(0.0, |t| {
            (1.0 - ((now - t) / GLOW_SECONDS)).clamp(0.0, 1.0) as f32
        });
        let selected = self.selected == index;
        let learning = self.learning == Some(index);
        let base = if response.hovered() { CARD_HOVER } else { CARD };
        let fill = lerp(base, ACCENT.gamma_multiply(0.55), glow);
        let stroke = if learning {
            Stroke::new(2.0, WARN)
        } else if selected {
            Stroke::new(2.0, ACCENT)
        } else {
            Stroke::new(1.0, BORDER)
        };
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(18), fill);
        painter.rect_stroke(rect, CornerRadius::same(18), stroke, StrokeKind::Inside);

        let pad = 14.0;
        painter.text(
            rect.left_top() + Vec2::new(pad, pad),
            egui::Align2::LEFT_TOP,
            format!("{}", index + 1),
            egui::FontId::proportional(13.0),
            if selected { ACCENT } else { MUTED },
        );
        painter.text(
            rect.center() - Vec2::new(0.0, 6.0),
            egui::Align2::CENTER_CENTER,
            truncate(&tile_title(button), 18),
            egui::FontId::proportional(17.0),
            TEXT,
        );
        let sub = if learning {
            "Press it on the dock...".to_string()
        } else if button.action == Action::Original {
            "original".to_string()
        } else {
            truncate(&button.trigger.label(), 24)
        };
        painter.text(
            rect.center_bottom() - Vec2::new(0.0, pad + 6.0),
            egui::Align2::CENTER_CENTER,
            sub,
            egui::FontId::proportional(12.0),
            if learning { WARN } else { MUTED },
        );
        if response.clicked() {
            self.selected = index;
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let index = self.selected;
        Frame::new()
            .fill(PANEL)
            .corner_radius(CornerRadius::same(18))
            .stroke(Stroke::new(1.0, BORDER))
            .inner_margin(Margin::same(20))
            .show(ui, |ui| {
                // The parent row is horizontal (cube | editor) - lay this card out top to bottom
                ui.vertical(|ui| self.editor_contents(ui, index));
            });
    }

    fn editor_contents(&mut self, ui: &mut egui::Ui, index: usize) {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new(format!("Button {} · {}", index + 1, POSITIONS[index]))
                .size(18.0)
                .strong()
                .color(TEXT),
        );
        ui.add_space(12.0);

        let trigger = self.config.buttons[index].trigger;
        field(ui, "Dock sends");
        ui.horizontal(|ui| {
            ui.label(RichText::new(trigger.label()).monospace().color(TEXT));
            if let Some(meaning) = trigger.meaning() {
                let os = if cfg!(target_os = "macos") {
                    "macOS"
                } else {
                    "Linux"
                };
                chip(ui, WARN, &format!("{os}: {meaning}"));
            }
        });
        ui.add_space(12.0);

        field(ui, "Action");
        let current = ActionKind::of(&self.config.buttons[index].action);
        let mut chosen = current;
        egui::ComboBox::from_id_salt("action_kind")
            .selected_text(current.label())
            .width(260.0)
            .show_ui(ui, |ui| {
                for kind in ActionKind::ALL {
                    ui.selectable_value(&mut chosen, kind, kind.label());
                }
            });
        if chosen != current {
            self.config.buttons[index].action = chosen.default_action();
            self.save();
        }
        ui.add_space(10.0);
        self.action_details(ui, index);

        ui.add_space(14.0);
        field(ui, "Quick picks");
        self.quick_picks(ui, index);

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            let learning = self.learning == Some(index);
            let text = if learning {
                "Cancel learning"
            } else {
                "Learn from dock"
            };
            if ui.add(secondary_button(text)).clicked() {
                self.learning = if learning { None } else { Some(index) };
            }
            if ui.add(secondary_button("Reset button")).clicked() {
                let defaults = Config::default();
                self.config.buttons[index] = defaults.buttons[index].clone();
                self.save();
            }
        });
    }

    /// Always-visible bar with Apply / Remove and the result of the last action.
    fn action_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let apply = egui::Button::new(
                RichText::new("Apply")
                    .size(15.0)
                    .strong()
                    .color(Color32::WHITE),
            )
            .fill(ACCENT)
            .corner_radius(CornerRadius::same(10))
            .min_size(Vec2::new(120.0, 36.0));
            if ui.add(apply).clicked() {
                self.apply();
            }
            if cfg!(target_os = "macos") && ui.add(secondary_button("Remove rule")).clicked() {
                self.remove_rule();
            }
            if self.dirty {
                ui.label(RichText::new("Unapplied changes").size(12.0).color(WARN));
            }
        });
        if cfg!(target_os = "macos") && !self.env.karabiner_installed {
            ui.add_space(10.0);
            ui.label(
                    RichText::new(
                        "QuadKey uses Karabiner-Elements to take over the dock, so its shortcuts (lock screen, screenshot) stop reaching macOS.",
                    )
                    .size(12.0)
                    .color(MUTED),
                );
            if ui.link("Install Karabiner-Elements").clicked() {
                let _ = std::process::Command::new("open")
                    .arg("https://karabiner-elements.pqrs.org/")
                    .spawn();
            }
        }
        if let Some((ok, message)) = &self.status {
            ui.add_space(10.0);
            ui.label(
                RichText::new(message)
                    .size(13.0)
                    .color(if *ok { OK } else { ERR }),
            );
        }
    }

    /// One click assigns a common action; recording takes any shortcut from the keyboard.
    fn quick_picks(&mut self, ui: &mut egui::Ui, index: usize) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
            let recording = self.recording;
            let record_text = if recording {
                "Press keys now... (Esc cancels)"
            } else {
                "Record shortcut"
            };
            let record =
                egui::Button::new(RichText::new(record_text).size(12.0).color(if recording {
                    WARN
                } else {
                    TEXT
                }))
                .fill(if recording {
                    WARN.gamma_multiply(0.15)
                } else {
                    CARD
                })
                .stroke(Stroke::new(1.0, if recording { WARN } else { ACCENT }))
                .corner_radius(CornerRadius::same(14));
            if ui.add(record).clicked() {
                self.recording = !self.recording;
            }
            for (name, action) in presets() {
                let active = self.config.buttons[index].action == action;
                let chip = egui::Button::new(RichText::new(name).size(12.0).color(if active {
                    Color32::WHITE
                } else {
                    TEXT
                }))
                .fill(if active { ACCENT } else { CARD })
                .stroke(Stroke::new(1.0, if active { ACCENT } else { BORDER }))
                .corner_radius(CornerRadius::same(14));
                if ui.add(chip).clicked() {
                    self.config.buttons[index].action = action;
                    self.recording = false;
                    self.save();
                }
            }
        });
    }

    fn action_details(&mut self, ui: &mut egui::Ui, index: usize) {
        let mut changed = false;
        let picked = self.picked_app.clone();
        match &mut self.config.buttons[index].action {
            Action::Original | Action::Disabled => {
                let text = if matches!(self.config.buttons[index].action, Action::Original) {
                    "The button keeps doing what the dock sends."
                } else {
                    "The button does nothing."
                };
                ui.label(RichText::new(text).size(12.0).color(MUTED));
            }
            Action::Media { key } => {
                egui::ComboBox::from_id_salt("media_key")
                    .selected_text(key.label())
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for media in Media::ALL {
                            changed |= ui.selectable_value(key, media, media.label()).changed();
                        }
                    });
            }
            Action::Shortcut {
                ctrl,
                alt,
                shift,
                cmd,
                key,
            } => {
                ui.horizontal(|ui| {
                    let names = if cfg!(target_os = "macos") {
                        ["Ctrl", "Option", "Shift", "Cmd"]
                    } else {
                        ["Ctrl", "Alt", "Shift", "Super"]
                    };
                    for (flag, name) in [ctrl, alt, shift, cmd].into_iter().zip(names) {
                        changed |= ui.toggle_value(flag, name).changed();
                    }
                    ui.label(RichText::new("+").color(MUTED));
                    egui::ComboBox::from_id_salt("shortcut_key")
                        .selected_text(key.to_uppercase())
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for name in shortcut_keys() {
                                let label = name.to_uppercase();
                                changed |= ui.selectable_value(key, name, label).changed();
                            }
                        });
                });
            }
            Action::OpenApp { app } => {
                ui.horizontal(|ui| {
                    let hint = if cfg!(target_os = "macos") {
                        "Zoom or /Applications/Zoom.app"
                    } else {
                        "firefox"
                    };
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(app)
                                .hint_text(hint)
                                .desired_width(260.0),
                        )
                        .changed();
                    if cfg!(target_os = "macos") && ui.add(secondary_button("Choose...")).clicked()
                    {
                        choose_app(picked);
                    }
                });
            }
            Action::Command { command } => {
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(command)
                            .hint_text("say hello")
                            .desired_width(320.0),
                    )
                    .changed();
            }
        }
        if changed {
            self.save();
        }
    }

    fn activity(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Activity").size(13.0).strong().color(TEXT));
            if self.vendor_packets > 0 {
                ui.label(
                    RichText::new(format!("· {} vendor packets", self.vendor_packets))
                        .size(12.0)
                        .color(MUTED),
                );
            }
        });
        egui::ScrollArea::vertical()
            .max_height(90.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.log.is_empty() {
                    ui.label(RichText::new("Nothing yet.").size(12.0).color(MUTED));
                }
                for line in &self.log {
                    ui.label(RichText::new(line).monospace().size(12.0).color(MUTED));
                }
            });
    }
}

impl eframe::App for DockStudio {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

pub fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = CARD;
    visuals.selection.bg_fill = ACCENT;
    visuals.hyperlink_color = ACCENT;
    visuals.widgets.inactive.bg_fill = CARD;
    visuals.widgets.inactive.weak_bg_fill = CARD;
    visuals.widgets.hovered.weak_bg_fill = CARD_HOVER;
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);
    ctx.set_visuals(visuals);
}

fn chip(ui: &mut egui::Ui, color: Color32, text: &str) {
    Frame::new()
        .fill(color.gamma_multiply(0.15))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // `horizontal` follows the parent's direction; in the right-to-left header the
                // first widget ends up on the right, so add the text first to keep the dot left
                let rtl = ui.layout().main_dir() == egui::Direction::RightToLeft;
                let dot = |ui: &mut egui::Ui| {
                    // A painted dot - the default font has no bullet glyph
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.5, color);
                };
                if !rtl {
                    dot(ui);
                }
                ui.label(RichText::new(text).size(12.0).color(color));
                if rtl {
                    dot(ui);
                }
            });
        });
}

/// What the tile shows: the action, or for "keep original" what the dock already does.
fn tile_title(button: &crate::model::Button) -> String {
    match button.action {
        Action::Original => button
            .trigger
            .meaning()
            .map_or_else(|| button.trigger.label(), str::to_string),
        _ => button.action.summary(),
    }
}

fn field(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).size(11.0).color(MUTED));
    ui.add_space(2.0);
}

fn secondary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).color(TEXT))
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .min_size(Vec2::new(0.0, 32.0))
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max - 1).collect::<String>())
    }
}

/// macOS application picker in a background thread (the dialog is modal).
fn choose_app(result: Arc<Mutex<Option<String>>>) {
    std::thread::spawn(move || {
        let output = std::process::Command::new("osascript")
            .args([
                "-e",
                "POSIX path of (choose application with prompt \"Choose an app for this button\")",
            ])
            .output();
        if let Ok(output) = output {
            let path = String::from_utf8_lossy(&output.stdout)
                .trim()
                .trim_end_matches('/')
                .to_string();
            if !path.is_empty()
                && let Ok(mut slot) = result.lock()
            {
                *slot = Some(path);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_kind_roundtrip() {
        for kind in ActionKind::ALL {
            assert_eq!(ActionKind::of(&kind.default_action()), kind);
        }
    }

    #[test]
    fn helpers() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
        assert_eq!(
            lerp(Color32::BLACK, Color32::WHITE, 0.5),
            Color32::from_rgb(128, 128, 128)
        );
    }
}

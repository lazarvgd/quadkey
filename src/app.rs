//! The user interface (egui, immediate mode: the whole UI is described again every frame).
//!
//! The look follows Ubuntu's Yaru / GNOME libadwaita style: a flat header bar, grouped cards,
//! generous spacing, the Ubuntu typeface and a single orange accent, in light and dark.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Layout, Margin,
    RichText, Sense, Stroke, StrokeKind, Vec2,
};
use egui_phosphor::regular as icon;

use crate::backend::{self, Environment};
use crate::device::DockEvent;
use crate::model::{
    Action, Button, Category, Config, DEVICE_NAME, Media, TargetOs, key_name_from_egui,
    preset_for_action, presets_for, shortcut_keys,
};

/// How long a pressed tile stays lit, in seconds.
const GLOW_SECONDS: f64 = 0.9;
const LOG_LINES: usize = 60;
const POSITIONS: [&str; 4] = ["Top left", "Top right", "Bottom left", "Bottom right"];
const ACCENT: Color32 = Color32::from_rgb(0xE9, 0x54, 0x20); // Ubuntu orange
const MEDIUM: &str = "medium";

/// Colors for the current theme (light or dark), in the spirit of libadwaita.
#[derive(Clone, Copy)]
struct Palette {
    window: Color32,
    header: Color32,
    card: Color32,
    card_hover: Color32,
    border: Color32,
    text: Color32,
    dim: Color32,
    accent_tint: Color32,
    ok: Color32,
    warn: Color32,
    err: Color32,
    shadow: Color32,
}

impl Palette {
    fn of(ui: &egui::Ui) -> Self {
        if ui.visuals().dark_mode {
            Palette {
                window: Color32::from_rgb(0x24, 0x24, 0x24),
                header: Color32::from_rgb(0x2C, 0x2C, 0x2C),
                card: Color32::from_rgb(0x30, 0x30, 0x30),
                card_hover: Color32::from_rgb(0x3A, 0x3A, 0x3A),
                border: Color32::from_white_alpha(18),
                text: Color32::from_rgb(0xF6, 0xF5, 0xF4),
                dim: Color32::from_rgb(0x9A, 0x99, 0x96),
                accent_tint: Color32::from_rgb(0x4A, 0x2B, 0x20),
                ok: Color32::from_rgb(0x57, 0xE3, 0x89),
                warn: Color32::from_rgb(0xF8, 0xE4, 0x5C),
                err: Color32::from_rgb(0xFF, 0x7B, 0x63),
                shadow: Color32::from_black_alpha(60),
            }
        } else {
            Palette {
                window: Color32::from_rgb(0xFA, 0xFA, 0xFA),
                header: Color32::from_rgb(0xEB, 0xEB, 0xEB),
                card: Color32::WHITE,
                card_hover: Color32::from_rgb(0xF4, 0xF4, 0xF4),
                border: Color32::from_black_alpha(22),
                text: Color32::from_rgb(0x1E, 0x1E, 0x1E),
                dim: Color32::from_rgb(0x6E, 0x6E, 0x6E),
                accent_tint: Color32::from_rgb(0xFC, 0xE8, 0xE0),
                ok: Color32::from_rgb(0x26, 0xA2, 0x69),
                warn: Color32::from_rgb(0xAE, 0x7B, 0x03),
                err: Color32::from_rgb(0xC0, 0x1C, 0x28),
                shadow: Color32::from_black_alpha(18),
            }
        }
    }
}

fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
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
    pub tab: Category,
    pub help_open: bool,
    pub show_activity: bool,
    lit_at: [Option<f64>; 4],
    env: Environment,
    events: Receiver<DockEvent>,
    picked_app: Arc<Mutex<Option<String>>>,
    vendor_packets: usize,
}

impl DockStudio {
    pub fn new(env: Environment, events: Receiver<DockEvent>) -> Self {
        let config = Config::load_from(&env.config_file);
        let tab = tab_for(&config.buttons[0], config.target_os);
        DockStudio {
            config,
            selected: 0,
            learning: None,
            connection: Connection::Waiting,
            status: None,
            log: VecDeque::new(),
            dirty: false,
            recording: false,
            tab,
            help_open: false,
            show_activity: false,
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
        self.persist();
    }

    fn persist(&mut self) {
        if let Err(err) = self.config.save_to(&self.env.config_file) {
            self.status = Some((false, format!("Could not save settings: {err}")));
        }
    }

    fn select(&mut self, index: usize) {
        self.selected = index;
        self.tab = tab_for(&self.config.buttons[index], self.config.target_os);
        self.recording = false;
    }

    fn assign(&mut self, action: Action) {
        self.config.buttons[self.selected].action = action;
        self.recording = false;
        self.save();
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
            self.assign(Action::OpenApp { app });
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
            self.assign(Action::Shortcut {
                ctrl: mods.ctrl,
                alt: mods.alt,
                shift: mods.shift,
                cmd: mods.mac_cmd,
                key: name,
            });
        }
    }

    // ------------------------------------------------------------------ drawing

    pub fn draw(&mut self, ui: &mut egui::Ui) {
        // Fonts set with `style` take effect from the next frame; never paint with a missing family
        let medium_loaded = ui.ctx().fonts(|f| {
            f.families()
                .iter()
                .any(|family| *family == FontFamily::Name(MEDIUM.into()))
        });
        if !medium_loaded {
            style(ui.ctx());
            ui.ctx().request_repaint();
            return;
        }
        let now = ui.input(|i| i.time);
        self.process_events(now);
        self.record_shortcut(ui);
        if self.lit_at.iter().flatten().any(|t| now - t < GLOW_SECONDS) {
            ui.ctx().request_repaint(); // keep animating the glow
        }
        let pal = Palette::of(ui);

        egui::Panel::top("header")
            .frame(
                Frame::new()
                    .fill(pal.header)
                    .inner_margin(Margin::symmetric(20, 12)),
            )
            .show(ui, |ui| self.header(ui, pal));
        egui::Panel::bottom("footer")
            .frame(
                Frame::new()
                    .fill(pal.header)
                    .inner_margin(Margin::symmetric(20, 12)),
            )
            .resizable(false)
            .show(ui, |ui| self.footer(ui, pal));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(pal.window).inner_margin(Margin::same(24)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 24.0;
                            self.dock_card(ui, pal, now);
                            ui.vertical(|ui| self.editor(ui, pal));
                        });
                    });
            });

        self.guide(ui.ctx(), pal);
    }

    fn header(&mut self, ui: &mut egui::Ui, pal: Palette) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(RichText::new(icon::SQUARES_FOUR).size(22.0).color(ACCENT));
            ui.label(RichText::new("QuadKey").font(medium(20.0)).color(pal.text));
            ui.label(RichText::new(DEVICE_NAME).size(14.0).color(pal.dim));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if ui
                    .add(pill_button(
                        &format!("{}  Help", icon::QUESTION),
                        pal,
                        false,
                    ))
                    .on_hover_text("How QuadKey works")
                    .clicked()
                {
                    self.help_open = true;
                }
                let (color, text) = match &self.connection {
                    Connection::Connected { keyboard_ok: true } => {
                        (pal.ok, "Dock connected".to_string())
                    }
                    Connection::Connected { keyboard_ok: false } => {
                        (pal.warn, "Connected - allow Input Monitoring".to_string())
                    }
                    Connection::Disconnected => (pal.err, "Dock not connected".to_string()),
                    Connection::Waiting => (pal.dim, "Looking for the dock...".to_string()),
                };
                status_pill(ui, pal, color, &text);
                if cfg!(target_os = "macos") {
                    let (color, text) = if !self.env.karabiner_installed {
                        (pal.warn, "Karabiner-Elements not installed")
                    } else if self.env.rule_active() {
                        (pal.ok, "Karabiner rule active")
                    } else {
                        (pal.dim, "Karabiner rule not applied")
                    };
                    status_pill(ui, pal, color, text);
                }
                // Which system's shortcuts the quick picks use - the user decides
                let mut os = self.config.target_os;
                for choice in [TargetOs::Linux, TargetOs::Mac] {
                    ui.selectable_value(&mut os, choice, RichText::new(choice.label()).size(13.0));
                }
                ui.label(RichText::new("Shortcuts for").size(13.0).color(pal.dim));
                if os != self.config.target_os {
                    self.config.target_os = os;
                    self.tab = tab_for(&self.config.buttons[self.selected], os);
                    self.persist();
                }
            });
        });
    }

    fn dock_card(&mut self, ui: &mut egui::Ui, pal: Palette, now: f64) {
        card(pal).inner_margin(Margin::same(18)).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Your dock")
                        .font(medium(15.0))
                        .color(pal.text),
                );
                ui.label(
                    RichText::new("Click a button to change it")
                        .size(13.0)
                        .color(pal.dim),
                );
                ui.add_space(14.0);
                ui.spacing_mut().item_spacing = Vec2::splat(12.0);
                for row in 0..2 {
                    ui.horizontal(|ui| {
                        for col in 0..2 {
                            self.tile(ui, pal, row * 2 + col, now);
                        }
                    });
                }
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!(
                        "{}  Press a button on the dock - it lights up here",
                        icon::HAND_POINTING
                    ))
                    .size(12.0)
                    .color(pal.dim),
                );
            });
        });
    }

    fn tile(&mut self, ui: &mut egui::Ui, pal: Palette, index: usize, now: f64) {
        let size = Vec2::new(168.0, 140.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        // Accessible name differs from the editor title ("Button 3") so both can be found
        let label = format!("Dock button {}", index + 1);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

        let button = &self.config.buttons[index];
        let preset = preset_for_action(&button.action, self.config.target_os);
        let glow = self.lit_at[index].map_or(0.0, |t| {
            (1.0 - ((now - t) / GLOW_SECONDS)).clamp(0.0, 1.0) as f32
        });
        let selected = self.selected == index;
        let learning = self.learning == Some(index);

        // Pressed tiles sink slightly and glow in the accent color
        let rect = rect.shrink(glow * 2.0);
        let radius = CornerRadius::same(16);
        let painter = ui.painter();
        let shadow = egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: pal.shadow,
        };
        painter.add(shadow.as_shape(rect, radius));
        let base = if response.hovered() {
            pal.card_hover
        } else {
            pal.card
        };
        let tint: f32 = if selected { 0.6 } else { 0.0 };
        let fill = lerp(base, pal.accent_tint, tint.max(glow));
        painter.rect_filled(rect, radius, fill);
        let stroke = if learning {
            Stroke::new(2.0, pal.warn)
        } else if selected || glow > 0.0 {
            Stroke::new(2.0, ACCENT)
        } else {
            Stroke::new(1.0, pal.border)
        };
        painter.rect_stroke(rect, radius, stroke, StrokeKind::Inside);

        let pad = 16.0;
        let glyph = preset
            .as_ref()
            .map_or_else(|| action_icon(&button.action), |p| p.icon);
        painter.text(
            rect.left_top() + Vec2::new(pad, pad),
            Align2::LEFT_TOP,
            glyph,
            FontId::proportional(24.0),
            if selected || glow > 0.0 {
                ACCENT
            } else {
                pal.dim
            },
        );
        painter.text(
            rect.right_top() + Vec2::new(-pad, pad + 2.0),
            Align2::RIGHT_TOP,
            format!("{}", index + 1),
            medium(13.0),
            pal.dim,
        );
        painter.text(
            rect.left_bottom() + Vec2::new(pad, -pad - 18.0),
            Align2::LEFT_BOTTOM,
            truncate(&tile_title(button, preset.as_ref().map(|p| p.name)), 17),
            medium(15.0),
            pal.text,
        );
        let sub = if learning {
            "Press it on the dock...".to_string()
        } else if button.action == Action::Original {
            "factory setting".to_string()
        } else {
            truncate(&button.trigger.label(), 22)
        };
        painter.text(
            rect.left_bottom() + Vec2::new(pad, -pad),
            Align2::LEFT_BOTTOM,
            sub,
            FontId::proportional(12.0),
            if learning { pal.warn } else { pal.dim },
        );
        if response.clicked() {
            self.select(index);
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui, pal: Palette) {
        let index = self.selected;
        ui.set_min_width(460.0);
        let trigger = self.config.buttons[index].trigger;

        // Title and what the dock sends
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Button {}", index + 1))
                    .font(medium(22.0))
                    .color(pal.text),
            );
            ui.label(RichText::new(POSITIONS[index]).size(15.0).color(pal.dim));
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("The dock sends").size(13.0).color(pal.dim));
            ui.label(
                RichText::new(trigger.label())
                    .monospace()
                    .size(13.0)
                    .color(pal.text),
            );
            if let Some(meaning) = trigger.meaning() {
                let os = if cfg!(target_os = "macos") {
                    "macOS"
                } else {
                    "Linux"
                };
                status_pill(ui, pal, pal.warn, &format!("{os}: {meaning}"));
            }
        });
        ui.add_space(16.0);

        // Current action
        card(pal)
            .inner_margin(Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let action = self.config.buttons[index].action.clone();
                    let preset = preset_for_action(&action, self.config.target_os);
                    let glyph = preset
                        .as_ref()
                        .map_or_else(|| action_icon(&action), |p| p.icon);
                    ui.label(RichText::new(glyph).size(22.0).color(ACCENT));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Current action").size(12.0).color(pal.dim));
                        let name = preset.as_ref().map_or_else(
                            || tile_title(&self.config.buttons[index], None),
                            |p| p.name.to_string(),
                        );
                        ui.label(RichText::new(name).font(medium(16.0)).color(pal.text));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(pill_button("Reset button", pal, false))
                            .on_hover_text("Back to the factory behaviour")
                            .clicked()
                        {
                            let defaults = Config::default();
                            self.config.buttons[index] = defaults.buttons[index].clone();
                            self.tab = tab_for(&self.config.buttons[index], self.config.target_os);
                            self.save();
                        }
                        let learning = self.learning == Some(index);
                        let text = if learning {
                            "Cancel learning"
                        } else {
                            "Learn from dock"
                        };
                        if ui
                            .add(pill_button(text, pal, false))
                            .on_hover_text("Press this button on the dock so QuadKey recognises it")
                            .clicked()
                        {
                            self.learning = if learning { None } else { Some(index) };
                        }
                    });
                });
            });
        ui.add_space(20.0);

        // Category tabs
        ui.label(
            RichText::new("Choose what the button does")
                .font(medium(15.0))
                .color(pal.text),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            for category in Category::ALL {
                let active = self.tab == category;
                let text = format!("{}  {}", category.icon(), category.label());
                if ui.add(pill_button(&text, pal, active)).clicked() {
                    self.tab = category;
                }
            }
        });
        ui.add_space(12.0);

        if self.tab == Category::Custom {
            self.custom(ui, pal, index);
        } else {
            self.preset_grid(ui, pal, index);
        }
    }

    /// One click assigns a common action.
    fn preset_grid(&mut self, ui: &mut egui::Ui, pal: Palette, index: usize) {
        let os = self.config.target_os;
        let presets: Vec<_> = presets_for(os)
            .into_iter()
            .filter(|(p, _)| p.category == self.tab)
            .collect();
        let columns = 2;
        let width = ((ui.available_width() - 12.0) / columns as f32).max(200.0);
        egui::Grid::new(("presets", self.tab as u8))
            .num_columns(columns)
            .spacing(Vec2::splat(12.0))
            .show(ui, |ui| {
                for (i, (preset, action)) in presets.into_iter().enumerate() {
                    let active = self.config.buttons[index].action == action;
                    // Shortcuts show their keys; media keys are already described by the hint
                    let hint = match &action {
                        Action::Shortcut { .. } => {
                            format!("{} · {}", preset.hint, action.summary())
                        }
                        _ => preset.hint.to_string(),
                    };
                    if option_card(ui, pal, preset.icon, preset.name, &hint, active, width)
                        .clicked()
                    {
                        self.assign(action);
                    }
                    if i % columns == columns - 1 {
                        ui.end_row();
                    }
                }
            });
    }

    fn custom(&mut self, ui: &mut egui::Ui, pal: Palette, index: usize) {
        let width = ((ui.available_width() - 12.0) / 2.0).max(200.0);
        let action = self.config.buttons[index].action.clone();
        let is = |kind: fn(&Action) -> bool| kind(&action);
        egui::Grid::new("custom")
            .num_columns(2)
            .spacing(Vec2::splat(12.0))
            .show(ui, |ui| {
                let recording = self.recording;
                let (title, hint) = if recording {
                    ("Press keys now...", "Type the combination - Esc cancels")
                } else {
                    ("Record shortcut", "Type any key combination to send")
                };
                if option_card(ui, pal, icon::KEYBOARD, title, hint, recording, width).clicked() {
                    self.recording = !self.recording;
                }
                if option_card(
                    ui,
                    pal,
                    icon::KEYBOARD,
                    "Choose keys",
                    "Pick modifiers and a key",
                    is(|a| matches!(a, Action::Shortcut { .. })),
                    width,
                )
                .clicked()
                    && !matches!(action, Action::Shortcut { .. })
                {
                    self.assign(Action::Shortcut {
                        ctrl: false,
                        alt: false,
                        shift: true,
                        cmd: true,
                        key: "5".into(),
                    });
                }
                ui.end_row();
                if option_card(
                    ui,
                    pal,
                    icon::APP_WINDOW,
                    "Open application",
                    "Start or bring up an app",
                    is(|a| matches!(a, Action::OpenApp { .. })),
                    width,
                )
                .clicked()
                    && !matches!(action, Action::OpenApp { .. })
                {
                    self.assign(Action::OpenApp { app: String::new() });
                }
                if option_card(
                    ui,
                    pal,
                    icon::TERMINAL,
                    "Run command",
                    "Any shell command you write",
                    is(|a| matches!(a, Action::Command { .. })),
                    width,
                )
                .clicked()
                    && !matches!(action, Action::Command { .. })
                {
                    self.assign(Action::Command {
                        command: String::new(),
                    });
                }
                ui.end_row();
                if option_card(
                    ui,
                    pal,
                    icon::PROHIBIT,
                    "Do nothing",
                    "Ignore the button completely",
                    is(|a| *a == Action::Disabled),
                    width,
                )
                .clicked()
                {
                    self.assign(Action::Disabled);
                }
                if option_card(
                    ui,
                    pal,
                    icon::ARROW_COUNTER_CLOCKWISE,
                    "Factory setting",
                    "Keep what the dock sends by itself",
                    is(|a| *a == Action::Original),
                    width,
                )
                .clicked()
                {
                    self.assign(Action::Original);
                }
                ui.end_row();
            });
        ui.add_space(12.0);
        self.custom_details(ui, pal, index);
    }

    fn custom_details(&mut self, ui: &mut egui::Ui, pal: Palette, index: usize) {
        let mut changed = false;
        let picked = self.picked_app.clone();
        let target_mac = self.config.target_os == TargetOs::Mac;
        match &mut self.config.buttons[index].action {
            Action::Shortcut {
                ctrl,
                alt,
                shift,
                cmd,
                key,
            } => {
                card(pal).inner_margin(Margin::same(14)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Keys to send").size(12.0).color(pal.dim));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let names = if target_mac {
                            ["Ctrl", "Option", "Shift", "Cmd"]
                        } else {
                            ["Ctrl", "Alt", "Shift", "Super"]
                        };
                        for (flag, name) in [ctrl, alt, shift, cmd].into_iter().zip(names) {
                            changed |= ui
                                .toggle_value(flag, RichText::new(name).size(13.0))
                                .changed();
                        }
                        ui.label(RichText::new("+").color(pal.dim));
                        egui::ComboBox::from_id_salt("shortcut_key")
                            .selected_text(key.to_uppercase())
                            .width(130.0)
                            .show_ui(ui, |ui| {
                                for name in shortcut_keys() {
                                    let label = name.to_uppercase();
                                    changed |= ui.selectable_value(key, name, label).changed();
                                }
                            });
                    });
                });
            }
            Action::OpenApp { app } => {
                card(pal).inner_margin(Margin::same(14)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Application").size(12.0).color(pal.dim));
                    ui.add_space(6.0);
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
                                    .desired_width(280.0),
                            )
                            .changed();
                        if cfg!(target_os = "macos")
                            && ui.add(pill_button("Choose...", pal, false)).clicked()
                        {
                            choose_app(picked);
                        }
                    });
                });
            }
            Action::Command { command } => {
                card(pal).inner_margin(Margin::same(14)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Shell command").size(12.0).color(pal.dim));
                    ui.add_space(6.0);
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(command)
                                .hint_text("e.g. open ~/Downloads")
                                .desired_width(f32::INFINITY),
                        )
                        .changed();
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Runs with your user's shell when the button is pressed.")
                            .size(12.0)
                            .color(pal.dim),
                    );
                });
            }
            Action::Media { key } => {
                // Reached only when a media key was set before the catalog existed
                egui::ComboBox::from_id_salt("media_key")
                    .selected_text(key.label())
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for media in Media::ALL {
                            changed |= ui.selectable_value(key, media, media.label()).changed();
                        }
                    });
            }
            Action::Original | Action::Disabled => {}
        }
        if changed {
            self.save();
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui, pal: Palette) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let apply = egui::Button::new(
                RichText::new("Apply")
                    .font(medium(15.0))
                    .color(Color32::WHITE),
            )
            .fill(ACCENT)
            .corner_radius(CornerRadius::same(18))
            .min_size(Vec2::new(120.0, 38.0));
            if ui
                .add(apply)
                .on_hover_text("Make the dock use these actions")
                .clicked()
            {
                self.apply();
            }
            if cfg!(target_os = "macos")
                && ui
                    .add(pill_button("Remove rule", pal, false))
                    .on_hover_text("Give the dock its factory behaviour back")
                    .clicked()
            {
                self.remove_rule();
            }
            if self.dirty {
                ui.label(
                    RichText::new(format!("{}  Unapplied changes", icon::DOT_OUTLINE))
                        .size(13.0)
                        .color(pal.warn),
                );
            }
            if let Some((ok, message)) = &self.status {
                ui.label(RichText::new(message).size(13.0).color(if *ok {
                    pal.ok
                } else {
                    pal.err
                }));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let text = if self.show_activity {
                    "Hide activity"
                } else {
                    "Activity"
                };
                if ui
                    .add(pill_button(
                        &format!("{}  {text}", icon::LIST_BULLETS),
                        pal,
                        self.show_activity,
                    ))
                    .clicked()
                {
                    self.show_activity = !self.show_activity;
                }
            });
        });
        if cfg!(target_os = "macos") && !self.env.karabiner_installed {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(
                        "QuadKey needs Karabiner-Elements to take over the dock on macOS.",
                    )
                    .size(12.0)
                    .color(pal.dim),
                );
                if ui.link("Install Karabiner-Elements").clicked() {
                    let _ = std::process::Command::new("open")
                        .arg("https://karabiner-elements.pqrs.org/")
                        .spawn();
                }
            });
        }
        if self.show_activity {
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(100.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if self.log.is_empty() {
                        ui.label(
                            RichText::new("Nothing yet - press a button on the dock.")
                                .size(12.0)
                                .color(pal.dim),
                        );
                    }
                    for line in &self.log {
                        ui.label(RichText::new(line).monospace().size(12.0).color(pal.dim));
                    }
                    if self.vendor_packets > 0 {
                        ui.label(
                            RichText::new(format!("{} vendor packets", self.vendor_packets))
                                .size(12.0)
                                .color(pal.dim),
                        );
                    }
                });
        }
    }

    /// Welcome guide on first start, and the Help dialog afterwards.
    fn guide(&mut self, ctx: &egui::Context, pal: Palette) {
        let first_run = !self.config.seen_guide;
        if !first_run && !self.help_open {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("guide"))
            .frame(card(pal).inner_margin(Margin::same(28)).corner_radius(CornerRadius::same(18)))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(RichText::new(icon::SQUARES_FOUR).size(30.0).color(ACCENT));
                ui.add_space(6.0);
                let title = if first_run { "Welcome to QuadKey" } else { "How QuadKey works" };
                ui.label(RichText::new(title).font(medium(22.0)).color(pal.text));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Give every button on your dock the action you want, in three steps.").size(14.0).color(pal.dim),
                );
                ui.add_space(18.0);
                for (n, title, text) in [
                    ("1", "Pick a button", "Click one of the four tiles. Pressing the button on the dock lights it up, so you always know which one it is."),
                    ("2", "Choose an action", "Browse Media, Screen, Windows, Meetings and Editing - or go to Custom to record your own shortcut, open an app or run a command."),
                    ("3", "Click Apply", "QuadKey makes the dock use your choices. Remove rule gives the factory behaviour back."),
                ] {
                    ui.horizontal_top(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 14.0, pal.accent_tint);
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, n, medium(14.0), ACCENT);
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(title).font(medium(15.0)).color(pal.text));
                            ui.label(RichText::new(text).size(13.0).color(pal.dim));
                        });
                    });
                    ui.add_space(12.0);
                }
                ui.add_space(4.0);
                ui.label(RichText::new("Good to know").font(medium(14.0)).color(pal.text));
                ui.add_space(4.0);
                for line in [
                    "\"Shortcuts for\" in the top bar chooses macOS or Linux shortcuts for the quick picks.",
                    "On macOS QuadKey uses Karabiner-Elements; on Linux it uses keyd.",
                    "The knob (volume and mute) keeps working as before.",
                    "Learn from dock re-assigns a tile if your buttons send something else.",
                ] {
                    ui.label(RichText::new(format!("•  {line}")).size(13.0).color(pal.dim));
                }
                ui.add_space(20.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let text = if first_run { "Get started" } else { "Close" };
                    let close = egui::Button::new(RichText::new(text).font(medium(15.0)).color(Color32::WHITE))
                        .fill(ACCENT)
                        .corner_radius(CornerRadius::same(18))
                        .min_size(Vec2::new(130.0, 38.0));
                    ui.add(close).clicked()
                })
                .inner
            });
        if response.inner || response.should_close() {
            self.help_open = false;
            if first_run {
                self.config.seen_guide = true;
                self.persist();
            }
        }
    }
}

impl eframe::App for DockStudio {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

/// Fonts (Ubuntu + Phosphor icons) and light/dark visuals following the system theme.
pub fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "ubuntu".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Ubuntu-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "ubuntu-medium".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Ubuntu-Medium.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "ubuntu".into());
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let mut medium_family = vec!["ubuntu-medium".to_string()];
    medium_family.extend(
        fonts.families[&FontFamily::Proportional]
            .iter()
            .skip(1)
            .cloned(),
    );
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM.into()), medium_family);
    ctx.set_fonts(fonts);

    for (theme, mut visuals) in [
        (egui::Theme::Light, egui::Visuals::light()),
        (egui::Theme::Dark, egui::Visuals::dark()),
    ] {
        let dark = theme == egui::Theme::Dark;
        visuals.selection.bg_fill = ACCENT;
        visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.hyperlink_color = ACCENT;
        visuals.window_corner_radius = CornerRadius::same(18);
        for w in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            w.corner_radius = CornerRadius::same(10);
        }
        visuals.widgets.inactive.weak_bg_fill = if dark {
            Color32::from_rgb(0x3A, 0x3A, 0x3A)
        } else {
            Color32::from_rgb(0xE6, 0xE6, 0xE6)
        };
        visuals.extreme_bg_color = if dark {
            Color32::from_rgb(0x1E, 0x1E, 0x1E)
        } else {
            Color32::WHITE
        };
        ctx.set_visuals_of(theme, visuals);
    }
    ctx.set_theme(egui::ThemePreference::System);
    ctx.global_style_mut(|style| {
        style.spacing.button_padding = Vec2::new(14.0, 8.0);
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.interact_size.y = 32.0;
    });
}

fn card(pal: Palette) -> Frame {
    Frame::new()
        .fill(pal.card)
        .corner_radius(CornerRadius::same(14))
        .stroke(Stroke::new(1.0, pal.border))
        .shadow(egui::epaint::Shadow {
            offset: [0, 1],
            blur: 6,
            spread: 0,
            color: pal.shadow,
        })
}

/// A rounded pill button; `active` uses the accent color.
fn pill_button(text: &str, pal: Palette, active: bool) -> egui::Button<'static> {
    let (fill, fg, stroke) = if active {
        (ACCENT, Color32::WHITE, Stroke::new(1.0, ACCENT))
    } else {
        (pal.card, pal.text, Stroke::new(1.0, pal.border))
    };
    egui::Button::new(RichText::new(text.to_string()).size(13.0).color(fg))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(18))
        .min_size(Vec2::new(0.0, 34.0))
}

/// A selectable card with an icon, a title and a one-line hint.
fn option_card(
    ui: &mut egui::Ui,
    pal: Palette,
    glyph: &str,
    title: &str,
    hint: &str,
    active: bool,
    width: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 64.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint);
    let painter = ui.painter();
    let radius = CornerRadius::same(12);
    let fill = if active {
        pal.accent_tint
    } else if response.hovered() {
        pal.card_hover
    } else {
        pal.card
    };
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(
        rect,
        radius,
        if active {
            Stroke::new(2.0, ACCENT)
        } else {
            Stroke::new(1.0, pal.border)
        },
        StrokeKind::Inside,
    );
    let icon_center = rect.left_center() + Vec2::new(30.0, 0.0);
    painter.circle_filled(
        icon_center,
        17.0,
        if active { ACCENT } else { pal.accent_tint },
    );
    painter.text(
        icon_center,
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(18.0),
        if active { Color32::WHITE } else { ACCENT },
    );
    let text_left = rect.left() + 58.0;
    let max_chars = ((width - 70.0) / 6.6) as usize;
    painter.text(
        egui::pos2(text_left, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        truncate(title, max_chars.max(10)),
        medium(14.0),
        pal.text,
    );
    painter.text(
        egui::pos2(text_left, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        truncate(hint, max_chars.max(10) + 4),
        FontId::proportional(12.0),
        pal.dim,
    );
    response
}

fn status_pill(ui: &mut egui::Ui, pal: Palette, color: Color32, text: &str) {
    Frame::new()
        .fill(pal.card)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::symmetric(12, 7))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // `horizontal` follows the parent's direction; in a right-to-left row the first
                // widget ends up on the right, so add the text first to keep the dot left
                let rtl = ui.layout().main_dir() == egui::Direction::RightToLeft;
                let dot = |ui: &mut egui::Ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, color);
                };
                if !rtl {
                    dot(ui);
                }
                ui.label(RichText::new(text).size(13.0).color(pal.text));
                if rtl {
                    dot(ui);
                }
            });
        });
}

/// The category tab to open for a button: where its action lives, or Custom.
fn tab_for(button: &Button, os: TargetOs) -> Category {
    preset_for_action(&button.action, os).map_or(Category::Custom, |p| p.category)
}

fn action_icon(action: &Action) -> &'static str {
    match action {
        Action::Original => icon::ARROW_COUNTER_CLOCKWISE,
        Action::Disabled => icon::PROHIBIT,
        Action::Media { .. } => icon::MUSIC_NOTES,
        Action::Shortcut { .. } => icon::KEYBOARD,
        Action::OpenApp { .. } => icon::APP_WINDOW,
        Action::Command { .. } => icon::TERMINAL,
    }
}

/// What a tile shows: the catalog name, or for "factory setting" what the dock already does.
fn tile_title(button: &Button, preset_name: Option<&str>) -> String {
    if let Some(name) = preset_name {
        return name.to_string();
    }
    match button.action {
        Action::Original => button
            .trigger
            .meaning()
            .map_or_else(|| button.trigger.label(), str::to_string),
        _ => button.action.summary(),
    }
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
    fn helpers() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
        assert_eq!(
            lerp(Color32::BLACK, Color32::WHITE, 0.5),
            Color32::from_rgb(128, 128, 128)
        );
    }

    #[test]
    fn tab_follows_the_action() {
        let mut button = Config::default().buttons[0].clone();
        assert_eq!(tab_for(&button, TargetOs::Mac), Category::Custom);
        button.action = Action::Media {
            key: Media::PlayPause,
        };
        assert_eq!(tab_for(&button, TargetOs::Mac), Category::Media);
        button.action = Action::Shortcut {
            ctrl: true,
            alt: false,
            shift: false,
            cmd: true,
            key: "q".into(),
        };
        assert_eq!(tab_for(&button, TargetOs::Mac), Category::Screen);
        assert_eq!(tab_for(&button, TargetOs::Linux), Category::Custom);
    }
}

//! The user interface (egui, immediate mode: the whole UI is described again every frame).
//!
//! The look follows Ubuntu's Yaru / GNOME libadwaita style: a flat header bar, grouped cards,
//! generous spacing, the Ubuntu typeface and a single orange accent, in light and dark.
//! Choices are a draft until Apply, so a stray click never changes what the dock does.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Layout, Margin,
    RichText, Sense, Stroke, StrokeKind, Vec2,
};
use egui_phosphor::regular as icon;

use crate::backend::{self, Environment};
use crate::device::DockEvent;
use crate::dockcfg;
use crate::model::{
    Action, Button, CONTROLS, Category, Config, DEVICE_NAME, KNOB_LEFT, KNOB_PRESS, KNOB_RIGHT,
    Media, TargetOs, is_knob, key_name_from_egui, preset_for_action, presets_for, shortcut_keys,
};

/// How long a pressed tile stays lit, in seconds.
const GLOW_SECONDS: f64 = 0.9;
/// How long the confirmation at the bottom stays visible, in seconds.
const TOAST_SECONDS: f64 = 5.0;
const LOG_LINES: usize = 60;
const POSITIONS: [&str; CONTROLS] = [
    "Top left",
    "Top right",
    "Bottom left",
    "Bottom right",
    "Turn left",
    "Turn right",
    "Press",
];
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpTab {
    Start,
    Controls,
    Questions,
}

/// The single status shown in the header, in plain words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    DockMissing,
    LookingForDock,
    NeedsInputMonitoring,
    NeedsKarabiner,
    KarabinerStopped,
    NeedsApply,
}

/// Short confirmation at the bottom of the window.
#[derive(Clone, Debug)]
pub struct Toast {
    pub ok: bool,
    pub text: String,
    shown_at: f64,
}

pub struct DockStudio {
    /// What the user sees and edits (a draft until Apply).
    pub config: Config,
    /// What was last applied - Discard returns to it.
    pub applied: Config,
    pub selected: usize,
    pub learning: Option<usize>,
    pub connection: Connection,
    pub toast: Option<Toast>,
    pub log: VecDeque<String>,
    /// Waiting for a key combination on the computer's keyboard.
    pub recording: bool,
    pub tab: Category,
    pub help: Option<HelpTab>,
    pub show_activity: bool,
    /// "Reset everything" was clicked once and waits for a confirming second click.
    pub confirm_reset: Option<f64>,
    lit_at: [Option<f64>; CONTROLS],
    env: Environment,
    events: Receiver<DockEvent>,
    karabiner_running: bool,
    karabiner_checked_at: f64,
    vendor_packets: usize,
    dock_link: Box<dyn Fn() -> anyhow::Result<Box<dyn dockcfg::DockLink>>>,
    now: f64,
}

impl DockStudio {
    pub fn new(env: Environment, events: Receiver<DockEvent>) -> Self {
        let config = Config::load_from(&env.config_file);
        let applied_file = applied_path(&env);
        let applied = if applied_file.exists() {
            Config::load_from(&applied_file)
        } else {
            Config::default()
        };
        let tab = tab_for(&config.buttons[0], config.target_os);
        DockStudio {
            config,
            applied,
            selected: 0,
            learning: None,
            connection: Connection::Waiting,
            toast: None,
            log: VecDeque::new(),
            recording: false,
            tab,
            help: None,
            show_activity: false,
            confirm_reset: None,
            lit_at: [None; CONTROLS],
            env,
            events,
            karabiner_running: true,
            karabiner_checked_at: f64::NEG_INFINITY,
            vendor_packets: 0,
            dock_link: Box::new(|| {
                Ok(Box::new(dockcfg::HidDock::open()?) as Box<dyn dockcfg::DockLink>)
            }),
            now: 0.0,
        }
    }

    /// Talk to a different dock (tests use a fake one).
    pub fn with_dock_link(
        mut self,
        link: impl Fn() -> anyhow::Result<Box<dyn dockcfg::DockLink>> + 'static,
    ) -> Self {
        self.dock_link = Box::new(link);
        self
    }

    /// For tests and the real app alike: an event channel the HID threads write into.
    pub fn channel() -> (Sender<DockEvent>, Receiver<DockEvent>) {
        std::sync::mpsc::channel()
    }

    pub fn is_lit(&self, index: usize, now: f64) -> bool {
        self.lit_at[index].is_some_and(|t| now - t < GLOW_SECONDS)
    }

    /// Controls whose draft differs from what was applied.
    pub fn changed_controls(&self) -> Vec<usize> {
        (0..CONTROLS)
            .filter(|&i| self.config.buttons[i] != self.applied.buttons[i])
            .collect()
    }

    /// True when Apply would change something (in the dock or in Karabiner / keyd) and every
    /// choice is complete.
    pub fn can_apply(&self) -> bool {
        self.incomplete().is_none()
            && (!self.changed_controls().is_empty() || self.env.needs_apply(&self.host_config()))
    }

    /// A choice that cannot work yet, e.g. "Open application" without an application.
    pub fn incomplete(&self) -> Option<String> {
        fn missing(action: &Action) -> Option<&'static str> {
            match action {
                Action::OpenApp { app } if app.trim().is_empty() => Some("choose an application"),
                Action::Command { command } if command.trim().is_empty() => {
                    Some("write the command")
                }
                Action::Toggle { first, second } => missing(first).or_else(|| missing(second)),
                _ => None,
            }
        }
        self.config.buttons.iter().enumerate().find_map(|(i, b)| {
            missing(&b.action).map(|what| format!("{}: {what} first", Self::control_name(i)))
        })
    }

    /// Controls whose action the dock cannot do by itself (open an app, run a command, toggle,
    /// do nothing) - those need Karabiner-Elements or keyd on the computer.
    pub fn computer_controls(&self) -> Vec<usize> {
        (0..CONTROLS)
            .filter(|&i| dockcfg::dock_setting(i, &self.config.buttons[i].action).is_none())
            .collect()
    }

    /// The settings for Karabiner / keyd: everything stored in the dock passes through unchanged.
    fn host_config(&self) -> Config {
        let mut host = self.config.clone();
        for (i, button) in host.buttons.iter_mut().enumerate() {
            if dockcfg::dock_setting(i, &button.action).is_some() {
                button.action = Action::Original;
            }
        }
        host
    }

    /// Whether the dock already does what the settings say.
    pub fn readiness(&self) -> Readiness {
        let needs_computer = !self.computer_controls().is_empty();
        match &self.connection {
            Connection::Disconnected => return Readiness::DockMissing,
            Connection::Waiting => return Readiness::LookingForDock,
            Connection::Connected { keyboard_ok: false } => return Readiness::NeedsInputMonitoring,
            Connection::Connected { keyboard_ok: true } => {}
        }
        if cfg!(target_os = "macos") && needs_computer && !self.env.karabiner_installed {
            return Readiness::NeedsKarabiner;
        }
        if cfg!(target_os = "macos") && needs_computer && !self.karabiner_running {
            return Readiness::KarabinerStopped;
        }
        if self.can_apply() {
            return Readiness::NeedsApply;
        }
        Readiness::Ready
    }

    fn push_log(&mut self, line: String) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    fn show_toast(&mut self, ok: bool, text: String) {
        self.toast = Some(Toast {
            ok,
            text,
            shown_at: self.now,
        });
    }

    fn persist(&mut self) {
        if let Err(err) = self.config.save_to(&self.env.config_file) {
            self.show_toast(false, format!("Could not save settings: {err}"));
        }
    }

    fn select(&mut self, index: usize) {
        self.selected = index;
        self.tab = tab_for(&self.config.buttons[index], self.config.target_os);
        self.recording = false;
    }

    pub fn control_name(index: usize) -> String {
        if is_knob(index) {
            format!("Knob ({})", POSITIONS[index].to_lowercase())
        } else {
            format!("Button {}", index + 1)
        }
    }

    /// Choose an action for the selected control (takes effect on Apply).
    fn assign(&mut self, action: Action) {
        self.config.buttons[self.selected].action = action;
        self.recording = false;
        self.persist();
    }

    /// Store in the dock what it can do by itself; hand the rest to Karabiner / keyd.
    pub fn apply(&mut self) {
        self.apply_controls(self.changed_controls());
    }

    fn apply_controls(&mut self, to_write: Vec<usize>) {
        let os = self.config.target_os;
        let describe = |s: &Self, i: usize| {
            format!(
                "{} → {}",
                Self::control_name(i),
                action_name(&s.config.buttons[i].action, os)
            )
        };

        // 1. The dock: shortcuts and media keys are stored in the dock itself
        let writes: Vec<(usize, dockcfg::DockSetting)> = to_write
            .iter()
            .filter_map(|&i| {
                dockcfg::dock_setting(i, &self.config.buttons[i].action).map(|s| (i, s))
            })
            .collect();
        let mut stored = Vec::new();
        if !writes.is_empty() {
            let mut link = match (self.dock_link)() {
                Ok(link) => link,
                Err(err) => {
                    self.show_toast(false, format!("Could not reach the dock: {err}"));
                    return;
                }
            };
            for (i, setting) in writes {
                if let Err(err) =
                    dockcfg::write(link.as_mut(), dockcfg::SLOT_OF_CONTROL[i], &setting)
                {
                    self.show_toast(
                        false,
                        format!(
                            "Could not write {} to the dock: {err}",
                            Self::control_name(i)
                        ),
                    );
                    return;
                }
                // The dock now sends something else for this control - recognise it by that
                if let Some(trigger) = dockcfg::trigger_for(i, &setting) {
                    self.config.buttons[i].trigger = trigger;
                }
                self.applied.buttons[i] = self.config.buttons[i].clone();
                stored.push(describe(self, i));
            }
            self.persist();
        }

        // 2. The computer: apps, commands, toggles and "do nothing"
        let host = self.host_config();
        let computer: Vec<usize> = to_write
            .iter()
            .copied()
            .filter(|i| self.computer_controls().contains(i))
            .collect();
        let mut host_error = None;
        if self.env.needs_apply(&host) {
            let all_factory = host.buttons.iter().all(|b| b.action == Action::Original);
            let result = if all_factory {
                self.env.remove()
            } else if cfg!(target_os = "macos") && !self.env.karabiner_installed {
                Err(anyhow::anyhow!(
                    "opening apps and running commands needs Karabiner-Elements"
                ))
            } else {
                self.env.apply(&host)
            };
            if let Err(err) = result {
                host_error = Some(err.to_string());
            }
        }
        if host_error.is_none() {
            for &i in &computer {
                self.applied.buttons[i] = self.config.buttons[i].clone();
            }
        }
        if let Err(err) = self.applied.save_to(&applied_path(&self.env)) {
            self.show_toast(false, format!("Applied, but could not remember it: {err}"));
            return;
        }

        let mut parts = Vec::new();
        if !stored.is_empty() {
            parts.push(format!("Saved in the dock: {}", stored.join(", ")));
        }
        let computer_done: Vec<String> = if host_error.is_none() {
            computer.iter().map(|&i| describe(self, i)).collect()
        } else {
            Vec::new()
        };
        if !computer_done.is_empty() {
            parts.push(format!(
                "Applied on this computer: {}",
                computer_done.join(", ")
            ));
        }
        match host_error {
            Some(err) => {
                let mut text = parts.join(". ");
                if !text.is_empty() {
                    text.push_str(". ");
                }
                text.push_str(&format!("Not applied: {err}"));
                self.show_toast(false, text);
            }
            None if parts.is_empty() => {
                self.show_toast(true, "Applied - use the dock to try it".into())
            }
            None => self.show_toast(true, parts.join(". ")),
        }
    }

    /// Throw the draft away and return to what is active.
    pub fn discard(&mut self) {
        let keep = (self.config.target_os, self.config.seen_guide);
        self.config = self.applied.clone();
        (self.config.target_os, self.config.seen_guide) = keep;
        self.persist();
        self.tab = tab_for(&self.config.buttons[self.selected], self.config.target_os);
        self.show_toast(true, "Changes discarded".into());
    }

    /// Every control back to factory behaviour - written to the dock even if QuadKey thinks it
    /// already is, because another app may have reprogrammed it.
    pub fn reset_everything(&mut self) {
        let keep = (self.config.target_os, self.config.seen_guide);
        self.config = Config::default();
        (self.config.target_os, self.config.seen_guide) = keep;
        self.persist();
        self.tab = tab_for(&self.config.buttons[self.selected], self.config.target_os);
        self.apply_controls((0..CONTROLS).collect());
        if self.toast.as_ref().is_some_and(|t| t.ok) {
            self.show_toast(true, "Everything is back to factory behaviour".into());
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
                        self.persist();
                        self.selected = index;
                        self.lit_at[index] = Some(now);
                        self.push_log(format!(
                            "{} learned: {}",
                            Self::control_name(index),
                            trigger.label()
                        ));
                        continue;
                    }
                    match self.config.button_for(&trigger) {
                        Some(index) => {
                            self.lit_at[index] = Some(now);
                            backend::run_local_action(index, &self.applied.buttons[index].action);
                            let outcome = self.outcome(index);
                            self.push_log(format!(
                                "{} · dock sent {} → {outcome}",
                                Self::control_name(index),
                                trigger.label()
                            ));
                        }
                        None => self.push_log(format!("Unknown input {}", trigger.label())),
                    }
                }
                DockEvent::PressedButton(index) => {
                    if index < CONTROLS {
                        self.lit_at[index] = Some(now);
                        let outcome = self.outcome(index);
                        self.push_log(format!("{} → {outcome}", Self::control_name(index)));
                    }
                }
                DockEvent::Released => {}
                DockEvent::Vendor(_) => self.vendor_packets += 1,
            }
        }
    }

    /// What really happened for a press, for the activity log.
    fn outcome(&self, index: usize) -> String {
        let active = &self.applied.buttons[index].action;
        let draft = &self.config.buttons[index].action;
        let os = self.config.target_os;
        let mut text = if *active == Action::Original
            || self.readiness() != Readiness::Ready
                && self.can_apply()
                && self.applied == Config::default()
        {
            "factory setting".to_string()
        } else {
            action_name(active, os)
        };
        if draft != active {
            text.push_str(&format!(
                " (your change to \"{}\" is not applied yet)",
                action_name(draft, os)
            ));
        }
        text
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
        self.now = now;
        if now - self.karabiner_checked_at > 3.0 {
            self.karabiner_checked_at = now;
            self.karabiner_running = self.env.is_karabiner_running();
        }
        self.process_events(now);
        self.record_shortcut(ui);
        if self.lit_at.iter().flatten().any(|t| now - t < GLOW_SECONDS) {
            ui.ctx().request_repaint();
        }
        if let Some(toast) = &self.toast {
            if now - toast.shown_at > TOAST_SECONDS {
                self.toast = None;
            } else {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        if self.confirm_reset.is_some_and(|t| now - t > 4.0) {
            self.confirm_reset = None;
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

        self.help_dialog(ui.ctx(), pal);
    }

    fn header(&mut self, ui: &mut egui::Ui, pal: Palette) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(RichText::new(icon::SQUARES_FOUR).size(22.0).color(ACCENT));
            ui.label(RichText::new("QuadKey").font(medium(20.0)).color(pal.text));
            ui.label(RichText::new(DEVICE_NAME).size(14.0).color(pal.dim));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                self.more_menu(ui, pal);
                if ui.add(pill_button(&format!("{}  Help", icon::QUESTION), pal, false)).on_hover_text("What everything does").clicked() {
                    self.help = Some(HelpTab::Start);
                }
                let (color, text, why) = match self.readiness() {
                    Readiness::Ready => (pal.ok, "Ready", "The dock does what you see here. Use it to try."),
                    Readiness::DockMissing => (pal.err, "Dock not connected", "Plug the dock into this computer."),
                    Readiness::LookingForDock => (pal.dim, "Looking for the dock...", "QuadKey is searching for the dock."),
                    Readiness::NeedsInputMonitoring => (
                        pal.warn,
                        "Allow Input Monitoring",
                        "System Settings → Privacy & Security → Input Monitoring: turn QuadKey (or your terminal) on, then restart QuadKey.",
                    ),
                    Readiness::NeedsKarabiner => (
                        pal.warn,
                        "Install Karabiner-Elements",
                        "Shortcuts and media keys are stored in the dock itself. Opening apps, running commands, toggles and \"do nothing\" run on the computer, and on macOS that needs Karabiner-Elements. Install it once, then click Apply.",
                    ),
                    Readiness::KarabinerStopped => (
                        pal.warn,
                        "Start Karabiner-Elements",
                        "Karabiner-Elements is installed but not running. Open it once (Applications → Karabiner-Elements) and allow what it asks for.",
                    ),
                    Readiness::NeedsApply => (pal.warn, "Changes not applied", "Click Apply at the bottom to make the dock use your choices."),
                };
                status_pill(ui, pal, color, text).on_hover_text(why);
            });
        });
    }

    /// Settings most people never need, kept out of the way.
    fn more_menu(&mut self, ui: &mut egui::Ui, pal: Palette) {
        let menu = egui::containers::menu::MenuButton::from_button(pill_button(
            icon::DOTS_THREE,
            pal,
            false,
        ));
        let (response, _) = menu.ui(ui, |ui| {
            ui.set_min_width(260.0);
            ui.label(RichText::new("Shortcuts are for").size(12.0).color(pal.dim));
            for os in [TargetOs::Mac, TargetOs::Linux] {
                if ui.radio(self.config.target_os == os, os.label()).clicked()
                    && self.config.target_os != os
                {
                    self.config.target_os = os;
                    self.tab = tab_for(&self.config.buttons[self.selected], os);
                    self.persist();
                }
            }
            ui.separator();
            let text = if self.show_activity {
                "Hide activity"
            } else {
                "Show activity"
            };
            if ui
                .button(format!("{}  {text}", icon::LIST_BULLETS))
                .clicked()
            {
                self.show_activity = !self.show_activity;
                ui.close();
            }
            let reset = if self.confirm_reset.is_some() {
                "Click again to reset everything"
            } else {
                "Reset everything to factory"
            };
            if ui
                .button(format!("{}  {reset}", icon::ARROW_COUNTER_CLOCKWISE))
                .clicked()
            {
                if self.confirm_reset.take().is_some() {
                    self.reset_everything();
                    ui.close();
                } else {
                    self.confirm_reset = Some(self.now);
                }
            }
        });
        response.on_hover_text("More: macOS/Linux shortcuts, activity, reset");
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
                    RichText::new("Click a control to change it")
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
                ui.add_space(6.0);
                ui.label(RichText::new("Knob").font(medium(14.0)).color(pal.text));
                ui.horizontal(|ui| {
                    for index in [KNOB_LEFT, KNOB_PRESS, KNOB_RIGHT] {
                        self.knob_part(ui, pal, index, now);
                    }
                });
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!(
                        "{}  Use the dock - the matching tile lights up",
                        icon::HAND_POINTING
                    ))
                    .size(12.0)
                    .color(pal.dim),
                );
            });
        });
    }

    /// One part of the knob: turning left or right (arrow tiles) or pressing it (a round knob).
    fn knob_part(&mut self, ui: &mut egui::Ui, pal: Palette, index: usize, now: f64) {
        let size = Vec2::new(104.0, 132.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let label = format!("Knob: {}", POSITIONS[index].to_lowercase());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        let response = response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(match index {
                KNOB_LEFT => "What turning the knob to the left (counter-clockwise) does",
                KNOB_RIGHT => "What turning the knob to the right (clockwise) does",
                _ => "What pressing the knob down does",
            });

        let button = &self.config.buttons[index];
        let changed = button != &self.applied.buttons[index];
        let preset = preset_for_action(&button.action, self.config.target_os);
        let glow = self.lit_at[index].map_or(0.0, |t| {
            (1.0 - ((now - t) / GLOW_SECONDS)).clamp(0.0, 1.0) as f32
        });
        let selected = self.selected == index;
        let active = selected || glow > 0.0;
        let base = if response.hovered() {
            pal.card_hover
        } else {
            pal.card
        };
        let tint: f32 = if selected { 0.6 } else { 0.0 };
        let fill = lerp(base, pal.accent_tint, tint.max(glow));
        let stroke = if self.learning == Some(index) {
            Stroke::new(2.0, pal.warn)
        } else if active {
            Stroke::new(2.0, ACCENT)
        } else {
            Stroke::new(1.0, pal.border)
        };
        let painter = ui.painter();
        let shadow = egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: pal.shadow,
        };
        let graphic = egui::Rect::from_center_size(
            rect.center_top() + Vec2::new(0.0, 40.0),
            Vec2::splat(76.0),
        );
        if index == KNOB_PRESS {
            // The knob itself: a round cap with a ring, pressed in when lit
            let radius = 36.0 - glow * 2.0;
            painter.add(shadow.as_shape(graphic, CornerRadius::same(38)));
            painter.circle(graphic.center(), radius, fill, stroke);
            painter.circle_stroke(graphic.center(), radius - 7.0, Stroke::new(1.0, pal.border));
            painter.text(
                graphic.center(),
                Align2::CENTER_CENTER,
                icon::HAND_TAP,
                FontId::proportional(22.0),
                if active { ACCENT } else { pal.dim },
            );
        } else {
            let radius = CornerRadius::same(16);
            painter.add(shadow.as_shape(graphic, radius));
            painter.rect_filled(graphic, radius, fill);
            painter.rect_stroke(graphic, radius, stroke, StrokeKind::Inside);
            let arrow = if index == KNOB_LEFT {
                icon::ARROW_COUNTER_CLOCKWISE
            } else {
                icon::ARROW_CLOCKWISE
            };
            painter.text(
                graphic.center(),
                Align2::CENTER_CENTER,
                arrow,
                FontId::proportional(34.0),
                if active { ACCENT } else { pal.dim },
            );
        }
        if changed {
            painter.circle_filled(graphic.right_top() + Vec2::new(-6.0, 6.0), 4.0, pal.warn);
        }
        let caption = match index {
            KNOB_LEFT => "Turn left",
            KNOB_RIGHT => "Turn right",
            _ => "Press",
        };
        painter.text(
            rect.center_top() + Vec2::new(0.0, 92.0),
            Align2::CENTER_TOP,
            caption,
            medium(13.0),
            pal.text,
        );
        let title = tile_title(button, preset.as_ref().map(|p| p.name));
        painter.text(
            rect.center_top() + Vec2::new(0.0, 112.0),
            Align2::CENTER_TOP,
            truncate(&title, 15),
            FontId::proportional(11.5),
            if changed { pal.warn } else { pal.dim },
        );
        if response.clicked() {
            self.select(index);
        }
    }

    fn tile(&mut self, ui: &mut egui::Ui, pal: Palette, index: usize, now: f64) {
        let knob = is_knob(index);
        let size = if knob {
            Vec2::new(104.0, 96.0)
        } else {
            Vec2::new(168.0, 140.0)
        };
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        // Accessible name differs from the editor title ("Button 3") so both can be found
        let label = if knob {
            format!("Knob: {}", POSITIONS[index].to_lowercase())
        } else {
            format!("Dock button {}", index + 1)
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

        let button = &self.config.buttons[index];
        let changed = button != &self.applied.buttons[index];
        let preset = preset_for_action(&button.action, self.config.target_os);
        let glow = self.lit_at[index].map_or(0.0, |t| {
            (1.0 - ((now - t) / GLOW_SECONDS)).clamp(0.0, 1.0) as f32
        });
        let selected = self.selected == index;
        let learning = self.learning == Some(index);

        // Pressed tiles sink slightly and glow in the accent color
        let rect = rect.shrink(glow * 2.0);
        let radius = CornerRadius::same(if knob { 14 } else { 16 });
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
        painter.rect_filled(rect, radius, lerp(base, pal.accent_tint, tint.max(glow)));
        let stroke = if learning {
            Stroke::new(2.0, pal.warn)
        } else if selected || glow > 0.0 {
            Stroke::new(2.0, ACCENT)
        } else {
            Stroke::new(1.0, pal.border)
        };
        painter.rect_stroke(rect, radius, stroke, StrokeKind::Inside);

        let pad = if knob { 12.0 } else { 16.0 };
        let glyph = preset.as_ref().map_or_else(|| icon_for(button), |p| p.icon);
        let highlight = if selected || glow > 0.0 {
            ACCENT
        } else {
            pal.dim
        };
        painter.text(
            rect.left_top() + Vec2::new(pad, pad),
            Align2::LEFT_TOP,
            glyph,
            FontId::proportional(if knob { 18.0 } else { 24.0 }),
            highlight,
        );
        let corner = match index {
            KNOB_LEFT => icon::ARROW_COUNTER_CLOCKWISE.to_string(),
            KNOB_RIGHT => icon::ARROW_CLOCKWISE.to_string(),
            KNOB_PRESS => icon::CIRCLE.to_string(),
            _ => format!("{}", index + 1),
        };
        painter.text(
            rect.right_top() + Vec2::new(-pad, pad + 2.0),
            Align2::RIGHT_TOP,
            corner,
            medium(13.0),
            pal.dim,
        );
        if changed {
            // Not applied yet: a small dot next to the number
            painter.circle_filled(
                rect.right_top() + Vec2::new(-pad - 18.0, pad + 9.0),
                4.0,
                pal.warn,
            );
        }
        let title = tile_title(button, preset.as_ref().map(|p| p.name));
        let (title_size, chars) = if knob { (12.5, 12) } else { (15.0, 17) };
        painter.text(
            rect.left_bottom() + Vec2::new(pad, -pad - if knob { 14.0 } else { 18.0 }),
            Align2::LEFT_BOTTOM,
            truncate(&title, chars),
            medium(title_size),
            pal.text,
        );
        let sub = if learning {
            "Press it on the dock...".to_string()
        } else if changed {
            "not applied yet".to_string()
        } else if knob {
            POSITIONS[index].to_string()
        } else if button.action == Action::Original {
            "factory setting".to_string()
        } else {
            truncate(&button.trigger.label(), 22)
        };
        let sub_color = if learning || changed {
            pal.warn
        } else {
            pal.dim
        };
        painter.text(
            rect.left_bottom() + Vec2::new(pad, -pad),
            Align2::LEFT_BOTTOM,
            sub,
            FontId::proportional(if knob { 11.0 } else { 12.0 }),
            sub_color,
        );
        if response.clicked() {
            self.select(index);
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui, pal: Palette) {
        let index = self.selected;
        ui.set_min_width(460.0);
        let trigger = self.config.buttons[index].trigger;

        ui.horizontal(|ui| {
            let title = if is_knob(index) {
                "Knob".to_string()
            } else {
                format!("Button {}", index + 1)
            };
            ui.label(RichText::new(title).font(medium(22.0)).color(pal.text));
            ui.label(RichText::new(POSITIONS[index]).size(15.0).color(pal.dim));
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Out of the box it sends").size(13.0).color(pal.dim));
            ui.label(RichText::new(trigger.label()).monospace().size(13.0).color(pal.text));
            if let Some(meaning) = trigger.meaning() {
                let os = if cfg!(target_os = "macos") { "macOS" } else { "Linux" };
                status_pill(ui, pal, pal.warn, &format!("{os}: {meaning}"));
            }
            let learning = self.learning == Some(index);
            let text = if learning { "Cancel learning" } else { "Learn from dock" };
            if ui
                .link(RichText::new(text).size(13.0))
                .on_hover_text("Only needed if using this control does not light up its tile: click, then use the control on the dock so QuadKey recognises it.")
                .clicked()
            {
                self.learning = if learning { None } else { Some(index) };
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
                        .map_or_else(|| icon_for(&self.config.buttons[index]), |p| p.icon);
                    ui.label(RichText::new(glyph).size(22.0).color(ACCENT));
                    ui.vertical(|ui| {
                        let changed = self.config.buttons[index] != self.applied.buttons[index];
                        let caption = if changed {
                            "Will do after Apply"
                        } else {
                            "Does now"
                        };
                        ui.label(RichText::new(caption).size(12.0).color(if changed {
                            pal.warn
                        } else {
                            pal.dim
                        }));
                        ui.label(
                            RichText::new(action_name(&action, self.config.target_os))
                                .font(medium(16.0))
                                .color(pal.text),
                        );
                        if action.needs_computer() {
                            ui.label(
                                RichText::new(
                                    "Runs on this computer while QuadKey or Karabiner is active",
                                )
                                .size(12.0)
                                .color(pal.dim),
                            );
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if action != Action::Original
                            && ui
                                .add(pill_button("Back to factory", pal, false))
                                .on_hover_text(
                                    "Give this control its out-of-the-box behaviour back",
                                )
                                .clicked()
                        {
                            let defaults = Config::default();
                            self.config.buttons[index] = defaults.buttons[index].clone();
                            self.tab = tab_for(&self.config.buttons[index], self.config.target_os);
                            self.persist();
                        }
                    });
                });
            });
        ui.add_space(20.0);

        ui.label(
            RichText::new("Choose what it does")
                .font(medium(15.0))
                .color(pal.text),
        );
        ui.label(
            RichText::new("Pick an action, then click Apply at the bottom.")
                .size(13.0)
                .color(pal.dim),
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

    /// One click picks a common action.
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
                    // Shortcuts show their keys; other actions are described by the hint
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
        let is =
            |example: &Action| std::mem::discriminant(&action) == std::mem::discriminant(example);
        let os = self.config.target_os;
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
                if option_card(ui, pal, icon::RECORD, title, hint, recording, width).clicked() {
                    self.recording = !self.recording;
                }
                let keys = Action::Shortcut {
                    ctrl: false,
                    alt: false,
                    shift: true,
                    cmd: true,
                    key: "5".into(),
                };
                if option_card(
                    ui,
                    pal,
                    icon::KEYBOARD,
                    "Choose keys",
                    "Pick modifiers and a key",
                    is(&keys),
                    width,
                )
                .clicked()
                    && !is(&keys)
                {
                    self.assign(keys);
                }
                ui.end_row();
                let app = Action::OpenApp { app: String::new() };
                if option_card(
                    ui,
                    pal,
                    icon::APP_WINDOW,
                    "Open application",
                    "Start or bring up an app",
                    is(&app),
                    width,
                )
                .clicked()
                    && !is(&app)
                {
                    self.assign(app);
                }
                let command = Action::Command {
                    command: String::new(),
                };
                if option_card(
                    ui,
                    pal,
                    icon::TERMINAL,
                    "Run command",
                    "Any shell command you write",
                    is(&command),
                    width,
                )
                .clicked()
                    && !is(&command)
                {
                    self.assign(command);
                }
                ui.end_row();
                let toggle = {
                    let first = presets_for(os)
                        .into_iter()
                        .find(|(p, _)| p.name == "Mute microphone (all apps)")
                        .map(|(_, a)| a);
                    Action::Toggle {
                        first: Box::new(first.unwrap_or(Action::Media { key: Media::Mute })),
                        second: Box::new(Action::Media {
                            key: Media::PlayPause,
                        }),
                    }
                };
                if option_card(
                    ui,
                    pal,
                    icon::TOGGLE_LEFT,
                    "Toggle between two",
                    "1st press does one thing, 2nd press another",
                    is(&toggle),
                    width,
                )
                .clicked()
                    && !is(&toggle)
                {
                    self.assign(toggle);
                }
                if option_card(
                    ui,
                    pal,
                    icon::PROHIBIT,
                    "Do nothing",
                    "Ignore it completely",
                    action == Action::Disabled,
                    width,
                )
                .clicked()
                {
                    self.assign(Action::Disabled);
                }
                ui.end_row();
                if option_card(
                    ui,
                    pal,
                    icon::ARROW_COUNTER_CLOCKWISE,
                    "Factory setting",
                    "Keep what the dock sends by itself",
                    action == Action::Original,
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
        let mut pick_app = false;
        let os = self.config.target_os;
        let target_mac = os == TargetOs::Mac;
        match &mut self.config.buttons[index].action {
            Action::Shortcut {
                ctrl,
                alt,
                shift,
                cmd,
                key,
            } => {
                section(ui, pal, "Keys to send", |ui| {
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
                section(ui, pal, "Application", |ui| {
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
                            pick_app = true;
                        }
                    });
                    let opens = if app.trim().is_empty() {
                        RichText::new("No application chosen yet")
                            .size(12.0)
                            .color(pal.warn)
                    } else {
                        let name = std::path::Path::new(app.as_str())
                            .file_stem()
                            .map_or_else(|| app.clone(), |s| s.to_string_lossy().to_string());
                        RichText::new(format!("{}  Opens {name}", icon::CHECK))
                            .size(12.0)
                            .color(pal.ok)
                    };
                    ui.label(opens);
                });
            }
            Action::Command { command } => {
                section(ui, pal, "Shell command", |ui| {
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(command)
                                .hint_text("e.g. open ~/Downloads")
                                .desired_width(f32::INFINITY),
                        )
                        .changed();
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Runs with your user's shell when the control is used.")
                            .size(12.0)
                            .color(pal.dim),
                    );
                });
            }
            Action::Toggle { first, second } => {
                section(ui, pal, "Toggle", |ui| {
                    for (label, half, salt) in [
                        ("First press", first, "toggle_first"),
                        ("Second press", second, "toggle_second"),
                    ] {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [110.0, 20.0],
                                egui::Label::new(RichText::new(label).size(13.0).color(pal.text)),
                            );
                            egui::ComboBox::from_id_salt(salt)
                                .selected_text(action_name(half, os))
                                .width(280.0)
                                .show_ui(ui, |ui| {
                                    let mut choices: Vec<(String, Action)> = presets_for(os)
                                        .into_iter()
                                        .map(|(p, a)| {
                                            (format!("{} · {}", p.category.label(), p.name), a)
                                        })
                                        .collect();
                                    choices.push(("Do nothing".into(), Action::Disabled));
                                    for (name, action) in choices {
                                        let selected = **half == action;
                                        if ui.selectable_label(selected, name).clicked()
                                            && !selected
                                        {
                                            **half = action;
                                            changed = true;
                                        }
                                    }
                                });
                        });
                    }
                    ui.label(
                        RichText::new(
                            "Press once for the first action, again for the second, and so on.",
                        )
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
            self.persist();
        }
        if pick_app && let Some(app) = choose_app() {
            self.assign(Action::OpenApp { app });
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui, pal: Palette) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let can_apply = self.can_apply();
            let apply = egui::Button::new(
                RichText::new("Apply")
                    .font(medium(15.0))
                    .color(Color32::WHITE),
            )
            .fill(ACCENT)
            .corner_radius(CornerRadius::same(18))
            .min_size(Vec2::new(120.0, 38.0));
            let response = ui.add_enabled(can_apply, apply);
            let response = if can_apply {
                let how = if cfg!(target_os = "linux") {
                    " (asks for the administrator password)"
                } else {
                    ""
                };
                response.on_hover_text(format!("Make the dock use your choices{how}"))
            } else {
                response
                    .on_disabled_hover_text("Nothing to apply - the dock already does what you see")
            };
            if response.clicked() {
                self.apply();
            }
            let changes = self.changed_controls().len();
            if ui
                .add_enabled(changes > 0, pill_button("Discard", pal, false))
                .on_hover_text("Throw away the changes and go back to what is active")
                .clicked()
            {
                self.discard();
            }
            match &self.toast {
                Some(toast) => {
                    let (glyph, color) = if toast.ok {
                        (icon::CHECK_CIRCLE, pal.ok)
                    } else {
                        (icon::WARNING_CIRCLE, pal.err)
                    };
                    ui.label(RichText::new(glyph).size(18.0).color(color));
                    ui.label(RichText::new(&toast.text).size(14.0).color(pal.text));
                }
                None if self.incomplete().is_some() => {
                    let text = self.incomplete().unwrap_or_default();
                    ui.label(RichText::new(text).size(13.0).color(pal.warn));
                }
                None if changes > 0 => {
                    let text = if changes == 1 {
                        "1 change not applied".to_string()
                    } else {
                        format!("{changes} changes not applied")
                    };
                    ui.label(RichText::new(text).size(13.0).color(pal.warn));
                }
                None => {
                    let hint = match self.readiness() {
                        Readiness::NeedsKarabiner => {
                            "Install Karabiner-Elements to make your choices work."
                        }
                        Readiness::KarabinerStopped => {
                            "Start Karabiner-Elements to make your choices work."
                        }
                        _ => "All saved. Pick a control to change it.",
                    };
                    ui.label(RichText::new(hint).size(13.0).color(pal.dim));
                }
            }
            if cfg!(target_os = "macos") && !self.env.karabiner_installed {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.link("Get Karabiner-Elements").clicked() {
                        let _ = std::process::Command::new("open")
                            .arg("https://karabiner-elements.pqrs.org/")
                            .spawn();
                    }
                });
            }
        });
        if self.show_activity {
            ui.add_space(8.0);
            ui.label(
                RichText::new("Activity - what the dock sent and what happened")
                    .size(12.0)
                    .color(pal.dim),
            );
            egui::ScrollArea::vertical()
                .max_height(100.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if self.log.is_empty() {
                        ui.label(
                            RichText::new("Nothing yet - use the dock.")
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
    fn help_dialog(&mut self, ctx: &egui::Context, pal: Palette) {
        let first_run = !self.config.seen_guide;
        if first_run && self.help.is_none() {
            self.help = Some(HelpTab::Start);
        }
        let Some(mut tab) = self.help else { return };
        let response = egui::Modal::new(egui::Id::new("help"))
            .frame(
                card(pal)
                    .inner_margin(Margin::same(28))
                    .corner_radius(CornerRadius::same(18)),
            )
            .show(ctx, |ui| {
                ui.set_width(560.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::SQUARES_FOUR).size(26.0).color(ACCENT));
                    let title = if first_run {
                        "Welcome to QuadKey"
                    } else {
                        "QuadKey help"
                    };
                    ui.label(RichText::new(title).font(medium(22.0)).color(pal.text));
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for (t, name) in [
                        (HelpTab::Start, "Getting started"),
                        (HelpTab::Controls, "Every control explained"),
                        (HelpTab::Questions, "Questions"),
                    ] {
                        if ui.add(pill_button(name, pal, tab == t)).clicked() {
                            tab = t;
                        }
                    }
                });
                ui.add_space(16.0);
                egui::ScrollArea::vertical()
                    .max_height(430.0)
                    .show(ui, |ui| match tab {
                        HelpTab::Start => help_start(ui, pal),
                        HelpTab::Controls => help_controls(ui, pal),
                        HelpTab::Questions => help_questions(ui, pal),
                    });
                ui.add_space(18.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let text = if first_run { "Get started" } else { "Close" };
                    let close = egui::Button::new(
                        RichText::new(text).font(medium(15.0)).color(Color32::WHITE),
                    )
                    .fill(ACCENT)
                    .corner_radius(CornerRadius::same(18))
                    .min_size(Vec2::new(130.0, 38.0));
                    ui.add(close).clicked()
                })
                .inner
            });
        self.help = Some(tab);
        if response.inner || response.should_close() {
            self.help = None;
            if first_run {
                self.config.seen_guide = true;
                self.persist();
            }
        }
    }
}

fn applied_path(env: &Environment) -> PathBuf {
    env.config_file.with_file_name("applied.json")
}

fn help_heading(ui: &mut egui::Ui, pal: Palette, glyph: &str, title: &str, text: &str) {
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 15.0, pal.accent_tint);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(16.0),
            ACCENT,
        );
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(title).font(medium(15.0)).color(pal.text));
            ui.label(RichText::new(text).size(13.0).color(pal.dim));
        });
    });
    ui.add_space(12.0);
}

fn help_start(ui: &mut egui::Ui, pal: Palette) {
    ui.label(
        RichText::new("Give every button and the knob on your dock the action you want.")
            .size(14.0)
            .color(pal.dim),
    );
    ui.add_space(14.0);
    help_heading(
        ui,
        pal,
        "1",
        "Pick a control",
        "Click one of the four button tiles or one of the three knob tiles (turn left, press, turn right). Using the dock lights up the matching tile, so you always know which is which.",
    );
    help_heading(
        ui,
        pal,
        "2",
        "Choose what it does",
        "Browse Media, Screen, Windows, Meetings and Editing, or open Custom to record your own shortcut, open an app, run a command or switch between two actions.",
    );
    help_heading(
        ui,
        pal,
        "3",
        "Click Apply",
        "Nothing changes on the dock until you click Apply, so a stray click is harmless. Tiles with a yellow dot have changes that are not applied yet; Discard throws them away.",
    );
    ui.label(RichText::new("The status in the top bar tells you if anything is missing, for example that the dock is not connected.").size(13.0).color(pal.dim));
}

fn help_controls(ui: &mut egui::Ui, pal: Palette) {
    for (glyph, title, text) in [
        (
            icon::SQUARES_FOUR,
            "Button and knob tiles",
            "Each tile is one control on the dock. Click it to change it. The tile lights up whenever you use that control; a yellow dot means a change waits for Apply.",
        ),
        (
            icon::INFO,
            "Out of the box it sends",
            "What the dock sends when nothing is changed - for example Ctrl+Cmd+Q, which locks a Mac.",
        ),
        (
            icon::MAGNIFYING_GLASS,
            "Learn from dock",
            "Only needed if a control does not light up its tile (for example after reprogramming the dock with another app). Click it, then use the control on the dock.",
        ),
        (
            icon::CHECK_CIRCLE,
            "Does now / Will do after Apply",
            "What the selected control does, or will do once you click Apply.",
        ),
        (
            icon::ARROW_COUNTER_CLOCKWISE,
            "Back to factory",
            "Gives just this control its out-of-the-box behaviour back (after Apply).",
        ),
        (
            icon::MUSIC_NOTES,
            "Categories and cards",
            "Ready-made actions. Click a card to choose it; the chosen one is highlighted.",
        ),
        (
            icon::RECORD,
            "Record shortcut",
            "Click, then type a key combination on your keyboard. Esc cancels.",
        ),
        (
            icon::KEYBOARD,
            "Choose keys",
            "Build a shortcut by hand: toggle Ctrl, Option/Alt, Shift, Cmd/Super and pick a key.",
        ),
        (
            icon::APP_WINDOW,
            "Open application / Run command",
            "Starts an app or runs a shell command you write. These run on the computer, so they need QuadKey (Linux) or Karabiner-Elements (macOS).",
        ),
        (
            icon::TOGGLE_LEFT,
            "Toggle between two",
            "The first press does one action, the next press the other - for example mute and unmute.",
        ),
        (
            icon::PROHIBIT,
            "Do nothing",
            "Turns the control off, for buttons you keep pressing by accident.",
        ),
        (
            icon::CHECK,
            "Apply",
            "Makes the dock use your choices. Greyed out when there is nothing to apply.",
        ),
        (
            icon::X,
            "Discard",
            "Throws away changes that are not applied yet.",
        ),
        (
            icon::CIRCLE,
            "Status in the top bar",
            "Ready means the dock does what you see. Otherwise it says what is missing; hover it for details.",
        ),
        (
            icon::DOTS_THREE,
            "More (⋯)",
            "Choose whether shortcuts are for macOS or Linux, show the activity log, or reset everything to factory.",
        ),
        (
            icon::LIST_BULLETS,
            "Activity",
            "A live list of what the dock sent and what QuadKey did, to check that a control works.",
        ),
    ] {
        help_heading(ui, pal, glyph, title, text);
    }
}

fn help_questions(ui: &mut egui::Ui, pal: Palette) {
    for (q, a) in [
        (
            "How do I know my change works?",
            "Click Apply, use the control on the dock and watch its tile light up. Under More (⋯) → Show activity you see exactly what happened.",
        ),
        (
            "Is my setup saved?",
            "Yes. Everything you choose is saved right away and survives restarts; Apply decides what the dock actually does.",
        ),
        (
            "When is Karabiner-Elements needed?",
            "Only for actions the dock cannot do by itself: opening an app, running a command, toggling between two actions, or doing nothing. Shortcuts and media keys are written into the dock and work on any computer without extra software.",
        ),
        (
            "Why does a control still do the old thing?",
            "Check the status in the top bar - it names what is missing (Apply, Karabiner-Elements or Input Monitoring).",
        ),
        (
            "Does the knob still change the volume?",
            "Yes, until you choose something else for it.",
        ),
        (
            "Does it work on Linux?",
            "Yes, with keyd. Choose \"Shortcuts are for: Linux\" under More (⋯) if you mostly use the dock on Linux.",
        ),
        (
            "How do I undo everything?",
            "More (⋯) → Reset everything to factory. The dock behaves exactly as out of the box.",
        ),
        (
            "Is this an official TobenONE app?",
            "No. QuadKey is an independent open-source project.",
        ),
    ] {
        ui.label(RichText::new(q).font(medium(14.0)).color(pal.text));
        ui.label(RichText::new(a).size(13.0).color(pal.dim));
        ui.add_space(12.0);
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

fn section(ui: &mut egui::Ui, pal: Palette, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    card(pal).inner_margin(Margin::same(14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).size(12.0).color(pal.dim));
        ui.add_space(6.0);
        add(ui);
    });
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

fn status_pill(ui: &mut egui::Ui, pal: Palette, color: Color32, text: &str) -> egui::Response {
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
        })
        .response
}

/// The category tab to open for a control: where its action lives, or Custom.
fn tab_for(button: &Button, os: TargetOs) -> Category {
    preset_for_action(&button.action, os).map_or(Category::Custom, |p| p.category)
}

/// Human name of an action: the catalog name when it is one, otherwise a summary.
pub fn action_name(action: &Action, os: TargetOs) -> String {
    if let Some(preset) = preset_for_action(action, os) {
        return preset.name.to_string();
    }
    match action {
        Action::Original => "Factory setting".into(),
        Action::Disabled => "Nothing".into(),
        Action::OpenApp { app } if app.is_empty() => {
            "Open an application (choose one below)".into()
        }
        Action::Command { command } if command.is_empty() => {
            "Run a command (write it below)".into()
        }
        Action::Toggle { first, second } => {
            format!("{} / {}", action_name(first, os), action_name(second, os))
        }
        other => other.summary(),
    }
}

/// Icon for a control: for the factory setting, the icon of what the dock sends by itself.
fn icon_for(button: &Button) -> &'static str {
    use crate::model::{MOD_LCTRL, MOD_LGUI, MOD_LSHIFT, Trigger};
    if button.action != Action::Original {
        return action_icon(&button.action);
    }
    match button.trigger {
        Trigger::Consumer { usage: 0x00B5 } => icon::SKIP_FORWARD,
        Trigger::Consumer { usage: 0x00B6 } => icon::SKIP_BACK,
        Trigger::Consumer { usage: 0x00CD } => icon::PLAY_PAUSE,
        Trigger::Consumer { usage: 0x00E2 } => icon::SPEAKER_SLASH,
        Trigger::Consumer { usage: 0x00E9 } => icon::SPEAKER_HIGH,
        Trigger::Consumer { usage: 0x00EA } => icon::SPEAKER_LOW,
        Trigger::Key { mods, key: 0x14 }
            if Trigger::normalized_mods(mods) == MOD_LCTRL | MOD_LGUI =>
        {
            icon::LOCK
        }
        Trigger::Key { mods, key: 0x21 }
            if Trigger::normalized_mods(mods) == MOD_LSHIFT | MOD_LGUI =>
        {
            icon::CROP
        }
        _ => icon::ARROW_COUNTER_CLOCKWISE,
    }
}

fn action_icon(action: &Action) -> &'static str {
    match action {
        Action::Original => icon::ARROW_COUNTER_CLOCKWISE,
        Action::Disabled => icon::PROHIBIT,
        Action::Media { .. } => icon::MUSIC_NOTES,
        Action::Shortcut { .. } => icon::KEYBOARD,
        Action::OpenApp { .. } => icon::APP_WINDOW,
        Action::Command { .. } => icon::TERMINAL,
        Action::Toggle { .. } => icon::TOGGLE_LEFT,
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
        Action::Toggle { .. } => "Toggle".into(),
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

/// The system's own "open" dialog, starting in Applications (macOS).
#[cfg(target_os = "macos")]
fn choose_app() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Choose an application for this control")
        .set_directory("/Applications")
        .add_filter("Application", &["app"])
        .pick_file()
        .map(|path| path.to_string_lossy().trim_end_matches('/').to_string())
}

#[cfg(not(target_os = "macos"))]
fn choose_app() -> Option<String> {
    None
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

    #[test]
    fn action_names_are_human() {
        assert_eq!(
            action_name(&Action::Original, TargetOs::Mac),
            "Factory setting"
        );
        assert_eq!(
            action_name(
                &Action::Media {
                    key: Media::PlayPause
                },
                TargetOs::Mac
            ),
            "Play / Pause"
        );
        let toggle = Action::Toggle {
            first: Box::new(Action::Media { key: Media::Mute }),
            second: Box::new(Action::Disabled),
        };
        assert_eq!(action_name(&toggle, TargetOs::Mac), "Mute sound / Nothing");
    }
}

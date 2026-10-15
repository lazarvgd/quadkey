//! Button triggers, actions and the persisted configuration.

use std::path::PathBuf;

use eframe::egui;
use serde::{Deserialize, Serialize};

/// TobenONE UDS060 dock.
pub const DEVICE_NAME: &str = "TobenONE UDS060";
pub const VID: u16 = 0xFFF1;
pub const PID: u16 = 0x00C2;

/// HID keyboard modifier bits (byte 0 of a boot keyboard report).
pub const MOD_LCTRL: u8 = 0x01;
pub const MOD_LSHIFT: u8 = 0x02;
pub const MOD_LALT: u8 = 0x04;
pub const MOD_LGUI: u8 = 0x08;

/// What the dock sends when a button is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// Consumer control usage (media keys), e.g. 0xB6 = previous track.
    Consumer { usage: u16 },
    /// Keyboard usage with modifier bits.
    Key { mods: u8, key: u8 },
}

impl Trigger {
    /// Modifier bits with left/right merged onto the left bits.
    pub fn normalized_mods(mods: u8) -> u8 {
        (mods & 0x0F) | (mods >> 4)
    }

    pub fn label(&self) -> String {
        match *self {
            Trigger::Consumer { usage } => consumer_name(usage)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Consumer 0x{usage:04X}")),
            Trigger::Key { mods, key } => {
                let mut parts: Vec<String> = Vec::new();
                let m = Trigger::normalized_mods(mods);
                if m & MOD_LCTRL != 0 {
                    parts.push("Ctrl".into());
                }
                if m & MOD_LALT != 0 {
                    parts.push(
                        if cfg!(target_os = "macos") {
                            "Option"
                        } else {
                            "Alt"
                        }
                        .into(),
                    );
                }
                if m & MOD_LSHIFT != 0 {
                    parts.push("Shift".into());
                }
                if m & MOD_LGUI != 0 {
                    parts.push(
                        if cfg!(target_os = "macos") {
                            "Cmd"
                        } else {
                            "Super"
                        }
                        .into(),
                    );
                }
                parts.push(key_label(key));
                parts.join("+")
            }
        }
    }

    /// What the trigger does on this OS by default, if it is a well-known shortcut.
    pub fn meaning(&self) -> Option<&'static str> {
        match *self {
            Trigger::Consumer { usage } => consumer_name(usage),
            Trigger::Key { mods, key } => {
                let m = Trigger::normalized_mods(mods);
                match (m, key) {
                    (x, 0x14) if x == MOD_LCTRL | MOD_LGUI && cfg!(target_os = "macos") => {
                        Some("Lock screen")
                    }
                    (x, 0x21) if x == MOD_LSHIFT | MOD_LGUI && cfg!(target_os = "macos") => {
                        Some("Screenshot (area)")
                    }
                    (x, 0x20) if x == MOD_LSHIFT | MOD_LGUI && cfg!(target_os = "macos") => {
                        Some("Screenshot (screen)")
                    }
                    _ => None,
                }
            }
        }
    }
}

pub fn consumer_name(usage: u16) -> Option<&'static str> {
    Some(match usage {
        0x00B5 => "Next track",
        0x00B6 => "Previous track",
        0x00B7 => "Stop",
        0x00CD => "Play / Pause",
        0x00E2 => "Mute",
        0x00E9 => "Volume up",
        0x00EA => "Volume down",
        0x006F => "Brightness up",
        0x0070 => "Brightness down",
        _ => return None,
    })
}

/// Karabiner key_code names indexed by HID keyboard usage; also used as the key list in the UI.
pub fn key_name(usage: u8) -> Option<String> {
    Some(match usage {
        0x04..=0x1D => ((b'a' + usage - 0x04) as char).to_string(),
        0x1E..=0x26 => ((b'1' + usage - 0x1E) as char).to_string(),
        0x27 => "0".into(),
        0x28 => "return_or_enter".into(),
        0x29 => "escape".into(),
        0x2A => "delete_or_backspace".into(),
        0x2B => "tab".into(),
        0x2C => "spacebar".into(),
        0x2D => "hyphen".into(),
        0x2E => "equal_sign".into(),
        0x2F => "open_bracket".into(),
        0x30 => "close_bracket".into(),
        0x33 => "semicolon".into(),
        0x34 => "quote".into(),
        0x36 => "comma".into(),
        0x37 => "period".into(),
        0x38 => "slash".into(),
        0x3A..=0x45 => format!("f{}", usage - 0x39),
        0x4A => "home".into(),
        0x4B => "page_up".into(),
        0x4C => "delete_forward".into(),
        0x4D => "end".into(),
        0x4E => "page_down".into(),
        0x4F => "right_arrow".into(),
        0x50 => "left_arrow".into(),
        0x51 => "down_arrow".into(),
        0x52 => "up_arrow".into(),
        0x68..=0x73 => format!("f{}", usage - 0x5B),
        _ => return None,
    })
}

/// Human-readable key name.
pub fn key_label(usage: u8) -> String {
    match key_name(usage) {
        Some(name) if name.len() == 1 => name.to_uppercase(),
        Some(name) => match name.as_str() {
            "return_or_enter" => "Enter".into(),
            "escape" => "Esc".into(),
            "delete_or_backspace" => "Backspace".into(),
            "spacebar" => "Space".into(),
            "delete_forward" => "Delete".into(),
            "page_up" => "Page Up".into(),
            "page_down" => "Page Down".into(),
            other if other.ends_with("_arrow") => other.trim_end_matches("_arrow").to_string(),
            other => other.to_uppercase(),
        },
        None => format!("0x{usage:02X}"),
    }
}

/// All keys that can be picked for a shortcut action (Karabiner key_code names).
pub fn shortcut_keys() -> Vec<String> {
    let mut keys: Vec<String> = (0x04u8..=0x73).filter_map(key_name).collect();
    keys.dedup();
    keys
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Media {
    PlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
    BrightnessUp,
    BrightnessDown,
}

impl Media {
    pub const ALL: [Media; 8] = [
        Media::PlayPause,
        Media::Next,
        Media::Previous,
        Media::VolumeUp,
        Media::VolumeDown,
        Media::Mute,
        Media::BrightnessUp,
        Media::BrightnessDown,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Media::PlayPause => "Play / Pause",
            Media::Next => "Next track",
            Media::Previous => "Previous track",
            Media::VolumeUp => "Volume up",
            Media::VolumeDown => "Volume down",
            Media::Mute => "Mute",
            Media::BrightnessUp => "Brightness up",
            Media::BrightnessDown => "Brightness down",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Keep what the dock sends.
    Original,
    /// Do nothing at all.
    Disabled,
    Media {
        key: Media,
    },
    Shortcut {
        ctrl: bool,
        alt: bool,
        shift: bool,
        cmd: bool,
        key: String,
    },
    OpenApp {
        app: String,
    },
    Command {
        command: String,
    },
}

impl Action {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Action::Original => "Keep original",
            Action::Disabled => "Do nothing",
            Action::Media { .. } => "Media key",
            Action::Shortcut { .. } => "Keyboard shortcut",
            Action::OpenApp { .. } => "Open application",
            Action::Command { .. } => "Run command",
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Action::Original => "Original".into(),
            Action::Disabled => "Disabled".into(),
            Action::Media { key } => key.label().into(),
            Action::Shortcut {
                ctrl,
                alt,
                shift,
                cmd,
                key,
            } => {
                let mut parts = Vec::new();
                if *ctrl {
                    parts.push("Ctrl".to_string());
                }
                if *alt {
                    parts.push(
                        if cfg!(target_os = "macos") {
                            "Option"
                        } else {
                            "Alt"
                        }
                        .to_string(),
                    );
                }
                if *shift {
                    parts.push("Shift".to_string());
                }
                if *cmd {
                    parts.push(
                        if cfg!(target_os = "macos") {
                            "Cmd"
                        } else {
                            "Super"
                        }
                        .to_string(),
                    );
                }
                parts.push(if key.len() == 1 {
                    key.to_uppercase()
                } else {
                    key.clone()
                });
                parts.join("+")
            }
            Action::OpenApp { app } => {
                let name = std::path::Path::new(app)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| app.clone());
                format!("Open {name}")
            }
            Action::Command { command } => format!("$ {command}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Button {
    pub name: String,
    pub trigger: Trigger,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub buttons: Vec<Button>,
}

impl Default for Config {
    /// Factory behaviour captured from the dock on 2026-10-15.
    fn default() -> Self {
        let button = |name: &str, trigger| Button {
            name: name.into(),
            trigger,
            action: Action::Original,
        };
        Config {
            buttons: vec![
                button("Top left", Trigger::Consumer { usage: 0x00B6 }),
                button("Top right", Trigger::Consumer { usage: 0x00B5 }),
                button(
                    "Bottom left",
                    Trigger::Key {
                        mods: MOD_LCTRL | MOD_LGUI,
                        key: 0x14,
                    },
                ),
                button(
                    "Bottom right",
                    Trigger::Key {
                        mods: MOD_LSHIFT | MOD_LGUI,
                        key: 0x21,
                    },
                ),
            ],
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("quadkey")
            .join("config.json")
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Config>(&text).ok())
            .filter(|c| c.buttons.len() == 4)
            .unwrap_or_default()
    }

    pub fn save_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Index of the button whose trigger matches what the dock just sent.
    pub fn button_for(&self, trigger: &Trigger) -> Option<usize> {
        self.buttons
            .iter()
            .position(|b| match (&b.trigger, trigger) {
                (Trigger::Key { mods: a, key: k1 }, Trigger::Key { mods: b, key: k2 }) => {
                    k1 == k2 && Trigger::normalized_mods(*a) == Trigger::normalized_mods(*b)
                }
                (x, y) => x == y,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_triggers_match_capture() {
        let c = Config::default();
        assert_eq!(c.button_for(&Trigger::Consumer { usage: 0xB6 }), Some(0));
        assert_eq!(
            c.button_for(&Trigger::Key {
                mods: MOD_LCTRL | MOD_LGUI,
                key: 0x14
            }),
            Some(2)
        );
        // Right-hand modifiers match the same button
        assert_eq!(
            c.button_for(&Trigger::Key {
                mods: 0x20 | 0x80,
                key: 0x21
            }),
            Some(3)
        );
    }

    #[test]
    fn labels() {
        assert_eq!(Trigger::Consumer { usage: 0xB5 }.label(), "Next track");
        assert!(
            Trigger::Key {
                mods: MOD_LSHIFT | MOD_LGUI,
                key: 0x21
            }
            .label()
            .ends_with("+4")
        );
        assert_eq!(key_label(0x14), "Q");
    }

    #[test]
    fn config_roundtrip() {
        let mut c = Config::default();
        c.buttons[2].action = Action::OpenApp {
            app: "/Applications/Zoom.app".into(),
        };
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}

/// One-click actions offered in the editor. Shortcuts are the macOS ones on macOS.
pub fn presets() -> Vec<(&'static str, Action)> {
    let shortcut = |ctrl, alt, shift, cmd, key: &str| Action::Shortcut {
        ctrl,
        alt,
        shift,
        cmd,
        key: key.into(),
    };
    let media = |key| Action::Media { key };
    let mut list = vec![
        ("Play / Pause", media(Media::PlayPause)),
        ("Next track", media(Media::Next)),
        ("Previous track", media(Media::Previous)),
        ("Volume up", media(Media::VolumeUp)),
        ("Volume down", media(Media::VolumeDown)),
        ("Mute sound", media(Media::Mute)),
    ];
    if cfg!(target_os = "macos") {
        list.extend([
            (
                "Mission Control",
                shortcut(true, false, false, false, "up_arrow"),
            ),
            ("Spotlight", shortcut(false, false, false, true, "spacebar")),
            ("Screenshot area", shortcut(false, false, true, true, "4")),
            (
                "Screenshot to clipboard",
                shortcut(true, false, true, true, "4"),
            ),
            ("Lock screen", shortcut(true, false, false, true, "q")),
            ("Zoom: mute mic", shortcut(false, false, true, true, "a")),
            ("Teams: mute mic", shortcut(false, false, true, true, "m")),
            ("Copy", shortcut(false, false, false, true, "c")),
            ("Paste", shortcut(false, false, false, true, "v")),
        ]);
    } else {
        list.extend([
            ("Screenshot", shortcut(false, false, false, false, "f13")), // placeholder, many desktops use Print
            ("Lock screen", shortcut(false, false, false, true, "l")),
            ("Zoom: mute mic", shortcut(false, true, false, false, "a")),
            ("Teams: mute mic", shortcut(true, false, true, false, "m")),
            ("Copy", shortcut(true, false, false, false, "c")),
            ("Paste", shortcut(true, false, false, false, "v")),
        ]);
    }
    list.push(("Disable button", Action::Disabled));
    list
}

/// Karabiner key name for a key pressed in the UI (shortcut recording).
pub fn key_name_from_egui(key: egui::Key) -> Option<String> {
    use egui::Key;
    let name = key.name();
    Some(match key {
        Key::Space => "spacebar".into(),
        Key::Enter => "return_or_enter".into(),
        Key::Escape => "escape".into(),
        Key::Tab => "tab".into(),
        Key::Backspace => "delete_or_backspace".into(),
        Key::Delete => "delete_forward".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::PageUp => "page_up".into(),
        Key::PageDown => "page_down".into(),
        Key::ArrowUp => "up_arrow".into(),
        Key::ArrowDown => "down_arrow".into(),
        Key::ArrowLeft => "left_arrow".into(),
        Key::ArrowRight => "right_arrow".into(),
        Key::Minus => "hyphen".into(),
        Key::Equals => "equal_sign".into(),
        Key::Comma => "comma".into(),
        Key::Period => "period".into(),
        Key::Slash => "slash".into(),
        Key::Semicolon => "semicolon".into(),
        Key::OpenBracket => "open_bracket".into(),
        Key::CloseBracket => "close_bracket".into(),
        _ if name.len() == 1 => name.to_lowercase(),
        _ if name.starts_with('F')
            && name[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) =>
        {
            name.to_lowercase()
        }
        _ => return None,
    })
}

#[cfg(test)]
mod preset_tests {
    use super::*;

    #[test]
    fn presets_are_valid_and_unique() {
        let list = presets();
        let mut names: Vec<_> = list.iter().map(|(n, _)| *n).collect();
        names.dedup();
        assert_eq!(names.len(), list.len());
        let keys = shortcut_keys();
        for (_, action) in &list {
            if let Action::Shortcut { key, .. } = action {
                assert!(keys.contains(key), "unknown key {key}");
            }
        }
    }

    #[test]
    fn egui_keys_map_to_known_names() {
        let keys = shortcut_keys();
        for key in [
            egui::Key::A,
            egui::Key::Num5,
            egui::Key::F13,
            egui::Key::Space,
            egui::Key::ArrowUp,
            egui::Key::Minus,
        ] {
            let name = key_name_from_egui(key).unwrap();
            assert!(keys.contains(&name), "{name}");
        }
        assert_eq!(key_name_from_egui(egui::Key::Num5).as_deref(), Some("5"));
    }
}

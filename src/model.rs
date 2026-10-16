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
        0x46 => "print_screen".into(),
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
    /// Alternates between two actions: first press does `first`, the next one `second`.
    Toggle {
        first: Box<Action>,
        second: Box<Action>,
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
            Action::Toggle { .. } => "Toggle between two",
        }
    }

    /// Actions the dock can be told to do on its own (keys); the rest needs the computer.
    pub fn needs_computer(&self) -> bool {
        match self {
            Action::OpenApp { .. } | Action::Command { .. } => true,
            Action::Toggle { first, second } => first.needs_computer() || second.needs_computer(),
            _ => false,
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
            Action::Toggle { first, second } => {
                format!("{} / {}", first.summary(), second.summary())
            }
        }
    }
}

/// Number of controls: four buttons and the knob (turn left, turn right, press).
pub const CONTROLS: usize = 7;
pub const KNOB_LEFT: usize = 4;
pub const KNOB_RIGHT: usize = 5;
pub const KNOB_PRESS: usize = 6;

pub fn is_knob(index: usize) -> bool {
    index >= KNOB_LEFT
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Button {
    pub name: String,
    pub trigger: Trigger,
    pub action: Action,
}

/// Which operating system's shortcuts the quick picks use. Left to the user: the dock may be
/// plugged into a Mac today and a Linux machine tomorrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetOs {
    Mac,
    Linux,
}

impl TargetOs {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            TargetOs::Mac
        } else {
            TargetOs::Linux
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TargetOs::Mac => "macOS",
            TargetOs::Linux => "Linux",
        }
    }
}

impl Default for TargetOs {
    fn default() -> Self {
        TargetOs::current()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub buttons: Vec<Button>,
    #[serde(default)]
    pub target_os: TargetOs,
    /// The welcome guide is shown until the user dismisses it once.
    #[serde(default)]
    pub seen_guide: bool,
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
                // The knob sends ordinary media keys, captured on 2026-10-15
                button("Knob left", Trigger::Consumer { usage: 0x00EA }),
                button("Knob right", Trigger::Consumer { usage: 0x00E9 }),
                button("Knob press", Trigger::Consumer { usage: 0x00E2 }),
            ],
            target_os: TargetOs::current(),
            seen_guide: false,
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
            .map(|mut c| {
                // Settings saved before the knob was configurable: add its controls
                if c.buttons.len() == 4 {
                    c.buttons
                        .extend(Config::default().buttons.into_iter().skip(4));
                }
                c
            })
            .filter(|c| c.buttons.len() == CONTROLS)
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
    fn knob_controls_and_migration() {
        let c = Config::default();
        assert_eq!(c.buttons.len(), CONTROLS);
        assert_eq!(
            c.button_for(&Trigger::Consumer { usage: 0xE9 }),
            Some(KNOB_RIGHT)
        );
        assert_eq!(
            c.button_for(&Trigger::Consumer { usage: 0xEA }),
            Some(KNOB_LEFT)
        );
        // An old four-button file gains the knob and keeps its buttons
        let dir = std::env::temp_dir().join(format!("quadkey-migrate-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut old = Config::default();
        old.buttons.truncate(4);
        old.buttons[0].action = Action::Disabled;
        old.save_to(&path).unwrap();
        let loaded = Config::load_from(&path);
        assert_eq!(loaded.buttons.len(), CONTROLS);
        assert_eq!(loaded.buttons[0].action, Action::Disabled);
        assert_eq!(
            loaded.buttons[KNOB_PRESS].trigger,
            Trigger::Consumer { usage: 0xE2 }
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn toggle_summary_and_roundtrip() {
        let toggle = Action::Toggle {
            first: Box::new(Action::Media {
                key: Media::PlayPause,
            }),
            second: Box::new(Action::Disabled),
        };
        assert_eq!(toggle.summary(), "Play / Pause / Disabled");
        assert!(!toggle.needs_computer());
        let json = serde_json::to_string(&toggle).unwrap();
        assert_eq!(serde_json::from_str::<Action>(&json).unwrap(), toggle);
        let with_command = Action::Toggle {
            first: Box::new(Action::Command {
                command: "true".into(),
            }),
            second: Box::new(Action::Disabled),
        };
        assert!(with_command.needs_computer());
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Media,
    Screen,
    Windows,
    Meetings,
    Editing,
    Custom,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Media,
        Category::Screen,
        Category::Windows,
        Category::Meetings,
        Category::Editing,
        Category::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Media => "Media",
            Category::Screen => "Screen",
            Category::Windows => "Windows",
            Category::Meetings => "Meetings",
            Category::Editing => "Editing",
            Category::Custom => "Custom",
        }
    }

    pub fn icon(self) -> &'static str {
        use egui_phosphor::regular as i;
        match self {
            Category::Media => i::MUSIC_NOTES,
            Category::Screen => i::MONITOR,
            Category::Windows => i::SQUARES_FOUR,
            Category::Meetings => i::VIDEO_CAMERA,
            Category::Editing => i::PENCIL_SIMPLE,
            Category::Custom => i::SLIDERS_HORIZONTAL,
        }
    }
}

/// One entry of the quick-pick catalog. `mac` / `linux` is `None` when the OS has no
/// standard shortcut for it - the entry is then hidden for that OS.
#[derive(Clone, Debug)]
pub struct Preset {
    pub category: Category,
    pub name: &'static str,
    pub hint: &'static str,
    pub icon: &'static str,
    pub mac: Option<Action>,
    pub linux: Option<Action>,
}

impl Preset {
    /// Actions that undo themselves on the next press (mute / unmute, play / pause...). They
    /// need no "toggle between two".
    pub fn switches_back(&self) -> bool {
        matches!(
            self.name,
            "Play / Pause"
                | "Mute / unmute sound"
                | "Zoom: mute mic"
                | "Zoom: camera"
                | "Teams: mute mic"
                | "Teams: camera"
                | "Meet: mute mic"
                | "Meet: camera"
                | "Mute microphone (all apps)"
                | "Dark mode on/off"
                | "Show desktop"
                | "All windows"
        )
    }

    pub fn action(&self, os: TargetOs) -> Option<&Action> {
        match os {
            TargetOs::Mac => self.mac.as_ref(),
            TargetOs::Linux => self.linux.as_ref(),
        }
    }
}

fn keys(ctrl: bool, alt: bool, shift: bool, cmd: bool, key: &str) -> Option<Action> {
    Some(Action::Shortcut {
        ctrl,
        alt,
        shift,
        cmd,
        key: key.into(),
    })
}

fn media(key: Media) -> Option<Action> {
    Some(Action::Media { key })
}

fn cmd(command: &str) -> Option<Action> {
    Some(Action::Command {
        command: command.into(),
    })
}

/// Mutes the input by setting its volume to 0 and remembers the old level for unmuting.
pub const MAC_MIC_TOGGLE: &str = r#"f="$HOME/.quadkey-mic"; v=$(osascript -e 'input volume of (get volume settings)'); if [ "$v" -gt 0 ]; then echo "$v" > "$f"; osascript -e 'set volume input volume 0' -e 'display notification "Microphone muted" with title "QuadKey"'; else osascript -e "set volume input volume $(cat "$f" 2>/dev/null || echo 75)" -e 'display notification "Microphone on" with title "QuadKey"'; fi"#;
pub const LINUX_MIC_TOGGLE: &str =
    "wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle || pactl set-source-mute @DEFAULT_SOURCE@ toggle";
pub const MAC_DARK_TOGGLE: &str = r#"osascript -e 'tell application "System Events" to tell appearance preferences to set dark mode to not dark mode'"#;
pub const LINUX_DARK_TOGGLE: &str = r#"k=org.gnome.desktop.interface; if [ "$(gsettings get $k color-scheme)" = "'prefer-dark'" ]; then gsettings set $k color-scheme default; else gsettings set $k color-scheme prefer-dark; fi"#;

/// Everything offered as a one-click choice, grouped by category.
pub fn catalog() -> Vec<Preset> {
    use Category as C;
    use egui_phosphor::regular as i;
    let p = |category, name, hint, icon, mac, linux| Preset {
        category,
        name,
        hint,
        icon,
        mac,
        linux,
    };
    // keys(ctrl, alt, shift, cmd/super, key)
    vec![
        p(
            C::Media,
            "Play / Pause",
            "Start or pause music and video",
            i::PLAY_PAUSE,
            media(Media::PlayPause),
            media(Media::PlayPause),
        ),
        p(
            C::Media,
            "Next track",
            "Skip to the next song",
            i::SKIP_FORWARD,
            media(Media::Next),
            media(Media::Next),
        ),
        p(
            C::Media,
            "Previous track",
            "Go back one song",
            i::SKIP_BACK,
            media(Media::Previous),
            media(Media::Previous),
        ),
        p(
            C::Media,
            "Volume up",
            "Make it louder",
            i::SPEAKER_HIGH,
            media(Media::VolumeUp),
            media(Media::VolumeUp),
        ),
        p(
            C::Media,
            "Volume down",
            "Make it quieter",
            i::SPEAKER_LOW,
            media(Media::VolumeDown),
            media(Media::VolumeDown),
        ),
        p(
            C::Media,
            "Mute / unmute sound",
            "Press again to hear sound again",
            i::SPEAKER_SLASH,
            media(Media::Mute),
            media(Media::Mute),
        ),
        p(
            C::Media,
            "Brightness up",
            "Brighter display",
            i::SUN,
            keys(false, false, false, false, "f15"),
            media(Media::BrightnessUp),
        ),
        p(
            C::Media,
            "Brightness down",
            "Dimmer display",
            i::SUN_DIM,
            keys(false, false, false, false, "f14"),
            media(Media::BrightnessDown),
        ),
        p(
            C::Screen,
            "Screenshot",
            "Capture the whole screen",
            i::CAMERA,
            keys(false, false, true, true, "3"),
            keys(false, false, true, false, "print_screen"),
        ),
        p(
            C::Screen,
            "Screenshot of an area",
            "Drag to select what to capture",
            i::CROP,
            keys(false, false, true, true, "4"),
            keys(false, false, false, false, "print_screen"),
        ),
        p(
            C::Screen,
            "Screenshot to clipboard",
            "Area capture, ready to paste",
            i::CLIPBOARD_TEXT,
            keys(true, false, true, true, "4"),
            None,
        ),
        p(
            C::Screen,
            "Record the screen",
            "Open the screen recording tools",
            i::RECORD,
            keys(false, false, true, true, "5"),
            keys(true, true, true, false, "r"),
        ),
        p(
            C::Screen,
            "Lock screen",
            "Lock the computer right away",
            i::LOCK,
            keys(true, false, false, true, "q"),
            keys(false, false, false, true, "l"),
        ),
        p(
            C::Windows,
            "All windows",
            "Mission Control / Activities overview",
            i::SQUARES_FOUR,
            keys(true, false, false, false, "up_arrow"),
            keys(false, false, false, true, "s"),
        ),
        p(
            C::Windows,
            "Show desktop",
            "Move all windows out of the way",
            i::DESKTOP,
            keys(false, false, false, false, "f11"),
            keys(false, false, false, true, "d"),
        ),
        p(
            C::Windows,
            "Next desktop",
            "Switch to the workspace on the right",
            i::ARROW_RIGHT,
            keys(true, false, false, false, "right_arrow"),
            keys(false, false, false, true, "page_down"),
        ),
        p(
            C::Windows,
            "Previous desktop",
            "Switch to the workspace on the left",
            i::ARROW_LEFT,
            keys(true, false, false, false, "left_arrow"),
            keys(false, false, false, true, "page_up"),
        ),
        p(
            C::Windows,
            "Search apps",
            "Spotlight / application search",
            i::MAGNIFYING_GLASS,
            keys(false, false, false, true, "spacebar"),
            keys(false, false, false, true, "a"),
        ),
        p(
            C::Windows,
            "Emoji picker",
            "Insert an emoji",
            i::SMILEY,
            keys(true, false, false, true, "spacebar"),
            None,
        ),
        p(
            C::Windows,
            "Close window",
            "Close the current window or tab",
            i::X_SQUARE,
            keys(false, false, false, true, "w"),
            keys(true, false, false, false, "w"),
        ),
        p(
            C::Meetings,
            "Zoom: mute mic",
            "Mute or unmute yourself in Zoom",
            i::MICROPHONE_SLASH,
            keys(false, false, true, true, "a"),
            keys(false, true, false, false, "a"),
        ),
        p(
            C::Meetings,
            "Zoom: camera",
            "Turn your Zoom video on or off",
            i::VIDEO_CAMERA_SLASH,
            keys(false, false, true, true, "v"),
            keys(false, true, false, false, "v"),
        ),
        p(
            C::Meetings,
            "Teams: mute mic",
            "Mute or unmute yourself in Teams",
            i::MICROPHONE_SLASH,
            keys(false, false, true, true, "m"),
            keys(true, false, true, false, "m"),
        ),
        p(
            C::Meetings,
            "Teams: camera",
            "Turn your Teams video on or off",
            i::VIDEO_CAMERA_SLASH,
            keys(false, false, true, true, "o"),
            keys(true, false, true, false, "o"),
        ),
        p(
            C::Meetings,
            "Meet: mute mic",
            "Mute or unmute yourself in Google Meet",
            i::MICROPHONE_SLASH,
            keys(false, false, false, true, "d"),
            keys(true, false, false, false, "d"),
        ),
        p(
            C::Meetings,
            "Meet: camera",
            "Turn your Google Meet video on or off",
            i::VIDEO_CAMERA_SLASH,
            keys(false, false, false, true, "e"),
            keys(true, false, false, false, "e"),
        ),
        p(
            C::Meetings,
            "Mute microphone (all apps)",
            "Toggle the system microphone, press again to unmute",
            i::MICROPHONE_SLASH,
            cmd(MAC_MIC_TOGGLE),
            cmd(LINUX_MIC_TOGGLE),
        ),
        p(
            C::Screen,
            "Dark mode on/off",
            "Switch between light and dark appearance",
            i::MOON,
            cmd(MAC_DARK_TOGGLE),
            cmd(LINUX_DARK_TOGGLE),
        ),
        p(
            C::Editing,
            "Copy",
            "Copy the selection",
            i::COPY,
            keys(false, false, false, true, "c"),
            keys(true, false, false, false, "c"),
        ),
        p(
            C::Editing,
            "Paste",
            "Paste from the clipboard",
            i::CLIPBOARD,
            keys(false, false, false, true, "v"),
            keys(true, false, false, false, "v"),
        ),
        p(
            C::Editing,
            "Cut",
            "Cut the selection",
            i::SCISSORS,
            keys(false, false, false, true, "x"),
            keys(true, false, false, false, "x"),
        ),
        p(
            C::Editing,
            "Undo",
            "Take back the last change",
            i::ARROW_U_UP_LEFT,
            keys(false, false, false, true, "z"),
            keys(true, false, false, false, "z"),
        ),
        p(
            C::Editing,
            "Redo",
            "Redo what you undid",
            i::ARROW_U_UP_RIGHT,
            keys(false, false, true, true, "z"),
            keys(true, false, true, false, "z"),
        ),
        p(
            C::Editing,
            "Save",
            "Save the current document",
            i::FLOPPY_DISK,
            keys(false, false, false, true, "s"),
            keys(true, false, false, false, "s"),
        ),
        p(
            C::Editing,
            "Find",
            "Search in the current document",
            i::TEXT_AA,
            keys(false, false, false, true, "f"),
            keys(true, false, false, false, "f"),
        ),
        p(
            C::Editing,
            "Zoom in",
            "Make the page or document bigger",
            i::MAGNIFYING_GLASS_PLUS,
            keys(false, false, false, true, "equal_sign"),
            keys(true, false, false, false, "equal_sign"),
        ),
        p(
            C::Editing,
            "Zoom out",
            "Make the page or document smaller",
            i::MAGNIFYING_GLASS_MINUS,
            keys(false, false, false, true, "hyphen"),
            keys(true, false, false, false, "hyphen"),
        ),
        p(
            C::Editing,
            "New tab",
            "Open a new browser or editor tab",
            i::PLUS_SQUARE,
            keys(false, false, false, true, "t"),
            keys(true, false, false, false, "t"),
        ),
    ]
}

/// The catalog entries available for one OS, with the action to assign.
pub fn presets_for(os: TargetOs) -> Vec<(Preset, Action)> {
    catalog()
        .into_iter()
        .filter_map(|p| p.action(os).cloned().map(|a| (p, a)))
        .collect()
}

/// Catalog entry that matches an action (used to show its name and icon), if any.
pub fn preset_for_action(action: &Action, os: TargetOs) -> Option<Preset> {
    presets_for(os)
        .into_iter()
        .find(|(_, a)| a == action)
        .map(|(p, _)| p)
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
    fn catalog_is_valid_for_both_systems() {
        let keys = shortcut_keys();
        for os in [TargetOs::Mac, TargetOs::Linux] {
            let list = presets_for(os);
            assert!(
                list.len() >= 25,
                "{os:?} has only {} quick picks",
                list.len()
            );
            let mut names: Vec<_> = list.iter().map(|(p, _)| p.name).collect();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), list.len(), "duplicate names for {os:?}");
            for (preset, action) in &list {
                if let Action::Shortcut { key, .. } = action {
                    assert!(keys.contains(key), "{}: unknown key {key}", preset.name);
                }
                assert!(!preset.hint.is_empty());
            }
            // Every category except Custom (handled by the UI) has something to offer
            for cat in Category::ALL.iter().filter(|c| **c != Category::Custom) {
                assert!(
                    list.iter().any(|(p, _)| p.category == *cat),
                    "{os:?}: empty {cat:?}"
                );
            }
        }
    }

    #[test]
    fn toggle_commands_are_valid_shell() {
        for command in [
            MAC_MIC_TOGGLE,
            LINUX_MIC_TOGGLE,
            MAC_DARK_TOGGLE,
            LINUX_DARK_TOGGLE,
        ] {
            let status = std::process::Command::new("sh")
                .args(["-n", "-c", command])
                .status()
                .unwrap();
            assert!(status.success(), "invalid shell: {command}");
        }
    }

    #[test]
    fn shortcuts_differ_per_system() {
        let lock = catalog()
            .into_iter()
            .find(|p| p.name == "Lock screen")
            .unwrap();
        assert_eq!(
            lock.action(TargetOs::Mac),
            keys(true, false, false, true, "q").as_ref()
        );
        assert_eq!(
            lock.action(TargetOs::Linux),
            keys(false, false, false, true, "l").as_ref()
        );
        let mac_only = catalog()
            .into_iter()
            .find(|p| p.name == "Emoji picker")
            .unwrap();
        assert!(mac_only.action(TargetOs::Linux).is_none());
        assert!(
            presets_for(TargetOs::Linux)
                .iter()
                .all(|(p, _)| p.name != "Emoji picker")
        );
    }

    #[test]
    fn preset_lookup_by_action() {
        let play = Action::Media {
            key: Media::PlayPause,
        };
        assert_eq!(
            preset_for_action(&play, TargetOs::Mac).unwrap().name,
            "Play / Pause"
        );
        assert!(preset_for_action(&Action::Disabled, TargetOs::Mac).is_none());
    }

    #[test]
    fn old_config_without_new_fields_still_loads() {
        let json = serde_json::to_value(Config::default()).unwrap();
        let mut obj = json.as_object().unwrap().clone();
        obj.remove("target_os");
        obj.remove("seen_guide");
        let back: Config = serde_json::from_value(serde_json::Value::Object(obj)).unwrap();
        assert_eq!(back.target_os, TargetOs::current());
        assert!(!back.seen_guide);
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

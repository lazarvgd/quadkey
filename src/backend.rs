//! Applying the button configuration to the operating system.
//!
//! macOS: a Karabiner-Elements rule limited to the dock (by vendor/product id). Karabiner
//! seizes the device, so the original shortcuts (lock screen, screenshot) never reach macOS.
//! Linux: a keyd configuration for the dock; actions that run programs are executed by
//! QuadKey itself, because keyd runs as root.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::device::NOTIFY_PORT;
use crate::model::{
    Action, Config, MOD_LALT, MOD_LCTRL, MOD_LGUI, MOD_LSHIFT, Media, PID, Trigger, VID, key_name,
};

pub const RULE_DESCRIPTION: &str = "QuadKey - USB dock fff1:00c2 (managed automatically)";

// ---------------------------------------------------------------- Karabiner

pub fn karabiner_installed() -> bool {
    [
        "/Applications/Karabiner-Elements.app",
        "/Library/Application Support/org.pqrs/Karabiner-Elements",
    ]
    .iter()
    .any(|p| std::path::Path::new(p).exists())
}

pub fn karabiner_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/karabiner/karabiner.json")
}

fn karabiner_consumer(usage: u16) -> Option<&'static str> {
    Some(match usage {
        0x00B5 => "scan_next_track",
        0x00B6 => "scan_previous_track",
        0x00CD => "play_or_pause",
        0x00E2 => "mute",
        0x00E9 => "volume_increment",
        0x00EA => "volume_decrement",
        0x006F => "display_brightness_increment",
        0x0070 => "display_brightness_decrement",
        _ => return None,
    })
}

fn media_consumer_usage(media: Media) -> u16 {
    match media {
        Media::PlayPause => 0x00CD,
        Media::Next => 0x00B5,
        Media::Previous => 0x00B6,
        Media::VolumeUp => 0x00E9,
        Media::VolumeDown => 0x00EA,
        Media::Mute => 0x00E2,
        Media::BrightnessUp => 0x006F,
        Media::BrightnessDown => 0x0070,
    }
}

fn karabiner_mods(mods: u8) -> Vec<&'static str> {
    let m = Trigger::normalized_mods(mods);
    let mut out = Vec::new();
    if m & MOD_LCTRL != 0 {
        out.push("left_control");
    }
    if m & MOD_LSHIFT != 0 {
        out.push("left_shift");
    }
    if m & MOD_LALT != 0 {
        out.push("left_option");
    }
    if m & MOD_LGUI != 0 {
        out.push("left_command");
    }
    out
}

/// The event that the trigger itself corresponds to, as a Karabiner "to" event.
fn karabiner_trigger_as_event(trigger: &Trigger) -> Option<Value> {
    match *trigger {
        Trigger::Consumer { usage } => {
            karabiner_consumer(usage).map(|c| json!({ "consumer_key_code": c }))
        }
        Trigger::Key { mods, key } => {
            key_name(key).map(|k| json!({ "key_code": k, "modifiers": karabiner_mods(mods) }))
        }
    }
}

fn karabiner_from(trigger: &Trigger) -> Option<Value> {
    match *trigger {
        Trigger::Consumer { usage } => karabiner_consumer(usage).map(|c| json!({ "consumer_key_code": c })),
        Trigger::Key { mods, key } => key_name(key).map(|k| {
            json!({ "key_code": k, "modifiers": { "mandatory": karabiner_mods(mods), "optional": ["caps_lock"] } })
        }),
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn karabiner_action(action: &Action, trigger: &Trigger) -> Option<Value> {
    Some(match action {
        Action::Original => return karabiner_trigger_as_event(trigger),
        Action::Disabled => json!({ "key_code": "vk_none" }),
        Action::Media { key } => {
            json!({ "consumer_key_code": karabiner_consumer(media_consumer_usage(*key))? })
        }
        Action::Shortcut {
            ctrl,
            alt,
            shift,
            cmd,
            key,
        } => {
            let mut mods = Vec::new();
            if *ctrl {
                mods.push("left_control");
            }
            if *alt {
                mods.push("left_option");
            }
            if *shift {
                mods.push("left_shift");
            }
            if *cmd {
                mods.push("left_command");
            }
            json!({ "key_code": key, "modifiers": mods })
        }
        Action::OpenApp { app } => {
            json!({ "shell_command": format!("open -a {}", shell_quote(app)) })
        }
        Action::Command { command } => json!({ "shell_command": command }),
    })
}

/// The complex-modification rule for the dock.
pub fn karabiner_rule(config: &Config) -> Value {
    let manipulators: Vec<Value> = config
        .buttons
        .iter()
        .enumerate()
        .filter_map(|(i, button)| {
            let from = karabiner_from(&button.trigger)?;
            let mut to = Vec::new();
            if let Some(event) = karabiner_action(&button.action, &button.trigger) {
                to.push(event);
            }
            // Tell QuadKey which button was pressed (Karabiner hides the raw reports from it)
            to.push(json!({
                "shell_command": format!("/usr/bin/printf {} | /usr/bin/nc -u -w0 127.0.0.1 {NOTIFY_PORT}", i + 1)
            }));
            Some(json!({
                "type": "basic",
                "from": from,
                "to": to,
                "conditions": [{
                    "type": "device_if",
                    "identifiers": [{ "vendor_id": VID, "product_id": PID }]
                }]
            }))
        })
        .collect();
    json!({ "description": RULE_DESCRIPTION, "manipulators": manipulators })
}

/// Insert or replace the dock rule in the selected Karabiner profile.
pub fn update_karabiner_json(mut root: Value, rule: Option<Value>) -> Result<Value> {
    let profiles = root
        .get_mut("profiles")
        .and_then(Value::as_array_mut)
        .context("karabiner.json has no profiles")?;
    if profiles.is_empty() {
        bail!("karabiner.json has no profiles");
    }
    let index = profiles
        .iter()
        .position(|p| p.get("selected").and_then(Value::as_bool) == Some(true))
        .unwrap_or(0);
    let profile = profiles[index].as_object_mut().context("invalid profile")?;
    let complex = profile
        .entry("complex_modifications")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("invalid complex_modifications")?;
    let rules = complex
        .entry("rules")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("invalid rules")?;
    rules.retain(|r| r.get("description").and_then(Value::as_str) != Some(RULE_DESCRIPTION));
    if let Some(rule) = rule {
        rules.insert(0, rule);
    }
    Ok(root)
}

fn write_karabiner(path: &Path, rule: Option<Value>) -> Result<()> {
    let current = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).context("karabiner.json is not valid JSON")?,
        Err(_) => json!({ "profiles": [{ "name": "Default profile", "selected": true }] }),
    };
    if path.exists() {
        // One backup per change, next to the original
        std::fs::copy(path, path.with_extension("json.quadkey.bak"))?;
    } else if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let updated = update_karabiner_json(current, rule)?;
    std::fs::write(path, serde_json::to_string_pretty(&updated)?)?;
    Ok(())
}

/// Where and how the configuration is applied; replaced by temporary paths in tests.
#[derive(Clone, Debug)]
pub struct Environment {
    pub karabiner_installed: bool,
    pub karabiner_json: PathBuf,
    pub config_file: PathBuf,
}

impl Environment {
    pub fn system() -> Self {
        Environment {
            karabiner_installed: karabiner_installed(),
            karabiner_json: karabiner_config_path(),
            config_file: Config::path(),
        }
    }

    pub fn apply(&self, config: &Config) -> Result<String> {
        if cfg!(target_os = "linux") {
            #[cfg(target_os = "linux")]
            keyd_apply(config)?;
            return Ok(format!("Saved to {KEYD_PATH} and reloaded keyd"));
        }
        if !self.karabiner_installed {
            bail!("Karabiner-Elements is not installed");
        }
        write_karabiner(&self.karabiner_json, Some(karabiner_rule(config)))?;
        Ok("Applied - Karabiner-Elements picked up the new rule".into())
    }

    pub fn remove(&self) -> Result<String> {
        write_karabiner(&self.karabiner_json, None)?;
        Ok("Dock rule removed - the dock behaves like before".into())
    }

    pub fn rule_active(&self) -> bool {
        std::fs::read_to_string(&self.karabiner_json).is_ok_and(|t| t.contains(RULE_DESCRIPTION))
    }
}

// ---------------------------------------------------------------- keyd (Linux)

pub const KEYD_PATH: &str = "/etc/keyd/quadkey.conf";

fn keyd_key(usage: u8) -> Option<String> {
    let name = key_name(usage)?;
    Some(
        match name.as_str() {
            "return_or_enter" => "enter",
            "escape" => "esc",
            "delete_or_backspace" => "backspace",
            "spacebar" => "space",
            "hyphen" => "minus",
            "equal_sign" => "equal",
            "open_bracket" => "leftbrace",
            "close_bracket" => "rightbrace",
            "quote" => "apostrophe",
            "delete_forward" => "delete",
            "page_up" => "pageup",
            "page_down" => "pagedown",
            "right_arrow" => "right",
            "left_arrow" => "left",
            "down_arrow" => "down",
            "up_arrow" => "up",
            "print_screen" => "sysrq",
            other => other,
        }
        .to_string(),
    )
}

fn keyd_consumer(usage: u16) -> Option<&'static str> {
    Some(match usage {
        0x00B5 => "nextsong",
        0x00B6 => "previoussong",
        0x00CD => "playpause",
        0x00E2 => "mute",
        0x00E9 => "volumeup",
        0x00EA => "volumedown",
        0x006F => "brightnessup",
        0x0070 => "brightnessdown",
        _ => return None,
    })
}

fn keyd_action(action: &Action) -> Option<String> {
    Some(match action {
        Action::Original => return None,
        // Programs are started by QuadKey in the user session - keyd only swallows the key
        Action::Disabled | Action::OpenApp { .. } | Action::Command { .. } => "noop".into(),
        Action::Media { key } => keyd_consumer(media_consumer_usage(*key))?.into(),
        Action::Shortcut {
            ctrl,
            alt,
            shift,
            cmd,
            key,
        } => {
            let mut prefix = String::new();
            for (on, p) in [(ctrl, "C-"), (alt, "A-"), (shift, "S-"), (cmd, "M-")] {
                if *on {
                    prefix.push_str(p);
                }
            }
            let usage = (0x04u8..=0x73).find(|&u| key_name(u).as_deref() == Some(key.as_str()))?;
            format!("macro({prefix}{})", keyd_key(usage)?)
        }
    })
}

/// keyd configuration for the dock only.
pub fn keyd_config(config: &Config) -> String {
    let mut main = Vec::new();
    let mut layers: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for button in &config.buttons {
        let Some(action) = keyd_action(&button.action) else {
            continue;
        };
        match button.trigger {
            Trigger::Consumer { usage } => {
                if let Some(name) = keyd_consumer(usage) {
                    main.push(format!("{name} = {action}"));
                }
            }
            Trigger::Key { mods, key } => {
                let Some(name) = keyd_key(key) else { continue };
                let m = Trigger::normalized_mods(mods);
                let layer: Vec<&str> = [
                    (MOD_LCTRL, "control"),
                    (MOD_LSHIFT, "shift"),
                    (MOD_LALT, "alt"),
                    (MOD_LGUI, "meta"),
                ]
                .iter()
                .filter(|(bit, _)| m & bit != 0)
                .map(|(_, n)| *n)
                .collect();
                if layer.is_empty() {
                    main.push(format!("{name} = {action}"));
                } else {
                    layers
                        .entry(layer.join("+"))
                        .or_default()
                        .push(format!("{name} = {action}"));
                }
            }
        }
    }
    let mut out = format!(
        "# Generated by QuadKey - changes are overwritten on Apply\n[ids]\n{VID:04x}:{PID:04x}\n\n[main]\n"
    );
    for line in main {
        out.push_str(&line);
        out.push('\n');
    }
    for (layer, lines) in layers {
        out.push_str(&format!("\n[{layer}]\n"));
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

#[cfg(target_os = "linux")]
pub fn keyd_apply(config: &Config) -> Result<()> {
    use std::io::Write;
    let mut child = std::process::Command::new("pkexec")
        .args(["sh", "-c", &format!("cat > {KEYD_PATH} && keyd reload")])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("could not start pkexec")?;
    child
        .stdin
        .take()
        .context("no stdin")?
        .write_all(keyd_config(config).as_bytes())?;
    if !child.wait()?.success() {
        bail!("writing {KEYD_PATH} failed (is keyd installed?)");
    }
    Ok(())
}

/// Linux: run the program actions ourselves (keyd swallows the key).
pub fn run_local_action(action: &Action) {
    let command = match action {
        Action::OpenApp { app } if cfg!(target_os = "linux") => app.clone(),
        Action::Command { command } if cfg!(target_os = "linux") => command.clone(),
        _ => return,
    };
    let _ = std::process::Command::new("sh")
        .args(["-c", &command])
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        let mut c = Config::default();
        c.buttons[0].action = Action::Media {
            key: Media::PlayPause,
        };
        c.buttons[2].action = Action::OpenApp { app: "Zoom".into() };
        c.buttons[3].action = Action::Disabled;
        c
    }

    #[test]
    fn karabiner_rule_is_device_specific() {
        let rule = karabiner_rule(&config());
        let m = rule["manipulators"].as_array().unwrap();
        assert_eq!(m.len(), 4);
        for manipulator in m {
            assert_eq!(
                manipulator["conditions"][0]["identifiers"][0]["vendor_id"],
                65521
            );
            assert_eq!(
                manipulator["conditions"][0]["identifiers"][0]["product_id"],
                194
            );
        }
        assert_eq!(m[0]["from"]["consumer_key_code"], "scan_previous_track");
        assert_eq!(m[0]["to"][0]["consumer_key_code"], "play_or_pause");
        assert_eq!(m[2]["from"]["key_code"], "q");
        assert_eq!(
            m[2]["from"]["modifiers"]["mandatory"],
            json!(["left_control", "left_command"])
        );
        assert_eq!(m[2]["to"][0]["shell_command"], "open -a 'Zoom'");
        assert_eq!(m[3]["to"][0]["key_code"], "vk_none");
        assert!(
            m[3]["to"][1]["shell_command"]
                .as_str()
                .unwrap()
                .contains("printf 4")
        );
        // Original keeps the trigger itself
        assert_eq!(m[1]["to"][0]["consumer_key_code"], "scan_next_track");
    }

    #[test]
    fn karabiner_json_keeps_other_rules_and_replaces_ours() {
        let existing = json!({
            "global": {"show_in_menu_bar": true},
            "profiles": [
                {"name": "Other", "selected": false},
                {"name": "Main", "selected": true, "complex_modifications": {"rules": [
                    {"description": "My own rule", "manipulators": []},
                    {"description": RULE_DESCRIPTION, "manipulators": []}
                ]}}
            ]
        });
        let updated = update_karabiner_json(existing, Some(karabiner_rule(&config()))).unwrap();
        let rules = updated["profiles"][1]["complex_modifications"]["rules"]
            .as_array()
            .unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["description"], RULE_DESCRIPTION);
        assert_eq!(rules[1]["description"], "My own rule");
        assert_eq!(updated["global"]["show_in_menu_bar"], true);
        assert!(
            updated["profiles"][0]
                .get("complex_modifications")
                .is_none()
        );

        let removed = update_karabiner_json(updated, None).unwrap();
        assert_eq!(
            removed["profiles"][1]["complex_modifications"]["rules"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn keyd_config_uses_layers_for_shortcuts() {
        let text = keyd_config(&config());
        assert!(text.contains("[ids]\nfff1:00c2"));
        assert!(text.contains("previoussong = playpause"));
        assert!(text.contains("[control+meta]\nq = noop"));
        assert!(text.contains("[shift+meta]\n4 = noop"));
        assert!(!text.contains("nextsong")); // button 2 keeps its original action
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(shell_quote("It's"), r"'It'\''s'");
    }
}

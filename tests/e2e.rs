//! End-to-end tests: the real UI driven through egui_kittest, with a fake dock (events sent
//! into the app's channel) and temporary files instead of the real Karabiner configuration.

use std::sync::mpsc::Sender;

use eframe::egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use quadkey::app::{Connection, DockStudio};
use quadkey::backend::{Environment, RULE_DESCRIPTION};
use quadkey::device::DockEvent;
use quadkey::model::{Action, Config, MOD_LCTRL, MOD_LGUI, Trigger};
use serde_json::Value;
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    env: Environment,
    dock: Sender<DockEvent>,
}

fn fixture(karabiner_installed: bool) -> (Fixture, DockStudio) {
    let dir = tempfile::tempdir().unwrap();
    let env = Environment {
        karabiner_installed,
        karabiner_json: dir.path().join("karabiner/karabiner.json"),
        config_file: dir.path().join("quadkey/config.json"),
    };
    std::fs::create_dir_all(env.karabiner_json.parent().unwrap()).unwrap();
    std::fs::write(
        &env.karabiner_json,
        r#"{"profiles":[{"name":"Default","selected":true,"complex_modifications":{"rules":[{"description":"Mine","manipulators":[]}]}}]}"#,
    )
    .unwrap();
    let (tx, rx) = DockStudio::channel();
    let app = DockStudio::new(env.clone(), rx);
    (
        Fixture {
            _dir: dir,
            env,
            dock: tx,
        },
        app,
    )
}

fn harness(app: DockStudio) -> Harness<'static, DockStudio> {
    Harness::builder()
        .with_size(Vec2::new(900.0, 640.0))
        .build_ui_state(|ui, app: &mut DockStudio| app.draw(ui), app)
}

fn karabiner(env: &Environment) -> Value {
    serde_json::from_str(&std::fs::read_to_string(&env.karabiner_json).unwrap()).unwrap()
}

fn dock_rule(env: &Environment) -> Option<Value> {
    karabiner(env)["profiles"][0]["complex_modifications"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["description"] == RULE_DESCRIPTION)
        .cloned()
}

#[test]
fn shows_the_cube_and_header() {
    let (_fx, app) = fixture(true);
    let mut h = harness(app);
    h.run();
    h.get_by_label("QuadKey");
    for n in 1..=4 {
        h.get_by_label(&format!("Button {n}"));
    }
    h.get_by_label("Button 1 · Top left");
}

#[test]
fn dock_press_lights_the_matching_tile_and_logs_it() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run();
    fx.dock
        .send(DockEvent::Connected {
            interfaces: 4,
            keyboard_ok: true,
        })
        .unwrap();
    fx.dock
        .send(DockEvent::Pressed(Trigger::Key {
            mods: MOD_LCTRL | MOD_LGUI,
            key: 0x14,
        }))
        .unwrap();
    h.step();
    let now = h.ctx.input(|i| i.time);
    assert!(h.state().is_lit(2, now), "button 3 should glow");
    assert!(!h.state().is_lit(0, now));
    assert!(h.state().log.iter().any(|l| l.starts_with("Button 3")));
    assert_eq!(
        h.state().connection,
        Connection::Connected { keyboard_ok: true }
    );
    h.get_by_label("Dock connected");
}

#[test]
fn unknown_input_is_logged_not_lit() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    fx.dock
        .send(DockEvent::Pressed(Trigger::Consumer { usage: 0x00E9 }))
        .unwrap();
    h.step();
    let now = h.ctx.input(|i| i.time);
    assert!((0..4).all(|i| !h.state().is_lit(i, now)));
    assert!(h.state().log.iter().any(|l| l.contains("Unknown input")));
}

#[test]
fn karabiner_notification_lights_tile() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    fx.dock.send(DockEvent::PressedButton(3)).unwrap();
    h.step();
    let now = h.ctx.input(|i| i.time);
    assert!(h.state().is_lit(3, now));
}

#[test]
fn select_tile_change_action_apply_and_remove() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run();

    // Select the lock-screen button
    h.get_by_label("Button 3").click();
    h.run();
    h.get_by_label("Button 3 · Bottom left");
    if cfg!(target_os = "macos") {
        h.get_by_label("macOS: Lock screen");
    }

    // Change its action to "Do nothing" through the combo box
    h.get_by_value("Keep original").click();
    h.run();
    h.get_by_label("Do nothing").click();
    h.run();
    assert_eq!(h.state().config.buttons[2].action, Action::Disabled);
    assert!(h.state().dirty);
    h.get_by_label("Unapplied changes");

    // The choice is saved right away
    let saved = Config::load_from(&fx.env.config_file);
    assert_eq!(saved.buttons[2].action, Action::Disabled);

    if !cfg!(target_os = "macos") {
        return; // applying on Linux needs keyd and pkexec
    }

    h.get_by_label("Apply").click();
    h.run();
    let (ok, message) = h.state().status.clone().unwrap();
    assert!(ok, "apply failed: {message}");
    assert!(!h.state().dirty);
    let rule = dock_rule(&fx.env).expect("dock rule written");
    let lock = rule["manipulators"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["from"]["key_code"] == "q")
        .unwrap();
    assert_eq!(lock["to"][0]["key_code"], "vk_none");
    assert_eq!(lock["conditions"][0]["identifiers"][0]["vendor_id"], 0xFFF1);
    // The user's own rule is untouched and a backup exists
    assert!(karabiner(&fx.env).to_string().contains("\"Mine\""));
    assert!(
        fx.env
            .karabiner_json
            .with_extension("json.quadkey.bak")
            .exists()
    );
    h.get_by_label("Karabiner rule active");

    h.get_by_label("Remove rule").click();
    h.run();
    assert!(dock_rule(&fx.env).is_none());
    assert!(karabiner(&fx.env).to_string().contains("\"Mine\""));
}

#[test]
fn media_action_editor() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run();
    h.get_by_value("Keep original").click();
    h.run();
    h.get_by_label("Media key").click();
    h.run();
    // Default media key, then pick another one
    assert!(matches!(
        h.state().config.buttons[0].action,
        Action::Media { .. }
    ));
    h.get_by_value("Play / Pause").click();
    h.run();
    h.get_by_label("Mute").click();
    h.run();
    assert_eq!(
        Config::load_from(&fx.env.config_file).buttons[0].action,
        Action::Media {
            key: quadkey::model::Media::Mute
        }
    );
}

#[test]
fn learn_from_dock_replaces_trigger() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run();
    h.get_by_label("Button 2").click();
    h.run();
    h.get_by_label("Learn from dock").click();
    h.run();
    assert_eq!(h.state().learning, Some(1));
    h.get_by_label("Cancel learning");

    let f13 = Trigger::Key { mods: 0, key: 0x68 };
    fx.dock.send(DockEvent::Pressed(f13)).unwrap();
    h.run_steps(3); // the tile keeps animating its glow, so step a fixed number of frames
    assert_eq!(h.state().learning, None);
    assert_eq!(h.state().config.buttons[1].trigger, f13);
    assert_eq!(
        Config::load_from(&fx.env.config_file).buttons[1].trigger,
        f13
    );

    // Reset brings back the factory trigger
    h.get_by_label("Reset button").click();
    h.run();
    assert_eq!(
        h.state().config.buttons[1].trigger,
        Config::default().buttons[1].trigger
    );
}

#[test]
fn apply_without_karabiner_shows_install_hint() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let (fx, app) = fixture(false);
    let mut h = harness(app);
    h.run();
    h.get_by_label("Karabiner-Elements not installed");
    h.get_by_label("Install Karabiner-Elements");
    h.get_by_label("Apply").click();
    h.run();
    let (ok, message) = h.state().status.clone().unwrap();
    assert!(!ok && message.contains("not installed"));
    assert!(dock_rule(&fx.env).is_none());
}

#[test]
fn disconnect_is_shown() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    fx.dock.send(DockEvent::Disconnected).unwrap();
    h.run();
    h.get_by_label("Dock not connected");
}

#[test]
fn settings_survive_restart() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run();
    h.get_by_value("Keep original").click();
    h.run();
    h.get_by_label("Run command").click();
    h.run();
    drop(h);

    let (_tx, rx) = DockStudio::channel();
    let restarted = DockStudio::new(fx.env.clone(), rx);
    assert!(matches!(
        restarted.config.buttons[0].action,
        Action::Command { .. }
    ));
}

#[test]
fn quick_pick_assigns_action_in_one_click() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run();
    h.get_by_label("Button 4").click();
    h.run();
    h.get_by_label("Play / Pause").click();
    h.run();
    assert_eq!(
        h.state().config.buttons[3].action,
        Action::Media {
            key: quadkey::model::Media::PlayPause
        }
    );
    h.get_by_label("Disable button").click();
    h.run();
    assert_eq!(
        Config::load_from(&fx.env.config_file).buttons[3].action,
        Action::Disabled
    );
}

#[test]
fn record_shortcut_from_keyboard() {
    let (_fx, app) = fixture(true);
    let mut h = harness(app);
    h.run();
    h.get_by_label("Record shortcut").click();
    h.run();
    assert!(h.state().recording);
    h.get_by_label("Press keys now... (Esc cancels)");

    let mods = eframe::egui::Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };
    h.key_press_modifiers(mods, eframe::egui::Key::K);
    h.run();
    assert!(!h.state().recording);
    assert_eq!(
        h.state().config.buttons[0].action,
        Action::Shortcut {
            ctrl: true,
            alt: false,
            shift: true,
            cmd: false,
            key: "k".into()
        }
    );
}

#[test]
fn record_shortcut_can_be_cancelled_with_escape() {
    let (_fx, app) = fixture(true);
    let mut h = harness(app);
    h.run();
    h.get_by_label("Record shortcut").click();
    h.run();
    h.key_press(eframe::egui::Key::Escape);
    h.run();
    assert!(!h.state().recording);
    assert_eq!(h.state().config.buttons[0].action, Action::Original);
}

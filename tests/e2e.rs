//! End-to-end tests: the real UI driven through egui_kittest, with a fake dock (events sent
//! into the app's channel) and temporary files instead of the real Karabiner configuration.

use std::sync::mpsc::Sender;

use eframe::egui::{Key, Modifiers, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use quadkey::app::{Connection, DockStudio, Readiness, style};
use quadkey::backend::{Environment, RULE_DESCRIPTION};
use quadkey::device::DockEvent;
use quadkey::model::{
    Action, Category, Config, KNOB_PRESS, MOD_LCTRL, MOD_LGUI, Media, TargetOs, Trigger,
};
use serde_json::Value;
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    env: Environment,
    dock: Sender<DockEvent>,
}

fn fixture_with(karabiner_installed: bool, seen_guide: bool) -> (Fixture, DockStudio) {
    let dir = tempfile::tempdir().unwrap();
    let env = Environment {
        karabiner_installed,
        karabiner_running: Some(true),
        karabiner_json: dir.path().join("karabiner/karabiner.json"),
        config_file: dir.path().join("quadkey/config.json"),
    };
    std::fs::create_dir_all(env.karabiner_json.parent().unwrap()).unwrap();
    std::fs::write(
        &env.karabiner_json,
        r#"{"profiles":[{"name":"Default","selected":true,"complex_modifications":{"rules":[{"description":"Mine","manipulators":[]}]}}]}"#,
    )
    .unwrap();
    let config = Config {
        target_os: TargetOs::Mac,
        seen_guide,
        ..Config::default()
    };
    config.save_to(&env.config_file).unwrap();
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

fn fixture(karabiner_installed: bool) -> (Fixture, DockStudio) {
    fixture_with(karabiner_installed, true)
}

fn harness(app: DockStudio) -> Harness<'static, DockStudio> {
    let h = Harness::builder()
        .with_size(Vec2::new(1100.0, 900.0))
        .build_ui_state(|ui, app: &mut DockStudio| app.draw(ui), app);
    style(&h.ctx);
    h
}

fn tab(category: Category) -> String {
    format!("{}  {}", category.icon(), category.label())
}

fn connect(fx: &Fixture) {
    fx.dock
        .send(DockEvent::Connected {
            interfaces: 4,
            keyboard_ok: true,
        })
        .unwrap();
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
fn shows_buttons_knob_and_categories() {
    let (_fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label("QuadKey");
    h.get_by_label("TobenONE UDS060");
    for n in 1..=4 {
        h.get_by_label(&format!("Dock button {n}"));
    }
    for part in ["turn left", "press", "turn right"] {
        h.get_by_label(&format!("Knob: {part}"));
    }
    h.get_by_label("Button 1");
    h.get_by_label("Top left");
    for category in Category::ALL {
        h.get_by_label(&tab(category));
    }
}

#[test]
fn nothing_to_apply_out_of_the_box() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    connect(&fx);
    h.run_steps(4);
    assert!(
        !h.state().can_apply(),
        "Apply must be disabled when nothing changed"
    );
    assert_eq!(h.state().readiness(), Readiness::Ready);
    h.get_by_label("Ready");
    // Clicking the disabled button does nothing
    h.get_by_label("Apply").click();
    h.run_steps(4);
    assert!(h.state().toast.is_none());
}

#[test]
fn dock_press_lights_the_matching_tile_and_logs_it() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    connect(&fx);
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
    assert!(
        h.state()
            .log
            .iter()
            .any(|l| l.starts_with("Button 3 · dock sent"))
    );
    assert_eq!(
        h.state().connection,
        Connection::Connected { keyboard_ok: true }
    );
}

#[test]
fn knob_turns_light_their_tiles() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    fx.dock
        .send(DockEvent::Pressed(Trigger::Consumer { usage: 0x00E9 }))
        .unwrap();
    h.step();
    let now = h.ctx.input(|i| i.time);
    assert!(h.state().is_lit(5, now), "knob right should glow");
    assert!(
        h.state()
            .log
            .iter()
            .any(|l| l.starts_with("Knob (turn right)"))
    );
}

#[test]
fn karabiner_notification_lights_tile() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    fx.dock.send(DockEvent::PressedButton(KNOB_PRESS)).unwrap();
    h.step();
    let now = h.ctx.input(|i| i.time);
    assert!(h.state().is_lit(KNOB_PRESS, now));
}

#[test]
fn choose_apply_and_reset() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    connect(&fx);
    h.run_steps(4);

    h.get_by_label("Dock button 3").click();
    h.run_steps(4);
    h.get_by_label("Button 3");
    h.get_by_label("Bottom left");
    if cfg!(target_os = "macos") {
        h.get_by_label("macOS: Lock screen");
    }

    h.get_by_label(&tab(Category::Custom)).click();
    h.run_steps(4);
    h.get_by_label("Do nothing").click();
    h.run_steps(4);
    // A choice is a draft until Apply
    assert_eq!(h.state().config.buttons[2].action, Action::Disabled);
    assert_eq!(h.state().changed_controls(), vec![2]);
    assert_eq!(
        Config::load_from(&fx.env.config_file).buttons[2].action,
        Action::Disabled
    );
    h.get_by_label("1 change not applied");
    h.get_by_label("Will do after Apply");

    if !cfg!(target_os = "macos") {
        return; // applying on Linux needs keyd and pkexec
    }
    assert!(
        dock_rule(&fx.env).is_none(),
        "nothing may change before Apply"
    );
    assert!(h.state().can_apply());
    h.get_by_label("Changes not applied");

    h.get_by_label("Apply").click();
    h.run_steps(4);
    let toast = h.state().toast.clone().expect("confirmation shown");
    assert!(toast.ok, "{}", toast.text);
    assert!(toast.text.contains("Button 3 → Nothing"), "{}", toast.text);
    assert!(
        !h.state().can_apply(),
        "Apply is disabled again after applying"
    );
    assert!(h.state().changed_controls().is_empty());
    h.get_by_label("Ready");
    let rule = dock_rule(&fx.env).expect("dock rule written");
    let lock = rule["manipulators"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["from"]["key_code"] == "q")
        .unwrap();
    assert_eq!(lock["to"][0]["key_code"], "vk_none");
    assert_eq!(lock["conditions"][0]["identifiers"][0]["vendor_id"], 0xFFF1);
    assert!(karabiner(&fx.env).to_string().contains("\"Mine\""));
    assert!(
        fx.env
            .karabiner_json
            .with_extension("json.quadkey.bak")
            .exists()
    );

    // The activity log tells what really happened
    fx.dock.send(DockEvent::PressedButton(2)).unwrap();
    h.run_steps(3);
    assert!(
        h.state().log.iter().any(|l| l == "Button 3 → Nothing"),
        "{:?}",
        h.state().log
    );

    // Applied settings survive a restart
    let (_tx, rx) = DockStudio::channel();
    let restarted = DockStudio::new(fx.env.clone(), rx);
    assert_eq!(restarted.applied.buttons[2].action, Action::Disabled);

    h.state_mut().reset_everything();
    h.run_steps(3);
    assert!(dock_rule(&fx.env).is_none());
    assert!(karabiner(&fx.env).to_string().contains("\"Mine\""));
    assert_eq!(
        h.state().config,
        Config {
            target_os: TargetOs::Mac,
            seen_guide: true,
            ..Config::default()
        }
    );
}

#[test]
fn discard_returns_to_what_is_applied() {
    let (_fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label(&tab(Category::Media)).click();
    h.run_steps(4);
    h.get_by_label("Mute sound").click();
    h.run_steps(4);
    assert_eq!(
        h.state().config.buttons[0].action,
        Action::Media { key: Media::Mute }
    );
    h.get_by_label("Discard").click();
    h.run_steps(4);
    assert_eq!(h.state().config.buttons[0].action, Action::Original);
    assert!(h.state().changed_controls().is_empty());
}

#[test]
fn every_category_offers_options_to_every_control() {
    let (_fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    let controls: Vec<String> = (1..=4)
        .map(|n| format!("Dock button {n}"))
        .chain(["Knob: turn left", "Knob: turn right", "Knob: press"].map(String::from))
        .collect();
    for (index, control) in controls.iter().enumerate() {
        h.get_by_label(control).click();
        h.run_steps(4);
        assert_eq!(h.state().selected, index);
        for (category, option) in [
            (Category::Media, "Play / Pause"),
            (Category::Screen, "Screenshot"),
            (Category::Windows, "Show desktop"),
            (Category::Meetings, "Zoom: mute mic"),
            (Category::Editing, "Copy"),
        ] {
            h.get_by_label(&tab(category)).click();
            h.run_steps(4);
            h.get_by_label(option).click();
            h.run_steps(4);
            let action = h.state().config.buttons[index].action.clone();
            assert!(
                !matches!(action, Action::Original),
                "{option} not assigned to {control}"
            );
        }
    }
}

#[test]
fn toggle_between_two_actions() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label("Dock button 2").click();
    h.run_steps(4);
    h.get_by_label(&tab(Category::Custom)).click();
    h.run_steps(4);
    h.get_by_label("Toggle between two").click();
    h.run_steps(4);
    assert!(matches!(
        h.state().config.buttons[1].action,
        Action::Toggle { .. }
    ));
    h.get_by_label("First press");
    h.get_by_label("Second press");
    if !cfg!(target_os = "macos") {
        return;
    }
    h.get_by_label("Apply").click();
    h.run_steps(4);
    let rule = dock_rule(&fx.env).unwrap();
    let halves: Vec<_> = rule["manipulators"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["from"]["consumer_key_code"] == "scan_next_track")
        .collect();
    assert_eq!(halves.len(), 2, "a toggle becomes two manipulators");
    assert_eq!(halves[0]["conditions"][1]["type"], "variable_unless");
    assert_eq!(halves[1]["conditions"][1]["type"], "variable_if");
}

#[test]
fn knob_can_be_reassigned() {
    let (_fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label("Knob: press").click();
    h.run_steps(4);
    assert_eq!(h.state().selected, KNOB_PRESS);
    h.get_by_label("Press");
    h.get_by_label(&tab(Category::Meetings)).click();
    h.run_steps(4);
    h.get_by_label("Mute microphone (all apps)").click();
    h.run_steps(4);
    assert!(matches!(
        h.state().config.buttons[KNOB_PRESS].action,
        Action::Command { .. }
    ));
    h.get_by_label("Runs on this computer while QuadKey or Karabiner is active");
}

#[test]
fn learn_from_dock_replaces_trigger() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label("Dock button 2").click();
    h.run_steps(4);
    h.get_by_label("Learn from dock").click();
    h.run_steps(4);
    assert_eq!(h.state().learning, Some(1));
    h.get_by_label("Cancel learning");

    let f13 = Trigger::Key { mods: 0, key: 0x68 };
    fx.dock.send(DockEvent::Pressed(f13)).unwrap();
    h.run_steps(3); // the tile keeps animating its glow, so step a fixed number of frames
    assert_eq!(h.state().learning, None);
    assert_eq!(h.state().config.buttons[1].trigger, f13);
}

#[test]
fn back_to_factory_for_one_control() {
    let (_fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label(&tab(Category::Media)).click();
    h.run_steps(4);
    h.get_by_label("Volume up").click();
    h.run_steps(4);
    h.get_by_label("Back to factory").click();
    h.run_steps(4);
    assert_eq!(h.state().config.buttons[0].action, Action::Original);
}

#[test]
fn karabiner_missing_is_explained() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let (fx, app) = fixture(false);
    let mut h = harness(app);
    connect(&fx);
    h.run_steps(4);
    h.get_by_label(&tab(Category::Media)).click();
    h.run_steps(4);
    h.get_by_label("Play / Pause").click();
    h.run_steps(4);
    assert_eq!(h.state().readiness(), Readiness::NeedsKarabiner);
    h.get_by_label("Install Karabiner-Elements");
    h.get_by_label("Apply").click();
    h.run_steps(4);
    let toast = h.state().toast.clone().unwrap();
    assert!(!toast.ok && toast.text.contains("Install Karabiner-Elements"));
    assert!(dock_rule(&fx.env).is_none());
}

#[test]
fn disconnect_is_shown() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    fx.dock.send(DockEvent::Disconnected).unwrap();
    h.run_steps(4);
    h.get_by_label("Dock not connected");
}

#[test]
fn custom_command_survives_restart() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label(&tab(Category::Custom)).click();
    h.run_steps(4);
    h.get_by_label("Run command").click();
    h.run_steps(4);
    h.get_by_label("Shell command");
    drop(h);

    let (_tx, rx) = DockStudio::channel();
    let restarted = DockStudio::new(fx.env.clone(), rx);
    assert!(matches!(
        restarted.config.buttons[0].action,
        Action::Command { .. }
    ));
}

#[test]
fn record_shortcut_from_keyboard() {
    let (_fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label(&tab(Category::Custom)).click();
    h.run_steps(4);
    h.get_by_label("Record shortcut").click();
    h.run_steps(4);
    assert!(h.state().recording);
    h.get_by_label("Press keys now...");

    let mods = Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };
    h.key_press_modifiers(mods, Key::K);
    h.run_steps(4);
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
    h.get_by_label("Keys to send");
}

#[test]
fn record_shortcut_can_be_cancelled_with_escape() {
    let (_fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label(&tab(Category::Custom)).click();
    h.run_steps(4);
    h.get_by_label("Record shortcut").click();
    h.run_steps(4);
    h.key_press(Key::Escape);
    h.run_steps(4);
    assert!(!h.state().recording);
    assert_eq!(h.state().config.buttons[0].action, Action::Original);
}

#[test]
fn shortcuts_follow_the_chosen_system() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label(egui_phosphor::regular::DOTS_THREE).click();
    h.run_steps(4);
    h.get_by_label("Linux").click();
    h.run_steps(4);
    assert_eq!(h.state().config.target_os, TargetOs::Linux);
    h.key_press(Key::Escape); // close the menu
    h.run_steps(4);
    h.get_by_label(&tab(Category::Screen)).click();
    h.run_steps(4);
    h.get_by_label("Lock screen").click();
    h.run_steps(4);
    assert_eq!(
        Config::load_from(&fx.env.config_file).buttons[0].action,
        Action::Shortcut {
            ctrl: false,
            alt: false,
            shift: false,
            cmd: true,
            key: "l".into()
        }
    );
    h.get_by_label(&tab(Category::Windows)).click();
    h.run_steps(4);
    assert!(
        h.query_by_label("Emoji picker").is_none(),
        "macOS-only picks are hidden for Linux"
    );
}

#[test]
fn welcome_guide_on_first_start_and_help_later() {
    let (fx, app) = fixture_with(true, false);
    let mut h = harness(app);
    h.run_steps(4);
    h.get_by_label("Welcome to QuadKey");
    h.get_by_label("Get started").click();
    h.run_steps(4);
    assert!(h.query_by_label("Welcome to QuadKey").is_none());
    assert!(Config::load_from(&fx.env.config_file).seen_guide);

    h.get_by_label_contains("Help").click();
    h.run_steps(4);
    h.get_by_label("QuadKey help");
    h.get_by_label("Every control explained").click();
    h.run_steps(4);
    h.get_by_label("Status in the top bar");
    h.get_by_label("Questions").click();
    h.run_steps(4);
    h.get_by_label("Is my setup saved?");
    h.get_by_label("Close").click();
    h.run_steps(4);
    assert!(h.query_by_label("QuadKey help").is_none());
}

#[test]
fn open_application_needs_an_app_before_apply() {
    let (fx, app) = fixture(true);
    let mut h = harness(app);
    connect(&fx);
    h.run_steps(4);
    h.get_by_label("Dock button 3").click();
    h.run_steps(4);
    h.get_by_label(&tab(Category::Custom)).click();
    h.run_steps(4);
    h.get_by_label("Open application").click();
    h.run_steps(4);
    assert!(
        !h.state().can_apply(),
        "an empty application must not be applied"
    );
    h.get_by_label("Button 3: choose an application first");
    h.get_by_label("No application chosen yet");

    // Typing the application makes it complete
    h.get_by_label("Application");
    h.state_mut().config.buttons[2].action = Action::OpenApp {
        app: "/Applications/Calculator.app".into(),
    };
    h.run_steps(4);
    h.get_by_label_contains("Opens Calculator");
    if cfg!(target_os = "macos") {
        assert!(h.state().can_apply());
        h.get_by_label("Apply").click();
        h.run_steps(4);
        let rule = dock_rule(&fx.env).unwrap();
        let m = rule["manipulators"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["from"]["key_code"] == "q")
            .unwrap();
        assert_eq!(
            m["to"][0]["shell_command"],
            "open -a '/Applications/Calculator.app'"
        );
    }
}

#[test]
fn stopped_karabiner_is_explained() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let env = Environment {
        karabiner_installed: true,
        karabiner_running: Some(false),
        karabiner_json: dir.path().join("karabiner.json"),
        config_file: dir.path().join("config.json"),
    };
    let mut config = Config {
        target_os: TargetOs::Mac,
        seen_guide: true,
        ..Config::default()
    };
    config.buttons[0].action = Action::Media {
        key: Media::PlayPause,
    };
    config.save_to(&env.config_file).unwrap();
    let (tx, rx) = DockStudio::channel();
    let mut h = harness(DockStudio::new(env, rx));
    tx.send(DockEvent::Connected {
        interfaces: 4,
        keyboard_ok: true,
    })
    .unwrap();
    h.run_steps(4);
    assert_eq!(h.state().readiness(), Readiness::KarabinerStopped);
    h.get_by_label("Start Karabiner-Elements");
}

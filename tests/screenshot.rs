//! Renders the UI off-screen to a PNG for a visual check (run with --ignored).

use eframe::egui::Vec2;
use egui_kittest::Harness;
use quadkey::app::{DockStudio, style};
use quadkey::backend::Environment;
use quadkey::device::DockEvent;
use quadkey::model::{Action, MOD_LCTRL, MOD_LGUI, Media, Trigger};

#[test]
#[ignore]
fn render_main_window() {
    let dir = tempfile::tempdir().unwrap();
    let env = Environment {
        karabiner_installed: true,
        karabiner_running: Some(true),
        karabiner_json: dir.path().join("k.json"),
        config_file: dir.path().join("c.json"),
    };
    let (tx, rx) = DockStudio::channel();
    let mut app = DockStudio::new(env, rx);
    app.config.seen_guide = true;
    app.config.target_os = quadkey::model::TargetOs::Mac;
    app.tab = quadkey::model::Category::Media;
    app.applied = app.config.clone();
    app.applied.buttons[2].action = quadkey::model::Action::Original;
    app.config.buttons[0].action = Action::Media {
        key: Media::PlayPause,
    };
    app.config.buttons[2].action = Action::OpenApp {
        app: "/Applications/Zoom.app".into(),
    };
    app.selected = 2;
    if let Ok(tab) = std::env::var("TAB") {
        app.tab = quadkey::model::Category::ALL
            .into_iter()
            .find(|c| c.label().eq_ignore_ascii_case(&tab))
            .expect("TAB is a category name");
    }
    if std::env::var("TOGGLE").is_ok() {
        // The toggle editor, as after clicking "Toggle between two"
        app.tab = quadkey::model::Category::Custom;
        app.selected = 1;
        let preset = |name: &str| {
            quadkey::model::presets_for(quadkey::model::TargetOs::Mac)
                .into_iter()
                .find(|(p, _)| p.name == name)
                .unwrap()
                .1
        };
        app.config.buttons[1].action = Action::Toggle {
            first: Box::new(preset("Zoom in")),
            second: Box::new(preset("Zoom out")),
        };
    }
    let mut h = Harness::builder()
        .with_size(Vec2::new(1100.0, 860.0))
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut DockStudio| app.draw(ui), app);
    style(&h.ctx);
    if std::env::var("DARK").is_ok() {
        h.ctx.set_theme(eframe::egui::Theme::Dark);
    } else {
        h.ctx.set_theme(eframe::egui::Theme::Light);
    }
    tx.send(DockEvent::Connected {
        interfaces: 4,
        keyboard_ok: true,
    })
    .unwrap();
    tx.send(DockEvent::Pressed(Trigger::Key {
        mods: MOD_LCTRL | MOD_LGUI,
        key: 0x14,
    }))
    .unwrap();
    h.run_steps(3);
    let image = h.render().expect("render");
    image
        .save(std::env::var("SHOT").unwrap_or("/tmp/quadkey.png".into()))
        .unwrap();
}

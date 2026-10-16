use quadkey::app::{DockStudio, style};
use quadkey::backend::Environment;
use quadkey::device;

fn main() -> eframe::Result {
    let (tx, rx) = DockStudio::channel();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("QuadKey")
            // Big enough to show the dock and the actions without resizing, small enough for a
            // 13" laptop screen (1512 x 982 points)
            .with_inner_size([1120.0, 880.0])
            .with_min_inner_size([1040.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "QuadKey",
        options,
        Box::new(move |cc| {
            style(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            device::spawn(tx, move || ctx.request_repaint());
            Ok(Box::new(DockStudio::new(Environment::system(), rx)))
        }),
    )
}

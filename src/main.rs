use quadkey::app::{DockStudio, style};
use quadkey::backend::Environment;
use quadkey::device;

fn main() -> eframe::Result {
    let (tx, rx) = DockStudio::channel();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("QuadKey")
            .with_inner_size([860.0, 560.0])
            .with_min_inner_size([760.0, 500.0]),
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

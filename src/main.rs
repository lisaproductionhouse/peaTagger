mod app;
mod diff;
mod export;
mod state;
mod tagger;
mod translate;
mod ui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Auto HTML Tagger",
        native_options,
        Box::new(|cc| Ok(Box::new(app::AutoHtmlTaggerApp::new(cc)))),
    )
}

mod app;
mod command;
mod document;
mod gpu;
mod import;
mod pick;
mod session;
mod viewport;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("oxiprep")
            .with_icon(egui::IconData::default())
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "Oxiprep",
        options,
        Box::new(|cc| Ok(Box::new(app::OxiprepApp::new(cc)))),
    )
}

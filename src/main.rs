mod app;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("oxiprep")
            .with_icon(egui::IconData::default())
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Oxiprep",
        options,
        Box::new(|_cc| Ok(Box::new(app::OxiprepApp::new()))),
    )
}

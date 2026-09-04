use eframe::egui;
use oxiprep::{ai, app};

fn main() -> eframe::Result {
    if let Some(result) = ai::mcp::maybe_run_proxy_mode() {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(2);
        }
        return Ok(());
    }
    #[cfg(debug_assertions)]
    if let Some(result) = ai::acp::maybe_run_fixture_mode() {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(2);
        }
        return Ok(());
    }
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

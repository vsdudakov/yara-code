//! GPU window frontend. Requires a display server.

use std::path::PathBuf;

use yara::gui::app::App;

fn main() -> eframe::Result<()> {
    let root = yara::core::project_root(std::env::args().nth(1).map(PathBuf::from));

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("Yara")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([700.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Yara",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, root)))),
    )
}

//! GPU window frontend. Requires a display server.

use std::path::PathBuf;

use yara::gui::app::App;

fn main() -> eframe::Result<()> {
    let launched = yara::core::launch(std::env::args().nth(1).map(PathBuf::from));

    // The dock and task-switcher icon: the prompt chevron and the editor's
    // cursor, drawn once and carried in the binary.
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon/icon-256.png"))
        .expect("the bundled icon is a valid png");

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_icon(icon)
            .with_title("Yara Code")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([700.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Yara Code",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, launched.root, launched.file)))),
    )
}

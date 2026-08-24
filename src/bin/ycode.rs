//! Terminal frontend. Runs over SSH on a headless machine.

use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let arg = std::env::args().nth(1);
    if matches!(arg.as_deref(), Some("-h") | Some("--help")) {
        println!("ycode [PATH]\n\nYara, a terminal code editor. Ctrl+H inside shows all bindings.");
        return Ok(());
    }
    let root = yara::core::project_root(arg.map(PathBuf::from));
    yara::tui::run(root)
}

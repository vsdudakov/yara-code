//! The faces the window draws with.
//!
//! egui carries one monospace face and one proportional face of its own, and
//! neither has a glyph for the symbols a program in the terminal panel draws:
//! an agent's spinner and its bullets come out as empty boxes, and the box is
//! not even a cell wide, so every line it sits on jitters as the spinner
//! turns. So the desktop's own font files are read at startup and put in
//! front: a modern monospace for code, the system's own UI face for the
//! chrome, and a chain of symbol faces behind both — each one asked in turn
//! for a glyph the one before it lacks.
//!
//! Nothing here is bundled. Every file named below already belongs to the
//! machine, and a name that is not there is simply skipped, which is what
//! makes the same list safe on a stripped-down system.

use std::path::{Path, PathBuf};

use egui::{FontData, FontDefinitions, FontFamily};

/// A face to look for: where it lives, and which face inside the file when the
/// file is a collection (`.ttc`).
struct Face {
    path: &'static str,
    index: u32,
}

const fn face(path: &'static str) -> Face {
    Face { path, index: 0 }
}

/// Monospace faces for code and the terminal, best first.
#[cfg(target_os = "macos")]
const CODE: &[Face] = &[
    face("/System/Library/Fonts/SFNSMono.ttf"),
    Face {
        path: "/System/Library/Fonts/Menlo.ttc",
        index: 0,
    },
    face("/System/Library/Fonts/Monaco.ttf"),
];

/// Faces for the chrome — menus, labels, the status bar.
#[cfg(target_os = "macos")]
const UI: &[Face] = &[face("/System/Library/Fonts/SFNS.ttf")];

/// Symbols, box drawing, arrows and dingbats: what a code face does not have.
/// Asked in order, so the widest coverage can stand last.
#[cfg(target_os = "macos")]
const SYMBOLS: &[Face] = &[
    Face {
        path: "/System/Library/Fonts/Menlo.ttc",
        index: 0,
    },
    face("/System/Library/Fonts/Apple Symbols.ttf"),
    face("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
    face("/System/Library/Fonts/ZapfDingbats.ttf"),
];

#[cfg(target_os = "windows")]
const CODE: &[Face] = &[
    face("C:/Windows/Fonts/CascadiaMono.ttf"),
    face("C:/Windows/Fonts/consola.ttf"),
];

#[cfg(target_os = "windows")]
const UI: &[Face] = &[face("C:/Windows/Fonts/segoeui.ttf")];

#[cfg(target_os = "windows")]
const SYMBOLS: &[Face] = &[
    face("C:/Windows/Fonts/seguisym.ttf"),
    face("C:/Windows/Fonts/arialuni.ttf"),
];

#[cfg(all(unix, not(target_os = "macos")))]
const CODE: &[Face] = &[
    face("/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf"),
    face("/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf"),
    face("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
    face("/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf"),
    face("/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf"),
];

#[cfg(all(unix, not(target_os = "macos")))]
const UI: &[Face] = &[
    face("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
    face("/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf"),
];

#[cfg(all(unix, not(target_os = "macos")))]
const SYMBOLS: &[Face] = &[
    face("/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf"),
    face("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
    face("/usr/share/fonts/truetype/ancient-scripts/Symbola_hint.ttf"),
];

/// Where a user's own fonts land, searched for a Nerd Font: someone who has
/// installed one has said which glyphs they want their terminal to draw.
fn user_font_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs = Vec::new();
    if let Some(home) = home {
        dirs.push(home.join("Library/Fonts"));
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join(".fonts"));
    }
    dirs.push(PathBuf::from("/Library/Fonts"));
    dirs
}

/// The first Nerd Font — or symbols-only Nerd Font — the user has installed,
/// which covers the private-use icons no system face has.
fn nerd_font() -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in user_font_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy().to_lowercase();
            if !name.ends_with(".ttf") && !name.ends_with(".otf") {
                continue;
            }
            if name.contains("nerd") && !name.contains("italic") && !name.contains("bold") {
                found.push(path);
            }
        }
    }
    // A symbols-only Nerd Font is the one meant to sit behind another face,
    // so it is preferred over a full one when both are installed.
    found.sort();
    found
        .iter()
        .find(|p| p.to_string_lossy().to_lowercase().contains("symbol"))
        .or_else(|| found.first())
        .cloned()
}

/// Reads a font file, or nothing when it is not on this machine.
fn read(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok().filter(|bytes| !bytes.is_empty())
}

/// Puts the machine's own faces in front of the ones egui carries. Called once
/// at startup; a machine with none of them keeps egui's, which is what happens
/// on a bare container and in the tests.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut mono: Vec<String> = Vec::new();
    let mut ui_faces: Vec<String> = Vec::new();
    let mut symbols: Vec<String> = Vec::new();

    let add = |fonts: &mut FontDefinitions, path: &Path, index: u32| -> Option<String> {
        let name = path.file_stem()?.to_string_lossy().into_owned();
        if fonts.font_data.contains_key(&name) {
            return Some(name);
        }
        let data = FontData {
            index,
            ..FontData::from_owned(read(path)?)
        };
        fonts.font_data.insert(name.clone(), data.into());
        Some(name)
    };

    for f in CODE {
        if let Some(name) = add(&mut fonts, Path::new(f.path), f.index) {
            mono.push(name);
            // One code face is enough; the rest of the list is what to try
            // when it is missing.
            break;
        }
    }
    for f in UI {
        if let Some(name) = add(&mut fonts, Path::new(f.path), f.index) {
            ui_faces.push(name);
            break;
        }
    }
    if let Some(path) = nerd_font() {
        if let Some(name) = add(&mut fonts, &path, 0) {
            symbols.push(name);
        }
    }
    for f in SYMBOLS {
        if let Some(name) = add(&mut fonts, Path::new(f.path), f.index) {
            symbols.push(name);
        }
    }

    // In front of egui's own faces, and with the symbol chain behind them, so
    // a glyph the code face lacks is still drawn by somebody.
    for (family, front) in [
        (FontFamily::Monospace, &mono),
        (FontFamily::Proportional, &ui_faces),
    ] {
        let list = fonts.families.entry(family).or_default();
        for name in front.iter().rev() {
            list.insert(0, name.clone());
        }
        list.extend(symbols.iter().cloned());
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the two frontends draw in their chrome, and what an agent running
    /// in the terminal panel draws while it works.
    const DRAWN: &str = "▾▸▫●×›≡◎⎇…≠◫⏺✳✻✽⎿─│└├";

    /// Which of those glyphs a context can actually paint.
    fn covered(install_faces: bool) -> String {
        let ctx = egui::Context::default();
        if install_faces {
            install(&ctx);
        }
        let mut found = String::new();
        let _ = ctx.run(Default::default(), |ctx| {
            let font = egui::FontId::monospace(13.0);
            found = ctx.fonts(|f| DRAWN.chars().filter(|c| f.has_glyph(&font, *c)).collect());
        });
        found
    }

    #[test]
    fn the_machines_own_faces_only_ever_add_glyphs() {
        let before = covered(false);
        let after = covered(true);
        for glyph in before.chars() {
            assert!(
                after.contains(glyph),
                "{glyph} was drawn before and is not now"
            );
        }
        assert!(
            after.chars().count() >= before.chars().count(),
            "installing faces took coverage away"
        );
    }

    #[test]
    fn a_face_that_is_not_on_this_machine_is_simply_skipped() {
        assert!(read(Path::new("/no/such/font.ttf")).is_none());
        // Whatever this machine has, the code family still draws.
        let ctx = egui::Context::default();
        install(&ctx);
        let _ = ctx.run(Default::default(), |ctx| {
            let width = ctx.fonts(|f| f.glyph_width(&egui::FontId::monospace(13.0), 'm'));
            assert!(width > 0.0);
        });
    }
}

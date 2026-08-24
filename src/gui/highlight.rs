//! Turns core syntax regions into egui layout jobs, cached across frames.

use std::sync::{Mutex, OnceLock};

use egui::text::{LayoutJob, TextFormat};
use egui::FontId;

use crate::core::syntax::Syntax;
use crate::core::theme::Theme;
use crate::gui::theme::{color, CODE_FONT_SIZE};

/// The grammar set is expensive to load, so it lives once per process; only the
/// color scheme inside it is swapped when the user changes themes.
fn syntax() -> &'static Mutex<Syntax> {
    static SYNTAX: OnceLock<Mutex<Syntax>> = OnceLock::new();
    SYNTAX.get_or_init(|| Mutex::new(Syntax::default()))
}

pub fn set_theme(theme: &Theme) {
    syntax().lock().unwrap().set_theme(theme);
}

#[derive(Default)]
struct Highlighter;

/// Keyed by theme name as well as content, so switching themes doesn't serve
/// stale colors out of the cache.
impl egui::util::cache::ComputerMut<(&str, &str, &str), LayoutJob> for Highlighter {
    fn compute(&mut self, (_theme, extension, code): (&str, &str, &str)) -> LayoutJob {
        let mut job = LayoutJob::default();
        let font_id = FontId::monospace(CODE_FONT_SIZE);
        syntax()
            .lock()
            .unwrap()
            .highlight_lines(extension, code, |regions| {
                for region in regions {
                    job.append(
                        region.text,
                        0.0,
                        TextFormat {
                            font_id: font_id.clone(),
                            color: color(region.color),
                            italics: region.italic,
                            ..Default::default()
                        },
                    );
                }
            });
        job
    }
}

type HighlightCache = egui::util::cache::FrameCache<LayoutJob, Highlighter>;

pub fn highlight(ctx: &egui::Context, theme: &str, extension: &str, code: &str) -> LayoutJob {
    ctx.memory_mut(|mem| {
        mem.caches
            .cache::<HighlightCache>()
            .get((theme, extension, code))
    })
}

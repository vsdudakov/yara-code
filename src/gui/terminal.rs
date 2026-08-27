//! Integrated terminal: PTY-backed shells rendered through vt100 screens,
//! several sessions behind one tab strip.

use std::path::Path;

use crate::core::command::{Key, Mods};
use crate::core::pty::{Pty, Terminals};
use crate::core::theme::Theme;
use crate::gui::file_tree::menu_item;
use crate::gui::tabs;
use crate::gui::theme::{ansi_color, code_font, color};

#[derive(Default)]
pub struct Terminal {
    pub sessions: Terminals,
    /// Session being renamed from the tab strip, and the name so far.
    renaming: Option<(usize, String, bool)>,
    /// Whether the grid held keyboard focus last frame; the panel header uses
    /// it to show which pane the keyboard is talking to.
    pub focused: bool,
    /// Grab keyboard focus on the next frame, set when a session opens or the
    /// active tab changes.
    focus_pending: bool,
    /// Wheel movement not yet worth a row, in points. A trackpad sends a few
    /// points per frame, and rounding each frame on its own throws all of them
    /// away — kept here, they add up to the row they are.
    wheel_carry: f32,
}

impl Terminal {
    fn notifier(ctx: egui::Context) -> impl Fn() + Send + 'static {
        move || ctx.request_repaint()
    }

    /// Starts another shell and switches to it.
    pub fn open(&mut self, cwd: &Path, ctx: &egui::Context) {
        self.sessions.open(cwd, Self::notifier(ctx.clone()));
        self.focus_pending = true;
    }

    /// The tab strip in the panel header: one tab per shell — its number or
    /// the name it was given — and `+` to open another. It is the strip the
    /// editor's own tabs go through, so a shell tab is the size of a file tab
    /// and is clicked, closed and dragged the same way.
    pub fn tab_bar(&mut self, ui: &mut egui::Ui, theme: &Theme, cwd: &Path, ctx: &egui::Context) {
        let strip: Vec<tabs::Tab> = (0..self.sessions.len())
            .map(|i| tabs::Tab::new(self.sessions.id(i), self.sessions.name(i)))
            .collect();
        let mut close: Option<usize> = None;
        let mut rename: Option<usize> = None;
        let mut reset: Option<usize> = None;
        let named = |i: usize| self.sessions.is_named(i);
        let row = tabs::row(
            ui,
            theme,
            &strip,
            Some(self.sessions.active_index()),
            true,
            false,
            |ui, i| {
                if menu_item(ui, theme, "Rename Terminal").clicked() {
                    rename = Some(i);
                    ui.close_menu();
                }
                if named(i) && menu_item(ui, theme, "Reset Name").clicked() {
                    reset = Some(i);
                    ui.close_menu();
                }
                if menu_item(ui, theme, "Close Terminal").clicked() {
                    close = Some(i);
                    ui.close_menu();
                }
            },
        );
        let mut activate = None;
        for action in row.actions {
            match action {
                tabs::Action::Show(i) => activate = Some(i),
                tabs::Action::Close(i) => close = Some(i),
                tabs::Action::Move { from, to } => self.sessions.reorder(from, to),
            }
        }
        // The name box takes the place of the tab being renamed, drawn over it
        // rather than in the row, so the strip keeps its own layout.
        self.rename_field(ui, theme, &row.rects);
        ui.add_space(4.0);
        if plus_button(ui, theme, 16.0)
            .on_hover_text("New Terminal")
            .clicked()
        {
            self.open(cwd, ctx);
        }
        if let Some(i) = rename {
            self.renaming = Some((i, self.sessions.name(i), true));
        }
        if let Some(i) = reset {
            self.sessions.rename(i, "");
        }
        if let Some(i) = activate {
            self.sessions.set_active(i);
            self.focus_pending = true;
        }
        if let Some(i) = close {
            self.sessions.close(i);
            self.renaming = None;
        }
    }

    /// The inline name box, over the tab being renamed. Enter keeps the name,
    /// Escape or a click elsewhere drops it.
    fn rename_field(&mut self, ui: &mut egui::Ui, theme: &Theme, rects: &[egui::Rect]) {
        let Some((index, _, _)) = &self.renaming else {
            return;
        };
        let Some(rect) = rects.get(*index).copied() else {
            return;
        };
        let mut done = false;
        let mut cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if let Some((_, name, focus)) = &mut self.renaming {
            // Floated over the tab rather than put in the row, so the strip
            // keeps its layout while a name is being typed.
            let resp = egui::Area::new(ui.id().with("rename"))
                .order(egui::Order::Foreground)
                .fixed_pos(rect.min)
                .show(ui.ctx(), |ui| {
                    ui.add_sized(
                        rect.size(),
                        egui::TextEdit::singleline(name)
                            .font(egui::TextStyle::Small)
                            .text_color(color(theme.ui.fg_bright)),
                    )
                })
                .inner;
            if *focus {
                resp.request_focus();
                *focus = false;
            }
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                } else {
                    cancel = true;
                }
            }
        }
        if done {
            let (i, name, _) = self.renaming.take().unwrap();
            self.sessions.rename(i, &name);
        } else if cancel {
            self.renaming = None;
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &Theme, cwd: &Path, ctx: &egui::Context) {
        let Some(pty) = self.sessions.ensure(cwd, Self::notifier(ctx.clone())) else {
            let message = self
                .sessions
                .error
                .clone()
                .unwrap_or_else(|| "terminal unavailable".to_string());
            ui.label(
                egui::RichText::new(format!("terminal failed: {message}"))
                    .color(color(theme.ui.fg_dim)),
            );
            self.focused = false;
            return;
        };

        let font_id = code_font(ui);
        let (cell_w, cell_h) = ui.fonts(|f| (f.glyph_width(&font_id, ' '), f.row_height(&font_id)));
        // Laying out a run, epaint rounds each glyph's advance to a physical
        // pixel; a grid built on the raw advance drifts away from the glyphs
        // by a fraction per cell, and the cursor ends up further right the
        // longer the line. Snap the cell to the same pixel so they agree.
        let ppp = ui.ctx().pixels_per_point();
        let cell_w = (cell_w * ppp).round() / ppp;

        let avail = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
        let id = response.id;

        // Match the PTY grid to the visible area.
        let cols = ((rect.width() / cell_w).floor() as u16).clamp(4, 500);
        let rows = ((rect.height() / cell_h).floor() as u16).clamp(2, 200);
        pty.resize(rows, cols);

        if response.clicked() || self.focus_pending {
            response.request_focus();
        }
        self.focus_pending = false;
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            // A program that asked for the mouse — an agent, a pager — scrolls
            // a transcript of its own, so the wheel goes to it a notch at a
            // time. Anything else leaves the wheel to the panel, which walks
            // the scrollback; typing brings the live screen back, as in any
            // terminal.
            let to_program = pty.wants_mouse();
            let step = if to_program { cell_h * 3.0 } else { cell_h };
            self.wheel_carry += ui.input(|i| i.smooth_scroll_delta.y);
            let steps = (self.wheel_carry / step).trunc();
            self.wheel_carry -= steps * step;
            let steps = steps as isize;
            if steps != 0 {
                if to_program {
                    let at = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.min);
                    let (row, col) = cell_at(at, rect, cell_w, cell_h, rows, cols);
                    let notch = pty.wheel_bytes(steps > 0, row, col);
                    let bytes: Vec<u8> = notch.repeat(steps.unsigned_abs());
                    pty.write(&bytes);
                } else {
                    let current = pty.scrollback() as isize;
                    pty.set_scrollback((current + steps).max(0) as usize);
                }
            }
        }
        // The mouse selects text on the grid, the way it does in the terminal
        // frontend: a press starts a selection, a drag extends it, and a copy
        // takes what it covers. The pointer stays the panel's own even when a
        // program is reading the wheel — a full-screen program has no way to
        // hand text back, and selecting its output is the point.
        if response.drag_started() || response.clicked() {
            // Where the button went down, not where the pointer had reached by
            // the time egui called it a drag: a selection starts under the
            // press, as it does everywhere else.
            let down = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos());
            if let Some(pos) = down {
                let (row, col) = cell_at(pos, rect, cell_w, cell_h, rows, cols);
                pty.begin_selection(row, col);
            }
        }
        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let (row, col) = cell_at(pos, rect, cell_w, cell_h, rows, cols);
                pty.extend_selection(row, col);
            }
        }

        let focused = response.has_focus();
        self.focused = focused;
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        escape: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                    },
                )
            });
            handle_input(ui, pty);
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, egui::CornerRadius::ZERO, color(theme.ui.editor_bg));

        let origin = rect.min;
        // The selection is held against the shell's own text, so it stays on
        // that text while the panel scrolls; the highlight is worked out per
        // row, as the terminal frontend does it.
        let selection = pty.selection();
        let scrollback = pty.scrollback() as isize;
        let selected_bg = color(theme.ui.selection);
        pty.with_screen(|screen| {
            // A terminal is a grid, so every cell is painted at the column it
            // belongs to. Laying a whole row out as one string was cheaper,
            // but a glyph the code face lacks is not a cell wide, and the
            // whole line after it slid sideways every time an agent's spinner
            // turned.
            for row in 0..rows {
                let highlighted = selection
                    .and_then(|s| s.span_on(row as isize - scrollback, cols))
                    .unwrap_or((0, 0));
                let y = origin.y + row as f32 * cell_h;
                let mut cells: Vec<(String, egui::text::TextFormat, bool)> =
                    Vec::with_capacity(cols as usize);
                for col in 0..cols {
                    let (text, mut fmt) = match screen.cell(row, col) {
                        Some(cell) => {
                            let contents = cell.contents();
                            let text = if contents.is_empty() {
                                " ".to_string()
                            } else {
                                contents
                            };
                            (text, cell_format(cell, &font_id, theme))
                        }
                        None => (" ".to_string(), default_format(&font_id, theme)),
                    };
                    if (highlighted.0..highlighted.1).contains(&col) {
                        fmt.background = selected_bg;
                    }
                    // The background belongs to the cell, not to the glyph:
                    // painted as a rectangle it lines up whatever is in it.
                    if fmt.background != egui::Color32::TRANSPARENT {
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(origin.x + col as f32 * cell_w, y),
                                egui::vec2(cell_w, cell_h),
                            ),
                            egui::CornerRadius::ZERO,
                            fmt.background,
                        );
                        fmt.background = egui::Color32::TRANSPARENT;
                    }
                    let fits = fits_a_cell(ui, &font_id, &text, cell_w);
                    cells.push((text, fmt, fits));
                }

                // Cells that are exactly a cell wide and share a style are
                // still drawn together — that is most of a screen, and one
                // galley a run is what keeps this affordable.
                let mut run = String::new();
                let mut run_fmt: Option<egui::text::TextFormat> = None;
                let mut run_start = 0usize;
                let flush = |run: &mut String,
                             run_fmt: &mut Option<egui::text::TextFormat>,
                             start: usize| {
                    if let Some(fmt) = run_fmt.take() {
                        paint_run(
                            ui,
                            &painter,
                            origin.x + start as f32 * cell_w,
                            y,
                            std::mem::take(run),
                            fmt,
                            color(theme.ui.terminal_fg),
                        );
                    }
                };
                for (col, (text, fmt, fits)) in cells.into_iter().enumerate() {
                    if !fits {
                        flush(&mut run, &mut run_fmt, run_start);
                        // Off-width glyphs stand in the middle of their own
                        // cell: a narrow one is not left hanging on the left,
                        // and a double-width one simply runs into the blank
                        // cell the terminal left for it.
                        let width = text_width(ui, &fmt.font_id, &text);
                        let slack = ((cell_w - width) / 2.0).max(0.0);
                        paint_run(
                            ui,
                            &painter,
                            origin.x + col as f32 * cell_w + slack,
                            y,
                            text,
                            fmt,
                            color(theme.ui.terminal_fg),
                        );
                        run_start = col + 1;
                        continue;
                    }
                    match &run_fmt {
                        Some(f) if *f == fmt => run.push_str(&text),
                        _ => {
                            flush(&mut run, &mut run_fmt, run_start);
                            run_start = col;
                            run = text;
                            run_fmt = Some(fmt);
                        }
                    }
                }
                flush(&mut run, &mut run_fmt, run_start);
            }

            if !screen.hide_cursor() {
                let (crow, ccol) = screen.cursor_position();
                let cursor_rect = egui::Rect::from_min_size(
                    origin + egui::vec2(ccol as f32 * cell_w, crow as f32 * cell_h),
                    egui::vec2(cell_w, cell_h),
                );
                if focused {
                    painter.rect_filled(
                        cursor_rect,
                        egui::CornerRadius::ZERO,
                        color(theme.ui.cursor).gamma_multiply(0.55),
                    );
                } else {
                    painter.rect_stroke(
                        cursor_rect,
                        egui::CornerRadius::ZERO,
                        egui::Stroke::new(1.0_f32, color(theme.ui.cursor)),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        });
    }
}

/// The `+` at the end of the tab strip, drawn as two strokes rather than a
/// glyph so it shows up regardless of font coverage — the same shapes the
/// tabs' own marks are made of.
fn plus_button(ui: &mut egui::Ui, theme: &Theme, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let tint = if hovered {
            color(theme.ui.fg)
        } else {
            color(theme.ui.fg_dim)
        };
        if hovered {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(3), color(theme.ui.hover_bg));
        }
        let r = size * 0.28;
        let c = rect.center();
        let stroke = egui::Stroke::new(1.3_f32, tint);
        ui.painter()
            .line_segment([c + egui::vec2(-r, 0.0), c + egui::vec2(r, 0.0)], stroke);
        ui.painter()
            .line_segment([c + egui::vec2(0.0, -r), c + egui::vec2(0.0, r)], stroke);
    }
    resp
}

/// The cell of the grid under a point, clamped to the grid — where a selection
/// starts, and where the program that reads the mouse is told the wheel turned.
fn cell_at(
    pos: egui::Pos2,
    rect: egui::Rect,
    cell_w: f32,
    cell_h: f32,
    rows: u16,
    cols: u16,
) -> (u16, u16) {
    let row = ((pos.y - rect.min.y) / cell_h).max(0.0) as u16;
    let col = ((pos.x - rect.min.x) / cell_w).max(0.0) as u16;
    (
        row.min(rows.saturating_sub(1)),
        col.min(cols.saturating_sub(1)),
    )
}

fn handle_input(ui: &mut egui::Ui, pty: &mut Pty) {
    let mut bytes: Vec<u8> = Vec::new();
    let mut presses: Vec<(Key, Mods)> = Vec::new();
    let mut pastes: Vec<String> = Vec::new();
    // A copy while the grid has the keyboard takes what the mouse dragged
    // over. Taking it clears the highlight, so the next Ctrl+C is the shell's
    // interrupt again — the same trade the terminal frontend makes.
    //
    // Off macOS the copy key *is* Ctrl+C: egui turns it into this event and
    // never reports the key, so with nothing selected the event has to be
    // the interrupt itself, or no program in the panel could ever be
    // stopped. Cut is Ctrl+X by the same route. On a Mac the copy key is ⌘C
    // and Ctrl+C reaches the shell as a key of its own.
    let copies = cfg!(target_os = "macos");
    if ui.input(|i| i.events.contains(&egui::Event::Copy)) {
        if let Some(text) = pty.selected_text() {
            ui.ctx().copy_text(text);
            pty.clear_selection();
        } else if !copies {
            bytes.push(0x03);
        }
    }
    if !copies && ui.input(|i| i.events.contains(&egui::Event::Cut)) {
        bytes.push(0x18);
    }
    ui.input(|i| {
        for event in &i.events {
            match event {
                egui::Event::Text(t) => bytes.extend_from_slice(t.as_bytes()),
                // A paste goes the way the core sends one — bracketed when
                // the program asked, line ends as Return — not as raw text.
                egui::Event::Paste(t) => pastes.push(t.clone()),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    // ⌘ is the app's; Ctrl is the shell's. On Linux and
                    // Windows egui folds Ctrl into `command`, so only a real
                    // ⌘ press may be skipped here.
                    if modifiers.mac_cmd {
                        continue; // global app shortcuts
                    }
                    let mods = Mods {
                        cmd: false,
                        ctrl: modifiers.ctrl,
                        alt: modifiers.alt,
                        shift: modifiers.shift,
                    };
                    if let Some(key) = shell_key(*key, mods) {
                        presses.push((key, mods));
                    }
                }
                _ => {}
            }
        }
    });
    if !bytes.is_empty() {
        // Typing returns the view to the live screen and drops the selection,
        // as it does in any terminal.
        pty.set_scrollback(0);
        pty.clear_selection();
        pty.write(&bytes);
    }
    for text in pastes {
        pty.paste(&text);
    }
    // What each press spells is [`crate::core::keyboard`]'s to say: with the
    // protocol on, Ctrl+Shift+V is a key of its own rather than Ctrl+V again.
    for (key, mods) in presses {
        pty.send_key(&key, mods);
    }
}

/// The key as [`crate::core::keyboard`] names it, or nothing where the press
/// is not the shell's to hear. A character typed with no Ctrl or Alt on it
/// arrives as text of its own and is sent as that: taking the key press too
/// would type everything twice.
fn shell_key(key: egui::Key, mods: Mods) -> Option<Key> {
    use egui::Key as K;
    let named = |name: &str| Some(Key::Named(name.to_string()));
    match key {
        K::Enter => named("enter"),
        K::Tab if mods.shift => named("backtab"),
        K::Tab => named("tab"),
        K::Backspace => named("backspace"),
        K::Escape => named("esc"),
        K::ArrowUp => named("up"),
        K::ArrowDown => named("down"),
        K::ArrowRight => named("right"),
        K::ArrowLeft => named("left"),
        K::Home => named("home"),
        K::End => named("end"),
        K::PageUp => named("pageup"),
        K::PageDown => named("pagedown"),
        K::Delete => named("delete"),
        K::Insert => named("insert"),
        K::Space if mods.ctrl || mods.alt => named("space"),
        other => {
            let name = other.name();
            if name
                .strip_prefix('F')
                .is_some_and(|n| n.parse::<u8>().is_ok())
            {
                return named(&name.to_ascii_lowercase());
            }
            // The keys that stand for one character: the letters, the digits,
            // and the punctuation egui names by its symbol.
            let mut chars = name.chars();
            match (chars.next(), chars.next(), mods.ctrl || mods.alt) {
                (Some(c), None, true) => Some(Key::Char(c.to_ascii_lowercase())),
                _ => None,
            }
        }
    }
}

/// Paints one run of cells at a point, in one style.
fn paint_run(
    ui: &egui::Ui,
    painter: &egui::Painter,
    x: f32,
    y: f32,
    text: String,
    fmt: egui::text::TextFormat,
    fallback: egui::Color32,
) {
    if text.is_empty() {
        return;
    }
    let mut job = egui::text::LayoutJob::default();
    job.append(&text, 0.0, fmt);
    let galley = ui.fonts(|f| f.layout_job(job));
    painter.galley(egui::pos2(x, y), galley, fallback);
}

/// What a cell's contents measure, whichever face ends up drawing them.
fn text_width(ui: &egui::Ui, font_id: &egui::FontId, text: &str) -> f32 {
    ui.fonts(|f| text.chars().map(|c| f.glyph_width(font_id, c)).sum())
}

/// Whether a cell's contents take exactly the width of a cell, and so can be
/// drawn shoulder to shoulder with its neighbours. Plain ASCII always does,
/// and asking is the expensive part, so it is not asked.
fn fits_a_cell(ui: &egui::Ui, font_id: &egui::FontId, text: &str, cell_w: f32) -> bool {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_graphic() || c == ' ' => true,
        (Some(_), _) => (text_width(ui, font_id, text) - cell_w).abs() < 0.5,
        (None, _) => true,
    }
}

fn vt_color(c: vt100::Color, default: egui::Color32, theme: &Theme) -> egui::Color32 {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => ansi_color(theme, i),
        vt100::Color::Rgb(r, g, b) => egui::Color32::from_rgb(r, g, b),
    }
}

fn default_format(font_id: &egui::FontId, theme: &Theme) -> egui::text::TextFormat {
    egui::text::TextFormat {
        font_id: font_id.clone(),
        color: color(theme.ui.terminal_fg),
        ..Default::default()
    }
}

fn cell_format(
    cell: &vt100::Cell,
    font_id: &egui::FontId,
    theme: &Theme,
) -> egui::text::TextFormat {
    let mut fg = vt_color(cell.fgcolor(), color(theme.ui.terminal_fg), theme);
    let mut bg = vt_color(cell.bgcolor(), egui::Color32::TRANSPARENT, theme);
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
        if bg == egui::Color32::TRANSPARENT {
            bg = color(theme.ui.terminal_fg);
        }
        if fg == egui::Color32::TRANSPARENT {
            fg = color(theme.ui.editor_bg);
        }
    }
    egui::text::TextFormat {
        font_id: font_id.clone(),
        color: fg,
        background: bg,
        italics: cell.italic(),
        underline: if cell.underline() {
            egui::Stroke::new(1.0_f32, fg)
        } else {
            egui::Stroke::NONE
        },
        ..Default::default()
    }
}

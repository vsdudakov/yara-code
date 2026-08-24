//! Integrated terminal: PTY-backed shells rendered through vt100 screens,
//! several sessions behind one tab strip.

use std::path::Path;

use crate::core::pty::{Pty, Terminals};
use crate::core::theme::Theme;
use crate::gui::theme::{ansi_color, color, CODE_FONT_SIZE};

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
}

/// The tab being dragged along the strip.
#[derive(Clone, Copy)]
struct TabDrag(usize);

/// The small painted marks on the tab strip.
enum Mark {
    Cross,
    Plus,
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
    /// the name it was given — a cross to close it, and `+` to open another.
    /// Tabs can be dragged to reorder and renamed from their context menu.
    pub fn tab_bar(&mut self, ui: &mut egui::Ui, theme: &Theme, cwd: &Path, ctx: &egui::Context) {
        let mut close: Option<usize> = None;
        let mut activate: Option<usize> = None;
        let mut moved: Option<(usize, usize)> = None;
        let mut rename: Option<usize> = None;
        ui.spacing_mut().item_spacing.x = 4.0;
        for i in 0..self.sessions.len() {
            if self.rename_field(ui, theme, i) {
                continue;
            }
            let selected = i == self.sessions.active_index();
            let mut text =
                egui::RichText::new(self.sessions.name(i))
                    .size(11.0)
                    .color(if selected {
                        color(theme.ui.fg)
                    } else {
                        color(theme.ui.fg_faint)
                    });
            if selected {
                text = text.strong();
            }
            let resp = ui
                .add(egui::Label::new(text).sense(egui::Sense::click_and_drag()))
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            // Drag a tab onto another to reorder the strip.
            resp.dnd_set_drag_payload(TabDrag(i));
            if let Some(src) = resp.dnd_release_payload::<TabDrag>() {
                moved = Some((src.0, i));
            }
            if resp.dnd_hover_payload::<TabDrag>().is_some() {
                let rect = resp.rect.expand2(egui::vec2(2.0, 1.0));
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(2),
                    egui::Stroke::new(1.0_f32, color(theme.ui.accent_light)),
                    egui::StrokeKind::Inside,
                );
            }
            if resp.clicked() {
                activate = Some(i);
            }
            resp.context_menu(|ui| {
                if ui.button("Rename Terminal").clicked() {
                    rename = Some(i);
                    ui.close_menu();
                }
                if self.sessions.is_named(i) && ui.button("Reset Name").clicked() {
                    self.sessions.rename(i, "");
                    ui.close_menu();
                }
                if ui.button("Close Terminal").clicked() {
                    close = Some(i);
                    ui.close_menu();
                }
            });
            if mark_button(ui, theme, Mark::Cross, 13.0)
                .on_hover_text("Close Terminal")
                .clicked()
            {
                close = Some(i);
            }
            ui.add_space(4.0);
        }
        if mark_button(ui, theme, Mark::Plus, 16.0)
            .on_hover_text("New Terminal")
            .clicked()
        {
            self.open(cwd, ctx);
        }
        if let Some(i) = rename {
            self.renaming = Some((i, self.sessions.name(i), true));
        }
        if let Some((from, to)) = moved {
            self.sessions.reorder(from, to);
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

    /// The inline name box shown in place of the tab being renamed. Returns
    /// true when it took this tab's place.
    fn rename_field(&mut self, ui: &mut egui::Ui, theme: &Theme, index: usize) -> bool {
        let Some((editing, _, _)) = &self.renaming else {
            return false;
        };
        if *editing != index {
            return false;
        }
        let mut done = false;
        let mut cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if let Some((_, name, focus)) = &mut self.renaming {
            let resp = ui.add(
                egui::TextEdit::singleline(name)
                    .desired_width(90.0)
                    .font(egui::TextStyle::Small)
                    .text_color(color(theme.ui.fg_bright)),
            );
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
        true
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

        let font_id = egui::FontId::monospace(CODE_FONT_SIZE);
        let (cell_w, cell_h) = ui.fonts(|f| (f.glyph_width(&font_id, ' '), f.row_height(&font_id)));

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
        pty.with_screen(|screen| {
            for row in 0..rows {
                let mut job = egui::text::LayoutJob::default();
                let mut run = String::new();
                let mut run_fmt: Option<egui::text::TextFormat> = None;
                for col in 0..cols {
                    let (mut text, fmt) = match screen.cell(row, col) {
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
                    match &run_fmt {
                        Some(f) if *f == fmt => run.push_str(&text),
                        _ => {
                            if let Some(f) = run_fmt.take() {
                                job.append(&run, 0.0, f);
                            }
                            run = std::mem::take(&mut text);
                            run_fmt = Some(fmt);
                        }
                    }
                }
                if let Some(f) = run_fmt {
                    job.append(&run, 0.0, f);
                }
                let galley = ui.fonts(|f| f.layout_job(job));
                painter.galley(
                    origin + egui::vec2(0.0, row as f32 * cell_h),
                    galley,
                    color(theme.ui.terminal_fg),
                );
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

/// A small ×/+ button drawn as shapes rather than glyphs, so it shows up
/// regardless of font coverage — the same trick the editor's tabs use.
fn mark_button(ui: &mut egui::Ui, theme: &Theme, mark: Mark, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let stroke_color = if hovered {
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
        let stroke = egui::Stroke::new(1.3_f32, stroke_color);
        match mark {
            Mark::Cross => {
                ui.painter()
                    .line_segment([c + egui::vec2(-r, -r), c + egui::vec2(r, r)], stroke);
                ui.painter()
                    .line_segment([c + egui::vec2(r, -r), c + egui::vec2(-r, r)], stroke);
            }
            Mark::Plus => {
                ui.painter()
                    .line_segment([c + egui::vec2(-r, 0.0), c + egui::vec2(r, 0.0)], stroke);
                ui.painter()
                    .line_segment([c + egui::vec2(0.0, -r), c + egui::vec2(0.0, r)], stroke);
            }
        }
    }
    resp
}

fn handle_input(ui: &mut egui::Ui, pty: &mut Pty) {
    let mut bytes: Vec<u8> = Vec::new();
    ui.input(|i| {
        for event in &i.events {
            match event {
                egui::Event::Text(t) => bytes.extend_from_slice(t.as_bytes()),
                egui::Event::Paste(t) => bytes.extend_from_slice(t.as_bytes()),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if modifiers.command {
                        continue; // global app shortcuts
                    }
                    if modifiers.ctrl {
                        let name = key.name();
                        if name.len() == 1 {
                            let ch = name.as_bytes()[0].to_ascii_lowercase();
                            if ch.is_ascii_lowercase() {
                                bytes.push(ch - b'a' + 1);
                                continue;
                            }
                        }
                    }
                    match key {
                        egui::Key::Enter => bytes.push(b'\r'),
                        egui::Key::Backspace => bytes.push(0x7f),
                        egui::Key::Tab => bytes.push(b'\t'),
                        egui::Key::Escape => bytes.push(0x1b),
                        egui::Key::ArrowUp => bytes.extend_from_slice(b"\x1b[A"),
                        egui::Key::ArrowDown => bytes.extend_from_slice(b"\x1b[B"),
                        egui::Key::ArrowRight => bytes.extend_from_slice(b"\x1b[C"),
                        egui::Key::ArrowLeft => bytes.extend_from_slice(b"\x1b[D"),
                        egui::Key::Home => bytes.extend_from_slice(b"\x1b[H"),
                        egui::Key::End => bytes.extend_from_slice(b"\x1b[F"),
                        egui::Key::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
                        egui::Key::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
                        egui::Key::Delete => bytes.extend_from_slice(b"\x1b[3~"),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    });
    pty.write(&bytes);
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

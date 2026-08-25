//! Side-by-side diff of one changed file: what it was on the left, what it is
//! now on the right, changed lines tinted in the theme's own red and green.

use crate::core::diff::{next_change, previous_change, Kind, Row};
use crate::core::theme::Theme;
use crate::gui::theme::{ansi_color, code_font, color, icons};

pub struct DiffView {
    /// Path as git reports it, relative to the worktree.
    pub path: String,
    pub rows: Vec<Row>,
    pub error: Option<String>,
    /// Row the arrows asked to put at the top, until the next frame draws it.
    jump_to: Option<usize>,
    /// Row at the top of the view as it was last drawn, which is where the
    /// arrows count the next change from.
    top_row: usize,
}

impl DiffView {
    pub fn new(path: String, rows: Result<Vec<Row>, String>) -> Self {
        match rows {
            Ok(rows) => Self {
                path,
                rows,
                error: None,
                jump_to: None,
                top_row: 0,
            },
            Err(message) => Self {
                path,
                rows: Vec::new(),
                error: Some(message),
                jump_to: None,
                top_row: 0,
            },
        }
    }

    /// Draws the view. Returns true when the user asked to close it.
    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &Theme) -> DiffEvent {
        let mut event = DiffEvent::None;
        let last_row = self.rows.len().saturating_sub(1);
        egui::Frame::default()
            .fill(color(theme.ui.status_bg))
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&self.path)
                            .color(color(theme.ui.fg))
                            .size(12.0),
                    );
                    ui.label(
                        egui::RichText::new(summary(&self.rows))
                            .color(color(theme.ui.fg_faint))
                            .size(11.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(egui::RichText::new(icons().close).size(11.0))
                            .on_hover_text("Close Diff")
                            .clicked()
                        {
                            event = DiffEvent::Close;
                        }
                        if ui
                            .button(egui::RichText::new("Open File").size(11.0))
                            .clicked()
                        {
                            event = DiffEvent::Open;
                        }
                        // Reviewing a diff is going change to change, which is
                        // what the terminal frontend's arrow keys do here; the
                        // window says so with a pair of arrows, since it has
                        // nowhere else to say it. Right to left, so the down
                        // arrow is added first to end up on the right.
                        if arrow_button(ui, theme, Arrow::Down)
                            .on_hover_text("Next Change")
                            .clicked()
                        {
                            self.jump_to =
                                Some(next_change(&self.rows, self.top_row).unwrap_or(last_row));
                        }
                        if arrow_button(ui, theme, Arrow::Up)
                            .on_hover_text("Previous Change")
                            .clicked()
                        {
                            self.jump_to =
                                Some(previous_change(&self.rows, self.top_row).unwrap_or(0));
                        }
                    });
                });
            });

        if let Some(message) = &self.error {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(message)
                    .color(color(theme.ui.danger))
                    .size(12.0),
            );
            return event;
        }
        if self.rows.is_empty() {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("no changes in this file")
                    .color(color(theme.ui.fg_faint))
                    .size(12.0),
            );
            return event;
        }

        let font = code_font(ui);
        let (char_w, row_h) = ui.fonts(|f| (f.glyph_width(&font, ' '), f.row_height(&font)));
        let gutter = char_w * 6.0;
        // The theme's own red and green, dimmed to a background wash and used
        // at full strength for the line numbers.
        let removed = ansi_color(theme, 1);
        let added = ansi_color(theme, 2);

        // The arrow keys walk the changes too, the way they do in the terminal
        // frontend, unless something on the page — the terminal, a name being
        // typed — is taking the keyboard.
        if ui.memory(|m| m.focused().is_none()) {
            let (down, up) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::ArrowUp),
                )
            });
            if down {
                self.jump_to = Some(next_change(&self.rows, self.top_row).unwrap_or(last_row));
            } else if up {
                self.jump_to = Some(previous_change(&self.rows, self.top_row).unwrap_or(0));
            }
        }

        // What one row costs down the page: its own height and the gap the
        // layout leaves under it, which is what the scroll area counts in.
        let pitch = row_h + ui.spacing().item_spacing.y;
        let mut area = egui::ScrollArea::both().auto_shrink([false, false]);
        // An arrow moves the view by setting where it starts; every other
        // frame leaves the offset to the scroll area itself.
        if let Some(row) = self.jump_to.take() {
            area = area.vertical_scroll_offset(row as f32 * pitch);
        }
        let output = area.show_rows(ui, row_h, self.rows.len(), |ui, range| {
            let width = ui.available_width();
            let half = (width / 2.0).max(80.0);
            for row in &self.rows[range] {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, row_h), egui::Sense::hover());
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let painter = ui.painter_at(rect);
                let side = |x: f32,
                            w: f32,
                            line: Option<&crate::core::diff::Side>,
                            tint: Option<egui::Color32>| {
                    let area =
                        egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(w, row_h));
                    // Clipped to its own half: a long line is cut at the
                    // seam instead of running over the other version.
                    let painter = painter.with_clip_rect(area);
                    let Some(line) = line else {
                        // The blank half beside an added or removed line.
                        painter.rect_filled(
                            area,
                            egui::CornerRadius::ZERO,
                            color(theme.ui.sidebar_bg),
                        );
                        return;
                    };
                    if let Some(tint) = tint {
                        painter.rect_filled(
                            area,
                            egui::CornerRadius::ZERO,
                            tint.gamma_multiply(0.22),
                        );
                    }
                    painter.text(
                        egui::pos2(x + gutter - char_w, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        line.line.to_string(),
                        font.clone(),
                        match tint {
                            Some(tint) => tint,
                            None => color(theme.ui.line_number),
                        },
                    );
                    painter.text(
                        egui::pos2(x + gutter, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &line.text,
                        font.clone(),
                        color(theme.ui.fg),
                    );
                };
                let (left_tint, right_tint) = match row.kind {
                    Kind::Same => (None, None),
                    Kind::Changed => (Some(removed), Some(added)),
                    Kind::Added => (None, Some(added)),
                    Kind::Removed => (Some(removed), None),
                };
                side(rect.min.x, half, row.left.as_ref(), left_tint);
                side(
                    rect.min.x + half,
                    width - half,
                    row.right.as_ref(),
                    right_tint,
                );
                // The seam between the two versions.
                painter.line_segment(
                    [
                        egui::pos2(rect.min.x + half, rect.min.y),
                        egui::pos2(rect.min.x + half, rect.max.y),
                    ],
                    egui::Stroke::new(1.0_f32, color(theme.ui.border)),
                );
            }
        });
        self.top_row = (output.state.offset.y / pitch).round() as usize;
        event
    }
}

/// Which way a header arrow points.
#[derive(Clone, Copy)]
enum Arrow {
    Up,
    Down,
}

/// The arrows are painted rather than written, the way the tab strips' marks
/// are: a monospace font is not asked to have a glyph for them.
fn arrow_button(ui: &mut egui::Ui, theme: &Theme, arrow: Arrow) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let stroke = egui::Stroke::new(
            1.3_f32,
            if response.hovered() {
                color(theme.ui.fg)
            } else {
                color(theme.ui.fg_dim)
            },
        );
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(3), color(theme.ui.hover_bg));
        }
        let c = rect.center();
        let (w, h) = (4.0, 3.0);
        let tip = match arrow {
            Arrow::Up => c + egui::vec2(0.0, -h),
            Arrow::Down => c + egui::vec2(0.0, h),
        };
        let back = match arrow {
            Arrow::Up => h,
            Arrow::Down => -h,
        };
        ui.painter()
            .line_segment([tip, tip + egui::vec2(-w, back)], stroke);
        ui.painter()
            .line_segment([tip, tip + egui::vec2(w, back)], stroke);
    }
    response
}

/// What the diff header asked for.
#[derive(PartialEq)]
pub enum DiffEvent {
    None,
    Close,
    Open,
}

fn summary(rows: &[Row]) -> String {
    let added = rows
        .iter()
        .filter(|r| matches!(r.kind, Kind::Added | Kind::Changed))
        .count();
    let removed = rows
        .iter()
        .filter(|r| matches!(r.kind, Kind::Removed | Kind::Changed))
        .count();
    format!("+{added}  −{removed}")
}

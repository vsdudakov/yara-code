//! Side-by-side diff of one changed file: what it was on the left, what it is
//! now on the right, changed lines tinted in the theme's own red and green.

use crate::core::diff::{Kind, Row};
use crate::core::theme::Theme;
use crate::gui::theme::{ansi_color, color, CODE_FONT_SIZE};

pub struct DiffView {
    /// Path as git reports it, relative to the worktree.
    pub path: String,
    pub rows: Vec<Row>,
    pub error: Option<String>,
}

impl DiffView {
    pub fn new(path: String, rows: Result<Vec<Row>, String>) -> Self {
        match rows {
            Ok(rows) => Self {
                path,
                rows,
                error: None,
            },
            Err(message) => Self {
                path,
                rows: Vec::new(),
                error: Some(message),
            },
        }
    }

    /// Draws the view. Returns true when the user asked to close it.
    pub fn ui(&self, ui: &mut egui::Ui, theme: &Theme) -> DiffEvent {
        let mut event = DiffEvent::None;
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
                            .button(egui::RichText::new("×").size(11.0))
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

        let font = egui::FontId::monospace(CODE_FONT_SIZE);
        let (char_w, row_h) = ui.fonts(|f| (f.glyph_width(&font, ' '), f.row_height(&font)));
        let gutter = char_w * 6.0;
        // The theme's own red and green, dimmed to a background wash and used
        // at full strength for the line numbers.
        let removed = ansi_color(theme, 1);
        let added = ansi_color(theme, 2);

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, self.rows.len(), |ui, range| {
                let width = ui.available_width();
                let half = (width / 2.0).max(80.0);
                for row in &self.rows[range] {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, row_h), egui::Sense::hover());
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let painter = ui.painter_at(rect);
                    let side = |x: f32, w: f32, line: Option<&crate::core::diff::Side>, tint: Option<egui::Color32>| {
                        let area = egui::Rect::from_min_size(
                            egui::pos2(x, rect.min.y),
                            egui::vec2(w, row_h),
                        );
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
                    side(rect.min.x + half, width - half, row.right.as_ref(), right_tint);
                    // The seam between the two versions.
                    painter.line_segment(
                        [
                            egui::pos2(rect.min.x + half, rect.min.y),
                            egui::pos2(rect.min.x + half, rect.max.y),
                        ],
                        egui::Stroke::new(1.0, color(theme.ui.border)),
                    );
                }
            });
        event
    }
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

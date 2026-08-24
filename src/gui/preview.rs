//! A markdown file as a reader sees it — the window's rendering of the same
//! blocks the terminal paints.

use crate::core::markdown::{Block, Span};
use crate::core::theme::Theme;
use crate::gui::theme::{code_font, color};

pub struct PreviewView {
    pub path: std::path::PathBuf,
    pub blocks: Vec<Block>,
}

/// What the preview's header asked for.
#[derive(PartialEq)]
pub enum PreviewEvent {
    None,
    Close,
}

impl PreviewView {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn ui(&self, ui: &mut egui::Ui, theme: &Theme) -> PreviewEvent {
        let mut event = PreviewEvent::None;
        egui::Frame::default()
            .fill(color(theme.ui.status_bg))
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} — preview", self.name()))
                            .color(color(theme.ui.fg))
                            .size(12.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(egui::RichText::new("×").size(11.0))
                            .on_hover_text("Close Preview")
                            .clicked()
                        {
                            event = PreviewEvent::Close;
                        }
                    });
                });
            });

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Prose needs a margin the editor's gutter would otherwise
                // provide; the width cap keeps lines readable on a wide
                // window.
                egui::Frame::default()
                    .inner_margin(egui::Margin::symmetric(28, 16))
                    .show(ui, |ui| {
                        ui.set_max_width(820.0);
                        ui.spacing_mut().item_spacing.y = 8.0;
                        self.blocks(ui, theme);
                    });
            });
        event
    }

    fn blocks(&self, ui: &mut egui::Ui, theme: &Theme) {
        let fg = color(theme.ui.fg);
        let faint = color(theme.ui.fg_faint);
        let accent = color(theme.ui.accent_light);
        let code_bg = color(theme.ui.sidebar_bg);
        {
            {
                for block in &self.blocks {
                    match block {
                        Block::Heading(level, spans) => {
                            let size = match level {
                                1 => 26.0,
                                2 => 21.0,
                                3 => 17.0,
                                _ => 14.5,
                            };
                            ui.add_space(if *level <= 2 { 10.0 } else { 4.0 });
                            let job = inline(spans, theme, size, true);
                            ui.label(job);
                            if *level == 1 {
                                ui.separator();
                            }
                        }
                        Block::Paragraph(spans) => {
                            ui.label(inline(spans, theme, 14.0, false));
                        }
                        Block::Code(language, text) => {
                            egui::Frame::default()
                                .fill(code_bg)
                                .inner_margin(egui::Margin::symmetric(10, 8))
                                .corner_radius(4.0)
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    if let Some(language) = language {
                                        ui.label(
                                            egui::RichText::new(language).color(faint).size(10.5),
                                        );
                                    }
                                    ui.label(
                                        egui::RichText::new(text.as_str())
                                            .color(fg)
                                            .font(code_font(ui)),
                                    );
                                });
                        }
                        Block::List(ordered, items) => {
                            for (n, item) in items.iter().enumerate() {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let bullet = if *ordered {
                                        format!("{}.", n + 1)
                                    } else {
                                        "•".to_string()
                                    };
                                    ui.add_space(12.0);
                                    ui.label(egui::RichText::new(bullet).color(faint).size(14.0));
                                    ui.label(inline(item, theme, 14.0, false));
                                });
                            }
                        }
                        Block::Quote(spans) => {
                            ui.horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(3.0, 20.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(rect, 0.0, accent);
                                ui.add_space(6.0);
                                ui.label(inline(spans, theme, 14.0, false));
                            });
                        }
                        Block::Rule => {
                            ui.separator();
                        }
                    }
                }
            }
        }
    }
}

/// A run of spans as one laid-out job, so bold, code and links sit inline.
fn inline(spans: &[Span], theme: &Theme, size: f32, heading: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let base_color = if heading {
        color(theme.ui.accent_light)
    } else {
        color(theme.ui.fg)
    };
    let fmt =
        |font: egui::FontId, colour: egui::Color32, bg: egui::Color32| egui::text::TextFormat {
            font_id: font,
            color: colour,
            background: bg,
            ..Default::default()
        };
    let prop = egui::FontId::proportional(size);
    let none = egui::Color32::TRANSPARENT;
    for span in spans {
        match span {
            Span::Text(t) => job.append(t, 0.0, fmt(prop.clone(), base_color, none)),
            Span::Bold(t) => {
                let mut f = fmt(prop.clone(), color(theme.ui.fg_bright), none);
                f.font_id = egui::FontId::new(size, egui::FontFamily::Proportional);
                job.append(t, 0.0, f);
            }
            Span::Italic(t) => {
                let mut f = fmt(prop.clone(), base_color, none);
                f.italics = true;
                job.append(t, 0.0, f);
            }
            Span::Code(t) => job.append(
                &format!(" {t} "),
                0.0,
                fmt(
                    egui::FontId::monospace(size - 1.0),
                    color(theme.ui.fg),
                    color(theme.ui.sidebar_bg),
                ),
            ),
            Span::Link(t, _) => {
                let mut f = fmt(prop.clone(), color(theme.ui.accent_light), none);
                f.underline = egui::Stroke::new(1.0_f32, color(theme.ui.accent_light));
                job.append(t, 0.0, f);
            }
        }
    }
    job
}

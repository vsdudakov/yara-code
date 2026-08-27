//! A markdown file as a reader sees it — the window's rendering of the same
//! blocks the terminal paints.

use crate::core::chart::{self, Chart, Shape};
use crate::core::markdown::{Align, Block, Item, Marker, Span, Table};
use crate::core::theme::{chart_color, Theme};
use crate::gui::theme::{code_font, color, icons};

pub struct PreviewView {
    pub path: std::path::PathBuf,
    pub blocks: Vec<Block>,
    /// The text the blocks were parsed from, so an edit re-renders.
    pub source: String,
}

impl PreviewView {
    /// Re-parses when the file's text moved on, so the preview follows the
    /// editor keystroke by keystroke.
    pub fn follow(&mut self, text: &str) {
        if self.source != text {
            self.source = text.to_string();
            self.blocks = crate::core::markdown::parse(text);
        }
    }
}

/// What the preview's header asked for.
#[derive(PartialEq)]
pub enum PreviewEvent {
    None,
    Close,
}

/// Prose is set at one size, with the leading a page of text needs: lines set
/// solid are what made the preview read as a terminal dump rather than as a
/// document.
const BODY: f32 = 14.0;
const LEADING: f32 = 1.65;
/// Headings lead more tightly than the prose under them, the way they do in
/// print — a two-line heading should still read as one thing.
const HEADING_LEADING: f32 = 1.25;
/// How far one level of a nested list is set in from the level above.
const NESTING: f32 = 20.0;

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
                            .button(egui::RichText::new(icons().close).size(11.0))
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
                        // One block stands off from the next by more than one
                        // line of its own text, which is what tells a
                        // paragraph from the paragraph under it.
                        ui.spacing_mut().item_spacing.y = 11.0;
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
        for (nth, block) in self.blocks.iter().enumerate() {
            // A centred block is painted like any other, in a column that
            // centres what goes in it and with its text set the same way —
            // which is what a README's `<div align="center">` asked for.
            let (block, centered) = match block {
                Block::Center(inner) => (inner.as_ref(), true),
                other => (other, false),
            };
            let job = |spans: &[Span], size: f32, heading: bool| {
                let mut job = inline(spans, theme, size, heading);
                if centered {
                    job.halign = egui::Align::Center;
                }
                job
            };
            let paint = |ui: &mut egui::Ui| match block {
                Block::Heading(level, spans) => {
                    let size = match level {
                        1 => 26.0,
                        2 => 21.0,
                        3 => 17.0,
                        _ => 14.5,
                    };
                    ui.add_space(if *level <= 2 { 14.0 } else { 6.0 });
                    ui.label(job(spans, size, true));
                    if *level <= 2 {
                        ui.add_space(2.0);
                        ui.separator();
                    }
                }
                Block::Paragraph(spans) => {
                    ui.label(job(spans, BODY, false));
                }
                Block::Code(language, text) => {
                    egui::Frame::default()
                        .fill(code_bg)
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            if let Some(language) = language {
                                ui.label(egui::RichText::new(language).color(faint).size(10.5));
                            }
                            ui.label(
                                egui::RichText::new(text.as_str())
                                    .color(fg)
                                    .font(code_font(ui)),
                            );
                        });
                }
                Block::List(items) => list(ui, theme, items),
                Block::Table(table) => grid(ui, theme, table, nth),
                Block::Chart(Chart::Pie { title, slices }) => pie(ui, theme, title, slices),
                Block::Chart(Chart::Flow(flow)) => flow_chart(ui, theme, flow),
                Block::Quote(spans) => {
                    // The bar runs the height of the quote rather than a fixed
                    // stub, so a quote of five lines is marked for five lines.
                    egui::Frame::default()
                        .inner_margin(egui::Margin {
                            left: 12,
                            right: 8,
                            top: 4,
                            bottom: 4,
                        })
                        .show(ui, |ui| {
                            let text = ui.label(job(spans, BODY, false));
                            let bar = egui::Rect::from_min_size(
                                egui::pos2(text.rect.left() - 12.0, text.rect.top()),
                                egui::vec2(3.0, text.rect.height()),
                            );
                            ui.painter().rect_filled(bar, 1.0, accent);
                        });
                }
                Block::Rule => {
                    ui.separator();
                }
                // Unwrapped above: a centred block holds one block, never
                // another wrapper.
                Block::Center(_) => {}
            };
            if centered {
                ui.vertical_centered(paint);
            } else {
                paint(ui);
            }
        }
    }
}

/// The items of a list, each set in by its depth and led by its own marker.
fn list(ui: &mut egui::Ui, theme: &Theme, items: &[Item]) {
    for item in items {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add_space(10.0 + item.depth as f32 * NESTING);
            match item.marker {
                Marker::Task(done) => {
                    let (mark, tint) = if done {
                        (icons().check_on, color(theme.ui.accent_light))
                    } else {
                        (icons().check_off, color(theme.ui.fg_faint))
                    };
                    ui.label(egui::RichText::new(mark).color(tint).size(BODY));
                }
                Marker::Number(n) => {
                    ui.label(
                        egui::RichText::new(format!("{n}."))
                            .color(color(theme.ui.fg_faint))
                            .size(BODY),
                    );
                }
                Marker::Bullet => {
                    // A level of nesting changes the bullet, as a printed list
                    // does, so depth is legible without counting indents.
                    let bullet = ["•", "◦", "▪"][item.depth % 3];
                    ui.label(
                        egui::RichText::new(bullet)
                            .color(color(theme.ui.fg_faint))
                            .size(BODY),
                    );
                }
            }
            ui.label(inline(&item.spans, theme, BODY, false));
        });
    }
}

/// A table, ruled under its heading and striped through its rows so a wide row
/// is followed across.
fn grid(ui: &mut egui::Ui, theme: &Theme, table: &Table, nth: usize) {
    if table.columns() == 0 {
        return;
    }
    let cell = |ui: &mut egui::Ui, spans: &[Span], align: Align, head: bool| {
        let job = inline(spans, theme, BODY, head);
        let layout = match align {
            Align::Left => egui::Layout::left_to_right(egui::Align::Center),
            Align::Center => egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            Align::Right => egui::Layout::right_to_left(egui::Align::Center),
        };
        ui.with_layout(layout, |ui| ui.label(job));
    };
    egui::Frame::default()
        .stroke(egui::Stroke::new(1.0_f32, color(theme.ui.border)))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            // Where the heading ends, so the rule under it can be drawn across
            // the whole table: a separator inside a grid cell is a vertical bar
            // as tall as the row, which is not what a table's heading wears.
            let under_head = std::cell::Cell::new(0.0_f32);
            let grid = egui::Grid::new(egui::Id::new(("preview_table", nth)))
                .striped(true)
                .spacing(egui::vec2(18.0, 6.0))
                .show(ui, |ui| {
                    for (c, head) in table.head.iter().enumerate() {
                        cell(ui, head, table.align[c], true);
                    }
                    ui.end_row();
                    under_head.set(ui.min_rect().bottom() + 3.0);
                    for row in &table.rows {
                        for (c, text) in row.iter().enumerate() {
                            cell(ui, text, table.align[c], false);
                        }
                        ui.end_row();
                    }
                });
            if !table.rows.is_empty() {
                ui.painter().hline(
                    grid.response.rect.x_range(),
                    under_head.get(),
                    egui::Stroke::new(1.0_f32, color(theme.ui.border)),
                );
            }
        });
}

/// A pie: the circle, and beside it what each slice stands for.
fn pie(ui: &mut egui::Ui, theme: &Theme, title: &Option<String>, slices: &[chart::Slice]) {
    if slices.is_empty() {
        return;
    }
    if let Some(title) = title {
        ui.label(
            egui::RichText::new(title)
                .color(color(theme.ui.fg_bright))
                .size(BODY + 1.0),
        );
    }
    let shares = chart::shares(slices);
    ui.horizontal(|ui| {
        const SIZE: f32 = 150.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::hover());
        let centre = rect.center();
        let radius = SIZE / 2.0 - 4.0;
        let mut from = -std::f32::consts::FRAC_PI_2;
        for (i, share) in shares.iter().enumerate() {
            // A slice is drawn as a fan of triangles: enough of them that its
            // arc reads as a curve at any size a preview is shown at.
            let sweep = *share as f32 * std::f32::consts::TAU;
            let steps = ((sweep * radius / 4.0) as usize).clamp(2, 180);
            let mut points = vec![centre];
            for step in 0..=steps {
                let angle = from + sweep * step as f32 / steps as f32;
                points.push(centre + egui::vec2(angle.cos(), angle.sin()) * radius);
            }
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                color(chart_color(theme, i)),
                egui::Stroke::new(1.0_f32, color(theme.ui.editor_bg)),
            ));
            from += sweep;
        }
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for (i, slice) in slices.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (swatch, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(swatch, 2.0, color(chart_color(theme, i)));
                    ui.label(
                        egui::RichText::new(format!("{}  {:.0}%", slice.label, shares[i] * 100.0))
                            .color(color(theme.ui.fg))
                            .size(BODY - 1.0),
                    );
                });
            }
        });
    });
}

/// A flowchart, painted from the same cell layout the terminal draws in
/// characters: one cell is one character of the code font.
fn flow_chart(ui: &mut egui::Ui, theme: &Theme, flow: &chart::Flow) {
    let diagram = chart::lay_out(flow);
    if diagram.width == 0 {
        return;
    }
    let font = egui::FontId::monospace(BODY - 1.0);
    let cell = egui::vec2(
        ui.fonts(|f| f.glyph_width(&font, 'M')),
        ui.text_style_height(&egui::TextStyle::Body) * 1.2,
    );
    let size = egui::vec2(
        diagram.width as f32 * cell.x,
        diagram.height as f32 * cell.y,
    );
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let at = |x: usize, y: usize| rect.min + egui::vec2(x as f32 * cell.x, y as f32 * cell.y);
    let line = egui::Stroke::new(1.4_f32, color(theme.ui.fg_faint));

    for wire in &diagram.wires {
        // The middle of a cell is where the terminal would put the character,
        // so the two frontends bend their arrows in the same places.
        let bend: Vec<egui::Pos2> = wire
            .points
            .iter()
            .map(|(x, y)| at(*x, *y) + cell / 2.0)
            .collect();
        for (i, pair) in bend.windows(2).enumerate() {
            let last = i + 2 == bend.len();
            if last {
                painter.arrow(pair[0], pair[1] - pair[0], line);
            } else if wire.dashed {
                dashes(&painter, pair[0], pair[1], line);
            } else {
                painter.line_segment([pair[0], pair[1]], line);
            }
        }
        if let Some(label) = &wire.label {
            if let Some(start) = bend.first() {
                painter.text(
                    *start + egui::vec2(4.0, -2.0),
                    egui::Align2::LEFT_BOTTOM,
                    label,
                    egui::FontId::proportional(BODY - 3.0),
                    color(theme.ui.fg_dim),
                );
            }
        }
    }
    for placed in &diagram.boxes {
        let node = &flow.nodes[placed.node];
        let box_rect = egui::Rect::from_min_size(
            at(placed.x, placed.y),
            egui::vec2(placed.w as f32 * cell.x, chart::BOX_H as f32 * cell.y),
        );
        let fill = color(theme.ui.sidebar_bg);
        let edge = egui::Stroke::new(1.4_f32, color(theme.ui.accent_light));
        match node.shape {
            Shape::Diamond => {
                let c = box_rect.center();
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(c.x, box_rect.top()),
                        egui::pos2(box_rect.right(), c.y),
                        egui::pos2(c.x, box_rect.bottom()),
                        egui::pos2(box_rect.left(), c.y),
                    ],
                    fill,
                    edge,
                ));
            }
            shape => {
                let radius = if shape == Shape::Round {
                    box_rect.height() / 2.0
                } else {
                    4.0
                };
                painter.rect(box_rect, radius, fill, edge, egui::StrokeKind::Inside);
            }
        }
        painter.text(
            box_rect.center(),
            egui::Align2::CENTER_CENTER,
            &node.label,
            font.clone(),
            color(theme.ui.fg),
        );
    }
}

/// A dotted arrow, drawn as the broken line the terminal writes with `╌`.
fn dashes(painter: &egui::Painter, from: egui::Pos2, to: egui::Pos2, stroke: egui::Stroke) {
    let step = 6.0;
    let span = to - from;
    let length = span.length();
    let mut walked = 0.0;
    while walked < length {
        let next = (walked + step / 2.0).min(length);
        painter.line_segment(
            [
                from + span * (walked / length),
                from + span * (next / length),
            ],
            stroke,
        );
        walked += step;
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
    // The leading is set on every run, since a line takes the tallest of the
    // formats on it: a paragraph with code in it must not lead tighter than
    // the paragraph above it.
    let leading = size * if heading { HEADING_LEADING } else { LEADING };
    let fmt =
        |font: egui::FontId, colour: egui::Color32, bg: egui::Color32| egui::text::TextFormat {
            font_id: font,
            color: colour,
            background: bg,
            line_height: Some(leading),
            ..Default::default()
        };
    let prop = egui::FontId::proportional(size);
    let none = egui::Color32::TRANSPARENT;
    for span in spans {
        match span {
            Span::Text(t) => job.append(t, 0.0, fmt(prop.clone(), base_color, none)),
            // With no bold face in the tree, weight is carried by the brighter
            // of the two text colours — the same stand-in the terminal makes.
            Span::Bold(t) => job.append(t, 0.0, fmt(prop.clone(), color(theme.ui.fg_bright), none)),
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
            // Nothing here decodes a picture, so an image is set as what it
            // was described as, marked as standing in for one.
            Span::Image(alt, _) => {
                let mut f = fmt(prop.clone(), color(theme.ui.fg_dim), none);
                f.italics = true;
                job.append(&format!("{} {alt}", icons().preview), 0.0, f);
            }
        }
    }
    job
}

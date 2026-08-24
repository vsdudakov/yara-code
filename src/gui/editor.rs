//! Tabbed code editor: line numbers, themed syntax highlighting, smart indent
//! and ⌘-click navigation.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use crate::core::buffer::{word_at, Buffers};
use crate::core::find::Find;
use crate::core::fold::{self, Region};
use crate::core::git::LineState;
use crate::core::history::EditKind;
use crate::core::indent;
use crate::core::settings::{Indent, Modifier};
use crate::core::theme as core_theme;
use crate::core::theme::Theme;
use crate::gui::diff::DiffView;
use crate::gui::fold_view::Mapping;
use crate::gui::highlight;
use crate::gui::theme::{ansi_color, code_font, color};

/// Space between the tab strip and the first line of text.
const EDITOR_TOP_PAD: f32 = 8.0;

#[derive(Default)]
pub struct Editor {
    pub buffers: Buffers,
    /// Open markdown previews, tabs of their own beside the files.
    pub previews: Vec<crate::gui::preview::PreviewView>,
    /// Where the tab strip is scrolled to, and where a `‹`/`›` click asks it
    /// to go on the next frame.
    tab_offset: f32,
    tab_scroll_to: Option<f32>,
    /// Whether the tabs ran past the strip last frame, so the arrows show.
    tabs_overflow: bool,
    /// The tab in front last frame; when it changes the new one is scrolled
    /// into view.
    shown_tab: Option<(usize, Option<usize>, Option<usize>)>,
    /// Which preview tab is in front; `None` while a file or diff is.
    pub active_preview: Option<usize>,
    /// Open diffs, tabs of their own beside the files.
    pub diffs: Vec<DiffView>,
    /// Which diff tab is in front; `None` while a file is.
    pub active_diff: Option<usize>,
    /// (line, column), 1-based, of the primary cursor in the active buffer.
    pub cursor: Option<(usize, usize)>,
    /// Scroll to this 1-based line of this buffer on the next frame it is shown.
    pub pending_jump: Option<(PathBuf, usize)>,
    /// Identifier the user ⌘-clicked; the app resolves it to a definition.
    pub goto_request: Option<String>,
    /// Buffer whose close is awaiting the save/discard confirmation.
    pub pending_close: Option<usize>,
    /// Collapsed block headers, per file.
    folds: HashMap<PathBuf, BTreeSet<usize>>,
    /// Foldable blocks of the active buffer, recomputed when its text changes.
    regions: Vec<Region>,
    regions_key: Option<(PathBuf, usize)>,
    /// The find bar's state, shared across buffers like the editor's own.
    pub find: Find,
    /// Character range to select on the next frame, from a find step.
    pending_select: Option<(usize, usize)>,
}

/// The tab being dragged along the strip.
#[derive(Clone, Copy)]
struct TabDrag(usize);

impl Editor {
    /// Opens `path` in a tab. False when it cannot be read as text, so the
    /// caller can say so rather than let a click do nothing.
    pub fn open(&mut self, path: PathBuf) -> bool {
        if !self.buffers.open(path) {
            return false;
        }
        // The file asked for is what should be in front — not a diff or a
        // preview that happened to be showing.
        self.active_diff = None;
        self.active_preview = None;
        true
    }

    // ----- find in file --------------------------------------------------

    /// Opens the bar, seeding it from the buffer and landing on the match
    /// nearest the cursor.
    pub fn open_find(&mut self) {
        let Some((text, path)) = self
            .buffers
            .active()
            .map(|b| (b.text.clone(), b.path.clone()))
        else {
            return;
        };
        self.find.open_on(&path);
        self.find.focus_pending = true;
        self.find.refresh(&text);
        let cursor_chars = self.cursor_char_index();
        self.find.select_near(cursor_chars);
    }

    /// Seeds the find bar from a project-search result so the file opens with
    /// every match highlighted and the clicked one current.
    pub fn seed_find(
        &mut self,
        query: String,
        regex: bool,
        case_sensitive: bool,
        whole_word: bool,
        line: usize,
    ) {
        if query.is_empty() {
            return;
        }
        self.find.query = query;
        self.find.regex = regex;
        self.find.case_sensitive = case_sensitive;
        self.find.whole_word = whole_word;
        let Some((text, path)) = self
            .buffers
            .active()
            .map(|b| (b.text.clone(), b.path.clone()))
        else {
            return;
        };
        self.find.open_on(&path);
        self.find.refresh(&text);
        if let Some(index) = self.find.hits.iter().position(|h| h.line + 1 == line) {
            self.find.current = index;
        }
    }

    /// The identifier the caret is on or touching, for go-to-definition
    /// from the keyboard — the same word Cmd+click would pick.
    pub fn word_at_cursor(&self) -> Option<String> {
        let text = &self.buffers.active()?.text;
        word_at(text, self.cursor_char_index()).map(|(word, _, _)| word)
    }

    fn cursor_char_index(&self) -> usize {
        let Some(buf) = self.buffers.active() else {
            return 0;
        };
        let (line, col) = self.cursor.unwrap_or((1, 1));
        let mut index = 0;
        for (n, text) in buf.text.split('\n').enumerate() {
            if n + 1 == line {
                return index + col.saturating_sub(1).min(text.chars().count());
            }
            index += text.chars().count() + 1;
        }
        index
    }

    /// Re-runs the search on the open buffer — after the query or an option
    /// changed — and lands on the match nearest the cursor.
    pub fn refresh_find(&mut self) {
        let Some(text) = self.buffers.active().map(|b| b.text.clone()) else {
            return;
        };
        self.find.refresh(&text);
        let cursor = self.cursor_char_index();
        self.find.select_near(cursor);
        self.reveal_current_hit();
    }

    /// Moves to the next or previous match and selects it.
    pub fn find_step(&mut self, delta: isize) {
        let Some(text) = self.buffers.active().map(|b| b.text.clone()) else {
            return;
        };
        self.find.refresh(&text);
        self.find.step(delta);
        self.reveal_current_hit();
    }

    /// Expands any fold hiding the current match, then queues its selection.
    fn reveal_current_hit(&mut self) {
        let Some(hit) = self.find.hit() else { return };
        self.refresh_regions();
        if let Some(path) = self.buffers.active().map(|b| b.path.clone()) {
            let enclosing = fold::context(&self.regions, hit.line, usize::MAX);
            if let Some(folds) = self.folds.get_mut(&path) {
                for header in enclosing {
                    folds.remove(&header);
                }
            }
        }
        self.pending_select = Some((hit.start, hit.end));
    }

    pub fn find_replace_current(&mut self) {
        let Some(text) = self.buffers.active().map(|b| b.text.clone()) else {
            return;
        };
        self.find.refresh(&text);
        let at = self.cursor_offset();
        if let Some((updated, cursor)) = self.find.replace_current(&text) {
            if let Some(buf) = self.buffers.active_mut() {
                buf.record(EditKind::Bulk, at);
                buf.text = updated.clone();
            }
            self.find.refresh(&updated);
            self.find.select_near(cursor);
            self.reveal_current_hit();
        }
    }

    pub fn find_replace_all(&mut self) -> usize {
        let Some(text) = self.buffers.active().map(|b| b.text.clone()) else {
            return 0;
        };
        self.find.refresh(&text);
        match self.find.replace_all(&text) {
            Some((updated, count)) => {
                let at = self.cursor_offset();
                if let Some(buf) = self.buffers.active_mut() {
                    buf.record(EditKind::Bulk, at);
                    buf.text = updated.clone();
                }
                self.find.refresh(&updated);
                count
            }
            None => 0,
        }
    }

    /// The cursor as a char offset into the active buffer, from the (line,
    /// column) the text widget last reported.
    fn cursor_offset(&self) -> usize {
        let Some((line, col)) = self.cursor else {
            return 0;
        };
        let Some(buf) = self.buffers.active() else {
            return 0;
        };
        let mut offset = 0usize;
        for (n, text) in buf.text.split('\n').enumerate() {
            if n + 1 == line {
                return offset + col.saturating_sub(1).min(text.chars().count());
            }
            offset += text.chars().count() + 1;
        }
        offset
    }

    /// Undo (`back`) or redo; returns false when there is nothing to step to.
    pub fn step_history(&mut self, back: bool) -> bool {
        let at = self.cursor_offset();
        let Some(buf) = self.buffers.active_mut() else {
            return false;
        };
        let moved = if back { buf.undo(at) } else { buf.redo(at) };
        let Some(cursor) = moved else {
            return false;
        };
        let text = buf.text.clone();
        // Put the cursor back where the step left it, and keep the find bar's
        // hits in step with the text it is searching.
        self.pending_select = Some((cursor, cursor));
        if self.find_showing() {
            self.find.refresh(&text);
        }
        true
    }

    // ----- folding -------------------------------------------------------

    /// Recomputes the active buffer's blocks when its text has changed, and
    /// drops folds whose header no longer opens one.
    fn refresh_regions(&mut self) {
        let Some(buf) = self.buffers.active() else {
            self.regions.clear();
            self.regions_key = None;
            return;
        };
        let key = (buf.path.clone(), buf.text.len());
        if self.regions_key.as_ref() == Some(&key) {
            return;
        }
        self.regions = fold::regions(&buf.text, &buf.extension);
        self.regions_key = Some(key);
        let starts = fold::all_starts(&self.regions);
        if let Some(folds) = self.folds.get_mut(&buf.path) {
            folds.retain(|line| starts.contains(line));
        }
    }

    pub fn toggle_fold(&mut self, line: usize) {
        if fold::region_at(&self.regions, line).is_none() {
            return;
        }
        let Some(path) = self.buffers.active().map(|b| b.path.clone()) else {
            return;
        };
        let folds = self.folds.entry(path).or_default();
        if !folds.remove(&line) {
            folds.insert(line);
        }
    }

    /// Folds the block at the cursor, or the innermost one around it.
    pub fn toggle_fold_at_cursor(&mut self) {
        self.refresh_regions();
        let line = self.cursor.map_or(1, |(l, _)| l) - 1;
        if fold::region_at(&self.regions, line).is_some() {
            self.toggle_fold(line);
        } else if let Some(header) = fold::context(&self.regions, line, 1).first().copied() {
            self.toggle_fold(header);
        }
    }

    pub fn fold_all(&mut self) {
        self.refresh_regions();
        let starts = fold::all_starts(&self.regions);
        if let Some(path) = self.buffers.active().map(|b| b.path.clone()) {
            self.folds.insert(path, starts);
        }
    }

    pub fn unfold_all(&mut self) {
        if let Some(path) = self.buffers.active().map(|b| b.path.clone()) {
            self.folds.remove(&path);
        }
    }

    /// Current (path, 1-based line) for the navigation history.
    pub fn location(&self) -> Option<(PathBuf, usize)> {
        let buf = self.buffers.active()?;
        Some((buf.path.clone(), self.cursor.map_or(1, |(l, _)| l)))
    }

    /// Opens a diff as a tab of its own, or brings the one already open for
    /// that path to the front.
    pub fn open_diff(&mut self, view: DiffView) {
        match self.diffs.iter().position(|d| d.path == view.path) {
            Some(i) => {
                self.diffs[i] = view;
                self.active_diff = Some(i);
            }
            None => {
                self.diffs.push(view);
                self.active_diff = Some(self.diffs.len() - 1);
            }
        }
    }

    /// Opens a rendered view of the file in front, or closes the one already
    /// open for it. Only markdown has a preview to show.
    pub fn toggle_preview(&mut self) -> Result<(), String> {
        let Some(buf) = self.buffers.active() else {
            return Err("open a markdown file first".into());
        };
        if !matches!(buf.extension.as_str(), "md" | "markdown") {
            return Err(format!("{} is not markdown", buf.name()));
        }
        let path = buf.path.clone();
        if let Some(i) = self.previews.iter().position(|p| p.path == path) {
            self.close_preview(i);
            return Ok(());
        }
        self.previews.push(crate::gui::preview::PreviewView {
            path,
            blocks: crate::core::markdown::parse(&buf.text),
        });
        self.active_preview = Some(self.previews.len() - 1);
        self.active_diff = None;
        Ok(())
    }

    pub fn close_preview(&mut self, index: usize) {
        if index >= self.previews.len() {
            return;
        }
        self.previews.remove(index);
        self.active_preview = match self.active_preview {
            Some(a) if a == index => None,
            Some(a) if a > index => Some(a - 1),
            other => other,
        };
    }

    pub fn close_diff(&mut self, index: usize) {
        if index >= self.diffs.len() {
            return;
        }
        self.diffs.remove(index);
        self.active_diff = match self.active_diff {
            Some(active) if active == index => None,
            Some(active) if active > index => Some(active - 1),
            other => other,
        };
    }

    /// The tab strip: the open files, then any open diffs. Tabs can be dragged
    /// onto one another to reorder them.
    /// The strip of tabs: files, then diffs, then previews. More than fit
    /// scroll behind `‹ ›` at the right; a markdown file in front gets a
    /// Preview button there too, `preview_chord` being the key that does the
    /// same.
    pub fn tab_bar(&mut self, ui: &mut egui::Ui, theme: &Theme, preview_chord: Option<&str>) {
        let mut close: Option<usize> = None;
        let mut preview_clicked = false;
        let in_front = (self.buffers.active, self.active_diff, self.active_preview);
        let reveal = self.shown_tab != Some(in_front);
        self.shown_tab = Some(in_front);
        let markdown_in_front = self.active_diff.is_none()
            && self.active_preview.is_none()
            && self
                .buffers
                .active()
                .is_some_and(|buf| matches!(buf.extension.as_str(), "md" | "markdown"));
        let mut activate: Option<usize> = None;
        let mut moved: Option<(usize, usize)> = None;
        let mut close_diff: Option<usize> = None;
        let mut show_diff: Option<usize> = None;
        let mut close_preview: Option<usize> = None;
        let mut show_preview: Option<usize> = None;
        ui.spacing_mut().item_spacing.x = 1.0;
        ui.horizontal(|ui| {
            // What stands at the right edge is measured first so the strip
            // knows how much room it has.
            let controls = if self.tabs_overflow { 52.0 } else { 0.0 }
                + if markdown_in_front { 96.0 } else { 0.0 };
            let strip_width = (ui.available_width() - controls).max(0.0);
            let mut area = egui::ScrollArea::horizontal()
                .id_salt("tab_strip")
                .max_width(strip_width)
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
            if let Some(x) = self.tab_scroll_to.take() {
                area = area.horizontal_scroll_offset(x);
            }
            let strip = area.show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                ui.horizontal(|ui| {
                    for (i, buf) in self.buffers.list.iter().enumerate() {
                        let selected = i == self.buffers.active && self.active_diff.is_none();
                        let fill = if selected {
                            color(theme.ui.tab_active_bg)
                        } else {
                            color(theme.ui.tab_inactive_bg)
                        };
                        let frame = egui::Frame::default()
                            .fill(fill)
                            .inner_margin(egui::Margin::symmetric(10, 7));
                        let tab = frame.show(ui, |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let fg = if selected {
                                color(theme.ui.fg)
                            } else {
                                color(theme.ui.fg_dim)
                            };
                            let title = egui::RichText::new(buf.name()).color(fg).size(13.0);
                            let resp = ui
                                .add(egui::Label::new(title).sense(egui::Sense::click_and_drag()))
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            resp.dnd_set_drag_payload(TabDrag(i));
                            // Answered below, once the tab's full rect is known.
                            let dropped = resp.dnd_release_payload::<TabDrag>().map(|src| src.0);
                            let hovering = resp.dnd_hover_payload::<TabDrag>().is_some();

                            // Modified dot / close cross, drawn as shapes so they show
                            // up regardless of font coverage. Hovering the dot turns it
                            // into a cross, like VS Code.
                            let (icon_rect, close_resp) = ui
                                .allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::click());
                            let close_resp =
                                close_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                            if ui.is_rect_visible(icon_rect) {
                                let hovered = close_resp.hovered();
                                let mark = if hovered {
                                    color(theme.ui.fg)
                                } else {
                                    color(theme.ui.fg_dim)
                                };
                                if hovered {
                                    ui.painter().rect_filled(
                                        icon_rect,
                                        egui::CornerRadius::same(3),
                                        color(theme.ui.hover_bg),
                                    );
                                }
                                if buf.modified() && !hovered {
                                    ui.painter().circle_filled(icon_rect.center(), 3.5, mark);
                                } else {
                                    let r = 3.2;
                                    let c = icon_rect.center();
                                    let stroke = egui::Stroke::new(1.3_f32, mark);
                                    ui.painter().line_segment(
                                        [c + egui::vec2(-r, -r), c + egui::vec2(r, r)],
                                        stroke,
                                    );
                                    ui.painter().line_segment(
                                        [c + egui::vec2(r, -r), c + egui::vec2(-r, r)],
                                        stroke,
                                    );
                                }
                            }
                            if close_resp.clicked() {
                                close = Some(i);
                            } else if resp.clicked() {
                                activate = Some(i);
                            }
                            (dropped, hovering)
                        });
                        // Dropping is answered by the tab's own label — adding a
                        // second widget over the tab would swallow its clicks — but the
                        // mark is painted over the whole tab, which is what reads as
                        // "this tab moves here".
                        let (dropped, hovering) = tab.inner;
                        if selected && self.active_preview.is_none() && reveal {
                            tab.response.scroll_to_me(None);
                        }
                        if hovering {
                            ui.painter().rect_stroke(
                                tab.response.rect,
                                egui::CornerRadius::ZERO,
                                egui::Stroke::new(1.0_f32, color(theme.ui.accent_light)),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if let Some(src) = dropped {
                            moved = Some((src, i));
                        }
                    }
                    for (i, diff) in self.diffs.iter().enumerate() {
                        let name = diff
                            .path
                            .rsplit('/')
                            .next()
                            .unwrap_or(&diff.path)
                            .to_string();
                        let (tab, cross) =
                            Self::side_tab(ui, theme, "≠", &name, self.active_diff == Some(i));
                        if self.active_diff == Some(i) && reveal {
                            tab.scroll_to_me(None);
                        }
                        if cross.clicked() {
                            close_diff = Some(i);
                        } else if tab.clicked() {
                            show_diff = Some(i);
                        }
                    }
                    for (i, preview) in self.previews.iter().enumerate() {
                        let (tab, cross) = Self::side_tab(
                            ui,
                            theme,
                            "◫",
                            &format!("{} preview", preview.name()),
                            self.active_preview == Some(i),
                        );
                        if self.active_preview == Some(i) && reveal {
                            tab.scroll_to_me(None);
                        }
                        if cross.clicked() {
                            close_preview = Some(i);
                        } else if tab.clicked() {
                            show_preview = Some(i);
                        }
                    }
                });
            });
            self.tab_offset = strip.state.offset.x;
            let overflow = strip.content_size.x > strip.inner_rect.width() + 0.5;
            self.tabs_overflow = overflow;
            if overflow {
                let step = (strip.inner_rect.width() * 0.6).max(120.0);
                let farthest = (strip.content_size.x - strip.inner_rect.width()).max(0.0);
                ui.spacing_mut().item_spacing.x = 0.0;
                let arrow = |ui: &mut egui::Ui, text: &str, enabled: bool| -> bool {
                    let label = egui::RichText::new(text)
                        .size(16.0)
                        .color(color(if enabled {
                            theme.ui.fg
                        } else {
                            theme.ui.fg_faint
                        }));
                    ui.add_enabled(enabled, egui::Button::new(label).frame(false))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                };
                if arrow(ui, " ‹ ", self.tab_offset > 0.5) {
                    self.tab_scroll_to = Some((self.tab_offset - step).max(0.0));
                }
                if arrow(ui, " › ", self.tab_offset < farthest - 0.5) {
                    self.tab_scroll_to = Some((self.tab_offset + step).min(farthest));
                }
            }
            if markdown_in_front {
                ui.spacing_mut().item_spacing.x = 0.0;
                let hint = match preview_chord {
                    Some(chord) => format!("Render this markdown ({chord})"),
                    None => "Render this markdown".to_string(),
                };
                let label = egui::RichText::new("◫ Preview")
                    .size(12.5)
                    .color(color(theme.ui.fg));
                if ui
                    .add(egui::Button::new(label).frame(false))
                    .on_hover_text(hint)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    preview_clicked = true;
                }
            }
        });
        if preview_clicked {
            // Cannot fail: the button is only there when markdown is in front.
            let _ = self.toggle_preview();
        }
        if let Some((from, to)) = moved {
            self.buffers.reorder(from, to);
        }
        if let Some(i) = activate {
            self.buffers.active = i;
            self.active_diff = None;
            self.active_preview = None;
        }
        if let Some(i) = close {
            self.request_close(i);
        }
        if let Some(i) = show_diff {
            self.active_diff = Some(i);
            self.active_preview = None;
        }
        if let Some(i) = show_preview {
            self.active_preview = Some(i);
            self.active_diff = None;
        }
        if let Some(i) = close_preview {
            self.close_preview(i);
        }
        if let Some(i) = close_diff {
            self.close_diff(i);
        }
    }

    /// A diff's own tab, drawn after the files it sits beside.
    /// A tab that is not a file: a diff or a preview, told apart by its
    /// glyph — `≠` and `◫`, the same two the terminal frontend draws.
    fn side_tab(
        ui: &mut egui::Ui,
        theme: &Theme,
        glyph: &str,
        label: &str,
        selected: bool,
    ) -> (egui::Response, egui::Response) {
        let fill = if selected {
            color(theme.ui.tab_active_bg)
        } else {
            color(theme.ui.tab_inactive_bg)
        };
        let mut clicks = None;
        egui::Frame::default()
            .fill(fill)
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let fg = if selected {
                    color(theme.ui.fg)
                } else {
                    color(theme.ui.fg_dim)
                };
                let title = egui::RichText::new(format!("{glyph} {label}"))
                    .color(fg)
                    .size(13.0);
                let tab = ui
                    .add(egui::Label::new(title).sense(egui::Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let (icon_rect, cross) =
                    ui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::click());
                let cross = cross.on_hover_cursor(egui::CursorIcon::PointingHand);
                if ui.is_rect_visible(icon_rect) {
                    let mark = if cross.hovered() {
                        color(theme.ui.fg)
                    } else {
                        color(theme.ui.fg_dim)
                    };
                    let r = 3.2;
                    let c = icon_rect.center();
                    let stroke = egui::Stroke::new(1.3_f32, mark);
                    ui.painter()
                        .line_segment([c + egui::vec2(-r, -r), c + egui::vec2(r, r)], stroke);
                    ui.painter()
                        .line_segment([c + egui::vec2(r, -r), c + egui::vec2(-r, r)], stroke);
                }
                clicks = Some((tab, cross));
            });
        clicks.unwrap()
    }

    /// Closes a buffer outright when it is clean; otherwise asks first.
    pub fn request_close(&mut self, index: usize) {
        if self.buffers.list.get(index).is_some_and(|b| b.modified()) {
            self.pending_close = Some(index);
        } else {
            self.close(index);
        }
    }

    /// Closes a buffer. A find bar belonging to that file closes with it;
    /// one belonging to another tab stays as it was.
    pub fn close(&mut self, index: usize) {
        let closed = self.buffers.list.get(index).map(|b| b.path.clone());
        self.buffers.close(index);
        if closed.is_some() && self.find.owner == closed {
            self.find.open = false;
            self.find.owner = None;
        }
    }

    /// Whether the find bar is showing: it belongs to one file, and hides
    /// while another tab is in front.
    pub fn find_showing(&self) -> bool {
        match self.buffers.active() {
            Some(buf) => self.find.shows_for(&buf.path),
            None => false,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        indent_config: &Indent,
        goto_modifiers: &[Modifier],
        git_lines: &BTreeMap<usize, LineState>,
    ) {
        self.refresh_regions();
        // Nothing open: the app draws its start page instead.
        let Some(buf) = self.buffers.active() else {
            self.cursor = None;
            return;
        };

        let extension = buf.extension.clone();
        let theme_name = theme.name.clone();
        let path = buf.path.clone();
        let folds = self.folds.get(&path).cloned().unwrap_or_default();
        let hidden = fold::hidden_lines(&self.regions, &folds);
        let regions = self.regions.clone();

        // The widget edits one flat string, so folding shows a display copy
        // with the hidden lines removed and maps edits back afterwards.
        let mapping = Mapping::new(&buf.text, &hidden);
        let mut shown = mapping.display.clone();
        // What the widget starts from, and where the cursor was: an edit is
        // whatever differs from this once the widget has had its turn.
        let display_len = mapping.display.chars().count();
        let cursor_before = self.cursor_offset();
        let visible = mapping.lines.clone();
        let text_lines: Vec<String> = buf.text.split('\n').map(str::to_string).collect();

        // Find hits, mapped from the real text into what is on screen, so
        // every match is painted and the current one stands out.
        let hits: Vec<(usize, usize, bool)> = if self.find.shows_for(&path) {
            let mut starts = vec![0usize; text_lines.len() + 1];
            let mut display_start = vec![0usize; text_lines.len() + 1];
            let mut real = 0usize;
            let mut shown_at = 0usize;
            for (n, line) in text_lines.iter().enumerate() {
                starts[n] = real;
                display_start[n] = shown_at;
                let len = line.chars().count() + 1;
                real += len;
                if visible.contains(&n) {
                    shown_at += len;
                }
            }
            self.find
                .hits
                .iter()
                .enumerate()
                .filter(|(_, h)| visible.contains(&h.line))
                .map(|(i, h)| {
                    let offset = display_start[h.line] + (h.start - starts[h.line]);
                    (offset, offset + (h.end - h.start), i == self.find.current)
                })
                .collect()
        } else {
            Vec::new()
        };

        let layout_extension = extension.clone();
        let layout_theme = theme_name.clone();
        let match_bg = color(theme.ui.match_bg);
        let current_bg = color(theme.ui.selected_bg);
        let mut layouter = move |ui: &egui::Ui, text: &str, wrap_width: f32| -> Arc<egui::Galley> {
            let mut job = highlight::highlight(ui.ctx(), &layout_theme, &layout_extension, text);
            paint_matches(&mut job, text, &hits, match_bg, current_bg);
            job.wrap.max_width = wrap_width;
            ui.fonts(|f| f.layout_job(job))
        };

        let row_height = ui.fonts(|f| f.row_height(&code_font(ui)));
        let mut toggle: Option<usize> = None;

        let scroll = egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut jumped_to: Option<usize> = None;
                // Breathing room over the first line. It scrolls with the
                // text, so a pinned header can sit flush under the tabs.
                ui.add_space(EDITOR_TOP_PAD);
                ui.spacing_mut().item_spacing.x = 14.0;
                ui.horizontal_top(|ui| {
                    // Gutter: real line numbers plus a fold marker per row.
                    let (gutter, gutter_resp) = ui.allocate_exact_size(
                        egui::vec2(62.0, visible.len() as f32 * row_height),
                        egui::Sense::click(),
                    );
                    if ui.is_rect_visible(gutter) {
                        for (row, line) in visible.iter().enumerate() {
                            let y = gutter.min.y + row as f32 * row_height;
                            let center_y = y + row_height / 2.0;
                            // A bar at the gutter's edge where git says this
                            // line is new, changed, or closed over a removal.
                            if let Some(state) = git_lines.get(&(line + 1)) {
                                let (tint, height) = match state {
                                    LineState::Added => (ansi_color(theme, 2), row_height),
                                    LineState::Modified => (ansi_color(theme, 3), row_height),
                                    LineState::Removed => (ansi_color(theme, 1), 2.0),
                                };
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(gutter.min.x, y),
                                        egui::vec2(2.5, height),
                                    ),
                                    egui::CornerRadius::ZERO,
                                    tint,
                                );
                            }
                            ui.painter().text(
                                egui::pos2(gutter.min.x + 40.0, center_y),
                                egui::Align2::RIGHT_CENTER,
                                (line + 1).to_string(),
                                code_font(ui),
                                color(theme.ui.line_number),
                            );
                            if fold::region_at(&regions, *line).is_some() {
                                let hovered = gutter_resp
                                    .hover_pos()
                                    .is_some_and(|p| p.y >= y && p.y < y + row_height);
                                chevron(
                                    ui.painter(),
                                    egui::pos2(gutter.min.x + 52.0, center_y),
                                    !folds.contains(line),
                                    if hovered {
                                        color(theme.ui.fg)
                                    } else {
                                        color(theme.ui.fg_faint)
                                    },
                                );
                            }
                        }
                    }
                    if gutter_resp.clicked() {
                        if let Some(pos) = gutter_resp.interact_pointer_pos() {
                            let row = ((pos.y - gutter.min.y) / row_height).floor() as usize;
                            if let Some(line) = visible.get(row) {
                                toggle = Some(*line);
                            }
                        }
                    }
                    if gutter_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }

                    let te_id = egui::Id::new(("buffer", &path));
                    let jump = match &self.pending_jump {
                        Some((p, line)) if *p == path => {
                            let line = *line;
                            self.pending_jump = None;
                            Some(line)
                        }
                        _ => None,
                    };
                    if let Some(line) = jump {
                        // Place the cursor at the start of the target row.
                        let row = visible
                            .binary_search(&line.saturating_sub(1))
                            .unwrap_or_else(|i| i.min(visible.len().saturating_sub(1)));
                        let mut idx = 0usize;
                        for (n, l) in shown.split('\n').enumerate() {
                            if n >= row {
                                break;
                            }
                            idx += l.chars().count() + 1;
                        }
                        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), te_id)
                            .unwrap_or_default();
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(
                                egui::text::CCursor::new(idx),
                            )));
                        state.store(ui.ctx(), te_id);
                        ui.ctx().memory_mut(|m| m.request_focus(te_id));
                        jumped_to = Some(row);
                    }

                    // A find step selects its match, unfolded above, so the
                    // display offsets line up with the real ones.
                    if let Some((from, to)) = self.pending_select.take() {
                        let to_display = |real: usize| -> usize {
                            let mut seen = 0usize;
                            let mut display = 0usize;
                            for (n, line) in text_lines.iter().enumerate() {
                                let len = line.chars().count() + 1;
                                if seen + len > real {
                                    return if visible.contains(&n) {
                                        display + (real - seen)
                                    } else {
                                        display
                                    };
                                }
                                if visible.contains(&n) {
                                    display += len;
                                }
                                seen += len;
                            }
                            display
                        };
                        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), te_id)
                            .unwrap_or_default();
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(to_display(from)),
                                egui::text::CCursor::new(to_display(to)),
                            )));
                        state.store(ui.ctx(), te_id);
                        ui.ctx().memory_mut(|m| m.request_focus(te_id));
                        let row = visible
                            .binary_search(&line_of_char(&text_lines, from))
                            .unwrap_or_else(|i| i.min(visible.len().saturating_sub(1)));
                        jumped_to = Some(row);
                    }

                    // Smart indent: handle Enter ourselves so the new line
                    // inherits (and adjusts) the current indentation.
                    if ui.memory(|m| m.has_focus(te_id))
                        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                    {
                        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), te_id)
                            .unwrap_or_default();
                        if let Some(range) = state.cursor.char_range() {
                            let char_count = shown.chars().count();
                            let (a, b) = (range.primary.index, range.secondary.index);
                            let (start, end) = (a.min(b).min(char_count), a.max(b).min(char_count));
                            let byte_of = |idx: usize, text: &str| {
                                text.char_indices().nth(idx).map_or(text.len(), |(b, _)| b)
                            };
                            let (bs, be) = (byte_of(start, &shown), byte_of(end, &shown));
                            if start != end {
                                shown.replace_range(bs..be, "");
                            }
                            let edit =
                                indent::newline_edit(&shown, start, &extension, indent_config);
                            shown.insert_str(bs, &edit.insert);
                            let new_cursor = egui::text::CCursor::new(start + edit.cursor_offset);
                            state
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::one(new_cursor)));
                            state.store(ui.ctx(), te_id);
                        }
                    }

                    let output = egui::TextEdit::multiline(&mut shown)
                        .id(te_id)
                        .code_editor()
                        .frame(false)
                        .margin(egui::Margin::ZERO)
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter)
                        .show(ui);

                    // Indent guides: a hairline per level of indentation,
                    // drawn over the leading whitespace of the rows on screen.
                    let guide = color(core_theme::indent_guide(theme));
                    let char_w = ui.fonts(|f| f.glyph_width(&code_font(ui), ' '));
                    let clip = ui.clip_rect();
                    let guide_width = indent_config.width.max(1);
                    for (row, count) in indent::guides(&shown, guide_width).iter().enumerate() {
                        let y = output.galley_pos.y + row as f32 * row_height;
                        if *count == 0 || y + row_height < clip.min.y || y > clip.max.y {
                            continue;
                        }
                        for level in 0..*count {
                            let x = output.galley_pos.x + (level * guide_width) as f32 * char_w;
                            ui.painter().vline(
                                x + 0.5,
                                y..=y + row_height,
                                egui::Stroke::new(1.0_f32, guide),
                            );
                        }
                    }

                    if let Some(row) = jumped_to {
                        let rect = output.response.rect;
                        let y = rect.min.y + row as f32 * row_height;
                        let target = egui::Rect::from_min_size(
                            egui::pos2(rect.min.x, y),
                            egui::vec2(1.0, row_height),
                        );
                        ui.scroll_to_rect(target, Some(egui::Align::Center));
                    }

                    // ⌘-hover underlines the identifier under the pointer;
                    // ⌘-click asks the app to jump to its definition.
                    let goto_held = ui.input(|i| {
                        goto_modifiers.iter().any(|wanted| match wanted {
                            Modifier::Cmd => i.modifiers.command,
                            Modifier::Ctrl => i.modifiers.ctrl,
                            Modifier::Alt => i.modifiers.alt,
                            Modifier::Shift => i.modifiers.shift,
                        })
                    });
                    if goto_held {
                        if let Some(pos) = output.response.hover_pos() {
                            let cursor = output.galley.cursor_from_pos(pos - output.galley_pos);
                            if let Some((word, start, end)) = word_at(&shown, cursor.ccursor.index)
                            {
                                let rect_of = |idx: usize| {
                                    let c =
                                        output.galley.from_ccursor(egui::text::CCursor::new(idx));
                                    output
                                        .galley
                                        .pos_from_cursor(&c)
                                        .translate(output.galley_pos.to_vec2())
                                };
                                let (a, b) = (rect_of(start), rect_of(end));
                                if (a.top() - b.top()).abs() < 0.5 {
                                    ui.painter().hline(
                                        a.left()..=b.left(),
                                        b.bottom() - 1.0,
                                        egui::Stroke::new(1.0_f32, color(theme.ui.accent_light)),
                                    );
                                }
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                if ui.input(|i| i.pointer.primary_pressed()) {
                                    self.goto_request = Some(word);
                                }
                            }
                        }
                    }

                    // Cursor position is reported in real line numbers.
                    self.cursor = output.cursor_range.map(|r| {
                        let idx = r.primary.ccursor.index;
                        let mut row = 0;
                        let mut col = 1;
                        for ch in shown.chars().take(idx) {
                            if ch == '\n' {
                                row += 1;
                                col = 1;
                            } else {
                                col += 1;
                            }
                        }
                        (visible.get(row).copied().unwrap_or(row) + 1, col)
                    });
                });
            });

        if let Some(line) = toggle {
            self.toggle_fold(line);
        }
        // The edited display text goes back into the real buffer. What it
        // replaced becomes an undo step, folded into the run being typed.
        let edited = shown != mapping.display;
        if let Some(buf) = self.buffers.list.get_mut(self.buffers.active) {
            if edited {
                let kind = match shown.chars().count().cmp(&display_len) {
                    std::cmp::Ordering::Greater => EditKind::Insert,
                    std::cmp::Ordering::Less => EditKind::Delete,
                    // Same length, different text: a replacement, not a run.
                    std::cmp::Ordering::Equal => EditKind::Bulk,
                };
                buf.record(kind, cursor_before);
            }
            if hidden.is_empty() {
                buf.text = shown;
            } else {
                mapping.splice(&mut buf.text, &shown);
            }
        }
        // Match highlighting is painted from character offsets, so an edit
        // that shifts the text has to shift the hits with it.
        if edited && self.find_showing() {
            if let Some(text) = self.buffers.active().map(|b| b.text.clone()) {
                self.find.refresh(&text);
            }
        }
        // A cursor that moved without an edit ends the run being typed, so undo
        // follows what was typed where.
        if !edited && self.cursor_offset() != cursor_before {
            if let Some(buf) = self.buffers.list.get_mut(self.buffers.active) {
                buf.history.end_run();
            }
        }

        // Sticky scroll: pin the headers enclosing the topmost visible row.
        let first_row =
            ((scroll.state.offset.y - EDITOR_TOP_PAD).max(0.0) / row_height).floor() as usize;
        let first_line = visible.get(first_row).copied().unwrap_or(0);
        let sticky: Vec<usize> = fold::context(&self.regions, first_line, 3)
            .into_iter()
            .filter(|header| *header < first_line)
            .collect();
        if sticky.is_empty() {
            return;
        }
        let viewport = scroll.inner_rect;
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("sticky_scroll"),
        ));
        let band = egui::Rect::from_min_size(
            viewport.min,
            egui::vec2(viewport.width(), sticky.len() as f32 * row_height),
        );
        painter.rect_filled(band, egui::CornerRadius::ZERO, color(theme.ui.status_bg));
        painter.hline(
            band.x_range(),
            band.max.y,
            egui::Stroke::new(1.0_f32, color(theme.ui.border)),
        );
        let text = self
            .buffers
            .active()
            .map(|b| b.text.clone())
            .unwrap_or_default();
        let lines: Vec<&str> = text.split('\n').collect();
        for (row, header) in sticky.iter().enumerate() {
            let y = band.min.y + row as f32 * row_height;
            painter.text(
                egui::pos2(band.min.x + 40.0, y + row_height / 2.0),
                egui::Align2::RIGHT_CENTER,
                (header + 1).to_string(),
                code_font(ui),
                color(theme.ui.line_number),
            );
            let job = highlight::highlight(
                ui.ctx(),
                &theme_name,
                &extension,
                lines.get(*header).copied().unwrap_or(""),
            );
            let galley = ui.fonts(|f| f.layout_job(job));
            painter.galley(egui::pos2(band.min.x + 76.0, y), galley, color(theme.ui.fg));
        }
    }
}

/// Gives every find hit a background, splitting the highlighter's sections at
/// the hit boundaries so the colors survive syntax coloring.
fn paint_matches(
    job: &mut egui::text::LayoutJob,
    text: &str,
    hits: &[(usize, usize, bool)],
    normal: egui::Color32,
    current: egui::Color32,
) {
    if hits.is_empty() {
        return;
    }
    let byte_of = |chars: usize| {
        text.char_indices()
            .nth(chars)
            .map_or(text.len(), |(b, _)| b)
    };
    let ranges: Vec<(usize, usize, bool)> = hits
        .iter()
        .map(|(s, e, c)| (byte_of(*s), byte_of(*e), *c))
        .collect();

    let mut sections = Vec::with_capacity(job.sections.len());
    for section in std::mem::take(&mut job.sections) {
        let (start, end) = (section.byte_range.start, section.byte_range.end);
        let mut cuts = vec![start, end];
        for (from, to, _) in &ranges {
            for point in [*from, *to] {
                if point > start && point < end {
                    cuts.push(point);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let mut format = section.format.clone();
            if let Some((_, _, is_current)) =
                ranges.iter().find(|(from, to, _)| a >= *from && b <= *to)
            {
                format.background = if *is_current { current } else { normal };
            }
            sections.push(egui::text::LayoutSection {
                leading_space: if a == start {
                    section.leading_space
                } else {
                    0.0
                },
                byte_range: a..b,
                format,
            });
        }
    }
    job.sections = sections;
}

/// Which line a character offset falls on.
fn line_of_char(lines: &[String], index: usize) -> usize {
    let mut seen = 0usize;
    for (n, line) in lines.iter().enumerate() {
        let len = line.chars().count() + 1;
        if seen + len > index {
            return n;
        }
        seen += len;
    }
    lines.len().saturating_sub(1)
}

/// Fold marker, the same triangle the navigator draws.
fn chevron(painter: &egui::Painter, center: egui::Pos2, expanded: bool, color: egui::Color32) {
    let r = 3.4;
    let pts = if expanded {
        vec![
            center + egui::vec2(-r, -r * 0.6),
            center + egui::vec2(r, -r * 0.6),
            center + egui::vec2(0.0, r * 0.8),
        ]
    } else {
        vec![
            center + egui::vec2(-r * 0.6, -r),
            center + egui::vec2(-r * 0.6, r),
            center + egui::vec2(r * 0.8, 0.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

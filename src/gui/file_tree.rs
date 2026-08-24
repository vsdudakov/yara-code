//! Project navigator: file tree with the same context menu the terminal
//! frontend shows (open / new file / new folder / rename / move / delete),
//! plus drag-and-drop moving.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::core::fs_ops;
use crate::core::theme::Theme;
use crate::gui::theme::color;

pub enum TreeEvent {
    Open(PathBuf),
    /// User picked "Delete"; the app confirms before touching the disk.
    RequestDelete(PathBuf),
    Moved { from: PathBuf, to: PathBuf },
    /// A file operation failed; the app shows this in the status bar.
    Failed(String),
}

enum Pending {
    NewFile(PathBuf),
    NewDir(PathBuf),
    Rename(PathBuf),
    MoveTo(PathBuf),
}

impl Pending {
    fn dir(&self) -> &Path {
        match self {
            Self::NewFile(p) | Self::NewDir(p) => p,
            Self::Rename(p) | Self::MoveTo(p) => p.parent().unwrap_or(p),
        }
    }

    fn hint(&self) -> &'static str {
        match self {
            Self::NewFile(_) => "file name",
            Self::NewDir(_) => "folder name",
            Self::Rename(_) => "new name",
            Self::MoveTo(_) => "destination folder",
        }
    }
}

struct Editing {
    what: Pending,
    name: String,
    focus: bool,
}

pub struct FileTree {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    /// The highlighted row, exactly as in the terminal navigator.
    selected: Option<PathBuf>,
    editing: Option<Editing>,
    /// Set while a drag hovers a folder that would accept the drop.
    valid_drop_target: bool,
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            expanded: HashSet::new(),
            selected: None,
            editing: None,
            valid_drop_target: false,
        }
    }

    /// Expands every ancestor of `path` and highlights it — used when a jump
    /// (search result, go-to-definition) opens a file from elsewhere.
    pub fn reveal(&mut self, path: &Path) {
        let mut dir = path.parent();
        while let Some(d) = dir {
            if !d.starts_with(&self.root) {
                break;
            }
            self.expanded.insert(d.to_path_buf());
            if d == self.root {
                break;
            }
            dir = d.parent();
        }
        self.selected = Some(path.to_path_buf());
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        events: &mut Vec<TreeEvent>,
    ) {
        let root = self.root.clone();
        self.valid_drop_target = false;
        self.show_dir(ui, theme, &root, 0, events);

        // Remaining empty space: context menu + drop target for the root.
        let remaining = ui.available_height().max(40.0);
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), remaining),
            egui::Sense::click(),
        );
        if resp.dnd_hover_payload::<PathBuf>().is_some() {
            self.valid_drop_target = true;
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                egui::CornerRadius::ZERO,
                egui::Stroke::new(1.0, color(theme.ui.accent_light)),
                egui::StrokeKind::Inside,
            );
        }
        if let Some(src) = resp.dnd_release_payload::<PathBuf>() {
            report_move(&src, &root, events);
        }
        resp.context_menu(|ui| {
            self.context_menu(ui, theme, None, events);
        });

        // Ghost label near the pointer while dragging.
        if let Some(payload) = egui::DragAndDrop::payload::<PathBuf>(ui.ctx()) {
            ui.ctx().set_cursor_icon(if self.valid_drop_target {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::NoDrop
            });
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let painter = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("tree_dnd_ghost"),
                ));
                let name = payload
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                painter.text(
                    pos + egui::vec2(14.0, 14.0),
                    egui::Align2::LEFT_TOP,
                    name,
                    egui::FontId::proportional(12.0),
                    color(theme.ui.fg),
                );
            }
        }
    }

    /// The navigator context menu, identical in both frontends:
    /// Open | New File, New Folder | Rename, Move To... | Delete.
    fn context_menu(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        target: Option<(&Path, bool)>,
        events: &mut Vec<TreeEvent>,
    ) {
        let dir = match target {
            Some((path, true)) => path.to_path_buf(),
            Some((path, false)) => path.parent().unwrap_or(&self.root).to_path_buf(),
            None => self.root.clone(),
        };

        if let Some((path, false)) = target {
            if menu_item(ui, theme, "Open").clicked() {
                events.push(TreeEvent::Open(path.to_path_buf()));
                ui.close_menu();
            }
            ui.separator();
        }

        if menu_item(ui, theme, "New File").clicked() {
            self.start_editing(Pending::NewFile(dir.clone()), String::new());
            ui.close_menu();
        }
        if menu_item(ui, theme, "New Folder").clicked() {
            self.start_editing(Pending::NewDir(dir), String::new());
            ui.close_menu();
        }

        let Some((path, _)) = target else { return };
        ui.separator();
        if menu_item(ui, theme, "Rename").clicked() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.start_editing(Pending::Rename(path.to_path_buf()), name);
            ui.close_menu();
        }
        if menu_item(ui, theme, "Move To...").clicked() {
            self.start_editing(Pending::MoveTo(path.to_path_buf()), String::new());
            ui.close_menu();
        }
        ui.separator();
        if menu_item(ui, theme, "Delete").clicked() {
            events.push(TreeEvent::RequestDelete(path.to_path_buf()));
            ui.close_menu();
        }
    }

    fn start_editing(&mut self, what: Pending, name: String) {
        self.expanded.insert(what.dir().to_path_buf());
        self.editing = Some(Editing {
            what,
            name,
            focus: true,
        });
    }

    /// The inline name field, shown in place of (or above) a row.
    fn edit_row(&mut self, ui: &mut egui::Ui, depth: usize, events: &mut Vec<TreeEvent>) {
        let indent = 8.0 + depth as f32 * 12.0 + 16.0;
        let mut submit = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            let editing = self.editing.as_mut().unwrap();
            let resp = ui.add(
                egui::TextEdit::singleline(&mut editing.name)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Body)
                    .hint_text(editing.what.hint()),
            );
            if editing.focus {
                resp.request_focus();
                editing.focus = false;
            }
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                } else {
                    cancel = true;
                }
            }
        });
        if submit {
            let editing = self.editing.take().unwrap();
            match &editing.what {
                Pending::NewFile(dir) => match fs_ops::create_file(dir, &editing.name) {
                    Ok(path) => events.push(TreeEvent::Open(path)),
                    Err(e) => events.push(TreeEvent::Failed(format!("create failed: {e}"))),
                },
                Pending::NewDir(dir) => {
                    if let Err(e) = fs_ops::create_dir(dir, &editing.name) {
                        events.push(TreeEvent::Failed(format!("create failed: {e}")));
                    }
                }
                Pending::Rename(path) => match fs_ops::rename(path, &editing.name) {
                    Ok(to) => events.push(TreeEvent::Moved {
                        from: path.clone(),
                        to,
                    }),
                    Err(e) => events.push(TreeEvent::Failed(format!("rename failed: {e}"))),
                },
                Pending::MoveTo(path) => {
                    // A destination is taken relative to the project root
                    // unless it is absolute; empty means the root itself.
                    let input = editing.name.trim();
                    let dest = if input.is_empty() {
                        self.root.clone()
                    } else if Path::new(input).is_absolute() {
                        PathBuf::from(input)
                    } else {
                        self.root.join(input)
                    };
                    if report_move(path, &dest, events) {
                        self.expanded.insert(dest);
                    }
                }
            }
        } else if cancel {
            self.editing = None;
        }
    }

    fn show_dir(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        dir: &Path,
        depth: usize,
        events: &mut Vec<TreeEvent>,
    ) {
        let creating_here = self.editing.as_ref().is_some_and(|e| {
            matches!(e.what, Pending::NewFile(_) | Pending::NewDir(_)) && e.what.dir() == dir
        });
        if creating_here {
            self.edit_row(ui, depth, events);
        }

        for (path, is_dir) in fs_ops::list_dir(dir) {
            let renaming = self
                .editing
                .as_ref()
                .is_some_and(|e| matches!(&e.what, Pending::Rename(p) if *p == path));
            if renaming {
                self.edit_row(ui, depth, events);
                if is_dir && self.expanded.contains(&path) {
                    self.show_dir(ui, theme, &path, depth + 1, events);
                }
                continue;
            }

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let indent = 8.0 + depth as f32 * 12.0;
            let is_expanded = self.expanded.contains(&path);
            let is_selected = self.selected.as_deref() == Some(path.as_path());

            let full_width = ui.available_width();
            let (row_rect, resp) = ui
                .allocate_exact_size(egui::vec2(full_width, 22.0), egui::Sense::click_and_drag());

            // Drag source: any row can be picked up and dropped onto a folder.
            resp.dnd_set_drag_payload(path.clone());
            let dragging_any = egui::DragAndDrop::has_any_payload(ui.ctx());
            if resp.hovered() && !dragging_any {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let drop_hover = is_dir
                && resp
                    .dnd_hover_payload::<PathBuf>()
                    .is_some_and(|p| *p != path);

            if drop_hover {
                self.valid_drop_target = true;
            }
            if ui.is_rect_visible(row_rect) {
                // Row states mirror the terminal navigator: drop target wins,
                // then selection, then hover.
                let bg = if drop_hover {
                    Some(color(theme.ui.accent))
                } else if is_selected {
                    Some(color(theme.ui.selected_bg))
                } else if resp.hovered() {
                    Some(color(theme.ui.hover_bg))
                } else {
                    None
                };
                if let Some(bg) = bg {
                    ui.painter()
                        .rect_filled(row_rect, egui::CornerRadius::ZERO, bg);
                }
                let fg = if drop_hover || is_selected {
                    color(theme.ui.fg_bright)
                } else if is_dir {
                    color(theme.ui.fg_dim)
                } else {
                    color(theme.ui.fg)
                };
                let icon_fg = if drop_hover || is_selected {
                    color(theme.ui.fg_bright)
                } else {
                    color(theme.ui.fg_faint)
                };
                let icon_x = row_rect.min.x + indent + 5.0;
                let cy = row_rect.center().y;
                if is_dir {
                    chevron(ui.painter(), egui::pos2(icon_x, cy), is_expanded, icon_fg);
                } else {
                    file_glyph(ui.painter(), egui::pos2(icon_x, cy), icon_fg);
                }
                ui.painter().text(
                    egui::pos2(row_rect.min.x + indent + 16.0, cy),
                    egui::Align2::LEFT_CENTER,
                    &name,
                    egui::FontId::proportional(13.0),
                    fg,
                );
            }

            if is_dir {
                if let Some(src) = resp.dnd_release_payload::<PathBuf>() {
                    report_move(&src, &path, events);
                    self.expanded.insert(path.clone());
                }
            }

            let path_for_menu = path.clone();
            if resp.secondary_clicked() {
                self.selected = Some(path.clone());
            }
            resp.context_menu(|ui| {
                self.context_menu(ui, theme, Some((&path_for_menu, is_dir)), events);
            });

            if resp.clicked() {
                self.selected = Some(path.clone());
                if is_dir {
                    if is_expanded {
                        self.expanded.remove(&path);
                    } else {
                        self.expanded.insert(path.clone());
                    }
                } else {
                    events.push(TreeEvent::Open(path.clone()));
                }
            }
            if is_dir && self.expanded.contains(&path) {
                self.show_dir(ui, theme, &path, depth + 1, events);
            }
        }
    }
}

/// Disclosure triangle, drawn as a shape — the Unicode arrows aren't covered by
/// egui's bundled fonts.
fn chevron(painter: &egui::Painter, center: egui::Pos2, expanded: bool, color: egui::Color32) {
    let r = 3.6;
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

/// A small hollow square — the shape the terminal frontend prints as `▫`.
fn file_glyph(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let half = 3.0;
    painter.rect_stroke(
        egui::Rect::from_center_size(center, egui::vec2(half * 2.0, half * 2.0)),
        egui::CornerRadius::ZERO,
        egui::Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
}

/// Moves `src` into `dest_dir`, reporting either outcome to the app.
fn report_move(src: &Path, dest_dir: &Path, events: &mut Vec<TreeEvent>) -> bool {
    match fs_ops::move_into(src, dest_dir) {
        Ok(to) => {
            events.push(TreeEvent::Moved {
                from: src.to_path_buf(),
                to,
            });
            true
        }
        Err(e) => {
            events.push(TreeEvent::Failed(format!("move failed: {e}")));
            false
        }
    }
}

/// A dropdown row drawn like the terminal one: a `>` marker appears on the
/// highlighted entry, and the label sits in a fixed column beside it.
fn menu_item(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(150.0), 22.0),
        egui::Sense::click(),
    );
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        if hovered {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, color(theme.ui.selected_bg));
        }
        let fg = if hovered {
            color(theme.ui.fg_bright)
        } else {
            color(theme.ui.fg)
        };
        if hovered {
            ui.painter().text(
                egui::pos2(rect.min.x + 6.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                ">",
                egui::FontId::proportional(12.0),
                fg,
            );
        }
        ui.painter().text(
            egui::pos2(rect.min.x + 18.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            fg,
        );
    }
    resp
}

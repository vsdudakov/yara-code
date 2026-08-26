//! Project navigator: file tree with the same context menu the terminal
//! frontend shows (open / new file / new folder / rename / move / delete),
//! plus drag-and-drop moving.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::core::command::Command;
use crate::core::fs_ops;
use crate::core::git::GitState;
use crate::core::theme::Theme;
use crate::gui::theme::{color, glyph, icons};

pub enum TreeEvent {
    Open(PathBuf),
    /// "Add Folder to Project" from the navigator's menu.
    AddFolder,
    /// A command the navigator asked for, for the app to run as if it had
    /// come from a menu — the empty state's two ways to get a folder.
    Run(Command),
    /// "Remove Folder from Project": drops a root, leaving it on disk.
    RemoveFolder(PathBuf),
    /// User picked "Delete"; the app confirms before touching the disk.
    RequestDelete(PathBuf),
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
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
    /// Project folders, primary first; empty when no folder is open. With more
    /// than one, each gets a header row of its own.
    roots: Vec<PathBuf>,
    expanded: HashSet<PathBuf>,
    /// The highlighted row, exactly as in the terminal navigator.
    selected: Option<PathBuf>,
    editing: Option<Editing>,
    /// Set while a drag hovers a folder that would accept the drop.
    valid_drop_target: bool,
    /// What git says about the paths on screen, refreshed each frame.
    git_colors: GitColors,
}

/// A snapshot of the git view's state, so a row can be painted without
/// borrowing the whole panel.
#[derive(Default, Clone)]
struct GitColors {
    changed: Vec<(PathBuf, egui::Color32)>,
}

impl GitColors {
    fn of(&self, path: &Path) -> Option<egui::Color32> {
        self.changed
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, color)| *color)
    }
}

/// The color a changed path is drawn in — VS Code's own reading of the status
/// letters, taken from the theme's terminal palette.
fn git_colors(git: &GitState, theme: &Theme) -> GitColors {
    let Some(dir) = git.dir() else {
        return GitColors::default();
    };
    let mut changed = Vec::new();
    for change in &git.changes {
        let path = dir.join(&change.path);
        let tint = crate::gui::git::letter_color(change.letter(), theme);
        // Every folder above a changed file is tinted too, so a collapsed tree
        // still shows where the changes are.
        let mut at = path.parent();
        while let Some(dir_path) = at {
            if dir_path == dir {
                break;
            }
            changed.push((dir_path.to_path_buf(), tint));
            at = dir_path.parent();
        }
        changed.push((path, tint));
    }
    GitColors { changed }
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        Self::with_roots(vec![root])
    }

    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        let mut tree = Self {
            roots,
            expanded: HashSet::new(),
            selected: None,
            editing: None,
            valid_drop_target: false,
            git_colors: GitColors::default(),
        };
        tree.expand_roots();
        tree
    }

    /// Replaces the folder list, keeping what is expanded and selected.
    pub fn set_roots(&mut self, roots: Vec<PathBuf>) {
        self.roots = roots;
        self.expand_roots();
    }

    /// Added folders start open, so a folder is never added to no effect.
    fn expand_roots(&mut self) {
        if self.roots.len() > 1 {
            for root in self.roots.clone() {
                self.expanded.insert(root);
            }
        }
    }

    fn root(&self) -> Option<&Path> {
        self.roots.first().map(PathBuf::as_path)
    }

    /// The highlighted row, which is what the Rename, Move and Delete
    /// bindings act on.
    pub fn selected(&self) -> Option<PathBuf> {
        self.selected.clone()
    }

    /// Starts the inline rename box on the selected row; false when nothing is
    /// selected.
    pub fn start_rename(&mut self) -> bool {
        let Some(path) = self.selected.clone() else {
            return false;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.start_editing(Pending::Rename(path), name);
        true
    }

    /// Starts the inline "move to" box on the selected row.
    pub fn start_move(&mut self) -> bool {
        let Some(path) = self.selected.clone() else {
            return false;
        };
        self.start_editing(Pending::MoveTo(path), String::new());
        true
    }

    /// Expands every ancestor of `path` and highlights it — used when a jump
    /// (search result, go-to-definition) opens a file from elsewhere.
    pub fn reveal(&mut self, path: &Path) {
        let Some(root) = self.roots.iter().find(|r| path.starts_with(r)).cloned() else {
            return;
        };
        self.expanded.insert(root.clone());
        let mut dir = path.parent();
        while let Some(d) = dir {
            if !d.starts_with(&root) {
                break;
            }
            self.expanded.insert(d.to_path_buf());
            if d == root {
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
        git: &GitState,
        events: &mut Vec<TreeEvent>,
    ) {
        self.git_colors = git_colors(git, theme);
        self.valid_drop_target = false;
        let roots = self.roots.clone();
        match roots.len() {
            0 => {
                self.empty_state(ui, theme, events);
                return;
            }
            1 => self.show_dir(ui, theme, &roots[0], 0, events),
            _ => {
                for root in &roots {
                    self.show_root(ui, theme, root, events);
                }
            }
        }
        let root = roots[0].clone();

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
                egui::Stroke::new(1.0_f32, color(theme.ui.accent_light)),
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
    /// Open | New File, New Folder | Rename, Move To... | Delete | and the
    /// project-level entry, which a folder gets and a file does not.
    fn context_menu(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        target: Option<(&Path, bool)>,
        events: &mut Vec<TreeEvent>,
    ) {
        let fallback = self.root().map(Path::to_path_buf);
        let dir = match (target, &fallback) {
            (Some((path, true)), _) => Some(path.to_path_buf()),
            (Some((path, false)), _) => path.parent().map(Path::to_path_buf),
            (None, root) => root.clone(),
        };
        let is_root = target.is_some_and(|(path, _)| self.roots.iter().any(|r| r == path));

        if let Some((path, false)) = target {
            if menu_item(ui, theme, "Open").clicked() {
                events.push(TreeEvent::Open(path.to_path_buf()));
                ui.close_menu();
            }
            ui.separator();
        }

        if let Some(dir) = dir {
            if menu_item(ui, theme, "New File").clicked() {
                self.start_editing(Pending::NewFile(dir.clone()), String::new());
                ui.close_menu();
            }
            if menu_item(ui, theme, "New Folder").clicked() {
                self.start_editing(Pending::NewDir(dir), String::new());
                ui.close_menu();
            }
            ui.separator();
        }

        match target {
            // A project folder can leave the project; renaming, moving or
            // deleting one on disk is not what the navigator offers.
            Some((path, _)) if is_root => {
                if menu_item(ui, theme, "Remove Folder from Project").clicked() {
                    events.push(TreeEvent::RemoveFolder(path.to_path_buf()));
                    ui.close_menu();
                }
            }
            Some((path, _)) => {
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
                ui.separator();
            }
            None => {}
        }
        // The project-level entry is about folders, and reads the other way
        // round on one the project already holds — that is the Remove above.
        // A file says nothing about the project at all.
        let a_folder = target.is_none() || matches!(target, Some((_, true)));
        if a_folder && !is_root && menu_item(ui, theme, Command::AddFolder.label()).clicked() {
            events.push(TreeEvent::AddFolder);
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
                    let root = self.root().map(Path::to_path_buf);
                    let dest = if Path::new(input).is_absolute() {
                        Some(PathBuf::from(input))
                    } else if input.is_empty() {
                        root
                    } else {
                        root.map(|root| root.join(input))
                    };
                    let Some(dest) = dest else {
                        events.push(TreeEvent::Failed("no folder in the project".into()));
                        return;
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

    /// A project folder's own row: its name, and its contents under it. Only
    /// drawn once a project holds more than one folder.
    fn show_root(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        root: &Path,
        events: &mut Vec<TreeEvent>,
    ) {
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let is_expanded = self.expanded.contains(root);
        let (row_rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
        let drop_hover = resp.dnd_hover_payload::<PathBuf>().is_some();
        if drop_hover {
            self.valid_drop_target = true;
        }
        if ui.is_rect_visible(row_rect) {
            let bg = if drop_hover {
                Some(color(theme.ui.accent))
            } else if resp.hovered() {
                Some(color(theme.ui.hover_bg))
            } else {
                None
            };
            if let Some(bg) = bg {
                ui.painter()
                    .rect_filled(row_rect, egui::CornerRadius::ZERO, bg);
            }
            let fg = color(theme.ui.fg_bright);
            let cy = row_rect.center().y;
            let icons = icons();
            glyph(
                ui.painter(),
                egui::pos2(row_rect.min.x + 13.0, cy),
                if is_expanded {
                    icons.dir_open
                } else {
                    icons.dir_closed
                },
                12.0,
                fg,
            );
            ui.painter().text(
                egui::pos2(row_rect.min.x + 24.0, cy),
                egui::Align2::LEFT_CENTER,
                name.to_uppercase(),
                egui::FontId::proportional(11.5),
                fg,
            );
        }
        if let Some(src) = resp.dnd_release_payload::<PathBuf>() {
            report_move(&src, root, events);
        }
        let root_for_menu = root.to_path_buf();
        resp.context_menu(|ui| {
            self.context_menu(ui, theme, Some((&root_for_menu, true)), events);
        });
        if resp.clicked() {
            if is_expanded {
                self.expanded.remove(root);
            } else {
                self.expanded.insert(root.to_path_buf());
            }
        }
        if self.expanded.contains(root) {
            self.show_dir(ui, theme, root, 1, events);
        }
    }

    /// What the navigator shows with no folder open: how to get one. The
    /// recent list leads, because the folder wanted next is nearly always one
    /// of the folders opened before; adding a folder to a project that has
    /// none is the same thing as opening it, and is offered as that.
    fn empty_state(&mut self, ui: &mut egui::Ui, theme: &Theme, events: &mut Vec<TreeEvent>) {
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No folder in the project")
                    .color(color(theme.ui.fg_dim))
                    .size(12.0),
            );
            ui.add_space(8.0);
            for command in [Command::OpenRecent, Command::OpenFolder] {
                if ui
                    .button(
                        egui::RichText::new(command.label())
                            .color(color(theme.ui.fg))
                            .size(12.0),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    events.push(TreeEvent::Run(command));
                }
                ui.add_space(4.0);
            }
        });
        let remaining = ui.available_height().max(20.0);
        let (_, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), remaining),
            egui::Sense::click(),
        );
        resp.context_menu(|ui| {
            self.context_menu(ui, theme, None, events);
        });
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
            let (row_rect, resp) =
                ui.allocate_exact_size(egui::vec2(full_width, 22.0), egui::Sense::click_and_drag());

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
                } else if let Some(tint) = self.git_colors.of(&path) {
                    tint
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
                let icons = icons();
                let mark = if is_dir {
                    if is_expanded {
                        icons.dir_open
                    } else {
                        icons.dir_closed
                    }
                } else {
                    icons.file
                };
                glyph(ui.painter(), egui::pos2(icon_x, cy), mark, 12.0, icon_fg);
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
pub fn menu_item(ui: &mut egui::Ui, theme: &Theme, label: &str) -> egui::Response {
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
                icons().menu_marker,
                egui::FontId::monospace(12.0),
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

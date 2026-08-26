use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::core::clipboard as core_clipboard;
use crate::core::command::{Command, ALL, FILE_MENU, HELP_MENU, START_PAGE, VIEW_MENU};
use crate::core::fs_ops;
use crate::core::git::{Blame, Change, LineState};
/// Placeholder for an empty input; the heading above already names the field.
/// Nothing stands in for an empty field: the label beside it already
/// names it, and the caret says it is waiting for text.
const FIELD_HINT: &str = "";
use crate::core::project::Project;
use crate::core::search::{self, Candidate, Field as SearchField, Search};
use crate::core::settings::Settings;
use crate::core::theme::{self as core_theme, Theme};
use crate::gui::diff::{DiffEvent, DiffView};
use crate::gui::editor::Editor;
use crate::gui::file_tree::{FileTree, TreeEvent};
use crate::gui::git::GitPanel;
use crate::gui::highlight;
use crate::gui::keys;
use crate::gui::terminal::Terminal;
use crate::gui::theme::{ansi_color, color, glyph, icons};

#[derive(PartialEq, Clone, Copy)]
enum SidebarView {
    Files,
    Search,
    Git,
}

struct GotoPicker {
    word: String,
    candidates: Vec<Candidate>,
    is_definition: bool,
}

pub struct App {
    project: Project,
    tree: FileTree,
    editor: Editor,
    terminal: Terminal,
    show_terminal: bool,
    show_sidebar: bool,
    sidebar_view: SidebarView,
    search: Search,
    git: GitPanel,
    pending_delete: Option<PathBuf>,
    /// The project-wide Replace All is waiting for a yes.
    pending_replace_all: bool,
    goto_picker: Option<GotoPicker>,
    /// The command palette, the file finder or go to line, while one is up.
    picker: Option<Picker>,
    /// Locations to return to with ⌃- after a goto jump.
    history: Vec<(PathBuf, usize)>,
    themes: Vec<Theme>,
    theme_index: usize,
    show_theme_picker: bool,
    /// Last file-operation error, shown in the status bar.
    status: Option<String>,
    /// Set by Quit (or the window's close button) while a dirty buffer is
    /// being asked about; cleared by Cancel.
    quit_after_close: bool,
    /// Close All Tabs is working its way through the dirty buffers, asking
    /// about each one as it reaches it; cleared by Cancel.
    closing_all: bool,
    /// The update check, and the release it found.
    updates: crate::core::update::Checker,
    update: Option<crate::core::update::Release>,
    /// The install of that release, and how far it has got.
    installs: crate::core::update::Installer,
    installing: Option<crate::core::update::Progress>,
    /// Who last touched the line the cursor is on.
    blame: Option<Blame>,
    /// What `blame` was read for.
    blame_key: Option<(PathBuf, usize)>,
    /// Changed lines of the file in front, for the gutter marks.
    git_lines: BTreeMap<usize, LineState>,
    /// What `git_lines` was read for, so it is re-read only when it must be.
    git_lines_key: Option<(PathBuf, usize)>,
    settings: Settings,
    /// When the settings files were last written, as of the last look; `None`
    /// before the first look.
    settings_stamp: Option<Vec<Option<std::time::SystemTime>>>,
    /// When the files were last looked at.
    settings_polled: std::time::Instant,
    show_recent: bool,
    /// The key bindings overlay, opened from Help.
    show_help: bool,
    /// The indentation picker, opened from View or the status bar.
    show_indent_picker: bool,
    recent_selected: usize,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        root: Option<PathBuf>,
        file: Option<PathBuf>,
    ) -> Self {
        let mut app = Self::with_context(&cc.egui_ctx, root);
        if let Some(file) = file {
            app.open_file(file);
        }
        app
    }

    /// The editor over a bare egui context — what `new` does once eframe has
    /// made one, and what an end-to-end test can make for itself.
    pub fn with_context(ctx: &egui::Context, root: Option<PathBuf>) -> Self {
        let themes = core_theme::load_all();
        let (mut settings, settings_error) = Settings::load_for(root.as_deref());
        if let Some(root) = &root {
            settings.push_recent(root);
            let _ = settings.save();
        }
        let theme_index = themes
            .iter()
            .position(|t| t.name == settings.theme)
            .unwrap_or(0);
        let theme = themes.get(theme_index).cloned().unwrap_or_default();
        crate::gui::fonts::install(ctx);
        crate::gui::theme::apply(ctx, &theme, settings.font_size);
        highlight::set_theme(&theme);
        Self {
            tree: FileTree::with_roots(root.iter().cloned().collect()),
            project: Project::opened(root),
            editor: Editor::default(),
            terminal: Terminal::default(),
            show_terminal: settings.show_terminal,
            show_sidebar: settings.show_sidebar,
            sidebar_view: SidebarView::Files,
            search: Search::default(),
            git: GitPanel::default(),
            pending_delete: None,
            pending_replace_all: false,
            goto_picker: None,
            picker: None,
            history: Vec::new(),
            themes,
            theme_index,
            show_theme_picker: false,
            status: settings_error,
            quit_after_close: false,
            closing_all: false,
            updates: crate::core::update::Checker::default(),
            update: None,
            installs: crate::core::update::Installer::default(),
            installing: None,
            blame: None,
            blame_key: None,
            git_lines: BTreeMap::new(),
            git_lines_key: None,
            settings,
            settings_stamp: None,
            settings_polled: std::time::Instant::now(),
            show_recent: false,
            show_help: false,
            show_indent_picker: false,
            recent_selected: 0,
        }
    }

    fn theme(&self) -> &Theme {
        &self.themes[self.theme_index]
    }

    fn set_theme(&mut self, ctx: &egui::Context, index: usize) {
        if index >= self.themes.len() {
            return;
        }
        self.theme_index = index;
        let theme = self.themes[index].clone();
        // What the picker chose is what the next save writes; the sidebar and
        // terminal flags are synced the same way, in `remember_layout`.
        self.settings.theme = theme.name.clone();
        crate::gui::theme::apply(ctx, &theme, self.settings.font_size);
        highlight::set_theme(&theme);
        let _ = self.settings.save();
    }

    /// Runs a bound command, whether it came from a key chord or the menu bar.
    fn execute(&mut self, ctx: &egui::Context, command: Command) {
        self.status = None;
        match command {
            Command::OpenFile => {
                if let Some(path) = self.pick_file("Open File") {
                    self.open_path(path);
                }
            }
            Command::OpenFolder => {
                if let Some(path) = self.pick_folder("Open Folder as Project") {
                    self.set_root(path);
                }
            }
            Command::AddFolder => {
                if let Some(path) = self.pick_folder("Add Folder to Project") {
                    self.add_folder(path);
                }
            }
            Command::NewFile | Command::SaveAs => {
                if command == Command::SaveAs && self.editor.buffers.is_empty() {
                    self.status = Some("nothing to save".into());
                    return;
                }
                let new_file = command == Command::NewFile;
                let title = if new_file { "New File" } else { "Save As" };
                match self.pick_save_path(title, !new_file) {
                    Some(path) if new_file => self.create_file(path),
                    Some(path) => self.save_as(path),
                    None => {}
                }
            }
            Command::NewFolder => {
                if let Some(path) = self.pick_save_path("New Folder", false) {
                    let (dir, name) = match path.parent().zip(path.file_name()) {
                        Some((dir, name)) => {
                            (dir.to_path_buf(), name.to_string_lossy().into_owned())
                        }
                        None => (self.project.root_or_cwd(), path.display().to_string()),
                    };
                    match fs_ops::create_dir(&dir, &name) {
                        Ok(created) => self.tree.reveal(&created),
                        Err(e) => self.status = Some(format!("create failed: {e}")),
                    }
                }
            }
            Command::Rename => {
                if !self.tree.start_rename() {
                    self.status = Some("select a file in the navigator first".into());
                }
            }
            Command::MoveTo => {
                if !self.tree.start_move() {
                    self.status = Some("select a file in the navigator first".into());
                }
            }
            Command::Delete => match self.tree.selected() {
                Some(path) => self.pending_delete = Some(path),
                None => self.status = Some("select a file in the navigator first".into()),
            },
            Command::FindNext => self.editor.find_step(1),
            Command::FindPrev => self.editor.find_step(-1),
            Command::ReplaceAll => {
                let replaced = self.editor.find_replace_all();
                self.status = Some(format!(
                    "replaced {}",
                    crate::core::count(replaced, "occurrence")
                ));
            }
            Command::PickRepository | Command::PickWorktree => {
                self.sidebar_view = SidebarView::Git;
                self.show_sidebar = true;
            }
            Command::CheckForUpdates => {
                self.status = Some(format!(
                    "checking for updates (this is {})…",
                    crate::core::update::CURRENT
                ));
                let ctx = ctx.clone();
                self.updates.start(move || ctx.request_repaint());
            }
            Command::InstallUpdate => match self.update.clone() {
                // The download is minutes of work on a slow line, so it goes
                // to a thread and reports back into the status bar.
                Some(release) if !self.installs.is_running() => {
                    self.status = Some(format!(
                        "installing {} — starting the download",
                        release.tag
                    ));
                    let ctx = ctx.clone();
                    self.installs.start(&release, move || ctx.request_repaint());
                }
                Some(_) => {}
                None => self.status = Some("nothing to install; check for updates first".into()),
            },
            Command::Documentation => {
                let opened = crate::core::open_url(crate::core::DOCUMENTATION);
                self.status = Some(if opened {
                    crate::core::DOCUMENTATION.to_string()
                } else {
                    format!(
                        "open {} to read the documentation",
                        crate::core::DOCUMENTATION
                    )
                });
            }
            // The window's menus open from the bar itself.
            Command::NextPane
            | Command::PrevPane
            | Command::ContextMenu
            | Command::FileMenu
            | Command::ViewMenu
            | Command::HelpMenu => {}
            Command::IndentPicker => self.show_indent_picker = true,
            Command::TogglePreview => {
                if let Err(message) = self.editor.toggle_preview() {
                    self.status = Some(message);
                }
            }
            Command::Undo | Command::Redo => {
                let back = command == Command::Undo;
                if !self.editor.step_history(back) {
                    self.status = Some(if back {
                        "nothing to undo".into()
                    } else {
                        "nothing to redo".into()
                    });
                }
            }
            Command::OpenRecent => {
                if self.settings.recent_projects.is_empty() {
                    self.status = Some("no recent projects yet".into());
                } else {
                    self.recent_selected = 0;
                    self.show_recent = true;
                }
            }
            Command::Save => match self.editor.buffers.try_save_active() {
                Ok(()) => self.after_save(ctx),
                Err(e) => self.status = Some(e.to_string()),
            },
            Command::SaveAll => {
                let previous = self.editor.buffers.active;
                let mut saved = 0;
                let mut failed = Vec::new();
                for i in 0..self.editor.buffers.list.len() {
                    self.editor.buffers.active = i;
                    if !self.editor.buffers.list[i].modified() {
                        continue;
                    }
                    if self.editor.buffers.save_active() {
                        saved += 1;
                    } else {
                        failed.push(self.editor.buffers.list[i].name());
                    }
                }
                self.editor.buffers.active = previous;
                self.after_save(ctx);
                self.status = Some(crate::core::save_all_report(saved, &failed));
            }
            Command::Settings => match self.settings.ensure_file() {
                Ok(path) => {
                    self.open_file(path);
                    self.status = Some("editing settings — save to apply".into());
                }
                Err(e) => self.status = Some(format!("cannot open settings: {e}")),
            },
            Command::CloseTab => {
                // A diff and a preview are tabs like any other, so what closes
                // is whatever is in front, not the file hidden behind it.
                if let Some(index) = self.editor.active_preview {
                    self.editor.close_preview(index);
                } else if let Some(index) = self.editor.active_diff {
                    self.editor.close_diff(index);
                } else {
                    let active = self.editor.buffers.active;
                    self.editor.request_close(active);
                }
            }
            Command::CloseAllTabs => self.close_all_tabs(),
            Command::Quit => self.request_quit(ctx),
            Command::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            Command::ToggleTerminal => self.show_terminal = !self.show_terminal,
            Command::NewTerminal => {
                self.show_terminal = true;
                self.terminal.open(&self.project.root_or_cwd(), ctx);
            }
            Command::CloseTerminal => {
                self.terminal.sessions.close_active();
                if self.terminal.sessions.is_empty() {
                    self.show_terminal = false;
                }
            }
            Command::FindInFile => {
                if self.editor.buffers.is_empty() {
                    self.status = Some("open a file first".into());
                } else {
                    self.editor.open_find();
                }
            }
            Command::FocusSearch => {
                self.show_sidebar = true;
                self.sidebar_view = SidebarView::Search;
                self.search.focus_pending = true;
            }
            Command::FocusFiles => {
                self.show_sidebar = true;
                self.sidebar_view = SidebarView::Files;
            }
            Command::FocusGit => {
                self.show_sidebar = true;
                self.sidebar_view = SidebarView::Git;
            }
            Command::ThemePicker => self.show_theme_picker = true,
            Command::ToggleFold => self.editor.toggle_fold_at_cursor(),
            Command::FoldAll => self.editor.fold_all(),
            Command::UnfoldAll => self.editor.unfold_all(),
            Command::GoBack => {
                if let Some((path, line)) = self.history.pop() {
                    self.open_file(path.clone());
                    self.editor.pending_jump = Some((path, line));
                }
            }
            Command::NextTab => {
                if !self.editor.buffers.is_empty() {
                    let n = self.editor.buffers.list.len();
                    self.editor.buffers.active = (self.editor.buffers.active + 1) % n;
                }
            }
            Command::PrevTab => {
                if !self.editor.buffers.is_empty() {
                    let n = self.editor.buffers.list.len();
                    self.editor.buffers.active = (self.editor.buffers.active + n - 1) % n;
                }
            }
            // Selection and clipboard belong to the text widget itself, and
            // these three are mouse- or menu-driven in this frontend.
            // The clipboard belongs to the text widget, go-to-definition to
            // the mouse, and the bindings overlay is the Help menu itself.
            Command::GotoDefinition => {
                if let Some(word) = self.editor.word_at_cursor() {
                    self.editor.goto_request = Some(word);
                }
            }
            Command::SelectAll | Command::Copy | Command::Cut => {}
            // Text pasting belongs to whichever widget is being typed in, and
            // egui does it. An *image* has nowhere to go in a text widget, so
            // it is written to a file and its path is what the terminal gets
            // — the same trade the terminal frontend makes, because a program
            // in the shell can open a path and cannot open a bitmap.
            Command::Paste => {
                if self.terminal.focused {
                    if let Some(path) = core_clipboard::system_image() {
                        let quoted = core_clipboard::shell_quoted(&path);
                        if let Some(pty) = self.terminal.sessions.active_mut() {
                            pty.paste(&quoted);
                            self.status = Some(format!("pasted {}", path.display()));
                        }
                    }
                }
            }
            Command::Help => self.show_help = true,
            Command::CommandPalette => self.picker = Some(Picker::new(PickerKind::Commands)),
            Command::QuickOpen => {
                let mut picker = Picker::new(PickerKind::Files);
                picker.files = search::project_files(self.project.roots(), 5000);
                self.picker = Some(picker);
            }
            Command::GotoLine => {
                if self.editor.buffers.is_empty() {
                    self.status = Some("open a file first".into());
                } else {
                    self.picker = Some(Picker::new(PickerKind::Line));
                }
            }
        }
    }

    /// Re-reads settings when the settings file itself was just saved.
    fn after_save(&mut self, ctx: &egui::Context) {
        // A save changes the git status; show it right away, not on the timer.
        self.git.invalidate();
        let root = self.project.root().map(Path::to_path_buf);
        let is_settings = self
            .editor
            .buffers
            .active()
            .is_some_and(|buf| Settings::is_settings_file(&buf.path, root.as_deref()));
        if !is_settings {
            self.status = Some("saved".into());
            return;
        }
        self.reload_settings(ctx);
    }

    /// Once a second, looks whether a settings file was written by something
    /// other than this window — the terminal frontend, a script, a hand edit
    /// — and applies it, so a change lands without a restart either way.
    fn poll_settings(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        if self.settings_polled.elapsed() < std::time::Duration::from_secs(1) {
            return;
        }
        self.settings_polled = std::time::Instant::now();
        self.poll_disk();
        let stamp = Settings::stamp(self.project.root());
        match &self.settings_stamp {
            // The first look only remembers what is there.
            None => self.settings_stamp = Some(stamp),
            Some(known) if *known != stamp => self.reload_settings(ctx),
            Some(_) => {}
        }
    }

    /// Reads the settings files again and puts everything they say into
    /// effect: the theme, the font size it is applied with, which panels are
    /// open, and the bindings.
    fn reload_settings(&mut self, ctx: &egui::Context) {
        let root = self.project.root().map(Path::to_path_buf);
        self.settings_stamp = Some(Settings::stamp(root.as_deref()));
        let (settings, error) = Settings::load_for(root.as_deref());
        let recent = std::mem::take(&mut self.settings.recent_projects);
        self.settings = settings;
        self.settings.recent_projects = recent;
        self.show_sidebar = self.settings.show_sidebar;
        self.show_terminal = self.settings.show_terminal;
        let index = self
            .themes
            .iter()
            .position(|t| t.name == self.settings.theme)
            .unwrap_or(self.theme_index);
        self.theme_index = index;
        let theme = self.themes[index].clone();
        crate::gui::theme::apply(ctx, &theme, self.settings.font_size);
        highlight::set_theme(&theme);
        self.status = Some(error.unwrap_or_else(|| "settings applied".to_string()));
    }

    /// The start page: what fills the editor while nothing is open — the
    /// name, the folder in play, and the keys that are actually bound.
    fn start_page(&self, ui: &mut egui::Ui, theme: &Theme) -> Option<Command> {
        let chord = |command| self.settings.gui_chord(command).map(|c| c.to_string());
        let project = match self.project.root() {
            Some(root) => Project::name_of(root),
            None => "no folder in the project".to_string(),
        };

        // The groups are measured before anything is drawn, so the block can be
        // centred as a whole instead of each column finding its own edge.
        let chord_font = egui::FontId::monospace(11.5);
        let label_font = egui::FontId::proportional(11.5);
        let ctx = ui.ctx().clone();
        let width_of = move |text: &str, font: &egui::FontId| {
            ctx.fonts(|f| {
                f.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            })
        };

        struct Group {
            name: String,
            rows: Vec<(String, &'static str, Command)>,
            chords: f32,
            width: f32,
        }
        const KEY_GAP: f32 = 16.0;
        const COLUMN_GAP: f32 = 44.0;
        let groups: Vec<Group> = START_PAGE
            .iter()
            .filter_map(|(name, commands)| {
                let rows: Vec<(String, &'static str, Command)> = commands
                    .iter()
                    .filter_map(|command| Some((chord(*command)?, command.label(), *command)))
                    .collect();
                if rows.is_empty() {
                    return None;
                }
                let chords = rows
                    .iter()
                    .map(|(chord, _, _)| width_of(chord, &chord_font))
                    .fold(0.0, f32::max);
                let labels = rows
                    .iter()
                    .map(|(_, label, _)| width_of(label, &label_font))
                    .fold(0.0, f32::max);
                Some(Group {
                    name: name.to_uppercase(),
                    rows,
                    chords,
                    width: chords + KEY_GAP + labels,
                })
            })
            .collect();
        if groups.is_empty() {
            return None;
        }
        let mut clicked = None;
        let block =
            groups.iter().map(|g| g.width).sum::<f32>() + COLUMN_GAP * (groups.len() - 1) as f32;
        let tallest = groups.iter().map(|g| g.rows.len()).max().unwrap_or(0);

        const ROW_HEIGHT: f32 = 21.0;
        const HEAD_HEIGHT: f32 = 96.0; // title, project name and the gap under
        let content = HEAD_HEIGHT + 18.0 + tallest as f32 * ROW_HEIGHT;
        let top = ((ui.available_height() - content) / 2.0).clamp(12.0, 160.0);

        ui.vertical_centered(|ui| {
            ui.add_space(top);
            ui.label(
                egui::RichText::new("YARA CODE")
                    .color(color(theme.ui.accent_light))
                    .size(30.0)
                    .strong(),
            );
            ui.label(
                egui::RichText::new(project)
                    .color(color(theme.ui.fg_faint))
                    .size(12.0),
            );
        });
        ui.add_space(26.0);

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.add_space(((ui.available_width() - block) / 2.0).max(0.0));
            for (i, group) in groups.iter().enumerate() {
                ui.allocate_ui_with_layout(
                    egui::vec2(group.width, tallest as f32 * ROW_HEIGHT + 22.0),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.spacing_mut().item_spacing.y = 5.0;
                        ui.label(
                            egui::RichText::new(&group.name)
                                .color(color(theme.ui.fg_dim))
                                .size(10.0)
                                .strong(),
                        );
                        for (key, label, command) in &group.rows {
                            // Each row is the action it names: a click runs
                            // it, so the page is a menu, not a manual.
                            let row = ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.label(
                                    egui::RichText::new(key)
                                        .color(color(theme.ui.fg_bright))
                                        .font(chord_font.clone()),
                                );
                                // The labels line up in a column of their own:
                                // pad each chord out to the widest one.
                                let used = width_of(key, &chord_font);
                                ui.add_space(group.chords - used + KEY_GAP);
                                let hovered = ui.rect_contains_pointer(ui.max_rect());
                                ui.label(
                                    egui::RichText::new(*label)
                                        .color(color(if hovered {
                                            theme.ui.fg
                                        } else {
                                            theme.ui.fg_faint
                                        }))
                                        .font(label_font.clone()),
                                );
                            });
                            let response = row.response.interact(egui::Sense::click());
                            if response.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if response.clicked() {
                                clicked = Some(*command);
                            }
                        }
                    },
                );
                if i + 1 < groups.len() {
                    ui.add_space(COLUMN_GAP);
                }
            }
        });
        clicked
    }

    // ----- native dialogs --------------------------------------------------

    /// Where a dialog should start: the folder the navigator is pointing at,
    /// falling back to the working directory when no folder is open.
    fn dialog_dir(&self) -> PathBuf {
        self.project.root_or_cwd()
    }

    fn pick_file(&self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .set_directory(self.dialog_dir())
            .pick_file()
    }

    fn pick_folder(&self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .set_directory(self.dialog_dir())
            .pick_folder()
    }

    /// The system's Save panel: used for both "Save As" and "New File", which
    /// differ only in what is written to the chosen path. Save As starts on the
    /// current file's name; a new file starts on a blank one.
    fn pick_save_path(&self, title: &str, suggest_current: bool) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new()
            .set_title(title)
            .set_directory(self.dialog_dir());
        let current = self
            .editor
            .buffers
            .active()
            .and_then(|buf| buf.path.file_name())
            .map(|name| name.to_string_lossy().into_owned());
        if let Some(name) = current.filter(|_| suggest_current) {
            dialog = dialog.set_file_name(name);
        }
        dialog.save_file()
    }

    // ----- what the dialogs feed -------------------------------------------

    fn open_path(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.set_root(path);
        } else if path.is_file() {
            self.tree.reveal(&path);
            self.open_file(path);
        } else {
            self.status = Some(format!("no such file: {}", path.display()));
        }
    }

    fn create_file(&mut self, path: PathBuf) {
        let (dir, name) = match path.parent().zip(path.file_name()) {
            Some((dir, name)) => (dir.to_path_buf(), name.to_string_lossy().into_owned()),
            None => (self.project.root_or_cwd(), path.display().to_string()),
        };
        // The Save panel offers to replace an existing file; opening it is the
        // only sane reading of "new file" on a path that already exists.
        if path.is_file() {
            self.tree.reveal(&path);
            self.open_file(path);
            return;
        }
        match fs_ops::create_file(&dir, &name) {
            Ok(created) => {
                self.tree.reveal(&created);
                self.open_file(created);
            }
            Err(e) => self.status = Some(format!("create failed: {e}")),
        }
    }

    fn save_as(&mut self, path: PathBuf) {
        if let Some(buf) = self.editor.buffers.active_mut() {
            buf.path = path.clone();
            buf.extension = path
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        if self.editor.buffers.save_active() {
            self.status = Some(format!("saved as {}", self.project.display(&path)));
            self.tree.reveal(&path);
        } else {
            self.status = Some(format!("could not write {}", path.display()));
        }
    }

    /// Keeps the gutter marks in step with the file in front. Reading them is
    /// a `git diff` of one file, so it is done only when something moved.
    fn refresh_git_lines(&mut self) {
        let Some(buf) = self.editor.buffers.active() else {
            self.git_lines.clear();
            self.git_lines_key = None;
            return;
        };
        let key = (buf.path.clone(), buf.text.len());
        if self.git_lines_key.as_ref() == Some(&key) {
            return;
        }
        self.git_lines_key = Some(key.clone());
        self.git_lines = match self.git.state.dir() {
            Some(dir) => match key.0.strip_prefix(&dir) {
                Ok(relative) => crate::core::git::changed_lines(&dir, &relative.to_string_lossy()),
                Err(_) => BTreeMap::new(),
            },
            None => BTreeMap::new(),
        };
    }

    /// A project may carry settings of its own; they take effect the moment
    /// the project opens, without disturbing what the user has saved.
    fn reload_project_settings(&mut self, root: Option<&Path>) {
        let (settings, complaint) = Settings::load_for(root);
        let recent = std::mem::take(&mut self.settings.recent_projects);
        self.settings = settings;
        self.settings.recent_projects = recent;
        if let Some(message) = complaint {
            self.status = Some(message);
        }
    }

    /// Notices files edited outside the window — a formatter, a script,
    /// another editor — and says what happened to them.
    fn poll_disk(&mut self) {
        for event in self.editor.buffers.poll_disk() {
            self.status = Some(match event {
                crate::core::buffer::DiskEvent::Reloaded(name) => {
                    format!("{name} changed on disk — reloaded")
                }
                crate::core::buffer::DiskEvent::ChangedOnDisk(name) => {
                    format!("{name} changed on disk — saving will overwrite it")
                }
            });
        }
    }

    /// Re-reads the file in front from disk, replacing the buffer's text.
    /// What an outside edit — a formatter, a script, another editor — asks
    /// for; the undo history is kept so the reload itself can be undone.
    pub fn reload_active_from_disk(&mut self) {
        let Some(buf) = self.editor.buffers.active_mut() else {
            return;
        };
        if let Ok(text) = std::fs::read_to_string(&buf.path) {
            let cursor = 0;
            buf.record(crate::core::history::EditKind::Bulk, cursor);
            buf.text = text.clone();
            buf.saved_text = text;
        }
    }

    /// Closes every tab. The clean ones go at once; the first with unsaved
    /// work is asked about, and answering it comes back here for the rest.
    /// Diffs and previews are tabs too, and they have nothing to save.
    fn close_all_tabs(&mut self) {
        self.editor.diffs.clear();
        self.editor.active_diff = None;
        self.editor.previews.clear();
        self.editor.active_preview = None;
        while let Some(index) = self.editor.buffers.list.iter().position(|b| !b.modified()) {
            self.editor.close(index);
        }
        match self.editor.buffers.list.iter().position(|b| b.modified()) {
            Some(index) => {
                self.closing_all = true;
                self.editor.pending_close = Some(index);
            }
            None => self.closing_all = false,
        }
    }

    /// Quitting with unsaved work asks about each dirty buffer in turn, the
    /// way closing a tab does, and only then closes the window.
    fn request_quit(&mut self, ctx: &egui::Context) {
        match self.editor.buffers.list.iter().position(|b| b.modified()) {
            Some(index) => {
                self.quit_after_close = true;
                self.editor.pending_close = Some(index);
            }
            None => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    /// Whether anything is layered over the editor — a picker, a
    /// confirmation, the bindings overlay — and so owns the next Escape.
    fn modal_open(&self) -> bool {
        self.show_theme_picker
            || self.show_recent
            || self.show_help
            || self.show_indent_picker
            || self.goto_picker.is_some()
            || self.picker.is_some()
            || self.pending_delete.is_some()
            || self.pending_replace_all
            || self.editor.pending_close.is_some()
    }

    /// Picks up an update check that has finished.
    fn collect_update(&mut self) {
        let Some(answer) = self.updates.take() else {
            // Nothing yet: keep the marker turning so the wait looks alive.
            if self.updates.is_running() {
                self.status = Some(format!(
                    "{} checking for updates (this is {})…",
                    crate::core::update::spinner(self.updates.tick()),
                    crate::core::update::CURRENT
                ));
            }
            return;
        };
        match answer {
            Ok(release) if release.is_newer() => {
                self.status = Some(format!(
                    "{} is out — Help → Install Update ({})",
                    release.tag,
                    crate::core::update::how_to_update()
                ));
                self.update = Some(release);
            }
            Ok(_) => {
                self.status = Some(format!(
                    "Yara Code {} is the latest",
                    crate::core::update::CURRENT
                ))
            }
            Err(message) => self.status = Some(format!("update check failed: {message}")),
        }
    }

    /// Shows how an install is getting on. The work happens on a thread, so
    /// this reads the newest step and writes it into the status bar, with a
    /// turning marker in front of it while there is more to come.
    fn collect_install(&mut self) {
        if let Some(progress) = self.installs.poll() {
            if progress.is_finished() {
                if matches!(progress, crate::core::update::Progress::Done(_)) {
                    self.update = None;
                }
                self.status = Some(progress.line(self.installs.tag()));
                self.installing = None;
                return;
            }
            self.installing = Some(progress);
        }
        if let Some(progress) = &self.installing {
            self.status = Some(format!(
                "{} {}",
                crate::core::update::spinner(self.installs.tick()),
                progress.line(self.installs.tag())
            ));
        }
    }

    /// Reads who last touched the cursor's line, when the cursor moves to
    /// another line or another file.
    fn refresh_blame(&mut self) {
        let Some((buf, (line, _))) = self.editor.buffers.active().zip(self.editor.cursor) else {
            self.blame = None;
            self.blame_key = None;
            return;
        };
        let key = (buf.path.clone(), line);
        if self.blame_key.as_ref() == Some(&key) {
            return;
        }
        self.blame_key = Some(key.clone());
        self.blame = self.git.state.dir().and_then(|dir| {
            let relative = key.0.strip_prefix(&dir).ok()?;
            crate::core::git::blame(&dir, &relative.to_string_lossy(), line)
        });
    }

    /// Opens the two-pane diff of a changed file as a tab of its own.
    fn show_diff(&mut self, change: &Change) {
        let Some(dir) = self.git.state.dir() else {
            self.status = Some("not a git repository".into());
            return;
        };
        let rows = crate::core::git::diff(&dir, change);
        self.editor
            .open_diff(DiffView::new(change.path.clone(), rows));
    }

    // ----- project folders -------------------------------------------------

    fn set_root(&mut self, root: PathBuf) {
        let root = self.project.set_root(root);
        self.reload_project_settings(Some(&root));
        self.tree.set_roots(self.project.roots().to_vec());
        self.search = Search::default();
        self.terminal.sessions.clear();
        self.settings.push_recent(&root);
        let _ = self.settings.save();
    }

    fn add_folder(&mut self, path: PathBuf) {
        match self.project.add(path) {
            Ok(added) => {
                self.tree.set_roots(self.project.roots().to_vec());
                self.tree.reveal(&added);
                self.settings.push_recent(&added);
                let _ = self.settings.save();
                self.search.run(self.project.roots());
            }
            Err(message) => self.status = Some(message),
        }
    }

    fn remove_folder(&mut self, path: &Path) {
        match self.project.remove(path) {
            Ok(()) => {
                self.tree.set_roots(self.project.roots().to_vec());
                self.search.run(self.project.roots());
            }
            Err(message) => self.status = Some(message),
        }
    }

    fn jump_to(&mut self, path: PathBuf, line: usize) {
        if let Some(loc) = self.editor.location() {
            if loc != (path.clone(), line) {
                self.history.push(loc);
                if self.history.len() > 100 {
                    self.history.remove(0);
                }
            }
        }
        self.open_file(path.clone());
        self.tree.reveal(&path);
        self.editor.pending_jump = Some((path, line));
    }

    fn handle_tree_events(&mut self, events: Vec<TreeEvent>) {
        for event in events {
            match event {
                TreeEvent::Open(path) => {
                    self.tree.reveal(&path);
                    self.open_file(path);
                }
                TreeEvent::RequestDelete(path) => self.pending_delete = Some(path),
                TreeEvent::Moved { from, to } => {
                    self.editor.buffers.retarget(&from, &to);
                    self.status = None;
                }
                TreeEvent::Failed(message) => self.status = Some(message),
                TreeEvent::AddFolder => {
                    if let Some(path) = self.pick_folder("Add Folder to Project") {
                        self.add_folder(path);
                    }
                }
                TreeEvent::RemoveFolder(path) => self.remove_folder(&path),
            }
        }
    }

    /// Opens a file in the editor, reporting the one thing that can go wrong.
    fn open_file(&mut self, path: PathBuf) {
        if !self.editor.open(path.clone()) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.status = Some(format!("cannot open {name} as text"));
        }
    }

    fn handle_goto(&mut self) {
        let Some(word) = self.editor.goto_request.take() else {
            return;
        };
        let current = self.editor.location().map(|(p, _)| p);
        let mut candidates = search::find_definitions(self.project.roots(), &word);
        let is_definition = !candidates.is_empty();
        if !is_definition {
            candidates = search::find_references(self.project.roots(), &word, 100);
        }
        candidates.sort_by_key(|c| (Some(&c.path) != current.as_ref(), c.path.clone(), c.line));
        match candidates.len() {
            0 => {}
            1 if is_definition => {
                let c = &candidates[0];
                self.jump_to(c.path.clone(), c.line);
            }
            _ => {
                self.goto_picker = Some(GotoPicker {
                    word,
                    candidates,
                    is_definition,
                });
            }
        }
    }

    /// The three pickers share one shape: a field to type in, and under it
    /// the rows that still match, best first. Enter takes the highlighted
    /// row, the arrows move it, Escape leaves.
    fn picker_modal(&mut self, ctx: &egui::Context) {
        let theme = self.theme().clone();
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let title = match picker.kind {
            PickerKind::Commands => "Command Palette",
            PickerKind::Files => "Go to File",
            PickerKind::Line => "Go to Line",
        };
        let rows: Vec<(usize, String, String)> = match picker.kind {
            PickerKind::Commands => {
                let settings = &self.settings;
                crate::core::fuzzy::rank(&picker.query, ALL.iter().map(|c| c.label()))
                    .into_iter()
                    .map(|(i, label)| {
                        let chord = settings
                            .gui_chord(ALL[i])
                            .map(|c| c.to_string())
                            .unwrap_or_default();
                        (i, label.to_string(), chord)
                    })
                    .collect()
            }
            PickerKind::Files => crate::core::fuzzy::rank(
                &picker.query,
                picker.files.iter().map(|(_, rel)| rel.as_str()),
            )
            .into_iter()
            .take(200)
            .map(|(i, rel)| (i, rel.to_string(), String::new()))
            .collect(),
            PickerKind::Line => Vec::new(),
        };
        if picker.selected >= rows.len() {
            picker.selected = rows.len().saturating_sub(1);
        }
        let (escape, enter, up, down) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            )
        });
        if up {
            picker.selected = picker.selected.saturating_sub(1);
        }
        if down && !rows.is_empty() {
            picker.selected = (picker.selected + 1).min(rows.len() - 1);
        }
        let mut chosen: Option<usize> = None;
        let mut typed_line = false;
        egui::Modal::new(egui::Id::new("picker")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(
                egui::RichText::new(title)
                    .color(color(theme.ui.fg))
                    .size(13.5),
            );
            ui.add_space(4.0);
            let hint = match picker.kind {
                PickerKind::Commands => "Type a command",
                PickerKind::Files => "Type a file name",
                PickerKind::Line => "Line number",
            };
            let field = ui.add(
                egui::TextEdit::singleline(&mut picker.query)
                    .hint_text(hint)
                    // A modal sizes itself to its contents, so the field
                    // needs a width of its own rather than "all of it".
                    .desired_width(540.0)
                    .font(egui::TextStyle::Monospace),
            );
            if std::mem::take(&mut picker.focus_pending) {
                field.request_focus();
            }
            if field.changed() {
                picker.selected = 0;
            }
            if picker.kind == PickerKind::Line {
                typed_line = enter;
                return;
            }
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (n, (_, label, chord)) in rows.iter().enumerate() {
                        let selected = n == picker.selected;
                        let mut job = egui::text::LayoutJob::default();
                        let fmt = |c| egui::text::TextFormat {
                            font_id: egui::FontId::monospace(11.5),
                            color: c,
                            ..Default::default()
                        };
                        // The highlighted row carries a pointer and the
                        // brighter colour, the way the terminal's list does.
                        job.append(
                            &format!("{} {label}", if selected { "▸" } else { " " }),
                            0.0,
                            fmt(color(if selected {
                                theme.ui.fg_bright
                            } else {
                                theme.ui.fg_dim
                            })),
                        );
                        if !chord.is_empty() {
                            job.append(&format!("   {chord}"), 0.0, fmt(color(theme.ui.fg_faint)));
                        }
                        job.wrap.max_rows = 1;
                        job.wrap.break_anywhere = true;
                        let response = ui
                            .add(egui::Button::new(job).frame(false))
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if selected && (up || down) {
                            response.scroll_to_me(None);
                        }
                        if response.clicked() {
                            chosen = Some(n);
                        }
                    }
                });
        });
        if enter && picker.kind != PickerKind::Line && !rows.is_empty() {
            chosen = Some(picker.selected);
        }
        if escape {
            self.picker = None;
            return;
        }
        if typed_line {
            let query = picker.query.trim().to_string();
            self.picker = None;
            match query.parse::<usize>() {
                Ok(line) if line > 0 => {
                    if let Some(path) = self.editor.buffers.active().map(|b| b.path.clone()) {
                        self.jump_to(path, line);
                    }
                }
                _ => self.status = Some(format!("not a line number: {query}")),
            }
            return;
        }
        let Some(n) = chosen else { return };
        let Some((index, _, _)) = rows.get(n) else {
            return;
        };
        let kind = picker.kind;
        let file = picker.files.get(*index).map(|(path, _)| path.clone());
        let index = *index;
        self.picker = None;
        match kind {
            PickerKind::Commands => {
                if let Some(command) = ALL.get(index) {
                    self.execute(ctx, *command);
                }
            }
            PickerKind::Files => {
                if let Some(path) = file {
                    self.tree.reveal(&path);
                    self.open_file(path);
                }
            }
            PickerKind::Line => {}
        }
    }

    fn goto_modal(&mut self, ctx: &egui::Context) {
        let Some(picker) = &self.goto_picker else {
            return;
        };
        let theme = self.theme().clone();
        let mut chosen: Option<(PathBuf, usize)> = None;
        egui::Modal::new(egui::Id::new("goto_picker")).show(ctx, |ui| {
            ui.set_width(560.0);
            let title = format!(
                "{} \"{}\"",
                if picker.is_definition {
                    "Definitions of"
                } else {
                    "References to"
                },
                picker.word
            );
            ui.label(
                egui::RichText::new(title)
                    .color(color(theme.ui.fg))
                    .size(13.5),
            );
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for c in &picker.candidates {
                        let mut job = egui::text::LayoutJob::default();
                        let fmt = |c| egui::text::TextFormat {
                            font_id: egui::FontId::monospace(11.5),
                            color: c,
                            ..Default::default()
                        };
                        job.append(
                            &format!("{}:{}  ", self.project.display(&c.path), c.line),
                            0.0,
                            fmt(color(theme.ui.accent_light)),
                        );
                        job.append(&c.text, 0.0, fmt(color(theme.ui.fg_dim)));
                        job.wrap.max_rows = 1;
                        job.wrap.break_anywhere = true;
                        if ui
                            .add(egui::Button::new(job).frame(false))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            chosen = Some((c.path.clone(), c.line));
                        }
                    }
                });
        });
        if let Some((path, line)) = chosen {
            self.jump_to(path, line);
            self.goto_picker = None;
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.goto_picker = None;
        }
    }

    fn theme_modal(&mut self, ctx: &egui::Context) {
        if !self.show_theme_picker {
            return;
        }
        let theme = self.theme().clone();
        let mut chosen: Option<usize> = None;
        egui::Modal::new(egui::Id::new("theme_picker")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(
                egui::RichText::new("Color Theme")
                    .color(color(theme.ui.fg))
                    .size(13.5),
            );
            ui.add_space(6.0);
            for (i, t) in self.themes.iter().enumerate() {
                let selected = i == self.theme_index;
                let label = egui::RichText::new(&t.name)
                    .color(if selected {
                        color(theme.ui.accent_light)
                    } else {
                        color(theme.ui.fg)
                    })
                    .size(13.0);
                if ui
                    .add(egui::Button::new(label).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    chosen = Some(i);
                }
            }
            ui.add_space(6.0);
            if let Some(dir) = core_theme::user_theme_dir() {
                ui.label(
                    egui::RichText::new(format!(
                        "Drop VS Code theme .json files in {}",
                        dir.display()
                    ))
                    .color(color(theme.ui.fg_faint))
                    .size(10.5),
                );
            }
        });
        if let Some(i) = chosen {
            self.set_theme(ctx, i);
            self.show_theme_picker = false;
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.show_theme_picker = false;
        }
    }

    /// The top bar: a File menu whose entries and chords come from settings,
    /// laid out like the terminal frontend's dropdown.
    fn menu_bar(&mut self, ctx: &egui::Context) {
        let theme = self.theme().clone();
        let mut chosen: Option<Command> = None;
        egui::TopBottomPanel::top("menubar")
            .exact_height(26.0)
            .frame(
                egui::Frame::default()
                    .fill(color(theme.ui.status_bg))
                    .inner_margin(egui::Margin::symmetric(6, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        egui::RichText::new("YCODE")
                            .color(color(theme.ui.accent_light))
                            .strong()
                            .size(12.0),
                    );
                    ui.add_space(4.0);
                    // The same three menus the terminal frontend carries.
                    let menus: [MenuBarEntry; 3] = [
                        ("File", FILE_MENU, None),
                        ("View", VIEW_MENU, None),
                        (
                            "Help",
                            HELP_MENU,
                            Some(format!("Yara Code {}", env!("CARGO_PKG_VERSION"))),
                        ),
                    ];
                    for (title, entries, note) in menus {
                        ui.menu_button(
                            egui::RichText::new(title)
                                .color(color(theme.ui.fg))
                                .size(13.0),
                            |ui| {
                                ui.set_min_width(240.0);
                                if let Some(note) = &note {
                                    ui.label(
                                        egui::RichText::new(note)
                                            .color(color(theme.ui.fg_faint))
                                            .size(12.0),
                                    );
                                    ui.separator();
                                }
                                for entry in entries {
                                    let Some(command) = entry else {
                                        ui.separator();
                                        continue;
                                    };
                                    // Install Update is offered only once a
                                    // check has found one to install.
                                    if *command == Command::InstallUpdate && self.update.is_none() {
                                        continue;
                                    }
                                    let shortcut = self
                                        .settings
                                        .gui_chord(*command)
                                        .map(|c| c.to_string())
                                        .unwrap_or_default();
                                    if command_row(ui, &theme, command.label(), &shortcut).clicked()
                                    {
                                        chosen = Some(*command);
                                        ui.close_menu();
                                    }
                                }
                            },
                        );
                    }
                });
            });
        if let Some(command) = chosen {
            self.execute(ctx, command);
        }
    }

    /// Spaces of a width, or tabs — the same choices the terminal offers.
    fn indent_modal(&mut self, ctx: &egui::Context) {
        if !self.show_indent_picker {
            return;
        }
        let theme = self.theme().clone();
        let current = self.settings.indent.choice_index();
        let mut chosen: Option<usize> = None;
        egui::Modal::new(egui::Id::new("indent_picker")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label(
                egui::RichText::new("Indentation")
                    .color(color(theme.ui.fg))
                    .size(13.5),
            );
            ui.add_space(6.0);
            for (i, (label, _, _)) in crate::core::settings::Indent::CHOICES.iter().enumerate() {
                let text = egui::RichText::new(*label)
                    .color(if i == current {
                        color(theme.ui.fg_bright)
                    } else {
                        color(theme.ui.fg_dim)
                    })
                    .size(12.5);
                if ui
                    .add(egui::Button::new(text).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    chosen = Some(i);
                }
            }
        });
        if let Some(i) = chosen {
            let (_, style, width) = crate::core::settings::Indent::CHOICES[i];
            self.settings.indent.style = style;
            self.settings.indent.width = width;
            let _ = self.settings.save();
            self.status = Some(format!("indentation: {}", self.settings.indent.label()));
            self.show_indent_picker = false;
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.show_indent_picker = false;
        }
    }

    /// Every binding in effect, opened from Help → Show Key Bindings.
    fn help_modal(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        let theme = self.theme().clone();
        egui::Modal::new(egui::Id::new("help")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(
                egui::RichText::new("Key Bindings")
                    .color(color(theme.ui.fg))
                    .size(13.5),
            );
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    egui::Grid::new("help_grid")
                        .num_columns(2)
                        .spacing(egui::vec2(18.0, 4.0))
                        .show(ui, |ui| {
                            for command in crate::core::command::ALL {
                                let Some(chord) = self.settings.gui_chord(*command) else {
                                    continue;
                                };
                                ui.label(
                                    egui::RichText::new(chord.to_string())
                                        .color(color(theme.ui.fg_bright))
                                        .monospace()
                                        .size(11.5),
                                );
                                ui.label(
                                    egui::RichText::new(command.label())
                                        .color(color(theme.ui.fg_faint))
                                        .size(11.5),
                                );
                                ui.end_row();
                            }
                        });
                });
            ui.add_space(8.0);
            if ui
                .button(egui::RichText::new("Close").color(color(theme.ui.fg)))
                .clicked()
            {
                self.show_help = false;
            }
        });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.show_help = false;
        }
    }

    fn recent_modal(&mut self, ctx: &egui::Context) {
        if !self.show_recent {
            return;
        }
        let theme = self.theme().clone();
        let mut chosen: Option<PathBuf> = None;
        egui::Modal::new(egui::Id::new("recent")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(
                egui::RichText::new("Recent Projects")
                    .color(color(theme.ui.fg))
                    .size(13.5),
            );
            ui.add_space(6.0);
            for path in &self.settings.recent_projects {
                let label = egui::RichText::new(path.display().to_string())
                    .color(color(theme.ui.fg_dim))
                    .size(12.0);
                if ui
                    .add(egui::Button::new(label).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    chosen = Some(path.clone());
                }
            }
        });
        if let Some(path) = chosen {
            if path.is_dir() {
                self.set_root(path);
            } else {
                self.status = Some(format!("gone: {}", path.display()));
            }
            self.show_recent = false;
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.show_recent = false;
        }
    }

    /// Save-or-discard prompt shown when a modified buffer is being closed.
    fn close_modal(&mut self, ctx: &egui::Context) {
        let Some(index) = self.editor.pending_close else {
            return;
        };
        let Some(buf) = self.editor.buffers.list.get(index) else {
            self.editor.pending_close = None;
            return;
        };
        let name = buf.name();
        let theme = self.theme().clone();
        #[derive(Clone, Copy)]
        enum Choice {
            Save,
            Discard,
            Cancel,
        }
        let mut choice: Option<Choice> = None;
        egui::Modal::new(egui::Id::new("confirm_close")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(
                egui::RichText::new(format!("Save changes to \"{name}\"?"))
                    .color(color(theme.ui.fg))
                    .size(14.0),
            );
            ui.label(
                egui::RichText::new("The changes will be lost if you don't save them.")
                    .color(color(theme.ui.fg_dim))
                    .size(12.0),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new("Cancel").color(color(theme.ui.fg)))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    choice = Some(Choice::Cancel);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save =
                        egui::Button::new(egui::RichText::new("Save").color(egui::Color32::WHITE))
                            .fill(color(theme.ui.accent));
                    if ui
                        .add(save)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        choice = Some(Choice::Save);
                    }
                    if ui
                        .button(egui::RichText::new("Don't Save").color(color(theme.ui.fg)))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        choice = Some(Choice::Discard);
                    }
                });
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            choice = Some(Choice::Save);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            choice = Some(Choice::Cancel);
        }
        match choice {
            Some(Choice::Save) => {
                if self.editor.buffers.save(index) {
                    self.editor.close(index);
                    self.git.invalidate();
                    self.editor.pending_close = None;
                    if self.quit_after_close {
                        self.request_quit(ctx);
                    } else if self.closing_all {
                        self.close_all_tabs();
                    }
                } else {
                    self.status = Some(format!("could not write \"{name}\""));
                    self.editor.pending_close = None;
                    self.quit_after_close = false;
                    self.closing_all = false;
                }
            }
            Some(Choice::Discard) => {
                self.editor.close(index);
                self.editor.pending_close = None;
                if self.quit_after_close {
                    self.request_quit(ctx);
                } else if self.closing_all {
                    self.close_all_tabs();
                }
            }
            Some(Choice::Cancel) => {
                self.editor.pending_close = None;
                self.quit_after_close = false;
                self.closing_all = false;
            }
            None => {}
        }
    }

    /// Rewrites every match in the project, once the user has said yes.
    fn replace_all_in_project(&mut self) {
        match self.search.replace_all(self.project.roots()) {
            Ok((count, files)) => {
                self.reload_unmodified();
                self.status = Some(format!(
                    "replaced {} in {}",
                    crate::core::count(count, "occurrence"),
                    crate::core::count(files, "file")
                ));
            }
            Err(message) => self.status = Some(message),
        }
    }

    fn replace_all_modal(&mut self, ctx: &egui::Context) {
        if !self.pending_replace_all {
            return;
        }
        let theme = self.theme().clone();
        let mut answer: Option<bool> = None;
        egui::Modal::new(egui::Id::new("confirm_replace_all")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(
                egui::RichText::new(self.search.replace_all_question())
                    .color(color(theme.ui.fg))
                    .size(14.0),
            );
            ui.label(
                egui::RichText::new(search::Search::REPLACE_ALL_WARNING)
                    .color(color(theme.ui.fg_dim))
                    .size(12.0),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new("Cancel").color(color(theme.ui.fg)))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    answer = Some(false);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let replace = egui::Button::new(
                        egui::RichText::new("Replace All").color(egui::Color32::WHITE),
                    )
                    .fill(color(theme.ui.danger));
                    if ui
                        .add(replace)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        answer = Some(true);
                    }
                });
            });
        });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            answer = Some(false);
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            answer = Some(true);
        }
        if let Some(yes) = answer {
            self.pending_replace_all = false;
            if yes {
                self.replace_all_in_project();
            }
        }
    }

    fn delete_modal(&mut self, ctx: &egui::Context) {
        let Some(path) = self.pending_delete.clone() else {
            return;
        };
        let theme = self.theme().clone();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        egui::Modal::new(egui::Id::new("confirm_delete")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(
                egui::RichText::new(format!("Delete \"{name}\"?"))
                    .color(color(theme.ui.fg))
                    .size(14.0),
            );
            if path.is_dir() {
                ui.label(
                    egui::RichText::new("The folder and all its contents will be removed.")
                        .color(color(theme.ui.fg_dim))
                        .size(12.0),
                );
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new("Cancel").color(color(theme.ui.fg)))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    self.pending_delete = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let delete = egui::Button::new(
                        egui::RichText::new("Delete").color(egui::Color32::WHITE),
                    )
                    .fill(color(theme.ui.danger));
                    if ui
                        .add(delete)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        if fs_ops::delete(&path).is_ok() {
                            self.editor.buffers.close_path(&path);
                        }
                        self.pending_delete = None;
                    }
                });
            });
        });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.pending_delete = None;
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            if fs_ops::delete(&path).is_ok() {
                self.editor.buffers.close_path(&path);
            }
            self.pending_delete = None;
        }
    }

    /// The search panel: options, query, optional replace, excludes, then the
    /// grouped results — laid out so every field spans the panel exactly.
    fn search_ui(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let mut rerun = false;

        egui::Frame::default()
            .inner_margin(egui::Margin::symmetric(10, 0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;

                // Headings over the fields, in the git view's form; each one
                // lights up while its field has the keyboard.
                let field_id = |which: SearchField| {
                    egui::Id::new(match which {
                        SearchField::Query => "search_query",
                        SearchField::Replace => "search_replace",
                        SearchField::Exclude => "search_exclude",
                    })
                };
                let heading = |ui: &mut egui::Ui, text: &str, focused: bool| {
                    ui.label(egui::RichText::new(text).size(10.0).color(if focused {
                        color(theme.ui.accent_light)
                    } else {
                        color(theme.ui.fg_faint)
                    }));
                };

                ui.horizontal(|ui| {
                    let query_focused = ui.memory(|m| m.has_focus(field_id(SearchField::Query)));
                    heading(ui, "SEARCH", query_focused);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut toggle =
                            |ui: &mut egui::Ui, on: &mut bool, label: &str, hint: &str| {
                                let text = egui::RichText::new(label)
                                    .color(if *on {
                                        color(theme.ui.fg_bright)
                                    } else {
                                        color(theme.ui.fg_faint)
                                    })
                                    .size(11.0);
                                let button = egui::Button::new(text).frame(true).fill(if *on {
                                    color(theme.ui.selected_bg)
                                } else {
                                    egui::Color32::TRANSPARENT
                                });
                                if ui
                                    .add(button)
                                    .on_hover_text(hint)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    *on = !*on;
                                    rerun = true;
                                }
                            };
                        toggle(ui, &mut self.search.regex, ".*", "Use Regular Expression");
                        toggle(ui, &mut self.search.whole_word, "ab", "Match Whole Word");
                        toggle(ui, &mut self.search.case_sensitive, "Aa", "Match Case");
                    });
                });

                let mut field = |ui: &mut egui::Ui, which: SearchField, focus: bool| {
                    let hint = which.hint();
                    let text = match which {
                        SearchField::Query => &mut self.search.query,
                        SearchField::Replace => &mut self.search.replace,
                        SearchField::Exclude => &mut self.search.exclude,
                    };
                    let resp = ui.add(
                        egui::TextEdit::singleline(text)
                            .id(field_id(which))
                            .desired_width(f32::INFINITY)
                            .hint_text(hint)
                            .font(egui::TextStyle::Body),
                    );
                    if focus {
                        resp.request_focus();
                    }
                    resp.changed()
                };

                let focus = std::mem::take(&mut self.search.focus_pending);
                if field(ui, SearchField::Query, focus) {
                    rerun = true;
                }
                // A gap between each label-and-field pair, so the form reads
                // as groups rather than a stack of rows.
                ui.add_space(6.0);
                let focused = ui.memory(|m| m.has_focus(field_id(SearchField::Replace)));
                heading(ui, "REPLACE", focused);
                field(ui, SearchField::Replace, false);
                ui.add_space(6.0);
                let focused = ui.memory(|m| m.has_focus(field_id(SearchField::Exclude)));
                heading(ui, "EXCLUDE", focused);
                if field(ui, SearchField::Exclude, false) {
                    rerun = true;
                }
                if let Some(example) = SearchField::Exclude.example() {
                    ui.label(
                        egui::RichText::new(example)
                            .size(10.0)
                            .color(color(theme.ui.fg_faint)),
                    );
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let summary = self.search.summary();
                    let tone = if self.search.error.is_some() {
                        theme.ui.danger
                    } else {
                        theme.ui.fg_faint
                    };
                    ui.label(egui::RichText::new(summary).color(color(tone)).size(11.0));
                    if !self.search.results.is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(
                                    egui::RichText::new("Replace All")
                                        .color(color(theme.ui.fg))
                                        .size(11.0),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.pending_replace_all = true;
                            }
                        });
                    }
                });
            });

        if rerun {
            self.search.run(self.project.roots());
        } else {
            self.search.run_if_changed(self.project.roots());
        }

        ui.add_space(4.0);
        let mut jump: Option<(PathBuf, usize)> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for file in &self.search.results {
                    let rel = self.project.display(&file.path);
                    egui::CollapsingHeader::new(
                        egui::RichText::new(rel)
                            .color(color(theme.ui.fg))
                            .size(12.5),
                    )
                    .id_salt(&file.path)
                    .default_open(true)
                    .show(ui, |ui| {
                        for m in &file.matches {
                            let mut job = egui::text::LayoutJob::default();
                            let font = egui::FontId::monospace(11.5);
                            let fmt = |fg, bg| egui::text::TextFormat {
                                font_id: font.clone(),
                                color: fg,
                                background: bg,
                                ..Default::default()
                            };
                            let clear = egui::Color32::TRANSPARENT;
                            job.append(
                                &format!("{:>4}  ", m.line),
                                0.0,
                                fmt(color(theme.ui.fg_faint), clear),
                            );
                            job.append(&m.prefix, 0.0, fmt(color(theme.ui.fg_dim), clear));
                            job.append(
                                &m.matched,
                                0.0,
                                fmt(color(theme.ui.fg), color(theme.ui.match_bg)),
                            );
                            job.append(&m.suffix, 0.0, fmt(color(theme.ui.fg_dim), clear));
                            job.wrap.max_rows = 1;
                            job.wrap.break_anywhere = true;
                            if ui
                                .add(egui::Button::new(job).frame(false))
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                jump = Some((file.path.clone(), m.line));
                            }
                        }
                    });
                }
            });
        if let Some((path, line)) = jump {
            self.jump_to(path, line);
            self.editor.seed_find(
                self.search.query.clone(),
                self.search.regex,
                self.search.case_sensitive,
                self.search.whole_word,
                line,
            );
        }
    }

    /// Re-reads open buffers that have no unsaved edits, after files were
    /// rewritten underneath them.
    fn reload_unmodified(&mut self) {
        for buf in &mut self.editor.buffers.list {
            if buf.modified() {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&buf.path) {
                buf.saved_text = text.clone();
                buf.text = text;
            }
        }
        // The find bar's hits were offsets into the old text.
        if self.editor.find_showing() {
            self.editor.refresh_find();
        }
    }

    /// The find bar above the editor, opened with Cmd+F.
    /// Find in this file, drawn as the project search panel's form: a lit
    /// heading over each field, both fields always present, and the counter and
    /// the actions on one line under them.
    fn find_bar(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        if !self.editor.find_showing() {
            return;
        }
        let mut close = false;
        let mut step = 0isize;
        let mut replace_one = false;
        let mut replace_all = false;
        // Typing in the query, or flipping an option, changes what matches.
        let mut refresh = false;
        let query_id = egui::Id::new("find_query");
        let replace_id = egui::Id::new("find_replace");

        egui::Frame::default()
            .fill(color(theme.ui.status_bg))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let heading = |ui: &mut egui::Ui, text: &str, focused: bool| {
                    ui.label(egui::RichText::new(text).size(10.0).color(if focused {
                        color(theme.ui.accent_light)
                    } else {
                        color(theme.ui.fg_faint)
                    }));
                };

                // Heading row: FIND on the left, the option toggles at the
                // right edge — the search panel's arrangement.
                ui.horizontal(|ui| {
                    heading(ui, "FIND", ui.memory(|m| m.has_focus(query_id)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // The close mark sits in the corner, above everything
                        // the bar does.
                        if ui
                            .button(egui::RichText::new(icons().close).size(11.0))
                            .clicked()
                        {
                            close = true;
                        }
                        let toggle = |ui: &mut egui::Ui, on: &mut bool, label: &str, hint: &str| {
                            let text = egui::RichText::new(label)
                                .color(if *on {
                                    color(theme.ui.fg_bright)
                                } else {
                                    color(theme.ui.fg_faint)
                                })
                                .size(11.0);
                            let button = egui::Button::new(text).frame(true).fill(if *on {
                                color(theme.ui.selected_bg)
                            } else {
                                egui::Color32::TRANSPARENT
                            });
                            if ui
                                .add(button)
                                .on_hover_text(hint)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                *on = !*on;
                            }
                        };
                        toggle(
                            ui,
                            &mut self.editor.find.regex,
                            ".*",
                            "Use Regular Expression",
                        );
                        toggle(
                            ui,
                            &mut self.editor.find.whole_word,
                            "ab",
                            "Match Whole Word",
                        );
                        toggle(ui, &mut self.editor.find.case_sensitive, "Aa", "Match Case");
                    });
                });

                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.editor.find.query)
                        .id(query_id)
                        .desired_width(f32::INFINITY)
                        .hint_text(FIELD_HINT)
                        .font(egui::TextStyle::Body),
                );
                if std::mem::take(&mut self.editor.find.focus_pending) {
                    resp.request_focus();
                }
                if resp.changed() {
                    refresh = true;
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    // Enter steps forward, Shift+Enter back — as in the
                    // terminal frontend and every find bar since.
                    step = if ui.input(|i| i.modifiers.shift) {
                        -1
                    } else {
                        1
                    };
                    self.editor.find.focus_pending = true;
                }

                // A gap between each label-and-field pair, as in the panel.
                ui.add_space(6.0);
                heading(ui, "REPLACE", ui.memory(|m| m.has_focus(replace_id)));
                ui.add(
                    egui::TextEdit::singleline(&mut self.editor.find.replace)
                        .id(replace_id)
                        .desired_width(f32::INFINITY)
                        .hint_text(FIELD_HINT)
                        .font(egui::TextStyle::Body),
                );

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let tone = if self.editor.find.error.is_some() {
                        theme.ui.danger
                    } else {
                        theme.ui.fg_faint
                    };
                    ui.label(
                        egui::RichText::new(self.editor.find.summary())
                            .color(color(tone))
                            .size(11.0),
                    );
                    // Right to left, so they read Replace · Replace All · < ·
                    // > along the bottom edge. The two replace actions only
                    // appear once there is something to replace with.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new(">").size(11.0)).clicked() {
                            step = 1;
                        }
                        if ui.button(egui::RichText::new("<").size(11.0)).clicked() {
                            step = -1;
                        }
                        if !self.editor.find.replace.is_empty() {
                            if ui
                                .button(egui::RichText::new("Replace All").size(11.0))
                                .clicked()
                            {
                                replace_all = true;
                            }
                            if ui
                                .button(egui::RichText::new("Replace").size(11.0))
                                .clicked()
                            {
                                replace_one = true;
                            }
                        }
                    });
                });
            });

        // The modals are drawn after this bar, so they would never see an
        // Escape it took. Whatever is layered on top gets first refusal.
        if !self.modal_open()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            close = true;
        }
        if close {
            self.editor.find.open = false;
        }
        if refresh {
            self.editor.refresh_find();
        }
        if step != 0 {
            self.editor.find_step(step);
        }
        if replace_one {
            self.editor.find_replace_current();
        }
        if replace_all {
            let replaced = self.editor.find_replace_all();
            self.status = Some(format!(
                "replaced {}",
                crate::core::count(replaced, "occurrence")
            ));
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum PickerKind {
    Commands,
    Files,
    Line,
}

/// One of the three pickers while it is up.
struct Picker {
    kind: PickerKind,
    query: String,
    selected: usize,
    /// What Go to File can offer: every file, with its path from the root.
    files: Vec<(PathBuf, String)>,
    focus_pending: bool,
}

impl Picker {
    fn new(kind: PickerKind) -> Self {
        Self {
            kind,
            query: String::new(),
            selected: 0,
            files: Vec::new(),
            focus_pending: true,
        }
    }
}

/// A menu in the top bar: its title, its entries, and a line about itself.
type MenuBarEntry = (&'static str, &'static [Option<Command>], Option<String>);

/// One dropdown row: label on the left, key chord greyed out on the right —
/// the same shape the terminal frontend draws.
fn command_row(ui: &mut egui::Ui, theme: &Theme, label: &str, shortcut: &str) -> egui::Response {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        label,
        0.0,
        egui::text::TextFormat {
            font_id: egui::FontId::proportional(13.0),
            color: color(theme.ui.fg),
            ..Default::default()
        },
    );
    if !shortcut.is_empty() {
        job.append(
            shortcut,
            18.0,
            egui::text::TextFormat {
                font_id: egui::FontId::proportional(12.0),
                color: color(theme.ui.fg_faint),
                ..Default::default()
            },
        );
    }
    ui.add(egui::Button::new(job).frame(false))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}

impl App {
    /// One frame of the editor. `eframe` calls it through `update`; a test
    /// calls it through `egui::Context::run`.
    pub fn ui(&mut self, ctx: &egui::Context) {
        speed_up_scrolling(ctx, self.settings.scroll_factor());
        self.poll_settings(ctx);
        // Every shortcut comes from settings.json, so a rebind takes effect
        // the moment the file is saved.
        //
        // The most specific chord is offered first: egui ignores extra Shift
        // and Alt when matching, so Cmd+F would otherwise swallow Cmd+Shift+F
        // and Search would open Find in File.
        let mut bindings: Vec<(Command, _)> = self
            .settings
            .keys
            .gui
            .iter()
            .filter_map(|(id, chord)| Some((Command::from_id(id)?, chord.clone())))
            .collect();
        bindings.sort_by_key(|(_, chord)| {
            let m = &chord.mods;
            std::cmp::Reverse(
                usize::from(m.cmd)
                    + usize::from(m.ctrl)
                    + usize::from(m.alt)
                    + usize::from(m.shift),
            )
        });
        // A dialog owns the keyboard while it is up: a chord pressed then is
        // meant for it, not for whatever lies underneath.
        if !self.modal_open() {
            for (command, chord) in bindings {
                if keys::consumed(ctx, &chord) {
                    self.execute(ctx, command);
                }
            }
        }

        let theme = self.theme().clone();
        // Git's view of the project feeds the navigator's colours and the
        // gutter too, so it is kept current whichever panel is showing.
        if let Some(root) = self.project.root().map(Path::to_path_buf) {
            self.git.state.tick(&root);
        }
        self.refresh_git_lines();
        self.refresh_blame();
        self.collect_update();
        self.collect_install();
        // A check or an install turns a marker, and a download fills a bar;
        // neither moves unless there are frames to move them in.
        if self.updates.is_running() || self.installs.is_running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // The window's close button is a Quit too: with unsaved work, hold
        // the window open and ask.
        if ctx.input(|i| i.viewport().close_requested())
            && self.editor.buffers.list.iter().any(|b| b.modified())
            && !self.quit_after_close
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_quit(ctx);
        }
        self.menu_bar(ctx);

        // Navigator / search / git sidebar. Declared before the status bar so
        // it runs the full window height; the view switcher lives in the
        // panel's footer, as a row of painted icons.
        if self.show_sidebar {
            egui::SidePanel::left("navigator")
                .resizable(true)
                .default_width(220.0)
                .width_range(160.0..=420.0)
                .frame(egui::Frame::default().fill(color(theme.ui.sidebar_bg)))
                .show(ctx, |ui| {
                    egui::TopBottomPanel::bottom("nav_footer")
                        .exact_height(32.0)
                        .frame(egui::Frame::default().fill(color(theme.ui.status_bg)))
                        .show_inside(ui, |ui| {
                            let views = [
                                (SidebarView::Files, "FILES"),
                                (SidebarView::Search, "SEARCH"),
                                (SidebarView::Git, "GIT"),
                            ];
                            ui.horizontal_centered(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                ui.add_space(2.0);
                                for (view, label) in views {
                                    let selected = self.sidebar_view == view;
                                    if nav_tab(ui, &theme, view, selected, label).clicked() {
                                        self.sidebar_view = view;
                                        if view == SidebarView::Search {
                                            self.search.focus_pending = true;
                                        }
                                    }
                                }
                            });
                        });
                    let mut diff_from_git: Option<Change> = None;
                    egui::CentralPanel::default()
                        .frame(egui::Frame::default().fill(color(theme.ui.sidebar_bg)))
                        .show_inside(ui, |ui| {
                            ui.add_space(10.0);
                            match self.sidebar_view {
                                SidebarView::Files => {
                                    let mut events = Vec::new();
                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            self.tree.ui(ui, &theme, &self.git.state, &mut events);
                                        });
                                    self.handle_tree_events(events);
                                }
                                SidebarView::Search => self.search_ui(ui, &theme),
                                SidebarView::Git => {
                                    diff_from_git =
                                        self.git.ui(ui, &theme, &self.project.root_or_cwd());
                                }
                            }
                        });
                    if let Some(change) = diff_from_git {
                        self.show_diff(&change);
                    }
                    if let Some(message) = self.git.message.take() {
                        self.status = Some(message);
                    }
                });
        }

        // Status bar. Declared after the sidebar, so it spans the editor
        // region only.
        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(
                egui::Frame::default()
                    .fill(color(theme.ui.status_bg))
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(buf) = self.editor.buffers.active() {
                        let path = self.project.display(&buf.path);
                        let modified = buf.modified();
                        let resp = ui.label(
                            egui::RichText::new(path)
                                .color(color(theme.ui.fg_dim))
                                .size(11.5),
                        );
                        if modified {
                            let c = egui::pos2(resp.rect.right() + 7.0, resp.rect.center().y);
                            ui.painter()
                                .circle_filled(c, 3.0, color(theme.ui.accent_light));
                            ui.add_space(10.0);
                        }
                    }
                    if let Some(message) = &self.status {
                        // Most of what lands here — "saved as…", "added…", a
                        // note about settings.json — is not a failure, so the
                        // status bar speaks in the theme's warning yellow and
                        // keeps red for what actually went wrong.
                        ui.label(
                            egui::RichText::new(message)
                                .color(if crate::core::is_failure(message) {
                                    color(theme.ui.danger)
                                } else {
                                    ansi_color(&theme, 3)
                                })
                                .size(11.5),
                        );
                    } else if let Some(blame) = &self.blame {
                        // Who last touched the line the cursor is on.
                        ui.label(
                            egui::RichText::new(blame.line())
                                .color(color(theme.ui.fg_faint))
                                .size(11.5),
                        );
                    }
                    // A download says how far it has got as a bar as well as
                    // in words; the steps after it have no length to measure.
                    if let Some(fraction) = self.installing.as_ref().and_then(|p| p.fraction()) {
                        ui.add_space(8.0);
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(80.0, 5.0), egui::Sense::hover());
                        let painter = ui.painter();
                        painter.rect_filled(rect, 2.0, color(theme.ui.fg_faint));
                        let mut filled = rect;
                        filled.set_width(rect.width() * fraction);
                        painter.rect_filled(filled, 2.0, color(theme.ui.accent_light));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new(&theme.name)
                                        .color(color(theme.ui.fg_faint))
                                        .size(11.5),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.show_theme_picker = true;
                        }
                        if let Some(buf) = self.editor.buffers.active() {
                            let lang = if buf.extension.is_empty() {
                                "plain text".to_string()
                            } else {
                                buf.extension.clone()
                            };
                            ui.label(
                                egui::RichText::new(lang)
                                    .color(color(theme.ui.accent_light))
                                    .size(11.5),
                            );
                            if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(self.settings.indent.label())
                                            .color(color(theme.ui.fg_dim))
                                            .size(11.5),
                                    )
                                    .sense(egui::Sense::click()),
                                )
                                .on_hover_text("Indentation")
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.show_indent_picker = true;
                            }
                            if let Some((line, col)) = self.editor.cursor {
                                ui.label(
                                    egui::RichText::new(format!("Ln {line}, Col {col}"))
                                        .color(color(theme.ui.fg_dim))
                                        .size(11.5),
                                );
                            }
                        }
                    });
                });
            });

        // Terminal panel. Declared after the sidebar so it sits under the
        // editor only, matching the terminal frontend.
        if self.show_terminal {
            egui::TopBottomPanel::bottom("terminal")
                .resizable(true)
                .default_height(240.0)
                .height_range(80.0..=600.0)
                .frame(
                    egui::Frame::default()
                        .fill(color(theme.ui.editor_bg))
                        .inner_margin(egui::Margin::ZERO),
                )
                .show(ctx, |ui| {
                    // Panel header, matching the terminal frontend's: the
                    // label plus the session tab strip.
                    let root = self.project.root_or_cwd();
                    let focused = self.terminal.focused;
                    egui::Frame::default()
                        .fill(color(theme.ui.status_bg))
                        .inner_margin(egui::Margin::symmetric(10, 3))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut text =
                                    egui::RichText::new("TERMINAL")
                                        .size(11.0)
                                        .color(if focused {
                                            color(theme.ui.accent_light)
                                        } else {
                                            color(theme.ui.fg_faint)
                                        });
                                if focused {
                                    text = text.strong();
                                }
                                ui.label(text);
                                ui.add_space(10.0);
                                let sessions = self.terminal.sessions.len();
                                self.terminal.tab_bar(ui, &theme, &root, ctx);
                                // Closing the last tab dismisses the panel,
                                // instead of respawning a shell right away.
                                if sessions > 0 && self.terminal.sessions.is_empty() {
                                    self.show_terminal = false;
                                }
                                ui.allocate_space(egui::vec2(ui.available_width(), 0.0));
                            });
                        });
                    ui.add_space(4.0);
                    egui::Frame::default()
                        .inner_margin(egui::Margin::symmetric(8, 0))
                        .show(ui, |ui| {
                            if self.show_terminal {
                                self.terminal.ui(ui, &theme, &root, ctx);
                            }
                        });
                });
        }

        // Tab bar: files and diffs share it, so it stands whenever either is
        // open.
        if !self.editor.buffers.is_empty()
            || !self.editor.diffs.is_empty()
            || !self.editor.previews.is_empty()
        {
            egui::TopBottomPanel::top("tabs")
                .exact_height(34.0)
                .frame(egui::Frame::default().fill(color(theme.ui.status_bg)))
                .show(ctx, |ui| {
                    let preview_chord = self
                        .settings
                        .gui_chord(Command::TogglePreview)
                        .map(|c| c.to_string());
                    self.editor.tab_bar(ui, &theme, preview_chord.as_deref());
                    if std::mem::take(&mut self.editor.close_all_requested) {
                        self.close_all_tabs();
                    }
                });
        }

        // Editor.
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(color(theme.ui.editor_bg))
                    .inner_margin(egui::Margin {
                        left: 4,
                        right: 0,
                        top: 0,
                        bottom: 0,
                    }),
            )
            .show(ctx, |ui| {
                // A preview tab shows the rendered file in place of the text.
                if let Some(index) = self.editor.active_preview {
                    let Some(preview) = self.editor.previews.get(index) else {
                        self.editor.active_preview = None;
                        return;
                    };
                    if preview.ui(ui, &theme) == crate::gui::preview::PreviewEvent::Close {
                        self.editor.close_preview(index);
                    }
                    return;
                }
                // A diff tab shows its two panes in place of the text.
                if let Some(index) = self.editor.active_diff {
                    let Some(diff) = self.editor.diffs.get_mut(index) else {
                        self.editor.active_diff = None;
                        return;
                    };
                    match diff.ui(ui, &theme) {
                        DiffEvent::Close => self.editor.close_diff(index),
                        DiffEvent::Open => {
                            let path = self.git.state.dir().map(|dir| dir.join(&diff.path));
                            self.editor.close_diff(index);
                            if let Some(path) = path {
                                self.tree.reveal(&path);
                                self.open_file(path);
                            }
                        }
                        DiffEvent::None => {}
                    }
                    return;
                }
                self.find_bar(ui, &theme);
                if self.editor.buffers.is_empty() {
                    ui.add_space(8.0);
                    if let Some(command) = self.start_page(ui, &theme) {
                        let ctx = ui.ctx().clone();
                        self.execute(&ctx, command);
                    }
                } else {
                    self.editor.ui(
                        ui,
                        &theme,
                        &self.settings.indent,
                        &self.settings.goto_modifiers.gui,
                        &self.git_lines,
                    );
                }
            });

        self.handle_goto();
        self.picker_modal(ctx);
        self.recent_modal(ctx);
        self.help_modal(ctx);
        self.indent_modal(ctx);
        self.goto_modal(ctx);
        self.theme_modal(ctx);
        self.delete_modal(ctx);
        self.replace_all_modal(ctx);
        self.close_modal(ctx);
    }
}

/// Scales this frame's scrolling before any panel reads it, so every scrolling
/// thing in the window — the editor, the navigator, the terminal panel — moves
/// at the same multiple of what the desktop sent. The multiple is
/// `scroll_speed` in settings.json, the same number the terminal frontend
/// counts its rows with.
fn speed_up_scrolling(ctx: &egui::Context, factor: f32) {
    ctx.input_mut(|i| {
        i.raw_scroll_delta *= factor;
        i.smooth_scroll_delta *= factor;
    });
}

/// One footer switch — a glyph plus its label, the same row the terminal
/// frontend draws at the bottom of its sidebar, in the same three shapes.
fn nav_tab(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: SidebarView,
    selected: bool,
    label: &str,
) -> egui::Response {
    let font = egui::FontId::proportional(10.5);
    let label_width = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), egui::Color32::WHITE)
        .size()
        .x;
    let icon_w = 13.0;
    let width = 6.0 + icon_w + 4.0 + label_width + 6.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let stroke_color = if selected {
        color(theme.ui.fg_bright)
    } else if resp.hovered() {
        color(theme.ui.fg)
    } else {
        color(theme.ui.fg_faint)
    };
    if resp.hovered() && !selected {
        ui.painter().rect_filled(
            egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width() - 2.0, 20.0)),
            egui::CornerRadius::same(4),
            color(theme.ui.hover_bg),
        );
    }
    let c = egui::pos2(rect.min.x + 6.0 + icon_w / 2.0, rect.center().y);
    let icons = icons();
    let mark = match view {
        SidebarView::Files => icons.nav_files,
        SidebarView::Search => icons.nav_search,
        SidebarView::Git => icons.nav_git,
    };
    glyph(ui.painter(), c, mark, 13.0, stroke_color);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, stroke_color);
    let text_pos = egui::pos2(
        rect.min.x + 6.0 + icon_w + 4.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(text_pos, galley, stroke_color);
    resp
}

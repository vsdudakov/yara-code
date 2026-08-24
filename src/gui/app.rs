use std::path::PathBuf;

use crate::core::buffer::relative_path;
use crate::core::command::{Command, FILE_MENU};
use crate::core::fs_ops;
use crate::core::search::{self, Candidate, Field as SearchField, Search};
use crate::core::settings::Settings;
use crate::core::theme::{self as core_theme, Theme};
use crate::gui::keys;
use crate::gui::editor::Editor;
use crate::gui::file_tree::{FileTree, TreeEvent};
use crate::gui::git::GitPanel;
use crate::gui::highlight;
use crate::gui::terminal::Terminal;
use crate::gui::theme::color;

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
    root: PathBuf,
    tree: FileTree,
    editor: Editor,
    terminal: Terminal,
    show_terminal: bool,
    show_sidebar: bool,
    sidebar_view: SidebarView,
    search: Search,
    git: GitPanel,
    pending_delete: Option<PathBuf>,
    goto_picker: Option<GotoPicker>,
    /// Locations to return to with ⌃- after a goto jump.
    history: Vec<(PathBuf, usize)>,
    themes: Vec<Theme>,
    theme_index: usize,
    show_theme_picker: bool,
    /// Last file-operation error, shown in the status bar.
    status: Option<String>,
    settings: Settings,
    /// Open text prompt (Open File..., Open Folder..., Save As...).
    text_prompt: Option<(Command, String, bool)>,
    show_recent: bool,
    recent_selected: usize,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, root: PathBuf) -> Self {
        let themes = core_theme::load_all();
        let (mut settings, settings_error) = Settings::load();
        settings.push_recent(&root);
        let _ = settings.save();
        let theme_index = themes
            .iter()
            .position(|t| t.name == settings.theme)
            .unwrap_or(0);
        let theme = themes.get(theme_index).cloned().unwrap_or_default();
        crate::gui::theme::apply(&cc.egui_ctx, &theme);
        highlight::set_theme(&theme);
        Self {
            tree: FileTree::new(root.clone()),
            root,
            editor: Editor::default(),
            terminal: Terminal::default(),
            show_terminal: settings.show_terminal,
            show_sidebar: settings.show_sidebar,
            sidebar_view: SidebarView::Files,
            search: Search::default(),
            git: GitPanel::default(),
            pending_delete: None,
            goto_picker: None,
            history: Vec::new(),
            themes,
            theme_index,
            show_theme_picker: false,
            status: settings_error,
            settings,
            text_prompt: None,
            show_recent: false,
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
        crate::gui::theme::apply(ctx, &theme);
        highlight::set_theme(&theme);
    }

    /// Runs a bound command, whether it came from a key chord or the menu bar.
    fn execute(&mut self, ctx: &egui::Context, command: Command) {
        self.status = None;
        match command {
            Command::NewFile | Command::OpenFile | Command::OpenFolder | Command::SaveAs => {
                if command == Command::SaveAs && self.editor.buffers.is_empty() {
                    self.status = Some("nothing to save".into());
                    return;
                }
                self.text_prompt = Some((command, String::new(), true));
            }
            Command::OpenRecent => {
                if self.settings.recent_projects.is_empty() {
                    self.status = Some("no recent projects yet".into());
                } else {
                    self.recent_selected = 0;
                    self.show_recent = true;
                }
            }
            Command::Save => {
                if self.editor.buffers.save_active() {
                    self.after_save(ctx);
                } else {
                    self.status = Some("nothing to save".into());
                }
            }
            Command::SaveAll => {
                let previous = self.editor.buffers.active;
                let mut saved = 0;
                for i in 0..self.editor.buffers.list.len() {
                    self.editor.buffers.active = i;
                    if self.editor.buffers.list[i].modified()
                        && self.editor.buffers.save_active()
                    {
                        saved += 1;
                    }
                }
                self.editor.buffers.active = previous;
                self.after_save(ctx);
                self.status = Some(format!("saved {saved} file(s)"));
            }
            Command::Settings => match self.settings.ensure_file() {
                Ok(path) => {
                    self.editor.open(path);
                    self.status = Some("editing settings — save to apply".into());
                }
                Err(e) => self.status = Some(format!("cannot open settings: {e}")),
            },
            Command::CloseEditor => {
                let active = self.editor.buffers.active;
                self.editor.request_close(active);
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            Command::ToggleTerminal => self.show_terminal = !self.show_terminal,
            Command::NewTerminal => {
                self.show_terminal = true;
                self.terminal.open(&self.root, ctx);
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
                    self.editor.open(path.clone());
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
            Command::SelectAll
            | Command::Copy
            | Command::Cut
            | Command::Paste
            | Command::GotoDefinition
            | Command::ContextMenu
            | Command::FileMenu
            | Command::Help => {}
        }
    }

    /// Re-reads settings when the settings file itself was just saved.
    fn after_save(&mut self, ctx: &egui::Context) {
        // A save changes the git status; show it right away, not on the timer.
        self.git.invalidate();
        let is_settings = self
            .editor
            .buffers
            .active()
            .zip(Settings::path())
            .is_some_and(|(buf, path)| buf.path == path);
        if !is_settings {
            self.status = Some("saved".into());
            return;
        }
        let (settings, error) = Settings::load();
        self.settings = settings;
        if let Some(index) = self
            .themes
            .iter()
            .position(|t| t.name == self.settings.theme)
        {
            self.set_theme(ctx, index);
        }
        self.status = Some(error.unwrap_or_else(|| "settings applied".to_string()));
    }

    /// The empty-editor hint, built from the bindings actually in effect.
    fn empty_hint(&self) -> String {
        let chord = |command| {
            self.settings
                .gui_chord(command)
                .map(|c| c.to_string())
                .unwrap_or_default()
        };
        format!(
            "Open a file from the navigator   ·   {} save   ·   {} terminal   ·   {} sidebar   ·   {} search",
            chord(Command::Save),
            chord(Command::ToggleTerminal),
            chord(Command::ToggleSidebar),
            chord(Command::FocusSearch),
        )
    }

    /// Interprets typed input as a path: absolute, `~`-relative, or relative to
    /// the project root.
    fn resolve(&self, input: &str) -> PathBuf {
        let input = input.trim();
        if let Some(rest) = input.strip_prefix("~/") {
            if let Some(home) = std::env::var_os("HOME") {
                return PathBuf::from(home).join(rest);
            }
        }
        let path = PathBuf::from(input);
        if path.is_absolute() {
            path
        } else {
            self.root.join(path)
        }
    }

    fn set_root(&mut self, root: PathBuf) {
        let root = root.canonicalize().unwrap_or(root);
        self.tree = FileTree::new(root.clone());
        self.search = Search::default();
        self.terminal.sessions.clear();
        self.settings.push_recent(&root);
        let _ = self.settings.save();
        self.root = root;
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
        self.editor.open(path.clone());
        self.tree.reveal(&path);
        self.editor.pending_jump = Some((path, line));
    }

    fn handle_tree_events(&mut self, events: Vec<TreeEvent>) {
        for event in events {
            match event {
                TreeEvent::Open(path) => {
                    self.tree.reveal(&path);
                    self.editor.open(path);
                }
                TreeEvent::RequestDelete(path) => self.pending_delete = Some(path),
                TreeEvent::Moved { from, to } => {
                    self.editor.buffers.retarget(&from, &to);
                    self.status = None;
                }
                TreeEvent::Failed(message) => self.status = Some(message),
            }
        }
    }

    fn handle_goto(&mut self) {
        let Some(word) = self.editor.goto_request.take() else {
            return;
        };
        let current = self.editor.location().map(|(p, _)| p);
        let mut candidates = search::find_definitions(&self.root, &word);
        let is_definition = !candidates.is_empty();
        if !is_definition {
            candidates = search::find_references(&self.root, &word, 100);
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
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for c in &picker.candidates {
                    let mut job = egui::text::LayoutJob::default();
                    let fmt = |c| egui::text::TextFormat {
                        font_id: egui::FontId::monospace(11.5),
                        color: c,
                        ..Default::default()
                    };
                    job.append(
                        &format!("{}:{}  ", relative_path(&c.path, &self.root), c.line),
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
        } else if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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
        } else if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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
                        egui::RichText::new("YARA")
                            .color(color(theme.ui.accent_light))
                            .strong()
                            .size(12.0),
                    );
                    ui.add_space(4.0);
                    ui.menu_button(
                        egui::RichText::new("File").color(color(theme.ui.fg)).size(13.0),
                        |ui| {
                            ui.set_min_width(240.0);
                            for entry in FILE_MENU {
                                let Some(command) = entry else {
                                    ui.separator();
                                    continue;
                                };
                                let shortcut = self
                                    .settings
                                    .gui_chord(*command)
                                    .map(|c| c.to_string())
                                    .unwrap_or_default();
                                if command_row(ui, &theme, command.label(), &shortcut).clicked() {
                                    chosen = Some(*command);
                                    ui.close_menu();
                                }
                            }
                        },
                    );
                });
            });
        if let Some(command) = chosen {
            self.execute(ctx, command);
        }
    }

    /// Text prompts for Open File..., Open Folder..., New File... and Save As...
    fn text_prompt_modal(&mut self, ctx: &egui::Context) {
        let Some((command, _, _)) = &self.text_prompt else {
            return;
        };
        let command = *command;
        let theme = self.theme().clone();
        let title = match command {
            Command::NewFile => "New file (path relative to the project)",
            Command::OpenFile => "Open file (path relative to the project)",
            Command::OpenFolder => "Open folder as project",
            _ => "Save as (path relative to the project)",
        };
        let mut submit = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("text_prompt")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(
                egui::RichText::new(title)
                    .color(color(theme.ui.fg))
                    .size(13.5),
            );
            ui.add_space(6.0);
            let Some((_, input, focus)) = &mut self.text_prompt else {
                return;
            };
            let resp = ui.add(
                egui::TextEdit::singleline(input)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            if *focus {
                resp.request_focus();
                *focus = false;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                submit = true;
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new("Cancel").color(color(theme.ui.fg)))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    cancel = true;
                }
                if ui
                    .button(egui::RichText::new("OK").color(color(theme.ui.fg)))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    submit = true;
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if cancel {
            self.text_prompt = None;
            return;
        }
        if !submit {
            return;
        }
        let (_, input, _) = self.text_prompt.take().unwrap();
        let path = self.resolve(&input);
        match command {
            Command::NewFile => {
                let (dir, name) = match path.parent().zip(path.file_name()) {
                    Some((dir, name)) => (dir.to_path_buf(), name.to_string_lossy().into_owned()),
                    None => (self.root.clone(), input.clone()),
                };
                match fs_ops::create_file(&dir, &name) {
                    Ok(created) => self.editor.open(created),
                    Err(e) => self.status = Some(format!("create failed: {e}")),
                }
            }
            Command::OpenFile => {
                if path.is_dir() {
                    self.set_root(path);
                } else if path.is_file() {
                    self.editor.open(path);
                } else {
                    self.status = Some(format!("no such file: {}", path.display()));
                }
            }
            Command::OpenFolder => {
                if path.is_dir() {
                    self.set_root(path);
                } else {
                    self.status = Some(format!("not a folder: {}", path.display()));
                }
            }
            _ => {
                if input.trim().is_empty() {
                    return;
                }
                if let Some(buf) = self.editor.buffers.active_mut() {
                    buf.path = path.clone();
                    buf.extension = path
                        .extension()
                        .map(|e| e.to_string_lossy().into_owned())
                        .unwrap_or_default();
                }
                if self.editor.buffers.save_active() {
                    self.status = Some(format!("saved as {}", relative_path(&path, &self.root)));
                } else {
                    self.status = Some(format!("could not write {}", path.display()));
                }
            }
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
                egui::RichText::new("Recent projects")
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
        } else if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            choice = Some(Choice::Cancel);
        }
        match choice {
            Some(Choice::Save) => {
                if self.editor.buffers.save(index) {
                    self.editor.buffers.close(index);
                    self.git.invalidate();
                } else {
                    self.status = Some(format!("could not write \"{name}\""));
                }
                self.editor.pending_close = None;
            }
            Some(Choice::Discard) => {
                self.editor.buffers.close(index);
                self.editor.pending_close = None;
            }
            Some(Choice::Cancel) => self.editor.pending_close = None,
            None => {}
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
                    let delete =
                        egui::Button::new(egui::RichText::new("Delete").color(egui::Color32::WHITE))
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
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.pending_delete = None;
        }
    }

    /// The search panel: options, query, optional replace, excludes, then the
    /// grouped results — laid out so every field spans the panel exactly.
    fn search_ui(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let mut rerun = false;
        let mut replace_all = false;

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
                    ui.label(
                        egui::RichText::new(text).size(10.0).color(if focused {
                            color(theme.ui.accent_light)
                        } else {
                            color(theme.ui.fg_faint)
                        }),
                    );
                };

                ui.horizontal(|ui| {
                    let query_focused =
                        ui.memory(|m| m.has_focus(field_id(SearchField::Query)));
                    heading(ui, "SEARCH", query_focused);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut toggle = |ui: &mut egui::Ui,
                                          on: &mut bool,
                                          label: &str,
                                          hint: &str| {
                            let text = egui::RichText::new(label)
                                .color(if *on {
                                    color(theme.ui.fg_bright)
                                } else {
                                    color(theme.ui.fg_faint)
                                })
                                .size(11.0);
                            let button = egui::Button::new(text)
                                .frame(true)
                                .fill(if *on {
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
                    let summary = match (&self.search.error, self.search.query.is_empty()) {
                        (Some(error), _) => error.clone(),
                        (None, true) => String::new(),
                        (None, false) => format!(
                            "{}{} results in {} files",
                            self.search.total_matches(),
                            if self.search.truncated { "+" } else { "" },
                            self.search.results.len()
                        ),
                    };
                    let tone = if self.search.error.is_some() {
                        theme.ui.danger
                    } else {
                        theme.ui.fg_faint
                    };
                    ui.label(egui::RichText::new(summary).color(color(tone)).size(11.0));
                    if !self.search.results.is_empty() {
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui
                                    .button(
                                        egui::RichText::new("Replace All")
                                            .color(color(theme.ui.fg))
                                            .size(11.0),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    replace_all = true;
                                }
                            },
                        );
                    }
                });
            });

        if rerun {
            self.search.run(&self.root);
        } else {
            self.search.run_if_changed(&self.root);
        }
        if replace_all {
            match self.search.replace_all(&self.root) {
                Ok((count, files)) => {
                    self.reload_unmodified();
                    self.status = Some(format!("replaced {count} occurrence(s) in {files} file(s)"));
                }
                Err(message) => self.status = Some(message),
            }
        }

        ui.add_space(4.0);
        let mut jump: Option<(PathBuf, usize)> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for file in &self.search.results {
                    let rel = relative_path(&file.path, &self.root);
                    egui::CollapsingHeader::new(
                        egui::RichText::new(rel).color(color(theme.ui.fg)).size(12.5),
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
    }

    /// The find bar above the editor, opened with Cmd+F.
    fn find_bar(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        if !self.editor.find.open {
            return;
        }
        let mut close = false;
        let mut step = 0isize;
        let mut replace_one = false;
        let mut replace_all = false;

        egui::Frame::default()
            .fill(color(theme.ui.status_bg))
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal(|ui| {
                    if disclosure_button(ui, theme, self.editor.find.replace_mode)
                        .on_hover_text("Toggle Replace")
                        .clicked()
                    {
                        self.editor.find.replace_mode = !self.editor.find.replace_mode;
                    }
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.editor.find.query)
                            .desired_width(260.0)
                            .hint_text("Find")
                            .font(egui::TextStyle::Body),
                    );
                    if std::mem::take(&mut self.editor.find.focus_pending) {
                        resp.request_focus();
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        step = 1;
                        self.editor.find.focus_pending = true;
                    }

                    let toggle = |ui: &mut egui::Ui, on: &mut bool, label: &str, hint: &str| {
                        let text = egui::RichText::new(label)
                            .color(if *on {
                                color(theme.ui.fg_bright)
                            } else {
                                color(theme.ui.fg_faint)
                            })
                            .size(11.0);
                        if ui
                            .add(egui::Button::new(text).fill(if *on {
                                color(theme.ui.selected_bg)
                            } else {
                                egui::Color32::TRANSPARENT
                            }))
                            .on_hover_text(hint)
                            .clicked()
                        {
                            *on = !*on;
                        }
                    };
                    toggle(ui, &mut self.editor.find.case_sensitive, "Aa", "Match Case");
                    toggle(ui, &mut self.editor.find.whole_word, "ab", "Match Whole Word");
                    toggle(ui, &mut self.editor.find.regex, ".*", "Use Regular Expression");

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
                    if ui.button(egui::RichText::new("<").size(11.0)).clicked() {
                        step = -1;
                    }
                    if ui.button(egui::RichText::new(">").size(11.0)).clicked() {
                        step = 1;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new("x").size(11.0)).clicked() {
                            close = true;
                        }
                    });
                });

                if self.editor.find.replace_mode {
                    ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.editor.find.replace)
                                .desired_width(260.0)
                                .hint_text("Replace")
                                .font(egui::TextStyle::Body),
                        );
                        if ui
                            .button(egui::RichText::new("Replace").size(11.0))
                            .clicked()
                        {
                            replace_one = true;
                        }
                        if ui
                            .button(egui::RichText::new("Replace All").size(11.0))
                            .clicked()
                        {
                            replace_all = true;
                        }
                    });
                }
            });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if close {
            self.editor.find.open = false;
        }
        if step != 0 {
            self.editor.find_step(step);
        }
        if replace_one {
            self.editor.find_replace_current();
        }
        if replace_all {
            let replaced = self.editor.find_replace_all();
            self.status = Some(format!("replaced {replaced} occurrence(s)"));
        }
    }
}

/// One dropdown row: label on the left, key chord greyed out on the right —
/// the same shape the terminal frontend draws.
fn command_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    shortcut: &str,
) -> egui::Response {
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
        // Every shortcut comes from settings.json, so a rebind takes effect
        // the moment the file is saved.
        let bindings: Vec<(Command, _)> = self
            .settings
            .keys
            .gui
            .iter()
            .filter_map(|(id, chord)| Some((Command::from_id(id)?, chord.clone())))
            .collect();
        for (command, chord) in bindings {
            if keys::consumed(ctx, &chord) {
                self.execute(ctx, command);
            }
        }

        let theme = self.theme().clone();
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
                    let mut open_from_git: Option<PathBuf> = None;
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
                                            self.tree.ui(ui, &theme, &mut events);
                                        });
                                    self.handle_tree_events(events);
                                }
                                SidebarView::Search => self.search_ui(ui, &theme),
                                SidebarView::Git => {
                                    open_from_git = self.git.ui(ui, &theme, &self.root);
                                }
                            }
                        });
                    if let Some(path) = open_from_git {
                        self.tree.reveal(&path);
                        self.editor.open(path);
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
                        let path = relative_path(&buf.path, &self.root);
                        let modified = buf.modified();
                        let resp = ui.label(
                            egui::RichText::new(path)
                                .color(color(theme.ui.fg_dim))
                                .size(11.5),
                        );
                        if modified {
                            let c = egui::pos2(resp.rect.right() + 7.0, resp.rect.center().y);
                            ui.painter().circle_filled(c, 3.0, color(theme.ui.accent_light));
                            ui.add_space(10.0);
                        }
                    }
                    if let Some(message) = &self.status {
                        ui.label(
                            egui::RichText::new(message)
                                .color(color(theme.ui.danger))
                                .size(11.5),
                        );
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
                    let root = self.root.clone();
                    let focused = self.terminal.focused;
                    egui::Frame::default()
                        .fill(color(theme.ui.status_bg))
                        .inner_margin(egui::Margin::symmetric(10, 3))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut text = egui::RichText::new("TERMINAL")
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

        // Tab bar.
        if !self.editor.buffers.is_empty() {
            egui::TopBottomPanel::top("tabs")
                .exact_height(34.0)
                .frame(egui::Frame::default().fill(color(theme.ui.status_bg)))
                .show(ctx, |ui| {
                    self.editor.tab_bar(ui, &theme);
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
                        top: 8,
                        bottom: 0,
                    }),
            )
            .show(ctx, |ui| {
                self.find_bar(ui, &theme);
                let hint = self.empty_hint();
                self.editor.ui(
                    ui,
                    &theme,
                    &self.settings.indent,
                    &self.settings.goto_modifiers.gui,
                    &hint,
                );
            });

        self.handle_goto();
        self.text_prompt_modal(ctx);
        self.recent_modal(ctx);
        self.goto_modal(ctx);
        self.theme_modal(ctx);
        self.delete_modal(ctx);
        self.close_modal(ctx);
    }
}

/// One footer switch — a painted icon plus its label, the same row the
/// terminal frontend draws at the bottom of its sidebar.
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
    let stroke = egui::Stroke::new(1.2, stroke_color);
    let c = egui::pos2(rect.min.x + 6.0 + icon_w / 2.0, rect.center().y);
    let p = ui.painter();
    match view {
        SidebarView::Files => {
            // A file list: three lines.
            for dy in [-3.5, 0.0, 3.5] {
                p.line_segment(
                    [c + egui::vec2(-4.5, dy), c + egui::vec2(4.5, dy)],
                    stroke,
                );
            }
        }
        SidebarView::Search => {
            p.circle_stroke(c + egui::vec2(-1.2, -1.2), 3.6, stroke);
            p.line_segment([c + egui::vec2(1.6, 1.6), c + egui::vec2(4.6, 4.6)], stroke);
        }
        SidebarView::Git => {
            // A branch: the main line with a commit at each end and a short
            // fork peeling off to the right.
            let r = 1.7;
            let top = c + egui::vec2(-2.5, -4.0);
            let bottom = c + egui::vec2(-2.5, 4.0);
            p.circle_stroke(top, r, stroke);
            p.circle_stroke(bottom, r, stroke);
            p.line_segment([top + egui::vec2(0.0, r), bottom - egui::vec2(0.0, r)], stroke);
            p.line_segment(
                [c + egui::vec2(-2.5, -1.0), c + egui::vec2(3.5, -3.2)],
                stroke,
            );
        }
    }
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

/// The little expand/collapse triangle in front of a replace field, painted
/// like the navigator's disclosure marks.
fn disclosure_button(ui: &mut egui::Ui, theme: &Theme, open: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let tone = if resp.hovered() {
            color(theme.ui.fg)
        } else {
            color(theme.ui.fg_dim)
        };
        let c = rect.center();
        let r = 3.4;
        let pts = if open {
            vec![
                c + egui::vec2(-r, -r * 0.6),
                c + egui::vec2(r, -r * 0.6),
                c + egui::vec2(0.0, r * 0.8),
            ]
        } else {
            vec![
                c + egui::vec2(-r * 0.6, -r),
                c + egui::vec2(-r * 0.6, r),
                c + egui::vec2(r * 0.8, 0.0),
            ]
        };
        ui.painter()
            .add(egui::Shape::convex_polygon(pts, tone, egui::Stroke::NONE));
    }
    resp
}

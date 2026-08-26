//! User settings, stored as JSON and editable inside the editor itself.
//!
//! Everything the app actually reads lives here: theme, indentation, the
//! modifier that turns a click into go-to-definition, and the full key map for
//! both frontends. Missing keys fall back to the defaults below, so a partial
//! file is valid.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::command::{Chord, Command, ALL};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndentStyle {
    Spaces,
    Tabs,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Indent {
    pub style: IndentStyle,
    /// Number of spaces per indent level (also the rendered width of a tab).
    pub width: usize,
    /// Infer the unit from the file being edited, falling back to the settings
    /// above when the file gives no signal.
    pub detect_from_file: bool,
}

impl Default for Indent {
    fn default() -> Self {
        Self {
            style: IndentStyle::Spaces,
            width: 4,
            detect_from_file: true,
        }
    }
}

impl Indent {
    /// What the indentation picker offers, in the order it lists them.
    pub const CHOICES: [(&'static str, IndentStyle, usize); 4] = [
        ("Spaces: 2", IndentStyle::Spaces, 2),
        ("Spaces: 4", IndentStyle::Spaces, 4),
        ("Spaces: 8", IndentStyle::Spaces, 8),
        ("Tabs", IndentStyle::Tabs, 4),
    ];

    /// Which of `CHOICES` this is, for a picker to start on.
    pub fn choice_index(&self) -> usize {
        Self::CHOICES
            .iter()
            .position(|(_, style, width)| {
                *style == self.style && (*style == IndentStyle::Tabs || *width == self.width)
            })
            .unwrap_or(1)
    }

    /// How the setting reads in a status bar: "Spaces: 4" or "Tabs".
    pub fn label(&self) -> String {
        match self.style {
            IndentStyle::Tabs => "Tabs".to_string(),
            IndentStyle::Spaces => format!("Spaces: {}", self.width),
        }
    }

    /// The literal string one indent level inserts.
    pub fn unit(&self) -> String {
        match self.style {
            IndentStyle::Tabs => "\t".to_string(),
            IndentStyle::Spaces => " ".repeat(self.width.clamp(1, 16)),
        }
    }
}

/// Modifiers that make a click navigate to a definition. Any one of them is
/// enough, which is why it is a list: terminals differ in which they deliver.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Cmd,
    Ctrl,
    Alt,
    Shift,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GotoModifiers {
    pub gui: Vec<Modifier>,
    pub tui: Vec<Modifier>,
}

impl Default for GotoModifiers {
    fn default() -> Self {
        Self {
            // Terminals cannot report Cmd, so the TUI takes Ctrl or Alt.
            gui: vec![Modifier::Cmd],
            tui: vec![Modifier::Ctrl, Modifier::Alt],
        }
    }
}

/// Modifiers held while the pointer rests on a line to blame that line rather
/// than the one the caret is on. Same shape as [`GotoModifiers`], and the same
/// reason for being a list.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BlameModifiers {
    pub gui: Vec<Modifier>,
    pub tui: Vec<Modifier>,
}

impl Default for BlameModifiers {
    fn default() -> Self {
        // Shift in both: the other three are spoken for by go-to-definition
        // in one frontend or the other.
        Self {
            gui: vec![Modifier::Shift],
            tui: vec![Modifier::Shift],
        }
    }
}

pub type KeyMap = BTreeMap<String, Chord>;

#[derive(Clone, Debug)]
pub struct Keys {
    pub gui: KeyMap,
    pub tui: KeyMap,
    /// Bindings the file spelled wrongly, as `(frontend, command, text)`. They
    /// keep their defaults and are named in the status bar; one typo does not
    /// cost the user the rest of their settings.
    pub rejected: Vec<(&'static str, String, String)>,
}

/// Only bindings that differ from the defaults are written out. Saving the
/// whole map would freeze today's defaults into every user's file, so a later
/// change to a default — or a newly added command — would never reach them.
impl Serialize for Keys {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let defaults = Keys::default();
        let changed = |map: &KeyMap, base: &KeyMap| -> KeyMap {
            map.iter()
                .filter(|(id, chord)| base.get(*id) != Some(chord))
                .map(|(id, chord)| (id.clone(), chord.clone()))
                .collect()
        };
        let mut out = s.serialize_struct("Keys", 2)?;
        out.serialize_field("gui", &changed(&self.gui, &defaults.gui))?;
        out.serialize_field("tui", &changed(&self.tui, &defaults.tui))?;
        out.end()
    }
}

/// Bindings in the file are laid *over* the defaults, so rebinding one key
/// doesn't silently drop every other shortcut.
impl<'de> Deserialize<'de> for Keys {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Read the chords as text and parse them here, so a single mistyped
        // chord loses that one binding rather than the whole settings file.
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Overrides {
            gui: BTreeMap<String, String>,
            tui: BTreeMap<String, String>,
        }
        let overrides = Overrides::deserialize(d)?;
        let mut keys = Keys::default();
        for (frontend, from, into) in [
            ("gui", overrides.gui, &mut keys.gui),
            ("tui", overrides.tui, &mut keys.tui),
        ] {
            for (id, text) in from {
                match text.parse::<Chord>() {
                    Ok(chord) => {
                        into.insert(id, chord);
                    }
                    Err(_) => keys.rejected.push((frontend, id, text)),
                }
            }
        }
        Ok(keys)
    }
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            gui: default_map(gui_default_chord),
            tui: default_map(tui_default_chord),
            rejected: Vec::new(),
        }
    }
}

fn default_map(chord_for: fn(Command) -> Option<&'static str>) -> KeyMap {
    ALL.iter()
        .filter_map(|command| {
            let text = chord_for(*command)?;
            Some((command.id().to_string(), text.parse().ok()?))
        })
        .collect()
}

/// Defaults follow VS Code on macOS, which Zed also broadly matches, so the
/// keys are the ones a hand already knows.
fn gui_default_chord(command: Command) -> Option<&'static str> {
    Some(match command {
        Command::NewFile => "Cmd+N",
        Command::OpenFile => "Cmd+O",
        Command::OpenFolder => "Cmd+Shift+O",
        Command::AddFolder => "Cmd+Shift+A",
        Command::OpenRecent => "Cmd+R",
        Command::Save => "Cmd+S",
        Command::SaveAs => "Cmd+Shift+S",
        Command::SaveAll => "Cmd+Alt+S",
        Command::Settings => "Cmd+,",
        Command::CloseTab => "Cmd+W",
        Command::CloseAllTabs => "Cmd+Shift+W",
        Command::Quit => "Cmd+Q",
        Command::ToggleSidebar => "Cmd+B",
        Command::ToggleTerminal => "Cmd+J",
        Command::NewTerminal => "Cmd+Alt+T",
        Command::CloseTerminal => "Cmd+Alt+W",
        Command::FindInFile => "Cmd+F",
        Command::FocusSearch => "Cmd+Shift+F",
        Command::FocusFiles => "Cmd+Shift+E",
        // VS Code keeps source control on Ctrl+Shift+G even on macOS, which
        // leaves Cmd+Shift+G where it belongs: the previous match.
        Command::FocusGit => "Ctrl+Shift+G",
        Command::PickRepository => "Cmd+Alt+G",
        Command::PickWorktree => "Cmd+Alt+K",
        Command::ThemePicker => "Cmd+Shift+T",
        Command::IndentPicker => "Cmd+Alt+I",
        // VS Code's own key for the markdown preview.
        Command::TogglePreview => "Cmd+Shift+V",
        Command::GotoDefinition => "F12",
        // ⌃- is VS Code's back on macOS; elsewhere it is Alt+Left, as in the
        // terminal frontend.
        Command::GoBack => mac_or("Ctrl+-", "Alt+Left"),
        Command::NewFolder => "Cmd+Alt+N",
        Command::Rename => "F2",
        // Cmd+Backspace deletes to the start of the line in every macOS text
        // field, so it is not the key for deleting a file from under the
        // editor. Shift+Delete is the terminal frontend's, and has no meaning
        // in the text.
        Command::Delete => "Shift+Delete",
        Command::MoveTo => "Cmd+Alt+M",
        // Cmd+G / Cmd+Shift+G on macOS; F3 / Shift+F3 elsewhere, where
        // Ctrl+Shift+G is the Git sidebar and would otherwise collide, since
        // Cmd means Ctrl on those platforms.
        Command::FindNext => mac_or("Cmd+G", "F3"),
        Command::FindPrev => mac_or("Cmd+Shift+G", "Shift+F3"),
        Command::ReplaceAll => "Cmd+Alt+Enter",
        // Panes are switched with the mouse in the window.
        Command::NextPane | Command::PrevPane => return None,
        Command::Undo => "Cmd+Z",
        Command::Redo => "Cmd+Shift+Z",
        // Select all, copy and cut are the text widget's own; binding them
        // here would take them away from it. Paste is bound because the
        // terminal panel needs it for an image — egui delivers pasted *text*
        // as its own event, which this binding does not touch, so a text
        // paste still lands in whichever field is being typed in.
        Command::SelectAll | Command::Copy | Command::Cut => return None,
        Command::Paste => "Cmd+V",
        // VS Code folds with two-key chords (⌘K ⌘0); these are the single-chord
        // shape of the same three actions.
        Command::ToggleFold => "Cmd+Alt+F",
        Command::FoldAll => "Cmd+Alt+0",
        Command::UnfoldAll => "Cmd+Alt+9",
        Command::NextTab => "Ctrl+PageDown",
        Command::PrevTab => "Ctrl+PageUp",
        Command::Help => "F1",
        // VS Code's own three: the palette, the file finder, and go to line.
        Command::CommandPalette => "Cmd+Shift+P",
        Command::QuickOpen => "Cmd+P",
        Command::GotoLine => "Ctrl+G",
        // Driven by the mouse and the menu bar in the window frontend.
        Command::ContextMenu | Command::FileMenu | Command::ViewMenu | Command::HelpMenu => {
            return None
        }
        Command::Documentation => "Cmd+Shift+H",
        // Updating is a menu action; a key for it would only be pressed by
        // accident.
        Command::CheckForUpdates | Command::InstallUpdate => return None,
    })
}

/// The macOS chord, or the one the other platforms use where VS Code differs.
fn mac_or(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

/// The same bindings with Ctrl in place of Cmd, and Ctrl+Shift where VS Code
/// uses Cmd+Shift. Telling Ctrl+Shift+X from Ctrl+X needs the kitty keyboard
/// protocol, which [`crate::tui::run`] asks for; in a terminal without it,
/// rebind those few in `settings.json`.
fn tui_default_chord(command: Command) -> Option<&'static str> {
    Some(match command {
        Command::NewFile => "Ctrl+N",
        Command::OpenFile => "Ctrl+O",
        Command::OpenFolder => "Ctrl+Shift+O",
        Command::AddFolder => "Ctrl+Shift+A",
        Command::OpenRecent => "Ctrl+R",
        Command::Save => "Ctrl+S",
        Command::SaveAs => "Ctrl+Shift+S",
        Command::SaveAll => "Ctrl+Alt+S",
        Command::Settings => "Ctrl+,",
        Command::CloseTab => "Ctrl+W",
        Command::CloseAllTabs => "Ctrl+Shift+W",
        Command::Quit => "Ctrl+Q",
        Command::ToggleSidebar => "Ctrl+B",
        Command::ToggleTerminal => "Ctrl+J",
        Command::NewTerminal => "Ctrl+Alt+T",
        Command::CloseTerminal => "Ctrl+Alt+W",
        Command::FindInFile => "Ctrl+F",
        Command::FocusSearch => "Ctrl+Shift+F",
        Command::FocusFiles => "Ctrl+Shift+E",
        Command::FocusGit => "Ctrl+Shift+G",
        Command::PickRepository => "Ctrl+Alt+G",
        Command::PickWorktree => "Ctrl+Alt+K",
        Command::ThemePicker => "Ctrl+Shift+T",
        Command::IndentPicker => "Ctrl+Alt+I",
        Command::TogglePreview => "Ctrl+Shift+V",
        Command::GotoDefinition => "F12",
        // VS Code's own back on Windows and Linux, where ⌃- does not exist.
        Command::GoBack => "Alt+Left",
        Command::NewFolder => "Ctrl+Alt+N",
        Command::Rename => "F2",
        Command::Delete => "Shift+Delete",
        Command::MoveTo => "Ctrl+Alt+M",
        Command::FindNext => "F3",
        Command::FindPrev => "Shift+F3",
        Command::ReplaceAll => "Ctrl+Alt+Enter",
        Command::NextPane => "Tab",
        Command::PrevPane => "Shift+Tab",
        Command::Undo => "Ctrl+Z",
        Command::Redo => "Ctrl+Shift+Z",
        Command::SelectAll => "Ctrl+A",
        Command::Copy => "Ctrl+C",
        Command::Cut => "Ctrl+X",
        Command::Paste => "Ctrl+V",
        Command::ToggleFold => "Ctrl+Alt+F",
        Command::FoldAll => "Ctrl+Alt+0",
        Command::UnfoldAll => "Ctrl+Alt+9",
        Command::NextTab => "Ctrl+PageDown",
        Command::PrevTab => "Ctrl+PageUp",
        // The keyboard's own menu keys, which no terminal claims.
        Command::ContextMenu => "Shift+F10",
        Command::FileMenu => "F10",
        Command::ViewMenu => "Alt+F10",
        Command::HelpMenu => "Shift+F1",
        Command::Help => "F1",
        Command::Documentation => "Ctrl+Shift+H",
        Command::CheckForUpdates | Command::InstallUpdate => return None,
        Command::CommandPalette => "Ctrl+Shift+P",
        Command::QuickOpen => "Ctrl+P",
        Command::GotoLine => "Ctrl+G",
    })
}

/// The folder a project's own files live in — settings now, and whatever
/// else a project needs to carry with it later.
pub const PROJECT_DIR: &str = ".ycode";

/// What a project file may say. Every field is optional, so a project that
/// only cares about indentation says only that; `recent_projects` is not
/// here on purpose — that is the user's, not the project's.
#[derive(Default, Deserialize)]
#[serde(default)]
struct ProjectOverrides {
    theme: Option<String>,
    indent: Option<Indent>,
    font_size: Option<f32>,
    scroll_speed: Option<f32>,
    show_sidebar: Option<bool>,
    show_terminal: Option<bool>,
    goto_modifiers: Option<GotoModifiers>,
    blame_modifiers: Option<BlameModifiers>,
    keys: Option<Keys>,
}

impl ProjectOverrides {
    fn apply(self, settings: &mut Settings) {
        if let Some(v) = self.theme {
            settings.theme = v;
        }
        if let Some(v) = self.indent {
            settings.indent = v;
        }
        if let Some(v) = self.font_size {
            settings.font_size = v;
        }
        if let Some(v) = self.scroll_speed {
            settings.scroll_speed = v;
        }
        if let Some(v) = self.show_sidebar {
            settings.show_sidebar = v;
        }
        if let Some(v) = self.show_terminal {
            settings.show_terminal = v;
        }
        if let Some(v) = self.goto_modifiers {
            settings.goto_modifiers = v;
        }
        if let Some(v) = self.blame_modifiers {
            settings.blame_modifiers = v;
        }
        if let Some(keys) = self.keys {
            // A project's keys lay over the user's, as the user's lay over
            // the defaults. Keys deserialises over the defaults, so only the
            // entries the file actually named differ from them.
            let defaults = Keys::default();
            for (id, chord) in keys.gui {
                if defaults.gui.get(&id) != Some(&chord) {
                    settings.keys.gui.insert(id, chord);
                }
            }
            for (id, chord) in keys.tui {
                if defaults.tui.get(&id) != Some(&chord) {
                    settings.keys.tui.insert(id, chord);
                }
            }
            settings.keys.rejected.extend(keys.rejected);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Name of the active theme, as shown in the theme picker.
    pub theme: String,
    pub indent: Indent,
    /// Editor font size, window frontend only.
    pub font_size: f32,
    /// How much further the wheel and the trackpad carry than the platform
    /// asks for. 1.0 is the platform's own notch — three lines in a terminal.
    /// The default is half again: a notch of three is barely a move in a long
    /// file, and twice is a page gone by under the hand.
    pub scroll_speed: f32,
    pub show_sidebar: bool,
    pub show_terminal: bool,
    /// Modifier that turns a click into go-to-definition.
    pub goto_modifiers: GotoModifiers,
    /// Modifier held while hovering a line to blame it.
    pub blame_modifiers: BlameModifiers,
    pub keys: Keys,
    /// Recently opened project folders, most recent first.
    pub recent_projects: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "Dark+".to_string(),
            indent: Indent::default(),
            font_size: 13.5,
            scroll_speed: 1.5,
            show_sidebar: true,
            show_terminal: true,
            goto_modifiers: GotoModifiers::default(),
            blame_modifiers: BlameModifiers::default(),
            keys: Keys::default(),
            recent_projects: Vec::new(),
        }
    }
}

/// Serialises one value the way the file writes it. A trait object so the
/// template above can hold values of different types in one closure.
mod erased_json {
    pub trait ToJson {
        fn to_json(&self) -> String;
    }
    impl<T: serde::Serialize> ToJson for T {
        fn to_json(&self) -> String {
            serde_json::to_string_pretty(self).unwrap_or_else(|_| "null".to_string())
        }
    }
}

/// Removes `//` comments so the file can carry them and serde need not know.
/// A `//` inside a string — a URL in a recent project, say — is left alone.
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_string = !in_string;
                out.push(c);
            }
            '\\' if in_string => {
                out.push(c);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '/' if !in_string && chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

impl Settings {
    /// The scroll speed as the frontends use it, held inside sane bounds so a
    /// typo in the file cannot freeze the wheel or send it flying.
    pub fn scroll_factor(&self) -> f32 {
        let speed = self.scroll_speed;
        if speed.is_finite() {
            speed.clamp(0.25, 8.0)
        } else {
            Self::default().scroll_speed
        }
    }

    /// Rows one notch of the wheel moves a pane in the terminal frontend,
    /// which counts in whole rows: the three lines a terminal reports, taken
    /// at that speed and cut to a whole row. The window scales the platform's
    /// own delta instead, and needs no such cut.
    pub fn scroll_rows(&self) -> isize {
        ((3.0 * self.scroll_factor()) as isize).max(1)
    }

    /// `$XDG_CONFIG_HOME/yara/settings.json`, else `~/.config/...`.
    pub fn path() -> Option<PathBuf> {
        Some(crate::core::config_dir()?.join("settings.json"))
    }

    /// Loads the settings file, falling back to defaults for anything missing
    /// or malformed. Returns the settings and a message if the file was bad, so
    /// the caller can show it instead of silently ignoring a typo.
    pub fn load() -> (Self, Option<String>) {
        Self::load_for(None)
    }

    /// The global file, with a project's own `.ycode/settings.json` laid over
    /// it when a project is open. A project can pin its indentation or its
    /// theme without touching the user's defaults; keys merge the same way
    /// the global file merges over the built-in defaults.
    pub fn load_for(root: Option<&Path>) -> (Self, Option<String>) {
        let mut complaint = None;
        let mut settings = match Self::path().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => match serde_json::from_str::<Self>(&strip_comments(&text)) {
                Ok(settings) => settings,
                Err(e) => {
                    complaint = Some(format!("settings.json ignored: {e}"));
                    Self::default()
                }
            },
            None => Self::default(),
        };
        if let Some(project) = root.map(Self::project_path) {
            if let Ok(text) = std::fs::read_to_string(&project) {
                match serde_json::from_str::<ProjectOverrides>(&strip_comments(&text)) {
                    Ok(overrides) => overrides.apply(&mut settings),
                    Err(e) => {
                        complaint = Some(format!(".ycode/settings.json ignored: {e}"));
                    }
                }
            }
        }
        if complaint.is_none() {
            complaint = settings.binding_complaint();
        }
        (settings, complaint)
    }

    /// Whether `path` is a settings file the editor reads — the user's own or
    /// a project's — so a save to it can take effect on the spot.
    pub fn is_settings_file(path: &Path, root: Option<&Path>) -> bool {
        Self::path().as_deref() == Some(path) || root.is_some_and(|r| Self::project_path(r) == path)
    }

    /// Where a project keeps its own settings.
    pub fn project_path(root: &Path) -> PathBuf {
        root.join(PROJECT_DIR).join("settings.json")
    }

    /// What is wrong with the bindings the file gave us, if anything: a chord
    /// that could not be read, or one handed to two commands at once. Either
    /// way the editor keeps working — this is what it says about it.
    fn binding_complaint(&self) -> Option<String> {
        if let Some((frontend, id, text)) = self.keys.rejected.first() {
            return Some(format!(
                "settings.json: {frontend} {id} is not a key chord: \"{text}\" — keeping the default"
            ));
        }
        self.clashing_binding()
    }

    /// The first chord bound to two commands, if the file rebound one onto
    /// another. Only one of them could ever run, so it is worth saying.
    fn clashing_binding(&self) -> Option<String> {
        for (frontend, map) in [("gui", &self.keys.gui), ("tui", &self.keys.tui)] {
            let mut seen: BTreeMap<String, &str> = BTreeMap::new();
            for (id, chord) in map {
                let text = chord.to_string();
                if let Some(other) = seen.insert(text.clone(), id) {
                    return Some(format!(
                        "settings.json: {frontend} {text} is bound to both {other} and {id}"
                    ));
                }
            }
        }
        None
    }

    /// Writes the global file: every setting, each with a line saying what it
    /// is for, so the file explains itself when opened.
    pub fn save(&self) -> std::io::Result<PathBuf> {
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no config directory")
        })?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, self.to_commented_json())?;
        Ok(path)
    }

    /// The settings as JSON with a comment over each key. The values are what
    /// serde would write; only the words around them are added.
    pub fn to_commented_json(&self) -> String {
        let json = |value: &dyn erased_json::ToJson| value.to_json();
        let themes = crate::core::theme::builtin()
            .iter()
            .map(|t| format!("\"{}\"", t.name))
            .collect::<Vec<_>>()
            .join(", ");
        let nested = |text: String| text.replace('\n', "\n  ");
        let reference = Self::keys_reference();
        format!(
            r#"// Yara Code settings. Every key is optional: leave one out and the built-in
// default applies. Lines starting with // are comments and are kept when the
// editor writes the file. A project can override any of these from its own
// .ycode/settings.json.
{{
  // Colour theme: {themes}. Also View → Theme.
  "theme": {theme},

  // Editor font size in points. Window frontend only: the terminal frontend
  // draws in whatever font the terminal itself uses.
  "font_size": {font_size},

  // How much further the wheel and the trackpad carry than the platform asks
  // for. 1.0 is the platform's own notch — three lines in a terminal — so 1.5
  // moves four rows where a terminal would move three.
  "scroll_speed": {scroll_speed},

  "indent": {{
    // "spaces" or "tabs". Also View → Indentation.
    "style": {indent_style},
    // Spaces per level, and how wide a tab is drawn.
    "width": {indent_width},
    // Follow the indentation a file already uses, falling back to the above.
    "detect_from_file": {detect_from_file}
  }},

  // Panels open at start. View → Toggle Sidebar / Toggle Terminal.
  "show_sidebar": {show_sidebar},
  "show_terminal": {show_terminal},

  // Modifier held while clicking an identifier to jump to its definition,
  // per frontend: any of "cmd", "ctrl", "alt", "shift". A terminal cannot see
  // Cmd, so the tui list is what the terminal frontend uses.
  "goto_modifiers": {goto_modifiers},

  // Modifier held while the pointer rests on a line to read who last touched
  // it, in the status bar, without moving the caret there. Same choices as
  // above; a terminal cannot see Cmd.
  "blame_modifiers": {blame_modifiers},

  // Key bindings per frontend, command id to chord. Only bindings that
  // differ from the defaults need listing, for example
  //   "keys": {{ "gui": {{ "save": "Cmd+S" }}, "tui": {{ "save": "Ctrl+S" }} }}
  // Chords are written Cmd/Ctrl/Alt/Shift + a key: "Cmd+Shift+F", "Ctrl+-",
  // "Alt+Left", "F12". The commands and their defaults, window then terminal
  // (see also {docs}guides/keys/):
{reference}
  "keys": {keys},

  // Folders offered by File → Open Recent, newest first. Kept by the editor.
  "recent_projects": {recent_projects}
}}
"#,
            theme = json(&self.theme),
            font_size = json(&self.font_size),
            scroll_speed = json(&self.scroll_speed),
            indent_style = json(&self.indent.style),
            indent_width = json(&self.indent.width),
            detect_from_file = json(&self.indent.detect_from_file),
            show_sidebar = json(&self.show_sidebar),
            show_terminal = json(&self.show_terminal),
            goto_modifiers = nested(json(&self.goto_modifiers)),
            blame_modifiers = nested(json(&self.blame_modifiers)),
            keys = nested(json(&self.keys)),
            recent_projects = nested(json(&self.recent_projects)),
            docs = crate::core::DOCUMENTATION,
            reference = reference,
        )
    }

    /// Every command with its default chord in each frontend, as comment
    /// lines for the settings file — so a rebind starts from what is there
    /// rather than from the documentation.
    fn keys_reference() -> String {
        let id_width = ALL.iter().map(|c| c.id().len()).max().unwrap_or(0);
        let gui_width = ALL
            .iter()
            .map(|c| gui_default_chord(*c).map_or(1, str::len))
            .max()
            .unwrap_or(0);
        ALL.iter()
            .map(|command| {
                format!(
                    "  //   {:id_width$}  {:gui_width$}  {}",
                    command.id(),
                    gui_default_chord(*command).unwrap_or("—"),
                    tui_default_chord(*command).unwrap_or("—"),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// When the files the settings come from were last written — the global
    /// one and the project's — so a frontend can notice an edit made outside
    /// the editor and apply it without being told.
    pub fn stamp(root: Option<&Path>) -> Vec<Option<std::time::SystemTime>> {
        let mut paths = vec![Self::path()];
        paths.push(root.map(Self::project_path));
        paths
            .into_iter()
            .map(|path| path.and_then(|p| std::fs::metadata(p).ok()?.modified().ok()))
            .collect()
    }

    /// Path to the settings file, writing the current values first if it does
    /// not exist yet — so "Settings" always opens something real.
    pub fn ensure_file(&self) -> std::io::Result<PathBuf> {
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no config directory")
        })?;
        if !path.exists() {
            return self.save();
        }
        Ok(path)
    }

    /// Records a project in the recent list, newest first, capped at 15.
    pub fn push_recent(&mut self, root: &Path) {
        self.recent_projects.retain(|p| p != root);
        self.recent_projects.insert(0, root.to_path_buf());
        self.recent_projects.truncate(15);
    }

    pub fn gui_chord(&self, command: Command) -> Option<&Chord> {
        self.keys.gui.get(command.id())
    }

    pub fn tui_chord(&self, command: Command) -> Option<&Chord> {
        self.keys.tui.get(command.id())
    }

    /// The command a chord is bound to in the window frontend.
    pub fn gui_command(&self, chord: &Chord) -> Option<Command> {
        lookup(&self.keys.gui, chord)
    }

    pub fn tui_command(&self, chord: &Chord) -> Option<Command> {
        lookup(&self.keys.tui, chord)
    }
}

fn lookup(map: &KeyMap, chord: &Chord) -> Option<Command> {
    map.iter()
        .find(|(_, bound)| *bound == chord)
        .and_then(|(id, _)| Command::from_id(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_keeps_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"indent":{"width":2}}"#).unwrap();
        assert_eq!(settings.indent.width, 2);
        assert_eq!(settings.indent.style, IndentStyle::Spaces);
        assert_eq!(settings.theme, "Dark+");
        assert!(!settings.keys.tui.is_empty());
    }

    #[test]
    fn indent_unit_follows_style() {
        let mut indent = Indent {
            style: IndentStyle::Spaces,
            width: 2,
            detect_from_file: false,
        };
        assert_eq!(indent.unit(), "  ");
        indent.style = IndentStyle::Tabs;
        assert_eq!(indent.unit(), "\t");
    }

    #[test]
    fn chords_resolve_back_to_commands() {
        let settings = Settings::default();
        let save = settings.tui_chord(Command::Save).unwrap().clone();
        assert_eq!(settings.tui_command(&save), Some(Command::Save));
        let gui_save = settings.gui_chord(Command::Save).unwrap().clone();
        assert_eq!(settings.gui_command(&gui_save), Some(Command::Save));
    }

    #[test]
    fn defaults_bind_no_chord_twice_per_frontend() {
        let settings = Settings::default();
        for map in [&settings.keys.gui, &settings.keys.tui] {
            let mut seen: Vec<String> = map.values().map(|c| c.to_string()).collect();
            seen.sort();
            let total = seen.len();
            seen.dedup();
            assert_eq!(seen.len(), total, "duplicate chord in defaults");
        }
    }

    #[test]
    fn saved_settings_only_record_changed_bindings() {
        let mut settings = Settings::default();
        settings
            .keys
            .tui
            .insert("save".into(), "Ctrl+D".parse().unwrap());
        let json = serde_json::to_string(&settings).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let tui = value["keys"]["tui"].as_object().unwrap();
        assert_eq!(tui.len(), 1, "only the rebound key is written: {tui:?}");
        assert_eq!(tui["save"], "Ctrl+D");
        assert!(value["keys"]["gui"].as_object().unwrap().is_empty());

        // Reading it back still yields the full map.
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.tui_chord(Command::Save).unwrap().to_string(),
            "Ctrl+D"
        );
        assert_eq!(
            restored.tui_chord(Command::Quit).unwrap().to_string(),
            "Ctrl+Q"
        );
    }

    #[test]
    fn every_command_is_bound_out_of_the_box() {
        // The window leaves selection, copy and cut to the text widget and
        // opens its menus with the mouse; everything else answers to a key in
        // both.
        let widget_owned = [
            Command::SelectAll,
            Command::Copy,
            Command::Cut,
            Command::ContextMenu,
            Command::FileMenu,
            Command::ViewMenu,
            Command::HelpMenu,
            Command::NextPane,
            Command::PrevPane,
            Command::CheckForUpdates,
            Command::InstallUpdate,
        ];
        // The terminal has no font of its own to scale, and neither frontend
        // has a documentation page to open yet.
        let unbound_in_tui = [Command::CheckForUpdates, Command::InstallUpdate];
        for command in ALL {
            assert!(
                unbound_in_tui.contains(command) || tui_default_chord(*command).is_some(),
                "{} has no terminal binding",
                command.id()
            );
            if widget_owned.contains(command) {
                continue;
            }
            assert!(
                gui_default_chord(*command).is_some(),
                "{} has no window binding",
                command.id()
            );
        }
    }

    #[test]
    fn no_two_commands_share_a_chord() {
        let settings = Settings::default();
        for (name, map) in [
            ("window", &settings.keys.gui),
            ("terminal", &settings.keys.tui),
        ] {
            let mut seen: Vec<(String, String)> = Vec::new();
            for (id, chord) in map {
                let text = chord.to_string();
                if let Some((other, _)) = seen.iter().find(|(_, c)| *c == text) {
                    panic!("{name}: {id} and {other} are both bound to {text}");
                }
                seen.push((id.clone(), text));
            }
        }
    }

    #[test]
    fn rebinding_one_key_keeps_the_rest() {
        let settings: Settings =
            serde_json::from_str(r#"{"keys":{"tui":{"save":"Ctrl+D"}}}"#).unwrap();
        let save = settings.tui_chord(Command::Save).unwrap();
        assert_eq!(save.to_string(), "Ctrl+D");
        // Everything else still has its default.
        assert_eq!(
            settings.tui_chord(Command::Quit).unwrap().to_string(),
            "Ctrl+Q"
        );
        assert_eq!(
            settings.gui_chord(Command::Save).unwrap().to_string(),
            "Cmd+S"
        );
    }

    #[test]
    fn recent_projects_are_deduped_newest_first() {
        let mut settings = Settings::default();
        settings.push_recent(Path::new("/a"));
        settings.push_recent(Path::new("/b"));
        settings.push_recent(Path::new("/a"));
        assert_eq!(
            settings.recent_projects,
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn the_scroll_speed_is_a_multiple_of_the_platforms_own_notch() {
        let at = |speed: f32| {
            Settings {
                scroll_speed: speed,
                ..Default::default()
            }
            .scroll_rows()
        };
        // One notch is what a terminal itself reports; the default is a row
        // over that, and the setting moves it either way.
        assert_eq!(at(1.0), 3);
        assert_eq!(Settings::default().scroll_rows(), 4);
        assert_eq!(at(2.0), 6);
        // A wheel that cannot move is not a setting, and neither is one that
        // clears the file in a notch.
        assert_eq!(at(0.0), 1);
        assert_eq!(at(-5.0), 1);
        assert_eq!(at(1000.0), 24);
        assert_eq!(at(f32::NAN), Settings::default().scroll_rows());
    }

    #[test]
    fn blaming_and_going_to_a_definition_never_want_the_same_modifier() {
        let settings = Settings::default();
        for (blame, goto) in [
            (&settings.blame_modifiers.gui, &settings.goto_modifiers.gui),
            (&settings.blame_modifiers.tui, &settings.goto_modifiers.tui),
        ] {
            assert!(!blame.is_empty(), "a modifier that is nothing blames never");
            for held in blame {
                assert!(
                    !goto.contains(held),
                    "{held:?} would blame and jump at once"
                );
            }
        }
    }

    #[test]
    fn every_field_survives_a_round_trip() {
        let mut settings = Settings {
            theme: "Monokai".into(),
            font_size: 15.0,
            scroll_speed: 2.5,
            show_terminal: false,
            blame_modifiers: BlameModifiers {
                gui: vec![Modifier::Alt],
                tui: vec![Modifier::Alt],
            },
            indent: Indent {
                width: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        settings.push_recent(Path::new("/work/one"));
        let text = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.theme, "Monokai");
        assert_eq!(back.font_size, 15.0);
        assert_eq!(back.scroll_speed, 2.5);
        assert_eq!(back.blame_modifiers.tui, [Modifier::Alt]);
        assert!(!back.show_terminal);
        assert_eq!(back.indent.width, 2);
        assert_eq!(back.recent_projects, [PathBuf::from("/work/one")]);
    }

    #[test]
    fn an_empty_file_still_gives_every_default() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.theme, Settings::default().theme);
        assert!(settings.gui_chord(Command::Save).is_some());
        assert!(settings.tui_chord(Command::Quit).is_some());
    }

    #[test]
    fn a_more_specific_chord_is_never_shadowed() {
        let settings = Settings::default();
        // Cmd+Shift+F is Search, not Find in File with a stray Shift; the
        // terminal frontend matches chords exactly, and the window sorts its
        // bindings so the specific one is offered first.
        let search: Chord = "Cmd+Shift+F".parse().unwrap();
        assert_eq!(settings.gui_command(&search), Some(Command::FocusSearch));
        let find: Chord = "Cmd+F".parse().unwrap();
        assert_eq!(settings.gui_command(&find), Some(Command::FindInFile));

        let save_as: Chord = "Ctrl+Shift+S".parse().unwrap();
        assert_eq!(settings.tui_command(&save_as), Some(Command::SaveAs));
        let save: Chord = "Ctrl+S".parse().unwrap();
        assert_eq!(settings.tui_command(&save), Some(Command::Save));
    }

    #[test]
    fn a_chord_resolves_to_its_command_and_back() {
        let settings = Settings::default();
        let save = settings.gui_chord(Command::Save).unwrap().clone();
        assert_eq!(settings.gui_command(&save), Some(Command::Save));
        let quit = settings.tui_chord(Command::Quit).unwrap().clone();
        assert_eq!(settings.tui_command(&quit), Some(Command::Quit));
        // A chord nobody claimed is nobody's.
        let unbound: Chord = "Ctrl+Alt+Shift+F9".parse().unwrap();
        assert_eq!(settings.gui_command(&unbound), None);
        assert_eq!(settings.tui_command(&unbound), None);
    }

    #[test]
    fn a_binding_the_file_gives_to_two_commands_is_reported() {
        let clashing: Settings =
            serde_json::from_str(r#"{"keys":{"tui":{"file_menu":"Ctrl+X"}}}"#).unwrap();
        let message = clashing.clashing_binding().expect("Ctrl+X is cut's");
        assert!(message.contains("Ctrl+X"), "{message}");
        assert!(
            message.contains("cut") && message.contains("file_menu"),
            "{message}"
        );
        // The defaults themselves never clash.
        assert_eq!(Settings::default().clashing_binding(), None);
    }

    #[test]
    fn an_unparsable_chord_is_skipped_rather_than_fatal() {
        let settings: Settings =
            serde_json::from_str(r#"{"keys":{"gui":{"save":"Cmd+Shift+S","quit":"Ctrl+"}}}"#)
                .unwrap();
        assert_eq!(
            settings.gui_chord(Command::Save).unwrap().to_string(),
            "Cmd+Shift+S"
        );
        // The broken one keeps its default rather than unbinding the command.
        assert_eq!(
            settings.gui_chord(Command::Quit).unwrap().to_string(),
            "Cmd+Q"
        );
    }

    #[test]
    fn the_recent_list_is_newest_first_and_capped() {
        let mut settings = Settings::default();
        for i in 0..20 {
            settings.push_recent(&PathBuf::from(format!("/p/{i}")));
        }
        assert_eq!(settings.recent_projects.len(), 15);
        assert_eq!(settings.recent_projects[0], PathBuf::from("/p/19"));
        // Opening one again moves it to the front instead of duplicating it.
        settings.push_recent(&PathBuf::from("/p/10"));
        assert_eq!(settings.recent_projects[0], PathBuf::from("/p/10"));
        assert_eq!(
            settings
                .recent_projects
                .iter()
                .filter(|p| *p == &PathBuf::from("/p/10"))
                .count(),
            1
        );
    }

    #[test]
    fn the_indent_unit_reads_as_what_it_inserts() {
        let spaces = Indent {
            style: IndentStyle::Spaces,
            width: 3,
            detect_from_file: true,
        };
        assert_eq!(spaces.unit(), "   ");
        let tabs = Indent {
            style: IndentStyle::Tabs,
            width: 4,
            detect_from_file: false,
        };
        assert_eq!(tabs.unit(), "\t");
    }

    #[test]
    fn go_to_definition_modifiers_are_read_per_frontend() {
        let settings: Settings =
            serde_json::from_str(r#"{"goto_modifiers":{"gui":["cmd"],"tui":["ctrl","alt"]}}"#)
                .unwrap();
        assert_eq!(settings.goto_modifiers.gui, [Modifier::Cmd]);
        assert_eq!(settings.goto_modifiers.tui, [Modifier::Ctrl, Modifier::Alt]);
    }

    #[test]
    fn a_project_file_lays_over_the_global_one() {
        let dir = crate::core::test_support::Dir::new("yara-project-settings");
        // Read against an empty global file, not whatever the machine has.
        let _lock = crate::core::test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("YARA_CONFIG_DIR", dir.path().join("config"));
        std::fs::create_dir_all(dir.path().join(PROJECT_DIR)).unwrap();
        std::fs::write(
            Settings::project_path(dir.path()),
            r#"{"indent":{"style":"tabs","width":8},"keys":{"tui":{"save":"Ctrl+D"}}}"#,
        )
        .unwrap();
        let (settings, complaint) = Settings::load_for(Some(dir.path()));
        assert_eq!(complaint, None);
        assert_eq!(settings.indent.style, IndentStyle::Tabs);
        assert_eq!(settings.indent.width, 8);
        assert_eq!(
            settings.tui_chord(Command::Save).unwrap().to_string(),
            "Ctrl+D"
        );
        // What the project did not mention keeps the user's value.
        assert_eq!(
            settings.tui_chord(Command::Quit).unwrap().to_string(),
            "Ctrl+Q"
        );
        assert_eq!(settings.theme, Settings::default().theme);
        std::env::remove_var("YARA_CONFIG_DIR");
    }

    #[test]
    fn a_broken_project_file_is_reported_and_the_rest_still_loads() {
        let dir = crate::core::test_support::Dir::new("yara-project-settings-bad");
        let _lock = crate::core::test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("YARA_CONFIG_DIR", dir.path().join("config"));
        std::fs::create_dir_all(dir.path().join(PROJECT_DIR)).unwrap();
        std::fs::write(Settings::project_path(dir.path()), "{not json").unwrap();
        let (settings, complaint) = Settings::load_for(Some(dir.path()));
        assert!(complaint.unwrap().contains(".ycode/settings.json ignored"));
        assert!(
            settings.gui_chord(Command::Save).is_some(),
            "defaults survive"
        );
        // No project file at all is simply the global settings.
        let (_, none) = Settings::load_for(Some(Path::new("/nowhere/at/all")));
        assert_eq!(none, None);
        std::env::remove_var("YARA_CONFIG_DIR");
    }

    #[test]
    fn the_settings_file_lives_under_the_editor_s_own_config_directory() {
        // Another test may be pointing the directory elsewhere; wait for it.
        let _lock = crate::core::test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = Settings::path().expect("a config directory on every platform");
        assert!(
            path.ends_with("yara-code/settings.json") || path.ends_with("yara-code\\settings.json"),
            "{}",
            path.display()
        );
    }

    #[test]
    fn comments_are_stripped_outside_strings_only() {
        let text = "{\n  // a note\n  \"theme\": \"Dark+\", // trailing\n  \"recent_projects\": [\"http://x//y\", \"a\\\"//b\"]\n}\n";
        let stripped = strip_comments(text);
        let value: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(value["theme"], "Dark+");
        assert_eq!(value["recent_projects"][0], "http://x//y");
        assert_eq!(value["recent_projects"][1], "a\"//b");
    }

    #[test]
    fn the_written_file_explains_itself_and_reads_back_whole() {
        let mut settings = Settings {
            theme: "Monokai".into(),
            font_size: 17.0,
            ..Default::default()
        };
        settings
            .keys
            .gui
            .insert("save".into(), "Cmd+Shift+S".parse().unwrap());
        settings.recent_projects.push(PathBuf::from("/tmp/p"));
        let text = settings.to_commented_json();
        // Every top-level key is there with a comment somewhere above it.
        for key in [
            "\"theme\"",
            "\"font_size\"",
            "\"scroll_speed\"",
            "\"indent\"",
            "\"show_sidebar\"",
            "\"show_terminal\"",
            "\"goto_modifiers\"",
            "\"blame_modifiers\"",
            "\"keys\"",
            "\"recent_projects\"",
        ] {
            let at = text
                .find(key)
                .unwrap_or_else(|| panic!("{key} missing:\n{text}"));
            assert!(text[..at].contains("//"), "{key} has no comment");
        }
        assert!(text.contains("Monokai"), "the theme names are listed");
        // Every command is named over the keys, with both of its defaults.
        for command in ALL {
            let line = text
                .lines()
                .find(|l| l.starts_with("  //") && l.contains(command.id()))
                .unwrap_or_else(|| panic!("{} missing from the reference", command.id()));
            if let Some(chord) = tui_default_chord(*command) {
                assert!(line.contains(chord), "{line}");
            }
        }
        let back: Settings = serde_json::from_str(&strip_comments(&text)).unwrap();
        assert_eq!(back.theme, "Monokai");
        assert_eq!(back.font_size, 17.0);
        assert_eq!(
            back.keys.gui.get("save").unwrap().to_string(),
            "Cmd+Shift+S"
        );
        assert_eq!(back.recent_projects, vec![PathBuf::from("/tmp/p")]);
    }

    #[test]
    fn a_commented_file_on_disk_loads_and_a_stamp_moves_when_it_is_written() {
        let dir = crate::core::test_support::Dir::new("yara-settings-comments");
        let lock = crate::core::test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("YARA_CONFIG_DIR", dir.path());
        let before = Settings::stamp(None);
        assert_eq!(before, vec![None, None], "nothing written yet");
        let settings = Settings {
            theme: "Light+".into(),
            ..Default::default()
        };
        settings.save().unwrap();
        let (loaded, complaint) = Settings::load();
        assert_eq!(complaint, None);
        assert_eq!(loaded.theme, "Light+");
        let after = Settings::stamp(None);
        assert!(after[0].is_some() && after != before);
        std::env::remove_var("YARA_CONFIG_DIR");
        drop(lock);
    }
}

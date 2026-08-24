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
        Command::CloseEditor => "Cmd+W",
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
        Command::GotoDefinition => "F12",
        Command::GoBack => "Ctrl+-",
        Command::NewFolder => "Cmd+Alt+N",
        Command::Rename => "F2",
        Command::Delete => "Cmd+Backspace",
        Command::MoveTo => "Cmd+Alt+M",
        Command::FindNext => "Cmd+G",
        Command::FindPrev => "Cmd+Shift+G",
        Command::ReplaceAll => "Cmd+Alt+Enter",
        // Panes are switched with the mouse in the window.
        Command::NextPane | Command::PrevPane => return None,
        Command::Undo => "Cmd+Z",
        Command::Redo => "Cmd+Shift+Z",
        // Select all, copy, cut and paste are the text widget's own; binding
        // them here would take them away from it.
        Command::SelectAll | Command::Copy | Command::Cut | Command::Paste => return None,
        // VS Code folds with two-key chords (⌘K ⌘0); these are the single-chord
        // shape of the same three actions.
        Command::ToggleFold => "Cmd+Alt+F",
        Command::FoldAll => "Cmd+Alt+0",
        Command::UnfoldAll => "Cmd+Alt+9",
        Command::NextTab => "Ctrl+PageDown",
        Command::PrevTab => "Ctrl+PageUp",
        Command::Help => "F1",
        Command::ZoomIn => "Cmd+=",
        Command::ZoomOut => "Cmd+-",
        Command::ResetZoom => "Cmd+0",
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
        Command::CloseEditor => "Ctrl+W",
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
        // The terminal owns its own font size.
        Command::ZoomIn | Command::ZoomOut | Command::ResetZoom => return None,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Name of the active theme, as shown in the theme picker.
    pub theme: String,
    pub indent: Indent,
    /// Editor font size, window frontend only.
    pub font_size: f32,
    pub show_sidebar: bool,
    pub show_terminal: bool,
    /// Modifier that turns a click into go-to-definition.
    pub goto_modifiers: GotoModifiers,
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
            show_sidebar: true,
            show_terminal: true,
            goto_modifiers: GotoModifiers::default(),
            keys: Keys::default(),
            recent_projects: Vec::new(),
        }
    }
}

impl Settings {
    /// `$XDG_CONFIG_HOME/yara/settings.json`, else `~/.config/...`.
    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("yara-code").join("settings.json"))
    }

    /// Loads the settings file, falling back to defaults for anything missing
    /// or malformed. Returns the settings and a message if the file was bad, so
    /// the caller can show it instead of silently ignoring a typo.
    pub fn load() -> (Self, Option<String>) {
        let Some(path) = Self::path() else {
            return (Self::default(), None);
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (Self::default(), None);
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(settings) => {
                let complaint = settings.binding_complaint();
                (settings, complaint)
            }
            Err(e) => (Self::default(), Some(format!("settings.json ignored: {e}"))),
        }
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

    pub fn save(&self) -> std::io::Result<PathBuf> {
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no config directory")
        })?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, format!("{text}\n"))?;
        Ok(path)
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
        // The window leaves the clipboard keys to the text widget and opens its
        // menus with the mouse; everything else answers to a key in both.
        let widget_owned = [
            Command::SelectAll,
            Command::Copy,
            Command::Cut,
            Command::Paste,
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
        let unbound_in_tui = [
            Command::ZoomIn,
            Command::ZoomOut,
            Command::ResetZoom,
            Command::CheckForUpdates,
            Command::InstallUpdate,
        ];
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
    fn every_field_survives_a_round_trip() {
        let mut settings = Settings {
            theme: "Monokai".into(),
            font_size: 15.0,
            show_terminal: false,
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
    fn the_settings_file_lives_under_the_editor_s_own_config_directory() {
        let path = Settings::path().expect("a config directory on every platform");
        assert!(
            path.ends_with("yara-code/settings.json") || path.ends_with("yara-code\\settings.json")
        );
    }
}

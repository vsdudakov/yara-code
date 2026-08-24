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
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Overrides {
            gui: KeyMap,
            tui: KeyMap,
        }
        let overrides = Overrides::deserialize(d)?;
        let mut keys = Keys::default();
        keys.gui.extend(overrides.gui);
        keys.tui.extend(overrides.tui);
        Ok(keys)
    }
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            gui: default_map(gui_default_chord),
            tui: default_map(tui_default_chord),
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

fn gui_default_chord(command: Command) -> Option<&'static str> {
    Some(match command {
        Command::NewFile => "Cmd+N",
        Command::OpenFile => "Cmd+O",
        Command::OpenFolder => "Cmd+Shift+O",
        Command::AddFolder => "Cmd+Shift+A",
        Command::OpenRecent => "Cmd+Alt+O",
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
        Command::FocusGit => "Cmd+Shift+G",
        Command::ThemePicker => "Cmd+Shift+T",
        Command::GoBack => "Ctrl+-",
        // Select all, copy, cut and paste are the text widget's own; binding
        // them here would take them away from it.
        Command::SelectAll | Command::Copy | Command::Cut | Command::Paste => return None,
        Command::ToggleFold => "Cmd+Alt+F",
        Command::FoldAll => "Cmd+Alt+0",
        Command::UnfoldAll => "Cmd+Alt+9",
        Command::NextTab => "Alt+Right",
        Command::PrevTab => "Alt+Left",
        // Driven by the mouse and the menu bar in the window frontend.
        Command::GotoDefinition | Command::ContextMenu | Command::FileMenu | Command::Help => {
            return None
        }
    })
}

/// Terminals cannot tell `Ctrl+Shift+X` from `Ctrl+X` unless they speak the
/// kitty keyboard protocol, so nothing here relies on that distinction — the
/// second tier of shortcuts uses Alt instead.
fn tui_default_chord(command: Command) -> Option<&'static str> {
    Some(match command {
        Command::NewFile => "Ctrl+N",
        Command::OpenFile => "Ctrl+O",
        Command::OpenFolder => "Alt+O",
        Command::AddFolder => "Alt+P",
        Command::OpenRecent => "Ctrl+R",
        Command::Save => "Ctrl+S",
        Command::SaveAs => "Alt+S",
        Command::SaveAll => "Alt+A",
        Command::Settings => "Ctrl+P",
        Command::CloseEditor => "Ctrl+W",
        Command::Quit => "Ctrl+Q",
        Command::ToggleSidebar => "Ctrl+B",
        Command::ToggleTerminal => "Ctrl+J",
        Command::NewTerminal => "Alt+T",
        Command::CloseTerminal => "Alt+W",
        Command::FindInFile => "Ctrl+F",
        Command::FocusSearch => "Alt+F",
        Command::FocusFiles => "Ctrl+E",
        Command::FocusGit => "Alt+G",
        Command::ThemePicker => "Ctrl+T",
        Command::GotoDefinition => "Ctrl+G",
        Command::GoBack => "Ctrl+Y",
        Command::SelectAll => "Ctrl+A",
        Command::Copy => "Ctrl+C",
        Command::Cut => "Ctrl+X",
        Command::Paste => "Ctrl+V",
        Command::ToggleFold => "Alt+Z",
        Command::FoldAll => "Alt+0",
        Command::UnfoldAll => "Alt+9",
        Command::NextTab => "Alt+Right",
        Command::PrevTab => "Alt+Left",
        Command::ContextMenu => "Ctrl+K",
        Command::FileMenu => "Alt+M",
        Command::Help => "Ctrl+H",
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
        Some(base.join("yara").join("settings.json"))
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
            Ok(settings) => (settings, None),
            Err(e) => (
                Self::default(),
                Some(format!("settings.json ignored: {e}")),
            ),
        }
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
    fn terminal_defaults_avoid_ctrl_shift() {
        // Ordinary terminals deliver Ctrl+Shift+F as plain Ctrl+F, so such a
        // binding would be unreachable and would shadow another command.
        let settings = Settings::default();
        for (id, chord) in &settings.keys.tui {
            assert!(
                !(chord.mods.ctrl && chord.mods.shift),
                "{id} is bound to {chord}, which most terminals cannot report"
            );
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

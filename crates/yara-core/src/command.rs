//! Every action the frontend can be asked to perform, plus the key chords
//! bound to it. Key events are translated into a [`Command`] through the map
//! in `settings.json`, so a rebind moves the action with it.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    // File.
    NewFile,
    OpenFolder,
    AddFolder,
    OpenRecent,
    Save,
    Settings,
    Quit,
    // Help.
    Documentation,
    Help,
    CheckForUpdates,
    InstallUpdate,
    // Panes and overlays.
    ToggleSidebar,
    Changes,
    CommandPalette,
    SearchProject,
    AgentUsage,
    ThemePicker,
    QuickOpen,
    FileMenu,
    HelpMenu,
    NextPane,
    SwapPanes,
    Close,
    // Tasks: one agent and the folders it works in, a tab apiece.
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    RenameTab,
    // The editor.
    Undo,
    Redo,
    Copy,
    Paste,
    // The follow loop.
    FollowLive,
    ScrubBack,
    ScrubForward,
    MarkReviewed,
    ToggleView,
}

impl Command {
    /// Stable identifier used as the JSON key in `settings.json`.
    pub fn id(&self) -> &'static str {
        match self {
            Self::NewFile => "new_file",
            Self::OpenFolder => "open_folder",
            Self::AddFolder => "add_folder",
            Self::OpenRecent => "open_recent",
            Self::Save => "save",
            Self::Settings => "settings",
            Self::Quit => "quit",
            Self::Documentation => "documentation",
            Self::Help => "help",
            Self::CheckForUpdates => "check_for_updates",
            Self::InstallUpdate => "install_update",
            Self::ToggleSidebar => "toggle_sidebar",
            Self::Changes => "changes",
            Self::CommandPalette => "command_palette",
            Self::SearchProject => "search_project",
            Self::AgentUsage => "agent_usage",
            Self::ThemePicker => "theme_picker",
            Self::QuickOpen => "quick_open",
            Self::FileMenu => "file_menu",
            Self::HelpMenu => "help_menu",
            Self::NextPane => "next_pane",
            Self::SwapPanes => "swap_panes",
            Self::Close => "close",
            Self::NewTab => "new_tab",
            Self::CloseTab => "close_tab",
            Self::NextTab => "next_tab",
            Self::PrevTab => "prev_tab",
            Self::RenameTab => "rename_tab",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::FollowLive => "follow_live",
            Self::ScrubBack => "scrub_back",
            Self::ScrubForward => "scrub_forward",
            Self::MarkReviewed => "mark_reviewed",
            Self::ToggleView => "toggle_view",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        ALL.iter().copied().find(|c| c.id() == id)
    }

    /// How the command reads in a menu or the palette.
    pub fn label(&self) -> &'static str {
        match self {
            Self::NewFile => "New File",
            Self::OpenFolder => "Open Folder…",
            Self::AddFolder => "Add Folder to Task…",
            Self::OpenRecent => "Open Recent…",
            Self::Save => "Save",
            Self::Settings => "Settings",
            Self::Quit => "Quit",
            Self::Documentation => "Documentation",
            Self::Help => "Key Bindings",
            Self::CheckForUpdates => "Check for Updates…",
            Self::InstallUpdate => "Restart to Update",
            Self::ToggleSidebar => "Toggle Files",
            Self::Changes => "Changes",
            Self::CommandPalette => "Command Palette",
            Self::SearchProject => "Search Project",
            Self::AgentUsage => "Agent Usage",
            Self::ThemePicker => "Theme…",
            Self::QuickOpen => "Go to File",
            Self::FileMenu => "File Menu",
            Self::HelpMenu => "Help Menu",
            Self::NextPane => "Switch Pane",
            Self::SwapPanes => "Move Panes to the Other Side",
            Self::Close => "Close",
            Self::NewTab => "New Task…",
            Self::CloseTab => "Close Task",
            Self::NextTab => "Next Task",
            Self::PrevTab => "Previous Task",
            Self::RenameTab => "Rename Task…",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::FollowLive => "Follow: Go Live",
            Self::ScrubBack => "Follow: Previous Edit",
            Self::ScrubForward => "Follow: Next Edit",
            Self::MarkReviewed => "Follow: Mark Reviewed",
            Self::ToggleView => "Follow: Diff / File",
        }
    }
}

pub const ALL: &[Command] = &[
    Command::NewFile,
    Command::OpenFolder,
    Command::AddFolder,
    Command::OpenRecent,
    Command::Save,
    Command::Settings,
    Command::Quit,
    Command::Documentation,
    Command::Help,
    Command::CheckForUpdates,
    Command::InstallUpdate,
    Command::ToggleSidebar,
    Command::Changes,
    Command::CommandPalette,
    Command::SearchProject,
    Command::AgentUsage,
    Command::ThemePicker,
    Command::QuickOpen,
    Command::FileMenu,
    Command::HelpMenu,
    Command::NextPane,
    Command::SwapPanes,
    Command::Close,
    Command::NewTab,
    Command::CloseTab,
    Command::NextTab,
    Command::PrevTab,
    Command::RenameTab,
    Command::Undo,
    Command::Redo,
    Command::Copy,
    Command::Paste,
    Command::FollowLive,
    Command::ScrubBack,
    Command::ScrubForward,
    Command::MarkReviewed,
    Command::ToggleView,
];

/// A menu: its entries in order, `None` where a separator goes.
pub type Menu = &'static [Option<Command>];

pub const FILE_MENU: Menu = &[
    Some(Command::NewFile),
    Some(Command::NewTab),
    None,
    Some(Command::OpenFolder),
    Some(Command::AddFolder),
    Some(Command::OpenRecent),
    None,
    Some(Command::Save),
    None,
    Some(Command::Settings),
    None,
    Some(Command::Quit),
];

pub const HELP_MENU: Menu = &[
    Some(Command::Documentation),
    Some(Command::Help),
    None,
    Some(Command::CheckForUpdates),
];

/// What the palette offers: everything but the menus themselves, which are
/// a way of reaching commands rather than commands.
pub fn palette() -> impl Iterator<Item = Command> {
    ALL.iter()
        .copied()
        .filter(|c| !matches!(c, Command::FileMenu | Command::HelpMenu | Command::Close))
}

// ---------------------------------------------------------------------------
// Key chords
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Mods {
    /// Terminals never report Cmd; it is still read, so a chord a user typed
    /// with it is a chord rather than a typo, but no key press will match it.
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The key itself: a printable character or a named non-printing key.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    Char(char),
    Named(String),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    pub mods: Mods,
    pub key: Key,
}

impl Chord {
    /// Whether the chord is a bare printable key — `f`, `v` — which is a
    /// shortcut only where the user is not typing.
    pub fn is_bare_char(&self) -> bool {
        matches!(self.key, Key::Char(_)) && self.mods == Mods::default()
    }

    /// The chord as the status bar and the hints spell it: `^⇧G`, `F1`, `⏎`.
    pub fn glyphs(&self) -> String {
        let mut out = String::new();
        for (active, glyph) in [
            (self.mods.cmd, '⌘'),
            (self.mods.ctrl, '^'),
            (self.mods.alt, '⌥'),
            (self.mods.shift, '⇧'),
        ] {
            if active {
                out.push(glyph);
            }
        }
        match &self.key {
            Key::Char(c) => out.push(c.to_ascii_uppercase()),
            Key::Named(name) => match name.as_str() {
                "enter" => out.push('⏎'),
                "left" => out.push('←'),
                "right" => out.push('→'),
                "up" => out.push('↑'),
                "down" => out.push('↓'),
                "tab" => out.push('⇥'),
                "backspace" => out.push('⌫'),
                "delete" => out.push('⌦'),
                other => {
                    let mut chars = other.chars();
                    out.extend(chars.next().into_iter().flat_map(char::to_uppercase));
                    out.push_str(chars.as_str());
                }
            },
        }
        out
    }
}

#[derive(Debug)]
pub struct ChordParseError(pub String);

impl fmt::Display for ChordParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a key chord: {}", self.0)
    }
}

impl FromStr for Chord {
    type Err = ChordParseError;

    /// Parses `"Ctrl+Shift+F"`, `"Ctrl+-"`, `"Alt+Left"`, `"F"`. Modifier
    /// spellings are forgiving: cmd/command/super/meta, ctrl/control,
    /// alt/option, shift.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        // The separator is also a possible key, so "Ctrl++" and a bare "+" are
        // split off before the modifiers are parsed.
        let (modifier_text, key_text) = if let Some(rest) = text.strip_suffix("++") {
            (rest, "+")
        } else if text == "+" {
            ("", "+")
        } else {
            match text.rsplit_once('+') {
                Some((modifiers, key)) => (modifiers, key),
                None => ("", text),
            }
        };
        if key_text.is_empty() {
            return Err(ChordParseError(text.to_string()));
        }

        let mut mods = Mods::default();
        for part in modifier_text.split('+').map(str::trim) {
            if part.is_empty() {
                continue;
            }
            match part.to_ascii_lowercase().as_str() {
                "cmd" | "command" | "super" | "meta" | "win" => mods.cmd = true,
                "ctrl" | "control" => mods.ctrl = true,
                "alt" | "option" | "opt" => mods.alt = true,
                "shift" => mods.shift = true,
                other => return Err(ChordParseError(other.to_string())),
            }
        }

        let key_text = key_text.trim();
        let key = if key_text.chars().count() == 1 {
            Key::Char(key_text.chars().next().unwrap().to_ascii_lowercase())
        } else {
            // "Escape" and "Esc" are one key; the short form is what the
            // frontend reports.
            let name = key_text.to_ascii_lowercase();
            Key::Named(if name == "escape" { "esc".into() } else { name })
        };
        Ok(Chord { mods, key })
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        for (active, name) in [
            (self.mods.cmd, "Cmd"),
            (self.mods.ctrl, "Ctrl"),
            (self.mods.alt, "Alt"),
            (self.mods.shift, "Shift"),
        ] {
            if active {
                out.push_str(name);
                out.push('+');
            }
        }
        match &self.key {
            Key::Char(c) => out.push(c.to_ascii_uppercase()),
            Key::Named(name) => {
                let mut chars = name.chars();
                if let Some(first) = chars.next() {
                    out.extend(first.to_uppercase());
                    out.push_str(chars.as_str());
                }
            }
        }
        f.write_str(&out)
    }
}

impl Serialize for Chord {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Chord {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_round_trip() {
        for text in [
            "Ctrl+S",
            "Ctrl+Shift+F",
            "Alt+Left",
            "Ctrl+-",
            "Ctrl+,",
            "F",
            "Enter",
        ] {
            let chord: Chord = text.parse().unwrap();
            assert_eq!(chord.to_string(), text, "round trip of {text}");
        }
    }

    #[test]
    fn modifier_spellings_are_forgiving() {
        let a: Chord = "Control+Option+K".parse().unwrap();
        let b: Chord = "ctrl+alt+k".parse().unwrap();
        assert_eq!(a, b);
        assert!(a.mods.ctrl && a.mods.alt && !a.mods.cmd);
    }

    #[test]
    fn named_keys_and_plus() {
        assert_eq!(
            "Ctrl+Enter".parse::<Chord>().unwrap().key,
            Key::Named("enter".into())
        );
        assert_eq!("Ctrl++".parse::<Chord>().unwrap().key, Key::Char('+'));
        assert_eq!(
            "F12".parse::<Chord>().unwrap().key,
            Key::Named("f12".into())
        );
        assert_eq!(
            "Escape".parse::<Chord>().unwrap(),
            "Esc".parse::<Chord>().unwrap()
        );
    }

    #[test]
    fn a_bare_letter_is_a_chord_only_where_nobody_is_typing() {
        assert!("f".parse::<Chord>().unwrap().is_bare_char());
        assert!(!"Ctrl+F".parse::<Chord>().unwrap().is_bare_char());
        assert!(!"Enter".parse::<Chord>().unwrap().is_bare_char());
    }

    #[test]
    fn a_chord_has_a_glyph_spelling_for_the_status_bar() {
        let glyph = |text: &str| text.parse::<Chord>().unwrap().glyphs();
        assert_eq!(glyph("Ctrl+Shift+G"), "^⇧G");
        assert_eq!(glyph("Ctrl+B"), "^B");
        assert_eq!(glyph("F1"), "F1");
        assert_eq!(glyph("Enter"), "⏎");
        assert_eq!(glyph("Left"), "←");
        assert_eq!(glyph("Esc"), "Esc");
        assert_eq!(glyph("Alt+PageDown"), "⌥Pagedown");
    }

    #[test]
    fn nonsense_is_not_a_chord() {
        assert!("".parse::<Chord>().is_err());
        assert!("Ctrl+".parse::<Chord>().is_err());
        assert!("Hyper+K".parse::<Chord>().is_err(), "no such modifier");
        let message = "Hyper+K".parse::<Chord>().unwrap_err().to_string();
        assert!(message.contains("hyper"), "{message}");
    }

    #[test]
    fn a_chord_prints_as_a_binding_a_user_would_type() {
        let chord: Chord = "alt+shift+left".parse().unwrap();
        assert_eq!(chord.to_string(), "Alt+Shift+Left");
        let all: Chord = "shift+alt+ctrl+cmd+s".parse().unwrap();
        assert_eq!(all.to_string(), "Cmd+Ctrl+Alt+Shift+S");
    }

    #[test]
    fn every_command_has_a_label_and_a_stable_unique_id() {
        let mut ids: Vec<&str> = ALL.iter().map(|c| c.id()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "command ids must be unique");
        for command in ALL {
            let id = command.id();
            assert!(
                id.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{id} is not a settings key"
            );
            assert!(!command.label().is_empty(), "{id} has no label");
            assert_eq!(Command::from_id(id), Some(*command));
        }
        assert_eq!(Command::from_id("no_such_command"), None);
    }

    #[test]
    fn the_menus_only_list_commands_that_exist_and_never_start_with_a_line() {
        for (name, menu) in [("File", FILE_MENU), ("Help", HELP_MENU)] {
            let entries: Vec<Command> = menu.iter().flatten().copied().collect();
            assert!(!entries.is_empty(), "the {name} menu is empty");
            for command in &entries {
                assert!(ALL.contains(command), "{name}: {command:?}");
            }
            assert!(menu.first().is_some_and(Option::is_some), "{name}");
            assert!(menu.last().is_some_and(Option::is_some), "{name}");
        }
    }

    #[test]
    fn the_file_menu_is_the_designs() {
        let entries: Vec<Command> = FILE_MENU.iter().flatten().copied().collect();
        assert_eq!(
            entries,
            [
                Command::NewFile,
                Command::NewTab,
                Command::OpenFolder,
                Command::AddFolder,
                Command::OpenRecent,
                Command::Save,
                Command::Settings,
                Command::Quit
            ]
        );
    }

    #[test]
    fn the_palette_offers_actions_not_menus() {
        let offered: Vec<Command> = palette().collect();
        assert!(offered.contains(&Command::Changes));
        assert!(!offered.contains(&Command::FileMenu));
        assert!(!offered.contains(&Command::Close));
    }

    #[test]
    fn a_command_survives_the_settings_file() {
        let json = serde_json::to_string(&Command::AgentUsage).unwrap();
        assert_eq!(json, "\"agent_usage\"");
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Command::AgentUsage);
    }
}

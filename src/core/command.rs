//! Every action a frontend can be asked to perform, plus the key chords bound
//! to it. Both frontends translate their native key events into a [`Command`],
//! so a rebind in `settings.json` moves the action in lockstep.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    NewFile,
    OpenFile,
    OpenFolder,
    AddFolder,
    OpenRecent,
    Save,
    SaveAs,
    SaveAll,
    Settings,
    CloseEditor,
    Quit,
    ToggleSidebar,
    ToggleTerminal,
    NewTerminal,
    CloseTerminal,
    FindInFile,
    FocusSearch,
    FocusFiles,
    FocusGit,
    ThemePicker,
    GotoDefinition,
    GoBack,
    Undo,
    Redo,
    SelectAll,
    Copy,
    Cut,
    Paste,
    NewFolder,
    Rename,
    Delete,
    MoveTo,
    FindNext,
    FindPrev,
    ReplaceAll,
    NextPane,
    PrevPane,
    PickRepository,
    PickWorktree,
    ToggleFold,
    FoldAll,
    UnfoldAll,
    NextTab,
    PrevTab,
    ContextMenu,
    FileMenu,
    ViewMenu,
    HelpMenu,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    Documentation,
    CheckForUpdates,
    InstallUpdate,
    Help,
}

impl Command {
    /// Stable identifier used as the JSON key in `settings.json`.
    pub fn id(&self) -> &'static str {
        match self {
            Self::NewFile => "new_file",
            Self::OpenFile => "open_file",
            Self::OpenFolder => "open_folder",
            Self::AddFolder => "add_folder",
            Self::OpenRecent => "open_recent",
            Self::Save => "save",
            Self::SaveAs => "save_as",
            Self::SaveAll => "save_all",
            Self::Settings => "settings",
            Self::CloseEditor => "close_editor",
            Self::Quit => "quit",
            Self::ToggleSidebar => "toggle_sidebar",
            Self::ToggleTerminal => "toggle_terminal",
            Self::NewTerminal => "new_terminal",
            Self::CloseTerminal => "close_terminal",
            Self::FindInFile => "find_in_file",
            Self::FocusSearch => "focus_search",
            Self::FocusFiles => "focus_files",
            Self::FocusGit => "focus_git",
            Self::ThemePicker => "theme_picker",
            Self::GotoDefinition => "goto_definition",
            Self::GoBack => "go_back",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::SelectAll => "select_all",
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::Paste => "paste",
            Self::NewFolder => "new_folder",
            Self::Rename => "rename",
            Self::Delete => "delete",
            Self::MoveTo => "move_to",
            Self::FindNext => "find_next",
            Self::FindPrev => "find_prev",
            Self::ReplaceAll => "replace_all",
            Self::NextPane => "next_pane",
            Self::PrevPane => "prev_pane",
            Self::PickRepository => "pick_repository",
            Self::PickWorktree => "pick_worktree",
            Self::ToggleFold => "toggle_fold",
            Self::FoldAll => "fold_all",
            Self::UnfoldAll => "unfold_all",
            Self::NextTab => "next_tab",
            Self::PrevTab => "prev_tab",
            Self::ContextMenu => "context_menu",
            Self::FileMenu => "file_menu",
            Self::ViewMenu => "view_menu",
            Self::HelpMenu => "help_menu",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            Self::ResetZoom => "reset_zoom",
            Self::Documentation => "documentation",
            Self::CheckForUpdates => "check_for_updates",
            Self::InstallUpdate => "install_update",
            Self::Help => "help",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        ALL.iter().copied().find(|c| c.id() == id)
    }

    /// Human label used in menus.
    pub fn label(&self) -> &'static str {
        match self {
            Self::NewFile => "New File...",
            Self::OpenFile => "Open File...",
            Self::OpenFolder => "Open Folder...",
            Self::AddFolder => "Add Folder to Project...",
            Self::OpenRecent => "Open Recent...",
            Self::Save => "Save",
            Self::SaveAs => "Save As...",
            Self::SaveAll => "Save All",
            Self::Settings => "Settings",
            Self::CloseEditor => "Close Editor",
            Self::Quit => "Quit",
            Self::ToggleSidebar => "Toggle Sidebar",
            Self::ToggleTerminal => "Toggle Terminal",
            Self::NewTerminal => "New Terminal",
            Self::CloseTerminal => "Close Terminal",
            Self::FindInFile => "Find in File...",
            Self::FocusSearch => "Search",
            Self::FocusFiles => "Files",
            Self::FocusGit => "Git",
            Self::ThemePicker => "Color Theme...",
            Self::GotoDefinition => "Go to Definition",
            Self::GoBack => "Go Back",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::SelectAll => "Select All",
            Self::Copy => "Copy",
            Self::Cut => "Cut",
            Self::Paste => "Paste",
            Self::NewFolder => "New Folder...",
            Self::Rename => "Rename...",
            Self::Delete => "Delete",
            Self::MoveTo => "Move To...",
            Self::FindNext => "Find Next",
            Self::FindPrev => "Find Previous",
            Self::ReplaceAll => "Replace All",
            Self::NextPane => "Next Pane",
            Self::PrevPane => "Previous Pane",
            Self::PickRepository => "Repository...",
            Self::PickWorktree => "Worktree...",
            Self::ToggleFold => "Fold / Unfold",
            Self::FoldAll => "Fold All",
            Self::UnfoldAll => "Unfold All",
            Self::NextTab => "Next Tab",
            Self::PrevTab => "Previous Tab",
            Self::ContextMenu => "Context Menu",
            Self::FileMenu => "File Menu",
            Self::ViewMenu => "View Menu",
            Self::HelpMenu => "Help Menu",
            Self::ZoomIn => "Zoom In",
            Self::ZoomOut => "Zoom Out",
            Self::ResetZoom => "Reset Zoom",
            Self::Documentation => "Documentation",
            Self::CheckForUpdates => "Check for Updates...",
            Self::InstallUpdate => "Install Update",
            Self::Help => "Show Key Bindings",
        }
    }
}

pub const ALL: &[Command] = &[
    Command::NewFile,
    Command::OpenFile,
    Command::OpenFolder,
    Command::AddFolder,
    Command::OpenRecent,
    Command::Save,
    Command::SaveAs,
    Command::SaveAll,
    Command::Settings,
    Command::CloseEditor,
    Command::Quit,
    Command::ToggleSidebar,
    Command::ToggleTerminal,
    Command::NewTerminal,
    Command::CloseTerminal,
    Command::FindInFile,
    Command::FocusSearch,
    Command::FocusFiles,
    Command::FocusGit,
    Command::ThemePicker,
    Command::GotoDefinition,
    Command::GoBack,
    Command::Undo,
    Command::Redo,
    Command::SelectAll,
    Command::Copy,
    Command::Cut,
    Command::Paste,
    Command::NewFolder,
    Command::Rename,
    Command::Delete,
    Command::MoveTo,
    Command::FindNext,
    Command::FindPrev,
    Command::ReplaceAll,
    Command::NextPane,
    Command::PrevPane,
    Command::PickRepository,
    Command::PickWorktree,
    Command::ToggleFold,
    Command::FoldAll,
    Command::UnfoldAll,
    Command::NextTab,
    Command::PrevTab,
    Command::ContextMenu,
    Command::FileMenu,
    Command::ViewMenu,
    Command::HelpMenu,
    Command::ZoomIn,
    Command::ZoomOut,
    Command::ResetZoom,
    Command::Documentation,
    Command::CheckForUpdates,
    Command::InstallUpdate,
    Command::Help,
];

/// What the File menu lists, in order; `None` is a separator.
pub const FILE_MENU: &[Option<Command>] = &[
    Some(Command::NewFile),
    None,
    Some(Command::OpenFile),
    Some(Command::OpenFolder),
    Some(Command::AddFolder),
    Some(Command::OpenRecent),
    None,
    Some(Command::Save),
    Some(Command::SaveAs),
    Some(Command::SaveAll),
    None,
    Some(Command::Settings),
    None,
    Some(Command::CloseEditor),
    Some(Command::Quit),
];

/// What the View menu lists, in order; `None` is a separator.
pub const VIEW_MENU: &[Option<Command>] = &[
    Some(Command::ZoomIn),
    Some(Command::ZoomOut),
    Some(Command::ResetZoom),
    None,
    Some(Command::ToggleSidebar),
    Some(Command::ToggleTerminal),
    None,
    Some(Command::FocusFiles),
    Some(Command::FocusSearch),
    Some(Command::FocusGit),
    None,
    Some(Command::ToggleFold),
    Some(Command::FoldAll),
    Some(Command::UnfoldAll),
    None,
    Some(Command::ThemePicker),
];

/// What the Help menu lists. The version stands above it, drawn by each
/// frontend from the crate's own version.
pub const HELP_MENU: &[Option<Command>] = &[
    Some(Command::Help),
    Some(Command::Documentation),
    None,
    Some(Command::CheckForUpdates),
    Some(Command::InstallUpdate),
];

/// The start page's key list: what to reach for first, in groups. Both
/// frontends draw the same groups, each with the chord actually bound.
pub const START_PAGE: &[(&str, &[Command])] = &[
    (
        "Project",
        &[
            Command::OpenFolder,
            Command::AddFolder,
            Command::OpenRecent,
            Command::OpenFile,
            Command::NewFile,
        ],
    ),
    (
        "Edit",
        &[
            Command::Undo,
            Command::Redo,
            Command::Save,
            Command::FindInFile,
            Command::CloseEditor,
        ],
    ),
    (
        "Panels",
        &[
            Command::ToggleSidebar,
            Command::FocusSearch,
            Command::FocusGit,
            Command::ToggleTerminal,
        ],
    ),
    (
        "More",
        &[Command::ThemePicker, Command::Settings, Command::Help],
    ),
];

// ---------------------------------------------------------------------------
// Key chords
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Mods {
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

#[derive(Debug)]
pub struct ChordParseError(pub String);

impl fmt::Display for ChordParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a key chord: {}", self.0)
    }
}

impl FromStr for Chord {
    type Err = ChordParseError;

    /// Parses `"Cmd+Shift+F"`, `"Ctrl+-"`, `"Alt+Left"`. Modifier spellings are
    /// forgiving: cmd/command/super/meta, ctrl/control, alt/option, shift.
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
            Key::Named(key_text.to_ascii_lowercase())
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
        for text in ["Cmd+S", "Ctrl+Shift+F", "Alt+Left", "Ctrl+-", "Cmd+,"] {
            let chord: Chord = text.parse().unwrap();
            assert_eq!(chord.to_string(), text, "round trip of {text}");
        }
    }

    #[test]
    fn modifier_spellings_are_forgiving() {
        let a: Chord = "Command+Option+K".parse().unwrap();
        let b: Chord = "cmd+alt+k".parse().unwrap();
        assert_eq!(a, b);
        assert!(a.mods.cmd && a.mods.alt && !a.mods.ctrl);
    }

    #[test]
    fn named_keys_and_plus() {
        assert_eq!(
            "Ctrl+Enter".parse::<Chord>().unwrap().key,
            Key::Named("enter".into())
        );
        assert_eq!("Ctrl++".parse::<Chord>().unwrap().key, Key::Char('+'));
    }

    #[test]
    fn every_command_id_is_unique_and_resolvable() {
        let mut ids: Vec<&str> = ALL.iter().map(|c| c.id()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "command ids must be unique");
        for command in ALL {
            assert_eq!(Command::from_id(command.id()), Some(*command));
        }
    }
}

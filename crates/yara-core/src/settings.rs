//! User settings: one JSON file, every key optional, comments allowed.
//! Anything a user might want different — the agent, the layout, the theme,
//! the keys — lives here rather than in the code.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::command::{Chord, Command, ALL};

/// Command id → chord. Only the bindings that differ from the defaults are
/// written out, so a later change to a default still reaches every user.
pub type KeyMap = BTreeMap<String, Chord>;

/// Which side of the window the AGENT pane takes; FOLLOW takes the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keys {
    pub map: KeyMap,
    /// Bindings the file spelled wrongly, as `(command, text)`: they keep
    /// their defaults, are named in the status bar, and are written back as
    /// they were — a save is not the moment to lose the user's line.
    pub rejected: Vec<(String, String)>,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            map: ALL
                .iter()
                .filter_map(|c| Some((c.id().to_string(), default_chord(*c)?.parse().ok()?)))
                .collect(),
            rejected: Vec::new(),
        }
    }
}

impl Serialize for Keys {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let defaults = Keys::default().map;
        let mut out: BTreeMap<&str, String> = self
            .map
            .iter()
            .filter(|(id, chord)| defaults.get(*id) != Some(chord))
            .map(|(id, chord)| (id.as_str(), chord.to_string()))
            .collect();
        out.extend(
            self.rejected
                .iter()
                .map(|(id, text)| (id.as_str(), text.clone())),
        );
        out.serialize(s)
    }
}

/// The file's bindings are laid over the defaults, so rebinding one key does
/// not drop the rest. A file from before v1 nested them under `"tui"`; that
/// shape is still read.
impl<'de> Deserialize<'de> for Keys {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut raw = BTreeMap::<String, serde_json::Value>::deserialize(d)?;
        if let Some(serde_json::Value::Object(tui)) = raw.remove("tui") {
            raw.extend(tui);
        }
        let mut keys = Keys::default();
        for (id, value) in raw {
            let Some(text) = value.as_str() else { continue };
            match text.parse::<Chord>() {
                Ok(chord) => {
                    keys.map.insert(id, chord);
                }
                Err(_) => keys.rejected.push((id, text.to_string())),
            }
        }
        Ok(keys)
    }
}

/// The folder's own settings, relative to the project.
pub const LOCAL_FILE: &str = ".ycode/settings.json";

/// What a folder's settings file says when the editor makes it: nothing is
/// set yet, and here is what setting something would do.
const LOCAL_STUB: &str = "\
// Yara Code settings for this folder. Any key from the global settings.json
// set here wins over it while the folder is open; a map — keys, usage_slash —
// merges entry by entry. Every key is optional; // comments are allowed.
{
}
";

/// Keys every terminal can send: function keys and plain Ctrl+letter. There
/// is no Ctrl+Shift here on purpose — without the kitty keyboard protocol a
/// terminal cannot tell it from Ctrl, and most terminals do not have it.
pub fn default_chord(command: Command) -> Option<&'static str> {
    Some(match command {
        Command::NewFile => "Ctrl+N",
        // New Folder is the tree's own, on a right click.
        Command::NewFolder => return None,
        Command::OpenRecent => "Ctrl+R",
        Command::Save => "Ctrl+S",
        Command::Settings => "F12",
        // The folder's settings are a menu and palette action.
        Command::LocalSettings => return None,
        Command::Quit => "Ctrl+Q",
        Command::Help => "F1",
        // Updating, moving the panes, the Help menu and the documentation are
        // menu and palette actions; F10 then → opens Help.
        Command::CheckForUpdates
        | Command::InstallUpdate
        | Command::SwapPanes
        | Command::HelpMenu
        | Command::AddFolder
        | Command::RemoveFolder
        | Command::Documentation => return None,
        Command::ToggleSidebar => "Ctrl+B",
        Command::ToggleTerminal => "Ctrl+T",
        Command::Changes => "F4",
        Command::CommandPalette => "F5",
        Command::SearchProject => "F3",
        Command::AgentUsage => "F8",
        Command::ThemePicker => "F9",
        Command::QuickOpen => "Ctrl+P",
        Command::FileMenu => "F10",
        // The keyboard's own key for moving between parts of a window, and
        // one no program in the agent pane is listening for.
        Command::NextPane => "F6",
        Command::Close => "Esc",
        Command::NewTab => "F7",
        Command::CloseTab => "Ctrl+W",
        // Ctrl with an arrow or a page key does not survive every terminal;
        // two letters do.
        Command::NextTab => "Ctrl+L",
        Command::PrevTab => "Ctrl+K",
        Command::RenameTab => "F2",
        Command::Undo => "Ctrl+Z",
        Command::Redo => "Ctrl+Y",
        Command::Copy => "Ctrl+C",
        Command::Paste => "Ctrl+V",
        Command::FollowLive => "F",
        Command::ScrubBack => "Left",
        Command::ScrubForward => "Right",
        Command::MarkReviewed => "Enter",
        Command::ToggleView => "V",
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Name of the active theme, as the theme picker lists it.
    pub theme: String,
    /// The command that runs in the AGENT pane.
    pub agent: String,
    /// Where the AGENT pane sits, and its share of the width in percent.
    pub agent_side: Side,
    pub agent_width: u16,
    /// The FILES sidebar, in columns, and whether it is open at start.
    pub sidebar_width: u16,
    pub show_sidebar: bool,
    /// Under this many columns — a phone, a split — the panes take turns:
    /// the one with the keyboard has the whole body; 0 keeps them side by
    /// side at any width.
    pub narrow_width: u16,
    /// The shell the terminal runs; empty means `$SHELL`, or `sh`.
    pub shell: String,
    /// The terminal's share of the agent pane's height, in percent.
    pub terminal_height: u16,
    /// Edits shown on the timeline strip before it windows around the cursor.
    pub timeline_ticks: usize,
    /// One step of the editor caret's blink, in milliseconds; 0 keeps it on.
    pub cursor_blink_ms: u64,
    /// Whether the editor says, at the end of the line under the mouse, who
    /// last committed it.
    pub blame: bool,
    /// How often the working tree is looked at for the agent's edits, in
    /// milliseconds.
    pub refresh_ms: u64,
    /// The branch changes are measured against; empty means the one the main
    /// working copy has checked out.
    pub base_branch: String,
    /// Where a new tab's worktree is made; empty means a `<repo>-worktrees`
    /// folder beside the repository.
    pub worktrees_dir: String,
    /// What Agent Usage types at each agent: the slash command that makes
    /// it show its own figures, since none of them will say outside.
    pub usage_slash: BTreeMap<String, String>,
    /// A command per agent that prints its plan usage as JSON — see
    /// `usage.rs` — for the AGENT USAGE panel and the header chip, for those
    /// who have such a thing. Set, it is used instead of `usage_slash`.
    pub usage_commands: BTreeMap<String, String>,
    /// Folders nobody works in: left out of the tree, the finder, search
    /// and the watching. A name matches a folder anywhere; a leading dot
    /// on its own — "." — stands for every hidden folder.
    pub ignore_folders: Vec<String>,
    /// What project search leaves out on top of those, in VS Code's glob
    /// spelling.
    pub search_exclude: Vec<String>,
    /// Chords the agent keeps even though the editor binds them, because
    /// the programs in that pane use them themselves.
    pub agent_keys: Vec<Chord>,
    pub keys: Keys,
    /// Workspaces opened before, newest first: each is its list of folders.
    pub recent_workspaces: Vec<Vec<PathBuf>>,
    /// The file could not be read. Nothing is written over it until it can:
    /// a save would replace the user's file, typo and all, with defaults.
    #[serde(skip)]
    pub unreadable: bool,
    /// What the folder's own file — `.ycode/settings.json` in the project —
    /// laid over the global one: those keys are the folder's, and a save
    /// leaves them out of the global file.
    #[serde(skip)]
    pub local: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "Dark Modern".into(),
            agent: "claude".into(),
            agent_side: Side::Left,
            agent_width: 42,
            sidebar_width: 30,
            show_sidebar: false,
            narrow_width: 80,
            shell: String::new(),
            terminal_height: 40,
            timeline_ticks: 12,
            cursor_blink_ms: 500,
            blame: true,
            refresh_ms: 500,
            base_branch: String::new(),
            worktrees_dir: String::new(),
            usage_slash: [
                ("claude", "/usage"),
                ("cursor-agent", "/usage"),
                ("codex", "/status"),
            ]
            .into_iter()
            .map(|(a, c)| (a.to_string(), c.to_string()))
            .collect(),
            usage_commands: BTreeMap::new(),
            ignore_folders: [
                ".",
                "node_modules",
                "target",
                "dist",
                "build",
                "vendor",
                "venv",
            ]
            .map(String::from)
            .to_vec(),
            search_exclude: Vec::new(),
            agent_keys: ["Ctrl+R", "Ctrl+N", "Ctrl+Z", "Ctrl+W", "Ctrl+Y", "Ctrl+C"]
                .iter()
                .filter_map(|c| c.parse().ok())
                .collect(),
            keys: Keys::default(),
            recent_workspaces: Vec::new(),
            unreadable: false,
            local: serde_json::Map::new(),
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
                out.extend(chars.next());
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
    /// Whether a folder of this name is one nobody works in.
    pub fn ignores(&self, name: &str) -> bool {
        self.ignore_folders.iter().any(|rule| match rule.as_str() {
            "." => name.starts_with('.'),
            other => other == name,
        })
    }

    /// `$XDG_CONFIG_HOME/ycode/settings.json`, else `~/.config/ycode/...`.
    pub fn path() -> Option<PathBuf> {
        Some(crate::config_dir()?.join("settings.json"))
    }

    /// The folder's own settings file, laid over the global one while the
    /// folder is open.
    pub fn local_path(folder: &Path) -> PathBuf {
        folder.join(LOCAL_FILE)
    }

    /// The global settings with the folder's file laid over them: a key the
    /// folder sets wins, a map — the keys, the usage commands — merges entry
    /// by entry. A folder file that cannot be read is reported and skipped.
    pub fn load_in(folder: Option<&Path>) -> (Self, Option<String>) {
        let (global, complaint) = Self::load();
        let Some(path) = folder.map(Self::local_path) else {
            return (global, complaint);
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (global, complaint);
        };
        let local = match serde_json::from_str::<serde_json::Value>(&strip_comments(&text)) {
            Ok(serde_json::Value::Object(map)) => map,
            Ok(_) => {
                return (
                    global,
                    Some(format!("{} ignored: not an object", path.display())),
                )
            }
            Err(e) => return (global, Some(format!("{} ignored: {e}", path.display()))),
        };
        let mut merged = match serde_json::to_value(&global) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => return (global, complaint),
        };
        for (key, value) in &local {
            match (merged.get_mut(key), value) {
                (Some(serde_json::Value::Object(mine)), serde_json::Value::Object(theirs)) => {
                    mine.extend(theirs.clone());
                }
                _ => {
                    merged.insert(key.clone(), value.clone());
                }
            }
        }
        match serde_json::from_value::<Self>(serde_json::Value::Object(merged)) {
            Ok(mut settings) => {
                settings.unreadable = global.unreadable;
                settings.local = local;
                let complaint = complaint.or_else(|| settings.binding_complaint());
                (settings, complaint)
            }
            Err(e) => (global, Some(format!("{} ignored: {e}", path.display()))),
        }
    }

    /// Loads the file, falling back to defaults for anything missing. Returns
    /// a message when something in it was wrong, so the caller can show it
    /// instead of silently ignoring a typo.
    pub fn load() -> (Self, Option<String>) {
        let Some(text) = Self::path().and_then(|p| std::fs::read_to_string(p).ok()) else {
            return (Self::default(), None);
        };
        match serde_json::from_str::<Self>(&strip_comments(&text)) {
            Ok(settings) => {
                let complaint = settings.binding_complaint();
                (settings, complaint)
            }
            Err(e) => (
                Self {
                    unreadable: true,
                    ..Self::default()
                },
                Some(format!(
                    "settings.json ignored: {e} — fix it or delete it; it will not be written over"
                )),
            ),
        }
    }

    /// A chord that could not be read, or one handed to two commands at once.
    /// Either way the editor keeps working — this is what it says about it.
    fn binding_complaint(&self) -> Option<String> {
        if let Some((id, text)) = self.keys.rejected.first() {
            return Some(format!(
                "settings.json: {id} is not a key chord: \"{text}\" — keeping the default"
            ));
        }
        let mut seen: BTreeMap<String, &str> = BTreeMap::new();
        for (id, chord) in &self.keys.map {
            if let Some(other) = seen.insert(chord.to_string(), id) {
                return Some(format!(
                    "settings.json: {chord} is bound to both {other} and {id}"
                ));
            }
        }
        None
    }

    /// Writes the file, with a line over each key saying what it is for. A
    /// file that could not be read is left alone — the user's, mistake
    /// included, beats ours.
    pub fn save(&self) -> std::io::Result<PathBuf> {
        if self.unreadable {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "settings.json could not be read and was not written over",
            ));
        }
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no config directory")
        })?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, self.for_global()?.to_commented_json())?;
        Ok(path)
    }

    /// What the global file gets: everything but the keys the folder's file
    /// pins, which stay as the global file has them — a folder that runs
    /// codex must not make every other folder run it.
    fn for_global(&self) -> std::io::Result<Self> {
        if self.local.is_empty() {
            return Ok(self.clone());
        }
        let (global, _) = Self::load();
        let mut mine = serde_json::to_value(self).map_err(std::io::Error::other)?;
        let theirs = serde_json::to_value(&global).map_err(std::io::Error::other)?;
        for key in self.local.keys() {
            if let Some(value) = theirs.get(key) {
                mine[key] = value.clone();
            }
        }
        serde_json::from_value(mine).map_err(std::io::Error::other)
    }

    /// The folder's settings file, written empty first if it is not there —
    /// so "Local Settings" always opens something real, and what it opens
    /// says what it is for.
    pub fn ensure_local_file(folder: &Path) -> std::io::Result<PathBuf> {
        let path = Self::local_path(folder);
        if !path.exists() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, LOCAL_STUB)?;
        }
        Ok(path)
    }

    /// Path to the settings file, writing the current values first if it does
    /// not exist yet — so "Settings" always opens something real.
    pub fn ensure_file(&self) -> std::io::Result<PathBuf> {
        match Self::path() {
            Some(path) if path.exists() => Ok(path),
            _ => self.save(),
        }
    }

    /// The settings as JSON with a comment over each key.
    pub fn to_commented_json(&self) -> String {
        let json = |value: &dyn erased::ToJson| value.to_json().replace('\n', "\n  ");
        let themes = crate::theme::builtin()
            .iter()
            .map(|t| format!("\"{}\"", t.name))
            .collect::<Vec<_>>()
            .join(", ");
        let id_width = ALL.iter().map(|c| c.id().len()).max().unwrap_or(0);
        let reference = ALL
            .iter()
            .map(|c| {
                format!(
                    "  //   {:id_width$}  {}",
                    c.id(),
                    default_chord(*c).unwrap_or("—")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            r#"// Yara Code settings. Every key is optional: leave one out and the built-in
// default applies. Lines starting with // are comments; the editor writes the
// file afresh, with these comments, whenever it saves a setting. A folder's
// own .ycode/settings.json lays any of these keys over this file.
{{
  // Colour theme, one of those built in:
  //   {themes}
  // or the name of any VS Code theme JSON dropped in the themes/ folder
  // beside this file.
  "theme": {theme},

  // The command that runs in the AGENT pane.
  "agent": {agent},

  // Layout: which side the AGENT pane sits on ("left" or "right") and its
  // share of the width in percent; the FILES sidebar's width in columns and
  // whether it is open at start.
  "agent_side": {agent_side},
  "agent_width": {agent_width},
  "sidebar_width": {sidebar_width},
  "show_sidebar": {show_sidebar},

  // Under this many columns the panes take turns instead of sharing the
  // width: the one with the keyboard fills the body, and Next Pane brings
  // up another. 0 keeps them side by side at any width.
  "narrow_width": {narrow_width},

  // The terminal under the agent (Ctrl+T): the shell it runs ("" = $SHELL)
  // and its share of that pane's height, in percent — dragging its top
  // border sets it too.
  "shell": {shell},
  "terminal_height": {terminal_height},

  // Edits on the timeline strip before it windows around the current one.
  "timeline_ticks": {timeline_ticks},

  // One step of the editor caret's blink, in milliseconds; 0 keeps it on.
  "cursor_blink_ms": {cursor_blink_ms},

  // Whether the editor says, at the end of the line under the mouse, who
  // last committed it and when.
  "blame": {blame},

  // How often the working tree is checked for the agent's edits, in
  // milliseconds, and the branch CHANGES are measured against ("" = the one
  // the main working copy has checked out).
  "refresh_ms": {refresh_ms},
  "base_branch": {base_branch},

  // Where a new tab's worktree is made ("" = a <repo>-worktrees folder
  // beside the repository).
  "worktrees_dir": {worktrees_dir},

  // What Agent Usage (F8) types at each agent, by program name:
  // the agents only show their limits from inside their own session.
  "usage_slash": {usage_slash},

  // Or, a command per agent that prints what it has used of its plan, as JSON:
  //   {{"plan": "Max", "percent": 62, "detail": "1.2M tokens", "reset": "in 3h"}}
  // for example  "usage_commands": {{ "claude": "my-claude-usage" }}. Set, it
  // is shown by Agent Usage (F8) instead, and as the chip in the header.
  "usage_commands": {usage_commands},

  // Folders nobody works in, left out of the tree, Go to File, Search and
  // the watching that feeds the timeline. A name matches a folder anywhere;
  // "." on its own stands for every hidden folder.
  "ignore_folders": {ignore_folders},

  // What Search Project leaves out on top of those: a bare name matches a
  // folder anywhere, "*.lock" a file, "src/**/gen" a path.
  "search_exclude": {search_exclude},

  // With the agent focused, its own keys and every unbound one reach it;
  // a bound Ctrl/Alt chord or function key is the editor's — except these,
  // which the programs in that pane use themselves.
  "agent_keys": {agent_keys},

  // Key bindings, command id to chord. Only bindings that differ from the
  // defaults need listing, for example  "keys": {{ "save": "Ctrl+D" }}.
  // Chords are Ctrl/Alt/Shift + a key: "F3", "Ctrl+-", "Alt+Left",
  // "F12", or a bare key like "F" for the follow pane. The commands and
  // their defaults (see also {docs}guides/keys/):
{reference}
  "keys": {keys},

  // Workspaces offered by File → Open Recent, newest first, each the list of
  // folders it holds. Kept by the editor.
  "recent_workspaces": {recent_workspaces}
}}
"#,
            theme = json(&self.theme),
            agent = json(&self.agent),
            agent_side = json(&self.agent_side),
            agent_width = json(&self.agent_width),
            sidebar_width = json(&self.sidebar_width),
            show_sidebar = json(&self.show_sidebar),
            narrow_width = json(&self.narrow_width),
            shell = json(&self.shell),
            terminal_height = json(&self.terminal_height),
            timeline_ticks = json(&self.timeline_ticks),
            cursor_blink_ms = json(&self.cursor_blink_ms),
            blame = json(&self.blame),
            refresh_ms = json(&self.refresh_ms),
            base_branch = json(&self.base_branch),
            worktrees_dir = json(&self.worktrees_dir),
            agent_keys = json(&self.agent_keys),
            search_exclude = json(&self.search_exclude),
            ignore_folders = json(&self.ignore_folders),
            usage_commands = json(&self.usage_commands),
            usage_slash = json(&self.usage_slash),
            keys = json(&self.keys),
            recent_workspaces = json(&self.recent_workspaces),
            docs = crate::DOCUMENTATION,
        )
    }

    /// When the file was last written, so a frontend can notice an edit made
    /// outside the editor and apply it without being told.
    pub fn stamp() -> Option<std::time::SystemTime> {
        std::fs::metadata(Self::path()?).ok()?.modified().ok()
    }

    /// Records a workspace — its folders — in the recent list, newest
    /// first, capped at 15. A workspace with no folders is not one yet.
    pub fn push_recent(&mut self, folders: &[PathBuf]) {
        if folders.is_empty() {
            return;
        }
        self.recent_workspaces.retain(|w| w != folders);
        self.recent_workspaces.insert(0, folders.to_vec());
        self.recent_workspaces.truncate(15);
    }

    pub fn chord(&self, command: Command) -> Option<&Chord> {
        self.keys.map.get(command.id())
    }

    pub fn command(&self, chord: &Chord) -> Option<Command> {
        self.keys
            .map
            .iter()
            .find(|(_, bound)| *bound == chord)
            .and_then(|(id, _)| Command::from_id(id))
    }
}

/// Serialises one value the way the file writes it. A trait object so the
/// template above can hold values of different types in one closure.
mod erased {
    pub trait ToJson {
        fn to_json(&self) -> String;
    }
    impl<T: serde::Serialize> ToJson for T {
        fn to_json(&self) -> String {
            serde_json::to_string_pretty(self).unwrap_or_else(|_| "null".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Dir, ENV_LOCK};

    #[test]
    fn a_folder_nobody_works_in_is_known_by_name_or_by_its_dot() {
        let settings = Settings::default();
        assert!(settings.ignores(".git") && settings.ignores(".venv"));
        assert!(settings.ignores("node_modules") && settings.ignores("target"));
        assert!(!settings.ignores("src") && !settings.ignores("crates"));
        let named: Settings = serde_json::from_str(r#"{"ignore_folders":["logs"]}"#).unwrap();
        assert!(
            named.ignores("logs") && !named.ignores(".git"),
            "the list is the list"
        );
    }

    #[test]
    fn an_empty_file_still_gives_every_default() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.theme, "Dark Modern");
        assert!(settings.chord(Command::Save).is_some());
    }

    #[test]
    fn partial_json_keeps_the_rest() {
        let settings: Settings =
            serde_json::from_str(r#"{"agent":"codex","agent_side":"right","agent_width":50}"#)
                .unwrap();
        assert_eq!(settings.agent, "codex");
        assert_eq!(settings.agent_side, Side::Right);
        assert_eq!(settings.agent_width, 50);
        assert_eq!(settings.sidebar_width, 30);
    }

    #[test]
    fn a_folders_file_lays_its_keys_over_the_global_ones_and_maps_merge() {
        let dir = Dir::new("yara-settings-local");
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("YARA_CONFIG_DIR", dir.path().join("config"));
        Settings {
            agent: "codex".into(),
            agent_width: 60,
            keys: serde_json::from_str(r#"{"save": "Ctrl+O"}"#).unwrap(),
            ..Default::default()
        }
        .save()
        .unwrap();
        let folder = dir.path().join("project");
        std::fs::create_dir_all(folder.join(".ycode")).unwrap();
        std::fs::write(
            Settings::local_path(&folder),
            "// this folder's agent\n{\"agent\": \"claude\", \"keys\": {\"quit\": \"Ctrl+X\"}}",
        )
        .unwrap();

        let (settings, complaint) = Settings::load_in(Some(&folder));
        assert_eq!(complaint, None);
        assert_eq!(settings.agent, "claude", "the folder's key wins");
        assert_eq!(settings.agent_width, 60, "the rest is the global file's");
        assert_eq!(
            settings.chord(Command::Save),
            Some(&"Ctrl+O".parse().unwrap())
        );
        assert_eq!(
            settings.chord(Command::Quit),
            Some(&"Ctrl+X".parse().unwrap())
        );
        assert_eq!(settings.local.len(), 2);

        // A save from that folder keeps its keys out of the global file.
        let mut settings = settings;
        settings.agent_width = 50;
        settings.save().unwrap();
        let (global, _) = Settings::load();
        assert_eq!(global.agent, "codex");
        assert_eq!(global.agent_width, 50);
        assert_eq!(
            global.chord(Command::Quit),
            Some(&"Ctrl+Q".parse().unwrap())
        );
        assert!(global.local.is_empty());

        // Without a folder, or with one that has no file, it is the global file.
        assert_eq!(Settings::load_in(None).0, global);
        assert_eq!(Settings::load_in(Some(dir.path())).0, global);
        std::env::remove_var("YARA_CONFIG_DIR");
    }

    #[test]
    fn a_broken_folder_file_is_named_and_skipped_and_a_made_one_sets_nothing() {
        let dir = Dir::new("yara-settings-local-broken");
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("YARA_CONFIG_DIR", dir.path().join("config"));
        let folder = dir.path().join("project");
        std::fs::create_dir_all(folder.join(".ycode")).unwrap();
        std::fs::write(Settings::local_path(&folder), "{\"agent\": \"codex\",}").unwrap();
        let (settings, complaint) = Settings::load_in(Some(&folder));
        assert_eq!(settings, Settings::default());
        assert!(complaint.unwrap().contains(".ycode"));

        std::fs::remove_file(Settings::local_path(&folder)).unwrap();
        let path = Settings::ensure_local_file(&folder).unwrap();
        assert_eq!(path, folder.join(".ycode").join("settings.json"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("wins over"), "{text}");
        let (settings, complaint) = Settings::load_in(Some(&folder));
        assert_eq!(complaint, None);
        assert_eq!(settings, Settings::default());
        assert_eq!(
            Settings::ensure_local_file(&folder).unwrap(),
            path,
            "a second time opens the file as it is"
        );
        std::env::remove_var("YARA_CONFIG_DIR");
    }

    #[test]
    fn the_paste_key_is_the_editors_even_in_the_agents_pane() {
        let settings = Settings::default();
        let paste = settings.chord(Command::Paste).unwrap();
        assert!(
            !settings.agent_keys.contains(paste),
            "Ctrl+V pastes the clipboard at the program, it is never typed at it"
        );
        assert!(settings.agent_keys.contains(&"Ctrl+C".parse().unwrap()));
    }

    #[test]
    fn every_command_is_bound_out_of_the_box_and_no_chord_twice() {
        let settings = Settings::default();
        for command in ALL {
            let unbound = matches!(
                command,
                Command::CheckForUpdates
                    | Command::InstallUpdate
                    | Command::SwapPanes
                    | Command::HelpMenu
                    | Command::AddFolder
                    | Command::RemoveFolder
                    | Command::NewFolder
                    | Command::LocalSettings
                    | Command::Documentation
            );
            assert_eq!(
                settings.chord(*command).is_none(),
                unbound,
                "{}",
                command.id()
            );
        }
        assert_eq!(settings.binding_complaint(), None);
    }

    #[test]
    fn a_chord_resolves_to_its_command_and_back() {
        let settings = Settings::default();
        let save = settings.chord(Command::Save).unwrap().clone();
        assert_eq!(settings.command(&save), Some(Command::Save));
        // Ctrl+Shift+F is Search, not a Ctrl+F with a stray Shift.
        let search: Chord = "F3".parse().unwrap();
        assert_eq!(settings.command(&search), Some(Command::SearchProject));
        let live: Chord = "F".parse().unwrap();
        assert_eq!(settings.command(&live), Some(Command::FollowLive));
        let unbound: Chord = "Ctrl+Alt+Shift+F9".parse().unwrap();
        assert_eq!(settings.command(&unbound), None);
    }

    #[test]
    fn rebinding_one_key_keeps_the_rest_and_only_the_change_is_saved() {
        let settings: Settings = serde_json::from_str(r#"{"keys":{"save":"Ctrl+D"}}"#).unwrap();
        assert_eq!(settings.chord(Command::Save).unwrap().to_string(), "Ctrl+D");
        assert_eq!(settings.chord(Command::Quit).unwrap().to_string(), "Ctrl+Q");
        let value: serde_json::Value = serde_json::to_value(&settings).unwrap();
        assert_eq!(value["keys"], serde_json::json!({"save": "Ctrl+D"}));
    }

    #[test]
    fn a_file_from_before_v1_still_reads_its_terminal_keys() {
        let settings: Settings =
            serde_json::from_str(r#"{"keys":{"gui":{"save":"Cmd+D"},"tui":{"save":"Ctrl+D"}}}"#)
                .unwrap();
        assert_eq!(settings.chord(Command::Save).unwrap().to_string(), "Ctrl+D");
    }

    #[test]
    fn a_bad_chord_keeps_its_default_is_reported_and_survives_a_save() {
        let settings: Settings =
            serde_json::from_str(r#"{"keys":{"save":"Ctrl+D","quit":"Ctrl+"}}"#).unwrap();
        assert_eq!(settings.chord(Command::Quit).unwrap().to_string(), "Ctrl+Q");
        let complaint = settings.binding_complaint().unwrap();
        assert!(
            complaint.contains("quit") && complaint.contains("Ctrl+"),
            "{complaint}"
        );
        assert!(settings.to_commented_json().contains(r#""quit": "Ctrl+""#));
    }

    #[test]
    fn a_binding_given_to_two_commands_is_reported() {
        let settings: Settings =
            serde_json::from_str(r#"{"keys":{"file_menu":"Ctrl+S"}}"#).unwrap();
        let message = settings.binding_complaint().unwrap();
        assert!(
            message.contains("save") && message.contains("file_menu"),
            "{message}"
        );
    }

    #[test]
    fn the_recent_list_is_newest_first_deduped_and_capped() {
        let mut settings = Settings::default();
        let one = |i: usize| vec![PathBuf::from(format!("/p/{i}"))];
        for i in 0..20 {
            settings.push_recent(&one(i));
        }
        settings.push_recent(&one(10));
        assert_eq!(settings.recent_workspaces.len(), 15);
        assert_eq!(settings.recent_workspaces[0], one(10));
        assert_eq!(settings.recent_workspaces[1], one(19));
        assert_eq!(
            settings
                .recent_workspaces
                .iter()
                .filter(|w| **w == one(10))
                .count(),
            1
        );
        // A workspace of several folders is one entry, folders and all.
        let two = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        settings.push_recent(&two);
        assert_eq!(settings.recent_workspaces[0], two);
        settings.push_recent(&[]);
        assert_eq!(
            settings.recent_workspaces[0], two,
            "nothing is not a workspace"
        );
    }

    #[test]
    fn comments_are_stripped_outside_strings_only() {
        let text = "{\n  // a note\n  \"theme\": \"Monokai\", // trailing\n  \"recent_projects\": [\"http://x//y\", \"a\\\"//b\"]\n}\n";
        let value: serde_json::Value = serde_json::from_str(&strip_comments(text)).unwrap();
        assert_eq!(value["theme"], "Monokai");
        assert_eq!(value["recent_projects"][0], "http://x//y");
        assert_eq!(value["recent_projects"][1], "a\"//b");
    }

    #[test]
    fn the_written_file_explains_itself_and_reads_back_whole() {
        let mut settings = Settings {
            theme: "Dark Modern".into(),
            show_sidebar: true,
            ..Default::default()
        };
        settings
            .keys
            .map
            .insert("save".into(), "Ctrl+D".parse().unwrap());
        settings.push_recent(&[PathBuf::from("/tmp/p")]);
        let text = settings.to_commented_json();
        for key in [
            "\"theme\"",
            "\"agent\"",
            "\"agent_side\"",
            "\"agent_width\"",
            "\"sidebar_width\"",
            "\"show_sidebar\"",
            "\"narrow_width\"",
            "\"shell\"",
            "\"terminal_height\"",
            "\"timeline_ticks\"",
            "\"cursor_blink_ms\"",
            "\"blame\"",
            "\"refresh_ms\"",
            "\"base_branch\"",
            "\"worktrees_dir\"",
            "\"agent_keys\"",
            "\"search_exclude\"",
            "\"ignore_folders\"",
            "\"usage_commands\"",
            "\"usage_slash\"",
            "\"keys\"",
            "\"recent_workspaces\"",
        ] {
            let at = text
                .find(key)
                .unwrap_or_else(|| panic!("{key} missing:\n{text}"));
            assert!(text[..at].contains("//"), "{key} has no comment");
        }
        for command in ALL {
            let line = text
                .lines()
                .find(|l| l.split_whitespace().nth(1) == Some(command.id()))
                .unwrap_or_else(|| panic!("{} missing from the reference", command.id()));
            if let Some(chord) = default_chord(*command) {
                assert!(line.contains(chord), "{line}");
            }
        }
        let back: Settings = serde_json::from_str(&strip_comments(&text)).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn the_file_round_trips_through_disk_and_a_broken_one_is_not_written_over() {
        let dir = Dir::new("yara-settings");
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("YARA_CONFIG_DIR", dir.path());
        assert_eq!(Settings::stamp(), None, "nothing written yet");
        let settings = Settings {
            agent: "codex".into(),
            ..Default::default()
        };
        assert_eq!(settings.ensure_file().unwrap(), Settings::path().unwrap());
        assert!(Settings::stamp().is_some());
        let (loaded, complaint) = Settings::load();
        assert_eq!(complaint, None);
        assert_eq!(loaded, settings);

        // One trailing comma: the file the user has, mistake and all.
        let broken = "{\n  \"theme\": \"Monokai\",\n}\n";
        let path = Settings::path().unwrap();
        std::fs::write(&path, broken).unwrap();
        let (mut settings, complaint) = Settings::load();
        assert!(complaint.unwrap().contains("not be written over"));
        settings.push_recent(&[dir.path().to_path_buf()]);
        assert!(settings.save().is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        std::env::remove_var("YARA_CONFIG_DIR");
    }
}

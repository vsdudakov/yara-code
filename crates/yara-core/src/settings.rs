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

/// Keys every terminal can send: function keys and plain Ctrl+letter. There
/// is no Ctrl+Shift here on purpose — without the kitty keyboard protocol a
/// terminal cannot tell it from Ctrl, and most terminals do not have it.
pub fn default_chord(command: Command) -> Option<&'static str> {
    Some(match command {
        Command::NewFile => "Ctrl+N",
        Command::OpenFolder => "Ctrl+O",
        Command::OpenRecent => "Ctrl+R",
        Command::Save => "Ctrl+S",
        Command::Settings => "F12",
        Command::Quit => "Ctrl+Q",
        Command::Documentation => "Shift+F12",
        Command::Help => "F1",
        // Updating is a menu action; a key for it would only be pressed by
        // accident.
        Command::CheckForUpdates | Command::InstallUpdate => return None,
        Command::ToggleSidebar => "Ctrl+B",
        Command::Changes => "F4",
        Command::CommandPalette => "F5",
        Command::SearchProject => "F3",
        Command::AgentUsage => "F8",
        Command::ThemePicker => "F9",
        Command::QuickOpen => "Ctrl+P",
        Command::FileMenu => "F10",
        Command::HelpMenu => "Shift+F1",
        // The keyboard's own key for moving between parts of a window, and
        // one no program in the agent pane is listening for.
        Command::NextPane => "F6",
        Command::Close => "Esc",
        Command::NewTab => "F7",
        Command::CloseTab => "Ctrl+W",
        Command::NextTab => "Ctrl+PageDown",
        Command::PrevTab => "Ctrl+PageUp",
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
    /// Edits shown on the timeline strip before it windows around the cursor.
    pub timeline_ticks: usize,
    /// One step of the editor caret's blink, in milliseconds; 0 keeps it on.
    pub cursor_blink_ms: u64,
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
    /// What project search leaves out, in VS Code's glob spelling.
    pub search_exclude: Vec<String>,
    /// Chords the agent keeps even though the editor binds them, because
    /// the programs in that pane use them themselves.
    pub agent_keys: Vec<Chord>,
    pub keys: Keys,
    /// Recently opened project folders, most recent first.
    pub recent_projects: Vec<PathBuf>,
    /// The file could not be read. Nothing is written over it until it can:
    /// a save would replace the user's file, typo and all, with defaults.
    #[serde(skip)]
    pub unreadable: bool,
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
            timeline_ticks: 12,
            cursor_blink_ms: 500,
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
            search_exclude: ["target", "node_modules", ".*"].map(String::from).to_vec(),
            agent_keys: [
                "Ctrl+R", "Ctrl+N", "Ctrl+Z", "Ctrl+O", "Ctrl+W", "Ctrl+Y", "Ctrl+C", "Ctrl+V",
            ]
            .iter()
            .filter_map(|c| c.parse().ok())
            .collect(),
            keys: Keys::default(),
            recent_projects: Vec::new(),
            unreadable: false,
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
    /// `$XDG_CONFIG_HOME/ycode/settings.json`, else `~/.config/ycode/...`.
    pub fn path() -> Option<PathBuf> {
        Some(crate::config_dir()?.join("settings.json"))
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
        std::fs::write(&path, self.to_commented_json())?;
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
// file afresh, with these comments, whenever it saves a setting.
{{
  // Colour theme: {themes}, or the name of any VS Code theme JSON dropped in
  // the themes/ folder beside this file.
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

  // Edits on the timeline strip before it windows around the current one.
  "timeline_ticks": {timeline_ticks},

  // One step of the editor caret's blink, in milliseconds; 0 keeps it on.
  "cursor_blink_ms": {cursor_blink_ms},

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

  // What Search Project leaves out: a bare name matches a folder anywhere,
  // "*.lock" a file, "src/**/gen" a path.
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

  // Folders offered by File → Open Recent, newest first. Kept by the editor.
  "recent_projects": {recent_projects}
}}
"#,
            theme = json(&self.theme),
            agent = json(&self.agent),
            agent_side = json(&self.agent_side),
            agent_width = json(&self.agent_width),
            sidebar_width = json(&self.sidebar_width),
            show_sidebar = json(&self.show_sidebar),
            timeline_ticks = json(&self.timeline_ticks),
            cursor_blink_ms = json(&self.cursor_blink_ms),
            refresh_ms = json(&self.refresh_ms),
            base_branch = json(&self.base_branch),
            worktrees_dir = json(&self.worktrees_dir),
            agent_keys = json(&self.agent_keys),
            search_exclude = json(&self.search_exclude),
            usage_commands = json(&self.usage_commands),
            usage_slash = json(&self.usage_slash),
            keys = json(&self.keys),
            recent_projects = json(&self.recent_projects),
            docs = crate::DOCUMENTATION,
        )
    }

    /// When the file was last written, so a frontend can notice an edit made
    /// outside the editor and apply it without being told.
    pub fn stamp() -> Option<std::time::SystemTime> {
        std::fs::metadata(Self::path()?).ok()?.modified().ok()
    }

    /// Records a project in the recent list, newest first, capped at 15.
    pub fn push_recent(&mut self, root: &Path) {
        self.recent_projects.retain(|p| p != root);
        self.recent_projects.insert(0, root.to_path_buf());
        self.recent_projects.truncate(15);
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
    fn every_command_is_bound_out_of_the_box_and_no_chord_twice() {
        let settings = Settings::default();
        for command in ALL {
            let unbound = matches!(command, Command::CheckForUpdates | Command::InstallUpdate);
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
        for i in 0..20 {
            settings.push_recent(&PathBuf::from(format!("/p/{i}")));
        }
        settings.push_recent(&PathBuf::from("/p/10"));
        assert_eq!(settings.recent_projects.len(), 15);
        assert_eq!(settings.recent_projects[0], PathBuf::from("/p/10"));
        assert_eq!(settings.recent_projects[1], PathBuf::from("/p/19"));
        assert_eq!(
            settings
                .recent_projects
                .iter()
                .filter(|p| p.ends_with("10"))
                .count(),
            1
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
        settings.recent_projects.push(PathBuf::from("/tmp/p"));
        let text = settings.to_commented_json();
        for key in [
            "\"theme\"",
            "\"agent\"",
            "\"agent_side\"",
            "\"agent_width\"",
            "\"sidebar_width\"",
            "\"show_sidebar\"",
            "\"timeline_ticks\"",
            "\"cursor_blink_ms\"",
            "\"refresh_ms\"",
            "\"base_branch\"",
            "\"worktrees_dir\"",
            "\"agent_keys\"",
            "\"search_exclude\"",
            "\"usage_commands\"",
            "\"usage_slash\"",
            "\"keys\"",
            "\"recent_projects\"",
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
        settings.push_recent(dir.path());
        assert!(settings.save().is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        std::env::remove_var("YARA_CONFIG_DIR");
    }
}

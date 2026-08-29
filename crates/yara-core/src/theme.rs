//! Themes as data, not constants.
//!
//! A [`Theme`] carries every colour the frontend paints with: the chrome
//! roles below, the 16 ANSI colours of the agent's terminal, and syntax
//! rules keyed by TextMate scope. Themes come from the built-ins here, from
//! JSON files in the user's config directory, and from any VS Code theme
//! file, which this module reads directly.

use std::path::{Path, PathBuf};

pub type Rgb = (u8, u8, u8);

pub const fn rgb(hex: u32) -> Rgb {
    ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Chrome colours, by role. `accent` is deletions, unreviewed edits and the
/// live border; `success` is additions, the agent at work, the worktree.
#[derive(Clone, Debug, PartialEq)]
pub struct Ui {
    pub bg: Rgb,
    pub fg: Rgb,
    /// Secondary text: paths, hints, counters, line numbers.
    pub fg_dim: Rgb,
    /// An idle pane's border.
    pub border: Rgb,
    pub accent: Rgb,
    pub accent_dim: Rgb,
    /// The ground of a removed line.
    pub accent_bg: Rgb,
    pub success: Rgb,
    pub success_dim: Rgb,
    /// The ground of an added line.
    pub success_bg: Rgb,
    /// The row under the cursor in a list.
    pub selected_bg: Rgb,
    /// The row under the mouse.
    pub hover_bg: Rgb,
    pub match_bg: Rgb,
    pub cursor: Rgb,
}

/// One syntax rule: a TextMate scope selector and how to paint it.
#[derive(Clone, Debug, PartialEq)]
pub struct TokenRule {
    pub scope: String,
    pub color: Rgb,
    pub italic: bool,
    pub bold: bool,
}

impl TokenRule {
    pub fn new(scope: &str, color: u32) -> Self {
        Self {
            scope: scope.to_string(),
            color: rgb(color),
            italic: false,
            bold: false,
        }
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub dark: bool,
    pub ui: Ui,
    pub ansi: [Rgb; 16],
    pub tokens: Vec<TokenRule>,
}

impl Default for Theme {
    fn default() -> Self {
        dark_modern()
    }
}

fn ansi_from(hexes: [u32; 16]) -> [Rgb; 16] {
    hexes.map(rgb)
}

/// Expands an ANSI index into a full RGB triple, covering the 256-colour cube
/// for indices past the 16 the theme defines.
pub fn ansi256(theme: &Theme, idx: u8) -> Rgb {
    match idx {
        0..=15 => theme.ansi[idx as usize],
        16..=231 => {
            let n = idx - 16;
            let comp = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            (comp(n / 36), comp((n % 36) / 6), comp(n % 6))
        }
        232..=255 => {
            let g = 8 + 10 * (idx - 232);
            (g, g, g)
        }
    }
}

/// VS Code's default, "Dark Modern", with comments sat back and module
/// names in imports coloured so the eye stays on the code.
pub fn dark_modern() -> Theme {
    Theme {
        name: "Dark Modern".into(),
        dark: true,
        ui: Ui {
            bg: rgb(0x1F1F1F),
            fg: rgb(0xCCCCCC),
            fg_dim: rgb(0x8B8B8B),
            border: rgb(0x2B2B2B),
            accent: rgb(0x3F7396),
            accent_dim: rgb(0x7FAFCC),
            accent_bg: rgb(0x4B1818),
            success: rgb(0x2EA043),
            success_dim: rgb(0x56D364),
            success_bg: rgb(0x1B3B22),
            selected_bg: rgb(0x04395E),
            hover_bg: rgb(0x2A2D2E),
            match_bg: rgb(0x724514),
            cursor: rgb(0xAEAFAD),
        },
        ansi: ansi_from([
            0x000000, 0xCD3131, 0x0DBC79, 0xE5E510, 0x2472C8, 0xBC3FBC, 0x11A8CD, 0xE5E5E5,
            0x666666, 0xF14C4C, 0x23D18B, 0xF5F543, 0x3B8EEA, 0xD670D6, 0x29B8DB, 0xFFFFFF,
        ]),
        tokens: vec![
            TokenRule::new("comment", 0x5C6370),
            TokenRule::new("string", 0xCE9178),
            TokenRule::new("string.regexp", 0xD16969),
            TokenRule::new("constant.character.escape", 0xD7BA7D),
            TokenRule::new("constant.numeric", 0xB5CEA8),
            TokenRule::new(
                "constant.language, constant.character, support.constant",
                0x569CD6,
            ),
            TokenRule::new("keyword, storage.modifier, storage.type", 0x569CD6),
            TokenRule::new("keyword.control", 0xC586C0),
            TokenRule::new("keyword.operator", 0xD4D4D4),
            TokenRule::new(
                "entity.name.function, support.function, variable.function, support.macro",
                0xDCDCAA,
            ),
            TokenRule::new(
                "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, \
                 entity.name.union, entity.name.trait, support.type, support.class, \
                 entity.other.inherited-class, meta.generic",
                0x4EC9B0,
            ),
            TokenRule::new("variable.parameter", 0x9CDCFE),
            TokenRule::new("variable", 0x9CDCFE),
            TokenRule::new("entity.name.tag", 0x569CD6),
            TokenRule::new("entity.other.attribute-name", 0x9CDCFE),
            TokenRule::new("markup.heading", 0x569CD6).bold(),
            TokenRule::new("markup.bold", 0xD4D4D4).bold(),
            TokenRule::new("markup.italic", 0xD4D4D4).italic(),
            TokenRule::new("punctuation", 0xD4D4D4),
            TokenRule::new(IMPORTS, 0x4EC9B0),
        ],
    }
}

/// The module and namespace names an import names, which most grammars
/// leave as plain identifiers.
const IMPORTS: &str = "entity.name.namespace, entity.name.module, support.other.namespace, \
                       meta.path, variable.other.module, entity.name.import";

/// The one theme that ships: VS Code's, so the editor looks like the one
/// beside it. Any other is a VS Code theme JSON in the user's themes folder.
pub fn builtin() -> Vec<Theme> {
    vec![dark_modern()]
}

/// Where user themes live: `~/.config/ycode/themes`, beside the settings.
pub fn user_theme_dir() -> Option<PathBuf> {
    Some(crate::config_dir()?.join("themes"))
}

/// Built-in themes plus every `*.json` in the user theme directory. A user
/// theme with a built-in's name replaces it.
pub fn load_all() -> Vec<Theme> {
    let mut themes = builtin();
    let Some(entries) = user_theme_dir().and_then(|dir| std::fs::read_dir(dir).ok()) else {
        return themes;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    for theme in paths.iter().filter_map(|p| from_vscode_file(p).ok()) {
        match themes.iter().position(|t| t.name == theme.name) {
            Some(i) => themes[i] = theme,
            None => themes.push(theme),
        }
    }
    themes
}

/// The theme of that name, or the default when there is none.
pub fn by_name<'a>(themes: &'a [Theme], name: &str) -> Option<&'a Theme> {
    themes.iter().find(|t| t.name == name)
}

pub fn from_vscode_file(path: &Path) -> Result<Theme, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let fallback = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Custom".to_string());
    from_vscode_json(&text, &fallback)
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let digits: Vec<char> = s.trim().trim_start_matches('#').chars().collect();
    // Read by char and never by byte, so a stray non-ASCII character in a
    // theme file is a bad colour, not a panic.
    if !digits.iter().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let hex = |a: char, b: char| u8::from_str_radix(&format!("{a}{b}"), 16).ok();
    match digits.len() {
        3 => Some((
            hex(digits[0], digits[0])?,
            hex(digits[1], digits[1])?,
            hex(digits[2], digits[2])?,
        )),
        6 | 8 => Some((
            hex(digits[0], digits[1])?,
            hex(digits[2], digits[3])?,
            hex(digits[4], digits[5])?,
        )),
        _ => None,
    }
}

/// Reads a VS Code colour-theme JSON: `colors` drives the chrome and the
/// terminal palette, `tokenColors` the syntax. Anything the file omits keeps
/// Dark Modern's value.
pub fn from_vscode_json(text: &str, fallback_name: &str) -> Result<Theme, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let light = value.get("type").and_then(|v| v.as_str()) == Some("light");
    let mut theme = dark_modern();
    theme.dark = !light;
    theme.name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(fallback_name)
        .to_string();

    let get = |key: &str| {
        value["colors"]
            .get(key)
            .and_then(|v| v.as_str())
            .and_then(parse_hex)
    };
    let ui = &mut theme.ui;
    for (slot, key) in [
        (&mut ui.bg, "editor.background"),
        (&mut ui.fg, "editor.foreground"),
        (&mut ui.fg_dim, "descriptionForeground"),
        (&mut ui.border, "panel.border"),
        (&mut ui.accent, "focusBorder"),
        (&mut ui.accent_dim, "textLink.foreground"),
        (&mut ui.accent_bg, "diffEditor.removedLineBackground"),
        (&mut ui.success, "gitDecoration.addedResourceForeground"),
        (&mut ui.success_dim, "terminal.ansiBrightGreen"),
        (&mut ui.success_bg, "diffEditor.insertedLineBackground"),
        (&mut ui.selected_bg, "list.activeSelectionBackground"),
        (&mut ui.hover_bg, "list.hoverBackground"),
        (&mut ui.match_bg, "editor.findMatchHighlightBackground"),
        (&mut ui.cursor, "editorCursor.foreground"),
    ] {
        if let Some(c) = get(key) {
            *slot = c;
        }
    }
    const ANSI: [&str; 16] = [
        "Black",
        "Red",
        "Green",
        "Yellow",
        "Blue",
        "Magenta",
        "Cyan",
        "White",
        "BrightBlack",
        "BrightRed",
        "BrightGreen",
        "BrightYellow",
        "BrightBlue",
        "BrightMagenta",
        "BrightCyan",
        "BrightWhite",
    ];
    for (slot, name) in theme.ansi.iter_mut().zip(ANSI) {
        if let Some(c) = get(&format!("terminal.ansi{name}")) {
            *slot = c;
        }
    }

    let rules: Vec<TokenRule> = value["tokenColors"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let settings = &entry["settings"];
                    let color = parse_hex(settings["foreground"].as_str()?)?;
                    let style = settings["fontStyle"].as_str().unwrap_or("");
                    let scope = match &entry["scope"] {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Array(items) => items
                            .iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        _ => return None,
                    };
                    (!scope.trim().is_empty()).then(|| TokenRule {
                        scope,
                        color,
                        italic: style.contains("italic"),
                        bold: style.contains("bold"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if !rules.is_empty() {
        theme.tokens = rules;
    }
    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Dir;

    #[test]
    fn dark_modern_is_the_one_theme_that_ships() {
        let names: Vec<String> = builtin().into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["Dark Modern"]);
        assert_eq!(Theme::default().name, "Dark Modern");
        assert!(dark_modern().dark);
        assert_eq!(
            by_name(&builtin(), "Dark Modern").unwrap().ui.bg,
            rgb(0x1F1F1F)
        );
        assert!(by_name(&builtin(), "Nope").is_none());
    }

    #[test]
    fn the_ansi_palette_answers_for_every_index() {
        let theme = dark_modern();
        assert_eq!(ansi256(&theme, 1), theme.ansi[1]);
        assert_eq!(ansi256(&theme, 16), (0, 0, 0));
        assert_eq!(ansi256(&theme, 231), (255, 255, 255));
        let (dark, light) = (ansi256(&theme, 232), ansi256(&theme, 255));
        assert!(dark.0 < light.0);
        assert_eq!(dark.0, dark.1, "the ramp is grey, not tinted");
    }

    #[test]
    fn a_vs_code_theme_json_becomes_a_theme() {
        let json = r##"{
            "name": "Test Theme",
            "type": "light",
            "colors": {
                "editor.background": "#fafafa",
                "editor.foreground": "#101010",
                "terminal.ansiRed": "#ff0000",
                "diffEditor.insertedLineBackground": "#e6ffec80"
            },
            "tokenColors": [
                { "scope": "comment", "settings": { "foreground": "#008000", "fontStyle": "italic" } },
                { "scope": ["keyword", "storage"], "settings": { "foreground": "#0000ff", "fontStyle": "bold" } },
                { "settings": { "foreground": "#123456" } }
            ]
        }"##;
        let theme = from_vscode_json(json, "ignored").unwrap();
        assert_eq!(theme.name, "Test Theme");
        assert!(!theme.dark, "type: light");
        assert_eq!(theme.ui.bg, (250, 250, 250));
        assert_eq!(theme.ui.fg, (16, 16, 16));
        assert_eq!(theme.ui.success_bg, (0xE6, 0xFF, 0xEC), "alpha is dropped");
        assert_eq!(theme.ansi[1], (255, 0, 0));
        // Untouched keys keep Dark Modern's value.
        assert_eq!(theme.ui.border, dark_modern().ui.border);
        let comment = theme.tokens.iter().find(|t| t.scope == "comment").unwrap();
        assert_eq!(comment.color, (0, 128, 0));
        assert!(comment.italic);
        let keywords = theme
            .tokens
            .iter()
            .find(|t| t.scope == "keyword, storage")
            .expect("both scopes in one selector");
        assert!(keywords.bold && !keywords.italic);
        // A rule with no scope at all is VS Code's editor default, not a rule.
        assert_eq!(theme.tokens.len(), 2);
    }

    #[test]
    fn a_theme_without_a_name_takes_the_file_name_and_stays_dark() {
        let theme = from_vscode_json(r##"{"colors": {}}"##, "Fallback").unwrap();
        assert_eq!(theme.name, "Fallback");
        assert!(theme.dark);
        assert_eq!(theme.tokens, dark_modern().tokens);
    }

    #[test]
    fn malformed_json_and_missing_files_are_errors() {
        assert!(from_vscode_json("{not json", "x").is_err());
        let dir = Dir::new("yara-theme-missing");
        assert!(from_vscode_file(&dir.path().join("nope.json")).is_err());
    }

    #[test]
    fn a_theme_file_is_loaded_from_disk() {
        let dir = Dir::new("yara-theme-file");
        let path = dir.file(
            "solar.json",
            r##"{"type":"dark","colors":{"editor.background":"#002b36"}}"##,
        );
        let theme = from_vscode_file(&path).unwrap();
        assert_eq!(theme.name, "solar");
        assert_eq!(theme.ui.bg, (0, 43, 54));
    }

    #[test]
    fn colours_are_read_in_every_shape_vs_code_writes_them() {
        assert_eq!(parse_hex("#ffffff"), Some((255, 255, 255)));
        assert_eq!(parse_hex("#abc"), Some((170, 187, 204)), "short form");
        assert_eq!(parse_hex("#11223344"), Some((17, 34, 51)));
        assert_eq!(parse_hex("nonsense"), None);
        assert_eq!(parse_hex(""), None);
        assert_eq!(parse_hex("#ééé"), None);
    }

    #[test]
    fn user_themes_lay_over_the_built_ins_by_name() {
        let dir = Dir::new("yara-user-themes");
        let _lock = crate::test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("YARA_CONFIG_DIR", dir.path());
        dir.file(
            "themes/Dark Modern.json",
            r##"{"name":"Dark Modern","colors":{"editor.background":"#000000"}}"##,
        );
        dir.file("themes/extra.json", r##"{"name":"Extra"}"##);
        dir.file("themes/notes.txt", "not a theme");
        let themes = load_all();
        std::env::remove_var("YARA_CONFIG_DIR");
        assert_eq!(themes.len(), 2);
        assert_eq!(by_name(&themes, "Dark Modern").unwrap().ui.bg, (0, 0, 0));
        assert!(by_name(&themes, "Extra").is_some());
    }
}

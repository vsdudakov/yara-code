//! Themes as data, not constants.
//!
//! A [`Theme`] carries every color both frontends need: chrome colors, the 16
//! ANSI terminal colors, and syntax token rules keyed by TextMate scope. Themes
//! come from three places — the built-ins below, JSON files the user drops in
//! their config directory, and any VS Code theme file, which this module reads
//! directly.

use std::path::{Path, PathBuf};

pub type Rgb = (u8, u8, u8);

pub const fn rgb(hex: u32) -> Rgb {
    ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Chrome colors — everything that isn't source code text.
#[derive(Clone, Debug)]
pub struct Ui {
    pub editor_bg: Rgb,
    pub sidebar_bg: Rgb,
    pub tab_active_bg: Rgb,
    pub tab_inactive_bg: Rgb,
    pub status_bg: Rgb,
    pub border: Rgb,
    pub fg: Rgb,
    pub fg_dim: Rgb,
    pub fg_faint: Rgb,
    pub fg_bright: Rgb,
    pub line_number: Rgb,
    pub accent: Rgb,
    pub accent_light: Rgb,
    pub selection: Rgb,
    pub cursor: Rgb,
    pub hover_bg: Rgb,
    pub selected_bg: Rgb,
    pub match_bg: Rgb,
    pub danger: Rgb,
    pub terminal_fg: Rgb,
}

/// One syntax rule: a TextMate scope selector and how to paint it.
#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: String,
    /// Whether the background is dark; frontends use it to pick base styles.
    pub dark: bool,
    pub ui: Ui,
    pub ansi: [Rgb; 16],
    pub tokens: Vec<TokenRule>,
}

impl Default for Theme {
    fn default() -> Self {
        dark_plus()
    }
}

const DARK_ANSI: [u32; 16] = [
    0x000000, 0xCD3131, 0x0DBC79, 0xE5E510, 0x2472C8, 0xBC3FBC, 0x11A8CD, 0xE5E5E5, 0x666666,
    0xF14C4C, 0x23D18B, 0xF5F543, 0x3B8EEA, 0xD670D6, 0x29B8DB, 0xFFFFFF,
];

fn ansi_from(hexes: [u32; 16]) -> [Rgb; 16] {
    let mut out = [(0, 0, 0); 16];
    for (i, hex) in hexes.into_iter().enumerate() {
        out[i] = rgb(hex);
    }
    out
}

/// Expands an ANSI index into a full RGB triple, covering the 256-color cube
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

pub fn dark_plus() -> Theme {
    Theme {
        name: "Dark+".into(),
        dark: true,
        ui: Ui {
            editor_bg: rgb(0x1E1E1E),
            sidebar_bg: rgb(0x252526),
            tab_active_bg: rgb(0x1E1E1E),
            tab_inactive_bg: rgb(0x2D2D2D),
            status_bg: rgb(0x252526),
            border: rgb(0x2D2D30),
            fg: rgb(0xD4D4D4),
            fg_dim: rgb(0x9D9D9D),
            fg_faint: rgb(0x6E6E6E),
            fg_bright: rgb(0xFFFFFF),
            line_number: rgb(0x858585),
            accent: rgb(0x007ACC),
            accent_light: rgb(0x4FC1FF),
            selection: rgb(0x264F78),
            cursor: rgb(0xAEAFAD),
            hover_bg: rgb(0x2A2D2E),
            selected_bg: rgb(0x094771),
            match_bg: rgb(0x724514),
            danger: rgb(0xA1260D),
            terminal_fg: rgb(0xCCCCCC),
        },
        ansi: ansi_from(DARK_ANSI),
        tokens: vec![
            TokenRule::new("comment", 0x6A9955),
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
        ],
    }
}

pub fn light_plus() -> Theme {
    Theme {
        name: "Light+".into(),
        dark: false,
        ui: Ui {
            editor_bg: rgb(0xFFFFFF),
            sidebar_bg: rgb(0xF3F3F3),
            tab_active_bg: rgb(0xFFFFFF),
            tab_inactive_bg: rgb(0xECECEC),
            status_bg: rgb(0xF3F3F3),
            border: rgb(0xE7E7E7),
            fg: rgb(0x333333),
            fg_dim: rgb(0x616161),
            fg_faint: rgb(0x8E8E8E),
            fg_bright: rgb(0x000000),
            line_number: rgb(0x237893),
            accent: rgb(0x005FB8),
            accent_light: rgb(0x0078D4),
            selection: rgb(0xADD6FF),
            cursor: rgb(0x000000),
            hover_bg: rgb(0xE8E8E8),
            selected_bg: rgb(0xCCE5FF),
            match_bg: rgb(0xF8E3A1),
            danger: rgb(0xC72E0F),
            terminal_fg: rgb(0x333333),
        },
        ansi: ansi_from([
            0x000000, 0xCD3131, 0x00BC00, 0x949800, 0x0451A5, 0xBC05BC, 0x0598BC, 0x555555,
            0x666666, 0xCD3131, 0x14CE14, 0xB5BA00, 0x0451A5, 0xBC05BC, 0x0598BC, 0xA5A5A5,
        ]),
        tokens: vec![
            TokenRule::new("comment", 0x008000),
            TokenRule::new("string", 0xA31515),
            TokenRule::new("string.regexp", 0x811F3F),
            TokenRule::new("constant.numeric", 0x098658),
            TokenRule::new(
                "constant.language, constant.character, support.constant",
                0x0000FF,
            ),
            TokenRule::new("keyword, storage.modifier, storage.type", 0x0000FF),
            TokenRule::new("keyword.control", 0xAF00DB),
            TokenRule::new("keyword.operator", 0x000000),
            TokenRule::new(
                "entity.name.function, support.function, variable.function, support.macro",
                0x795E26,
            ),
            TokenRule::new(
                "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, \
                 entity.name.union, entity.name.trait, support.type, support.class",
                0x267F99,
            ),
            TokenRule::new("variable, variable.parameter", 0x001080),
            TokenRule::new("entity.name.tag", 0x800000),
            TokenRule::new("entity.other.attribute-name", 0xE50000),
            TokenRule::new("markup.heading", 0x000080).bold(),
            TokenRule::new("punctuation", 0x000000),
        ],
    }
}

pub fn monokai() -> Theme {
    Theme {
        name: "Monokai".into(),
        dark: true,
        ui: Ui {
            editor_bg: rgb(0x272822),
            sidebar_bg: rgb(0x1E1F1C),
            tab_active_bg: rgb(0x272822),
            tab_inactive_bg: rgb(0x34352F),
            status_bg: rgb(0x1E1F1C),
            border: rgb(0x3B3C35),
            fg: rgb(0xF8F8F2),
            fg_dim: rgb(0xBCBCB4),
            fg_faint: rgb(0x75715E),
            fg_bright: rgb(0xFFFFFF),
            line_number: rgb(0x90908A),
            accent: rgb(0xA6E22E),
            accent_light: rgb(0xE6DB74),
            selection: rgb(0x49483E),
            cursor: rgb(0xF8F8F0),
            hover_bg: rgb(0x3E3D32),
            selected_bg: rgb(0x49483E),
            match_bg: rgb(0x6A5F1B),
            danger: rgb(0xF92672),
            terminal_fg: rgb(0xF8F8F2),
        },
        ansi: ansi_from([
            0x272822, 0xF92672, 0xA6E22E, 0xE6DB74, 0x66D9EF, 0xAE81FF, 0xA1EFE4, 0xF8F8F2,
            0x75715E, 0xFD5FF0, 0xC2E37A, 0xF3EFA0, 0x9CE7F7, 0xCBA6FF, 0xC7F5EF, 0xFFFFFF,
        ]),
        tokens: vec![
            TokenRule::new("comment", 0x75715E),
            TokenRule::new("string", 0xE6DB74),
            TokenRule::new("constant.numeric, constant.language", 0xAE81FF),
            TokenRule::new("keyword, keyword.control, storage.modifier", 0xF92672),
            TokenRule::new("storage.type", 0x66D9EF).italic(),
            TokenRule::new("keyword.operator", 0xF92672),
            TokenRule::new(
                "entity.name.function, support.function, variable.function, support.macro",
                0xA6E22E,
            ),
            TokenRule::new(
                "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, \
                 entity.name.trait, support.type, support.class",
                0x66D9EF,
            ),
            TokenRule::new("variable.parameter", 0xFD971F).italic(),
            TokenRule::new("variable", 0xF8F8F2),
            TokenRule::new("entity.name.tag", 0xF92672),
            TokenRule::new("entity.other.attribute-name", 0xA6E22E),
            TokenRule::new("punctuation", 0xF8F8F2),
        ],
    }
}

pub fn builtin() -> Vec<Theme> {
    vec![dark_plus(), light_plus(), monokai()]
}

/// Where user themes live: `$XDG_CONFIG_HOME/yara/themes` or
/// `~/.config/yara-code/themes`.
pub fn user_theme_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("yara-code").join("themes"))
}

/// Built-in themes plus every `*.json` in the user theme directory. User themes
/// with a name matching a built-in replace it.
pub fn load_all() -> Vec<Theme> {
    let mut themes = builtin();
    let Some(dir) = user_theme_dir() else {
        return themes;
    };
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return themes;
    };
    let mut paths: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    for path in paths {
        if let Ok(theme) = from_vscode_file(&path) {
            match themes.iter().position(|t| t.name == theme.name) {
                Some(i) => themes[i] = theme,
                None => themes.push(theme),
            }
        }
    }
    themes
}

#[derive(Debug)]
pub enum ThemeError {
    Io(std::io::Error),
    Parse(String),
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse(e) => write!(f, "{e}"),
        }
    }
}

pub fn from_vscode_file(path: &Path) -> Result<Theme, ThemeError> {
    let text = std::fs::read_to_string(path).map_err(ThemeError::Io)?;
    let fallback_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Custom".to_string());
    from_vscode_json(&text, &fallback_name)
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let s = s.trim().strip_prefix('#')?;
    let hex = match s.len() {
        3 => {
            let mut out = String::with_capacity(6);
            for c in s.chars() {
                out.push(c);
                out.push(c);
            }
            out
        }
        // Trailing alpha is dropped: these colors are painted opaque.
        6 | 8 => s[..6].to_string(),
        _ => return None,
    };
    u32::from_str_radix(&hex, 16).ok().map(rgb)
}

/// Reads a VS Code color-theme JSON: the `colors` map drives chrome and the
/// terminal palette, `tokenColors` drives syntax. Anything the file omits keeps
/// its value from the closest built-in (Dark+ or Light+, per `type`).
pub fn from_vscode_json(text: &str, fallback_name: &str) -> Result<Theme, ThemeError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| ThemeError::Parse(e.to_string()))?;

    let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("dark");
    let mut theme = if kind == "light" {
        light_plus()
    } else {
        dark_plus()
    };
    theme.dark = kind != "light";
    theme.name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(fallback_name)
        .to_string();

    let colors = value.get("colors").and_then(|v| v.as_object());
    let get = |key: &str| -> Option<Rgb> {
        colors
            .and_then(|c| c.get(key))
            .and_then(|v| v.as_str())
            .and_then(parse_hex)
    };
    let ui = &mut theme.ui;
    let set_color = |slot: &mut Rgb, key: &str| {
        if let Some(c) = get(key) {
            *slot = c;
        }
    };
    set_color(&mut ui.editor_bg, "editor.background");
    set_color(&mut ui.fg, "editor.foreground");
    set_color(&mut ui.sidebar_bg, "sideBar.background");
    set_color(&mut ui.tab_active_bg, "tab.activeBackground");
    set_color(&mut ui.tab_inactive_bg, "tab.inactiveBackground");
    set_color(&mut ui.status_bg, "statusBar.background");
    set_color(&mut ui.border, "panel.border");
    set_color(&mut ui.fg_dim, "sideBar.foreground");
    set_color(&mut ui.fg_faint, "descriptionForeground");
    set_color(&mut ui.line_number, "editorLineNumber.foreground");
    set_color(&mut ui.accent, "focusBorder");
    set_color(&mut ui.accent_light, "textLink.foreground");
    set_color(&mut ui.selection, "editor.selectionBackground");
    set_color(&mut ui.cursor, "editorCursor.foreground");
    set_color(&mut ui.hover_bg, "list.hoverBackground");
    set_color(&mut ui.selected_bg, "list.activeSelectionBackground");
    set_color(&mut ui.match_bg, "editor.findMatchHighlightBackground");
    set_color(&mut ui.danger, "errorForeground");
    set_color(&mut ui.terminal_fg, "terminal.foreground");

    const ANSI_KEYS: [&str; 16] = [
        "terminal.ansiBlack",
        "terminal.ansiRed",
        "terminal.ansiGreen",
        "terminal.ansiYellow",
        "terminal.ansiBlue",
        "terminal.ansiMagenta",
        "terminal.ansiCyan",
        "terminal.ansiWhite",
        "terminal.ansiBrightBlack",
        "terminal.ansiBrightRed",
        "terminal.ansiBrightGreen",
        "terminal.ansiBrightYellow",
        "terminal.ansiBrightBlue",
        "terminal.ansiBrightMagenta",
        "terminal.ansiBrightCyan",
        "terminal.ansiBrightWhite",
    ];
    for (i, key) in ANSI_KEYS.into_iter().enumerate() {
        if let Some(c) = get(key) {
            theme.ansi[i] = c;
        }
    }

    if let Some(token_colors) = value.get("tokenColors").and_then(|v| v.as_array()) {
        let mut rules = Vec::new();
        for entry in token_colors {
            let settings = entry.get("settings");
            let Some(color) = settings
                .and_then(|s| s.get("foreground"))
                .and_then(|v| v.as_str())
                .and_then(parse_hex)
            else {
                continue;
            };
            let style = settings
                .and_then(|s| s.get("fontStyle"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let scope = match entry.get("scope") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Array(items)) => items
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => continue,
            };
            if scope.trim().is_empty() {
                continue;
            }
            rules.push(TokenRule {
                scope,
                color,
                italic: style.contains("italic"),
                bold: style.contains("bold"),
            });
        }
        if !rules.is_empty() {
            theme.tokens = rules;
        }
    }

    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_forms_parse() {
        assert_eq!(parse_hex("#FF0000"), Some((255, 0, 0)));
        assert_eq!(parse_hex("#f00"), Some((255, 0, 0)));
        assert_eq!(parse_hex("#FF000080"), Some((255, 0, 0)));
        assert_eq!(parse_hex("nope"), None);
    }

    #[test]
    fn vscode_json_overrides_only_what_it_names() {
        let json = r##"{
            "name": "Test",
            "type": "dark",
            "colors": { "editor.background": "#101010", "terminal.ansiRed": "#ff5555" },
            "tokenColors": [
                { "scope": ["comment", "punctuation.definition.comment"],
                  "settings": { "foreground": "#00FF00", "fontStyle": "italic" } }
            ]
        }"##;
        let theme = from_vscode_json(json, "fallback").unwrap();
        assert_eq!(theme.name, "Test");
        assert_eq!(theme.ui.editor_bg, (0x10, 0x10, 0x10));
        // Untouched keys keep the Dark+ value.
        assert_eq!(theme.ui.fg, dark_plus().ui.fg);
        assert_eq!(theme.ansi[1], (0xFF, 0x55, 0x55));
        assert_eq!(theme.tokens.len(), 1);
        assert_eq!(
            theme.tokens[0].scope,
            "comment, punctuation.definition.comment"
        );
        assert!(theme.tokens[0].italic);
    }

    #[test]
    fn ansi256_extends_past_the_named_sixteen() {
        let theme = dark_plus();
        assert_eq!(ansi256(&theme, 1), theme.ansi[1]);
        assert_eq!(ansi256(&theme, 232), (8, 8, 8));
    }
}

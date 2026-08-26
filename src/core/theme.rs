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
/// The colour of an indent guide: the editor background nudged a little
/// toward the faint text, so the lines are there when looked for and not
/// otherwise.
pub fn indent_guide(theme: &Theme) -> Rgb {
    let (bg, fg) = (theme.ui.editor_bg, theme.ui.fg_faint);
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * 0.35).round() as u8;
    (mix(bg.0, fg.0), mix(bg.1, fg.1), mix(bg.2, fg.2))
}

/// The colours a chart's slices and bars are painted in, in order. They come
/// out of the palette the terminal already draws with, so a chart belongs to
/// the theme rather than to a palette of its own.
pub fn chart_color(theme: &Theme, index: usize) -> Rgb {
    const WHEEL: [usize; 6] = [12, 10, 11, 13, 14, 9];
    theme.ansi[WHEEL[index % WHEEL.len()]]
}

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
    let mut theme = dark_plus_stock();
    dim_comments(&mut theme, 0x5A6472);
    color_imports(&mut theme, 0x4EC9B0);
    theme
}

/// Dark+ exactly as VS Code ships it, before the two touches above.
fn dark_plus_stock() -> Theme {
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
            // Chrome, not syntax: a splitter under the pointer, a lit heading,
            // a link. Muted on purpose — at full saturation these read as a
            // warning every time a pane is resized.
            accent: rgb(0x3B6E8F),
            accent_light: rgb(0x7FAFCC),
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

/// VS Code's current default, "Dark Modern": a touch darker and flatter than
/// Dark+, with the same syntax colours. Two deliberate departures from the
/// stock theme, both to keep the eye on the code: module names in imports are
/// coloured rather than left plain white, and comments are dimmed so they sit
/// back instead of glowing.
pub fn dark_modern() -> Theme {
    let mut theme = dark_plus_stock();
    theme.name = "Dark Modern".into();
    theme.ui.editor_bg = rgb(0x1F1F1F);
    theme.ui.sidebar_bg = rgb(0x181818);
    theme.ui.tab_active_bg = rgb(0x1F1F1F);
    theme.ui.tab_inactive_bg = rgb(0x181818);
    theme.ui.status_bg = rgb(0x181818);
    theme.ui.border = rgb(0x2B2B2B);
    theme.ui.line_number = rgb(0x6E7681);
    theme.ui.accent = rgb(0x3F7396);
    theme.ui.hover_bg = rgb(0x2A2D2E);
    theme.ui.selected_bg = rgb(0x04395E);
    dim_comments(&mut theme, 0x5C6370);
    color_imports(&mut theme, 0x4EC9B0);
    theme
}

pub fn light_plus() -> Theme {
    let mut theme = light_plus_stock();
    dim_comments(&mut theme, 0x6E8B6E);
    color_imports(&mut theme, 0x267F99);
    theme
}

/// Light+ exactly as VS Code ships it, before the two touches above.
fn light_plus_stock() -> Theme {
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
            accent: rgb(0x3A6D9A),
            accent_light: rgb(0x33698F),
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
    let mut theme = Theme {
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
            accent: rgb(0x8A9A5B),
            accent_light: rgb(0xC2B98A),
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
            TokenRule::new(
                "entity.name.namespace, entity.name.module, support.other.namespace, \
                 meta.path, variable.other.module, entity.name.import",
                0x66D9EF,
            ),
        ],
    };
    // The classic Monokai comment, sat back a little further so it stops
    // competing with the code.
    dim_comments(&mut theme, 0x5B584C);
    theme
}

/// Repaints a theme's comment rule, so a theme can sit its comments back
/// without restating the whole token list.
fn dim_comments(theme: &mut Theme, color: u32) {
    for rule in &mut theme.tokens {
        if rule.scope.split(',').any(|s| s.trim() == "comment") {
            rule.color = rgb(color);
        }
    }
}

/// Colours the module and namespace names an import names, which most grammars
/// leave as plain identifiers — white against a dark background.
fn color_imports(theme: &mut Theme, color: u32) {
    theme.tokens.push(TokenRule::new(
        "entity.name.namespace, entity.name.module, support.other.namespace,          meta.path, variable.other.module, entity.name.import",
        color,
    ));
}

pub fn builtin() -> Vec<Theme> {
    vec![dark_modern(), dark_plus(), light_plus(), monokai()]
}

/// Where user themes live: `~/.config/ycode/themes`, beside the settings.
pub fn user_theme_dir() -> Option<PathBuf> {
    Some(crate::core::config_dir()?.join("themes"))
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
    let digits: Vec<char> = s.trim().trim_start_matches('#').chars().collect();
    // Only ASCII hex digits are read, by char and never by byte, so a stray
    // non-ASCII character in a theme file is a bad colour, not a panic.
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

/// Reads a VS Code color-theme JSON: the `colors` map drives chrome and the
/// terminal palette, `tokenColors` drives syntax. Anything the file omits keeps
/// its value from the closest built-in (Dark+ or Light+, per `type`).
pub fn from_vscode_json(text: &str, fallback_name: &str) -> Result<Theme, ThemeError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| ThemeError::Parse(e.to_string()))?;

    let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("dark");
    let mut theme = if kind == "light" {
        light_plus_stock()
    } else {
        dark_plus_stock()
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
    fn dark_modern_is_the_default_and_dims_comments_and_colours_imports() {
        let all = builtin();
        assert_eq!(all[0].name, "Dark Modern", "Dark Modern leads the list");
        let dm = dark_modern();
        // Dark Modern's own background, not Dark+'s.
        assert_eq!(dm.ui.editor_bg, rgb(0x1F1F1F));
        // Comments are the dim slate, not Dark+'s bright green.
        let comment = dm.tokens.iter().find(|r| r.scope == "comment").unwrap();
        assert_eq!(comment.color, rgb(0x5C6370));
        // Imports have a rule of their own now.
        assert!(dm
            .tokens
            .iter()
            .any(|r| r.scope.contains("entity.name.namespace")));
    }

    #[test]
    fn monokai_keeps_sublime_colours_but_sits_comments_back() {
        let m = monokai();
        assert_eq!(m.ui.editor_bg, rgb(0x272822), "still Sublime's ground");
        let comment = m.tokens.iter().find(|r| r.scope == "comment").unwrap();
        assert_eq!(
            comment.color,
            rgb(0x5B584C),
            "dimmer than the stock #75715E"
        );
        assert!(m
            .tokens
            .iter()
            .any(|r| r.scope.contains("entity.name.namespace")));
    }

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
    fn an_indent_guide_sits_between_the_background_and_faint_text() {
        for theme in builtin() {
            let guide = indent_guide(&theme);
            let (bg, fg) = (theme.ui.editor_bg, theme.ui.fg_faint);
            let between = |g: u8, a: u8, b: u8| g >= a.min(b) && g <= a.max(b);
            assert!(between(guide.0, bg.0, fg.0));
            assert!(between(guide.1, bg.1, fg.1));
            assert!(between(guide.2, bg.2, fg.2));
            assert_ne!(guide, bg, "a guide the colour of the page is no guide");
        }
    }

    #[test]
    fn ansi256_extends_past_the_named_sixteen() {
        let theme = dark_plus();
        assert_eq!(ansi256(&theme, 1), theme.ansi[1]);
        assert_eq!(ansi256(&theme, 232), (8, 8, 8));
    }
}

#[cfg(test)]
mod theme_tests {
    use super::*;
    use crate::core::test_support::Dir;

    #[test]
    fn the_built_in_themes_are_the_ones_we_ship() {
        let names: Vec<String> = builtin().into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["Dark Modern", "Dark+", "Light+", "Monokai"]);
        assert!(dark_modern().dark);
        assert!(dark_plus().dark);
        assert!(!light_plus().dark);
        assert!(monokai().dark);
    }

    #[test]
    fn the_ansi_palette_answers_for_every_index() {
        let theme = dark_plus();
        // The first sixteen come from the theme itself.
        assert_eq!(ansi256(&theme, 1), theme.ansi[1]);
        // 16..232 is the 6×6×6 cube: 16 is black, 231 white.
        assert_eq!(ansi256(&theme, 16), (0, 0, 0));
        assert_eq!(ansi256(&theme, 231), (255, 255, 255));
        // 232..255 is the grey ramp, dark to light.
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
                "terminal.ansiRed": "#ff0000"
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
        assert_eq!(theme.ui.editor_bg, (250, 250, 250));
        assert_eq!(theme.ui.fg, (16, 16, 16));
        assert_eq!(theme.ansi[1], (255, 0, 0));

        let comment = theme
            .tokens
            .iter()
            .find(|t| t.scope == "comment")
            .expect("the comment rule survived");
        assert_eq!(comment.color, (0, 128, 0));
        assert!(comment.italic);
        // Several scopes stay one rule: a comma-separated selector is what
        // the highlighter matches against.
        let keywords = theme
            .tokens
            .iter()
            .find(|t| t.scope == "keyword, storage")
            .expect("both scopes in one selector");
        assert!(keywords.bold && !keywords.italic);
        // A rule with no scope at all is VS Code's editor default, not a rule.
        assert!(theme.tokens.iter().all(|t| !t.scope.trim().is_empty()));
    }

    #[test]
    fn a_theme_without_a_name_takes_the_file_name() {
        let theme = from_vscode_json(r##"{"colors": {}}"##, "Fallback").unwrap();
        assert_eq!(theme.name, "Fallback");
        // With nothing to go on it stays dark, like VS Code's own default.
        assert!(theme.dark);
    }

    #[test]
    fn malformed_json_is_reported_rather_than_guessed_at() {
        assert!(matches!(
            from_vscode_json("{not json", "x"),
            Err(ThemeError::Parse(_))
        ));
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
        // No name in the file: the file name stands in.
        assert_eq!(theme.name, "solar");
        assert_eq!(theme.ui.editor_bg, (0, 43, 54));
    }

    #[test]
    fn colours_are_read_in_every_shape_vs_code_writes_them() {
        assert_eq!(parse_hex("#ffffff"), Some((255, 255, 255)));
        assert_eq!(parse_hex("#abc"), Some((170, 187, 204)), "short form");
        // Alpha is accepted and dropped: the editor paints opaque.
        assert_eq!(parse_hex("#11223344"), Some((17, 34, 51)));
        assert_eq!(parse_hex("nonsense"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn a_token_rule_carries_its_style() {
        let rule = TokenRule::new("string", 0x00ff00).italic().bold();
        assert_eq!(rule.color, (0, 255, 0));
        assert!(rule.italic && rule.bold);
    }

    #[test]
    fn loading_everything_always_yields_the_built_ins() {
        let themes = load_all();
        assert!(themes.iter().any(|t| t.name == "Dark+"));
        assert!(themes.len() >= builtin().len());
    }
}

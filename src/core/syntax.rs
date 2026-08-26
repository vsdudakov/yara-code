//! Shared syntect setup: grammars plus a syntect theme compiled from whatever
//! [`crate::core::theme::Theme`] is active. Frontends turn the emitted regions
//! into their own text primitives.

use std::str::FromStr;

use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, Style, StyleModifier, Theme as SyntectTheme, ThemeItem,
    ThemeSettings,
};
use syntect::parsing::{SyntaxDefinition, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::core::theme::{Rgb, Theme};

/// Grammars for languages the default set omits, compiled into the binary.
const BUNDLED: &[(&str, &str)] = &[
    (
        "TypeScript",
        include_str!("../../assets/syntaxes/TypeScript.sublime-syntax"),
    ),
    (
        "TOML",
        include_str!("../../assets/syntaxes/TOML.sublime-syntax"),
    ),
    (
        "Kotlin",
        include_str!("../../assets/syntaxes/Kotlin.sublime-syntax"),
    ),
    (
        "Swift",
        include_str!("../../assets/syntaxes/Swift.sublime-syntax"),
    ),
    (
        "Dart",
        include_str!("../../assets/syntaxes/Dart.sublime-syntax"),
    ),
    (
        "Dockerfile",
        include_str!("../../assets/syntaxes/Dockerfile.sublime-syntax"),
    ),
    (
        "Protobuf",
        include_str!("../../assets/syntaxes/Protobuf.sublime-syntax"),
    ),
    (
        "GraphQL",
        include_str!("../../assets/syntaxes/GraphQL.sublime-syntax"),
    ),
];

/// Extensions with no grammar of their own, pointed at the closest relative.
/// Approximate coloring beats none, and a real grammar dropped into the user
/// syntax folder takes precedence over any of these.
const ALIASES: &[(&str, &str)] = &[
    ("mjs", "js"),
    ("cjs", "js"),
    ("es6", "js"),
    ("vue", "html"),
    ("svelte", "html"),
    ("astro", "html"),
    ("zig", "c"),
    ("odin", "c"),
    ("hx", "java"),
    ("sol", "js"),
    ("hcl", "js"),
    ("tf", "js"),
    ("tfvars", "js"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("ipynb", "json"),
    ("nim", "py"),
    ("cr", "rb"),
    ("ex", "rb"),
    ("exs", "rb"),
    ("gleam", "rs"),
    ("ps1", "sh"),
    ("env", "sh"),
    ("cmake", "make"),
    ("ini", "toml"),
    ("cfg", "toml"),
    ("conf", "toml"),
    ("editorconfig", "toml"),
    ("gitconfig", "toml"),
];

/// Where user grammars live: `~/.config/ycode/syntaxes`, beside the settings.
/// Any `.sublime-syntax` there is loaded at startup.
pub fn user_syntax_dir() -> Option<std::path::PathBuf> {
    Some(crate::core::config_dir()?.join("syntaxes"))
}

/// The added grammars live in their own set: folding them into the default one
/// would mean relinking all 75 bundled syntaxes, which costs over a second of
/// startup. Building this small set instead takes a few milliseconds.
fn load_extra() -> SyntaxSet {
    let mut builder = SyntaxSet::new().into_builder();
    for (name, source) in BUNDLED {
        match SyntaxDefinition::load_from_str(source, true, Some(name)) {
            Ok(definition) => builder.add(definition),
            // A broken grammar must not take the editor down with it.
            Err(_) => continue,
        }
    }
    if let Some(dir) = user_syntax_dir() {
        let _ = builder.add_from_folder(dir, true);
    }
    builder.build()
}

pub struct Syntax {
    /// Syntect's bundled set, used as linked — never rebuilt.
    defaults: SyntaxSet,
    /// Grammars added by Yara and by the user.
    extra: SyntaxSet,
    theme: SyntectTheme,
}

fn color(c: Rgb) -> Color {
    Color {
        r: c.0,
        g: c.1,
        b: c.2,
        a: 0xFF,
    }
}

/// Compiles the theme's token rules into a syntect theme. Rules whose scope
/// selector syntect can't parse are skipped rather than failing the theme.
fn compile(theme: &Theme) -> SyntectTheme {
    let scopes = theme
        .tokens
        .iter()
        .filter_map(|rule| {
            let mut font_style = FontStyle::empty();
            if rule.italic {
                font_style |= FontStyle::ITALIC;
            }
            if rule.bold {
                font_style |= FontStyle::BOLD;
            }
            Some(ThemeItem {
                scope: ScopeSelectors::from_str(&rule.scope).ok()?,
                style: StyleModifier {
                    foreground: Some(color(rule.color)),
                    background: None,
                    font_style: (!font_style.is_empty()).then_some(font_style),
                },
            })
        })
        .collect();
    SyntectTheme {
        name: Some(theme.name.clone()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(color(theme.ui.fg)),
            background: Some(color(theme.ui.editor_bg)),
            ..Default::default()
        },
        scopes,
    }
}

impl Syntax {
    pub fn new(theme: &Theme) -> Self {
        Self {
            defaults: SyntaxSet::load_defaults_newlines(),
            extra: load_extra(),
            theme: compile(theme),
        }
    }

    /// The grammar for a file extension, with its owning set. Added grammars
    /// win over the bundled ones, so a user file can replace any of them; the
    /// alias table is the last resort before plain text.
    fn syntax_for(&self, extension: &str) -> (&SyntaxSet, &SyntaxReference) {
        let lower = extension.to_ascii_lowercase();
        for candidate in [extension, lower.as_str()] {
            if let Some(syntax) = self.extra.find_syntax_by_extension(candidate) {
                return (&self.extra, syntax);
            }
            if let Some(syntax) = self.defaults.find_syntax_by_extension(candidate) {
                return (&self.defaults, syntax);
            }
        }
        if let Some((_, target)) = ALIASES.iter().find(|(from, _)| *from == lower) {
            if let Some(syntax) = self.extra.find_syntax_by_extension(target) {
                return (&self.extra, syntax);
            }
            if let Some(syntax) = self.defaults.find_syntax_by_extension(target) {
                return (&self.defaults, syntax);
            }
        }
        (&self.defaults, self.defaults.find_syntax_plain_text())
    }

    /// The human name of the grammar in use, for the status bar.
    pub fn language_name(&self, extension: &str) -> String {
        self.syntax_for(extension).1.name.clone()
    }

    /// Swaps the color scheme without reloading the (expensive) grammar set.
    pub fn set_theme(&mut self, theme: &Theme) {
        self.theme = compile(theme);
    }
}

impl Default for Syntax {
    fn default() -> Self {
        Self::new(&Theme::default())
    }
}

/// One styled run of text: (r, g, b), italic flag, and the text itself.
pub struct Region<'a> {
    pub color: (u8, u8, u8),
    pub italic: bool,
    pub text: &'a str,
}

impl Syntax {
    /// Highlights `code` line by line, handing each line's styled regions to
    /// `emit`. Lines are emitted in order, including their trailing newline.
    pub fn highlight_lines<'a, F>(&self, extension: &str, code: &'a str, mut emit: F)
    where
        F: FnMut(Vec<Region<'a>>),
    {
        let (set, syntax) = self.syntax_for(extension);
        let mut hl = HighlightLines::new(syntax, &self.theme);
        for line in LinesWithEndings::from(code) {
            let styled: Vec<(Style, &'a str)> = hl.highlight_line(line, set).unwrap_or_default();
            emit(
                styled
                    .into_iter()
                    .map(|(style, text)| Region {
                        color: (style.foreground.r, style.foreground.g, style.foreground.b),
                        italic: style.font_style.contains(FontStyle::ITALIC),
                        text,
                    })
                    .collect(),
            );
        }
    }
}

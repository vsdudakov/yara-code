//! Syntax colours: syntect's grammars plus a syntect theme compiled from
//! whatever [`Theme`] is active. The frontend turns the regions into its
//! own text.

use std::str::FromStr;

use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, Style, StyleModifier, Theme as SyntectTheme, ThemeItem,
    ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::theme::{Rgb, Theme};

/// Extensions with no grammar of their own, pointed at the closest relative.
const ALIASES: &[(&str, &str)] = &[
    ("mjs", "js"),
    ("cjs", "js"),
    ("ts", "js"),
    ("tsx", "js"),
    ("vue", "html"),
    ("svelte", "html"),
    ("zig", "c"),
    ("kt", "java"),
    ("swift", "c"),
    ("toml", "yaml"),
    ("ini", "yaml"),
    ("cfg", "yaml"),
    ("conf", "yaml"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("env", "sh"),
    ("dockerfile", "sh"),
];

/// Where user grammars live: `~/.config/ycode/syntaxes`, beside the settings.
pub fn user_syntax_dir() -> Option<std::path::PathBuf> {
    Some(crate::config_dir()?.join("syntaxes"))
}

pub struct Syntax {
    /// Syntect's bundled set, used as linked — never rebuilt.
    defaults: SyntaxSet,
    /// Any `.sublime-syntax` the user dropped in; a small set of its own,
    /// since relinking the bundled one costs a second at startup.
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
/// selector syntect cannot parse are skipped rather than failing the theme.
fn compile(theme: &Theme) -> SyntectTheme {
    let scopes = theme
        .tokens
        .iter()
        .filter_map(|rule| {
            let mut font_style = FontStyle::empty();
            font_style.set(FontStyle::ITALIC, rule.italic);
            font_style.set(FontStyle::BOLD, rule.bold);
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
            background: Some(color(theme.ui.bg)),
            ..Default::default()
        },
        scopes,
    }
}

/// One styled run of text.
pub struct Region<'a> {
    pub color: Rgb,
    pub italic: bool,
    pub bold: bool,
    pub text: &'a str,
}

impl Syntax {
    pub fn new(theme: &Theme) -> Self {
        let mut builder = SyntaxSet::new().into_builder();
        if let Some(dir) = user_syntax_dir() {
            let _ = builder.add_from_folder(dir, true);
        }
        Self {
            defaults: SyntaxSet::load_defaults_newlines(),
            extra: builder.build(),
            theme: compile(theme),
        }
    }

    /// Swaps the colours without reloading the grammars.
    pub fn set_theme(&mut self, theme: &Theme) {
        self.theme = compile(theme);
    }

    /// The grammar for an extension: the user's first, then the bundled
    /// ones, then the alias table, then plain text.
    fn syntax_for(&self, extension: &str) -> (&SyntaxSet, &SyntaxReference) {
        let lower = extension.to_ascii_lowercase();
        let alias = ALIASES
            .iter()
            .find(|(from, _)| *from == lower)
            .map(|(_, to)| *to);
        for candidate in [lower.as_str()].into_iter().chain(alias) {
            for set in [&self.extra, &self.defaults] {
                if let Some(syntax) = set.find_syntax_by_extension(candidate) {
                    return (set, syntax);
                }
            }
        }
        (&self.defaults, self.defaults.find_syntax_plain_text())
    }

    /// The grammar's own name, for the status bar.
    pub fn language(&self, extension: &str) -> String {
        self.syntax_for(extension).1.name.clone()
    }

    /// Highlights `code` line by line, handing each line's regions to
    /// `emit`, trailing newline included.
    pub fn highlight<'a>(
        &self,
        extension: &str,
        code: &'a str,
        mut emit: impl FnMut(Vec<Region<'a>>),
    ) {
        let (set, syntax) = self.syntax_for(extension);
        let mut lines = HighlightLines::new(syntax, &self.theme);
        for line in LinesWithEndings::from(code) {
            let styled: Vec<(Style, &'a str)> = lines.highlight_line(line, set).unwrap_or_default();
            emit(
                styled
                    .into_iter()
                    .map(|(style, text)| Region {
                        color: (style.foreground.r, style.foreground.g, style.foreground.b),
                        italic: style.font_style.contains(FontStyle::ITALIC),
                        bold: style.font_style.contains(FontStyle::BOLD),
                        text,
                    })
                    .collect(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::dark_modern;

    fn regions(syntax: &Syntax, ext: &str, code: &str) -> Vec<(String, Rgb)> {
        let mut out = Vec::new();
        syntax.highlight(ext, code, |line| {
            out.extend(line.into_iter().map(|r| (r.text.to_string(), r.color)));
        });
        out
    }

    #[test]
    fn a_keyword_takes_the_themes_keyword_colour_and_plain_text_the_foreground() {
        let theme = dark_modern();
        let syntax = Syntax::new(&theme);
        let keyword = theme
            .tokens
            .iter()
            .find(|t| t.scope.starts_with("keyword"))
            .unwrap()
            .color;
        let rust = regions(&syntax, "rs", "fn main() {}\n");
        assert_eq!(rust[0].0, "fn");
        assert_eq!(rust[0].1, keyword);
        let plain = regions(&syntax, "nope", "fn main\n");
        assert!(plain.iter().all(|(_, c)| *c == theme.ui.fg));
        assert_eq!(syntax.language("rs"), "Rust");
        assert_eq!(syntax.language("ts"), "JavaScript", "an alias");
        assert_eq!(syntax.language("zzz"), "Plain Text");
    }

    #[test]
    fn the_theme_can_be_swapped_without_reloading() {
        let mut syntax = Syntax::new(&dark_modern());
        let mut other = dark_modern();
        for rule in &mut other.tokens {
            rule.color = (1, 2, 3);
        }
        syntax.set_theme(&other);
        let rust = regions(&syntax, "rs", "fn\n");
        assert_eq!(rust[0].1, (1, 2, 3));
    }
}

//! Smart indentation: what to insert when Enter is pressed.
//!
//! Language-aware but deliberately lightweight — it reads the line around the
//! cursor rather than parsing, which is what editors do before a language
//! server is attached.

use crate::core::settings::Indent;

/// Languages where a trailing `:` opens a block.
const COLON_LANGS: &[&str] = &["py", "pyi", "pyx", "pyw", "yaml", "yml", "toml"];
/// Languages using `#` for line comments.
const HASH_COMMENT_LANGS: &[&str] = &[
    "py", "pyi", "pyx", "pyw", "yaml", "yml", "toml", "rb", "sh", "bash", "zsh", "fish", "pl", "r",
    "nim", "ex", "exs", "cr",
];
/// Python statements after which the next line dedents.
const PY_DEDENT_AFTER: &[&str] = &["return", "pass", "break", "continue", "raise"];

pub struct NewlineEdit {
    /// Text to insert in place of the plain `\n`.
    pub insert: String,
    /// Where the cursor lands, in chars from the start of `insert`.
    pub cursor_offset: usize,
}

/// The indent unit to insert: what the file already uses when detection is on
/// and the file gives a clear signal, otherwise the configured setting.
fn unit(text: &str, config: &Indent) -> String {
    if !config.detect_from_file {
        return config.unit();
    }
    let mut min_spaces = usize::MAX;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            return "\t".to_string();
        }
        let spaces = line.len() - line.trim_start_matches(' ').len();
        if spaces > 0 && spaces < min_spaces {
            min_spaces = spaces;
        }
    }
    match min_spaces {
        usize::MAX => config.unit(),
        found => " ".repeat(found.clamp(1, 16)),
    }
}

/// Drops a trailing line comment so `def f():  # note` still counts as opening
/// a block. Quote-aware only to the extent of counting quotes before the
/// comment marker, which is enough for ordinary code.
fn strip_trailing_comment(line: &str, extension: &str) -> String {
    let marker = if HASH_COMMENT_LANGS.contains(&extension) {
        "#"
    } else {
        "//"
    };
    let mut search_from = 0;
    while let Some(rel) = line[search_from..].find(marker) {
        let idx = search_from + rel;
        let before = &line[..idx];
        let quotes = before.matches('"').count() + before.matches('\'').count();
        if quotes.is_multiple_of(2) {
            return before.to_string();
        }
        search_from = idx + marker.len();
    }
    line.to_string()
}

fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

fn strip_one_unit<'a>(indent: &'a str, unit: &str) -> &'a str {
    indent.strip_suffix(unit).unwrap_or(indent)
}

/// Computes the replacement for a newline typed at char index `cursor`.
pub fn newline_edit(text: &str, cursor: usize, extension: &str, config: &Indent) -> NewlineEdit {
    let unit = unit(text, config);
    let byte = char_to_byte(text, cursor);
    let line_start = text[..byte].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[byte..].find('\n').map_or(text.len(), |i| byte + i);

    let full_line = &text[line_start..line_end];
    let leading: String = full_line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();

    let before_cursor = &text[line_start..byte];
    let code_before = strip_trailing_comment(before_cursor, extension);
    let code_before = code_before.trim_end();
    let after_cursor = text[byte..line_end].trim_start();

    let colon_lang = COLON_LANGS.contains(&extension);
    let opener = code_before.chars().last();
    let opens_brace = matches!(opener, Some('{') | Some('(') | Some('['));
    let opens_block = opens_brace || (colon_lang && opener == Some(':'));

    let mut indent = leading.clone();
    if opens_block {
        indent.push_str(&unit);
    } else if colon_lang {
        let first_word = code_before
            .trim_start()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or("");
        if PY_DEDENT_AFTER.contains(&first_word) {
            indent = strip_one_unit(&leading, &unit).to_string();
        }
    }

    // Typing Enter between a bracket pair puts the closer on its own line.
    let closer = match opener {
        Some('{') => Some('}'),
        Some('(') => Some(')'),
        Some('[') => Some(']'),
        _ => None,
    };
    if let Some(closer) = closer {
        if after_cursor.starts_with(closer) {
            let insert = format!("\n{indent}\n{leading}");
            return NewlineEdit {
                cursor_offset: 1 + indent.chars().count(),
                insert,
            };
        }
    }

    let insert = format!("\n{indent}");
    NewlineEdit {
        cursor_offset: insert.chars().count(),
        insert,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, ext: &str) -> String {
        apply_with(text, ext, &Indent::default())
    }

    fn apply_with(text: &str, ext: &str, config: &Indent) -> String {
        let cursor = text.chars().count();
        let edit = newline_edit(text, cursor, ext, config);
        format!("{text}{}", edit.insert)
    }

    #[test]
    fn python_colon_indents() {
        assert_eq!(apply("def f():", "py"), "def f():\n    ");
        assert_eq!(apply("    if x:  # go", "py"), "    if x:  # go\n        ");
    }

    #[test]
    fn python_keeps_indent_and_dedents_after_return() {
        assert_eq!(apply("    x = 1", "py"), "    x = 1\n    ");
        let nested = "def f():\n    if x:\n        return x";
        assert_eq!(apply(nested, "py"), format!("{nested}\n    "));
    }

    #[test]
    fn colon_in_string_is_not_a_block() {
        assert_eq!(apply("x = \"a:\"", "py"), "x = \"a:\"\n");
    }

    #[test]
    fn brace_langs_indent_and_pair() {
        assert_eq!(apply("fn main() {", "rs"), "fn main() {\n    ");
        let text = "fn main() {}";
        let edit = newline_edit(text, text.chars().count() - 1, "rs", &Indent::default());
        assert_eq!(edit.insert, "\n    \n");
        assert_eq!(edit.cursor_offset, 5);
    }

    #[test]
    fn indent_unit_follows_the_file() {
        assert_eq!(apply("function f() {\n  a();\n  if (b) {", "js"), {
            let mut s = "function f() {\n  a();\n  if (b) {".to_string();
            s.push_str("\n    ");
            s
        });
    }

    #[test]
    fn configured_width_wins_when_detection_is_off() {
        use crate::core::settings::IndentStyle;
        let config = Indent {
            style: IndentStyle::Spaces,
            width: 2,
            detect_from_file: false,
        };
        // The file is indented with 4, but detection is off.
        assert_eq!(
            apply_with("def f():\n    pass\n\ndef g():", "py", &config),
            "def f():\n    pass\n\ndef g():\n  "
        );
        let tabs = Indent {
            style: IndentStyle::Tabs,
            width: 4,
            detect_from_file: false,
        };
        assert_eq!(apply_with("def f():", "py", &tabs), "def f():\n\t");
    }

    #[test]
    fn configured_width_is_the_fallback_when_the_file_is_silent() {
        use crate::core::settings::IndentStyle;
        let config = Indent {
            style: IndentStyle::Spaces,
            width: 2,
            detect_from_file: true,
        };
        // No indented line anywhere, so detection has nothing to go on.
        assert_eq!(apply_with("def f():", "py", &config), "def f():\n  ");
    }

    #[test]
    fn rust_colon_does_not_indent() {
        assert_eq!(apply("    foo:", "rs"), "    foo:\n    ");
    }
}

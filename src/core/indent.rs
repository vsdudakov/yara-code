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

/// How many indent guides each line carries: one per whole level of leading
/// whitespace before the line's own text, a tab counting as `width` columns.
/// A blank line takes the guides shared by its nearest non-blank neighbours,
/// so a block's guides run unbroken through the empty lines inside it.
/// A tab as many spaces as it stands for on screen, aligned to the next tab
/// stop. A tab is one character in the buffer and several columns in a
/// terminal, and every part of the display has to agree on how many: a frame
/// drawn with fewer cells than the terminal advances leaves the row holding
/// whatever was under it, which is how opening a tab-indented Makefile and then
/// another file left the Makefile's characters behind.
pub fn expand_tabs(text: &str, width: usize, from_column: usize) -> String {
    if !text.contains('\t') {
        return text.to_string();
    }
    let width = width.max(1);
    let mut out = String::with_capacity(text.len());
    let mut column = from_column;
    for ch in text.chars() {
        if ch == '\t' {
            let stop = width - (column % width);
            out.extend(std::iter::repeat_n(' ', stop));
            column += stop;
        } else {
            out.push(ch);
            column += 1;
        }
    }
    out
}

/// The screen column a character index sits at, once the tabs before it have
/// taken the columns they stand for.
pub fn display_column(line: &str, char_index: usize, width: usize) -> usize {
    let width = width.max(1);
    let mut column = 0;
    for ch in line.chars().take(char_index) {
        column += if ch == '\t' {
            width - (column % width)
        } else {
            1
        };
    }
    column
}

pub fn guides(text: &str, width: usize) -> Vec<usize> {
    let width = width.max(1);
    let lines: Vec<&str> = text.split('\n').collect();
    let levels: Vec<Option<usize>> = lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                return None;
            }
            let columns: usize = line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .map(|c| if c == '\t' { width } else { 1 })
                .sum();
            Some(columns / width)
        })
        .collect();

    let mut out = vec![0usize; lines.len()];
    let mut above = 0usize;
    for (i, level) in levels.iter().enumerate() {
        out[i] = match level {
            Some(level) => {
                above = *level;
                *level
            }
            None => {
                let below = levels[i..].iter().find_map(|l| *l).unwrap_or(0);
                above.min(below)
            }
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_takes_the_columns_it_stands_for() {
        // Aligned to the next stop, not a fixed run of spaces.
        assert_eq!(expand_tabs("\tx", 4, 0), "    x");
        assert_eq!(expand_tabs("ab\tx", 4, 0), "ab  x");
        assert_eq!(expand_tabs("abc\tx", 4, 0), "abc x");
        assert_eq!(expand_tabs("abcd\tx", 4, 0), "abcd    x");
        // A piece that starts partway along the row carries on from there.
        assert_eq!(expand_tabs("\tx", 4, 2), "  x");
        // Text with no tab is handed back as it is.
        assert_eq!(expand_tabs("plain", 4, 0), "plain");
    }

    #[test]
    fn the_caret_counts_the_columns_a_tab_took() {
        // Two tabs and a letter: the caret after the letter is at column 9.
        assert_eq!(display_column("\t\tx", 3, 4), 9);
        assert_eq!(display_column("\t\tx", 0, 4), 0);
        assert_eq!(display_column("\t\tx", 1, 4), 4);
        assert_eq!(display_column("no tabs", 3, 4), 3);
    }

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

    #[test]
    fn guides_follow_the_leading_whitespace_in_whole_levels() {
        let text = "fn a() {\n    if x {\n        y();\n\n    }\n\tz();\n      w();\n}";
        assert_eq!(guides(text, 4), vec![0, 1, 2, 1, 1, 1, 1, 0]);
        // A blank line between two equally deep lines keeps their guides; a
        // trailing blank line has nothing below and takes nothing.
        assert_eq!(guides("    a\n\n    b\n", 4), vec![1, 1, 1, 0]);
        // A width of zero cannot divide anything and is read as one.
        assert_eq!(guides("  a", 0), vec![2]);
        assert_eq!(guides("", 4), vec![0]);
    }
}

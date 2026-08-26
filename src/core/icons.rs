//! The one set of glyphs both frontends draw: the navigator, the tab bar and
//! the sidebar footer say the same thing in the window as they do in the
//! terminal.
//!
//! Unicode by default; set `YARA_ASCII=1` (or run under a non-UTF-8 locale)
//! to get the ASCII set for terminals with sparse font coverage. The window
//! has its fonts under its own control and always draws the Unicode set.

#[derive(Clone, Copy)]
pub struct Icons {
    pub dir_open: &'static str,
    pub dir_closed: &'static str,
    pub file: &'static str,
    pub modified: &'static str,
    pub close: &'static str,
    pub menu_marker: &'static str,
    /// Sidebar footer switch: files, search, git.
    pub nav_files: &'static str,
    pub nav_search: &'static str,
    pub nav_git: &'static str,
    /// Placeholder in an empty input.
    pub ellipsis: &'static str,
    /// Tabs that are not files: a diff, and a rendered markdown preview.
    pub diff: &'static str,
    pub preview: &'static str,
    /// The box a markdown task list draws, ticked or not.
    pub check_on: &'static str,
    pub check_off: &'static str,
    /// Step to the previous or next change in a diff. The window draws its own
    /// arrows for the same pair, since it has the shapes to draw them with.
    pub up: &'static str,
    pub down: &'static str,
    /// Whether this is the ASCII set — what a chart drawn out of characters
    /// has to know, since it draws lines rather than any one glyph.
    pub ascii: bool,
}

const UNICODE: Icons = Icons {
    dir_open: "▾",
    dir_closed: "▸",
    file: "▫",
    modified: "●",
    close: "×",
    menu_marker: "›",
    nav_files: "≡",
    nav_search: "◎",
    nav_git: "⎇",
    ellipsis: "…",
    diff: "≠",
    preview: "◫",
    check_on: "☑",
    check_off: "☐",
    up: "▴",
    down: "▾",
    ascii: false,
};

const ASCII: Icons = Icons {
    dir_open: "v",
    dir_closed: ">",
    file: "-",
    modified: "*",
    close: "x",
    menu_marker: ">",
    nav_files: "=",
    nav_search: "o",
    nav_git: "Y",
    ellipsis: "...",
    diff: "=",
    preview: "#",
    check_on: "[x]",
    check_off: "[ ]",
    up: "^",
    down: "v",
    ascii: true,
};

/// The full set, for a frontend that carries its own fonts and so never has
/// to fall back — the window.
pub fn unicode() -> Icons {
    UNICODE
}

pub fn detect() -> Icons {
    if std::env::var_os("YARA_ASCII").is_some() {
        return ASCII;
    }
    // An explicitly non-UTF-8 locale is a real signal; an unset one is not, so
    // the Unicode set stays the default over bare SSH sessions.
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if value.trim().is_empty() {
                continue;
            }
            let upper = value.to_uppercase();
            return if upper.contains("UTF-8") || upper.contains("UTF8") {
                UNICODE
            } else {
                ASCII
            };
        }
    }
    UNICODE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ascii_set_stands_in_for_terminals_without_the_glyphs() {
        // Every field is filled in both sets, and none of them is empty.
        for icons in [UNICODE, ASCII] {
            for glyph in [
                icons.dir_open,
                icons.dir_closed,
                icons.file,
                icons.modified,
                icons.close,
                icons.menu_marker,
                icons.nav_files,
                icons.nav_search,
                icons.nav_git,
                icons.ellipsis,
                icons.diff,
                icons.preview,
                icons.check_on,
                icons.check_off,
                icons.up,
                icons.down,
            ] {
                assert!(!glyph.is_empty());
            }
        }
        assert!(ASCII.ellipsis.is_ascii(), "that is the point of the set");
        assert!(!UNICODE.ellipsis.is_ascii());
    }

    #[test]
    fn the_window_always_gets_the_drawn_set() {
        assert_eq!(unicode().dir_open, UNICODE.dir_open);
        assert_eq!(unicode().preview, UNICODE.preview);
    }

    #[test]
    fn the_environment_chooses_the_set() {
        // Whatever the machine says, one of the two comes back whole.
        let chosen = detect();
        assert!(chosen.file == UNICODE.file || chosen.file == ASCII.file);
        assert_eq!(
            chosen.menu_marker,
            if chosen.file == ASCII.file {
                ASCII.menu_marker
            } else {
                UNICODE.menu_marker
            },
            "the sets are never mixed"
        );
    }
}

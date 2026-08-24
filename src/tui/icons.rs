//! Glyphs for the terminal navigator and tab bar, mirroring the shapes the GPU
//! frontend draws.
//!
//! Unicode by default; set `YARA_ASCII=1` (or run under a non-UTF-8 locale)
//! to get the ASCII set for terminals with sparse font coverage.

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
};

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
            ] {
                assert!(!glyph.is_empty());
            }
        }
        assert!(ASCII.ellipsis.is_ascii(), "that is the point of the set");
        assert!(!UNICODE.ellipsis.is_ascii());
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

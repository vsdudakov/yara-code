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

//! Find (and replace) inside the open file — the bar Cmd+F opens.
//!
//! It shares the query options with project search so the two feel like one
//! feature: plain or regular expression, case sensitivity, whole words.

use std::path::{Path, PathBuf};

use regex::{Regex, RegexBuilder};

/// One hit, as character offsets into the buffer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hit {
    pub start: usize,
    pub end: usize,
    /// 0-based line the hit starts on.
    pub line: usize,
}

#[derive(Default)]
pub struct Find {
    pub open: bool,
    /// The file being searched. The bar belongs to that file: switching tabs
    /// hides it, and coming back brings it — query, hits and all — with you.
    pub owner: Option<PathBuf>,
    pub query: String,
    pub replace: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    /// Which input takes typing while the bar is open.
    pub in_replace_field: bool,
    pub hits: Vec<Hit>,
    pub current: usize,
    pub error: Option<String>,
    pub focus_pending: bool,
    ran: Option<(String, bool, bool, bool, u64)>,
}

fn hash_of(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

impl Find {
    /// Whether the bar belongs to `path` and should be drawn over it.
    pub fn shows_for(&self, path: &Path) -> bool {
        self.open && self.owner.as_deref() == Some(path)
    }

    /// Opens the bar on `path`, moving it there if it was on another file.
    pub fn open_on(&mut self, path: &Path) {
        if self.owner.as_deref() != Some(path) {
            self.owner = Some(path.to_path_buf());
            self.hits.clear();
            self.current = 0;
            self.ran = None;
        }
        self.open = true;
    }

    fn compile(&self) -> Result<Regex, String> {
        let base = if self.regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        let pattern = if self.whole_word {
            format!(r"\b(?:{base})\b")
        } else {
            base
        };
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.case_sensitive)
            .multi_line(true)
            .build()
            .map_err(|e| {
                e.to_string()
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("invalid pattern")
                    .to_string()
            })
    }

    /// Recomputes hits when the query, the options or the text changed.
    pub fn refresh(&mut self, text: &str) {
        // The text is keyed by a hash, not its length: a replacement of equal
        // length (and the undo of one) changes what matches without changing
        // how long the file is.
        let key = (
            self.query.clone(),
            self.regex,
            self.case_sensitive,
            self.whole_word,
            hash_of(text),
        );
        if self.ran.as_ref() == Some(&key) {
            return;
        }
        self.ran = Some(key);
        self.search(text);
    }

    fn search(&mut self, text: &str) {
        self.hits.clear();
        self.error = None;
        if self.query.is_empty() {
            return;
        }
        let regex = match self.compile() {
            Ok(regex) => regex,
            Err(message) => {
                self.error = Some(message);
                return;
            }
        };
        // Byte offsets come back from the regex; the editors count characters.
        let mut chars_before = 0usize;
        let mut byte_cursor = 0usize;
        let mut line = 0usize;
        for m in regex.find_iter(text) {
            let skipped = &text[byte_cursor..m.start()];
            chars_before += skipped.chars().count();
            line += skipped.matches('\n').count();
            let length = text[m.start()..m.end()].chars().count();
            self.hits.push(Hit {
                start: chars_before,
                end: chars_before + length,
                line,
            });
            chars_before += length;
            line += text[m.start()..m.end()].matches('\n').count();
            byte_cursor = m.end();
        }
        if self.current >= self.hits.len() {
            self.current = 0;
        }
    }

    pub fn hit(&self) -> Option<Hit> {
        self.hits.get(self.current).copied()
    }

    /// Steps to the next or previous hit, wrapping around.
    pub fn step(&mut self, delta: isize) {
        if self.hits.is_empty() {
            return;
        }
        let len = self.hits.len() as isize;
        self.current = ((self.current as isize + delta).rem_euclid(len)) as usize;
    }

    /// Selects the first hit at or after `from`, so opening the bar lands on
    /// the match nearest the cursor rather than the top of the file.
    pub fn select_near(&mut self, from: usize) {
        if let Some(index) = self.hits.iter().position(|h| h.start >= from) {
            self.current = index;
        }
    }

    pub fn summary(&self) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        if self.query.is_empty() {
            return String::new();
        }
        if self.hits.is_empty() {
            return "No results".to_string();
        }
        format!("{} of {}", self.current + 1, self.hits.len())
    }

    /// Replaces the current hit, returning the text to store and where the
    /// cursor should end up.
    pub fn replace_current(&mut self, text: &str) -> Option<(String, usize)> {
        let hit = self.hit()?;
        let replacement = self.expand(text, hit)?;
        let start = byte_of(text, hit.start);
        let end = byte_of(text, hit.end);
        let mut updated = text.to_string();
        updated.replace_range(start..end, &replacement);
        let cursor = hit.start + replacement.chars().count();
        self.ran = None; // hits are stale now
        Some((updated, cursor))
    }

    /// Replaces every hit; returns the new text and how many were replaced.
    pub fn replace_all(&mut self, text: &str) -> Option<(String, usize)> {
        if self.hits.is_empty() {
            return None;
        }
        let regex = self.compile().ok()?;
        let replacement = if self.regex {
            self.replace.clone()
        } else {
            self.replace.replace('$', "$$")
        };
        let count = self.hits.len();
        let updated = regex.replace_all(text, replacement.as_str()).into_owned();
        self.ran = None;
        Some((updated, count))
    }

    /// The replacement for one hit, with capture groups expanded in regex mode.
    fn expand(&self, text: &str, hit: Hit) -> Option<String> {
        if !self.regex {
            return Some(self.replace.clone());
        }
        let regex = self.compile().ok()?;
        let start = byte_of(text, hit.start);
        let end = byte_of(text, hit.end);
        let captures = regex.captures(&text[start..end])?;
        let mut out = String::new();
        captures.expand(&self.replace, &mut out);
        Some(out)
    }
}

fn byte_of(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map_or(text.len(), |(b, _)| b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "let total = 1;\nlet TOTAL = 2;\nlet subtotal = 3;\n";

    fn find(build: impl FnOnce(&mut Find)) -> Find {
        let mut f = Find::default();
        build(&mut f);
        f.refresh(TEXT);
        f
    }

    #[test]
    fn finds_every_hit_with_line_numbers() {
        let f = find(|f| f.query = "total".into());
        assert_eq!(f.hits.len(), 3);
        assert_eq!(f.hits[0].line, 0);
        assert_eq!(f.hits[1].line, 1);
        assert_eq!(f.hits[2].line, 2);
        assert_eq!(f.summary(), "1 of 3");
    }

    #[test]
    fn options_narrow_the_hits() {
        let f = find(|f| {
            f.query = "total".into();
            f.case_sensitive = true;
            f.whole_word = true;
        });
        assert_eq!(f.hits.len(), 1);
    }

    #[test]
    fn stepping_wraps_in_both_directions() {
        let mut f = find(|f| f.query = "total".into());
        f.step(1);
        assert_eq!(f.current, 1);
        f.step(-1);
        f.step(-1);
        assert_eq!(f.current, 2, "wrapped backwards to the last hit");
    }

    #[test]
    fn hits_are_character_offsets_not_bytes() {
        let text = "фыва total\nещё total\n";
        let mut f = Find {
            query: "total".into(),
            ..Find::default()
        };
        f.refresh(text);
        assert_eq!(f.hits[0].start, 5, "five characters precede the first hit");
        let chars: String = text.chars().skip(f.hits[0].start).take(5).collect();
        assert_eq!(chars, "total");
    }

    #[test]
    fn selecting_near_the_cursor_picks_the_next_hit() {
        let mut f = find(|f| f.query = "total".into());
        f.select_near(20);
        assert_eq!(f.current, 2);
    }

    #[test]
    fn replacing_the_current_hit_leaves_the_rest() {
        let mut f = find(|f| {
            f.query = "total".into();
            f.replace = "sum".into();
        });
        let (updated, cursor) = f.replace_current(TEXT).unwrap();
        assert_eq!(updated, "let sum = 1;\nlet TOTAL = 2;\nlet subtotal = 3;\n");
        assert_eq!(cursor, 7);
    }

    #[test]
    fn replace_all_takes_them_all() {
        let mut f = find(|f| {
            f.query = "total".into();
            f.case_sensitive = true;
            f.replace = "sum".into();
        });
        let (updated, count) = f.replace_all(TEXT).unwrap();
        assert_eq!(count, 2);
        assert_eq!(updated, "let sum = 1;\nlet TOTAL = 2;\nlet subsum = 3;\n");
    }

    #[test]
    fn regex_replacement_expands_groups() {
        let mut f = Find {
            query: r"let (\w+) = 1;".into(),
            regex: true,
            replace: "const $1 = 1;".into(),
            ..Find::default()
        };
        f.refresh(TEXT);
        let (updated, _) = f.replace_current(TEXT).unwrap();
        assert!(updated.starts_with("const total = 1;"), "{updated}");
    }

    #[test]
    fn a_bad_pattern_reports_instead_of_matching() {
        let f = find(|f| {
            f.query = "(".into();
            f.regex = true;
        });
        assert!(f.hits.is_empty());
        assert!(f.error.is_some());
        assert_eq!(f.summary(), f.error.clone().unwrap());
    }
}

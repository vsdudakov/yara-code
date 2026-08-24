//! Project-wide search and replace.
//!
//! Plain and regular-expression searches take the same path: a plain query is
//! escaped into a pattern, so case sensitivity, whole-word matching and replace
//! all behave identically either way.

use std::path::{Path, PathBuf};

use regex::{Regex, RegexBuilder};

use crate::core::glob;

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".ycode",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
];
const MAX_FILE_SIZE: u64 = 1_000_000;
const MAX_MATCHES: usize = 1000;

pub struct SearchMatch {
    pub line: usize, // 1-based
    pub prefix: String,
    pub matched: String,
    pub suffix: String,
}

pub struct FileResult {
    pub path: PathBuf,
    pub matches: Vec<SearchMatch>,
}

/// Which input the frontends currently direct typing into. The same order is
/// used by both, so the panels look and behave alike.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Query,
    Replace,
    Exclude,
}

impl Field {
    /// The fields on screen, in order.
    pub fn visible() -> Vec<Field> {
        vec![Field::Query, Field::Replace, Field::Exclude]
    }

    /// Placeholder for an empty field. The heading above already names it, so
    /// this only marks the row as an input waiting for text.
    pub fn hint(&self) -> &'static str {
        "…"
    }

    /// A sample value shown under the field, for the one whose syntax isn't
    /// obvious from its name.
    pub fn example(&self) -> Option<&'static str> {
        match self {
            Field::Exclude => Some("e.g. target, *.lock, **/node_modules"),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct Search {
    pub query: String,
    pub replace: String,
    /// Comma-separated exclude globs, as in VS Code's "files to exclude".
    pub exclude: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub field: FieldState,
    pub results: Vec<FileResult>,
    pub truncated: bool,
    /// Why the last search could not run, e.g. a bad regular expression.
    pub error: Option<String>,
    pub focus_pending: bool,
    ran: Option<Key>,
}

/// The focused input, defaulting to the query.
pub struct FieldState(pub Field);

impl Default for FieldState {
    fn default() -> Self {
        Self(Field::Query)
    }
}

#[derive(PartialEq, Eq, Clone)]
struct Key {
    query: String,
    exclude: String,
    regex: bool,
    case_sensitive: bool,
    whole_word: bool,
}

impl Search {
    pub fn total_matches(&self) -> usize {
        self.results.iter().map(|f| f.matches.len()).sum()
    }

    pub fn field(&self) -> Field {
        self.field.0
    }

    pub fn set_field(&mut self, field: Field) {
        self.field.0 = field;
    }

    /// Moves focus through the visible inputs, wrapping around.
    pub fn cycle_field(&mut self, delta: isize) {
        let fields = Field::visible();
        let current = fields.iter().position(|f| *f == self.field()).unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(fields.len() as isize) as usize;
        self.field.0 = fields[next];
    }

    pub fn input_mut(&mut self) -> &mut String {
        match self.field() {
            Field::Query => &mut self.query,
            Field::Replace => &mut self.replace,
            Field::Exclude => &mut self.exclude,
        }
    }

    pub fn input(&self, field: Field) -> &str {
        match field {
            Field::Query => &self.query,
            Field::Replace => &self.replace,
            Field::Exclude => &self.exclude,
        }
    }

    /// Turns the query into a pattern: escaped when plain, wrapped in word
    /// boundaries when whole-word matching is on.
    fn pattern(&self) -> String {
        let base = if self.regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        if self.whole_word {
            format!(r"\b(?:{base})\b")
        } else {
            base
        }
    }

    fn compile(&self) -> Result<Regex, String> {
        RegexBuilder::new(&self.pattern())
            .case_insensitive(!self.case_sensitive)
            .build()
            .map_err(|e| {
                // The multi-line explanation regex produces is too tall for a
                // status bar; the first line names the problem.
                e.to_string()
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("invalid pattern")
                    .to_string()
            })
    }

    /// Re-runs the search when the query, the options or the exclude list
    /// changed. The replacement text alone never triggers a re-search.
    pub fn run_if_changed(&mut self, roots: &[PathBuf]) {
        let key = Key {
            query: self.query.clone(),
            exclude: self.exclude.clone(),
            regex: self.regex,
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
        };
        if self.ran.as_ref() == Some(&key) {
            return;
        }
        self.ran = Some(key);
        self.run(roots);
    }

    /// Runs the search unconditionally, e.g. after files changed on disk.
    pub fn run(&mut self, roots: &[PathBuf]) {
        self.results.clear();
        self.truncated = false;
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
        let excludes = glob::parse_list(&self.exclude);
        let mut total = 0usize;
        let mut results = Vec::new();
        let mut truncated = false;
        let mut visit = |path: &Path, text: &str| {
            let mut matches = Vec::new();
            for (i, line) in text.lines().enumerate() {
                if total >= MAX_MATCHES {
                    truncated = true;
                    break;
                }
                if let Some(m) = regex.find(line) {
                    matches.push(make_match(i + 1, line, m.start(), m.end()));
                    total += 1;
                }
            }
            if !matches.is_empty() {
                results.push(FileResult {
                    path: path.to_path_buf(),
                    matches,
                });
            }
            total < MAX_MATCHES
        };
        for root in roots {
            walk(root, root, &excludes, &mut visit);
        }
        self.results = results;
        self.truncated = truncated || total >= MAX_MATCHES;
    }

    /// Rewrites every match in the current results. Returns how many matches
    /// were replaced in how many files, or why it could not run.
    pub fn replace_all(&mut self, roots: &[PathBuf]) -> Result<(usize, usize), String> {
        if self.query.is_empty() {
            return Err("nothing to replace".into());
        }
        let regex = self.compile()?;
        // A plain search takes its replacement literally; only a regular
        // expression search expands $1 and friends.
        let replacement = if self.regex {
            self.replace.clone()
        } else {
            self.replace.replace('$', "$$")
        };

        let paths: Vec<PathBuf> = self.results.iter().map(|f| f.path.clone()).collect();
        let mut files = 0usize;
        let mut count = 0usize;
        for path in paths {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let hits = regex.find_iter(&text).count();
            if hits == 0 {
                continue;
            }
            let updated = regex.replace_all(&text, replacement.as_str());
            if std::fs::write(&path, updated.as_ref()).is_ok() {
                files += 1;
                count += hits;
            }
        }
        self.run(roots);
        Ok((count, files))
    }
}

/// Walks the project, handing each readable text file to `visit`. Returning
/// false from `visit` stops the walk.
fn walk(dir: &Path, root: &Path, excludes: &[String], visit: &mut dyn FnMut(&Path, &str) -> bool) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name().to_ascii_lowercase());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // Excluded paths are skipped whole, so an excluded directory is never
        // even walked.
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if glob::matches_any(excludes, &relative) {
            continue;
        }
        if kind.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(&path, root, excludes, visit);
            }
        } else if kind.is_file() {
            if entry
                .metadata()
                .map(|m| m.len() > MAX_FILE_SIZE)
                .unwrap_or(true)
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue; // binary / non-utf8
            };
            if !visit(&path, &text) {
                return;
            }
        }
    }
}

/// Splits a line into prefix / matched / suffix around a byte range.
fn make_match(line_no: usize, line: &str, start: usize, end: usize) -> SearchMatch {
    let (prefix, matched, suffix) =
        if line.is_char_boundary(start) && end <= line.len() && line.is_char_boundary(end) {
            (&line[..start], &line[start..end], &line[end..])
        } else {
            ("", "", line)
        };
    let clip_front = |s: &str, max: usize| -> String {
        if s.chars().count() > max {
            let skip = s.chars().count() - max;
            format!("...{}", s.chars().skip(skip).collect::<String>())
        } else {
            s.to_string()
        }
    };
    let clip_back = |s: &str, max: usize| -> String {
        if s.chars().count() > max {
            format!("{}...", s.chars().take(max).collect::<String>())
        } else {
            s.to_string()
        }
    };
    SearchMatch {
        line: line_no,
        prefix: clip_front(prefix.trim_start(), 40),
        matched: matched.to_string(),
        suffix: clip_back(suffix, 120),
    }
}

// ---------------------------------------------------------------------------
// Definition lookup
// ---------------------------------------------------------------------------

pub struct Candidate {
    pub path: PathBuf,
    pub line: usize, // 1-based
    pub text: String,
}

/// Definition-introducing keywords across common languages; the token right
/// before a clicked identifier is checked against this list.
const DEF_KEYWORDS: &[&str] = &[
    "fn",
    "struct",
    "enum",
    "trait",
    "type",
    "mod",
    "const",
    "static",
    "impl",
    "macro_rules!",
    "def",
    "class",
    "function",
    "func",
    "fun",
    "interface",
    "let",
    "var",
    "val",
    "protocol",
    "module",
    "object",
    "message",
    "service",
];

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Byte offsets of whole-word occurrences of `word` in `line`.
fn word_occurrences<'a>(line: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    line.match_indices(word).filter_map(move |(i, _)| {
        let before_ok = i == 0 || !line[..i].chars().last().is_some_and(is_ident_char);
        let after_ok = !line[i + word.len()..]
            .chars()
            .next()
            .is_some_and(is_ident_char);
        (before_ok && after_ok).then_some(i)
    })
}

fn is_definition_line(line: &str, word: &str) -> bool {
    for idx in word_occurrences(line, word) {
        let last_token = line[..idx]
            .split(|c: char| c.is_whitespace() || matches!(c, '(' | '<' | ',' | ':' | '{'))
            .rfind(|t| !t.is_empty());
        if last_token.is_some_and(|t| DEF_KEYWORDS.contains(&t)) {
            return true;
        }
    }
    false
}

fn clip_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() > 160 {
        format!("{}...", trimmed.chars().take(160).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

pub fn find_definitions(roots: &[PathBuf], word: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut visit = |path: &Path, text: &str| {
        for (i, line) in text.lines().enumerate() {
            if line.contains(word) && is_definition_line(line, word) {
                out.push(Candidate {
                    path: path.to_path_buf(),
                    line: i + 1,
                    text: clip_line(line),
                });
            }
        }
        out.len() < 50
    };
    for root in roots {
        walk(root, root, &[], &mut visit);
    }
    out
}

pub fn find_references(roots: &[PathBuf], word: &str, cap: usize) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut visit = |path: &Path, text: &str| {
        for (i, line) in text.lines().enumerate() {
            if line.contains(word) && word_occurrences(line, word).next().is_some() {
                out.push(Candidate {
                    path: path.to_path_buf(),
                    line: i + 1,
                    text: clip_line(line),
                });
                if out.len() >= cap {
                    return false;
                }
            }
        }
        true
    };
    for root in roots {
        walk(root, root, &[], &mut visit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_support::Dir;

    fn project() -> (Dir, PathBuf) {
        let dir = Dir::new("yara-search");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "let total = 1;\nlet TOTAL = 2;\n").unwrap();
        std::fs::write(root.join("src/b.rs"), "let subtotal = 3;\n").unwrap();
        std::fs::write(root.join("skip.md"), "total here\n").unwrap();
        (dir, root)
    }

    fn search(root: &Path, build: impl FnOnce(&mut Search)) -> Search {
        let mut s = Search::default();
        build(&mut s);
        s.run(&[root.to_path_buf()]);
        s
    }

    #[test]
    fn plain_search_is_case_insensitive_by_default() {
        let (_dir, root) = project();
        let s = search(&root, |s| s.query = "total".into());
        // a.rs twice, b.rs once (subtotal), skip.md once.
        assert_eq!(s.total_matches(), 4);
    }

    #[test]
    fn case_sensitivity_and_whole_words_narrow_it_down() {
        let (_dir, root) = project();
        let s = search(&root, |s| {
            s.query = "total".into();
            s.case_sensitive = true;
        });
        assert_eq!(s.total_matches(), 3, "TOTAL excluded");

        let s = search(&root, |s| {
            s.query = "total".into();
            s.case_sensitive = true;
            s.whole_word = true;
        });
        assert_eq!(s.total_matches(), 2, "subtotal excluded too");
    }

    #[test]
    fn regex_mode_matches_patterns() {
        let (_dir, root) = project();
        let s = search(&root, |s| {
            s.query = r"let \w+ = \d;".into();
            s.regex = true;
        });
        assert_eq!(s.total_matches(), 3);
    }

    #[test]
    fn a_bad_pattern_reports_instead_of_matching() {
        let (_dir, root) = project();
        let s = search(&root, |s| {
            s.query = "let (".into();
            s.regex = true;
        });
        assert!(s.error.is_some(), "expected a compile error");
        assert!(s.results.is_empty());
    }

    #[test]
    fn excludes_apply_to_search() {
        let (_dir, root) = project();
        let s = search(&root, |s| {
            s.query = "total".into();
            s.exclude = "*.md".into();
        });
        assert_eq!(s.total_matches(), 3);
    }

    #[test]
    fn replace_all_rewrites_matches_on_disk() {
        let (_dir, root) = project();
        let mut s = search(&root, |s| {
            s.query = "total".into();
            s.case_sensitive = true;
            s.whole_word = true;
            s.replace = "sum".into();
        });
        let (count, files) = s.replace_all(std::slice::from_ref(&root)).unwrap();
        assert_eq!((count, files), (2, 2));
        let a = std::fs::read_to_string(root.join("src/a.rs")).unwrap();
        assert_eq!(a, "let sum = 1;\nlet TOTAL = 2;\n");
        // The results refresh, so the replaced text is gone from them.
        assert_eq!(s.total_matches(), 0);
    }

    #[test]
    fn plain_replacement_text_is_literal() {
        let (_dir, root) = project();
        let mut s = search(&root, |s| {
            s.query = "subtotal".into();
            s.replace = "$1x".into();
        });
        s.replace_all(std::slice::from_ref(&root)).unwrap();
        let b = std::fs::read_to_string(root.join("src/b.rs")).unwrap();
        assert_eq!(b, "let $1x = 3;\n");
    }

    #[test]
    fn regex_replacement_expands_groups() {
        let (_dir, root) = project();
        let mut s = search(&root, |s| {
            s.query = r"let (\w+) = 3;".into();
            s.regex = true;
            s.replace = "const $1 = 3;".into();
        });
        s.replace_all(std::slice::from_ref(&root)).unwrap();
        let b = std::fs::read_to_string(root.join("src/b.rs")).unwrap();
        assert_eq!(b, "const subtotal = 3;\n");
    }

    #[test]
    fn fields_cycle_in_order() {
        let mut s = Search::default();
        assert_eq!(s.field(), Field::Query);
        s.cycle_field(1);
        assert_eq!(s.field(), Field::Replace);
        s.cycle_field(1);
        assert_eq!(s.field(), Field::Exclude);
        s.cycle_field(1);
        assert_eq!(s.field(), Field::Query, "wraps around");
        s.cycle_field(-1);
        assert_eq!(s.field(), Field::Exclude);
    }
}

#[cfg(test)]
mod more_search_tests {
    use super::*;
    use crate::core::test_support::Dir;

    fn ran(query: &str, roots: &[PathBuf], build: impl FnOnce(&mut Search)) -> Search {
        let mut search = Search {
            query: query.to_string(),
            ..Default::default()
        };
        build(&mut search);
        search.run(roots);
        search
    }

    #[test]
    fn the_fields_are_the_three_the_panel_draws() {
        assert_eq!(
            Field::visible(),
            [Field::Query, Field::Replace, Field::Exclude]
        );
        assert_eq!(Field::Query.hint(), "…");
        assert!(Field::Exclude.example().is_some());
        assert!(Field::Query.example().is_none());
    }

    #[test]
    fn the_focused_field_cycles_and_takes_the_typing() {
        let mut search = Search::default();
        assert_eq!(search.field(), Field::Query);
        search.input_mut().push_str("needle");
        assert_eq!(search.input(Field::Query), "needle");

        search.cycle_field(1);
        assert_eq!(search.field(), Field::Replace);
        search.input_mut().push_str("thread");
        search.cycle_field(1);
        assert_eq!(search.field(), Field::Exclude);
        // Past the end it wraps, so Tab keeps working.
        search.cycle_field(1);
        assert_eq!(search.field(), Field::Query);
        search.cycle_field(-1);
        assert_eq!(search.field(), Field::Exclude);

        search.set_field(Field::Replace);
        assert_eq!(search.input(Field::Replace), "thread");
    }

    #[test]
    fn a_search_can_be_narrowed_by_case_words_and_regex() {
        let dir = Dir::new("yara-search-opts");
        // A result is a line, not an occurrence, so each case gets its own.
        dir.file("a.txt", "Total\ntotal\ntotals\n");
        let roots = vec![dir.path().to_path_buf()];

        assert_eq!(ran("total", &roots, |_| {}).total_matches(), 3);
        assert_eq!(
            ran("total", &roots, |s| s.case_sensitive = true).total_matches(),
            2,
            "Total is out"
        );
        assert_eq!(
            ran("total", &roots, |s| s.whole_word = true).total_matches(),
            2,
            "totals is out"
        );
        assert_eq!(
            ran(r"tot\w+s", &roots, |s| s.regex = true).total_matches(),
            1
        );
    }

    #[test]
    fn a_bad_pattern_is_reported_and_finds_nothing() {
        let dir = Dir::new("yara-search-bad");
        dir.file("a.txt", "text\n");
        let search = ran("(unclosed", &[dir.path().to_path_buf()], |s| s.regex = true);
        assert!(search.error.is_some());
        assert_eq!(search.total_matches(), 0);
    }

    #[test]
    fn an_empty_query_searches_nothing_at_all() {
        let dir = Dir::new("yara-search-empty");
        dir.file("a.txt", "text\n");
        let search = ran("", &[dir.path().to_path_buf()], |_| {});
        assert!(search.results.is_empty());
        assert!(search.error.is_none());
    }

    #[test]
    fn a_second_run_is_skipped_unless_the_query_or_options_moved() {
        let dir = Dir::new("yara-search-cache");
        let file = dir.file("a.txt", "one\n");
        let roots = vec![dir.path().to_path_buf()];
        let mut search = Search {
            query: "one".into(),
            ..Default::default()
        };
        search.run_if_changed(&roots);
        assert_eq!(search.total_matches(), 1);

        // The file changed underneath: the cached run stands until something
        // about the query changes.
        std::fs::write(&file, "one\none\n").unwrap();
        search.run_if_changed(&roots);
        assert_eq!(search.total_matches(), 1, "the cached run stands");
        search.case_sensitive = true;
        search.run_if_changed(&roots);
        assert_eq!(search.total_matches(), 2, "an option is a new search");
    }

    #[test]
    fn several_project_folders_are_searched_as_one() {
        let one = Dir::new("yara-search-one");
        let two = Dir::new("yara-search-two");
        one.file("a.txt", "needle\n");
        two.file("b.txt", "needle\nneedle\n");
        let search = ran(
            "needle",
            &[one.path().to_path_buf(), two.path().to_path_buf()],
            |_| {},
        );
        assert_eq!(search.results.len(), 2, "one result group per file");
        assert_eq!(search.total_matches(), 3);
    }

    #[test]
    fn replacing_rewrites_every_match_and_re_runs() {
        let dir = Dir::new("yara-search-replace");
        let file = dir.file("a.txt", "one one\ntwo\n");
        let roots = vec![dir.path().to_path_buf()];
        let mut search = ran("one", &roots, |s| s.replace = "1".into());
        let (count, files) = search.replace_all(&roots).unwrap();
        assert_eq!((count, files), (2, 1));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "1 1\ntwo\n");
        assert_eq!(search.total_matches(), 0, "the results are refreshed");

        // A plain search takes its replacement literally, dollars and all.
        let file = dir.file("b.txt", "price\n");
        let mut search = ran("price", &roots, |s| s.replace = "$1 each".into());
        search.replace_all(&roots).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "$1 each\n");

        // Nothing to replace is an error, not a silent success.
        let mut empty = Search::default();
        assert!(empty.replace_all(&roots).is_err());
    }

    #[test]
    fn go_to_definition_prefers_a_definition_and_falls_back_to_uses() {
        let dir = Dir::new("yara-search-goto");
        dir.file(
            "lib.rs",
            "fn compute_total(x: u32) -> u32 { x }\nlet y = compute_total(1);\n",
        );
        let roots = vec![dir.path().to_path_buf()];

        let defs = find_definitions(&roots, "compute_total");
        assert_eq!(defs.len(), 1, "the fn line, not the call");
        assert_eq!(defs[0].line, 1);
        assert!(defs[0].text.contains("fn compute_total"));

        let refs = find_references(&roots, "compute_total", 10);
        assert_eq!(refs.len(), 2);
        // The cap is honoured, so a common word cannot flood the picker.
        assert_eq!(find_references(&roots, "compute_total", 1).len(), 1);
        assert!(find_definitions(&roots, "nothing_here").is_empty());
    }

    #[test]
    fn a_long_line_is_clipped_for_the_picker() {
        let dir = Dir::new("yara-search-clip");
        let long = format!("let x = 1; // {}", "y".repeat(400));
        dir.file("a.rs", &format!("{long}\n"));
        let refs = find_references(&[dir.path().to_path_buf()], "let", 5);
        assert!(refs[0].text.chars().count() < 200, "{}", refs[0].text.len());
    }

    #[test]
    fn excluded_folders_are_never_even_walked() {
        let dir = Dir::new("yara-search-exclude");
        dir.file("keep/a.txt", "needle\n");
        dir.file("skip/b.txt", "needle\n");
        dir.file("c.lock", "needle\n");
        let roots = vec![dir.path().to_path_buf()];
        let search = ran("needle", &roots, |s| s.exclude = "skip, *.lock".into());
        assert_eq!(search.results.len(), 1);
        assert!(search.results[0].path.ends_with("a.txt"));
    }
}

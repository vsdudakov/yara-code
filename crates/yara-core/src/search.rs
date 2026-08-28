//! Project search: every line in every file under the root that contains
//! the query, case-insensitively, skipping what the exclude list names and
//! anything that is not text.

use std::path::Path;

use crate::{glob, tree};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Relative to the root, with forward slashes.
    pub path: String,
    /// From one, as editors count.
    pub line: usize,
    pub text: String,
}

/// What a search found, and how many files it found it in. Stops at `cap`
/// hits so a common word cannot stall the editor.
pub fn search(root: &Path, query: &str, exclude: &[String], cap: usize) -> (Vec<Hit>, usize) {
    let mut hits = Vec::new();
    let mut files = 0;
    if query.is_empty() {
        return (hits, files);
    }
    let needle = query.to_lowercase();
    for path in tree::all_files(root) {
        if glob::matches_any(exclude, &path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&path)) else {
            continue;
        };
        let before = hits.len();
        for (i, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                hits.push(Hit {
                    path: path.clone(),
                    line: i + 1,
                    text: line.trim().to_string(),
                });
                if hits.len() == cap {
                    return (hits, files + 1);
                }
            }
        }
        if hits.len() > before {
            files += 1;
        }
    }
    (hits, files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Dir;

    #[test]
    fn a_query_is_found_in_every_text_file_but_the_excluded_ones() {
        let dir = Dir::new("yara-search");
        dir.file("src/main.rs", "fn main() {\n    let Total = 1;\n}\n");
        dir.file("README.md", "total recall\n");
        dir.file("target/out.rs", "total\n");
        dir.file("bin.dat", "\u{fffd}\x00total");
        std::fs::write(dir.path().join("raw.bin"), [0xff, 0xfe, b't']).unwrap();
        let exclude = vec!["target".to_string()];
        let (hits, files) = search(dir.path(), "TOTAL", &exclude, 100);
        assert_eq!(files, 3);
        let found: Vec<(&str, usize, &str)> = hits
            .iter()
            .map(|h| (h.path.as_str(), h.line, h.text.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("README.md", 1, "total recall"),
                ("bin.dat", 1, "\u{fffd}\x00total"),
                ("src/main.rs", 2, "let Total = 1;"),
            ]
        );
        assert_eq!(search(dir.path(), "", &exclude, 100).0.len(), 0);
        assert_eq!(search(dir.path(), "total", &[], 2).0.len(), 2, "capped");
    }
}

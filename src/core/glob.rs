//! Glob matching for the search's exclude list, in VS Code's spelling:
//! `target`, `*.lock`, `**/node_modules`, `src/**/tests`.

/// Splits a comma-separated exclude list into patterns.
pub fn parse_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(|p| p.trim().trim_matches('/'))
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

fn has_wildcard(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

/// Matches one path component against a pattern component containing `*`/`?`.
fn component_matches(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    // Position to resume from after the most recent `*`, for backtracking.
    let (mut star, mut resume) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            resume = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            resume += 1;
            ti = resume;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Matches `path` components against `pattern` components, where `**` spans any
/// number of components.
fn segments_match(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => {
            // `**` may consume zero or more components.
            (0..=path.len()).any(|skip| segments_match(rest, &path[skip..]))
        }
        Some((head, rest)) => match path.split_first() {
            Some((first, tail)) if component_matches(head, first) => {
                segments_match(rest, tail)
            }
            _ => false,
        },
    }
}

/// Whether a path relative to the project root is excluded by `pattern`.
///
/// A pattern without wildcards or slashes matches any path component, so
/// `target` excludes `target/` anywhere in the tree — the behavior people
/// expect from an exclude box.
pub fn matches(pattern: &str, relative_path: &str) -> bool {
    let path: Vec<&str> = relative_path
        .split('/')
        .filter(|c| !c.is_empty())
        .collect();
    if path.is_empty() {
        return false;
    }
    if !pattern.contains('/') && !has_wildcard(pattern) {
        return path.contains(&pattern);
    }
    if !pattern.contains('/') {
        // A bare wildcard pattern applies to any single component, which is how
        // `*.lock` is meant to read.
        return path.iter().any(|c| component_matches(pattern, c));
    }
    let segments: Vec<&str> = pattern.split('/').filter(|c| !c.is_empty()).collect();
    if segments_match(&segments, &path) {
        return true;
    }
    // An anchored pattern should also match anything beneath a directory it
    // names, so `src/generated` excludes `src/generated/api.rs`.
    (1..path.len()).any(|len| segments_match(&segments, &path[..len]))
}

pub fn matches_any(patterns: &[String], relative_path: &str) -> bool {
    patterns.iter().any(|p| matches(p, relative_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_names_match_any_component() {
        assert!(matches("target", "target/debug/app"));
        assert!(matches("target", "crates/target/x.rs"));
        assert!(!matches("target", "targets/x.rs"));
    }

    #[test]
    fn wildcards_apply_per_component() {
        assert!(matches("*.lock", "Cargo.lock"));
        assert!(matches("*.lock", "sub/dir/Cargo.lock"));
        assert!(!matches("*.lock", "Cargo.toml"));
        assert!(matches("test_?.py", "test_a.py"));
        assert!(!matches("test_?.py", "test_ab.py"));
    }

    #[test]
    fn double_star_spans_directories() {
        assert!(matches("**/node_modules", "a/b/node_modules"));
        assert!(matches("**/node_modules", "node_modules"));
        assert!(matches("src/**/tests", "src/a/b/tests"));
        assert!(matches("src/**/tests", "src/tests"));
        assert!(!matches("src/**/tests", "lib/tests"));
    }

    #[test]
    fn anchored_patterns_cover_their_contents() {
        assert!(matches("src/generated", "src/generated/api.rs"));
        assert!(!matches("src/generated", "src/other/api.rs"));
    }

    #[test]
    fn list_parsing_trims_and_drops_blanks() {
        assert_eq!(
            parse_list(" target, *.lock ,, /dist/ "),
            vec!["target", "*.lock", "dist"]
        );
    }

    #[test]
    fn empty_list_excludes_nothing() {
        assert!(!matches_any(&parse_list(""), "src/main.rs"));
    }
}

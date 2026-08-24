//! The matcher behind the pickers: a query matches a candidate when its
//! characters appear in order, and the closer together and the nearer the
//! start they are, the better the match. Both frontends rank with this, so
//! typing the same thing lists the same files in the same order.

/// A score for `candidate` against `query`, higher being better; `None` when
/// the query's characters are not all there in order. An empty query matches
/// everything equally.
pub fn score(query: &str, candidate: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let text: Vec<char> = candidate.chars().flat_map(char::to_lowercase).collect();
    let mut score: i64 = 0;
    let mut at = 0usize;
    let mut previous: Option<usize> = None;
    for q in &query {
        let found = (at..text.len()).find(|i| text[*i] == *q)?;
        // Adjacent characters read as one word typed; a character on a word
        // boundary reads as an initial, which is what abbreviations are.
        if previous == Some(found.wrapping_sub(1)) {
            score += 8;
        }
        if found == 0 || matches!(text[found - 1], '/' | '_' | '-' | '.' | ' ') {
            score += 6;
        }
        score -= (found - at) as i64;
        previous = Some(found);
        at = found + 1;
    }
    // Shorter candidates win ties: they are the more exact answer.
    Some(score * 4 - text.len() as i64)
}

/// The candidates that match, best first, as (index, label).
pub fn rank<'a>(query: &str, candidates: impl Iterator<Item = &'a str>) -> Vec<(usize, &'a str)> {
    let mut hits: Vec<(i64, usize, &str)> = candidates
        .enumerate()
        .filter_map(|(i, text)| score(query, text).map(|s| (s, i, text)))
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, i, text)| (i, text)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_must_appear_in_order() {
        assert!(score("abc", "a_b_c").is_some());
        assert!(score("cba", "a_b_c").is_none());
        assert!(score("", "anything").is_some());
    }

    #[test]
    fn a_run_and_an_initial_beat_scattered_letters() {
        let names = ["src/core/settings.rs", "src/gui/app.rs", "tests/gui_e2e.rs"];
        let ranked = rank("set", names.iter().copied());
        assert_eq!(ranked[0].1, "src/core/settings.rs");
        let ranked = rank("ga", names.iter().copied());
        assert_eq!(ranked[0].1, "src/gui/app.rs", "{ranked:?}");
    }

    #[test]
    fn the_shorter_of_two_equal_matches_comes_first() {
        let ranked = rank("app", ["src/gui/app.rs", "app.rs"].into_iter());
        assert_eq!(ranked[0].1, "app.rs");
    }
}

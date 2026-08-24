//! Foldable regions, derived from indentation.
//!
//! Indentation is what both frontends can compute for any language, including
//! ones with no grammar bundled, and it matches what editors fall back to when
//! no language server offers folding ranges. For brace languages the closing
//! line is pulled into the region so `}` disappears with its block.
//!
//! The same regions drive sticky scroll: the headers of the regions containing
//! the first visible line are exactly the context to pin at the top.

use std::collections::BTreeSet;

/// Languages whose blocks end with a closing bracket on its own line.
const BRACE_CLOSERS: [char; 3] = ['}', ')', ']'];

/// A collapsible block: `start` is the header line that stays visible, and
/// `start + 1 ..= end` is what folding hides. Lines are 0-based.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub start: usize,
    pub end: usize,
}

impl Region {
    pub fn hidden(&self) -> std::ops::RangeInclusive<usize> {
        self.start + 1..=self.end
    }
}

fn indent_of(line: &str) -> Option<usize> {
    if line.trim().is_empty() {
        return None;
    }
    // A tab counts as one level's worth of width; only ordering matters here.
    Some(
        line.chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum(),
    )
}

/// Whether the language's blocks close with a bracket line, which should fold
/// away together with the block it closes.
fn folds_closing_bracket(extension: &str) -> bool {
    !matches!(
        extension,
        "py" | "pyi" | "pyw" | "pyx" | "yaml" | "yml" | "toml" | "md" | "markdown" | "rst"
    )
}

/// Every foldable block in `text`, outermost first, nested regions included.
pub fn regions(text: &str, extension: &str) -> Vec<Region> {
    let lines: Vec<&str> = text.split('\n').collect();
    let indents: Vec<Option<usize>> = lines.iter().map(|l| indent_of(l)).collect();
    let close_too = folds_closing_bracket(extension);
    let mut out = Vec::new();

    for (i, indent) in indents.iter().enumerate() {
        let Some(indent) = *indent else { continue };
        // The block opens only if the next line with content is deeper.
        let Some(next) = (i + 1..lines.len()).find(|&j| indents[j].is_some()) else {
            break;
        };
        if indents[next].unwrap_or(0) <= indent {
            continue;
        }
        // The block runs while lines are blank or deeper than the header.
        let mut end = next;
        for (j, depth) in indents.iter().enumerate().skip(next) {
            match depth {
                None => continue,
                Some(depth) if *depth > indent => end = j,
                Some(_) => break,
            }
        }
        // Pull in a lone closing bracket that belongs to this block.
        if close_too {
            if let Some(after) = (end + 1..lines.len()).find(|&j| indents[j].is_some()) {
                let trimmed = lines[after].trim_start();
                if indents[after] == Some(indent) && trimmed.starts_with(BRACE_CLOSERS) {
                    end = after;
                }
            }
        }
        if end > i {
            out.push(Region { start: i, end });
        }
    }
    out
}

/// The region headed by `line`, if that line opens one.
pub fn region_at(regions: &[Region], line: usize) -> Option<Region> {
    regions.iter().copied().find(|r| r.start == line)
}

/// Lines hidden by the folded headers in `folded`.
///
/// A header never hides itself, but it does disappear when an enclosing block
/// is folded too — which is what makes "fold all" collapse to the outermost
/// headers rather than listing every nested one.
pub fn hidden_lines(regions: &[Region], folded: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut hidden = BTreeSet::new();
    for region in regions {
        if folded.contains(&region.start) {
            hidden.extend(region.hidden());
        }
    }
    hidden
}

/// Headers of the regions enclosing `line`, outermost first — the sticky-scroll
/// context. `line` is 0-based; at most `max` headers are returned, keeping the
/// innermost ones when there are more.
pub fn context(regions: &[Region], line: usize, max: usize) -> Vec<usize> {
    if max == 0 {
        return Vec::new();
    }
    let mut headers: Vec<usize> = regions
        .iter()
        .filter(|r| r.start < line && line <= r.end)
        .map(|r| r.start)
        .collect();
    headers.sort_unstable();
    headers.dedup();
    if headers.len() > max {
        headers.drain(..headers.len() - max);
    }
    headers
}

/// Every header that has a fold, for "fold all".
pub fn all_starts(regions: &[Region]) -> BTreeSet<usize> {
    regions.iter().map(|r| r.start).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PY: &str = "\
def outer():
    a = 1
    if a:
        b = 2
        c = 3
    return a

x = 9";

    const RS: &str = "\
fn main() {
    let a = 1;
    if a > 0 {
        println!(\"hi\");
    }
}

struct S;";

    #[test]
    fn python_blocks_follow_indentation() {
        let r = regions(PY, "py");
        assert!(r.contains(&Region { start: 0, end: 5 }), "def body: {r:?}");
        assert!(r.contains(&Region { start: 2, end: 4 }), "if body: {r:?}");
        // The trailing statement opens nothing.
        assert!(!r.iter().any(|x| x.start == 7));
    }

    #[test]
    fn brace_languages_swallow_the_closing_line() {
        let r = regions(RS, "rs");
        // fn main() { ... } — through the final brace on line 5.
        assert!(r.contains(&Region { start: 0, end: 5 }), "{r:?}");
        // The inner if closes on line 4.
        assert!(r.contains(&Region { start: 2, end: 4 }), "{r:?}");
    }

    #[test]
    fn python_keeps_the_dedented_line_out() {
        let r = regions(PY, "py");
        let outer = r.iter().find(|x| x.start == 0).unwrap();
        // `return a` is the last body line; the blank and `x = 9` stay out.
        assert_eq!(outer.end, 5);
    }

    #[test]
    fn blank_lines_inside_a_block_do_not_end_it() {
        let text = "def f():\n    a = 1\n\n    b = 2\nc = 3";
        let r = regions(text, "py");
        assert_eq!(r[0], Region { start: 0, end: 3 });
    }

    #[test]
    fn folding_hides_the_body_but_never_a_header() {
        let r = regions(PY, "py");
        let folded: BTreeSet<usize> = [0].into_iter().collect();
        let hidden = hidden_lines(&r, &folded);
        assert!(!hidden.contains(&0));
        assert_eq!(hidden, (1..=5).collect());
    }

    #[test]
    fn an_enclosing_fold_hides_nested_headers() {
        let r = regions(PY, "py");
        // Folding both the def and the if inside it: the inner header is part
        // of the outer body, so only the def stays on screen.
        let folded: BTreeSet<usize> = [0, 2].into_iter().collect();
        let hidden = hidden_lines(&r, &folded);
        assert!(!hidden.contains(&0), "the outermost header stays visible");
        assert!(hidden.contains(&2), "a nested header goes with its parent");
        assert_eq!(hidden, (1..=5).collect());
    }

    #[test]
    fn folding_only_the_inner_block_keeps_the_outer_body_visible() {
        let r = regions(PY, "py");
        let folded: BTreeSet<usize> = [2].into_iter().collect();
        let hidden = hidden_lines(&r, &folded);
        assert_eq!(hidden, (3..=4).collect());
    }

    #[test]
    fn context_lists_enclosing_headers_outermost_first() {
        let r = regions(PY, "py");
        assert_eq!(context(&r, 3, 5), vec![0, 2]);
        assert_eq!(context(&r, 1, 5), vec![0]);
        // Nothing encloses the top-level statement.
        assert!(context(&r, 7, 5).is_empty());
        // A header does not enclose itself.
        assert_eq!(context(&r, 0, 5), Vec::<usize>::new());
    }

    #[test]
    fn context_keeps_the_innermost_when_capped() {
        let r = regions(PY, "py");
        assert_eq!(context(&r, 3, 1), vec![2]);
    }

    #[test]
    fn empty_and_flat_files_have_no_regions() {
        assert!(regions("", "rs").is_empty());
        assert!(regions("a\nb\nc", "rs").is_empty());
    }
}

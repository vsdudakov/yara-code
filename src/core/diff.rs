//! Side-by-side diffs: what `git diff` reports, arranged as rows with the old
//! line on the left and the new one on the right.
//!
//! The diffing itself is git's — this only reads a unified diff back and pairs
//! the removals with the additions that replaced them, which is what a two-pane
//! view needs and a unified one does not.

/// What a row says about the pair of lines on it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// Unchanged; the same line stands on both sides.
    Same,
    /// Replaced: an old line on the left, its replacement on the right.
    Changed,
    Added,
    Removed,
}

/// One line of one side, with its number in that version of the file.
#[derive(Clone, Debug, PartialEq)]
pub struct Side {
    pub line: usize,
    pub text: String,
}

/// One row of the view. A side is `None` where that version has nothing —
/// the blank half beside an added or removed line.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: Kind,
    pub left: Option<Side>,
    pub right: Option<Side>,
}

/// Reads a unified diff (any context width) into side-by-side rows. Hunk
/// headers set the line numbers, so a diff with real hunks numbers correctly
/// even though the rows themselves are contiguous.
pub fn from_unified(diff: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let (mut old_line, mut new_line) = (1usize, 1usize);
    // Removals and additions are collected while a run lasts, then paired.
    let mut removed: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();

    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@") {
            flush(
                &mut rows,
                &mut removed,
                &mut added,
                &mut old_line,
                &mut new_line,
            );
            if let Some((old, new)) = hunk_start(header) {
                old_line = old;
                new_line = new;
            }
            continue;
        }
        // Everything before the first hunk is git's file header.
        if rows.is_empty() && removed.is_empty() && added.is_empty() && !started(line) {
            continue;
        }
        match line.chars().next() {
            Some('-') => removed.push(line[1..].to_string()),
            Some('+') => added.push(line[1..].to_string()),
            Some(' ') => {
                flush(
                    &mut rows,
                    &mut removed,
                    &mut added,
                    &mut old_line,
                    &mut new_line,
                );
                rows.push(Row {
                    kind: Kind::Same,
                    left: Some(Side {
                        line: old_line,
                        text: line[1..].to_string(),
                    }),
                    right: Some(Side {
                        line: new_line,
                        text: line[1..].to_string(),
                    }),
                });
                old_line += 1;
                new_line += 1;
            }
            // "\ No newline at end of file" and blank separators.
            _ => {}
        }
    }
    flush(
        &mut rows,
        &mut removed,
        &mut added,
        &mut old_line,
        &mut new_line,
    );
    rows
}

/// Whether a line is diff body rather than the header above the first hunk.
fn started(line: &str) -> bool {
    matches!(line.chars().next(), Some('-') | Some('+') | Some(' '))
        && !line.starts_with("--- ")
        && !line.starts_with("+++ ")
}

/// Pairs a run of removals with the additions that replaced them; whatever is
/// left over on either side becomes a one-sided row.
fn flush(
    rows: &mut Vec<Row>,
    removed: &mut Vec<String>,
    added: &mut Vec<String>,
    old_line: &mut usize,
    new_line: &mut usize,
) {
    let pairs = removed.len().min(added.len());
    for i in 0..pairs {
        rows.push(Row {
            kind: Kind::Changed,
            left: Some(Side {
                line: *old_line + i,
                text: removed[i].clone(),
            }),
            right: Some(Side {
                line: *new_line + i,
                text: added[i].clone(),
            }),
        });
    }
    for (i, text) in removed.iter().enumerate().skip(pairs) {
        rows.push(Row {
            kind: Kind::Removed,
            left: Some(Side {
                line: *old_line + i,
                text: text.clone(),
            }),
            right: None,
        });
    }
    for (i, text) in added.iter().enumerate().skip(pairs) {
        rows.push(Row {
            kind: Kind::Added,
            left: None,
            right: Some(Side {
                line: *new_line + i,
                text: text.clone(),
            }),
        });
    }
    *old_line += removed.len();
    *new_line += added.len();
    removed.clear();
    added.clear();
}

/// `@@ -12,7 +12,9 @@` — the first line number on each side. The trailing
/// `@@` and any section heading after it are not numbers and are skipped.
fn hunk_start(header: &str) -> Option<(usize, usize)> {
    let mut old = None;
    let mut new = None;
    for part in header.split_whitespace() {
        let Some((sign, rest)) = part.split_at_checked(1) else {
            continue;
        };
        if sign != "-" && sign != "+" {
            continue;
        }
        let Ok(number) = rest.split(',').next().unwrap_or_default().parse::<usize>() else {
            continue;
        };
        match sign {
            "-" => old = Some(number.max(1)),
            _ => new = Some(number.max(1)),
        }
    }
    Some((old?, new?))
}

/// A file with no old version — every line is an addition.
pub fn all_added(text: &str) -> Vec<Row> {
    text.lines()
        .enumerate()
        .map(|(i, line)| Row {
            kind: Kind::Added,
            left: None,
            right: Some(Side {
                line: i + 1,
                text: line.to_string(),
            }),
        })
        .collect()
}

/// A file that is gone — every line is a removal.
pub fn all_removed(text: &str) -> Vec<Row> {
    text.lines()
        .enumerate()
        .map(|(i, line)| Row {
            kind: Kind::Removed,
            left: Some(Side {
                line: i + 1,
                text: line.to_string(),
            }),
            right: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "\
diff --git a/f.txt b/f.txt
index 111..222 100644
--- a/f.txt
+++ b/f.txt
@@ -1,3 +1,4 @@
 first
-second
+SECOND
 third
+extra
";

    #[test]
    fn a_replacement_stands_on_one_row() {
        let rows = from_unified(DIFF);
        assert_eq!(rows[0].kind, Kind::Same);
        assert_eq!(rows[1].kind, Kind::Changed);
        assert_eq!(rows[1].left.as_ref().unwrap().text, "second");
        assert_eq!(rows[1].right.as_ref().unwrap().text, "SECOND");
        // Both sides keep their own numbering.
        assert_eq!(rows[1].left.as_ref().unwrap().line, 2);
        assert_eq!(rows[1].right.as_ref().unwrap().line, 2);
    }

    #[test]
    fn an_addition_leaves_the_left_side_blank() {
        let rows = from_unified(DIFF);
        let last = rows.last().unwrap();
        assert_eq!(last.kind, Kind::Added);
        assert!(last.left.is_none());
        assert_eq!(last.right.as_ref().unwrap().text, "extra");
        assert_eq!(last.right.as_ref().unwrap().line, 4);
    }

    #[test]
    fn the_file_header_is_not_mistaken_for_content() {
        let rows = from_unified(DIFF);
        assert!(
            rows.iter().all(|row| {
                let text = row
                    .left
                    .as_ref()
                    .or(row.right.as_ref())
                    .map(|s| s.text.as_str())
                    .unwrap_or("");
                !text.starts_with("-- a/") && !text.starts_with("++ b/")
            }),
            "the `--- a/f.txt` and `+++ b/f.txt` lines are not diff body"
        );
    }

    #[test]
    fn hunk_headers_set_the_line_numbers() {
        let rows = from_unified("@@ -10,2 +20,2 @@\n context\n-old\n+new\n");
        assert_eq!(rows[0].left.as_ref().unwrap().line, 10);
        assert_eq!(rows[0].right.as_ref().unwrap().line, 20);
        assert_eq!(rows[1].left.as_ref().unwrap().line, 11);
        assert_eq!(rows[1].right.as_ref().unwrap().line, 21);
    }

    #[test]
    fn removals_beyond_the_additions_stay_on_the_left() {
        let rows = from_unified("@@ -1,3 +1,1 @@\n-one\n-two\n three\n");
        assert_eq!(rows[0].kind, Kind::Removed);
        assert_eq!(rows[1].kind, Kind::Removed);
        assert!(rows[1].right.is_none());
        assert_eq!(rows[2].kind, Kind::Same);
    }
}

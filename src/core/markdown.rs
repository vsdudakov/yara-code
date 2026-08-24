//! The markdown a README is written in, read into blocks and spans that a
//! frontend can paint.
//!
//! This is the subset people actually write — headings, paragraphs, emphasis,
//! code, lists, quotes, rules and links — not CommonMark's every corner. A
//! preview that gets a README right is the job; a crate that gets tables in
//! nested footnotes right is not worth the dependency.

/// A run of text with one style.
#[derive(Clone, Debug, PartialEq)]
pub enum Span {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    /// Link text and where it goes.
    Link(String, String),
}

/// One block of the document, top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// Level 1–6 and the heading's spans.
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    /// Fenced code, with the language named after the fence if any.
    Code(Option<String>, String),
    /// Ordered when `true`; each item is its own spans.
    List(bool, Vec<Vec<Span>>),
    Quote(Vec<Span>),
    Rule,
}

/// Reads a document into blocks.
pub fn parse(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut lines = text.lines().peekable();
    let mut paragraph: Vec<String> = Vec::new();

    let flush = |paragraph: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if paragraph.is_empty() {
            return;
        }
        let joined = paragraph.join(" ");
        blocks.push(Block::Paragraph(spans(&joined)));
        paragraph.clear();
    };

    while let Some(line) = lines.next() {
        let trimmed = line.trim();

        if let Some(fence) = trimmed.strip_prefix("```") {
            flush(&mut paragraph, &mut blocks);
            let language = fence.trim();
            let language = (!language.is_empty()).then(|| language.to_string());
            let mut body = Vec::new();
            for inner in lines.by_ref() {
                if inner.trim().starts_with("```") {
                    break;
                }
                body.push(inner);
            }
            blocks.push(Block::Code(language, body.join("\n")));
            continue;
        }

        if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
            continue;
        }

        if let Some(rest) = heading(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Heading(rest.0, spans(rest.1)));
            continue;
        }

        if is_rule(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Rule);
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix('>') {
            flush(&mut paragraph, &mut blocks);
            let mut quoted = vec![rest.trim().to_string()];
            while let Some(next) = lines.peek() {
                match next.trim().strip_prefix('>') {
                    Some(more) => {
                        quoted.push(more.trim().to_string());
                        lines.next();
                    }
                    None => break,
                }
            }
            blocks.push(Block::Quote(spans(&quoted.join(" "))));
            continue;
        }

        if let Some((ordered, item)) = list_item(trimmed) {
            flush(&mut paragraph, &mut blocks);
            let mut items = vec![spans(item)];
            while let Some(next) = lines.peek() {
                match list_item(next.trim()) {
                    Some((o, more)) if o == ordered => {
                        items.push(spans(more));
                        lines.next();
                    }
                    _ => break,
                }
            }
            blocks.push(Block::List(ordered, items));
            continue;
        }

        paragraph.push(trimmed.to_string());
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

/// `## Title` → (2, "Title"). A `#` needs a space after it to be a heading.
fn heading(line: &str) -> Option<(u8, &str)> {
    let level = line.bytes().take_while(|b| *b == b'#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = line[level..].strip_prefix(' ')?;
    Some((level as u8, rest.trim_end_matches(['#', ' '])))
}

fn is_rule(line: &str) -> bool {
    line.len() >= 3
        && (line.chars().all(|c| c == '-' || c == ' ')
            || line.chars().all(|c| c == '*' || c == ' ')
            || line.chars().all(|c| c == '_' || c == ' '))
        && line.chars().filter(|c| !c.is_whitespace()).count() >= 3
}

/// `- item`, `* item` or `1. item` → (ordered, "item").
fn list_item(line: &str) -> Option<(bool, &str)> {
    if let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
    {
        return Some((false, rest));
    }
    let digits = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 {
        if let Some(rest) = line[digits..].strip_prefix(". ") {
            return Some((true, rest));
        }
    }
    None
}

/// Reads inline markup: `**bold**`, `*italic*` / `_italic_`, `` `code` `` and
/// `[text](url)`. Anything unclosed is plain text, as a reader would take it.
pub fn spans(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut i = 0;

    // Everything below indexes `chars`, never bytes: a README with an em dash
    // or a Cyrillic word must not throw the markup off.
    let at = |i: usize, pat: &str| -> bool {
        let pat: Vec<char> = pat.chars().collect();
        chars.len() >= i + pat.len() && chars[i..i + pat.len()] == pat[..]
    };
    let find_from = |start: usize, pat: &str| -> Option<usize> {
        let pat: Vec<char> = pat.chars().collect();
        (start..chars.len().saturating_sub(pat.len() - 1))
            .find(|&k| chars[k..k + pat.len()] == pat[..])
    };
    let slice = |a: usize, b: usize| -> String { chars[a..b].iter().collect() };
    let push_plain = |plain: &mut String, out: &mut Vec<Span>| {
        if !plain.is_empty() {
            out.push(Span::Text(std::mem::take(plain)));
        }
    };

    while i < chars.len() {
        if at(i, "`") {
            if let Some(end) = find_from(i + 1, "`") {
                push_plain(&mut plain, &mut out);
                out.push(Span::Code(slice(i + 1, end)));
                i = end + 1;
                continue;
            }
        }
        if at(i, "**") {
            if let Some(end) = find_from(i + 2, "**") {
                push_plain(&mut plain, &mut out);
                out.push(Span::Bold(slice(i + 2, end)));
                i = end + 2;
                continue;
            }
        }
        if (at(i, "*") || at(i, "_")) && !at(i, "* ") {
            let mark = chars[i].to_string();
            if let Some(end) = find_from(i + 1, &mark) {
                if end > i + 1 {
                    push_plain(&mut plain, &mut out);
                    out.push(Span::Italic(slice(i + 1, end)));
                    i = end + 1;
                    continue;
                }
            }
        }
        if at(i, "[") {
            if let Some(close) = find_from(i, "](") {
                if let Some(end) = find_from(close + 2, ")") {
                    push_plain(&mut plain, &mut out);
                    out.push(Span::Link(slice(i + 1, close), slice(close + 2, end)));
                    i = end + 1;
                    continue;
                }
            }
        }
        plain.push(chars[i]);
        i += 1;
    }
    push_plain(&mut plain, &mut out);
    out
}

/// The text of a run of spans with the markup stripped — what the terminal
/// paints when it has no way to show a style.
pub fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|span| match span {
            Span::Text(t) | Span::Bold(t) | Span::Italic(t) | Span::Code(t) => t.as_str(),
            Span::Link(t, _) => t.as_str(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_paragraphs_and_rules_come_apart() {
        let doc = "# Title\n\nSome *text* here.\nStill here.\n\n---\n\n## Sub";
        let blocks = parse(doc);
        assert_eq!(
            blocks[0],
            Block::Heading(1, vec![Span::Text("Title".into())])
        );
        // Two lines with no blank between them are one paragraph.
        assert!(
            matches!(&blocks[1], Block::Paragraph(s) if plain(s) == "Some text here. Still here.")
        );
        assert_eq!(blocks[2], Block::Rule);
        assert!(matches!(&blocks[3], Block::Heading(2, _)));
    }

    #[test]
    fn inline_markup_is_read_and_the_rest_left_alone() {
        let s = spans("a **bold** and *em* and `code` and [link](http://x) done");
        assert_eq!(s[1], Span::Bold("bold".into()));
        assert_eq!(s[3], Span::Italic("em".into()));
        assert_eq!(s[5], Span::Code("code".into()));
        assert_eq!(s[7], Span::Link("link".into(), "http://x".into()));
        assert_eq!(plain(&s), "a bold and em and code and link done");
        // Unclosed markup is just text.
        assert_eq!(spans("a * b"), vec![Span::Text("a * b".into())]);
        assert_eq!(spans("**open"), vec![Span::Text("**open".into())]);
    }

    #[test]
    fn fenced_code_keeps_its_body_verbatim() {
        let doc = "```rust\nfn main() {}\n    // **not bold**\n```\nafter";
        let blocks = parse(doc);
        assert_eq!(
            blocks[0],
            Block::Code(
                Some("rust".into()),
                "fn main() {}\n    // **not bold**".into()
            )
        );
        assert!(matches!(&blocks[1], Block::Paragraph(_)));
        // A fence with no language, and one that is never closed.
        assert!(matches!(&parse("```\nx\n```")[0], Block::Code(None, b) if b == "x"));
        assert!(matches!(&parse("```\nx")[0], Block::Code(None, b) if b == "x"));
    }

    #[test]
    fn lists_group_their_items_and_quotes_their_lines() {
        let doc = "- one\n- two\n\n1. first\n2. second\n\n> quoted\n> more";
        let blocks = parse(doc);
        assert!(matches!(&blocks[0], Block::List(false, items) if items.len() == 2));
        assert!(matches!(&blocks[1], Block::List(true, items) if items.len() == 2));
        assert!(matches!(&blocks[2], Block::Quote(s) if plain(s) == "quoted more"));
    }

    #[test]
    fn a_hash_without_a_space_is_not_a_heading() {
        assert!(matches!(&parse("#hashtag")[0], Block::Paragraph(_)));
        assert!(matches!(&parse("####### seven")[0], Block::Paragraph(_)));
        // Trailing hashes are decoration, not content.
        assert_eq!(
            parse("## Title ##")[0],
            Block::Heading(2, vec![Span::Text("Title".into())])
        );
    }

    #[test]
    fn non_ascii_text_does_not_throw_the_markup_off() {
        let s = spans("Привет — **жирный** и `код` — done");
        assert_eq!(s[1], Span::Bold("жирный".into()));
        assert_eq!(s[3], Span::Code("код".into()));
        assert_eq!(plain(&s), "Привет — жирный и код — done");
    }

    #[test]
    fn an_empty_document_is_no_blocks() {
        assert!(parse("").is_empty());
        assert!(parse("\n\n  \n").is_empty());
    }
}

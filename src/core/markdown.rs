//! The markdown a README is written in, read into blocks and spans that a
//! frontend can paint.
//!
//! This is the subset people actually write — headings, paragraphs, emphasis,
//! code, lists (nested, numbered, ticked), tables, quotes, rules, links and
//! the mermaid charts of [`crate::core::chart`] — not CommonMark's every
//! corner. A preview that gets a README right is the job; a crate that gets
//! tables in nested footnotes right is not worth the dependency.

use crate::core::chart::{self, Chart};

/// A run of text with one style.
#[derive(Clone, Debug, PartialEq)]
pub enum Span {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    /// Link text and where it goes.
    Link(String, String),
    /// An image's alt text and its source. Nothing here fetches or decodes a
    /// picture, so what a frontend can show of one is what it was described
    /// as — which is the whole of what a badge says anyway.
    Image(String, String),
}

/// What stands in front of a list item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Marker {
    Bullet,
    /// The number as it was written, so a list starting at 4 starts at 4.
    Number(u64),
    /// `- [ ]` and `- [x]`: a task, done or not.
    Task(bool),
}

/// One item of a list, at the depth its indent put it.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub depth: usize,
    pub marker: Marker,
    pub spans: Vec<Span>,
}

/// Which way a table column reads, from the dashes under its heading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub head: Vec<Vec<Span>>,
    /// One per column, the same length as `head`.
    pub align: Vec<Align>,
    pub rows: Vec<Vec<Vec<Span>>>,
}

impl Table {
    pub fn columns(&self) -> usize {
        self.head.len()
    }
}

/// One block of the document, top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// Level 1–6 and the heading's spans.
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    /// Fenced code, with the language named after the fence if any.
    Code(Option<String>, String),
    /// A run of items; each carries its own depth and marker, so one block
    /// holds a whole nested list.
    List(Vec<Item>),
    Table(Table),
    /// A mermaid fence this editor knows how to draw.
    Chart(Chart),
    Quote(Vec<Span>),
    Rule,
    /// A block written inside `<div align="center">`, which is how a README
    /// centres its title and its badges. One block, not the group: the
    /// terminal scrolls the preview a block at a time, and a group would make
    /// a whole header one step.
    Center(Box<Block>),
}

/// Reads a document into blocks.
pub fn parse(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut i = 0;

    let flush = |paragraph: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if paragraph.is_empty() {
            return;
        }
        let joined = paragraph.join(" ");
        blocks.push(Block::Paragraph(spans(&joined)));
        paragraph.clear();
    };

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if let Some(fence) = trimmed.strip_prefix("```") {
            flush(&mut paragraph, &mut blocks);
            let language = fence.trim();
            let language = (!language.is_empty()).then(|| language.to_string());
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            let body = body.join("\n");
            // A mermaid fence is a picture where it is one this editor draws,
            // and the code it was written as where it is not.
            let drawn = language
                .as_deref()
                .filter(|l| l.eq_ignore_ascii_case("mermaid"))
                .and_then(|_| chart::parse(&body));
            blocks.push(match drawn {
                Some(chart) => Block::Chart(chart),
                None => Block::Code(language, body),
            });
            continue;
        }

        if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
            i += 1;
            continue;
        }

        // A line that is nothing but raw HTML — the `<div align="center">` a
        // README opens with, and the `</div>` that closes it — is markup for a
        // browser to obey, not text to paint. The one thing it is obeyed for
        // is centring: what such a tag holds is read on its own and comes back
        // centred.
        if is_html_only(trimmed) {
            flush(&mut paragraph, &mut blocks);
            // A badge row is written as raw HTML as often as it is written as
            // markdown, and a line of tags that names a picture or a link is
            // not markup to drop: it is the row.
            if !centers(trimmed) {
                let inline = spans(trimmed);
                if !inline.is_empty() {
                    blocks.push(Block::Paragraph(inline));
                }
                i += 1;
                continue;
            }
            if centers(trimmed) {
                let (inner, read) = take_until_close(&lines, i);
                blocks.extend(parse(&inner).into_iter().map(|block| match block {
                    // A centring tag inside another asks for nothing the
                    // outer one did not. Wrapped twice it would be unwrapped
                    // once and the wrapper painted, which is nothing at all.
                    centred @ Block::Center(_) => centred,
                    block => Block::Center(Box::new(block)),
                }));
                i = read;
                continue;
            }
            i += 1;
            continue;
        }

        // A tag that centres and closes on the line it opened centres that
        // one line: `<h1 align="center">Yara Code</h1>` and the tagline under
        // it are how a README writes a header without a block around it.
        if centers(trimmed) && nesting(trimmed) <= 0 {
            let (level, inner) = match html_heading(trimmed) {
                Some((level, inner)) => (Some(level), spans(inner)),
                None => (None, spans(trimmed)),
            };
            if !inner.is_empty() {
                flush(&mut paragraph, &mut blocks);
                blocks.push(Block::Center(Box::new(match level {
                    Some(level) => Block::Heading(level, inner),
                    None => Block::Paragraph(inner),
                })));
                i += 1;
                continue;
            }
        }

        // A heading written as HTML — `<h1>Yara Code</h1>`, the title every
        // centred README header is built from — is a heading, not the line of
        // body text that dropping its tags would leave.
        if let Some((level, inner)) = html_heading(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Heading(level, spans(inner)));
            i += 1;
            continue;
        }

        if let Some((level, rest)) = heading(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Heading(level, spans(rest)));
            i += 1;
            continue;
        }

        if is_rule(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Rule);
            i += 1;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix('>') {
            flush(&mut paragraph, &mut blocks);
            let mut quoted = vec![rest.trim().to_string()];
            i += 1;
            while let Some(more) = lines.get(i).and_then(|l| l.trim().strip_prefix('>')) {
                quoted.push(more.trim().to_string());
                i += 1;
            }
            blocks.push(Block::Quote(spans(&quoted.join(" "))));
            continue;
        }

        // A row of cells is a table only when the row under it draws the
        // column rules; without that it is the paragraph it looks like.
        if trimmed.contains('|')
            && lines
                .get(i + 1)
                .is_some_and(|next| is_column_rule(next.trim()))
        {
            flush(&mut paragraph, &mut blocks);
            let (table, read) = take_table(&lines, i);
            blocks.push(Block::Table(table));
            i = read;
            continue;
        }

        if list_item(trimmed).is_some() {
            flush(&mut paragraph, &mut blocks);
            let (items, read) = take_list(&lines, i);
            blocks.push(Block::List(items));
            i = read;
            continue;
        }

        paragraph.push(trimmed.to_string());
        i += 1;
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

/// How far into the line the text starts, counting a tab as four.
fn indent_of(line: &str) -> usize {
    line.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// `- item`, `* item`, `1. item`, `- [x] item` → the marker and the text.
fn list_item(line: &str) -> Option<(Marker, &str)> {
    let bulleted = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
        .or_else(|| matches!(line, "-" | "*" | "+").then_some(""));
    if let Some(rest) = bulleted {
        return Some(match ticked(rest) {
            Some((done, rest)) => (Marker::Task(done), rest),
            None => (Marker::Bullet, rest),
        });
    }
    let digits = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let rest = line[digits..]
        .strip_prefix(". ")
        .or_else(|| line[digits..].strip_prefix(") "))?;
    let number = line[..digits].parse().unwrap_or(1);
    Some(match ticked(rest) {
        // A numbered task is still a task: the box is what the reader acts on.
        Some((done, rest)) => (Marker::Task(done), rest),
        None => (Marker::Number(number), rest),
    })
}

fn ticked(rest: &str) -> Option<(bool, &str)> {
    let done = match rest.get(..4) {
        Some("[ ] ") => false,
        Some("[x] ") | Some("[X] ") => true,
        _ => return None,
    };
    Some((done, &rest[4..]))
}

/// A run of list items, however deeply they nest. Depth comes from the column
/// each item starts at rather than from a fixed number of spaces, so a list
/// written with two-space indents nests as one written with four does.
fn take_list(lines: &[&str], start: usize) -> (Vec<Item>, usize) {
    let mut read: Vec<(usize, Marker, String)> = Vec::new();
    let mut columns: Vec<usize> = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // A blank line between items of the same kind is a loose list and
            // goes on. A blank line and then a different marker is where one
            // list ends and the next begins.
            let next = lines.get(i + 1).map(|next| list_item(next.trim()));
            match (next, read.last()) {
                (Some(Some((marker, _))), Some((_, last, _))) if same_kind(*last, marker) => {
                    i += 1;
                }
                _ => break,
            }
            continue;
        }
        if is_rule(trimmed) || heading(trimmed).is_some() {
            break;
        }
        let column = indent_of(line);
        if let Some((marker, text)) = list_item(trimmed) {
            read.push((depth_of(column, &mut columns), marker, text.to_string()));
            i += 1;
            continue;
        }
        // A line under an item and indented past its bullet carries on that
        // item's text, the way a wrapped sentence in a README does.
        match read.last_mut() {
            Some((_, _, text)) if column > 0 => {
                text.push(' ');
                text.push_str(trimmed);
                i += 1;
            }
            _ => break,
        }
    }
    let items = read
        .into_iter()
        .map(|(depth, marker, text)| Item {
            depth,
            marker,
            spans: spans(&text),
        })
        .collect();
    (items, i)
}

/// Whether two markers belong to the same list — a numbered list and a
/// bulleted one are two lists even where only a blank line divides them.
fn same_kind(a: Marker, b: Marker) -> bool {
    matches!(a, Marker::Number(_)) == matches!(b, Marker::Number(_))
}

/// The nesting level of an item starting at `column`, given the columns the
/// levels above it start at.
fn depth_of(column: usize, columns: &mut Vec<usize>) -> usize {
    while columns.last().is_some_and(|last| column < *last) {
        columns.pop();
    }
    match columns.last() {
        Some(last) if column > *last => columns.push(column),
        None => columns.push(column),
        _ => {}
    }
    columns.len() - 1
}

/// `|---|:--:|` — the row that turns the row above it into a table's heading.
fn is_column_rule(line: &str) -> bool {
    line.contains('|')
        && line.contains('-')
        && line.chars().all(|c| matches!(c, '-' | ':' | '|' | ' '))
}

fn cells(line: &str) -> Vec<&str> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(str::trim).collect()
}

/// A table: its heading, the alignments under it, and every row of cells that
/// follows until the table runs out.
fn take_table(lines: &[&str], start: usize) -> (Table, usize) {
    let head: Vec<Vec<Span>> = cells(lines[start]).into_iter().map(spans).collect();
    let align: Vec<Align> = cells(lines[start + 1])
        .into_iter()
        .map(|cell| match (cell.starts_with(':'), cell.ends_with(':')) {
            (true, true) => Align::Center,
            (false, true) => Align::Right,
            _ => Align::Left,
        })
        .collect();
    let mut table = Table {
        align: (0..head.len())
            .map(|c| align.get(c).copied().unwrap_or(Align::Left))
            .collect(),
        head,
        rows: Vec::new(),
    };
    let mut i = start + 2;
    while let Some(line) = lines.get(i) {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains('|') {
            break;
        }
        // Short rows are filled out and long ones cut, so every row has as
        // many cells as the heading promised.
        let mut row: Vec<Vec<Span>> = cells(trimmed).into_iter().map(spans).collect();
        row.resize(table.columns(), Vec::new());
        table.rows.push(row);
        i += 1;
    }
    (table, i)
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
                out.extend(emphasised(&slice(i + 2, end), Span::Bold));
                i = end + 2;
                continue;
            }
        }
        if (at(i, "*") || at(i, "_")) && !at(i, "* ") {
            let mark = chars[i].to_string();
            if let Some(end) = find_from(i + 1, &mark) {
                if end > i + 1 {
                    push_plain(&mut plain, &mut out);
                    out.extend(emphasised(&slice(i + 1, end), Span::Italic));
                    i = end + 1;
                    continue;
                }
            }
        }
        if at(i, "![") {
            if let Some((alt, src, next)) = bracketed(&chars, i + 1) {
                push_plain(&mut plain, &mut out);
                out.push(Span::Image(alt, src));
                i = next;
                continue;
            }
        }
        if at(i, "[") {
            if let Some((text, dest, next)) = bracketed(&chars, i) {
                push_plain(&mut plain, &mut out);
                // A link written round an image is how every badge in a README
                // is written; it reads as the image's alt text, so the row of
                // them comes out as CI, Release, Docs rather than as their
                // markup.
                out.push(Span::Link(self::plain(&spans(&text)), dest));
                i = next;
                continue;
            }
        }
        // A tag that says something a preview can paint — a picture, a link,
        // a run of emphasis — says it. Every other tag in the middle of a line
        // — `<br>`, `<sub>` — goes the way a whole line of them does, and a
        // lone `<` that opens no tag stays: it is a less-than sign.
        if at(i, "<") {
            // A link written as itself, which is the only way to write one
            // that has no name of its own.
            if let Some((link, next)) = autolink(&chars, i) {
                push_plain(&mut plain, &mut out);
                out.push(link);
                i = next;
                continue;
            }
            if let Some((painted, next)) = tagged(&chars, i) {
                push_plain(&mut plain, &mut out);
                out.extend(painted);
                i = next;
                continue;
            }
            if let Some(end) = tag_end(&chars, i) {
                i = end;
                continue;
            }
        }
        plain.push(chars[i]);
        i += 1;
    }
    push_plain(&mut plain, &mut out);
    out
}

/// `<https://example.com>` or `<hi@example.com>` — a link standing as its own
/// name — and where reading it ended. Only what a browser would follow counts:
/// a scheme it knows, or an address, so the `<String>` of a type is left where
/// it stands.
fn autolink(chars: &[char], start: usize) -> Option<(Span, usize)> {
    if chars.get(start) != Some(&'<') {
        return None;
    }
    let end = chars[start + 1..]
        .iter()
        .take_while(|c| !c.is_whitespace() && **c != '<')
        .position(|c| *c == '>')
        .map(|k| start + 1 + k)?;
    let text: String = chars[start + 1..end].iter().collect();
    let scheme = text
        .split_once("://")
        .filter(|(scheme, rest)| {
            !scheme.is_empty()
                && !rest.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        })
        .is_some();
    // An address is shown as itself and followed as mail, however it was
    // written.
    let address = text.strip_prefix("mailto:").unwrap_or(&text);
    let mailable = address
        .split_once('@')
        .is_some_and(|(name, host)| !name.is_empty() && host.contains('.') && !host.contains('@'));
    let link = if scheme {
        Span::Link(text.clone(), text)
    } else if mailable {
        Span::Link(address.to_string(), format!("mailto:{address}"))
    } else {
        return None;
    };
    Some((link, end + 1))
}

/// What a tag standing in a line of prose says, where it says anything a
/// preview can paint, and where reading it ended. A picture is what it was
/// described as, a link is where it goes, and `<b>`, `<em>` and `<code>` are
/// the runs markdown writes with punctuation. Everything else comes back
/// `None` and is dropped as the markup for a browser it is.
fn tagged(chars: &[char], start: usize) -> Option<(Vec<Span>, usize)> {
    let end = tag_end(chars, start)?;
    let tag: String = chars[start..end].iter().collect();
    let name = tag_name(&tag)?;
    if name == "img" {
        // A picture with nothing said about it is named after its file, which
        // is more than the icon alone would say.
        let src = attr(&tag, "src").unwrap_or_default();
        let alt = attr(&tag, "alt")
            .filter(|alt| !alt.is_empty())
            .unwrap_or_else(|| src.rsplit('/').next().unwrap_or_default().to_string());
        return Some((vec![Span::Image(alt, src)], end));
    }
    let (inner, next) = held(chars, end, &name)?;
    let painted = match name.as_str() {
        "b" | "strong" => emphasised(&inner, Span::Bold),
        "i" | "em" => emphasised(&inner, Span::Italic),
        "code" | "kbd" | "samp" => vec![Span::Code(plain(&spans(&inner)))],
        // A link written round a picture reads as the picture's name, which is
        // how a badge says which badge it is — the same reading a markdown
        // badge is given.
        "a" => vec![Span::Link(
            plain(&spans(&inner)),
            attr(&tag, "href").unwrap_or_default(),
        )],
        _ => return None,
    };
    Some((painted, next))
}

/// The text an element holds, and the index after the tag that closes it.
fn held(chars: &[char], from: usize, name: &str) -> Option<(String, usize)> {
    let rest: String = chars[from..].iter().collect();
    let close = format!("</{name}>");
    let at = rest.to_ascii_lowercase().find(&close)?;
    let inner = &rest[..at];
    let next = from + inner.chars().count() + close.chars().count();
    Some((inner.to_string(), next))
}

/// The name a tag opens with, in the one case names are compared in.
fn tag_name(tag: &str) -> Option<String> {
    let name: String = tag
        .trim_start_matches(['<', '/'])
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

/// What a tag says an attribute is, however the value was quoted. The name is
/// matched only where a space stands in front of it, so `data-src` is not read
/// as `src`.
fn attr(tag: &str, name: &str) -> Option<String> {
    // Only ASCII changes case, so the offsets found in the one hold in the
    // other however the alt text is written.
    let lower = tag.to_ascii_lowercase();
    let key = format!("{name}=");
    let mut from = 1;
    while let Some(at) = lower.get(from..)?.find(&key) {
        let at = from + at;
        let spaced = lower[..at].chars().last().is_some_and(char::is_whitespace);
        from = at + key.len();
        if !spaced {
            continue;
        }
        let value = &tag[from..];
        return Some(
            match value.chars().next() {
                Some(quote @ ('"' | '\'')) => value[1..].split(quote).next().unwrap_or_default(),
                _ => value
                    .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
                    .next()
                    .unwrap_or_default(),
            }
            .to_string(),
        );
    }
    None
}

/// The level and the text of `<h1>…</h1>` written on one line, if that is
/// what the line is. Only a heading closed on its own line counts: one left
/// open holds whatever follows, which is more than a title.
fn html_heading(line: &str) -> Option<(u8, &str)> {
    let lower = line.to_ascii_lowercase();
    let level = (1..=6u8).find(|n| {
        lower
            .strip_prefix(&format!("<h{n}"))
            .is_some_and(|rest| rest.starts_with('>') || rest.starts_with(char::is_whitespace))
    })?;
    let opened = line.find('>')?;
    let closed = lower.rfind(&format!("</h{level}>"))?;
    (closed > opened).then(|| (level, line[opened + 1..closed].trim()))
}

/// Whether a tag asks for what it holds to be centred.
fn centers(line: &str) -> bool {
    let line = line.replace(['"', '\''], "");
    line.contains("align=center") || line.contains("text-align: center")
}

/// The lines a centring tag holds, and the line to go on from. The tags in
/// between are counted, so a `<div>` inside the one that centres does not end
/// it early; a tag never closed holds the rest of the document, which is what
/// a browser does with it too.
fn take_until_close(lines: &[&str], open: usize) -> (String, usize) {
    // A tag closed on the line it opened holds what stands between the two —
    // the picture a header is built round, as often as not.
    let mut depth = nesting(lines[open]);
    if depth <= 0 {
        let line = lines[open];
        let held = match (line.find('>'), line.rfind("</")) {
            (Some(opened), Some(closed)) if closed > opened => &line[opened + 1..closed],
            _ => "",
        };
        return (held.to_string(), open + 1);
    }
    for (k, line) in lines.iter().enumerate().skip(open + 1) {
        let trimmed = line.trim();
        if !is_html_only(trimmed) {
            continue;
        }
        depth += nesting(trimmed);
        if depth <= 0 {
            return (lines[open + 1..k].join("\n"), k + 1);
        }
    }
    (lines[open + 1..].join("\n"), lines.len())
}

/// How far a line of raw HTML opens or closes, tag by tag. A tag that holds
/// nothing opens nothing: `<img src="logo.png">` is written without the slash
/// a browser does not ask for, and counted as a level it left the `</div>`
/// under it closing the picture instead of the block — which centred the rest
/// of the README.
fn nesting(line: &str) -> isize {
    let chars: Vec<char> = line.chars().collect();
    let mut depth = 0;
    let mut i = 0;
    while i < chars.len() {
        let Some(end) = tag_end(&chars, i) else {
            i += 1;
            continue;
        };
        let tag: String = chars[i..end].iter().collect();
        depth += if tag.starts_with("</") {
            -1
        } else if tag.starts_with("<!") || tag.ends_with("/>") || holds_nothing(&tag) {
            0
        } else {
            1
        };
        i = end;
    }
    depth
}

/// Whether a tag is one of those that hold nothing at all.
fn holds_nothing(tag: &str) -> bool {
    const VOID: &[&str] = &[
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ];
    tag_name(tag).is_some_and(|name| VOID.contains(&name.as_str()))
}

/// What stands inside `**…**` or `*…*`, read again so a link or a snippet of
/// code inside emphasis is not left as its own markup. Only plain text takes
/// the emphasis on: a link inside bold is still a link, which is as much as
/// one run of spans can say.
fn emphasised(inner: &str, mark: fn(String) -> Span) -> Vec<Span> {
    spans(inner)
        .into_iter()
        .map(|span| match span {
            Span::Text(t) => mark(t),
            other => other,
        })
        .collect()
}

/// `[text](dest)` starting at the `[`, and where reading it ended. Brackets
/// inside the text are counted rather than the first `](` taken, because a
/// link wrapped round an image — the shape every badge in a README is written
/// in — would otherwise be cut at the image's own closing bracket.
fn bracketed(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    let close = matching(chars, start, '[', ']')?;
    let open = close + 1;
    if chars.get(open) != Some(&'(') {
        return None;
    }
    let end = matching(chars, open, '(', ')')?;
    let text: String = chars[start + 1..close].iter().collect();
    let dest: String = chars[open + 1..end].iter().collect();
    Some((text, dest, end + 1))
}

/// Where the `open` at `start` is closed, counting the pairs in between.
fn matching(chars: &[char], start: usize, open: char, close: char) -> Option<usize> {
    if chars.get(start) != Some(&open) {
        return None;
    }
    let mut depth = 0usize;
    for (k, c) in chars.iter().enumerate().skip(start) {
        if *c == open {
            depth += 1;
        } else if *c == close {
            depth -= 1;
            if depth == 0 {
                return Some(k);
            }
        }
    }
    None
}

/// Where the tag opening at `start` ends, if what stands there is one: a `<`,
/// a name HTML actually has, and a `>` reached without another `<` on the way.
/// The name is checked against a list rather than taken for anything that
/// reads like one, because `<https://example.com>` is a link a reader wrote
/// and the `<String>` in `Vec<String>` is half of a type — dropping either as
/// markup loses a word of the README.
fn tag_end(chars: &[char], start: usize) -> Option<usize> {
    if chars.get(start) != Some(&'<') {
        return None;
    }
    // A comment or a doctype is markup whatever stands inside it.
    if chars.get(start + 1) == Some(&'!') {
        return chars[start + 1..]
            .iter()
            .position(|c| *c == '>')
            .map(|k| start + k + 2);
    }
    let mut i = start + 1;
    if chars.get(i) == Some(&'/') {
        i += 1;
    }
    let name: String = chars[i..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if !names_a_tag(&name) {
        return None;
    }
    i += name.chars().count();
    // What follows the name closes the tag or opens an attribute; a name run
    // straight into anything else was never a tag.
    match chars.get(i) {
        Some('>' | '/') => {}
        Some(c) if c.is_whitespace() => {}
        _ => return None,
    }
    chars[i..]
        .iter()
        .take_while(|c| **c != '<')
        .position(|c| *c == '>')
        .map(|k| i + k + 1)
}

/// Whether a name is HTML's. Tag names are written in one case throughout, so
/// the camel case of a type — `Vec<Data>` — names no tag however much
/// `data` is one.
fn names_a_tag(name: &str) -> bool {
    const TAGS: &[&str] = &[
        "a",
        "abbr",
        "address",
        "animate",
        "article",
        "aside",
        "audio",
        "b",
        "base",
        "bdi",
        "bdo",
        "big",
        "blockquote",
        "br",
        "button",
        "canvas",
        "caption",
        "center",
        "circle",
        "cite",
        "clippath",
        "code",
        "col",
        "colgroup",
        "dd",
        "defs",
        "del",
        "desc",
        "details",
        "dfn",
        "div",
        "dl",
        "dt",
        "ellipse",
        "em",
        "embed",
        "figcaption",
        "figure",
        "font",
        "footer",
        "form",
        "g",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "header",
        "hr",
        "html",
        "i",
        "iframe",
        "img",
        "input",
        "ins",
        "kbd",
        "label",
        "legend",
        "li",
        "line",
        "lineargradient",
        "link",
        "main",
        "mark",
        "marquee",
        "mask",
        "meta",
        "nav",
        "nobr",
        "noscript",
        "ol",
        "optgroup",
        "option",
        "p",
        "param",
        "path",
        "picture",
        "polygon",
        "polyline",
        "pre",
        "q",
        "rect",
        "s",
        "samp",
        "script",
        "section",
        "select",
        "small",
        "source",
        "span",
        "stop",
        "strike",
        "strong",
        "style",
        "sub",
        "summary",
        "sup",
        "svg",
        "symbol",
        "table",
        "tbody",
        "td",
        "text",
        "textarea",
        "tfoot",
        "th",
        "thead",
        "title",
        "tr",
        "tspan",
        "tt",
        "u",
        "ul",
        "use",
        "var",
        "video",
        "wbr",
    ];
    let one_case = !name.chars().any(|c| c.is_ascii_uppercase())
        || !name.chars().any(|c| c.is_ascii_lowercase());
    one_case && TAGS.iter().any(|tag| tag.eq_ignore_ascii_case(name))
}

/// Whether a line is raw HTML and nothing else, tag after tag.
fn is_html_only(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut tags = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        match tag_end(&chars, i) {
            Some(end) => {
                tags += 1;
                i = end;
            }
            None => return false,
        }
    }
    tags > 0
}

/// The text of a run of spans with the markup stripped — what the terminal
/// paints when it has no way to show a style.
pub fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|span| match span {
            Span::Text(t) | Span::Bold(t) | Span::Italic(t) | Span::Code(t) => t.as_str(),
            Span::Link(t, _) | Span::Image(t, _) => t.as_str(),
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
        assert!(matches!(&blocks[0], Block::List(items) if items.len() == 2));
        assert!(matches!(&blocks[1], Block::List(items) if items[1].marker == Marker::Number(2)));
        assert!(matches!(&blocks[2], Block::Quote(s) if plain(s) == "quoted more"));
    }

    #[test]
    fn a_nested_list_keeps_the_depth_its_indent_gave_it() {
        let doc = "- one\n  - under\n    - deeper\n- two";
        let Block::List(items) = &parse(doc)[0] else {
            panic!("that is a list");
        };
        let depths: Vec<usize> = items.iter().map(|i| i.depth).collect();
        assert_eq!(depths, vec![0, 1, 2, 0]);
        // Four-space indents nest the same way two-space ones do.
        let doc = "1. one\n    1. under\n2. two";
        let Block::List(items) = &parse(doc)[0] else {
            panic!("that is a list");
        };
        assert_eq!(
            items.iter().map(|i| i.depth).collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
    }

    #[test]
    fn a_ticked_item_is_a_task_and_keeps_only_its_text() {
        let doc = "- [x] done\n- [ ] not yet\n- plain";
        let Block::List(items) = &parse(doc)[0] else {
            panic!("that is a list");
        };
        assert_eq!(items[0].marker, Marker::Task(true));
        assert_eq!(plain(&items[0].spans), "done");
        assert_eq!(items[1].marker, Marker::Task(false));
        assert_eq!(items[2].marker, Marker::Bullet);
    }

    #[test]
    fn a_wrapped_item_stays_one_item() {
        let doc = "- one that runs\n  on to the next line\n- two";
        let Block::List(items) = &parse(doc)[0] else {
            panic!("that is a list");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(plain(&items[0].spans), "one that runs on to the next line");
    }

    #[test]
    fn a_table_reads_its_heading_alignments_and_rows() {
        let doc = "| Name | Size |\n|:-----|-----:|\n| one  | 1    |\n| two  | 22   |\n\nafter";
        let blocks = parse(doc);
        let Block::Table(table) = &blocks[0] else {
            panic!("that is a table");
        };
        assert_eq!(table.columns(), 2);
        assert_eq!(plain(&table.head[1]), "Size");
        assert_eq!(table.align, vec![Align::Left, Align::Right]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(plain(&table.rows[1][1]), "22");
        assert!(matches!(&blocks[1], Block::Paragraph(_)));
    }

    #[test]
    fn a_short_row_is_filled_out_to_the_heading() {
        let doc = "a | b | c\n--|---|--\n1 | 2";
        let Block::Table(table) = &parse(doc)[0] else {
            panic!("that is a table");
        };
        assert_eq!(table.rows[0].len(), 3);
        assert!(table.rows[0][2].is_empty());
        // A line of cells with no rule under it is only a paragraph.
        assert!(matches!(&parse("a | b\nc | d")[0], Block::Paragraph(_)));
    }

    #[test]
    fn a_mermaid_fence_is_a_chart_and_anything_else_is_code() {
        let doc = "```mermaid\nflowchart LR\n A[One] --> B[Two]\n```";
        assert!(matches!(&parse(doc)[0], Block::Chart(_)));
        let doc = "```mermaid\nsequenceDiagram\n A->>B: hi\n```";
        assert!(matches!(&parse(doc)[0], Block::Code(Some(l), _) if l == "mermaid"));
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
    fn a_badge_reads_as_the_name_it_was_given() {
        // The shape a README's badges are written in: a link wrapped round an
        // image, which used to be cut at the image's own closing bracket.
        let s = spans("[![CI](https://x/badge.svg)](https://x/ci), then ![shot](a.gif)");
        assert_eq!(s[0], Span::Link("CI".into(), "https://x/ci".into()));
        assert_eq!(s[1], Span::Text(", then ".into()));
        assert_eq!(s[2], Span::Image("shot".into(), "a.gif".into()));
    }

    #[test]
    fn raw_html_is_markup_for_a_browser_and_not_text_to_paint() {
        let doc = "<section>\n\n# Title\n\nOne<br>two\n\n</section>\n";
        let blocks = parse(doc);
        assert_eq!(
            blocks,
            vec![
                Block::Heading(1, vec![Span::Text("Title".into())]),
                Block::Paragraph(vec![Span::Text("Onetwo".into())]),
            ]
        );
        // A lone `<` opens no tag, so it stays the less-than sign it is.
        assert_eq!(spans("a < b"), vec![Span::Text("a < b".into())]);
    }

    #[test]
    fn a_centring_tag_is_the_one_html_asks_for_that_is_obeyed() {
        let doc = "<div align=\"center\">\n\n# Title\n\n</div>\n\nAfter.\n";
        let blocks = parse(doc);
        assert_eq!(
            blocks,
            vec![
                Block::Center(Box::new(Block::Heading(
                    1,
                    vec![Span::Text("Title".into())]
                ))),
                Block::Paragraph(vec![Span::Text("After.".into())]),
            ]
        );
        // A plain wrapper centres nothing.
        assert_eq!(
            parse("<div>\n\n# Title\n\n</div>"),
            vec![Block::Heading(1, vec![Span::Text("Title".into())])]
        );
    }

    #[test]
    fn a_picture_inside_the_centring_tag_does_not_swallow_the_readme() {
        // The shape every README opens with: a logo written without the
        // closing slash, and a heading that stands outside the block.
        let doc = "<div align=\"center\">\n<img src=\"logo.png\" width=\"120\">\n</div>\n\n# Title\n\nBody.\n";
        assert_eq!(
            parse(doc),
            vec![
                // The logo is named after its file, having been given no name
                // of its own, and what follows the block is not centred.
                Block::Center(Box::new(Block::Paragraph(vec![Span::Image(
                    "logo.png".into(),
                    "logo.png".into()
                )]))),
                Block::Heading(1, vec![Span::Text("Title".into())]),
                Block::Paragraph(vec![Span::Text("Body.".into())]),
            ]
        );
        // The same written on one line closes itself just as surely.
        let doc = "<div align=\"center\"><img src=\"logo.png\" alt=\"Logo\"></div>\n\n# Title\n";
        assert_eq!(
            parse(doc),
            vec![
                Block::Center(Box::new(Block::Paragraph(vec![Span::Image(
                    "Logo".into(),
                    "logo.png".into()
                )]))),
                Block::Heading(1, vec![Span::Text("Title".into())]),
            ]
        );
    }

    #[test]
    fn a_heading_written_as_html_is_a_heading() {
        let doc = "<div align=\"center\">\n<h1>Yara Code</h1>\n<p>Tagline.</p>\n</div>\n";
        assert_eq!(
            parse(doc),
            vec![
                Block::Center(Box::new(Block::Heading(
                    1,
                    vec![Span::Text("Yara Code".into())]
                ))),
                Block::Center(Box::new(Block::Paragraph(vec![Span::Text(
                    "Tagline.".into()
                )]))),
            ]
        );
        // The markup inside one is read as markup, and a level is kept.
        assert_eq!(
            parse("<h3>A <b>tagged</b> `bit`</h3>"),
            vec![Block::Heading(
                3,
                vec![
                    Span::Text("A ".into()),
                    Span::Bold("tagged".into()),
                    Span::Text(" ".into()),
                    Span::Code("bit".into())
                ]
            )]
        );
        // The same header written without a block around it.
        let doc = "<h1 align=\"center\">Yara Code</h1>\n<p align=\"center\">A tagline.</p>\n";
        assert_eq!(
            parse(doc),
            vec![
                Block::Center(Box::new(Block::Heading(
                    1,
                    vec![Span::Text("Yara Code".into())]
                ))),
                Block::Center(Box::new(Block::Paragraph(vec![Span::Text(
                    "A tagline.".into()
                )]))),
            ]
        );
        // A line that only starts like one is not a heading.
        assert!(matches!(&parse("<h1>Open")[0], Block::Paragraph(_)));
        assert!(matches!(&parse("<html>x</html>")[0], Block::Paragraph(_)));
    }

    #[test]
    fn a_centring_tag_inside_another_is_centred_once() {
        let doc = "<div align=\"center\">\n<div align=\"center\">\n\nText\n\n</div>\n</div>\n";
        assert_eq!(
            parse(doc),
            vec![Block::Center(Box::new(Block::Paragraph(vec![Span::Text(
                "Text".into()
            )])))]
        );
    }

    #[test]
    fn html_that_names_a_picture_or_a_link_is_painted_as_one() {
        // The badge row half a README is written with: a link round a picture,
        // on a line with nothing else on it.
        let doc = "<p align=\"center\">\n  <a href=\"https://x/ci\"><img src=\"https://x/b.svg\" alt=\"CI\"></a>\n</p>\n";
        assert_eq!(
            parse(doc),
            vec![Block::Center(Box::new(Block::Paragraph(vec![Span::Link(
                "CI".into(),
                "https://x/ci".into()
            )])))]
        );
        // A picture standing in a line of prose, and one written the long way.
        assert_eq!(
            spans("Before <img src='a/shot.png' alt=\"A shot\"/> after"),
            vec![
                Span::Text("Before ".into()),
                Span::Image("A shot".into(), "a/shot.png".into()),
                Span::Text(" after".into()),
            ]
        );
        // An attribute is not read out of the middle of another's name.
        assert_eq!(
            spans("<img data-src=\"no.png\" src=\"yes.png\" alt=\"Yes\">"),
            vec![Span::Image("Yes".into(), "yes.png".into())]
        );
    }

    #[test]
    fn emphasis_written_as_html_is_emphasis() {
        assert_eq!(
            spans("A <strong>bold</strong> and <em>italic</em> and <code>code</code> line"),
            vec![
                Span::Text("A ".into()),
                Span::Bold("bold".into()),
                Span::Text(" and ".into()),
                Span::Italic("italic".into()),
                Span::Text(" and ".into()),
                Span::Code("code".into()),
                Span::Text(" line".into()),
            ]
        );
        // A tag left open is markup that says nothing, and goes as it always
        // did.
        assert_eq!(
            spans("A <b>bold line"),
            vec![Span::Text("A bold line".into())]
        );
    }

    #[test]
    fn what_only_looks_like_a_tag_is_left_where_it_stands() {
        // An autolink is the link it names, not markup for a browser.
        assert_eq!(
            spans("See <https://example.com> for docs."),
            vec![
                Span::Text("See ".into()),
                Span::Link("https://example.com".into(), "https://example.com".into()),
                Span::Text(" for docs.".into()),
            ]
        );
        assert_eq!(
            parse("<https://example.com>\n"),
            vec![Block::Paragraph(vec![Span::Link(
                "https://example.com".into(),
                "https://example.com".into()
            )])]
        );
        // An address is followed as mail, written plainly or with the scheme.
        let mail = Span::Link("hi@example.com".into(), "mailto:hi@example.com".into());
        assert_eq!(spans("<hi@example.com>"), vec![mail.clone()]);
        assert_eq!(spans("<mailto:hi@example.com>"), vec![mail]);
        // Neither a bare word in brackets nor a type is a link.
        assert_eq!(
            spans("<not a link>"),
            vec![Span::Text("<not a link>".into())]
        );
        assert_eq!(spans("<nothing>"), vec![Span::Text("<nothing>".into())]);
        // A type is not a paragraph with a `<data>` in it.
        assert_eq!(
            plain(&spans("A Vec<String> holds items.")),
            "A Vec<String> holds items."
        );
        assert_eq!(
            plain(&spans("Option<Data> is fine.")),
            "Option<Data> is fine."
        );
        // A tag HTML does have still goes.
        assert_eq!(plain(&spans("One<br>two")), "Onetwo");
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

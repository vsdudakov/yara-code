//! Painting the frame: a header, the FILES sidebar when it is open, the
//! AGENT and FOLLOW panes side by side with one blank column between them,
//! a status bar, and whichever overlay is up. Every colour is the theme's;
//! every width and chord is the settings'.

use std::collections::BTreeSet;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;
use yara_core::command::Command;
use yara_core::follow::{EditEvent, LineKind, Tick};
use yara_core::settings::Side;
use yara_core::theme::{ansi256, Theme, Ui};

use crate::app::{App, Focus, Overlay, View};
use crate::theme::{base, bold, color, fg, on};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(base(&app.theme)), area);
    let [header, body, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(frame, app, header);

    let sidebar = Constraint::Length(if app.show_sidebar {
        app.settings.sidebar_width
    } else {
        0
    });
    let agent = Constraint::Percentage(app.settings.agent_width.min(100));
    let (first, second) = match app.settings.agent_side {
        Side::Left => (agent, Constraint::Min(0)),
        Side::Right => (Constraint::Min(0), agent),
    };
    let [files, first, _gap, second] =
        Layout::horizontal([sidebar, first, Constraint::Length(1), second]).areas(body);
    let (agent, follow) = match app.settings.agent_side {
        Side::Left => (first, second),
        Side::Right => (second, first),
    };
    if app.show_sidebar {
        draw_files(frame, app, files);
    }
    draw_agent(frame, app, agent);
    draw_follow(frame, app, follow);
    draw_status(frame, app, status);
    match &app.overlay {
        Some(Overlay::Changes(row)) => draw_changes(frame, app, *row, area),
        Some(Overlay::NewTab(text)) => {
            draw_prompt(frame, app, " NEW AGENT ", "branch / task name", text, area)
        }
        Some(Overlay::RenameTab(text)) => {
            draw_prompt(frame, app, " RENAME TAB ", "name", text, area)
        }
        None => {}
    }
}

/// A one-line question in a box: what is being asked, and what has been
/// typed so far with a block cursor after it.
fn draw_prompt(frame: &mut Frame, app: &App, title: &str, what: &str, text: &str, area: Rect) {
    let ui = &app.theme.ui;
    frame.render_widget(Block::new().style(fg(ui.fg_dim)), area);
    let width = area.width.min(60);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + area.height.saturating_sub(3) / 2,
        width,
        3.min(area.height),
    );
    let block = Block::bordered()
        .border_style(fg(ui.accent))
        .title(Line::styled(title, bold(ui.accent)))
        .style(base(&app.theme));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    let line = if text.is_empty() {
        Line::from(vec![
            Span::styled("█", fg(ui.cursor)),
            Span::styled(
                format!(" {what} · ⏎ ok · {} cancel", app.hint(Command::Close)),
                fg(ui.fg_dim),
            ),
        ])
    } else {
        Line::from(vec![
            Span::raw(text.to_string()),
            Span::styled("█", fg(ui.cursor)),
        ])
    };
    frame.render_widget(Paragraph::new(line), inner);
}

fn draw_header(frame: &mut Frame, app: &mut App, area: Rect) {
    let ui = app.theme.ui.clone();
    let mut left = vec![
        Span::styled(" YARA ", bold(ui.accent)),
        Span::raw(" File  Help "),
    ];
    left.push(Span::styled(" │ ", fg(ui.fg_dim)));
    left.push(Span::styled(
        app.project
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        fg(ui.fg_dim),
    ));
    if let Some(name) = app.repo.as_ref().and_then(|r| r.worktree.as_deref()) {
        left.push(Span::styled(" │ ", fg(ui.fg_dim)));
        left.push(Span::styled(format!("⌥ worktree: {name}"), fg(ui.success)));
    }
    // The tabs: one agent in one worktree each, and the way to another.
    left.push(Span::styled(" │ ", fg(ui.fg_dim)));
    for (i, session) in app.sessions.iter().enumerate() {
        let title = format!(" {} ", session.title());
        left.push(if i == app.active {
            Span::styled(title, on(ui.bg, ui.accent).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(title, fg(ui.fg_dim))
        });
        left.push(Span::raw(" "));
    }
    left.push(Span::styled("[+]", fg(ui.success)));
    let right = if app.agent_running() {
        Line::styled(format!("● {} — running ", app.agent_name()), fg(ui.success))
    } else {
        Line::styled(format!("○ {} — exited ", app.agent_name()), fg(ui.fg_dim))
    };
    shorten(
        &mut left,
        3,
        (area.width as usize).saturating_sub(right.width() + 1),
    );
    frame.render_widget(Paragraph::new(Line::from(left)), area);
    frame.render_widget(Paragraph::new(right.right_aligned()), area);
}

fn draw_files(frame: &mut Frame, app: &App, area: Rect) {
    let ui = &app.theme.ui;
    let block = Block::bordered()
        .border_style(fg(ui.border))
        .title(" FILES ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let footer = format!(
        "{} hide · {} open",
        app.hint(Command::ToggleSidebar),
        app.hint(Command::MarkReviewed)
    );
    if inner.height > 0 {
        let last = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
        frame.render_widget(Paragraph::new(Line::styled(footer, fg(ui.fg_dim))), last);
    }
}

fn draw_agent(frame: &mut Frame, app: &mut App, area: Rect) {
    let ui = app.theme.ui.clone();
    let focused = app.focus == Focus::Agent;
    let block = Block::bordered()
        .border_style(fg(if focused { ui.fg_dim } else { ui.border }))
        .title(format!(" AGENT · {} ", app.agent_name()));
    let grid = block.inner(area);
    frame.render_widget(block, area);
    let theme = app.theme.clone();
    let Some(pty) = app.agent.as_mut() else {
        return;
    };
    pty.resize(grid.height, grid.width);

    // Paint the screen cell by cell, merging runs that share a style.
    let (lines, cursor) = pty.with_screen(|screen| {
        let lines: Vec<Line> = (0..grid.height)
            .map(|row| {
                let mut spans: Vec<Span> = Vec::new();
                for col in 0..grid.width {
                    let (text, style) = match screen.cell(row, col) {
                        Some(cell) if !cell.contents().is_empty() => {
                            (cell.contents(), cell_style(cell, &ui, &theme))
                        }
                        Some(cell) => (" ".to_string(), cell_style(cell, &ui, &theme)),
                        None => (" ".to_string(), on(ui.fg, ui.bg)),
                    };
                    match spans.last_mut() {
                        Some(last) if last.style == style => last.content.to_mut().push_str(&text),
                        _ => spans.push(Span::styled(text, style)),
                    }
                }
                Line::from(spans)
            })
            .collect();
        let cursor =
            (!screen.hide_cursor() && screen.scrollback() == 0).then(|| screen.cursor_position());
        (lines, cursor)
    });
    frame.render_widget(Paragraph::new(lines), grid);
    if let (true, Some((row, col))) = (focused, cursor) {
        if row < grid.height && col < grid.width {
            frame.set_cursor_position((grid.x + col, grid.y + row));
        }
    }
}

/// One cell's colours and attributes as a ratatui style.
fn cell_style(cell: &vt100::Cell, ui: &Ui, theme: &Theme) -> Style {
    let resolve = |c: vt100::Color, default| match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => ansi256(theme, i),
        vt100::Color::Rgb(r, g, b) => (r, g, b),
    };
    let (mut fg_c, mut bg_c) = (
        resolve(cell.fgcolor(), ui.fg),
        resolve(cell.bgcolor(), ui.bg),
    );
    if cell.inverse() {
        std::mem::swap(&mut fg_c, &mut bg_c);
    }
    let mut style = on(fg_c, bg_c);
    for (set, modifier) in [
        (cell.bold(), Modifier::BOLD),
        (cell.italic(), Modifier::ITALIC),
        (cell.underline(), Modifier::UNDERLINED),
    ] {
        if set {
            style = style.add_modifier(modifier);
        }
    }
    style
}

fn draw_follow(frame: &mut Frame, app: &App, area: Rect) {
    let ui = &app.theme.ui;
    let dim = fg(ui.fg_dim);
    let focused = app.focus == Focus::Follow;
    let idle = fg(if focused { ui.fg_dim } else { ui.border });
    let block = if app.pinned.is_some() {
        Block::bordered()
            .border_style(idle)
            .title(Line::styled(" FOLLOW · CHANGES ", bold(ui.fg)))
            .title_top(
                Line::styled(
                    format!(" [ {} → timeline ] ", app.hint(Command::Close)),
                    fg(ui.accent),
                )
                .right_aligned(),
            )
    } else if app.follow.is_live() {
        Block::bordered()
            .border_style(fg(ui.accent))
            .title(Line::styled(" FOLLOW · LIVE ", bold(ui.accent)))
    } else {
        Block::bordered()
            .border_style(idle)
            .title(Line::styled(" FOLLOW · PAUSED ", bold(ui.fg)))
            .title_top(
                Line::styled(
                    format!(
                        " [ {} → live ] ",
                        app.hint(Command::FollowLive).to_lowercase()
                    ),
                    fg(ui.accent),
                )
                .right_aligned(),
            )
    };
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }
    let [file_row, timeline_row, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);

    let Some(edit) = app.shown() else {
        frame.render_widget(
            Paragraph::new(Line::styled("waiting for the agent's first edit", dim)),
            file_row,
        );
        frame.render_widget(Paragraph::new(Line::styled("edits —", dim)), timeline_row);
        return;
    };

    // File row: path, its counts, the view toggle on the right.
    let mut file = vec![
        Span::styled(edit.path.display().to_string(), bold(ui.fg)),
        Span::styled(format!(" +{}", edit.added()), fg(ui.success)),
        Span::styled(format!(" −{}", edit.removed()), fg(ui.accent)),
    ];
    if app.pinned.is_none() && app.follow.is_reviewed(app.follow.cursor()) {
        file.push(Span::styled(" [✓ reviewed]", fg(ui.success)));
    }
    frame.render_widget(Paragraph::new(Line::from(file)), file_row);
    let toggle = format!(
        "[ {}: {} ]",
        app.hint(Command::ToggleView).to_lowercase(),
        match app.view {
            View::Diff => "diff",
            View::File => "file",
        }
    );
    frame.render_widget(
        Paragraph::new(Line::styled(toggle, dim).right_aligned()),
        file_row,
    );

    // Timeline row: the tick strip, the counter, then the hints — which are
    // the first thing the row gives up when it runs out of room.
    let ticks = app.follow.ticks();
    let window = app.follow.window(app.settings.timeline_ticks.max(1));
    let mut strip = vec![Span::styled("edits ", dim)];
    if window.hidden_before > 0 {
        strip.push(Span::styled("‥", dim));
    }
    for tick in &ticks[window.start..window.end] {
        strip.push(match tick {
            Tick::Current => Span::styled("◉", fg(ui.accent)),
            Tick::Unreviewed => Span::styled("●", fg(ui.accent_dim)),
            Tick::Reviewed => Span::styled("○", dim),
        });
    }
    if window.hidden_after > 0 {
        strip.push(Span::styled("‥", dim));
    }
    strip.push(Span::styled(
        format!(" [{}/{}]", app.follow.cursor() + 1, app.follow.len()),
        dim,
    ));
    let strip_width: usize = strip.iter().map(Span::width).sum();
    frame.render_widget(Paragraph::new(Line::from(strip)), timeline_row);
    let hints = format!(
        "{} {} scrub · {} live · {} mark reviewed",
        app.hint(Command::ScrubBack),
        app.hint(Command::ScrubForward),
        app.hint(Command::FollowLive).to_lowercase(),
        app.hint(Command::MarkReviewed)
    );
    if strip_width + 2 + hints.chars().count() <= timeline_row.width as usize {
        frame.render_widget(
            Paragraph::new(Line::styled(hints, dim).right_aligned()),
            timeline_row,
        );
    }

    if app.view == View::File {
        draw_file(frame, app, edit, body);
        return;
    }
    // Body: the unified diff, one hunk after another.
    let mut lines = Vec::new();
    for hunk in &edit.hunks {
        let (mut old, mut new) = (hunk.old_start, hunk.new_start);
        for line in &hunk.lines {
            let (number, sign, style, sign_style) = match line.kind {
                LineKind::Context => {
                    old += 1;
                    new += 1;
                    (new - 1, " ", Style::new(), Style::new())
                }
                LineKind::Added => {
                    new += 1;
                    let row = Style::new().bg(color(ui.success_bg));
                    (new - 1, "+", row, row.fg(color(ui.success_dim)))
                }
                LineKind::Removed => {
                    old += 1;
                    let row = Style::new().bg(color(ui.accent_bg));
                    (old - 1, "−", row, row.fg(color(ui.accent_dim)))
                }
            };
            lines.push(Line::from(vec![
                Span::styled(" ", style),
                Span::styled(format!("{number:>5} "), style.patch(dim)),
                Span::styled(sign, sign_style),
                Span::styled(format!(" {}", line.text), style),
            ]));
        }
    }
    frame.render_widget(Paragraph::new(lines), body);
}

/// The file as it stands, with an accent bar beside every line the edit
/// added. A file that is gone reads as empty.
fn draw_file(frame: &mut Frame, app: &App, edit: &EditEvent, area: Rect) {
    let ui = &app.theme.ui;
    let dim = fg(ui.fg_dim);
    let root = app
        .repo
        .as_ref()
        .map(|r| r.root.clone())
        .unwrap_or_default();
    let text = std::fs::read_to_string(root.join(&edit.path)).unwrap_or_default();
    let mut added = BTreeSet::new();
    for hunk in &edit.hunks {
        let mut new = hunk.new_start;
        for line in &hunk.lines {
            match line.kind {
                LineKind::Removed => {}
                LineKind::Added => {
                    added.insert(new);
                    new += 1;
                }
                LineKind::Context => new += 1,
            }
        }
    }
    let lines: Vec<Line> = text
        .lines()
        .enumerate()
        .map(|(i, line)| {
            let number = i + 1;
            let bar = if added.contains(&number) {
                Span::styled("▎", fg(ui.accent))
            } else {
                Span::raw(" ")
            };
            Line::from(vec![
                bar,
                Span::styled(format!("{number:>5}  "), dim),
                Span::raw(line.to_string()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// The CHANGES overlay: what differs from the base branch, one row a file.
fn draw_changes(frame: &mut Frame, app: &App, row: usize, area: Rect) {
    let ui = &app.theme.ui;
    let dim = fg(ui.fg_dim);
    frame.render_widget(Block::new().style(dim), area);
    let width = area.width.min(72);
    let height = (app.changes.len() as u16 + 4).clamp(4, area.height.max(4));
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let block = Block::bordered()
        .border_style(fg(ui.accent))
        .title(Line::styled(" CHANGES ", bold(ui.accent)))
        .style(base(&app.theme));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    let (added, removed) = totals(app);
    let branch = app
        .repo
        .as_ref()
        .map_or("no repository", |r| r.branch.as_str());
    let footer = format!(
        "git status vs main · {} files · +{added} −{removed} · {} open diff · {} close",
        app.changes.len(),
        app.hint(Command::MarkReviewed),
        app.hint(Command::Close)
    );
    let path_width = (inner.width as usize).saturating_sub(16);
    let mut lines: Vec<Line> = app
        .changes
        .iter()
        .enumerate()
        .map(|(i, change)| {
            let letter = match change.letter {
                'M' | 'D' => Span::styled(format!(" {} ", change.letter), fg(ui.accent)),
                other => Span::styled(format!(" {other} "), fg(ui.success)),
            };
            let mut line = Line::from(vec![
                letter,
                Span::raw(format!("{:<path_width$}", change.path)),
                Span::styled(format!("+{:<4}", change.added), fg(ui.success)),
                Span::styled(format!("−{}", change.removed), fg(ui.accent)),
            ]);
            if i == row {
                line = line.style(Style::new().bg(color(ui.selected_bg)));
            }
            line
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::styled(
            format!(" nothing differs from main on {branch}"),
            dim,
        ));
    }
    let [list, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    let scroll = row.saturating_sub(list.height.saturating_sub(1) as usize) as u16;
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), list);
    frame.render_widget(Paragraph::new(Line::styled(footer, dim)), foot);
}

/// Lines added and removed against the base, over every changed file.
fn totals(app: &App) -> (usize, usize) {
    app.changes
        .iter()
        .fold((0, 0), |(a, r), c| (a + c.added, r + c.removed))
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let ui = &app.theme.ui;
    let dim = fg(ui.fg_dim);
    let (added, removed) = totals(app);
    let review = match app.follow.unreviewed_count() {
        0 if app.follow.is_empty() => Span::styled("no edits yet", dim),
        0 => Span::styled("✓ all reviewed", fg(ui.success)),
        n => Span::styled(format!("◆ {n} unreviewed"), fg(ui.accent)),
    };
    // The note is news; the hints give way to it until the next key.
    let right = match &app.note {
        Some(note) => Line::styled(format!(" {note} "), on(ui.fg, ui.selected_bg)),
        None => {
            let hints: Vec<String> = [
                (Command::NextPane, "pane"),
                (Command::Changes, "changes"),
                (Command::ToggleSidebar, "files"),
                (Command::CommandPalette, "palette"),
                (Command::SearchProject, "search"),
                (Command::Help, "keys"),
            ]
            .into_iter()
            .filter_map(|(command, word)| {
                Some(format!("{} {word}", app.settings.chord(command)?.glyphs()))
            })
            .collect();
            Line::styled(format!("{} ", hints.join("  ")), dim)
        }
    };

    let branch = app.repo.as_ref().map_or("—", |r| r.branch.as_str());
    let root = app
        .repo
        .as_ref()
        .map(|r| r.root.display().to_string())
        .unwrap_or_default();
    let mut left = vec![
        Span::styled(format!(" ⎇ {branch} "), fg(ui.fg)),
        Span::styled(root.clone(), dim),
        Span::styled(format!("  +{added}"), fg(ui.success)),
        Span::styled(format!(" −{removed}"), fg(ui.accent)),
        Span::raw("  "),
        review,
    ];
    // The path keeps a dozen columns whatever the hints want.
    let others: usize = left.iter().map(Span::width).sum::<usize>() - left[1].width();
    let room = (area.width as usize)
        .saturating_sub(right.width() + 1)
        .max(others + 12)
        .min(area.width as usize);
    shorten(&mut left, 1, room);
    let width: usize = left.iter().map(Span::width).sum();
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Length(width as u16), Constraint::Min(0)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(left)), left_area);
    // The hints are the least of it: they are cut before anything else is.
    frame.render_widget(Paragraph::new(right.right_aligned()), right_area);
}

/// Fits a row into `room` by cutting the one span that can be cut — a path,
/// at `index` — from its head, so the folder's own name is what survives.
fn shorten(spans: &mut [Span], index: usize, room: usize) {
    let others: usize = spans.iter().map(Span::width).sum::<usize>() - spans[index].width();
    let path = spans[index].content.to_string();
    let allowed = room.saturating_sub(others);
    if path.chars().count() <= allowed {
        return;
    }
    let tail: String = path
        .chars()
        .rev()
        .take(allowed.saturating_sub(1))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let shown = if allowed > 1 {
        format!("…{tail}")
    } else {
        String::new()
    };
    spans[index] = Span::styled(shown, spans[index].style);
}

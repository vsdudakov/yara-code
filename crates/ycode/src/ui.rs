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

use crate::app::{App, Focus, Hits, Overlay, View, MENUS};
use crate::theme::{base, bold, color, fg, on};

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.hits = Hits::default();
    let area = frame.area();
    frame.render_widget(Block::new().style(base(&app.theme)), area);
    let [header, body, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(frame, app, header);
    if app.project.is_none() {
        draw_start(frame, app, body);
        draw_status(frame, app, status);
        draw_overlay(frame, app, area);
        return;
    }

    // The tree sits at the edge away from the agent: on the right when the
    // agent is on the left, on the left otherwise, so the two panes that
    // talk to each other stay side by side.
    let sidebar = Constraint::Length(if app.show_sidebar {
        app.settings.sidebar_width
    } else {
        0
    });
    let agent = Constraint::Percentage(app.settings.agent_width.min(100));
    let gap = Constraint::Length(1);
    // A blank column between the tree and the pane beside it as well, so
    // the tree has the same seam to drag as the agent does.
    let tree_gap = Constraint::Length(u16::from(app.show_sidebar));
    let (files, agent, follow, seam, tree_seam) = match app.settings.agent_side {
        Side::Left => {
            let [agent, seam, follow, tree_seam, files] =
                Layout::horizontal([agent, gap, Constraint::Min(0), tree_gap, sidebar]).areas(body);
            (files, agent, follow, seam, tree_seam)
        }
        Side::Right => {
            let [files, tree_seam, follow, seam, agent] =
                Layout::horizontal([sidebar, tree_gap, Constraint::Min(0), gap, agent]).areas(body);
            (files, agent, follow, seam, tree_seam)
        }
    };
    app.hits.tree_seam = tree_seam;
    app.hits.seam = seam;
    app.hits.body = body;
    app.hits.files = files;
    app.hits.agent = agent;
    app.hits.follow = follow;
    if app.show_sidebar {
        draw_files(frame, app, files);
    }
    draw_agent(frame, app, agent);
    draw_follow(frame, app, follow);
    draw_status(frame, app, status);
    draw_overlay(frame, app, area);
    draw_hover(frame, app);
    draw_selection(frame, app);
}

/// What the mouse rests on, lit: a row, a tab, a button, a tick — and a
/// seam, which shows itself as a line so it reads as something to drag.
fn draw_hover(frame: &mut Frame, app: &mut App) {
    let Some((x, y)) = app.hover else { return };
    let hits = &app.hits;
    let inside = |r: Rect| x >= r.x && x < r.right() && y >= r.y && y < r.bottom();
    let seam = [hits.seam, hits.tree_seam].into_iter().find(|r| inside(*r));
    let buffer = frame.buffer_mut();
    if let Some(seam) = seam {
        for row in seam.y..seam.bottom() {
            let cell = &mut buffer[(seam.x, row)];
            cell.set_symbol("┃");
            cell.set_fg(color(app.theme.ui.accent));
        }
        return;
    }
    let rows = hits
        .file_rows
        .iter()
        .chain(&hits.rows)
        .chain(&hits.tabs)
        .chain(&hits.menus)
        .chain(&hits.ticks)
        .map(|(r, _)| *r)
        .chain([hits.plus, hits.usage, hits.live, hits.counter]);
    let Some(rect) = rows.filter(|r| r.width > 0).find(|r| inside(*r)) else {
        return;
    };
    // An overlay's rows are its own; the chrome under it is not lit through it.
    if app.overlay.is_some() && !inside(hits.overlay) {
        return;
    }
    let bg = color(app.theme.ui.hover_bg);
    for cx in rect.x..rect.right().min(buffer.area.width) {
        for cy in rect.y..rect.bottom().min(buffer.area.height) {
            buffer[(cx, cy)].set_bg(bg);
        }
    }
}

/// The cells the mouse dragged over, lit — and the frame's text kept, so a
/// copy can read them back.
fn draw_selection(frame: &mut Frame, app: &mut App) {
    let pane = app.selection_bounds();
    let buffer = frame.buffer_mut();
    let area = buffer.area;
    if let (Some(((x0, y0), (x1, y1))), Some(pane)) = (app.selection, pane) {
        if (x0, y0) != (x1, y1) {
            let (top, bottom) = (y0.min(y1), y0.max(y1).min(pane.bottom().saturating_sub(1)));
            let (left, right) = if y0 == y1 {
                (x0.min(x1), x0.max(x1))
            } else {
                (pane.x, pane.right().saturating_sub(1))
            };
            let bg = color(app.theme.ui.selected_bg);
            for y in top..=bottom.min(area.height.saturating_sub(1)) {
                for x in left.max(pane.x)..=right.min(pane.right().saturating_sub(1)) {
                    buffer[(x, y)].set_bg(bg);
                }
            }
        }
    }
    app.last_frame = (0..area.height)
        .map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect();
}

fn draw_overlay(frame: &mut Frame, app: &mut App, area: Rect) {
    match app.overlay.clone() {
        Some(Overlay::Usage) => draw_usage(frame, app, area),
        Some(Overlay::Themes(row)) => draw_themes(frame, app, row, area),
        Some(Overlay::CloseFile { .. }) => draw_close_file(frame, app, area),
        Some(Overlay::Changes(row)) => draw_changes(frame, app, row, area),
        Some(Overlay::NewTab(text)) => {
            draw_prompt(frame, app, " NEW WORKSPACE ", "workspace name", &text, area)
        }
        Some(Overlay::RenameTab(text)) => {
            draw_prompt(frame, app, " RENAME WORKSPACE ", "name", &text, area)
        }
        Some(Overlay::NewFile(text)) => {
            draw_prompt(frame, app, " NEW FILE ", "file name", &text, area)
        }
        Some(Overlay::OpenFolder(text)) => {
            draw_prompt(frame, app, " OPEN FOLDER ", "path", &text, area)
        }
        Some(Overlay::QuickOpen(query, row)) => draw_quick_open(frame, app, &query, row, area),
        Some(Overlay::Palette(query, row)) => draw_palette(frame, app, &query, row, area),
        Some(Overlay::Search(query, row, hits, files)) => {
            draw_search(frame, app, &query, row, &hits, files, area)
        }
        Some(Overlay::Keys(scroll)) => draw_keys(frame, app, scroll, area),
        Some(Overlay::Menu(menu, row)) => draw_menu(frame, app, menu, row, area),
        Some(Overlay::Recent(row)) => draw_recent(frame, app, row, area),
        None => {}
    }
}

/// A box in the middle of the screen over a dimmed frame, `height` rows
/// tall inside; what every overlay is drawn in.
fn overlay_box(
    frame: &mut Frame,
    app: &mut App,
    title: &str,
    width: u16,
    height: u16,
    area: Rect,
) -> Rect {
    let ui = app.theme.ui.clone();
    frame.render_widget(Block::new().style(fg(ui.fg_dim)), area);
    let width = width.min(area.width);
    let height = (height + 2).min(area.height.max(2));
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + area.height.saturating_sub(height) / 3,
        width,
        height,
    );
    let block = Block::bordered()
        .border_style(fg(ui.accent))
        .title(Line::styled(format!(" {title} "), bold(ui.accent)))
        .style(base(&app.theme));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    app.hits.overlay = rect;
    inner
}

/// Remembers where a list's rows were drawn, from `first` on, for the mouse.
fn row_hits(app: &mut App, area: Rect, first: usize, count: usize) {
    app.hits.rows = (0..count.min(area.height as usize))
        .map(|i| {
            (
                Rect::new(area.x, area.y + i as u16, area.width, 1),
                first + i,
            )
        })
        .collect();
}

/// A query line with a block cursor after it.
fn query_line<'a>(
    app: &mut App,
    prompt: &'a str,
    query: &'a str,
    placeholder: &'a str,
) -> Line<'a> {
    let ui = app.theme.ui.clone();
    let mut spans = vec![Span::styled(prompt, fg(ui.fg_dim)), Span::raw(query)];
    spans.push(Span::styled("█", fg(ui.cursor)));
    if query.is_empty() {
        spans.push(Span::styled(placeholder, fg(ui.fg_dim)));
    }
    Line::from(spans)
}

/// The rows of a list with the cursor's row lit, scrolled so it shows.
fn list_lines<'a>(app: &mut App, rows: Vec<Line<'a>>, row: usize, height: usize) -> Vec<Line<'a>> {
    let start = row.saturating_sub(height.saturating_sub(1));
    rows.into_iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(i, line)| {
            if i == row {
                line.style(Style::new().bg(color(app.theme.ui.selected_bg)))
            } else {
                line
            }
        })
        .collect()
}

fn draw_palette(frame: &mut Frame, app: &mut App, query: &str, row: usize, area: Rect) {
    let ui = app.theme.ui.clone();
    let hits = app.palette_hits(query);
    let inner = overlay_box(
        frame,
        app,
        "COMMAND PALETTE",
        64,
        hits.len().min(14) as u16 + 1,
        area,
    );
    let width = inner.width as usize;
    let rows: Vec<Line> = hits
        .iter()
        .map(|command| {
            let chord = app.hint(*command);
            let label = command.label();
            let pad = width.saturating_sub(label.chars().count() + chord.chars().count() + 2);
            Line::from(vec![
                Span::raw(format!(" {label}{}", " ".repeat(pad))),
                Span::styled(chord, fg(ui.fg_dim)),
            ])
        })
        .collect();
    let mut lines = vec![query_line(app, "> ", query, "type a command…")];
    lines.extend(list_lines(
        app,
        rows,
        row.min(hits.len().saturating_sub(1)),
        inner.height as usize - 1,
    ));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_search(
    frame: &mut Frame,
    app: &mut App,
    query: &str,
    row: usize,
    hits: &[yara_core::search::Hit],
    files: usize,
    area: Rect,
) {
    let ui = app.theme.ui.clone();
    let inner = overlay_box(
        frame,
        app,
        "SEARCH PROJECT",
        80,
        hits.len().min(14) as u16 + 2,
        area,
    );
    let rows: Vec<Line> = hits
        .iter()
        .map(|hit| {
            Line::from(vec![
                Span::styled(format!(" {}:{}  ", hit.path, hit.line), fg(ui.fg_dim)),
                Span::raw(hit.text.clone()),
            ])
        })
        .collect();
    let mut lines = vec![query_line(app, "> ", query, "search…")];
    lines.extend(list_lines(app, rows, row, inner.height as usize - 2));
    let footer = format!(
        "{} matches in {} files · exclude: {}",
        hits.len(),
        files,
        app.settings.search_exclude.join(", ")
    );
    let [list, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    frame.render_widget(Paragraph::new(lines), list);
    frame.render_widget(Paragraph::new(Line::styled(footer, fg(ui.fg_dim))), foot);
}

fn draw_keys(frame: &mut Frame, app: &mut App, scroll: usize, area: Rect) {
    let ui = app.theme.ui.clone();
    let all = yara_core::command::ALL;
    let inner = overlay_box(frame, app, "KEY BINDINGS", 60, all.len() as u16, area);
    let width = inner.width as usize;
    let lines: Vec<Line> = all
        .iter()
        .skip(scroll)
        .take(inner.height as usize)
        .map(|command| {
            let chord = app
                .settings
                .chord(*command)
                .map(|c| c.to_string())
                .unwrap_or_else(|| "—".into());
            let label = command.label();
            let dots = width.saturating_sub(label.chars().count() + chord.chars().count() + 3);
            Line::from(vec![
                Span::raw(format!(" {label} ")),
                Span::styled("·".repeat(dots), fg(ui.border)),
                Span::styled(format!(" {chord}"), fg(ui.fg_dim)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// A menu dropped down from its word in the header.
fn draw_menu(frame: &mut Frame, app: &mut App, menu: usize, row: usize, area: Rect) {
    let ui = app.theme.ui.clone();
    let (name, entries) = MENUS[menu];
    // Where the word sits in the header: after " YARA  ", each word two
    // spaces apart.
    let x = area.x
        + 7
        + MENUS
            .iter()
            .take(menu)
            .map(|(n, _)| n.len() as u16 + 2)
            .sum::<u16>();
    let width = 36u16.min(area.width.saturating_sub(x));
    let rect = Rect::new(
        x,
        area.y + 1,
        width,
        (entries.len() as u16 + 2).min(area.height - 1),
    );
    let block = Block::bordered()
        .border_style(fg(ui.accent))
        .title(Line::styled(format!(" {name} "), bold(ui.accent)))
        .style(base(&app.theme));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    let lines: Vec<Line> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| match entry {
            None => Line::styled("─".repeat(inner.width as usize), fg(ui.border)),
            Some(command) => {
                let chord = app.hint(*command);
                let label = command.label();
                let pad = (inner.width as usize)
                    .saturating_sub(label.chars().count() + chord.chars().count() + 2);
                let line = Line::from(vec![
                    Span::raw(format!(" {label}{}", " ".repeat(pad))),
                    Span::styled(chord, fg(ui.fg_dim)),
                ]);
                if i == row {
                    line.style(Style::new().bg(color(ui.selected_bg)))
                } else {
                    line
                }
            }
        })
        .collect();
    app.hits.overlay = rect;
    row_hits(app, inner, 0, entries.len());
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_recent(frame: &mut Frame, app: &mut App, row: usize, area: Rect) {
    let recent = app.settings.recent_projects.clone();
    let inner = overlay_box(
        frame,
        app,
        "OPEN RECENT",
        72,
        recent.len().max(1) as u16,
        area,
    );
    let rows: Vec<Line> = recent
        .iter()
        .map(|p| {
            let mut spans = [Span::raw(" "), Span::raw(p.display().to_string())];
            shorten(&mut spans, 1, inner.width as usize);
            Line::from(spans.to_vec())
        })
        .collect();
    let lines = if rows.is_empty() {
        vec![Line::styled(" nothing opened yet", fg(app.theme.ui.fg_dim))]
    } else {
        list_lines(app, rows, row, inner.height as usize)
    };
    row_hits(
        app,
        inner,
        row.saturating_sub(inner.height.saturating_sub(1) as usize),
        recent.len(),
    );
    frame.render_widget(Paragraph::new(lines), inner);
}

/// What each agent has used of its plan: a bar, a percent, the detail.
fn draw_usage(frame: &mut Frame, app: &mut App, area: Rect) {
    let ui = app.theme.ui.clone();
    let dim = fg(ui.fg_dim);
    let usage = app.usage.clone();
    let rows = usage.as_ref().map_or(1, |(u, _)| u.len().max(1));
    let inner = overlay_box(frame, app, "AGENT USAGE", 72, rows as u16 * 2 + 1, area);
    let mut lines: Vec<Line> = Vec::new();
    match &usage {
        None if app.settings.usage_commands.is_empty() => lines.push(Line::styled(
            " no usage_commands in settings.json — see the comment there",
            dim,
        )),
        None => lines.push(Line::styled(" asking the agents…", dim)),
        Some((usage, age)) => {
            for u in usage {
                let filled = (u.percent as usize * 10).div_ceil(100).min(10);
                let bar_colour = if u.percent >= 80 {
                    ui.accent
                } else {
                    ui.success
                };
                lines.push(Line::from(vec![
                    Span::styled(format!(" {:<8}", u.agent), bold(ui.fg)),
                    Span::styled(format!("{:<10}", u.plan), dim),
                    Span::styled("▰".repeat(filled), fg(bar_colour)),
                    Span::styled("▱".repeat(10 - filled), fg(ui.border)),
                    Span::raw(format!(" {:>3}%  ", u.percent)),
                    Span::styled(u.detail.clone(), dim),
                ]));
                lines.push(Line::styled(format!("          {}", u.reset), dim));
            }
            lines.push(Line::styled(
                format!(" polled from each agent CLI · refreshed {age}s ago"),
                dim,
            ));
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The question a dirty file asks on its way out.
fn draw_close_file(frame: &mut Frame, app: &mut App, area: Rect) {
    let name = app
        .editor
        .as_ref()
        .and_then(|b| b.path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let dim = fg(app.theme.ui.fg_dim);
    let inner = overlay_box(frame, app, "UNSAVED CHANGES", 56, 2, area);
    let lines = vec![
        Line::raw(format!(" {name} has unsaved changes. Save it?")),
        Line::styled(
            format!(" y save · n discard · {} stay", app.hint(Command::Close)),
            dim,
        ),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The themes to pick from, the current one under the cursor to begin with.
fn draw_themes(frame: &mut Frame, app: &mut App, row: usize, area: Rect) {
    let names: Vec<String> = app.themes.iter().map(|t| t.name.clone()).collect();
    let inner = overlay_box(frame, app, "THEME", 40, names.len() as u16, area);
    let rows: Vec<Line> = names.iter().map(|n| Line::raw(format!(" {n}"))).collect();
    let lines = list_lines(app, rows, row, inner.height as usize);
    row_hits(
        app,
        inner,
        row.saturating_sub(inner.height.saturating_sub(1) as usize),
        names.len(),
    );
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The start page: the logotype, the tagline, RECENT and the key hints.
fn draw_start(frame: &mut Frame, app: &mut App, area: Rect) {
    let ui = app.theme.ui.clone();
    let dim = fg(ui.fg_dim);
    const LOGO: [&str; 5] = [
        "██╗   ██╗ █████╗ ██████╗  █████╗ ",
        "╚██╗ ██╔╝██╔══██╗██╔══██╗██╔══██╗",
        " ╚████╔╝ ███████║██████╔╝███████║",
        "  ╚██╔╝  ██╔══██║██╔══██╗██╔══██║",
        "   ██║   ██║  ██║██║  ██║██║  ██║",
    ];
    let recent = app.settings.recent_projects.clone();
    let box_height = recent.len().max(1) as u16 + 2;
    let height = LOGO.len() as u16 + 2 + box_height + 2;
    let width = 60u16.min(area.width);
    let top = area.y + area.height.saturating_sub(height) / 2;
    let left = area.x + (area.width - width) / 2;
    let mut lines: Vec<Line> = LOGO
        .iter()
        .map(|l| Line::styled(*l, fg(ui.accent)).centered())
        .collect();
    lines.push(Line::styled("the terminal editor for the agent loop", dim).centered());
    lines.push(Line::raw(""));
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(left, top, width, LOGO.len() as u16 + 2),
    );

    let rect = Rect::new(
        left,
        top + LOGO.len() as u16 + 2,
        width,
        box_height.min(area.height),
    );
    let block = Block::bordered()
        .border_style(fg(ui.border))
        .title(" RECENT ");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let rows: Vec<Line> = recent
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut spans = [Span::raw(p.display().to_string())];
            shorten(&mut spans, 0, (inner.width as usize).saturating_sub(5));
            let name = spans[0].content.to_string();
            if i == app.start_row {
                let pad = (inner.width as usize).saturating_sub(name.chars().count() + 5);
                Line::from(vec![Span::raw(format!("▸ {name}{}⏎ ", " ".repeat(pad)))])
                    .style(on(ui.bg, ui.accent_dim))
            } else {
                Line::styled(format!("  {name}"), dim)
            }
        })
        .collect();
    let lines = if rows.is_empty() {
        vec![Line::styled("  open a folder: ycode <path>", dim)]
    } else {
        rows
    };
    frame.render_widget(Paragraph::new(lines), inner);
    let hints = format!(
        "{} open project · {} go to file · {} keys",
        app.hint(Command::MarkReviewed),
        app.hint(Command::QuickOpen),
        app.hint(Command::Help)
    );
    frame.render_widget(
        Paragraph::new(Line::styled(hints, dim).centered()),
        Rect::new(left, rect.bottom() + 1, width, 1),
    );
}

/// Go to file: the query with a block cursor, then the best matches.
fn draw_quick_open(frame: &mut Frame, app: &mut App, query: &str, row: usize, area: Rect) {
    let ui = app.theme.ui.clone();
    let dim = fg(ui.fg_dim);
    frame.render_widget(Block::new().style(dim), area);
    let hits = app.quick_open_hits(query);
    let width = area.width.min(72);
    let height = (hits.len().min(12) as u16 + 3).min(area.height.max(3));
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + area.height.saturating_sub(height) / 3,
        width,
        height,
    );
    let block = Block::bordered()
        .border_style(fg(ui.accent))
        .title(Line::styled(" GO TO FILE ", bold(ui.accent)))
        .style(base(&app.theme));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    let mut lines = vec![Line::from(vec![
        Span::styled("> ", dim),
        Span::raw(query.to_string()),
        Span::styled("█", fg(ui.cursor)),
    ])];
    let row = row.min(hits.len().saturating_sub(1));
    let start = row.saturating_sub(10);
    for (i, hit) in hits.iter().enumerate().skip(start).take(12) {
        let mut line = Line::from(format!("  {hit}"));
        if i == row {
            line = line.style(Style::new().bg(color(ui.selected_bg)));
        }
        lines.push(line);
    }
    if hits.is_empty() {
        lines.push(Line::styled("  no such file", dim));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// A one-line question in a box: what is being asked, and what has been
/// typed so far with a block cursor after it.
fn draw_prompt(frame: &mut Frame, app: &mut App, title: &str, what: &str, text: &str, area: Rect) {
    let ui = app.theme.ui.clone();
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
    let mut left = vec![Span::styled(" YARA ", bold(ui.accent))];
    let mut x = area.x + 6;
    for (i, (name, _)) in MENUS.iter().enumerate() {
        let open = matches!(app.overlay, Some(Overlay::Menu(m, _)) if m == i);
        left.push(Span::raw(" "));
        left.push(if open {
            Span::styled(*name, on(ui.bg, ui.accent))
        } else {
            Span::raw(*name)
        });
        left.push(Span::raw(" "));
        app.hits
            .menus
            .push((Rect::new(x, area.y, name.len() as u16 + 2, 1), i));
        x += name.len() as u16 + 2;
    }
    // The tabs: one agent in one worktree each, and the way to another.
    left.push(Span::styled(" │ ", fg(ui.fg_dim)));
    let tabs_from = left.len();
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
    let mut right = vec![if app.agent_running() {
        Span::styled(format!("● {} — running ", app.agent_name()), fg(ui.success))
    } else {
        Span::styled(format!("○ {} — exited ", app.agent_name()), fg(ui.fg_dim))
    }];
    if let Some(chip) = app.usage_chip() {
        right.insert(0, Span::styled(format!("{chip}  "), fg(ui.fg_dim)));
    }
    let right = Line::from(right);
    // Where the tabs and the [+] landed, for the mouse.
    let mut x = area.x + left[..tabs_from].iter().map(Span::width).sum::<usize>() as u16;
    for (i, pair) in left[tabs_from..].chunks(2).enumerate() {
        let w = pair[0].width() as u16;
        if pair.len() == 2 {
            app.hits.tabs.push((Rect::new(x, area.y, w, 1), i));
        } else {
            app.hits.plus = Rect::new(x, area.y, w, 1);
        }
        x += pair.iter().map(Span::width).sum::<usize>() as u16;
    }
    let right_x = area.right().saturating_sub(right.width() as u16);
    if app.usage_chip().is_some() {
        let w = right.spans[0].width() as u16;
        app.hits.usage = Rect::new(right_x, area.y, w, 1);
    }
    frame.render_widget(Paragraph::new(Line::from(left)), area);
    frame.render_widget(Paragraph::new(right.right_aligned()), area);
}

fn draw_files(frame: &mut Frame, app: &mut App, area: Rect) {
    let ui = app.theme.ui.clone();
    let focused = app.focus == Focus::Files;
    let block = Block::bordered()
        .border_style(fg(if focused { ui.fg_dim } else { ui.border }))
        .title(" FILES ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }
    let [list, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    let file_rows = if let Some(tree) = &app.tree {
        let root = tree.root.clone();
        let opened = app.editor.as_ref().map(|b| b.path.clone());
        let touched = |path: &std::path::Path| {
            path.strip_prefix(&root)
                .ok()
                .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                .is_some_and(|rel| app.changes.iter().any(|c| c.path == rel))
        };
        let lines: Vec<Line> = tree
            .rows()
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let name = row
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let glyph = if row.is_dir {
                    if tree
                        .rows()
                        .get(i + 1)
                        .is_some_and(|next| next.depth > row.depth)
                    {
                        "▾"
                    } else {
                        "▸"
                    }
                } else {
                    "▫"
                };
                let mut spans = vec![
                    Span::raw("  ".repeat(row.depth)),
                    Span::styled(format!("{glyph} "), fg(ui.fg_dim)),
                ];
                let changed = !row.is_dir && touched(&row.path);
                spans.push(Span::styled(
                    name,
                    fg(if changed { ui.accent_dim } else { ui.fg }),
                ));
                if changed {
                    spans.push(Span::styled(" ●", fg(ui.accent_dim)));
                }
                let mut line = Line::from(spans);
                if focused && i == tree.selected {
                    line = line.style(Style::new().bg(color(ui.selected_bg)));
                } else if opened.as_deref() == Some(row.path.as_path()) {
                    line = line.style(Style::new().bg(color(ui.accent_bg)));
                }
                line
            })
            .collect();
        let scroll = tree
            .selected
            .saturating_sub(list.height.saturating_sub(1) as usize);
        let rows: Vec<(Rect, usize)> = (scroll
            ..tree.rows().len().min(scroll + list.height as usize))
            .map(|i| {
                (
                    Rect::new(list.x, list.y + (i - scroll) as u16, list.width, 1),
                    i,
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), list);
        rows
    } else {
        Vec::new()
    };
    app.hits.file_rows = file_rows;
    let footer = format!(
        "{} hide · {} open",
        app.hint(Command::ToggleSidebar),
        app.hint(Command::MarkReviewed)
    );
    frame.render_widget(Paragraph::new(Line::styled(footer, fg(ui.fg_dim))), foot);
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

fn draw_follow(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.editor.is_some() {
        draw_editor(frame, app, area);
        return;
    }
    let ui = app.theme.ui.clone();
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
    // The right-hand title is a button: `[ f → live ]` or `[ Esc → timeline ]`.
    if !app.follow.is_live() || app.pinned.is_some() {
        app.hits.live = Rect::new(area.right().saturating_sub(18), area.y, 17, 1);
    }
    if inner.height == 0 {
        return;
    }
    let [file_row, timeline_row, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);

    let Some(edit) = app.shown().cloned() else {
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
    let first_x = timeline_row.x + 6 + u16::from(window.hidden_before > 0);
    for (i, tick) in ticks[window.start..window.end].iter().enumerate() {
        let x = first_x + i as u16;
        app.hits
            .ticks
            .push((Rect::new(x, timeline_row.y, 1, 1), window.start + i));
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
        draw_file(frame, app, &edit, body);
        return;
    }
    let scroll = app.scroll;
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
    // Never past the end: the wheel may have asked for more than there is.
    let scroll = scroll.min((lines.len() as u16).saturating_sub(body.height));
    app.scroll = scroll;
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), body);
}

/// A file being edited where the follow pane was: its path, a dot while it
/// is dirty, and the text coloured by its grammar with the caret in it.
fn draw_editor(frame: &mut Frame, app: &mut App, area: Rect) {
    let ui = app.theme.ui.clone();
    let dim = fg(ui.fg_dim);
    let focused = app.focus == Focus::Editor;
    let Some(buffer) = &app.editor else { return };
    let block = Block::bordered()
        .border_style(fg(if focused { ui.accent } else { ui.border }))
        .title(Line::styled(" EDIT ", bold(ui.accent)))
        .title_top(
            Line::styled(
                format!(
                    " {} save · {} close ",
                    app.hint(Command::Save),
                    app.hint(Command::Close)
                ),
                dim,
            )
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }
    let [file_row, body] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    let relative = app
        .project
        .as_deref()
        .and_then(|root| buffer.path.strip_prefix(root).ok())
        .unwrap_or(&buffer.path);
    let language = app.syntax.language(buffer.extension());
    let mut file = vec![
        Span::styled(relative.display().to_string(), bold(ui.fg)),
        Span::styled(if buffer.modified() { " ●" } else { "" }, fg(ui.accent)),
    ];
    shorten(
        &mut file,
        0,
        (file_row.width as usize).saturating_sub(language.len() + 1),
    );
    let [name_area, language_area] = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(language.len() as u16),
    ])
    .areas(file_row);
    frame.render_widget(Paragraph::new(Line::from(file)), name_area);
    frame.render_widget(Paragraph::new(Line::styled(language, dim)), language_area);

    let (line, col) = buffer.line_col();
    let height = body.height as usize;
    let total = buffer.text.lines().count()
        + usize::from(buffer.text.ends_with('\n') || buffer.text.is_empty());
    // The view scrolls by the wheel, and follows the caret only when the
    // caret moved — so reading a file does not drag the caret about.
    let mut top = (app.scroll as usize).min(total.saturating_sub(height));
    if app.caret_moved {
        if line < top {
            top = line;
        } else if line >= top + height {
            top = line + 1 - height;
        }
    }
    let new_scroll = top as u16;
    let mut lines: Vec<Line> = Vec::new();
    let mut number = 0;
    app.syntax
        .highlight(buffer.extension(), &buffer.text, |regions| {
            number += 1;
            if number <= top || lines.len() >= height {
                return;
            }
            let mut spans = vec![Span::styled(format!("{number:>5}  "), dim)];
            for region in regions {
                let mut style = fg(region.color);
                if region.italic {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if region.bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                spans.push(Span::styled(
                    region.text.trim_end_matches('\n').to_string(),
                    style,
                ));
            }
            lines.push(Line::from(spans));
        });
    // A file that ends in a newline has an empty last line the caret can
    // stand on, which the highlighter does not emit.
    if (buffer.text.is_empty() || buffer.text.ends_with('\n')) && lines.len() < height {
        number += 1;
        lines.push(Line::from(Span::styled(format!("{number:>5}  "), dim)));
    }
    frame.render_widget(Paragraph::new(lines), body);
    // The caret is drawn rather than the terminal's: a terminal's own
    // cursor stops blinking under the agent's constant redraws.
    if focused && line >= top && app.caret_on {
        let x = body.x + 7 + col as u16;
        let y = body.y + (line - top) as u16;
        if x < body.right() && y < body.bottom() {
            let cell = &mut frame.buffer_mut()[(x, y)];
            cell.set_bg(color(ui.cursor));
            cell.set_fg(color(ui.bg));
        }
    }
    app.scroll = new_scroll;
    app.caret_moved = false;
    app.hits.editor = Rect::new(
        body.x + 7,
        body.y,
        body.width.saturating_sub(7),
        body.height,
    );
}

/// The file as it stands, with an accent bar beside every line the edit
/// added. A file that is gone reads as empty.
fn draw_file(frame: &mut Frame, app: &mut App, edit: &EditEvent, area: Rect) {
    let ui = app.theme.ui.clone();
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
    let scroll = app
        .scroll
        .min((text.lines().count() as u16).saturating_sub(area.height));
    app.scroll = scroll;
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
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
}

/// The CHANGES overlay: what differs from the base branch, one row a file.
fn draw_changes(frame: &mut Frame, app: &mut App, row: usize, area: Rect) {
    let ui = app.theme.ui.clone();
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
    let scroll = row.saturating_sub(list.height.saturating_sub(1) as usize);
    row_hits(app, list, scroll, app.changes.len());
    frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), list);
    frame.render_widget(Paragraph::new(Line::styled(footer, dim)), foot);
}

/// Lines added and removed against the base, over every changed file.
fn totals(app: &mut App) -> (usize, usize) {
    app.changes
        .iter()
        .fold((0, 0), |(a, r), c| (a + c.added, r + c.removed))
}

fn draw_status(frame: &mut Frame, app: &mut App, area: Rect) {
    let ui = app.theme.ui.clone();
    let dim = fg(ui.fg_dim);
    let (added, removed) = totals(app);
    let review = match app.follow.unreviewed_count() {
        0 if app.follow.is_empty() => Span::styled("no edits yet", dim),
        0 => Span::styled("✓ all reviewed", fg(ui.success)),
        n => Span::styled(format!("◆ {n} unreviewed"), fg(ui.accent)),
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
    // The path keeps a dozen columns whatever the right side wants; the
    // hints go one by one, from the left, before anything else does.
    let others: usize = left.iter().map(Span::width).sum::<usize>() - left[1].width();
    let least = (others + 12).min(area.width as usize);
    // The note is news; the hints give way to it until the next key.
    let right = match &app.note {
        Some(note) => Line::styled(format!(" {note} "), on(ui.fg, ui.selected_bg)),
        None => {
            let mut hints: Vec<String> = [
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
            let version = format!("{} ", app.version_chip());
            let chip = fg(
                if app.updates.installed.is_some() || app.updates.available.is_some() {
                    ui.success
                } else {
                    ui.fg_dim
                },
            );
            let room = (area.width as usize).saturating_sub(least + 1);
            while !hints.is_empty() && hints.join("  ").len() + 2 + version.len() > room {
                hints.remove(0);
            }
            Line::from(vec![
                Span::styled(format!("{}  ", hints.join("  ")), dim),
                Span::styled(version, chip),
            ])
        }
    };
    let room = (area.width as usize)
        .saturating_sub(right.width() + 1)
        .max(others + 12)
        .min(area.width as usize);
    shorten(&mut left, 1, room);
    let width: usize = left.iter().map(Span::width).sum();
    // The counter is a button: a click goes to the next unreviewed edit.
    let before_counter: usize = left[..left.len() - 1].iter().map(Span::width).sum();
    app.hits.counter = Rect::new(
        area.x + before_counter as u16,
        area.y,
        left[left.len() - 1].width() as u16,
        1,
    );
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

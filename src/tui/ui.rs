//! Rendering for the terminal frontend.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use std::path::Path;

/// Rows the find bar takes: heading, field, heading, field, actions.
const FIND_ROWS: u16 = 5;
use crate::core::command::{Command, START_PAGE};
use crate::core::fold;
use crate::core::search::Field as SearchField;
use crate::core::diff;
use crate::core::git as core_git;
use crate::core::theme as core_theme;
use crate::tui::app::{App, Focus, Prompt, SidebarView, Splitter, TabStrip};
use crate::tui::theme::{color, on};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let theme = app.theme().clone();
    let area = frame.area();
    frame.render_widget(
        Block::default().style(on(theme.ui.fg, theme.ui.editor_bg)),
        area,
    );

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
        ])
        .split(area);
    let (menu_row, menu_rule, body) = (rows[0], rows[1], rows[2]);
    draw_menu_bar(frame, app, menu_row);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(menu_rule.width as usize),
            on(theme.ui.border, theme.ui.editor_bg),
        ))),
        menu_rule,
    );

    app.layout.body = body;
    let sidebar_width = if app.show_sidebar {
        app.sidebar_width
            .clamp(12, body.width.saturating_sub(24).max(12))
    } else {
        0
    };
    let split_width = u16::from(app.show_sidebar);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(sidebar_width),
            Constraint::Length(split_width),
            Constraint::Min(10),
        ])
        .split(body);
    let (sidebar, v_split, right) = (cols[0], cols[1], cols[2]);
    app.layout.sidebar = if app.show_sidebar { sidebar } else { Rect::default() };
    app.layout.v_split = v_split;

    // The sidebar runs the full height; the status line spans the editor
    // region only, matching the window frontend.
    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(1)])
        .split(right);
    let (main, status_row) = (right_rows[0], right_rows[1]);

    let shell_height = if app.show_shell {
        app.shell_height
            .clamp(3, main.height.saturating_sub(5).max(3))
    } else {
        0
    };
    let main_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(u16::from(app.show_shell)),
            Constraint::Length(shell_height),
        ])
        .split(main);
    let (editor_area, h_split, shell_area) = (main_rows[0], main_rows[1], main_rows[2]);
    app.layout.h_split = h_split;

    // The find bar sits between the tabs and the text, like the window's.
    // Heading, field, heading, field, actions — the search panel's form.
    let find_rows = if app.find_showing() { FIND_ROWS } else { 0 };
    let editor_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(find_rows), Constraint::Min(1)])
        .split(editor_area);
    let (tab_row, find_area, editor_area) = (editor_rows[0], editor_rows[1], editor_rows[2]);

    if app.show_sidebar && sidebar.width > 2 {
        // The view switcher sits in the panel's footer, like the window's.
        let footer = Rect {
            y: sidebar.y + sidebar.height.saturating_sub(1),
            height: 1,
            ..sidebar
        };
        let content = Rect {
            height: sidebar.height.saturating_sub(1),
            ..sidebar
        };
        draw_sidebar_footer(frame, app, footer);
        match app.sidebar_view {
            SidebarView::Files => {
                app.layout.search_options = Rect::default();
                app.layout.search_exclude = Rect::default();
                app.layout.search_replace = Rect::default();
                app.layout.search_input = Rect::default();
                app.layout.search_list = Rect::default();
                reset_git_rects(app);
                draw_tree(frame, app, content);
            }
            SidebarView::Search => {
                app.layout.tree = Rect::default();
                reset_git_rects(app);
                draw_search(frame, app, content);
            }
            SidebarView::Git => {
                app.layout.tree = Rect::default();
                app.layout.search_options = Rect::default();
                app.layout.search_exclude = Rect::default();
                app.layout.search_replace = Rect::default();
                app.layout.search_input = Rect::default();
                app.layout.search_list = Rect::default();
                draw_git(frame, app, content);
            }
        }
    } else {
        app.layout.tree = Rect::default();
        app.layout.search_input = Rect::default();
        app.layout.search_exclude = Rect::default();
        app.layout.search_list = Rect::default();
        app.layout.sidebar_header = Rect::default();
        app.layout.tab_files = Rect::default();
        app.layout.tab_search = Rect::default();
        app.layout.tab_git = Rect::default();
        reset_git_rects(app);
    }
    draw_tab_strip(frame, app, tab_row);
    if app.active_diff().is_some() {
        draw_diff(frame, app, editor_area);
    } else {
        draw_editor(frame, app, editor_area);
    }
    if app.find_showing() {
        draw_find(frame, app, find_area);
    } else {
        app.layout.find_query = Rect::default();
        app.layout.find_replace = Rect::default();
        app.layout.find_prev = Rect::default();
        app.layout.find_next = Rect::default();
        app.layout.find_close = Rect::default();
        app.layout.find_replace_one = Rect::default();
        app.layout.find_replace_all = Rect::default();
        app.layout.find_case = Rect::default();
        app.layout.find_word = Rect::default();
        app.layout.find_regex = Rect::default();
    }
    if app.show_shell && shell_area.height > 1 {
        draw_shell(frame, app, shell_area);
    } else {
        app.layout.shell = Rect::default();
        app.layout.shell_tabs.clear();
        app.layout.shell_new = Rect::default();
    }
    draw_splitters(frame, app, v_split, h_split);
    draw_status(frame, app, status_row);
    draw_prompt(frame, app, area);
    draw_menu(frame, app, area);
}

/// Pane borders. They light up under the pointer and while being dragged, the
/// same feedback the window frontend gives its splitters.
fn draw_splitters(frame: &mut Frame, app: &App, vertical: Rect, horizontal: Rect) {
    let theme = app.theme();
    let active = app.hovering_split();
    let line_style = |lit: bool| {
        if lit {
            on(theme.ui.accent_light, theme.ui.editor_bg)
        } else {
            on(theme.ui.border, theme.ui.editor_bg)
        }
    };
    if vertical.width > 0 {
        let lit = active == Some(Splitter::Sidebar);
        let lines: Vec<Line> = (0..vertical.height)
            .map(|_| Line::from(Span::styled("\u{2502}", line_style(lit))))
            .collect();
        frame.render_widget(Paragraph::new(lines), vertical);
    }
    if horizontal.height > 0 {
        let lit = active == Some(Splitter::Shell);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "\u{2500}".repeat(horizontal.width as usize),
                line_style(lit),
            ))),
            horizontal,
        );
    }
}

/// The top bar: the app name, then the File, View and Help menus.
fn draw_menu_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme().clone();
    let name = " YARA ";
    let labels = [" File ", " View ", " Help "];
    let mut spans = vec![Span::styled(
        name,
        Style::default()
            .fg(color(theme.ui.accent_light))
            .bg(color(theme.ui.status_bg))
            .add_modifier(Modifier::BOLD),
    )];
    let mut x = area.x + name.chars().count() as u16;
    for (i, label) in labels.iter().enumerate() {
        let button = Rect {
            x,
            y: area.y,
            width: label.chars().count() as u16,
            height: 1,
        };
        app.menu_buttons[i] = button;
        let open = app
            .menu
            .as_ref()
            .is_some_and(|m| m.target.is_none() && m.x == button.x && m.y == area.y + 1);
        let hovered = app
            .mouse
            .is_some_and(|(mx, my)| my == area.y && mx >= button.x && mx < button.x + button.width);
        spans.push(Span::styled(
            *label,
            if open || hovered {
                Style::default()
                    .fg(color(theme.ui.fg_bright))
                    .bg(color(theme.ui.selected_bg))
            } else {
                on(theme.ui.fg, theme.ui.status_bg)
            },
        ));
        x += button.width;
    }
    let used = (x - area.x) as usize;
    spans.push(Span::styled(
        pad("", (area.width as usize).saturating_sub(used)),
        on(theme.ui.fg_faint, theme.ui.status_bg),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn reset_git_rects(app: &mut App) {
    app.layout.git_repo = Rect::default();
    app.layout.git_worktree = Rect::default();
    app.layout.git_list = Rect::default();
    app.layout.git_list_offset = 0;
}

/// Sidebar footer: the Files / Search / Git switch as an icon row — the same
/// arrangement the window frontend keeps at the bottom of its panel.
fn draw_sidebar_footer(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme().clone();
    app.layout.sidebar_header = area;

    let entries = [
        (SidebarView::Files, app.icons.nav_files, "FILES", Focus::Tree),
        (SidebarView::Search, app.icons.nav_search, "SEARCH", Focus::Search),
        (SidebarView::Git, app.icons.nav_git, "GIT", Focus::Git),
    ];
    // Labels join the icons when the panel is wide enough for them.
    let labeled_width: usize = entries
        .iter()
        .map(|(_, icon, label, _)| 2 + icon.chars().count() + 1 + label.len())
        .sum();
    let labeled = labeled_width <= area.width as usize;

    let mouse = app.mouse;
    let hovered = |rect: Rect| {
        mouse.is_some_and(|(x, y)| y == rect.y && x >= rect.x && x < rect.x + rect.width)
    };

    let mut spans = Vec::new();
    let mut rects = [Rect::default(); 3];
    let mut x = area.x;
    for (i, (view, icon, label, focus)) in entries.into_iter().enumerate() {
        let text = if labeled {
            format!(" {icon} {label} ")
        } else {
            format!(" {icon} ")
        };
        let width = text.chars().count() as u16;
        let rect = Rect {
            x,
            y: area.y,
            width: width.min((area.x + area.width).saturating_sub(x)),
            height: 1,
        };
        // The active switch also carries the focus indicator: accent while
        // its pane has the keyboard, plain bright otherwise.
        let style = if app.sidebar_view == view {
            Style::default()
                .fg(color(if app.focus == focus {
                    theme.ui.accent_light
                } else {
                    theme.ui.fg_bright
                }))
                .bg(color(theme.ui.status_bg))
                .add_modifier(Modifier::BOLD)
        } else if hovered(rect) {
            on(theme.ui.fg, theme.ui.status_bg)
        } else {
            on(theme.ui.fg_faint, theme.ui.status_bg)
        };
        spans.push(Span::styled(text, style));
        rects[i] = rect;
        x += width;
    }
    app.layout.tab_files = rects[0];
    app.layout.tab_search = rects[1];
    app.layout.tab_git = rects[2];
    let used = (x - area.x) as usize;
    spans.push(Span::styled(
        pad("", (area.width as usize).saturating_sub(used)),
        on(theme.ui.fg_faint, theme.ui.status_bg),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Git view: repository and worktree pickers, then the changed files.
fn draw_git(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme().clone();
    frame.render_widget(
        Block::default().style(on(theme.ui.fg, theme.ui.sidebar_bg)),
        area,
    );
    // With no folder open there is no repository to report on.
    if let Some(root) = app.project.root().map(Path::to_path_buf) {
        app.git.tick(&root);
    }

    let width = area.width as usize;
    let bottom = area.y + area.height;
    let mut y = area.y;
    let row = |y: u16| Rect {
        y,
        height: 1,
        ..area
    };

    if area.height == 0 {
        reset_git_rects(app);
        return;
    }
    if app.git.repos.is_empty() {
        reset_git_rects(app);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                pad(" not a git repository", width),
                on(theme.ui.fg_faint, theme.ui.sidebar_bg),
            ))),
            row(y),
        );
        return;
    }

    let mouse = app.mouse;
    let hovered = |rect: Rect| {
        mouse.is_some_and(|(x, my)| my == rect.y && x >= rect.x && x < rect.x + rect.width)
    };
    let name_of = |path: &std::path::Path| {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    };

    // Repository and worktree pickers, each under its own heading; clicking
    // the value opens a selection prompt, like the theme picker.
    let heading = |frame: &mut Frame, text: &str, rect: Rect| {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                pad(&format!(" {text}"), width),
                on(theme.ui.fg_faint, theme.ui.sidebar_bg),
            ))),
            rect,
        );
    };
    // The value sits in an inset box on the editor background, like the
    // search panel's inputs.
    let picker = |frame: &mut Frame, value: String, rect: Rect| {
        let value_style = if hovered(rect) {
            on(theme.ui.fg_bright, theme.ui.editor_bg)
        } else {
            on(theme.ui.fg, theme.ui.editor_bg)
        };
        let inner = rect.width.saturating_sub(1) as usize;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ", on(theme.ui.fg_faint, theme.ui.editor_bg)),
                Span::styled(pad(&clip(&value, inner), inner), value_style),
            ])),
            rect,
        );
    };
    let inset = |y: u16| Rect {
        x: area.x + 1,
        y,
        width: area.width.saturating_sub(2),
        height: 1,
    };
    let give_up = |app: &mut App, worktree_too: bool| {
        if worktree_too {
            app.layout.git_worktree = Rect::default();
        }
        app.layout.git_list = Rect::default();
        app.layout.git_list_offset = 0;
    };

    heading(frame, "REPOSITORY", row(y));
    y += 1;
    if y >= bottom {
        app.layout.git_repo = Rect::default();
        give_up(app, true);
        return;
    }
    let repo_value = app
        .git
        .repos
        .get(app.git.repo)
        .map(|p| name_of(p))
        .unwrap_or_default();
    let repo_area = inset(y);
    app.layout.git_repo = repo_area;
    picker(frame, repo_value, repo_area);
    y += 2; // a blank row separates each label-and-value pair
    if y >= bottom {
        give_up(app, true);
        return;
    }

    heading(frame, "WORKTREE", row(y));
    y += 1;
    if y >= bottom {
        give_up(app, true);
        return;
    }
    let tree_value = app
        .git
        .worktrees
        .get(app.git.worktree)
        .map(|w| {
            if w.branch.is_empty() {
                w.name()
            } else {
                format!("{} · {}", w.name(), w.branch)
            }
        })
        .unwrap_or_default();
    let tree_area = inset(y);
    app.layout.git_worktree = tree_area;
    picker(frame, tree_value, tree_area);
    y += 2;
    if y >= bottom {
        give_up(app, false);
        return;
    }

    // Summary line, carrying any git error.
    let summary = match &app.git.error {
        Some(error) => error.clone(),
        None if app.git.changes.is_empty() => "no changes".to_string(),
        None => format!("{} changed file(s)", app.git.changes.len()),
    };
    let tone = if app.git.error.is_some() {
        theme.ui.danger
    } else {
        theme.ui.fg_faint
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            pad(&format!(" {}", clip(&summary, width.saturating_sub(1))), width),
            on(tone, theme.ui.sidebar_bg),
        ))),
        row(y),
    );
    y += 1;

    let list_area = Rect {
        y,
        height: bottom.saturating_sub(y),
        ..area
    };
    app.layout.git_list = list_area;
    let height = list_area.height as usize;
    if height == 0 || app.git.changes.is_empty() {
        app.layout.git_list_offset = 0;
        return;
    }
    app.git_selected = app.git_selected.min(app.git.changes.len() - 1);
    let offset = app
        .git_selected
        .saturating_sub(height.saturating_sub(1));
    app.layout.git_list_offset = offset;

    let lines: Vec<Line> = app
        .git
        .changes
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, change)| {
            let selected = i == app.git_selected;
            let bg = if selected {
                theme.ui.selected_bg
            } else {
                theme.ui.sidebar_bg
            };
            let letter = change.letter();
            let path_width = width.saturating_sub(4);
            Line::from(vec![
                Span::styled(
                    format!(" {letter}  "),
                    Style::default()
                        .fg(color(git_letter_color(letter, &theme)))
                        .bg(color(bg)),
                ),
                Span::styled(
                    pad(&trim_front(&change.path, path_width), path_width),
                    on(
                        if selected {
                            theme.ui.fg_bright
                        } else {
                            theme.ui.fg_dim
                        },
                        bg,
                    ),
                ),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), list_area);
}

/// Status letters use the terminal palette, so every theme colors them sanely.
fn git_letter_color(letter: char, theme: &crate::core::theme::Theme) -> crate::core::theme::Rgb {
    let index = match letter {
        'A' | 'U' => 2, // green: new or untracked
        'D' => 1,       // red: deleted
        'R' | 'C' => 6, // cyan: renamed or copied
        _ => 3,         // yellow: modified
    };
    theme.ansi[index]
}

fn draw_tree(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme().clone();
    frame.render_widget(
        Block::default().style(on(theme.ui.fg, theme.ui.sidebar_bg)),
        area,
    );

    let inner = area;
    let height = inner.height as usize;
    app.tree.clamp_scroll(height);
    app.layout.tree = inner;
    let scroll = app.tree.scroll;
    let selected = app.tree.selected;
    let icons = app.icons;
    let hover_row = app.mouse.and_then(|(x, y)| {
        (x >= inner.x && x < inner.x + inner.width && y >= inner.y && y < inner.y + inner.height)
            .then(|| scroll + (y - inner.y) as usize)
    });

    // Nothing in the project yet: say how to put a folder in it, rather than
    // showing an empty panel.
    if app.tree.rows().is_empty() {
        let chord = |command| {
            app.settings
                .tui_chord(command)
                .map(|c| c.to_string())
                .unwrap_or_default()
        };
        let hint = vec![
            Line::from(Span::styled(
                pad(" No folder in the project", inner.width as usize),
                on(theme.ui.fg_dim, theme.ui.sidebar_bg),
            )),
            Line::from(Span::styled(
                pad("", inner.width as usize),
                on(theme.ui.fg_dim, theme.ui.sidebar_bg),
            )),
            Line::from(Span::styled(
                pad(
                    &format!(" {}  add a folder", chord(Command::AddFolder)),
                    inner.width as usize,
                ),
                on(theme.ui.fg_faint, theme.ui.sidebar_bg),
            )),
            Line::from(Span::styled(
                pad(
                    &format!(" {}  open a folder", chord(Command::OpenFolder)),
                    inner.width as usize,
                ),
                on(theme.ui.fg_faint, theme.ui.sidebar_bg),
            )),
        ];
        frame.render_widget(Paragraph::new(hint), inner);
        return;
    }

    let lines: Vec<Line> = app
        .tree
        .rows()
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(i, row)| {
            let is_selected = i == selected;
            let is_hovered = hover_row == Some(i);
            let is_drop_target = app.drag_over.as_deref() == Some(row.path.as_path());
            let is_dragged = app.drag.as_deref() == Some(row.path.as_path());
            let indent = format!(" {}", "  ".repeat(row.depth));
            let icon = if row.is_dir {
                if app.tree.expanded.contains(&row.path) {
                    icons.dir_open
                } else {
                    icons.dir_closed
                }
            } else {
                icons.file
            };
            let name = row
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let bg = if is_drop_target {
                theme.ui.accent
            } else if is_selected {
                theme.ui.selected_bg
            } else if is_hovered {
                theme.ui.hover_bg
            } else {
                theme.ui.sidebar_bg
            };
            let git_tint = app.git.state_of(&row.path).map(|state| match state {
                core_git::FileState::Added | core_git::FileState::Untracked => {
                    core_theme::ansi256(&theme, 2)
                }
                core_git::FileState::Deleted => core_theme::ansi256(&theme, 1),
                core_git::FileState::Modified => core_theme::ansi256(&theme, 3),
            });
            // A folder wears the tint too while something under it has changed.
            let git_tint = git_tint.or_else(|| {
                (row.is_dir && app.git.folder_touched(&row.path))
                    .then(|| core_theme::ansi256(&theme, 3))
            });
            let fg = if is_drop_target || is_selected {
                theme.ui.fg_bright
            } else if is_dragged {
                theme.ui.accent_light
            } else if let Some(tint) = git_tint {
                tint
            } else if row.is_root {
                // A project folder heads its own subtree; it reads as a title,
                // not as one more directory.
                theme.ui.fg_bright
            } else if row.is_dir {
                theme.ui.fg_dim
            } else {
                theme.ui.fg
            };
            let icon_fg = if is_drop_target || is_selected {
                theme.ui.fg_bright
            } else {
                theme.ui.fg_faint
            };
            let width = inner.width as usize;
            let head = format!("{indent}{icon} ");
            let tail = pad(&name, width.saturating_sub(head.chars().count()));
            Line::from(vec![
                Span::styled(head, on(icon_fg, bg)),
                Span::styled(tail, on(fg, bg)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The start page: what fills the editor while nothing is open — the name, the
/// folder in play, and the keys that are actually bound, in the same groups the
/// window shows. Groups are packed into as many columns as the pane affords.
fn draw_start_page(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    frame.render_widget(
        Block::default().style(on(theme.ui.fg, theme.ui.editor_bg)),
        area,
    );
    if area.width < 24 || area.height < 6 {
        return;
    }
    let chord = |command| app.settings.tui_chord(command).map(|c| c.to_string());

    // One block per group: its heading, then a line per bound key.
    let mut blocks: Vec<Vec<(String, String)>> = Vec::new();
    for (name, commands) in START_PAGE {
        let mut rows: Vec<(String, String)> = Vec::new();
        for command in *commands {
            if let Some(chord) = chord(*command) {
                rows.push((chord, command.label().to_string()));
            }
        }
        if rows.is_empty() {
            continue;
        }
        rows.insert(0, (String::new(), name.to_uppercase()));
        blocks.push(rows);
    }
    if blocks.is_empty() {
        return;
    }

    let chord_width = blocks
        .iter()
        .flatten()
        .map(|(chord, _)| chord.chars().count())
        .max()
        .unwrap_or(0);
    let column_width = blocks
        .iter()
        .flatten()
        .map(|(chord, label)| {
            if chord.is_empty() {
                label.chars().count()
            } else {
                chord_width + 2 + label.chars().count()
            }
        })
        .max()
        .unwrap_or(10);

    // As many columns as fit, so a short pane spreads sideways instead of
    // running off the bottom.
    const GAP: usize = 4;
    let columns = (((area.width as usize + GAP) / (column_width + GAP)).max(1)).min(blocks.len());
    let per_column = blocks.len().div_ceil(columns);

    let title = Style::default()
        .fg(color(theme.ui.accent_light))
        .bg(color(theme.ui.editor_bg))
        .add_modifier(Modifier::BOLD);
    let group = Style::default()
        .fg(color(theme.ui.fg_dim))
        .bg(color(theme.ui.editor_bg))
        .add_modifier(Modifier::BOLD);
    let key = on(theme.ui.fg_bright, theme.ui.editor_bg);
    let plain = on(theme.ui.fg_faint, theme.ui.editor_bg);

    // Each column is a list of styled cells; columns are then zipped into rows.
    let mut grid: Vec<Vec<Vec<Span>>> = Vec::new();
    for chunk in blocks.chunks(per_column) {
        let mut cells: Vec<Vec<Span>> = Vec::new();
        for (i, rows) in chunk.iter().enumerate() {
            if i > 0 {
                cells.push(Vec::new());
            }
            for (chord, label) in rows {
                if chord.is_empty() {
                    cells.push(vec![Span::styled(label.clone(), group)]);
                } else {
                    cells.push(vec![
                        Span::styled(format!("{chord:<chord_width$}  "), key),
                        Span::styled(label.clone(), plain),
                    ]);
                }
            }
        }
        grid.push(cells);
    }
    let body_height = grid.iter().map(Vec::len).max().unwrap_or(0);

    let block_width = columns * column_width + (columns - 1) * GAP;
    let left = " ".repeat(((area.width as usize).saturating_sub(block_width)) / 2);
    let mut lines: Vec<Line> = Vec::new();
    let header = [
        (
            match app.project.root() {
                Some(root) => root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.display().to_string()),
                None => "no folder in the project".to_string(),
            },
            plain,
        ),
        (String::new(), plain),
    ];
    let head_room = area.height as usize;
    if head_room > body_height + 4 {
        lines.push(Line::from(vec![
            Span::styled(left.clone(), plain),
            Span::styled("YARA", title),
        ]));
        for (text, style) in header {
            lines.push(Line::from(vec![
                Span::styled(left.clone(), plain),
                Span::styled(text, style),
            ]));
        }
    }
    for row in 0..body_height {
        let mut spans = vec![Span::styled(left.clone(), plain)];
        for (i, column) in grid.iter().enumerate() {
            let cell = column.get(row).cloned().unwrap_or_default();
            let used: usize = cell.iter().map(|s| s.content.chars().count()).sum();
            spans.extend(cell);
            let tail = if i + 1 == grid.len() {
                0
            } else {
                column_width + GAP - used.min(column_width + GAP)
            };
            spans.push(Span::styled(" ".repeat(tail), plain));
        }
        lines.push(Line::from(spans));
    }
    // Sit a little above center, the way the window's start page does.
    let top = (area.height as usize).saturating_sub(lines.len()) / 3;
    let mut out: Vec<Line> = vec![Line::from(""); top];
    out.extend(lines);
    frame.render_widget(Paragraph::new(out), area);
}

fn draw_search(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme().clone();
    frame.render_widget(
        Block::default().style(on(theme.ui.fg, theme.ui.sidebar_bg)),
        area,
    );
    // Options row, then query, replace (when open) and excludes — the same
    // fields in the same order as the window frontend, one row each.
    let mut y = area.y;
    let row = |y: u16| Rect {
        y,
        height: 1,
        ..area
    };
    let width = area.width as usize;

    // Headings over the fields, in the git view's form; each one lights up
    // while its field has the keyboard.
    let heading = |frame: &mut Frame, text: &str, focused: bool, at: Rect| {
        let style = if focused {
            Style::default()
                .fg(color(theme.ui.accent_light))
                .bg(color(theme.ui.sidebar_bg))
                .add_modifier(Modifier::BOLD)
        } else {
            on(theme.ui.fg_faint, theme.ui.sidebar_bg)
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                pad(&format!(" {text}"), width),
                style,
            ))),
            at,
        );
    };
    let field_focused =
        |which: SearchField| app.focus == Focus::Search && app.search.field() == which;

    // Heading row: SEARCH on the left, the toggles at the right edge.
    let options = row(y);
    app.layout.search_options = options;
    let toggle_style = |enabled: bool| {
        if enabled {
            Style::default()
                .fg(color(theme.ui.fg_bright))
                .bg(color(theme.ui.selected_bg))
        } else {
            on(theme.ui.fg_faint, theme.ui.sidebar_bg)
        }
    };
    const LABEL: &str = "SEARCH";
    let toggles_width = 2 + 1 + 2 + 1 + 2 + 1;
    let gap = width.saturating_sub(1 + LABEL.len() + toggles_width);
    let toggles_x = options.x + (1 + LABEL.len() + gap) as u16;
    app.layout.search_toggles = Rect {
        x: toggles_x,
        width: (toggles_width as u16)
            .min((options.x + options.width).saturating_sub(toggles_x)),
        ..options
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", on(theme.ui.fg_faint, theme.ui.sidebar_bg)),
            Span::styled(
                LABEL,
                if field_focused(SearchField::Query) {
                    Style::default()
                        .fg(color(theme.ui.accent_light))
                        .bg(color(theme.ui.sidebar_bg))
                        .add_modifier(Modifier::BOLD)
                } else {
                    on(theme.ui.fg_faint, theme.ui.sidebar_bg)
                },
            ),
            Span::styled(pad("", gap), on(theme.ui.fg_faint, theme.ui.sidebar_bg)),
            Span::styled("Aa", toggle_style(app.search.case_sensitive)),
            Span::styled(" ", on(theme.ui.fg_faint, theme.ui.sidebar_bg)),
            Span::styled("ab", toggle_style(app.search.whole_word)),
            Span::styled(" ", on(theme.ui.fg_faint, theme.ui.sidebar_bg)),
            Span::styled(".*", toggle_style(app.search.regex)),
            Span::styled(" ", on(theme.ui.fg_faint, theme.ui.sidebar_bg)),
        ])),
        options,
    );
    y += 1;

    // Inputs are inset boxes on the editor background, so each one reads as a
    // field against the sidebar — the contrast the window's text boxes get.
    let inset = |y: u16| Rect {
        x: area.x + 1,
        y,
        width: area.width.saturating_sub(2),
        height: 1,
    };
    let ellipsis = app.icons.ellipsis;
    let draw_field = |frame: &mut Frame, which: SearchField, at: Rect| {
        let focused = field_focused(which);
        let text = app.search.input(which);
        let empty = text.is_empty();
        let shown = if empty { ellipsis } else { text };
        let inner = at.width.saturating_sub(1) as usize;
        let text_style = if empty {
            on(theme.ui.fg_faint, theme.ui.editor_bg)
        } else if focused {
            on(theme.ui.fg_bright, theme.ui.editor_bg)
        } else {
            on(theme.ui.fg, theme.ui.editor_bg)
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                // Just padding: the lit heading above is what marks the field
                // the keyboard is in.
                Span::styled(" ", on(theme.ui.fg_faint, theme.ui.editor_bg)),
                Span::styled(pad(&clip(shown, inner), inner), text_style),
            ])),
            at,
        );
    };

    let query_area = inset(y);
    app.layout.search_input = query_area;
    draw_field(frame, SearchField::Query, query_area);
    y += 2; // a blank row separates each label-and-field pair

    heading(
        frame,
        "REPLACE",
        field_focused(SearchField::Replace),
        row(y),
    );
    y += 1;
    let replace_area = inset(y);
    app.layout.search_replace = replace_area;
    draw_field(frame, SearchField::Replace, replace_area);
    y += 2;

    heading(
        frame,
        "EXCLUDE",
        field_focused(SearchField::Exclude),
        row(y),
    );
    y += 1;
    let exclude_area = inset(y);
    app.layout.search_exclude = exclude_area;
    draw_field(frame, SearchField::Exclude, exclude_area);
    y += 1;
    // The example goes in the gap row that separates this pair from the next.
    if let Some(example) = SearchField::Exclude.example() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                pad(&clip(&format!("  {example}"), width), width),
                on(theme.ui.fg_faint, theme.ui.sidebar_bg),
            ))),
            row(y),
        );
    }
    y += 1;

    // Summary line, carrying any pattern error.
    let summary = match (&app.search.error, app.search.query.is_empty()) {
        (Some(error), _) => error.clone(),
        (None, true) => String::new(),
        (None, false) => format!(
            "{}{} results in {} files",
            app.search.total_matches(),
            if app.search.truncated { "+" } else { "" },
            app.search.results.len()
        ),
    };
    let tone = if app.search.error.is_some() {
        theme.ui.danger
    } else {
        theme.ui.fg_faint
    };
    let summary_row = row(y);
    let action = "Replace All";
    let show_action = !app.search.results.is_empty();
    if show_action {
        let left = format!(" {}", clip(&summary, width.saturating_sub(action.len() + 3)));
        let pad_width = width.saturating_sub(left.chars().count() + action.len() + 1);
        app.layout.search_action = Rect {
            x: summary_row.x + (left.chars().count() + pad_width) as u16,
            width: action.len() as u16,
            ..summary_row
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(left, on(tone, theme.ui.sidebar_bg)),
                Span::styled(pad("", pad_width), on(tone, theme.ui.sidebar_bg)),
                Span::styled(action.to_string(), on(theme.ui.fg, theme.ui.sidebar_bg)),
                Span::styled(" ", on(tone, theme.ui.sidebar_bg)),
            ])),
            summary_row,
        );
    } else {
        app.layout.search_action = Rect::default();
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                pad(&format!(" {summary}"), width),
                on(tone, theme.ui.sidebar_bg),
            ))),
            summary_row,
        );
    }
    y += 1;

    let used = y - area.y;
    let list_area = Rect {
        y,
        height: area.height.saturating_sub(used),
        ..area
    };
    let width = list_area.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut row_map: Vec<Option<usize>> = Vec::new();
    let mut flat_index = 0usize;
    for file in &app.search.results {
        let rel = app.project.display(&file.path);
        // File heading with the same disclosure triangle the window shows.
        let heading = format!("{} {}", app.icons.dir_open, trim_front(&rel, width.saturating_sub(2)));
        lines.push(Line::from(Span::styled(
            pad(&heading, width),
            on(theme.ui.accent_light, theme.ui.sidebar_bg),
        )));
        row_map.push(None);
        for m in &file.matches {
            let selected = flat_index == app.search_selected;
            let bg = if selected {
                theme.ui.selected_bg
            } else {
                theme.ui.sidebar_bg
            };
            // Number, context and the match itself, which keeps its own
            // background exactly as in the window.
            let number = format!("{:>5}  ", m.line);
            let mut used = number.chars().count();
            let mut spans = vec![Span::styled(
                number,
                on(theme.ui.fg_faint, bg),
            )];
            for (text, highlight) in [
                (m.prefix.as_str(), false),
                (m.matched.as_str(), true),
                (m.suffix.as_str(), false),
            ] {
                if used >= width {
                    break;
                }
                let piece = clip(text, width - used);
                used += piece.chars().count();
                let fg = if selected {
                    theme.ui.fg_bright
                } else {
                    theme.ui.fg_dim
                };
                spans.push(Span::styled(
                    piece,
                    if highlight {
                        on(theme.ui.fg, theme.ui.match_bg)
                    } else {
                        on(fg, bg)
                    },
                ));
            }
            if used < width {
                spans.push(Span::styled(pad("", width - used), on(theme.ui.fg_dim, bg)));
            }
            lines.push(Line::from(spans));
            row_map.push(Some(flat_index));
            flat_index += 1;
        }
    }
    // Keep the selected match on screen.
    let selected_row = lines
        .len()
        .min(app.search_selected + app.search.results.len() + 1);
    let height = list_area.height as usize;
    let offset = selected_row.saturating_sub(height);
    let visible: Vec<Line> = lines.into_iter().skip(offset).take(height).collect();
    app.layout.search_list = list_area;
    app.layout.search_rows = row_map.into_iter().skip(offset).take(height).collect();
    frame.render_widget(Paragraph::new(visible), list_area);
}

/// The find-in-file bar: query, option toggles, the match counter with arrows
/// beside it, and a close button — plus a replace row when it is open. Every
/// piece is clickable, so its hit boxes are recorded as it is laid out.
/// Find in this file, drawn as the project search panel's form: a lit heading
/// over each inset field, both fields always present, and the counter and the
/// actions on one line under them.
fn draw_find(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme().clone();
    let width = area.width as usize;
    let focused = app.focus == Focus::Find;
    let in_query = !app.find.in_replace_field;
    let plain = on(theme.ui.fg_faint, theme.ui.status_bg);

    frame.render_widget(
        Block::default().style(on(theme.ui.fg, theme.ui.status_bg)),
        area,
    );
    let row = |y: u16| Rect {
        y,
        height: 1,
        ..area
    };
    let mut y = area.y;

    // Heading row: FIND on the left, the option toggles at the right edge.
    let heading_style = |active: bool| {
        if focused && active {
            Style::default()
                .fg(color(theme.ui.accent_light))
                .bg(color(theme.ui.status_bg))
                .add_modifier(Modifier::BOLD)
        } else {
            on(theme.ui.fg_faint, theme.ui.status_bg)
        }
    };
    let toggle_style = |enabled: bool| {
        if enabled {
            Style::default()
                .fg(color(theme.ui.fg_bright))
                .bg(color(theme.ui.selected_bg))
        } else {
            on(theme.ui.fg_dim, theme.ui.status_bg)
        }
    };
    const LABEL: &str = "FIND";
    // Aa ab .* then the close mark, each with a space after it.
    let toggles_width = 2 + 1 + 2 + 1 + 2 + 1 + 1 + 1;
    let gap = width.saturating_sub(1 + LABEL.len() + toggles_width);
    let toggles_x = area.x + (1 + LABEL.len() + gap) as u16;
    app.layout.find_case = Rect {
        x: toggles_x,
        y,
        width: 2,
        height: 1,
    };
    app.layout.find_word = Rect {
        x: toggles_x + 3,
        ..app.layout.find_case
    };
    app.layout.find_regex = Rect {
        x: toggles_x + 6,
        ..app.layout.find_case
    };
    app.layout.find_close = Rect {
        x: toggles_x + 9,
        width: 1,
        ..app.layout.find_case
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", plain),
            Span::styled(LABEL, heading_style(in_query)),
            Span::styled(pad("", gap), plain),
            Span::styled("Aa", toggle_style(app.find.case_sensitive)),
            Span::styled(" ", plain),
            Span::styled("ab", toggle_style(app.find.whole_word)),
            Span::styled(" ", plain),
            Span::styled(".*", toggle_style(app.find.regex)),
            Span::styled(" ", plain),
            Span::styled("×", on(theme.ui.fg, theme.ui.status_bg)),
            Span::styled(" ", plain),
        ])),
        row(y),
    );
    y += 1;

    // Inset fields on the editor background, exactly as in the search panel.
    let inset = |y: u16| Rect {
        x: area.x + 1,
        y,
        width: area.width.saturating_sub(2),
        height: 1,
    };
    let ellipsis = app.icons.ellipsis;
    let draw_field = |frame: &mut Frame, text: &str, active: bool, at: Rect| {
        let empty = text.is_empty();
        let shown = if empty { ellipsis } else { text };
        let inner = at.width.saturating_sub(1) as usize;
        let text_style = if empty {
            on(theme.ui.fg_faint, theme.ui.editor_bg)
        } else if focused && active {
            on(theme.ui.fg_bright, theme.ui.editor_bg)
        } else {
            on(theme.ui.fg, theme.ui.editor_bg)
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ", on(theme.ui.fg_faint, theme.ui.editor_bg)),
                Span::styled(pad(&clip(shown, inner), inner), text_style),
            ])),
            at,
        );
    };

    let query_area = inset(y);
    app.layout.find_query = query_area;
    draw_field(frame, &app.find.query, in_query, query_area);
    y += 1;

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", plain),
            Span::styled("REPLACE", heading_style(!in_query)),
            Span::styled(pad("", width.saturating_sub(8)), plain),
        ])),
        row(y),
    );
    y += 1;
    let replace_area = inset(y);
    app.layout.find_replace = replace_area;
    draw_field(frame, &app.find.replace, !in_query, replace_area);
    y += 1;

    // Counter on the left, the actions on the right — the panel's summary row.
    let summary = app.find.summary();
    let tone = if app.find.error.is_some() {
        theme.ui.danger
    } else {
        theme.ui.fg_faint
    };
    let action_row = row(y);
    let mut spans: Vec<Span> = Vec::new();
    let mut x = area.x;
    let place = |spans: &mut Vec<Span>, x: &mut u16, text: String, style: Style| -> Rect {
        let rect = Rect {
            x: *x,
            y: action_row.y,
            width: text.chars().count() as u16,
            height: 1,
        };
        *x += rect.width;
        spans.push(Span::styled(text, style));
        rect
    };
    let show_replace = !app.find.replace.is_empty();
    let mut actions: Vec<&str> = Vec::new();
    if show_replace {
        actions.push("Replace");
        actions.push("Replace All");
    }
    actions.push("<");
    actions.push(">");
    let actions_width: usize = actions.iter().map(|a| a.chars().count() + 2).sum();
    let left = format!(" {}", clip(&summary, width.saturating_sub(actions_width + 2)));
    let lead = width.saturating_sub(left.chars().count() + actions_width);
    place(&mut spans, &mut x, left, on(tone, theme.ui.status_bg));
    place(&mut spans, &mut x, pad("", lead), plain);
    let action = on(theme.ui.fg, theme.ui.status_bg);
    if show_replace {
        place(&mut spans, &mut x, " ".into(), plain);
        app.layout.find_replace_one = place(&mut spans, &mut x, "Replace".into(), action);
        place(&mut spans, &mut x, "  ".into(), plain);
        app.layout.find_replace_all = place(&mut spans, &mut x, "Replace All".into(), action);
        place(&mut spans, &mut x, " ".into(), plain);
    } else {
        app.layout.find_replace_one = Rect::default();
        app.layout.find_replace_all = Rect::default();
        place(&mut spans, &mut x, " ".into(), plain);
    }
    place(&mut spans, &mut x, " ".into(), plain);
    app.layout.find_prev = place(&mut spans, &mut x, "<".into(), action);
    place(&mut spans, &mut x, "  ".into(), plain);
    app.layout.find_next = place(&mut spans, &mut x, ">".into(), action);
    place(&mut spans, &mut x, " ".into(), plain);
    frame.render_widget(Paragraph::new(Line::from(spans)), action_row);
}

/// A changed file side by side: the old version on the left, the new on the
/// right, changed lines washed in the theme's own red and green.
fn draw_diff(frame: &mut Frame, app: &mut App, whole: Rect) {
    let theme = app.theme().clone();
    let focused = app.focus == Focus::Diff;
    let header = Rect {
        height: 1,
        ..whole
    };
    let area = Rect {
        y: whole.y + 1,
        height: whole.height.saturating_sub(1),
        ..whole
    };
    let Some(diff) = app.active_diff() else { return };

    let added = core_theme::ansi256(&theme, 2);
    let removed = core_theme::ansi256(&theme, 1);
    let counts = diff
        .rows
        .iter()
        .fold((0usize, 0usize), |(plus, minus), row| match row.kind {
            diff::Kind::Added => (plus + 1, minus),
            diff::Kind::Removed => (plus, minus + 1),
            diff::Kind::Changed => (plus + 1, minus + 1),
            diff::Kind::Same => (plus, minus),
        });
    let title = format!(" {} ", diff.path);
    let width = header.width as usize;
    let tail = format!("+{}  -{}   esc close · ⏎ open file ", counts.0, counts.1);
    let gap = width.saturating_sub(title.chars().count() + tail.chars().count());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                if focused {
                    Style::default()
                        .fg(color(theme.ui.fg_bright))
                        .bg(color(theme.ui.status_bg))
                        .add_modifier(Modifier::BOLD)
                } else {
                    on(theme.ui.fg_dim, theme.ui.status_bg)
                },
            ),
            Span::styled(pad("", gap), on(theme.ui.fg_faint, theme.ui.status_bg)),
            Span::styled(tail, on(theme.ui.fg_faint, theme.ui.status_bg)),
        ])),
        header,
    );

    frame.render_widget(
        Block::default().style(on(theme.ui.fg, theme.ui.editor_bg)),
        area,
    );
    let half = (area.width / 2) as usize;
    let number_width = 5usize;
    let text_width = half.saturating_sub(number_width + 2);
    let lines: Vec<Line> = diff
        .rows
        .iter()
        .skip(diff.scroll)
        .take(area.height as usize)
        .map(|row| {
            let side = |cell: Option<&diff::Side>, tint: Option<(u8, u8, u8)>| {
                let bg = match tint {
                    Some(rgb) => wash(rgb, theme.ui.editor_bg),
                    None => theme.ui.editor_bg,
                };
                match cell {
                    Some(cell) => vec![
                        Span::styled(
                            format!("{:>width$} ", cell.line, width = number_width),
                            on(tint.unwrap_or(theme.ui.line_number), bg),
                        ),
                        Span::styled(
                            pad(&clip(&cell.text, text_width), text_width + 1),
                            on(theme.ui.fg, bg),
                        ),
                    ],
                    // The blank half beside an added or removed line.
                    None => vec![Span::styled(
                        pad("", number_width + text_width + 2),
                        on(theme.ui.fg_faint, theme.ui.sidebar_bg),
                    )],
                }
            };
            let (left_tint, right_tint) = match row.kind {
                diff::Kind::Same => (None, None),
                diff::Kind::Changed => (Some(removed), Some(added)),
                diff::Kind::Added => (None, Some(added)),
                diff::Kind::Removed => (Some(removed), None),
            };
            let mut spans = side(row.left.as_ref(), left_tint);
            spans.push(Span::styled(
                "│",
                on(theme.ui.border, theme.ui.editor_bg),
            ));
            spans.extend(side(row.right.as_ref(), right_tint));
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// A changed line's background: the tint laid thinly over the editor's own.
fn wash(tint: (u8, u8, u8), base: (u8, u8, u8)) -> (u8, u8, u8) {
    let mix = |t: u8, b: u8| ((t as u16 * 22 + b as u16 * 78) / 100) as u8;
    (mix(tint.0, base.0), mix(tint.1, base.1), mix(tint.2, base.2))
}

/// The tab strip over the editor: the open files, then any open diffs.
fn draw_tab_strip(frame: &mut Frame, app: &mut App, tab_area: Rect) {
    let theme = app.theme().clone();
    app.layout.tabs = tab_area;
    let icons = app.icons;
    let hover = app.mouse.filter(|(_, y)| *y == tab_area.y).map(|(x, _)| x);
    let mut spans: Vec<Span> = Vec::new();
    let mut tab_spans: Vec<(u16, u16, usize, bool)> = Vec::new();
    let mut x = tab_area.x;
    let drop_on = match app.tab_drag {
        Some((TabStrip::Editor, _)) => app.tab_drag_over,
        _ => None,
    };
    for (i, buf) in app.buffers.list.iter().enumerate() {
        let selected = i == app.buffers.active && app.active_diff.is_none();
        let style = if drop_on == Some(i) {
            on(theme.ui.fg_bright, theme.ui.accent)
        } else if selected {
            Style::default()
                .fg(color(theme.ui.fg_bright))
                .bg(color(theme.ui.tab_active_bg))
                .add_modifier(Modifier::BOLD)
        } else {
            on(theme.ui.fg_dim, theme.ui.tab_inactive_bg)
        };
        let bg = if drop_on == Some(i) {
            theme.ui.accent
        } else if selected {
            theme.ui.tab_active_bg
        } else {
            theme.ui.tab_inactive_bg
        };
        let label = format!(" {} ", buf.name());
        let label_width = label.chars().count() as u16;
        // The marker doubles as the close button: a dot while unsaved, a cross
        // once the pointer is over the tab — the same swap the GUI does.
        let over_tab = hover.is_some_and(|hx| hx >= x && hx < x + label_width + 2);
        let marker = if buf.modified() && !over_tab {
            icons.modified
        } else {
            icons.close
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::styled(
            format!("{marker} "),
            on(
                if over_tab {
                    theme.ui.fg_bright
                } else {
                    theme.ui.fg_faint
                },
                bg,
            ),
        ));
        tab_spans.push((x, x + label_width, i, false));
        tab_spans.push((x + label_width, x + label_width + 2, i, true));
        x += label_width + 2;
    }
    // Diffs are tabs too, after the files, marked so the two never read alike.
    let mut diff_spans: Vec<(u16, u16, usize, bool)> = Vec::new();
    for (i, diff) in app.diffs.iter().enumerate() {
        let selected = app.active_diff == Some(i);
        let name = diff.path.rsplit('/').next().unwrap_or(&diff.path);
        let label = format!(" ≠ {name} ");
        let label_width = label.chars().count() as u16;
        let (fg, bg) = if selected {
            (theme.ui.fg_bright, theme.ui.tab_active_bg)
        } else {
            (theme.ui.fg_dim, theme.ui.tab_inactive_bg)
        };
        spans.push(Span::styled(label, on(fg, bg)));
        spans.push(Span::styled(
            format!("{} ", icons.close),
            on(theme.ui.fg_faint, bg),
        ));
        diff_spans.push((x, x + label_width, i, false));
        diff_spans.push((x + label_width, x + label_width + 2, i, true));
        x += label_width + 2;
    }
    app.layout.tab_spans = tab_spans;
    app.layout.diff_tabs = diff_spans;
    if spans.is_empty() {
        // No tab strip at all when nothing is open, as in the window.
        frame.render_widget(
            Block::default().style(on(theme.ui.fg, theme.ui.editor_bg)),
            tab_area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(on(theme.ui.fg_dim, theme.ui.status_bg)),
            tab_area,
        );
    }
}

fn draw_editor(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme().clone();
    frame.render_widget(
        Block::default().style(on(theme.ui.fg, theme.ui.editor_bg)),
        area,
    );
    let text_area = area;
    app.layout.editor = text_area;
    if app.buffers.is_empty() {
        draw_start_page(frame, app, text_area);
        return;
    }

    let height = text_area.height as usize;
    let (cursor_line, cursor_col) = {
        let state = app.edit_state();
        (state.line, state.col)
    };

    // Folded blocks drop out of the display, so scrolling counts visible rows.
    let visible = app.visible_lines();
    let cursor_row = visible
        .binary_search(&cursor_line)
        .unwrap_or_else(|insert| insert.min(visible.len().saturating_sub(1)));
    {
        let state = app.edit_state();
        if state.scroll > cursor_row {
            state.scroll = cursor_row;
        } else if height > 0 && cursor_row >= state.scroll + height {
            state.scroll = cursor_row + 1 - height;
        }
        if state.scroll >= visible.len() {
            state.scroll = visible.len().saturating_sub(1);
        }
    }
    let scroll = app.edit_state().scroll;
    let gutter = 7usize;
    let text_width = text_area.width.saturating_sub(gutter as u16) as usize;
    app.layout.editor = text_area;
    app.layout.gutter = gutter as u16;

    // Sticky scroll: the headers enclosing the first visible line are pinned at
    // the top so the function you are inside stays on screen.
    let first_visible = visible.get(scroll).copied().unwrap_or(0);
    let sticky: Vec<usize> = fold::context(&app.regions, first_visible, 3)
        .into_iter()
        .filter(|header| *header < first_visible)
        .collect();
    let sticky_rows = sticky.len().min(height.saturating_sub(1));
    let sticky = &sticky[..sticky_rows];

    // Selection is a character range over the whole buffer; each line needs to
    // know where it starts to paint its share of it.
    let selection = app.selection();
    let show_hits = app.find_showing() && !app.find.hits.is_empty();
    let mut line_starts: Vec<usize> = Vec::with_capacity(app.highlight.len() + 1);
    if selection.is_some() || show_hits {
        let mut at = 0usize;
        for regions in &app.highlight {
            line_starts.push(at);
            at += regions
                .iter()
                .map(|(_, _, text)| text.chars().count())
                .sum::<usize>()
                + 1;
        }
        line_starts.push(at);
    }

    let render_line = |n: usize, row_style: Option<ratatui::style::Style>| -> Line<'static> {
        let folded = app.is_folded(n);
        let foldable = fold::region_at(&app.regions, n).is_some();
        let marker = if !foldable {
            " "
        } else if folded {
            app.icons.dir_closed
        } else {
            app.icons.dir_open
        };
        let number_style = row_style.unwrap_or_else(|| {
            on(
                if n == cursor_line {
                    theme.ui.fg
                } else {
                    theme.ui.line_number
                },
                theme.ui.editor_bg,
            )
        });
        let git_mark = app.git_lines.get(&(n + 1)).map(|state| match state {
            core_git::LineState::Added => ("\u{2502}", core_theme::ansi256(&theme, 2)),
            core_git::LineState::Modified => ("\u{2502}", core_theme::ansi256(&theme, 3)),
            core_git::LineState::Removed => ("\u{2577}", core_theme::ansi256(&theme, 1)),
        });
        let mut spans = vec![
            Span::styled(
                match git_mark {
                    Some((mark, _)) => mark.to_string(),
                    None => " ".to_string(),
                },
                match git_mark {
                    Some((_, tint)) => on(tint, theme.ui.editor_bg),
                    None => on(theme.ui.editor_bg, theme.ui.editor_bg),
                },
            ),
            Span::styled(format!("{:>4} ", n + 1), number_style),
            Span::styled(
                marker.to_string(),
                row_style.unwrap_or_else(|| on(theme.ui.fg_faint, theme.ui.editor_bg)),
            ),
        ];
        let mut col = 0usize;
        let link = app.link.filter(|(line, _, _)| *line == n);
        // Find hits on this line, as column ranges, with the current one
        // marked so it stands out from the rest.
        let line_range = line_starts
            .get(n)
            .zip(line_starts.get(n + 1))
            .map(|(a, b)| (*a, *b));
        let hits: Vec<(usize, usize, bool)> = match (show_hits, line_range) {
            (true, Some((start, end))) => app
                .find
                .hits
                .iter()
                .enumerate()
                .filter(|(_, h)| h.start < end && h.end > start)
                .map(|(i, h)| {
                    (
                        h.start.saturating_sub(start),
                        (h.end - start).min(end - start),
                        i == app.find.current,
                    )
                })
                .collect(),
            _ => Vec::new(),
        };

        // The part of the selection that falls on this line, in columns.
        let selected = selection.and_then(|(from, to)| {
            let start = *line_starts.get(n)?;
            let end = *line_starts.get(n + 1)?;
            (from < end && to > start).then(|| {
                (
                    from.saturating_sub(start),
                    (to - start).min(end - start),
                )
            })
        });
        if let Some(regions) = app.highlight.get(n) {
            for (rgb, italic, text) in regions {
                if col >= text_width {
                    break;
                }
                let mut style = row_style.unwrap_or_else(|| on(*rgb, theme.ui.editor_bg));
                if *italic {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                let piece: String = text.chars().take(text_width - col).collect();
                let len = piece.chars().count();
                let touched = selected.is_some_and(|(s, e)| col < e && col + len > s);
                let underlined = link.is_some_and(|(_, s, e)| col < e && col + len > s);
                let hit_here = hits.iter().any(|(s, e, _)| col < *e && col + len > *s);
                if touched || underlined || hit_here {
                    for (i, ch) in piece.chars().enumerate() {
                        let at = col + i;
                        let mut style = style;
                        if let Some((_, _, current)) =
                            hits.iter().find(|(s, e, _)| at >= *s && at < *e)
                        {
                            style = style.bg(color(if *current {
                                theme.ui.selected_bg
                            } else {
                                theme.ui.match_bg
                            }));
                        }
                        if selected.is_some_and(|(s, e)| at >= s && at < e) {
                            style = style.bg(color(theme.ui.selection));
                        }
                        if link.is_some_and(|(_, s, e)| at >= s && at < e) {
                            style = on(theme.ui.accent_light, theme.ui.editor_bg)
                                .add_modifier(Modifier::UNDERLINED);
                        }
                        spans.push(Span::styled(ch.to_string(), style));
                    }
                } else {
                    spans.push(Span::styled(piece, style));
                }
                col += len;
            }
        }
        // A selection running past the end of the line shows on the newline.
        if selected.is_some_and(|(s, e)| e > col && col >= s) && col < text_width {
            spans.push(Span::styled(
                " ",
                on(theme.ui.fg, theme.ui.selection),
            ));
        }
        if folded {
            // A collapsed block shows how much it swallowed.
            let count = fold::region_at(&app.regions, n)
                .map(|r| r.end - r.start)
                .unwrap_or(0);
            let unit = if count == 1 { "line" } else { "lines" };
            spans.push(Span::styled(
                format!("  {} {count} {unit}", app.icons.file),
                on(theme.ui.fg_faint, theme.ui.selected_bg),
            ));
        }
        Line::from(spans)
    };

    let sticky_style = on(theme.ui.fg, theme.ui.status_bg);
    let mut lines: Vec<Line> = sticky
        .iter()
        .map(|n| render_line(*n, Some(sticky_style)))
        .collect();
    lines.extend(
        visible
            .iter()
            .skip(scroll + sticky_rows)
            .take(height.saturating_sub(sticky_rows))
            .map(|n| render_line(*n, None)),
    );
    frame.render_widget(Paragraph::new(lines), text_area);

    // Where the cursor lands on screen, accounting for hidden and pinned rows.
    let cursor_screen_row = cursor_row
        .checked_sub(scroll)
        .map(|offset| offset.max(sticky_rows));

    if app.focus == Focus::Editor {
        if let Some(row) = cursor_screen_row {
            let cx = text_area.x + gutter as u16 + cursor_col.min(text_width) as u16;
            let cy = text_area.y + row as u16;
            if cy < text_area.y + text_area.height {
                frame.set_cursor_position((cx, cy));
            }
        }
    }
}

fn draw_shell(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme().clone();
    let focused = app.focus == Focus::Shell;
    app.layout.shell = area;
    frame.render_widget(
        Block::default().style(on(theme.ui.fg, theme.ui.editor_bg)),
        area,
    );

    // Spawn the first shell before the header is drawn, so the tab strip can
    // already list it.
    let cwd = app.project.root_or_cwd();
    app.shell.ensure(&cwd);

    // Panel header: the label, one numbered tab per session with a close
    // mark, and `+` to open another — the same strip the window draws.
    let mut spans: Vec<Span> = Vec::new();
    let mut tabs: Vec<(u16, u16, usize, bool)> = Vec::new();
    let mut x = area.x;
    spans.push(Span::styled(
        " TERMINAL ",
        if focused {
            Style::default()
                .fg(color(theme.ui.accent_light))
                .bg(color(theme.ui.status_bg))
                .add_modifier(Modifier::BOLD)
        } else {
            on(theme.ui.fg_faint, theme.ui.status_bg)
        },
    ));
    x += 10;
    let active = app.shell.sessions.active_index();
    let drop_on = match app.tab_drag {
        Some((TabStrip::Terminal, _)) => app.tab_drag_over,
        _ => None,
    };
    for i in 0..app.shell.sessions.len() {
        let label = format!(" {} ", app.shell.sessions.name(i));
        let width = label.chars().count() as u16;
        let tab_style = if drop_on == Some(i) {
            on(theme.ui.fg_bright, theme.ui.accent)
        } else if i == active {
            on(theme.ui.fg, theme.ui.tab_active_bg)
        } else {
            on(theme.ui.fg_dim, theme.ui.status_bg)
        };
        spans.push(Span::styled(label, tab_style));
        tabs.push((x, x + width, i, false));
        x += width;
        spans.push(Span::styled(
            "× ",
            if drop_on == Some(i) {
                on(theme.ui.fg_bright, theme.ui.accent)
            } else if i == active {
                on(theme.ui.fg_dim, theme.ui.tab_active_bg)
            } else {
                on(theme.ui.fg_faint, theme.ui.status_bg)
            },
        ));
        tabs.push((x, x + 2, i, true));
        x += 2;
    }
    spans.push(Span::styled(" + ", on(theme.ui.fg_dim, theme.ui.status_bg)));
    app.layout.shell_new = Rect {
        x,
        y: area.y,
        width: 3.min(area.width.saturating_sub(x - area.x)),
        height: 1,
    };
    app.layout.shell_tabs = tabs;
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(on(theme.ui.fg_faint, theme.ui.status_bg)),
        Rect { height: 1, ..area },
    );

    let grid = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(1),
        ..area
    };
    if grid.height == 0 || grid.width == 0 {
        return;
    }

    let Some(pty) = app.shell.ensure(&cwd) else {
        let message = app
            .shell
            .error()
            .cloned()
            .unwrap_or_else(|| "terminal unavailable".to_string());
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" terminal failed: {message}"),
                on(theme.ui.danger, theme.ui.editor_bg),
            ))),
            grid,
        );
        return;
    };
    pty.resize(grid.height, grid.width);

    // Paint the shell's screen cell by cell, merging runs that share a style.
    let (lines, cursor) = pty.with_screen(|screen| {
        let mut lines: Vec<Line> = Vec::with_capacity(grid.height as usize);
        for row in 0..grid.height {
            let mut spans: Vec<Span> = Vec::new();
            let mut run = String::new();
            let mut run_style: Option<Style> = None;
            for col in 0..grid.width {
                let (text, style) = match screen.cell(row, col) {
                    Some(cell) => {
                        let contents = cell.contents();
                        let text = if contents.is_empty() {
                            " ".to_string()
                        } else {
                            contents
                        };
                        (text, cell_style(cell, &theme))
                    }
                    None => (" ".to_string(), on(theme.ui.terminal_fg, theme.ui.editor_bg)),
                };
                match run_style {
                    Some(current) if current == style => run.push_str(&text),
                    _ => {
                        if let Some(current) = run_style.take() {
                            spans.push(Span::styled(std::mem::take(&mut run), current));
                        }
                        run = text;
                        run_style = Some(style);
                    }
                }
            }
            if let Some(current) = run_style {
                spans.push(Span::styled(run, current));
            }
            lines.push(Line::from(spans));
        }
        let cursor = (!screen.hide_cursor() && screen.scrollback() == 0)
            .then(|| screen.cursor_position());
        (lines, cursor)
    });

    frame.render_widget(Paragraph::new(lines), grid);
    if focused {
        if let Some((row, col)) = cursor {
            if row < grid.height && col < grid.width {
                frame.set_cursor_position((grid.x + col, grid.y + row));
            }
        }
    }
}

/// Maps one shell cell's colors and attributes onto a ratatui style.
fn cell_style(cell: &vt100::Cell, theme: &crate::core::theme::Theme) -> Style {
    let resolve = |c: vt100::Color, default: crate::core::theme::Rgb| match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => crate::core::theme::ansi256(theme, i),
        vt100::Color::Rgb(r, g, b) => (r, g, b),
    };
    let mut fg = resolve(cell.fgcolor(), theme.ui.terminal_fg);
    let mut bg = resolve(cell.bgcolor(), theme.ui.editor_bg);
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    let mut style = on(fg, bg);
    if cell.bold() {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.italic() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.underline() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

fn draw_status(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme().clone();
    let width = area.width as usize;

    let mut left = match app.buffers.active() {
        Some(buf) => {
            let mut text = app.project.display(&buf.path);
            if buf.modified() {
                text.push_str("  \u{25cf}");
            }
            text
        }
        None => String::new(),
    };
    if !app.status.is_empty() {
        if !left.is_empty() {
            left.push_str("   ");
        }
        left.push_str(&app.status);
    } else if let Some(blame) = &app.blame {
        // Who last touched the line the cursor is on.
        if !left.is_empty() {
            left.push_str("   ");
        }
        left.push_str(&blame.line());
    }

    // The theme name closes the row and is clickable, so it is kept apart.
    let right_prefix = match app.buffers.active() {
        Some(buf) => {
            let state = &app.edit[app.buffers.active];
            let lang = if buf.extension.is_empty() {
                "plain text".to_string()
            } else {
                buf.extension.clone()
            };
            format!("Ln {}, Col {}   {}   ", state.line + 1, state.col + 1, lang)
        }
        None => String::new(),
    };
    let name = theme.name.clone();
    let right_len = right_prefix.chars().count() + name.chars().count();

    let left = clip(&left, width.saturating_sub(right_len + 3));
    let gap = width
        .saturating_sub(left.chars().count() + right_len + 2)
        .max(1);

    let name_x = area.x + (1 + left.chars().count() + gap + right_prefix.chars().count()) as u16;
    let name_w = (name.chars().count() as u16).min((area.x + area.width).saturating_sub(name_x));
    app.layout.status_theme = Rect {
        x: name_x,
        y: area.y,
        width: name_w,
        height: 1,
    };
    let hovered = app
        .mouse
        .is_some_and(|(x, y)| y == area.y && x >= name_x && x < name_x + name_w);

    let base = on(theme.ui.fg_dim, theme.ui.status_bg);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                clip(&format!(" {left}{}{right_prefix}", " ".repeat(gap)), width),
                base,
            ),
            Span::styled(
                name,
                if hovered {
                    on(theme.ui.fg_bright, theme.ui.status_bg)
                } else {
                    base
                },
            ),
            Span::styled(" ", base),
        ])),
        area,
    );
}

fn draw_prompt(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.prompt.is_none() {
        app.layout.prompt_list = Rect::default();
        return;
    }
    let theme = app.theme().clone();
    let prompt = app.prompt.as_ref().unwrap();

    let list: Vec<String> = match prompt {
        Prompt::Themes => app.themes.iter().map(|t| t.name.clone()).collect(),
        Prompt::Recent => app
            .settings
            .recent_projects
            .iter()
            .map(|path| {
                // Home-relative, the way the paths were typed in.
                let text = path.display().to_string();
                match std::env::var("HOME") {
                    Ok(home) if !home.is_empty() && text.starts_with(&home) => {
                        format!("~{}", &text[home.len()..])
                    }
                    _ => text,
                }
            })
            .collect(),
        Prompt::Help(entries) => entries.clone(),
        Prompt::GitRepo => app
            .git
            .repos
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        Prompt::GitWorktree => app
            .git
            .worktrees
            .iter()
            .map(|w| {
                if w.branch.is_empty() {
                    w.name()
                } else {
                    format!("{} · {}", w.name(), w.branch)
                }
            })
            .collect(),
        Prompt::Browse { dir, entries, .. } => {
            let mut list: Vec<String> = Vec::new();
            if dir.parent().is_some() {
                list.push("..".to_string());
            }
            list.extend(entries.iter().map(|(path, is_dir)| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if *is_dir {
                    format!("{name}/")
                } else {
                    name
                }
            }));
            list
        }
        Prompt::Goto { candidates, .. } => candidates
            .iter()
            .map(|c| {
                format!(
                    "{}:{}  {}",
                    app.project.display(&c.path),
                    c.line,
                    c.text
                )
            })
            .collect(),
        _ => Vec::new(),
    };

    let width = (area.width as usize * 3 / 4).clamp(30, 100) as u16;
    let room = area.height.saturating_sub(8) as usize;
    let list_height = list.len().min(room.max(4)) as u16;
    // One row for the title, one per list entry, plus the border and any input.
    let height = 3 + list_height + if prompt.is_input() { 1 } else { 0 };
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 3,
        width,
        height: height.min(area.height),
    };
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(on(theme.ui.border, theme.ui.sidebar_bg))
        .style(on(theme.ui.fg, theme.ui.sidebar_bg));
    let inner_width = rect.width.saturating_sub(2) as usize;

    let mut lines = vec![Line::from(Span::styled(
        pad(&format!(" {}", prompt.title()), inner_width),
        Style::default()
            .fg(color(theme.ui.accent_light))
            .bg(color(theme.ui.sidebar_bg))
            .add_modifier(Modifier::BOLD),
    ))];
    if prompt.is_input() {
        lines.push(Line::from(Span::styled(
            pad(&format!(" > {}", app.prompt_input), inner_width),
            on(theme.ui.fg_bright, theme.ui.sidebar_bg),
        )));
    }
    // Scroll the list so the selection stays visible.
    let list_y = rect.y + 2 + u16::from(prompt.is_input());
    app.layout.prompt_list = Rect {
        x: rect.x + 1,
        y: list_y,
        width: rect.width.saturating_sub(2),
        height: list_height,
    };
    let offset = app
        .prompt_selected
        .saturating_sub(list_height.saturating_sub(1) as usize);
    for (i, item) in list
        .iter()
        .enumerate()
        .skip(offset)
        .take(list_height as usize)
    {
        let selected = i == app.prompt_selected;
        lines.push(Line::from(Span::styled(
            pad(&clip(&format!(" {item}"), inner_width), inner_width),
            on(
                if selected {
                    theme.ui.fg_bright
                } else {
                    theme.ui.fg_dim
                },
                if selected {
                    theme.ui.selected_bg
                } else {
                    theme.ui.sidebar_bg
                },
            ),
        )));
    }
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

fn pad(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        text.chars().take(width).collect()
    } else {
        format!("{text}{}", " ".repeat(width - len))
    }
}

fn clip(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

/// Keeps the tail of a path when it doesn't fit, which is the informative end.
fn trim_front(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len <= width {
        return text.to_string();
    }
    format!("...{}", text.chars().skip(len - width + 3).collect::<String>())
}


/// The right-click context menu, drawn last so it sits above everything.
fn draw_menu(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(menu) = &app.menu else {
        app.layout.menu = Rect::default();
        return;
    };
    let theme = app.theme().clone();
    let icons = app.icons;

    let width = menu.width().min(area.width);
    let height = menu.height().min(area.height);
    // Flip the menu back on-screen when opened near an edge.
    let x = menu.x.min(area.width.saturating_sub(width));
    let y = if menu.y + height > area.y + area.height {
        menu.y.saturating_sub(height)
    } else {
        menu.y
    };
    let rect = Rect {
        x,
        y,
        width,
        height,
    };

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(on(theme.ui.border, theme.ui.sidebar_bg))
        .style(on(theme.ui.fg, theme.ui.sidebar_bg));
    let inner_width = rect.width.saturating_sub(2) as usize;
    let lines: Vec<Line> = menu
        .rows()
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let Some(item) = row else {
                return Line::from(Span::styled(
                    "-".repeat(inner_width),
                    on(theme.ui.border, theme.ui.sidebar_bg),
                ));
            };
            let selected = i == menu.selected;
            let marker = if selected { icons.menu_marker } else { " " };
            let shortcut = menu.shortcut_at_row(i);
            let left = format!("{marker} {}", item.label());
            // The chord hint is right-aligned in the same row.
            let gap = inner_width
                .saturating_sub(left.chars().count() + shortcut.chars().count() + 1);
            let label = pad(
                &format!("{left}{}{shortcut} ", " ".repeat(gap)),
                inner_width,
            );
            Line::from(Span::styled(
                label,
                on(
                    if selected {
                        theme.ui.fg_bright
                    } else {
                        theme.ui.fg
                    },
                    if selected {
                        theme.ui.selected_bg
                    } else {
                        theme.ui.sidebar_bg
                    },
                ),
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).block(block), rect);
    app.layout.menu = rect;
}

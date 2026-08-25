//! Rendering for the terminal frontend.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use std::path::Path;

/// Rows the find bar takes: heading, field, heading, field, actions.
const FIND_ROWS: u16 = 5;
use crate::core::command::{Command, ALL, START_PAGE};
use crate::core::diff;
use crate::core::fold;
use crate::core::git as core_git;
use crate::core::search::Field as SearchField;
use crate::core::theme as core_theme;
use crate::tui::app::{App, Focus, GitRow, Prompt, SidebarView, Splitter, TabStrip};
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
    app.layout.sidebar = if app.show_sidebar {
        sidebar
    } else {
        Rect::default()
    };
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
        .constraints([
            Constraint::Length(1),
            Constraint::Length(find_rows),
            Constraint::Min(1),
        ])
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
    if app.active_preview().is_some() {
        draw_preview(frame, app, editor_area);
    } else if app.active_diff().is_some() {
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
    let name = " YCODE ";
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
        (
            SidebarView::Files,
            app.icons.nav_files,
            "FILES",
            Focus::Tree,
        ),
        (
            SidebarView::Search,
            app.icons.nav_search,
            "SEARCH",
            Focus::Search,
        ),
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
        None => format!(
            "{} changed",
            crate::core::count(app.git.changes.len(), "file")
        ),
    };
    let tone = if app.git.error.is_some() {
        theme.ui.danger
    } else {
        theme.ui.fg_faint
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            pad(
                &format!(" {}", clip(&summary, width.saturating_sub(1))),
                width,
            ),
            on(tone, theme.ui.sidebar_bg),
        ))),
        row(y),
    );
    y += 1;

    // The rows: a heading over what is staged, one over what is not, and
    // the changes under each. A file with both kinds of edit is in both.
    let mut rows: Vec<GitRow> = Vec::new();
    let staged: Vec<usize> = (0..app.git.changes.len())
        .filter(|i| app.git.changes[*i].staged())
        .collect();
    let unstaged: Vec<usize> = (0..app.git.changes.len())
        .filter(|i| app.git.changes[*i].unstaged())
        .collect();
    if !staged.is_empty() {
        rows.push(GitRow::Heading(format!("STAGED CHANGES  {}", staged.len())));
        rows.extend(staged.iter().map(|i| GitRow::Change {
            index: *i,
            staged: true,
        }));
    }
    if !unstaged.is_empty() {
        rows.push(GitRow::Heading(format!("CHANGES  {}", unstaged.len())));
        rows.extend(unstaged.iter().map(|i| GitRow::Change {
            index: *i,
            staged: false,
        }));
    }
    app.git_rows = rows;
    // The last row says which keys the panel answers, when there is room.
    let hint = " s stage/unstage · a stage all · c commit";
    let hint_rows = u16::from(bottom.saturating_sub(y) > 3 && !app.git_rows.is_empty());
    let list_area = Rect {
        y,
        height: bottom.saturating_sub(y).saturating_sub(hint_rows),
        ..area
    };
    app.layout.git_list = list_area;
    let height = list_area.height as usize;
    if height == 0 || app.git_rows.is_empty() {
        app.layout.git_list_offset = 0;
        return;
    }
    if app.git_selected >= app.git_rows.len()
        || matches!(app.git_rows[app.git_selected], GitRow::Heading(_))
    {
        app.git_selected = app
            .git_rows
            .iter()
            .position(|r| matches!(r, GitRow::Change { .. }))
            .unwrap_or(0);
    }
    let offset = app.git_selected.saturating_sub(height.saturating_sub(1));
    app.layout.git_list_offset = offset;
    let mark_x = list_area.x + list_area.width.saturating_sub(3);
    let lines: Vec<Line> = app
        .git_rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, row)| match row {
            GitRow::Heading(text) => Line::from(Span::styled(
                pad(&format!(" {text}"), width),
                on(theme.ui.fg_faint, theme.ui.sidebar_bg),
            )),
            GitRow::Change { index, staged } => {
                let change = &app.git.changes[*index];
                let selected = i == app.git_selected;
                let bg = if selected {
                    theme.ui.selected_bg
                } else {
                    theme.ui.sidebar_bg
                };
                let letter = change.letter();
                let path_width = width.saturating_sub(7);
                let row_y = list_area.y + (i - offset) as u16;
                let on_mark = mouse.is_some_and(|(mx, my)| my == row_y && mx >= mark_x);
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
                    Span::styled(
                        format!(" {} ", if *staged { "\u{2212}" } else { "+" }),
                        on(
                            if on_mark || selected {
                                theme.ui.fg_bright
                            } else {
                                theme.ui.fg_faint
                            },
                            bg,
                        ),
                    ),
                ])
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), list_area);
    if hint_rows == 1 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                pad(&clip(hint, width), width),
                on(theme.ui.fg_faint, theme.ui.sidebar_bg),
            ))),
            Rect {
                y: list_area.y + list_area.height,
                height: 1,
                ..area
            },
        );
    }
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
fn draw_start_page(frame: &mut Frame, app: &mut App, area: Rect) {
    app.layout.start_rows.clear();
    let theme = app.theme().clone();
    frame.render_widget(
        Block::default().style(on(theme.ui.fg, theme.ui.editor_bg)),
        area,
    );
    if area.width < 24 || area.height < 6 {
        return;
    }
    let chord = |command| app.settings.tui_chord(command).map(|c| c.to_string());

    // One block per group: its heading, then a line per bound key.
    let mut blocks: Vec<Vec<(String, String, Option<Command>)>> = Vec::new();
    for (name, commands) in START_PAGE {
        let mut rows: Vec<(String, String, Option<Command>)> = Vec::new();
        for command in *commands {
            if let Some(chord) = chord(*command) {
                rows.push((chord, command.label().to_string(), Some(*command)));
            }
        }
        if rows.is_empty() {
            continue;
        }
        rows.insert(0, (String::new(), name.to_uppercase(), None));
        blocks.push(rows);
    }
    if blocks.is_empty() {
        return;
    }

    let chord_width = blocks
        .iter()
        .flatten()
        .map(|(chord, _, _)| chord.chars().count())
        .max()
        .unwrap_or(0);
    let column_width = blocks
        .iter()
        .flatten()
        .map(|(chord, label, _)| {
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
    let mouse = app.mouse;
    let mut grid: Vec<Vec<(Vec<Span>, Option<Command>)>> = Vec::new();
    for chunk in blocks.chunks(per_column) {
        let mut cells: Vec<(Vec<Span>, Option<Command>)> = Vec::new();
        for (i, rows) in chunk.iter().enumerate() {
            if i > 0 {
                cells.push((Vec::new(), None));
            }
            for (chord, label, command) in rows {
                if chord.is_empty() {
                    cells.push((vec![Span::styled(label.clone(), group)], None));
                } else {
                    cells.push((
                        vec![
                            Span::styled(format!("{chord:<chord_width$}  "), key),
                            Span::styled(label.clone(), plain),
                        ],
                        *command,
                    ));
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
            Span::styled("YARA CODE", title),
        ]));
        for (text, style) in header {
            lines.push(Line::from(vec![
                Span::styled(left.clone(), plain),
                Span::styled(text, style),
            ]));
        }
    }
    let header_rows = lines.len();
    for row in 0..body_height {
        let mut spans = vec![Span::styled(left.clone(), plain)];
        for (i, column) in grid.iter().enumerate() {
            let (mut cell, command) = column.get(row).cloned().unwrap_or_default();
            let used: usize = cell.iter().map(|s| s.content.chars().count()).sum();
            // Each row is the action it names: a click runs it, so the page
            // is a menu, not a manual. Where it lands is only known once the
            // block's top is, so the rect is filled in below.
            if let Some(command) = command {
                let x = area.x + (left.chars().count() + i * (column_width + GAP)) as u16;
                let rect = Rect {
                    x,
                    y: (header_rows + row) as u16,
                    width: used as u16,
                    height: 1,
                };
                app.layout.start_rows.push((rect, command));
                let hovered = mouse.is_some_and(|(mx, my)| {
                    my == rect.y && mx >= rect.x && mx < rect.x + rect.width
                });
                if hovered {
                    if let Some(last) = cell.last_mut() {
                        *last = Span::styled(last.content.to_string(), key);
                    }
                }
            }
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
    for (rect, _) in &mut app.layout.start_rows {
        rect.y += area.y + top as u16;
    }
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
        width: (toggles_width as u16).min((options.x + options.width).saturating_sub(toggles_x)),
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
    let summary = app.search.summary();
    let tone = if app.search.error.is_some() {
        theme.ui.danger
    } else {
        theme.ui.fg_faint
    };
    let summary_row = row(y);
    let action = "Replace All";
    let show_action = !app.search.results.is_empty();
    if show_action {
        let left = format!(
            " {}",
            clip(&summary, width.saturating_sub(action.len() + 3))
        );
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
        let heading = format!(
            "{} {}",
            app.icons.dir_open,
            trim_front(&rel, width.saturating_sub(2))
        );
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
            let mut spans = vec![Span::styled(number, on(theme.ui.fg_faint, bg))];
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
    let left = format!(
        " {}",
        clip(&summary, width.saturating_sub(actions_width + 2))
    );
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
    let header = Rect { height: 1, ..whole };
    let area = Rect {
        y: whole.y + 1,
        height: whole.height.saturating_sub(1),
        ..whole
    };
    // The whole pane, header included: a wheel anywhere over it scrolls.
    app.layout.viewer = whole;
    let Some(diff) = app.active_diff() else {
        return;
    };

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
            spans.push(Span::styled("│", on(theme.ui.border, theme.ui.editor_bg)));
            spans.extend(side(row.right.as_ref(), right_tint));
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// A markdown file as a reader sees it: headings in the accent, code on the
/// sidebar's background, lists with their bullets — one line of text per
/// drawn row, wrapped to the pane.
fn draw_preview(frame: &mut Frame, app: &mut App, whole: Rect) {
    use crate::core::markdown::{plain, Block, Span as Md};
    let theme = app.theme().clone();
    app.layout.viewer = whole;
    let Some(preview) = app.active_preview() else {
        return;
    };
    let width = whole.width.saturating_sub(4) as usize;

    let heading = |level: u8| {
        Style::default()
            .fg(color(theme.ui.accent_light))
            .bg(color(theme.ui.editor_bg))
            .add_modifier(if level <= 2 {
                Modifier::BOLD
            } else {
                Modifier::BOLD | Modifier::DIM
            })
    };
    let body = on(theme.ui.fg, theme.ui.editor_bg);
    let faint = on(theme.ui.fg_faint, theme.ui.editor_bg);
    let code = on(theme.ui.fg, theme.ui.sidebar_bg);

    // Inline spans, then wrapped into rows of `width`.
    let inline = |spans: &[Md], base: Style| -> Vec<Span<'static>> {
        spans
            .iter()
            .map(|s| match s {
                Md::Text(t) => Span::styled(t.clone(), base),
                Md::Bold(t) => Span::styled(t.clone(), base.add_modifier(Modifier::BOLD)),
                Md::Italic(t) => Span::styled(t.clone(), base.add_modifier(Modifier::ITALIC)),
                Md::Code(t) => Span::styled(format!(" {t} "), code),
                Md::Link(t, _) => Span::styled(
                    t.clone(),
                    base.fg(color(theme.ui.accent_light))
                        .add_modifier(Modifier::UNDERLINED),
                ),
            })
            .collect()
    };
    let wrap = |text: &str, indent: &str| -> Vec<String> {
        let mut rows = Vec::new();
        let mut line = String::new();
        for word in text.split_whitespace() {
            if !line.is_empty()
                && line.chars().count() + 1 + word.chars().count()
                    > width.saturating_sub(indent.len())
            {
                rows.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        if !line.is_empty() || rows.is_empty() {
            rows.push(line);
        }
        rows
    };

    let mut lines: Vec<Line> = Vec::new();
    for block in preview.blocks.iter().skip(preview.scroll) {
        match block {
            Block::Heading(level, spans) => {
                let text = plain(spans);
                let marker = if *level == 1 { "" } else { "  " };
                lines.push(Line::from(Span::styled(
                    format!("{marker}{text}"),
                    heading(*level),
                )));
                if *level == 1 {
                    lines.push(Line::from(Span::styled(
                        "═".repeat(text.chars().count().min(width)),
                        heading(1),
                    )));
                }
                lines.push(Line::from(""));
            }
            Block::Paragraph(spans) => {
                // Styled spans survive on the first row; wrapped rows carry the
                // text only, which keeps the styling honest where it matters.
                let text = plain(spans);
                let rows = wrap(&text, "  ");
                if rows.len() == 1 {
                    let mut styled = vec![Span::styled("  ", body)];
                    styled.extend(inline(spans, body));
                    lines.push(Line::from(styled));
                } else {
                    for row in rows {
                        lines.push(Line::from(Span::styled(format!("  {row}"), body)));
                    }
                }
                lines.push(Line::from(""));
            }
            Block::Code(language, text) => {
                if let Some(language) = language {
                    lines.push(Line::from(Span::styled(format!("  {language}"), faint)));
                }
                for row in text.lines() {
                    lines.push(Line::from(Span::styled(
                        pad(&format!("  {row}"), width + 2),
                        code,
                    )));
                }
                lines.push(Line::from(""));
            }
            Block::List(ordered, items) => {
                for (n, item) in items.iter().enumerate() {
                    let bullet = if *ordered {
                        format!("{}.", n + 1)
                    } else {
                        "•".to_string()
                    };
                    let rows = wrap(&plain(item), "     ");
                    for (r, row) in rows.into_iter().enumerate() {
                        let lead = if r == 0 {
                            format!("  {bullet} ")
                        } else {
                            "     ".to_string()
                        };
                        let mut styled = vec![Span::styled(lead, faint)];
                        if r == 0 && item.iter().any(|s| !matches!(s, Md::Text(_))) {
                            styled.extend(inline(item, body));
                        } else {
                            styled.push(Span::styled(row, body));
                        }
                        lines.push(Line::from(styled));
                    }
                }
                lines.push(Line::from(""));
            }
            Block::Quote(spans) => {
                for row in wrap(&plain(spans), "  │ ") {
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", on(theme.ui.accent_light, theme.ui.editor_bg)),
                        Span::styled(row, faint.add_modifier(Modifier::ITALIC)),
                    ]));
                }
                lines.push(Line::from(""));
            }
            Block::Rule => {
                lines.push(Line::from(Span::styled(
                    "  ".to_string() + &"─".repeat(width),
                    faint,
                )));
                lines.push(Line::from(""));
            }
        }
        if lines.len() > whole.height as usize + 1 {
            break;
        }
    }
    frame.render_widget(
        Paragraph::new(lines).style(on(theme.ui.fg, theme.ui.editor_bg)),
        whole,
    );
}

/// A changed line's background: the tint laid thinly over the editor's own.
fn wash(tint: (u8, u8, u8), base: (u8, u8, u8)) -> (u8, u8, u8) {
    let mix = |t: u8, b: u8| ((t as u16 * 22 + b as u16 * 78) / 100) as u8;
    (
        mix(tint.0, base.0),
        mix(tint.1, base.1),
        mix(tint.2, base.2),
    )
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
    let mut preview_spans: Vec<(u16, u16, usize, bool)> = Vec::new();
    for (i, preview) in app.previews.iter().enumerate() {
        let selected = app.active_preview == Some(i);
        let name = preview
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let label = format!(" ◫ {name} ");
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
        preview_spans.push((x, x + label_width, i, false));
        preview_spans.push((x + label_width, x + label_width + 2, i, true));
        x += label_width + 2;
    }
    // What stands at the right edge: a Preview button while a markdown file
    // is in front, and `‹ ›` when the tabs run past the strip.
    let markdown_in_front = app.active_diff.is_none()
        && app.active_preview.is_none()
        && app
            .buffers
            .active()
            .is_some_and(|buf| matches!(buf.extension.as_str(), "md" | "markdown"));
    let hint = markdown_in_front.then(|| match app.settings.tui_chord(Command::TogglePreview) {
        Some(chord) => format!(" ◫ Preview {chord} "),
        None => " ◫ Preview ".to_string(),
    });
    let hint_width = hint.as_ref().map_or(0, |h| h.chars().count() as u16);
    let total = x.saturating_sub(tab_area.x);
    let overflow = total > tab_area.width.saturating_sub(hint_width);
    let arrows_width = if overflow { 4 } else { 0 };
    let visible = tab_area.width.saturating_sub(hint_width + arrows_width);

    // Scrolling: clamp to what is there, and bring a newly fronted tab into
    // view the first time it is drawn.
    let in_front = (app.buffers.active, app.active_diff, app.active_preview);
    let max_scroll = total.saturating_sub(visible);
    app.tab_scroll = app.tab_scroll.min(max_scroll);
    if app.shown_tab != Some(in_front) {
        app.shown_tab = Some(in_front);
        let fronted = match in_front {
            (_, _, Some(p)) => preview_spans.get(p * 2).map(|s| (s.0, s.1 + 2)),
            (_, Some(d), _) => diff_spans.get(d * 2).map(|s| (s.0, s.1 + 2)),
            (b, _, _) => tab_spans.get(b * 2).map(|s| (s.0, s.1 + 2)),
        };
        if let Some((start, end)) = fronted {
            let (start, end) = (start - tab_area.x, end - tab_area.x);
            if end > app.tab_scroll + visible {
                app.tab_scroll = end.saturating_sub(visible);
            }
            if start < app.tab_scroll {
                app.tab_scroll = start;
            }
        }
    }
    let scroll = app.tab_scroll;
    let shift = |ranges: Vec<(u16, u16, usize, bool)>| -> Vec<(u16, u16, usize, bool)> {
        ranges
            .into_iter()
            .filter_map(|(start, end, i, close)| {
                let start = (start - tab_area.x).saturating_sub(scroll);
                let end = (end - tab_area.x).saturating_sub(scroll).min(visible);
                (end > start).then_some((tab_area.x + start, tab_area.x + end, i, close))
            })
            .collect()
    };
    app.layout.tab_spans = shift(tab_spans);
    app.layout.diff_tabs = shift(diff_spans);
    app.layout.preview_tabs = shift(preview_spans);

    // Drop the scrolled-off columns from the front of the strip, cut it to
    // the room it has, then put the controls after it.
    let mut skip = scroll as usize;
    let mut room = visible as usize;
    let mut shown: Vec<Span> = Vec::new();
    for span in spans.iter() {
        let text: Vec<char> = span.content.chars().collect();
        if skip >= text.len() {
            skip -= text.len();
            continue;
        }
        let piece: String = text[skip..].iter().take(room).collect();
        skip = 0;
        room -= piece.chars().count();
        shown.push(Span::styled(piece, span.style));
        if room == 0 {
            break;
        }
    }
    let empty = spans.is_empty();
    let mut spans = shown;
    if !empty {
        spans.push(Span::styled(
            " ".repeat(room),
            on(theme.ui.fg_dim, theme.ui.status_bg),
        ));
        let mut at = tab_area.x + visible;
        app.layout.tab_scroll_buttons = None;
        if overflow {
            let can_left = scroll > 0;
            let can_right = scroll < max_scroll;
            let tone = |on_: bool| {
                on(
                    if on_ { theme.ui.fg } else { theme.ui.fg_faint },
                    theme.ui.status_bg,
                )
            };
            spans.push(Span::styled(" ‹", tone(can_left)));
            spans.push(Span::styled(" ›", tone(can_right)));
            app.layout.tab_scroll_buttons = Some((at, at + 2));
            at += 4;
        }
        app.layout.preview_hint = None;
        if let Some(hint) = hint {
            let width = hint.chars().count() as u16;
            spans.push(Span::styled(
                hint,
                on(theme.ui.fg_bright, theme.ui.tab_inactive_bg),
            ));
            app.layout.preview_hint = Some((at, at + width));
        }
    } else {
        app.layout.tab_scroll_buttons = None;
        app.layout.preview_hint = None;
    }
    if empty {
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
    app.layout.viewer = Rect::default();
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
    let gutter = 7usize;
    let text_width = text_area.width.saturating_sub(gutter as u16) as usize;
    {
        let state = app.edit_state();
        if !state.free_scroll {
            if state.scroll > cursor_row {
                state.scroll = cursor_row;
            } else if height > 0 && cursor_row >= state.scroll + height {
                state.scroll = cursor_row + 1 - height;
            }
        }
        if state.scroll >= visible.len() {
            state.scroll = visible.len().saturating_sub(1);
        }
        // Sideways the view always follows the caret: a line wider than the
        // pane scrolls under it rather than hiding it.
        if cursor_col < state.col_scroll {
            state.col_scroll = cursor_col;
        } else if text_width > 0 && cursor_col >= state.col_scroll + text_width {
            state.col_scroll = cursor_col + 1 - text_width;
        }
    }
    let scroll = app.edit_state().scroll;
    let col_scroll = app.edit_state().col_scroll;
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

    // Indent guides: one faint bar per level of indentation before the text,
    // running through blank lines so a block reads as one.
    let guide_width = app.settings.indent.width.max(1);
    let guide_style = on(core_theme::indent_guide(&theme), theme.ui.editor_bg);
    let guides = {
        let text: String = app
            .highlight
            .iter()
            .map(|regions| {
                regions
                    .iter()
                    .map(|(_, _, t)| t.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        crate::core::indent::guides(&text, guide_width)
    };

    let render_line = |n: usize, row_style: Option<ratatui::style::Style>| -> Line<'static> {
        let folded = app.is_folded(n);
        let guide_cols = guides.get(n).copied().unwrap_or(0) * guide_width;
        let guide_at = |at: usize| at < guide_cols && at.is_multiple_of(guide_width);
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
        let limit = text_width + col_scroll;
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
            (from < end && to > start)
                .then(|| (from.saturating_sub(start), (to - start).min(end - start)))
        });
        if let Some(regions) = app.highlight.get(n) {
            for (rgb, italic, text) in regions {
                if col >= limit {
                    break;
                }
                let mut style = row_style.unwrap_or_else(|| on(*rgb, theme.ui.editor_bg));
                if *italic {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                // Tabs take the columns they stand for before anything is
                // measured: a Span carrying a raw tab advances the terminal
                // further than the frame accounts for, and the cells past it
                // keep what the last file left there.
                let piece = crate::core::indent::expand_tabs(text, guide_width, col);
                let piece: String = piece.chars().take(limit - col).collect();
                let len = piece.chars().count();
                let touched = selected.is_some_and(|(s, e)| col < e && col + len > s);
                let underlined = link.is_some_and(|(_, s, e)| col < e && col + len > s);
                let hit_here = hits.iter().any(|(s, e, _)| col < *e && col + len > *s);
                let guided = col < guide_cols && piece.chars().all(|c| c == ' ');
                if touched || underlined || hit_here || guided {
                    for (i, ch) in piece.chars().enumerate() {
                        let at = col + i;
                        let mut style = style;
                        let mut glyph = ch.to_string();
                        if guided && guide_at(at) && row_style.is_none() {
                            glyph = "\u{2502}".to_string();
                            style = guide_style;
                        }
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
                        spans.push(Span::styled(glyph, style));
                    }
                } else {
                    spans.push(Span::styled(piece, style));
                }
                col += len;
            }
        }
        // A blank line inside a block carries the block's guides too.
        if row_style.is_none() {
            while col < guide_cols.min(limit) {
                let glyph = if guide_at(col) { "\u{2502}" } else { " " };
                spans.push(Span::styled(glyph, guide_style));
                col += 1;
            }
        }
        // A selection running past the end of the line shows on the newline.
        if selected.is_some_and(|(s, e)| e > col && col >= s) && col < limit {
            spans.push(Span::styled(" ", on(theme.ui.fg, theme.ui.selection)));
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
        // The gutter stays; the text starts `col_scroll` columns in.
        if col_scroll > 0 {
            let mut skip = col_scroll;
            let mut kept = Vec::with_capacity(spans.len());
            for (i, span) in spans.into_iter().enumerate() {
                if i < 3 || skip == 0 {
                    kept.push(span);
                    continue;
                }
                let len = span.content.chars().count();
                if len <= skip {
                    skip -= len;
                    continue;
                }
                let rest: String = span.content.chars().skip(skip).collect();
                skip = 0;
                kept.push(Span::styled(rest, span.style));
            }
            spans = kept;
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
            // The caret sits where the text is drawn, so it counts the
            // columns a tab takes rather than the one character it is.
            let shown_col = app
                .buffers
                .active()
                .and_then(|b| b.text.split('\n').nth(cursor_line))
                .map(|line| crate::core::indent::display_column(line, cursor_col, guide_width))
                .unwrap_or(cursor_col);
            let cx = text_area.x
                + gutter as u16
                + shown_col.saturating_sub(col_scroll).min(text_width) as u16;
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

    // The mouse's selection, in view rows: it is held against the shell's own
    // text so it stays on that text while the panel scrolls.
    let selection = pty.selection();
    let scrollback = pty.scrollback() as isize;
    let selected_bg = color(theme.ui.selection);

    // Paint the shell's screen cell by cell, merging runs that share a style.
    let (lines, cursor) = pty.with_screen(|screen| {
        let mut lines: Vec<Line> = Vec::with_capacity(grid.height as usize);
        for row in 0..grid.height {
            let mut spans: Vec<Span> = Vec::new();
            let mut run = String::new();
            let mut run_style: Option<Style> = None;
            let highlighted = selection
                .and_then(|s| s.span_on(row as isize - scrollback, grid.width))
                .unwrap_or((0, 0));
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
                    None => (
                        " ".to_string(),
                        on(theme.ui.terminal_fg, theme.ui.editor_bg),
                    ),
                };
                // The selection changes the background only: a shell's own
                // colors carry meaning, and the text stays readable.
                let style = if col >= highlighted.0 && col < highlighted.1 {
                    style.bg(selected_bg)
                } else {
                    style
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
        let cursor =
            (!screen.hide_cursor() && screen.scrollback() == 0).then(|| screen.cursor_position());
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

    // The row reads, left to right: the file, then the message or the blame
    // line; on the right the cursor, the indentation, the language and the
    // theme — the same fields in the same order as the window's status bar.
    let mut path = match app.buffers.active() {
        Some(buf) => {
            let mut text = app.project.display(&buf.path);
            if buf.modified() {
                text.push_str("  \u{25cf}");
            }
            text
        }
        None => String::new(),
    };
    let message = if !app.status.is_empty() {
        app.status.clone()
    } else {
        // Who last touched the line the cursor is on.
        app.blame.as_ref().map(|b| b.line()).unwrap_or_default()
    };

    let (cursor, indent, lang) = match app.buffers.active() {
        Some(buf) => {
            let state = &app.edit[app.buffers.active];
            let lang = if buf.extension.is_empty() {
                "plain text".to_string()
            } else {
                buf.extension.clone()
            };
            (
                format!("Ln {}, Col {}", state.line + 1, state.col + 1),
                app.settings.indent.label(),
                lang,
            )
        }
        None => (String::new(), String::new(), String::new()),
    };
    let name = theme.name.clone();
    let right: Vec<&str> = [
        cursor.as_str(),
        indent.as_str(),
        lang.as_str(),
        name.as_str(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect();
    let right_len = right.iter().map(|s| s.chars().count()).sum::<usize>() + 3 * (right.len() - 1);

    // The message is the newer thing to read, so the path gives way to it:
    // first from its front — the file name is the end worth keeping — and
    // when even that leaves the message short, entirely.
    let room = width.saturating_sub(right_len + 3);
    if !message.is_empty() && !path.is_empty() {
        let path_room = room.saturating_sub(message.chars().count() + 3);
        path = if path_room >= 12 {
            trim_front(&path, path_room.min(room))
        } else {
            String::new()
        };
    } else {
        path = trim_front(&path, room);
    }
    let left_room = room.saturating_sub(path.chars().count() + if path.is_empty() { 0 } else { 3 });
    let message = clip(&message, left_room);
    let left_len = path.chars().count()
        + if message.is_empty() {
            0
        } else {
            3 + message.chars().count()
        };
    let gap = width.saturating_sub(left_len + right_len + 2).max(1);

    // The theme and the indentation are clickable, so where they land is kept.
    let mut x = area.x + (1 + left_len + gap) as u16;
    let mut spans = vec![Span::styled(
        format!(" {path}{}", if message.is_empty() { "" } else { "   " }),
        on(theme.ui.fg_dim, theme.ui.status_bg),
    )];
    spans.push(Span::styled(
        format!("{message}{}", " ".repeat(gap)),
        on(
            if crate::core::is_failure(&message) {
                theme.ui.danger
            } else {
                theme.ui.fg_dim
            },
            theme.ui.status_bg,
        ),
    ));
    app.layout.status_indent = Rect::default();
    app.layout.status_theme = Rect::default();
    let mouse = app.mouse;
    for (i, field) in right.iter().enumerate() {
        let w = field.chars().count() as u16;
        let rect = Rect {
            x,
            y: area.y,
            width: w.min((area.x + area.width).saturating_sub(x)),
            height: 1,
        };
        let clickable = *field == name || (*field == indent && !indent.is_empty());
        if *field == name {
            app.layout.status_theme = rect;
        } else if clickable {
            app.layout.status_indent = rect;
        }
        let hovered = clickable
            && mouse
                .is_some_and(|(mx, my)| my == rect.y && mx >= rect.x && mx < rect.x + rect.width);
        spans.push(Span::styled(
            field.to_string(),
            if hovered {
                on(theme.ui.fg_bright, theme.ui.status_bg)
            } else if *field == lang {
                on(theme.ui.accent_light, theme.ui.status_bg)
            } else {
                on(theme.ui.fg_dim, theme.ui.status_bg)
            },
        ));
        x += w;
        if i + 1 < right.len() {
            spans.push(Span::styled("   ", on(theme.ui.fg_dim, theme.ui.status_bg)));
            x += 3;
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(on(theme.ui.fg_dim, theme.ui.status_bg)),
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
        Prompt::Indent => crate::core::settings::Indent::CHOICES
            .iter()
            .map(|(label, _, _)| label.to_string())
            .collect(),
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
            .map(|c| format!("{}:{}  {}", app.project.display(&c.path), c.line, c.text))
            .collect(),
        Prompt::Palette => {
            let width = ALL.iter().map(|c| c.label().len()).max().unwrap_or(0);
            app.picker_items
                .iter()
                .filter_map(|i| ALL.get(*i))
                .map(|command| {
                    let chord = app
                        .settings
                        .tui_chord(*command)
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    format!("{:<width$}  {chord}", command.label())
                })
                .collect()
        }
        Prompt::QuickOpen => app
            .picker_items
            .iter()
            .filter_map(|i| app.picker_files.get(*i))
            .map(|(_, rel)| rel.clone())
            .collect(),
        _ => Vec::new(),
    };

    let width = (area.width as usize * 3 / 4).clamp(30, 100) as u16;
    let room = area.height.saturating_sub(8) as usize;
    let list_height = list.len().min(room.max(4)) as u16;
    let detail = prompt.detail();
    // One row for the title, one per list entry, plus the border, any input
    // and the lines a question adds under itself.
    let height = 3 + list_height + u16::from(prompt.is_input()) + detail.len() as u16;
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
        pad(
            &format!(
                " {}",
                match prompt {
                    Prompt::ConfirmReplaceAll => app.search.replace_all_question(),
                    _ => prompt.title(),
                }
            ),
            inner_width,
        ),
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
        // The caret sits where the next character lands.
        let col = 3 + app.prompt_input.chars().count() as u16;
        frame.set_cursor_position((
            (rect.x + 1 + col).min(rect.x + rect.width.saturating_sub(2)),
            rect.y + 2,
        ));
    }
    for line in &detail {
        lines.push(Line::from(Span::styled(
            pad(&format!(" {line}"), inner_width),
            on(theme.ui.fg_dim, theme.ui.sidebar_bg),
        )));
    }
    // Scroll the list so the selection stays visible.
    let list_y = rect.y + 2 + u16::from(prompt.is_input()) + detail.len() as u16;
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
    format!(
        "...{}",
        text.chars().skip(len - width + 3).collect::<String>()
    )
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
            let gap =
                inner_width.saturating_sub(left.chars().count() + shortcut.chars().count() + 1);
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

//! Git sidebar view: pick a repository and a worktree, see what changed.
//! The selection and status logic lives in [`crate::core::git::GitState`];
//! this module only renders it.

use std::path::{Path, PathBuf};

use crate::core::git::{Change, GitState, Worktree, REFRESH_EVERY};
use crate::core::theme::Theme;
use crate::gui::theme::{ansi_color, color};

#[derive(Default)]
pub struct GitPanel {
    pub state: GitState,
}

impl GitPanel {
    /// Marks the cached status stale, so it re-reads right away.
    pub fn invalidate(&mut self) {
        self.state.invalidate();
    }

    /// Draws the panel; returns the change the user clicked, to be shown as a
    /// diff.
    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &Theme, root: &Path) -> Option<Change> {
        self.state.tick(root);
        // Keep the list live while the view is on screen.
        ui.ctx().request_repaint_after(REFRESH_EVERY);

        if self.state.repos.is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("not a git repository")
                        .color(color(theme.ui.fg_faint))
                        .size(12.0),
                );
            });
            return None;
        }

        let mut open: Option<Change> = None;
        egui::Frame::default()
            .inner_margin(egui::Margin::symmetric(10, 0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;

                let name_of = |path: &PathBuf| {
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string())
                };
                // A heading above each picker, mirroring the terminal
                // frontend's layout.
                let heading = |ui: &mut egui::Ui, text: &str| {
                    ui.label(
                        egui::RichText::new(text)
                            .color(color(theme.ui.fg_faint))
                            .size(10.0),
                    );
                };

                let mut repo_choice = self.state.repo;
                heading(ui, "REPOSITORY");
                let current = self
                    .state
                    .repos
                    .get(self.state.repo)
                    .map(&name_of)
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("git_repo")
                    .width(ui.available_width())
                    .selected_text(egui::RichText::new(current).size(12.0))
                    .show_ui(ui, |ui| {
                        for (i, repo) in self.state.repos.iter().enumerate() {
                            ui.selectable_value(&mut repo_choice, i, name_of(repo))
                                .on_hover_text(repo.display().to_string());
                        }
                    });
                if repo_choice != self.state.repo {
                    self.state.select_repo(repo_choice);
                }

                let mut worktree_choice = self.state.worktree;
                // A gap between each label-and-value pair, matching the
                // search panel's form.
                ui.add_space(6.0);
                heading(ui, "WORKTREE");
                let label = |w: &Worktree| {
                    if w.branch.is_empty() {
                        w.name()
                    } else {
                        format!("{} · {}", w.name(), w.branch)
                    }
                };
                let current = self
                    .state
                    .worktrees
                    .get(self.state.worktree)
                    .map(label)
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("git_worktree")
                    .width(ui.available_width())
                    .selected_text(egui::RichText::new(current).size(12.0))
                    .show_ui(ui, |ui| {
                        for (i, w) in self.state.worktrees.iter().enumerate() {
                            ui.selectable_value(&mut worktree_choice, i, label(w))
                                .on_hover_text(w.path.display().to_string());
                        }
                    });
                if worktree_choice != self.state.worktree {
                    self.state.select_worktree(worktree_choice);
                }

                ui.add_space(6.0);
                let summary = match &self.state.error {
                    Some(error) => error.clone(),
                    None if self.state.changes.is_empty() => "no changes".to_string(),
                    None => format!("{} changed file(s)", self.state.changes.len()),
                };
                let tone = if self.state.error.is_some() {
                    theme.ui.danger
                } else {
                    theme.ui.fg_faint
                };
                ui.label(egui::RichText::new(summary).color(color(tone)).size(11.0));
            });

        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for change in &self.state.changes {
                    let letter = change.letter();
                    let mut job = egui::text::LayoutJob::default();
                    let fmt = |c| egui::text::TextFormat {
                        font_id: egui::FontId::monospace(11.5),
                        color: c,
                        ..Default::default()
                    };
                    job.append(&format!(" {letter}  "), 0.0, fmt(letter_color(letter, theme)));
                    job.append(&change.path, 0.0, fmt(color(theme.ui.fg_dim)));
                    job.wrap.max_rows = 1;
                    job.wrap.break_anywhere = true;
                    if ui
                        .add(egui::Button::new(job).frame(false))
                        .on_hover_text(&change.path)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        open = Some(change.clone());
                    }
                }
            });
        open
    }
}

/// Status letters use the terminal palette, so every theme colors them sanely.
pub fn letter_color(letter: char, theme: &Theme) -> egui::Color32 {
    match letter {
        'A' | 'U' => ansi_color(theme, 2), // green: new or untracked
        'D' => ansi_color(theme, 1),       // red: deleted
        'R' | 'C' => ansi_color(theme, 6), // cyan: renamed or copied
        _ => ansi_color(theme, 3),         // yellow: modified
    }
}

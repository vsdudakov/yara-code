//! The one tab strip the window draws. Files, diffs, markdown previews and
//! shells all sit in it, so a tab is the same size and answers the pointer the
//! same way wherever it is: click to bring it forward, click its mark to close
//! it, right-click for its own menu, and drag it to another place in the strip.
//!
//! A dragged tab is carried, not outlined: it leaves its slot empty, follows
//! the pointer whole, and the strip closes over the gap as it passes, so the
//! row always reads as it will read when the tab is let go.

use crate::core::theme::Theme;
use crate::gui::theme::{color, cross, dot};

/// One tab, as the strip needs to know it.
pub struct Tab {
    /// What this tab is, told apart from its neighbours for as long as it
    /// lives — a file's path, a shell's number. The widget is named after it
    /// rather than after the tab's position, because the strip reorders itself
    /// under a drag: named by position, the drag would end up holding
    /// whichever tab slid into the slot it started from.
    pub key: String,
    pub label: String,
    /// Wears a dot in place of the cross until the pointer is on it, the way
    /// VS Code marks unsaved work.
    pub modified: bool,
}

impl Tab {
    pub fn new(key: impl ToString, label: impl Into<String>) -> Self {
        Self {
            key: key.to_string(),
            label: label.into(),
            modified: false,
        }
    }

    pub fn modified(mut self, modified: bool) -> Self {
        self.modified = modified;
        self
    }
}

/// What the pointer asked of the strip this frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    /// Bring this tab forward.
    Show(usize),
    Close(usize),
    /// Take the tab at `from` out of the strip and put it back at `to`.
    Move {
        from: usize,
        to: usize,
    },
}

/// The strip's answer: what happened, and where the tabs ended up — a caller
/// with something of its own to put in a tab's place draws over its rect.
pub struct Row {
    pub actions: Vec<Action>,
    pub rects: Vec<egui::Rect>,
}

impl Row {
    /// The one thing a strip of tabs is usually asked: which tab, if any, the
    /// pointer brought forward.
    pub fn shown(&self) -> Option<usize> {
        self.actions.iter().find_map(|a| match a {
            Action::Show(i) => Some(*i),
            _ => None,
        })
    }
}

/// A tab in the pointer's hands. Which tab it is comes from egui's own drag
/// state, so it survives the strip reordering under it; what is carried here is
/// where inside the tab it was grabbed, so the copy under the pointer keeps
/// sitting under the same spot it was picked up by.
#[derive(Clone, Copy)]
struct Carried {
    grab: f32,
}

/// The size of a tab, the same in every strip: the padding around its label,
/// the label itself, and the box its mark sits in — wider than the mark, since
/// the box is what the pointer has to hit.
const PAD: egui::Margin = egui::Margin {
    left: 8,
    right: 8,
    top: 5,
    bottom: 5,
};
/// A tab's label is set at the size the navigator's rows are: the strip and
/// the tree stand side by side, and one scale across the window's chrome reads
/// as one window rather than two. The terminal has a single font for
/// everything and needs nothing said about it.
const TEXT: f32 = crate::gui::theme::SIDEBAR_TEXT;
const MARK: f32 = 12.0;
/// Between the label and the mark. One tab meets the next with no gap at all:
/// they are told apart by their fill, and a seam between them only let the
/// strip's own background through.
const GAP: f32 = 5.0;
const SEAM: f32 = 0.0;

/// Draws `tabs` in a row and says what the pointer did with them. `menu` adds
/// the entries a right-click on a tab opens — the strip picks that tab first,
/// so "this tab" is never in doubt. `reorder` is off for a strip whose order
/// is not the user's to change; `reveal` scrolls the tab in front into view.
pub fn row(
    ui: &mut egui::Ui,
    theme: &Theme,
    tabs: &[Tab],
    selected: Option<usize>,
    reorder: bool,
    reveal: bool,
    mut menu: impl FnMut(&mut egui::Ui, usize),
) -> Row {
    let mut actions = Vec::new();
    let mut rects = Vec::new();
    let mut carried = None;
    ui.spacing_mut().item_spacing.x = SEAM;
    for (i, tab) in tabs.iter().enumerate() {
        let selected = selected == Some(i);
        let fill = if selected {
            color(theme.ui.tab_active_bg)
        } else {
            color(theme.ui.tab_inactive_bg)
        };
        let painted = egui::Frame::default()
            .fill(fill)
            .inner_margin(PAD)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                let fg = if selected {
                    color(theme.ui.fg)
                } else {
                    color(theme.ui.fg_dim)
                };
                let title = egui::RichText::new(&tab.label).color(fg).size(TEXT);
                ui.add(egui::Label::new(title).selectable(false));
                // Room for the mark, painted below once it is known whether
                // the pointer is on it.
                ui.allocate_exact_size(egui::vec2(MARK, MARK), egui::Sense::hover())
                    .0
            });
        let rect = painted.response.rect;
        // The whole tab is the handle: grabbing one by its label alone was a
        // target the width of the text, and a drag that missed the padding did
        // nothing.
        //
        // That handle lies over the tab's own contents, and egui gives a click
        // to the last widget registered under the pointer, so a mark of its own
        // would never be clicked: whatever is drawn first is buried. The mark
        // is therefore a corner of the tab rather than a widget, and where the
        // pointer is says which of the two things a click on the tab means.
        let sense = if reorder {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::click()
        };
        let handle = ui
            .interact(rect, ui.id().with(("tab", &tab.key)), sense)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let mark_rect = painted.inner;
        let on_mark = |pos: Option<egui::Pos2>| pos.is_some_and(|pos| mark_rect.contains(pos));
        if ui.is_rect_visible(mark_rect) {
            let hovered = on_mark(handle.hover_pos());
            let tint = if hovered {
                color(theme.ui.fg)
            } else {
                color(theme.ui.fg_dim)
            };
            if hovered {
                ui.painter().rect_filled(
                    mark_rect,
                    egui::CornerRadius::same(3),
                    color(theme.ui.hover_bg),
                );
            }
            if tab.modified && !hovered {
                dot(ui.painter(), mark_rect.center(), MARK, tint);
            } else {
                cross(ui.painter(), mark_rect.center(), MARK, tint);
            }
        }
        if handle.clicked() {
            actions.push(if on_mark(handle.interact_pointer_pos()) {
                Action::Close(i)
            } else {
                Action::Show(i)
            });
        }
        // Picking a tab up brings it forward, as a click on it would, and
        // empties the slot it came from: from here it rides under the pointer.
        if handle.drag_started() {
            let grab = handle
                .interact_pointer_pos()
                .map_or(rect.width() / 2.0, |pos| pos.x - rect.left());
            handle.dnd_set_drag_payload(Carried { grab });
            actions.push(Action::Show(i));
        }
        if handle.dragged() {
            carried = Some(i);
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, color(theme.ui.status_bg));
        }
        handle.context_menu(|ui| menu(ui, i));
        if selected && reveal {
            painted.response.scroll_to_me(None);
        }
        rects.push(rect);
    }
    if let (Some(from), Some(pos)) = (carried, ui.ctx().pointer_interact_pos()) {
        // A carried tab changes places with a neighbour the moment the pointer
        // is past that neighbour's middle. There is nothing left for the drop
        // itself to do.
        let ahead = rects[..from].iter().position(|r| pos.x < r.center().x);
        let behind = rects
            .iter()
            .enumerate()
            .skip(from + 1)
            .filter(|(_, r)| pos.x > r.center().x)
            .map(|(j, _)| j)
            .next_back();
        if let Some(to) = ahead.or(behind) {
            actions.push(Action::Move { from, to });
        }
        paint_carried(ui, theme, &tabs[from], rects[from], pos);
    }
    Row { actions, rects }
}

/// True while a tab is being carried, whichever strip it came from — a strip
/// that scrolls follows a tab dragged against its edge.
pub fn carrying(ctx: &egui::Context) -> bool {
    egui::DragAndDrop::payload::<Carried>(ctx).is_some()
}

/// The whole of the tab in the pointer's hands, drawn above the strip so that
/// it passes over its neighbours rather than through them.
fn paint_carried(ui: &egui::Ui, theme: &Theme, tab: &Tab, slot: egui::Rect, pointer: egui::Pos2) {
    let grab = egui::DragAndDrop::payload::<Carried>(ui.ctx())
        .map_or(slot.width() / 2.0, |held| held.grab);
    let held = egui::Rect::from_min_size(egui::pos2(pointer.x - grab, slot.top()), slot.size());
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        ui.id().with("carried_tab"),
    ));
    painter.rect_filled(
        held,
        egui::CornerRadius::ZERO,
        color(theme.ui.tab_active_bg),
    );
    painter.rect_stroke(
        held,
        egui::CornerRadius::ZERO,
        egui::Stroke::new(1.0_f32, color(theme.ui.accent_light)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(held.left() + PAD.left as f32, held.center().y),
        egui::Align2::LEFT_CENTER,
        &tab.label,
        egui::FontId::proportional(TEXT),
        color(theme.ui.fg),
    );
    let mark = egui::pos2(
        held.right() - PAD.right as f32 - MARK / 2.0,
        held.center().y,
    );
    if tab.modified {
        dot(&painter, mark, MARK, color(theme.ui.fg_dim));
    } else {
        cross(&painter, mark, MARK, color(theme.ui.fg_dim));
    }
}

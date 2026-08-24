//! Bridges a core theme into ratatui styles.

use ratatui::style::{Color, Modifier, Style};

use crate::core::theme::{Rgb, Theme};

pub fn color(c: Rgb) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

pub fn fg(c: Rgb) -> Style {
    Style::default().fg(color(c))
}

pub fn on(fg_c: Rgb, bg_c: Rgb) -> Style {
    Style::default().fg(color(fg_c)).bg(color(bg_c))
}

pub fn syntax_style(theme: &Theme, region_color: Rgb, italic: bool) -> Style {
    let mut style = Style::default()
        .fg(color(region_color))
        .bg(color(theme.ui.editor_bg));
    if italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    style
}

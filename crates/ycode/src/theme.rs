//! Bridges a core theme into ratatui styles.

use ratatui::style::{Color, Modifier, Style};
use yara_core::theme::{Rgb, Theme};

pub fn color(c: Rgb) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

pub fn fg(c: Rgb) -> Style {
    Style::new().fg(color(c))
}

pub fn bold(c: Rgb) -> Style {
    fg(c).add_modifier(Modifier::BOLD)
}

pub fn on(fg_c: Rgb, bg_c: Rgb) -> Style {
    fg(fg_c).bg(color(bg_c))
}

pub fn base(theme: &Theme) -> Style {
    on(theme.ui.fg, theme.ui.bg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_cross_into_ratatui_unchanged() {
        assert_eq!(color((1, 2, 3)), Color::Rgb(1, 2, 3));
        let style = on((10, 20, 30), (40, 50, 60));
        assert_eq!(style.fg, Some(Color::Rgb(10, 20, 30)));
        assert_eq!(style.bg, Some(Color::Rgb(40, 50, 60)));
        assert_eq!(fg((7, 8, 9)).bg, None);
        assert!(bold((0, 0, 0)).add_modifier.contains(Modifier::BOLD));
        let theme = Theme::default();
        assert_eq!(base(&theme).bg, Some(color(theme.ui.bg)));
    }
}

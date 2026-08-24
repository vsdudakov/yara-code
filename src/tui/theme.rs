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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::theme::dark_plus;

    #[test]
    fn colours_cross_into_ratatui_unchanged() {
        assert_eq!(color((1, 2, 3)), Color::Rgb(1, 2, 3));
        let style = on((10, 20, 30), (40, 50, 60));
        assert_eq!(style.fg, Some(Color::Rgb(10, 20, 30)));
        assert_eq!(style.bg, Some(Color::Rgb(40, 50, 60)));
        assert_eq!(fg((7, 8, 9)).fg, Some(Color::Rgb(7, 8, 9)));
        assert_eq!(fg((7, 8, 9)).bg, None);
    }

    #[test]
    fn syntax_keeps_the_editor_background_and_adds_the_slant() {
        let theme = dark_plus();
        let plain = syntax_style(&theme, (200, 100, 50), false);
        assert_eq!(plain.fg, Some(Color::Rgb(200, 100, 50)));
        assert_eq!(plain.bg, Some(color(theme.ui.editor_bg)));
        assert!(!plain.add_modifier.contains(Modifier::ITALIC));
        let italic = syntax_style(&theme, (0, 0, 0), true);
        assert!(italic.add_modifier.contains(Modifier::ITALIC));
    }
}

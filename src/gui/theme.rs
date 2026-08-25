//! Bridges a [`core::theme::Theme`] into egui's style system.

use egui::{Color32, FontFamily, FontId, TextStyle};

use crate::core::theme::{self as core_theme, Rgb, Theme};

/// Fallback code font size; the live one comes from settings and is what
/// Zoom In and Zoom Out change.
pub const CODE_FONT_SIZE: f32 = 13.5;

/// The code font actually in effect, so measurements follow the zoom.
pub fn code_font(ui: &egui::Ui) -> FontId {
    ui.style()
        .text_styles
        .get(&TextStyle::Monospace)
        .cloned()
        .unwrap_or_else(|| FontId::new(CODE_FONT_SIZE, FontFamily::Monospace))
}

pub fn color(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

pub fn ansi_color(theme: &Theme, idx: u8) -> Color32 {
    color(core_theme::ansi256(theme, idx))
}

/// Applies the theme to egui's global style. Called on startup and whenever the
/// user switches themes.
pub fn apply(ctx: &egui::Context, theme: &Theme, code_size: f32) {
    let mut style = (*ctx.style()).clone();

    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(15.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(11.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(code_size, FontFamily::Monospace),
        ),
    ]
    .into();

    let ui = &theme.ui;
    let v = &mut style.visuals;
    *v = if theme.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = color(ui.editor_bg);
    v.window_fill = color(ui.editor_bg);
    v.extreme_bg_color = color(ui.editor_bg);
    v.selection.bg_fill = color(ui.selection);
    // Not NONE: egui paints a selected row's *text* in this stroke's colour,
    // and a stroke of none is a transparent one — which is how the repository
    // and worktree lists came up as an empty coloured box with the name that
    // was picked nowhere to be seen.
    v.selection.stroke = egui::Stroke::new(1.0_f32, color(ui.fg_bright));
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, color(ui.border));
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, color(ui.fg_dim));
    v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, color(ui.fg_dim));
    v.widgets.hovered.bg_fill = color(ui.hover_bg);
    v.widgets.hovered.weak_bg_fill = color(ui.hover_bg);
    v.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    v.widgets.active.bg_fill = color(ui.selected_bg);
    v.widgets.active.weak_bg_fill = color(ui.selected_bg);
    v.widgets.active.bg_stroke = egui::Stroke::NONE;
    // Panel splitters take their color from these strokes: a soft accent on
    // hover, brighter while dragging — never a hard white line.
    v.widgets.hovered.fg_stroke = egui::Stroke::new(2.0_f32, color(ui.accent));
    v.widgets.active.fg_stroke = egui::Stroke::new(2.0_f32, color(ui.accent_light));

    style.spacing.item_spacing = egui::vec2(6.0, 2.0);
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    style.spacing.scroll = egui::style::ScrollStyle::thin();

    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::theme::{dark_plus, light_plus};

    #[test]
    fn a_selected_row_has_text_to_show() {
        let ctx = egui::Context::default();
        for theme in [dark_plus(), light_plus()] {
            apply(&ctx, &theme, 13.0);
            let stroke = ctx.style().visuals.selection.stroke;
            assert_ne!(
                stroke.color,
                egui::Color32::TRANSPARENT,
                "{}: a selected row paints its text in this colour",
                theme.name
            );
            assert!(stroke.width > 0.0, "{}", theme.name);
        }
    }

    #[test]
    fn colours_cross_into_egui_unchanged() {
        assert_eq!(color((1, 2, 3)), Color32::from_rgb(1, 2, 3));
        let theme = dark_plus();
        assert_eq!(ansi_color(&theme, 1), color(theme.ansi[1]));
    }

    #[test]
    fn applying_a_theme_sets_the_code_font_and_the_visuals() {
        let ctx = egui::Context::default();
        crate::gui::theme::apply(&ctx, &dark_plus(), 17.0);
        let style = ctx.style();
        let mono = style.text_styles.get(&TextStyle::Monospace).unwrap();
        assert_eq!(mono.size, 17.0, "zoom reaches the code font");
        assert!(style.visuals.dark_mode);
        assert_eq!(style.visuals.panel_fill, color(dark_plus().ui.editor_bg));

        // A light theme switches egui's own visuals with it.
        crate::gui::theme::apply(&ctx, &light_plus(), 13.5);
        assert!(!ctx.style().visuals.dark_mode);
    }

    #[test]
    fn the_code_font_reported_is_the_one_in_effect() {
        let ctx = egui::Context::default();
        crate::gui::theme::apply(&ctx, &dark_plus(), 21.0);
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert_eq!(code_font(ui).size, 21.0);
            });
        });
    }
}

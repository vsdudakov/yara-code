//! Translates the settings' key chords into egui's key/modifier pairs.

use crate::core::command::{Chord, Key, Mods};

/// egui's `COMMAND` already means ⌘ on macOS and Ctrl elsewhere, which is what
/// a chord written as `Cmd+S` should do.
pub fn modifiers(mods: &Mods) -> egui::Modifiers {
    let mut out = egui::Modifiers::NONE;
    if mods.cmd {
        out |= egui::Modifiers::COMMAND;
    }
    if mods.ctrl {
        out |= egui::Modifiers::CTRL;
    }
    if mods.alt {
        out |= egui::Modifiers::ALT;
    }
    if mods.shift {
        out |= egui::Modifiers::SHIFT;
    }
    out
}

pub fn key(key: &Key) -> Option<egui::Key> {
    use egui::Key as K;
    Some(match key {
        Key::Char(c) => match c {
            'a'..='z' => K::from_name(&c.to_ascii_uppercase().to_string())?,
            '0'..='9' => K::from_name(&c.to_string())?,
            ',' => K::Comma,
            '.' => K::Period,
            '-' => K::Minus,
            '+' => K::Plus,
            '=' => K::Equals,
            '/' => K::Slash,
            '\\' => K::Backslash,
            ';' => K::Semicolon,
            '\'' => K::Quote,
            '[' => K::OpenBracket,
            ']' => K::CloseBracket,
            '`' => K::Backtick,
            _ => return None,
        },
        Key::Named(name) => match name.as_str() {
            "left" => K::ArrowLeft,
            "right" => K::ArrowRight,
            "up" => K::ArrowUp,
            "down" => K::ArrowDown,
            "enter" | "return" => K::Enter,
            "tab" => K::Tab,
            "esc" | "escape" => K::Escape,
            "space" => K::Space,
            "home" => K::Home,
            "end" => K::End,
            "pageup" => K::PageUp,
            "pagedown" => K::PageDown,
            "delete" => K::Delete,
            "backspace" => K::Backspace,
            other => K::from_name(other)?,
        },
    })
}

/// Consumes the chord's key press if it happened this frame.
pub fn consumed(ctx: &egui::Context, chord: &Chord) -> bool {
    let Some(egui_key) = key(&chord.key) else {
        return false;
    };
    let mods = modifiers(&chord.mods);
    ctx.input_mut(|i| i.consume_key(mods, egui_key))
}

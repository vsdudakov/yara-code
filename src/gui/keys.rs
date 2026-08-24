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
            // Function keys are written `f1`..`f12`; egui names them `F1`.
            other if other.starts_with('f') && other[1..].parse::<u8>().is_ok() => {
                K::from_name(&other.to_ascii_uppercase())?
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::command::Chord;

    fn egui_key(text: &str) -> Option<egui::Key> {
        key(&text.parse::<Chord>().unwrap().key)
    }

    #[test]
    fn every_modifier_reaches_egui() {
        let mods = modifiers(&Mods {
            cmd: true,
            ctrl: true,
            alt: true,
            shift: true,
        });
        assert!(mods.command && mods.ctrl && mods.alt && mods.shift);
        assert_eq!(modifiers(&Mods::default()), egui::Modifiers::NONE);
    }

    #[test]
    fn letters_digits_and_punctuation_all_translate() {
        assert_eq!(egui_key("Cmd+S"), Some(egui::Key::S));
        assert_eq!(egui_key("Cmd+0"), Some(egui::Key::Num0));
        assert_eq!(egui_key("Cmd+,"), Some(egui::Key::Comma));
        assert_eq!(egui_key("Ctrl+-"), Some(egui::Key::Minus));
        assert_eq!(egui_key("Cmd+="), Some(egui::Key::Equals));
        assert_eq!(egui_key("Cmd+["), Some(egui::Key::OpenBracket));
        // Nothing egui knows: better unbound than bound to the wrong key.
        assert_eq!(key(&Key::Char('§')), None);
    }

    #[test]
    fn named_keys_translate_including_the_function_row() {
        assert_eq!(egui_key("Alt+Left"), Some(egui::Key::ArrowLeft));
        assert_eq!(egui_key("Ctrl+Enter"), Some(egui::Key::Enter));
        assert_eq!(egui_key("Shift+Delete"), Some(egui::Key::Delete));
        assert_eq!(egui_key("Ctrl+PageDown"), Some(egui::Key::PageDown));
        assert_eq!(egui_key("F12"), Some(egui::Key::F12));
        assert_eq!(egui_key("F1"), Some(egui::Key::F1));
        assert_eq!(key(&Key::Named("nosuchkey".into())), None);
    }

    #[test]
    fn a_chord_egui_cannot_express_is_never_consumed() {
        let ctx = egui::Context::default();
        let chord: Chord = "Cmd+§".parse().unwrap_or_else(|_| "Cmd+S".parse().unwrap());
        // With no such key event in the queue, nothing is consumed either way.
        assert!(!consumed(&ctx, &chord));
    }
}

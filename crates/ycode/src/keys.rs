//! Translates a crossterm key event into a core chord, so the settings map
//! can be consulted. Terminals never report Cmd, so it is left unset.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use yara_core::command::{Chord, Key, Mods};

pub fn chord_of(key: KeyEvent) -> Option<Chord> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let mods = Mods {
        cmd: false,
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
    };
    let key = match key.code {
        KeyCode::Char(c) => Key::Char(c.to_ascii_lowercase()),
        KeyCode::F(n) => Key::Named(format!("f{n}")),
        other => Key::Named(
            match other {
                KeyCode::Left => "left",
                KeyCode::Right => "right",
                KeyCode::Up => "up",
                KeyCode::Down => "down",
                KeyCode::Enter => "enter",
                KeyCode::Tab | KeyCode::BackTab => "tab",
                KeyCode::Esc => "esc",
                KeyCode::Home => "home",
                KeyCode::End => "end",
                KeyCode::PageUp => "pageup",
                KeyCode::PageDown => "pagedown",
                KeyCode::Delete => "delete",
                KeyCode::Backspace => "backspace",
                _ => return None,
            }
            .into(),
        ),
    };
    Some(Chord { mods, key })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_event_reads_as_the_chord_a_user_would_write() {
        let chord = |code, mods| chord_of(KeyEvent::new(code, mods)).unwrap().to_string();
        assert_eq!(chord(KeyCode::Char('s'), KeyModifiers::CONTROL), "Ctrl+S");
        assert_eq!(
            chord(
                KeyCode::Char('G'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            "Ctrl+Shift+G"
        );
        assert_eq!(chord(KeyCode::F(1), KeyModifiers::NONE), "F1");
        assert_eq!(chord(KeyCode::Left, KeyModifiers::NONE), "Left");
        assert_eq!(chord(KeyCode::Char('f'), KeyModifiers::NONE), "F");
        assert!(chord_of(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE)).is_none());
        let mut release = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert!(chord_of(release).is_none());
    }
}

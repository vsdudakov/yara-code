//! The kitty keyboard protocol, both halves of it: what a program running in
//! the built-in terminal asks for, and what a key press sends it once it has.
//!
//! A terminal cannot tell Ctrl+Shift+V from Ctrl+V, because the control byte
//! the two share has no room for Shift. The protocol is how a program says it
//! wants the difference: it pushes a set of flags, and from then on the
//! terminal spells the ambiguous keys out as `CSI code ; modifiers u`. Without
//! it, this editor's own second tier of bindings — every `Ctrl+Shift` chord —
//! is unreachable in this editor's own terminal.

use crate::command::{Key, Mods};

/// Disambiguate escape codes, the first flag and the only one this terminal
/// claims. The rest of the protocol reports key releases, alternate layouts
/// and the text a press produced; a program that asks for those still gets the
/// keys it actually came for, and is told which flags it was given.
pub const DISAMBIGUATE: u8 = 0b1;

/// What the terminal will honour, whatever a program asks for.
const SUPPORTED: u8 = DISAMBIGUATE;

/// The protocol allows a program to push its flags and pop them back off, so
/// that a shell keeps what it set while an editor it launched has its own.
const DEPTH: usize = 100;

/// The flags in force, and enough of an escape-code reader to keep them: the
/// screen is drawn by a full terminal parser, and this only watches the same
/// stream for the four codes the protocol is made of.
#[derive(Default)]
pub struct Protocol {
    stack: Vec<u8>,
    state: Scan,
    params: Vec<u8>,
}

#[derive(Default, PartialEq)]
enum Scan {
    #[default]
    Ground,
    Escape,
    Csi,
}

impl Protocol {
    /// Reads a run of the shell's output and answers the protocol's own codes.
    /// What comes back is to be written straight to the pty; everything else in
    /// the stream is left for the parser that draws it.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut reply = Vec::new();
        for byte in bytes {
            match self.state {
                Scan::Ground => {
                    if *byte == 0x1b {
                        self.state = Scan::Escape;
                    }
                }
                Scan::Escape => {
                    self.state = match byte {
                        b'[' => {
                            self.params.clear();
                            Scan::Csi
                        }
                        // An escape after an escape starts the sequence over.
                        0x1b => Scan::Escape,
                        _ => Scan::Ground,
                    };
                }
                Scan::Csi => match byte {
                    // The byte that ends a control sequence and says what it
                    // was. Only `u` is ours.
                    0x40..=0x7e => {
                        if *byte == b'u' {
                            reply.extend(self.sequence());
                        }
                        self.state = Scan::Ground;
                    }
                    // A sequence longer than any of the protocol's own is one
                    // of somebody else's, and is read to its end and dropped.
                    _ => {
                        if self.params.len() < 32 {
                            self.params.push(*byte);
                        }
                    }
                },
            }
        }
        reply
    }

    /// The flags in force, which is what a key press is spelled by.
    pub fn flags(&self) -> u8 {
        self.stack.last().copied().unwrap_or(0)
    }

    /// One `CSI … u`, which is a question about the flags or a change to them.
    fn sequence(&mut self) -> Vec<u8> {
        let Some((head, rest)) = self.params.split_first() else {
            return Vec::new();
        };
        let rest = String::from_utf8_lossy(rest).to_string();
        let mut numbers = rest.split(';').map(|n| n.trim().parse::<u8>().ok());
        match head {
            b'?' => return format!("\x1b[?{}u", self.flags()).into_bytes(),
            b'>' => {
                let flags = numbers.next().flatten().unwrap_or(0) & SUPPORTED;
                // A stack that has run out drops what was pushed first: the
                // flags in force are the ones the program in front asked for.
                if self.stack.len() == DEPTH {
                    self.stack.remove(0);
                }
                self.stack.push(flags);
            }
            b'<' => {
                let count = numbers.next().flatten().unwrap_or(1) as usize;
                let keep = self.stack.len().saturating_sub(count);
                self.stack.truncate(keep);
            }
            b'=' => {
                let flags = numbers.next().flatten().unwrap_or(0) & SUPPORTED;
                let held = self.flags();
                let set = match numbers.next().flatten().unwrap_or(1) {
                    2 => held | flags,
                    3 => held & !flags,
                    _ => flags,
                };
                match self.stack.last_mut() {
                    Some(top) => *top = set,
                    None => self.stack.push(set),
                }
            }
            _ => {}
        }
        Vec::new()
    }
}

/// The bytes a key press sends, as the program in front has asked to be told
/// about it. Without the protocol these are the codes every terminal has sent
/// since the VT100; with it, the presses those codes cannot tell apart are
/// spelled out instead.
pub fn bytes(key: &Key, mods: Mods, flags: u8) -> Vec<u8> {
    let told = flags & DISAMBIGUATE != 0;
    // Shift alone is carried by the character a key produces, so it is not a
    // modifier a legacy code has to spell.
    let modified = mods.ctrl || mods.alt || mods.cmd;
    match key {
        Key::Char(c) => {
            if told && modified {
                return csi_u(u32::from(c.to_ascii_lowercase()), mods);
            }
            let mut out = Vec::new();
            if mods.ctrl {
                match control(*c) {
                    Some(byte) => out.push(byte),
                    // A Ctrl the keyboard has no control byte for is nothing
                    // at all to a program that has not asked for the protocol.
                    None => return Vec::new(),
                }
                return out;
            }
            if mods.alt {
                out.push(0x1b);
            }
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            out
        }
        Key::Named(name) => named(name, mods, told),
    }
}

fn named(name: &str, mods: Mods, told: bool) -> Vec<u8> {
    let modified = mods.ctrl || mods.alt || mods.cmd || mods.shift;
    match name {
        // Escape is the one key the protocol spells out even unmodified: on
        // its own it is the first byte of every other code, which is what
        // makes a program wait to see whether more is coming.
        "esc" | "escape" => {
            if told {
                csi_u(27, mods)
            } else {
                vec![0x1b]
            }
        }
        "enter" | "return" => {
            if told && modified {
                csi_u(13, mods)
            } else if mods.shift || mods.alt {
                // The newline an agent's prompt asks for: escape then return
                // is what a terminal set up for one sends.
                vec![0x1b, b'\r']
            } else {
                vec![b'\r']
            }
        }
        "tab" => {
            if told && modified {
                csi_u(9, mods)
            } else if mods.shift {
                b"\x1b[Z".to_vec()
            } else {
                vec![b'\t']
            }
        }
        "backtab" => named(
            "tab",
            Mods {
                shift: true,
                ..mods
            },
            told,
        ),
        "backspace" => {
            if told && modified {
                csi_u(127, mods)
            } else if mods.alt {
                vec![0x1b, 0x7f]
            } else {
                vec![0x7f]
            }
        }
        "space" => {
            if told && (mods.ctrl || mods.alt || mods.cmd) {
                csi_u(32, mods)
            } else if mods.ctrl {
                vec![0]
            } else {
                vec![b' ']
            }
        }
        "up" => arrow(b'A', mods),
        "down" => arrow(b'B', mods),
        "right" => arrow(b'C', mods),
        "left" => arrow(b'D', mods),
        "home" => arrow(b'H', mods),
        "end" => arrow(b'F', mods),
        "insert" => tilde(2, mods),
        "delete" => tilde(3, mods),
        "pageup" => tilde(5, mods),
        "pagedown" => tilde(6, mods),
        _ => function(name, mods),
    }
}

/// `f1` to `f12`. The first four are the keypad codes a terminal has always
/// sent; the rest are numbered, and a modifier turns both into the long form.
fn function(name: &str, mods: Mods) -> Vec<u8> {
    let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) else {
        return Vec::new();
    };
    if (1..=4).contains(&n) {
        let last = b'P' + (n - 1);
        return match modifier(mods) {
            1 => vec![0x1b, b'O', last],
            m => format!("\x1b[1;{m}{}", last as char).into_bytes(),
        };
    }
    let code = match n {
        5 => 15,
        6 => 17,
        7 => 18,
        8 => 19,
        9 => 20,
        10 => 21,
        11 => 23,
        12 => 24,
        _ => return Vec::new(),
    };
    tilde(code, mods)
}

/// An arrow or a corner key: `CSI A` on its own, `CSI 1 ; modifiers A` when a
/// modifier is held — the form every terminal has used for these since long
/// before the protocol, and the one it keeps using for them.
fn arrow(last: u8, mods: Mods) -> Vec<u8> {
    match modifier(mods) {
        1 => vec![0x1b, b'[', last],
        m => format!("\x1b[1;{m}{}", last as char).into_bytes(),
    }
}

/// The keys numbered rather than lettered — Insert, Delete, the pages and the
/// upper function keys.
fn tilde(code: u8, mods: Mods) -> Vec<u8> {
    match modifier(mods) {
        1 => format!("\x1b[{code}~").into_bytes(),
        m => format!("\x1b[{code};{m}~").into_bytes(),
    }
}

fn csi_u(code: u32, mods: Mods) -> Vec<u8> {
    format!("\x1b[{code};{}u", modifier(mods)).into_bytes()
}

/// The modifier as the protocol counts it: one, plus a bit for each held.
fn modifier(mods: Mods) -> u8 {
    1 + u8::from(mods.shift)
        + 2 * u8::from(mods.alt)
        + 4 * u8::from(mods.ctrl)
        + 8 * u8::from(mods.cmd)
}

/// The control byte a Ctrl'd character has always sent: Ctrl+A is 1, and the
/// handful of punctuation keys that carry one.
fn control(c: char) -> Option<u8> {
    let lower = c.to_ascii_lowercase();
    if lower.is_ascii_lowercase() {
        return Some(lower as u8 - b'a' + 1);
    }
    match c {
        ' ' => Some(0),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl_shift() -> Mods {
        Mods {
            ctrl: true,
            shift: true,
            ..Mods::default()
        }
    }

    #[test]
    fn a_program_is_told_the_flags_it_was_given() {
        let mut protocol = Protocol::default();
        assert_eq!(protocol.feed(b"\x1b[?u"), b"\x1b[?0u".to_vec());
        protocol.feed(b"\x1b[>1u");
        assert_eq!(protocol.flags(), DISAMBIGUATE);
        assert_eq!(protocol.feed(b"\x1b[?u"), b"\x1b[?1u".to_vec());
        // What it asks for beyond what this terminal does is not claimed.
        protocol.feed(b"\x1b[>31u");
        assert_eq!(protocol.flags(), DISAMBIGUATE);
        protocol.feed(b"\x1b[<1u");
        assert_eq!(protocol.flags(), DISAMBIGUATE);
        protocol.feed(b"\x1b[<9u");
        assert_eq!(protocol.flags(), 0);
    }

    #[test]
    fn a_code_split_across_two_reads_is_still_one_code() {
        let mut protocol = Protocol::default();
        assert!(protocol.feed(b"\x1b[>1").is_empty());
        protocol.feed(b"u");
        assert_eq!(protocol.flags(), DISAMBIGUATE);
        assert!(protocol.feed(b"\x1b[?").is_empty());
        assert_eq!(protocol.feed(b"u"), b"\x1b[?1u".to_vec());
    }

    #[test]
    fn everything_else_in_the_stream_goes_by_untouched() {
        let mut protocol = Protocol::default();
        assert!(protocol
            .feed(b"hello\x1b[31m world\x1b[2J\x1b[6n")
            .is_empty());
        assert_eq!(protocol.flags(), 0);
    }

    #[test]
    fn ctrl_shift_v_is_a_key_of_its_own_once_the_protocol_is_on() {
        let key = Key::Char('V');
        // Without it, the same byte Ctrl+V sends — which is the whole problem.
        assert_eq!(bytes(&key, ctrl_shift(), 0), vec![0x16]);
        assert_eq!(
            bytes(&key, ctrl_shift(), DISAMBIGUATE),
            b"\x1b[118;6u".to_vec()
        );
    }

    #[test]
    fn what_a_key_says_plainly_it_keeps_saying() {
        let plain = Mods::default();
        for flags in [0, DISAMBIGUATE] {
            assert_eq!(bytes(&Key::Char('a'), plain, flags), b"a".to_vec());
            assert_eq!(
                bytes(&Key::Named("enter".into()), plain, flags),
                vec![b'\r']
            );
            assert_eq!(bytes(&Key::Named("tab".into()), plain, flags), vec![b'\t']);
            assert_eq!(
                bytes(&Key::Named("left".into()), plain, flags),
                b"\x1b[D".to_vec()
            );
            assert_eq!(
                bytes(&Key::Named("f5".into()), plain, flags),
                b"\x1b[15~".to_vec()
            );
        }
        // Escape is the exception: on its own it is the start of every other
        // code, so a program that asked to be told hears it as itself.
        assert_eq!(bytes(&Key::Named("esc".into()), plain, 0), vec![0x1b]);
        assert_eq!(
            bytes(&Key::Named("esc".into()), plain, DISAMBIGUATE),
            b"\x1b[27;1u".to_vec()
        );
    }

    #[test]
    fn a_modifier_is_spelled_out_wherever_a_key_has_room_for_it() {
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        assert_eq!(bytes(&Key::Char('b'), alt, 0), vec![0x1b, b'b']);
        assert_eq!(
            bytes(&Key::Char('b'), alt, DISAMBIGUATE),
            b"\x1b[98;3u".to_vec()
        );
        // Shift+Enter is the newline an agent's prompt asks for, told apart
        // from a plain Return only once the protocol is on.
        assert_eq!(
            bytes(&Key::Named("enter".into()), shift, 0),
            vec![0x1b, b'\r']
        );
        assert_eq!(
            bytes(&Key::Named("enter".into()), shift, DISAMBIGUATE),
            b"\x1b[13;2u".to_vec()
        );
        // An arrow keeps the form it has always had, modifier and all.
        assert_eq!(
            bytes(&Key::Named("left".into()), shift, DISAMBIGUATE),
            b"\x1b[1;2D".to_vec()
        );
        assert_eq!(
            bytes(&Key::Named("backtab".into()), Mods::default(), 0),
            b"\x1b[Z".to_vec()
        );
    }
}

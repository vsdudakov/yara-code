//! Copy and paste for the terminal frontend.
//!
//! Copying also hands the text to the host terminal with OSC 52, so it lands in
//! the system clipboard — including over SSH, where the editor has no access to
//! the local one. Terminals that ignore the sequence simply keep the internal
//! copy, which paste uses.

use std::io::Write;

#[derive(Default)]
pub struct Clipboard {
    text: String,
}

impl Clipboard {
    pub fn set(&mut self, text: String) {
        self.text = text;
        offer_to_terminal(&self.text);
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// Terminals cap the OSC 52 payload; a whole file pasted at once would be
/// dropped anyway, so oversized copies stay internal only.
const OSC52_LIMIT: usize = 100_000;

fn offer_to_terminal(text: &str) {
    if text.is_empty() || text.len() > OSC52_LIMIT {
        return;
    }
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let triple = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_encoding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn multibyte_text_round_trips_through_the_encoder() {
        assert_eq!(base64("привет".as_bytes()), "0L/RgNC40LLQtdGC");
    }
}

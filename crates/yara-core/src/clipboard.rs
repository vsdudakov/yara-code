//! The system clipboard through whatever the platform has for it, and the
//! OSC 52 escape that reaches a clipboard through SSH.

use std::io::Write;
use std::process::{Command, Stdio};

/// The tools tried in turn, with their arguments, to write the clipboard.
const COPIERS: &[(&str, &[&str])] = &[
    ("pbcopy", &[]),
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["--clipboard", "--input"]),
    ("clip", &[]),
];

const PASTERS: &[(&str, &[&str])] = &[
    ("pbpaste", &[]),
    ("wl-paste", &["--no-newline"]),
    ("xclip", &["-selection", "clipboard", "-o"]),
    ("xsel", &["--clipboard", "--output"]),
    ("powershell", &["-command", "Get-Clipboard"]),
];

/// Hands `text` to the platform's clipboard tool. False when none of them
/// is there — over SSH, say — so the caller can fall back on OSC 52.
pub fn copy(text: &str) -> bool {
    COPIERS.iter().any(|(program, args)| {
        Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .and_then(|mut child| {
                child.stdin.take().unwrap().write_all(text.as_bytes())?;
                child.wait()
            })
            .is_ok_and(|status| status.success())
    })
}

pub fn paste() -> Option<String> {
    PASTERS.iter().find_map(|(program, args)| {
        let out = Command::new(program).args(*args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    })
}

/// The escape that asks the terminal itself to set the clipboard — what
/// works when the editor runs somewhere the clipboard is not.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |acc, b| (acc << 8) | *b as u32) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_escape_carries_the_text_in_base64() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
        assert_eq!(osc52("abc"), "\x1b]52;c;YWJj\x07");
        assert_eq!(osc52(""), "\x1b]52;c;\x07");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_clipboard_round_trips_where_there_is_one() {
        if copy("yara clipboard test") {
            assert_eq!(paste().as_deref(), Some("yara clipboard test"));
        }
    }
}

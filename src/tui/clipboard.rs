//! Copy and paste for the terminal frontend.
//!
//! A copy goes three places: an internal copy, the host terminal by OSC 52, and
//! the desktop's own clipboard tool. The OSC 52 sequence is what reaches the
//! *local* clipboard over SSH, where the editor is running on another machine;
//! the tool is what reaches it when the editor and the desktop are the same
//! machine and the terminal ignores the sequence. Writing both keeps the two
//! ends saying the same thing, so a paste can trust whichever answers.
//!
//! Pasting goes the other way: a terminal program cannot read the clipboard
//! itself, so the desktop's tool is asked — `pbpaste` on macOS, `wl-paste` or
//! `xclip` on Linux, PowerShell on Windows, told to write the text as it is
//! rather than print it — printing would add a newline the clipboard never held. That is also how an *image* reaches
//! the terminal panel: it is written to a file, and the path is what gets
//! pasted, which is what a program running in the shell can actually open.
//! Where no tool answers — over SSH, on a bare console — the internal copy is
//! all there is, and that is enough for copying and pasting within the editor.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Default)]
pub struct Clipboard {
    text: String,
}

impl Clipboard {
    pub fn set(&mut self, text: String) {
        self.text = text;
        offer_to_terminal(&self.text);
        set_system_text(&self.text);
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// What the system clipboard holds as text, or nothing when the desktop has no
/// tool to ask.
pub fn system_text() -> Option<String> {
    let text = if cfg!(target_os = "macos") {
        run("pbpaste", &[])
    } else if cfg!(windows) {
        // Written, not echoed: PowerShell ends anything it *prints* with a
        // newline that was never on the clipboard, and it prints in the
        // console's code page unless told otherwise.
        run(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "[Console]::OutputEncoding = [Text.Encoding]::UTF8; \
                 [Console]::Out.Write([string](Get-Clipboard -Raw))",
            ],
        )
    } else if wayland() {
        run("wl-paste", &["--no-newline"])
    } else if x11() {
        run("xclip", &["-selection", "clipboard", "-o"])
    } else {
        None
    }?;
    let text = String::from_utf8(text).ok()?;
    (!text.is_empty()).then_some(text)
}

/// Hands the copy to the desktop's clipboard, so it can be pasted into any
/// other program — and so a later paste here reads back what was copied rather
/// than whatever the clipboard held before.
fn set_system_text(text: &str) {
    if text.is_empty() {
        return;
    }
    if cfg!(target_os = "macos") {
        feed("pbcopy", &[], text);
    } else if cfg!(windows) {
        // Not `clip`: it reads its input in the console's code page, which
        // turns anything beyond ASCII into the wrong letters.
        feed(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "[Console]::InputEncoding = [Text.Encoding]::UTF8; \
                 Set-Clipboard -Value ([Console]::In.ReadToEnd())",
            ],
            text,
        );
    } else if wayland() {
        feed("wl-copy", &[], text);
    } else if x11() {
        feed("xclip", &["-selection", "clipboard"], text);
    }
}

/// Writes text to a clipboard tool's standard input. `wl-copy` and `xclip` stay
/// running to own the selection, so the child is waited on by a thread of its
/// own rather than here — otherwise every copy would leave a zombie behind.
fn feed(program: &str, args: &[&str], text: &str) {
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// The clipboard's image, written to a file of its own and handed back as that
/// path. Nothing when the clipboard holds no image, or when the desktop has no
/// tool to ask. The file lives in the system's temporary directory, which is
/// cleared by the system, not by the editor.
pub fn system_image() -> Option<PathBuf> {
    let path = image_path();
    let ok = if cfg!(target_os = "macos") {
        mac_clipboard_image(&path)
    } else if cfg!(windows) {
        windows_clipboard_image(&path)
    } else if wayland() {
        write_image(&path, run("wl-paste", &["--type", "image/png"]))
    } else if x11() {
        write_image(
            &path,
            run(
                "xclip",
                &["-selection", "clipboard", "-t", "image/png", "-o"],
            ),
        )
    } else {
        false
    };
    if !ok {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    Some(path)
}

/// A file for one pasted image, named so that two pastes never collide.
fn image_path() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("ycode-paste-{}-{n}.png", std::process::id()))
}

/// Runs a clipboard tool, treating "not installed" and "it failed" alike:
/// there is simply nothing to paste.
fn run(program: &str, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new(program).args(args).output().ok()?;
    output.status.success().then_some(output.stdout)
}

fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn x11() -> bool {
    std::env::var_os("DISPLAY").is_some()
}

/// True once the bytes are on disk and are actually an image.
fn write_image(path: &std::path::Path, bytes: Option<Vec<u8>>) -> bool {
    // A PNG starts with a fixed eight-byte signature; anything shorter is a
    // tool reporting that the clipboard holds something else.
    match bytes {
        Some(bytes) if bytes.starts_with(b"\x89PNG") => std::fs::write(path, bytes).is_ok(),
        _ => false,
    }
}

/// macOS keeps images on the pasteboard as a flavour of their own, and
/// AppleScript is what reads them. It converts to PNG on the way out, so a
/// screenshot taken as TIFF still arrives as one.
fn mac_clipboard_image(path: &std::path::Path) -> bool {
    let info = run("osascript", &["-e", "clipboard info"]).unwrap_or_default();
    let info = String::from_utf8_lossy(&info);
    if !(info.contains("PNGf") || info.contains("TIFF")) {
        return false;
    }
    let script = format!(
        "set f to open for access POSIX file \"{}\" with write permission\n\
         try\n\
             write (the clipboard as «class PNGf») to f\n\
         end try\n\
         close access f",
        path.display()
    );
    run("osascript", &["-e", &script]);
    is_image_file(path)
}

/// Windows hands the clipboard's bitmap to .NET, which writes the PNG.
fn windows_clipboard_image(path: &std::path::Path) -> bool {
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms, System.Drawing; \
         $image = [Windows.Forms.Clipboard]::GetImage(); \
         if ($image -ne $null) {{ \
             $image.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png) \
         }}",
        path.display()
    );
    run("powershell", &["-NoProfile", "-Command", &script]);
    is_image_file(path)
}

fn is_image_file(path: &std::path::Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.len() > 8)
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
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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

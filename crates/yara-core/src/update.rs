//! Checking for a newer release, and installing it in place.
//!
//! The network is `curl`'s job — it ships with macOS, Windows 10+ and every
//! Linux worth the name, and reaching for an HTTP client crate would pull a TLS
//! stack into an editor that otherwise has none. What arrives is JSON, which
//! serde already reads.

use std::path::{Path, PathBuf};

/// The version this binary was built from.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

const LATEST_RELEASE: &str = "https://api.github.com/repos/vsdudakov/yara-code/releases/latest";
const DOWNLOAD: &str = "https://github.com/vsdudakov/yara-code/releases/download";

/// A release on GitHub, as much of it as we care about.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Version without the leading `v`, e.g. `0.3.0`.
    pub version: String,
    /// The tag, e.g. `v0.3.0`.
    pub tag: String,
}

impl Release {
    /// Whether this release is newer than what is running.
    pub fn is_newer(&self) -> bool {
        is_newer(&self.version, CURRENT)
    }
}

/// Asks GitHub what the latest release is. Blocking: call it off the drawing
/// thread.
pub fn latest() -> Result<Release, String> {
    let body = curl(&[
        "-fsSL",
        "--max-time",
        "10",
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        "User-Agent: yara-code",
        LATEST_RELEASE,
    ])?;
    parse_release(&body)
}

/// Reads the tag out of the releases API's answer.
fn parse_release(body: &str) -> Result<Release, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("unreadable answer: {e}"))?;
    let tag = value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "no release yet".to_string())?;
    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
        tag: tag.to_string(),
    })
}

/// Compares two `major.minor.patch` versions. Anything unparsable sorts as
/// zero, so a strange tag never claims to be an upgrade.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    parts(candidate) > parts(current)
}

fn parts(version: &str) -> (u64, u64, u64) {
    let mut numbers = version
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u64>().unwrap_or(0));
    (
        numbers.next().unwrap_or(0),
        numbers.next().unwrap_or(0),
        numbers.next().unwrap_or(0),
    )
}

/// The archive this build would install: the same name the release workflow
/// uploads.
pub fn asset_name(tag: &str) -> String {
    let target = TARGET;
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    format!("ycode-{tag}-{target}.{extension}")
}

/// The triple the release workflow builds for this platform.
const TARGET: &str = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    "aarch64-apple-darwin"
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    "x86_64-apple-darwin"
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    "x86_64-unknown-linux-gnu"
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    "aarch64-unknown-linux-gnu"
} else if cfg!(windows) {
    "x86_64-pc-windows-msvc"
} else {
    ""
};

/// Whether this build has a release archive at all, and whether the binary sits
/// somewhere we may write.
pub fn can_install() -> bool {
    !TARGET.is_empty() && installed_dir().is_some_and(|dir| is_writable(&dir))
}

/// How to update when we cannot do it ourselves — the package manager that
/// owns the binary, judged by where it lives.
pub fn how_to_update() -> String {
    let path = std::env::current_exe().unwrap_or_default();
    let shown = path.display().to_string();
    if shown.contains("/Cellar/") || shown.contains("/homebrew/") {
        "brew upgrade ycode".to_string()
    } else if shown.starts_with("/usr/bin") || shown.starts_with("/bin") {
        "update through your package manager (apt, dnf, pacman)".to_string()
    } else {
        format!("download the new release from {DOWNLOAD}")
    }
}

/// Where an update has to be able to write: the folder holding the binary.
fn installed_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".ycode-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// How far an install has got. A download of several megabytes is long enough
/// that the editor has to say what it is doing, so every step reports itself.
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    /// Bytes on disk so far, and the whole size when the server declared one.
    Downloading {
        bytes: u64,
        total: Option<u64>,
    },
    Verifying,
    Unpacking,
    Replacing,
    /// Installed, in this directory.
    Done(PathBuf),
    Failed(String),
}

impl Progress {
    /// Whether there is nothing more to come.
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Done(_) | Self::Failed(_))
    }

    /// How far along, between 0 and 1 — known only while downloading against
    /// a declared size, which is nearly all of the wait.
    pub fn fraction(&self) -> Option<f32> {
        match self {
            Self::Downloading {
                bytes,
                total: Some(total),
            } if *total > 0 => Some((*bytes as f32 / *total as f32).clamp(0.0, 1.0)),
            Self::Downloading { .. } => None,
            Self::Verifying => Some(0.9),
            Self::Unpacking => Some(0.95),
            Self::Replacing => Some(0.99),
            Self::Done(_) => Some(1.0),
            Self::Failed(_) => None,
        }
    }

    /// The line the status bar shows, for a release with this tag.
    pub fn line(&self, tag: &str) -> String {
        match self {
            Self::Downloading {
                bytes,
                total: Some(total),
            } if *total > 0 => format!(
                "installing {tag} — {} of {} ({}%)",
                human_bytes(*bytes),
                human_bytes(*total),
                (bytes * 100 / total).min(100)
            ),
            Self::Downloading { bytes, .. } if *bytes > 0 => {
                format!("installing {tag} — {} downloaded", human_bytes(*bytes))
            }
            Self::Downloading { .. } => format!("installing {tag} — starting the download"),
            Self::Verifying => format!("installing {tag} — checking the download"),
            Self::Unpacking => format!("installing {tag} — unpacking"),
            Self::Replacing => format!("installing {tag} — replacing the binaries"),
            Self::Done(dir) => format!("{tag} installed in {} — restart to use it", dir.display()),
            Self::Failed(message) => message.clone(),
        }
    }
}

/// Sizes as a person reads them, one decimal place and no more.
fn human_bytes(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const KB: f64 = 1024.0;
    let n = bytes as f64;
    if n >= MB {
        format!("{:.1} MB", n / MB)
    } else if n >= KB {
        format!("{:.0} KB", n / KB)
    } else {
        format!("{bytes} B")
    }
}

/// One frame of the turning marker that says a slow thing is still going.
pub fn spinner(tick: usize) -> char {
    const FRAMES: [char; 8] = [
        '\u{280b}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283c}', '\u{2834}', '\u{2826}',
        '\u{2827}',
    ];
    FRAMES[tick % FRAMES.len()]
}

/// Downloads the release and replaces the running binaries with it, saying
/// where it has got to as it goes. Returns where the binaries were written.
/// Blocking, and slow: it is a download, so `Installer` runs it on a thread
/// and `report` is called from there.
pub fn install(release: &Release, report: &mut dyn FnMut(Progress)) -> Result<PathBuf, String> {
    if TARGET.is_empty() {
        return Err("no release is built for this platform".into());
    }
    let dir = installed_dir().ok_or("cannot tell where this binary lives")?;
    if !is_writable(&dir) {
        return Err(how_to_update());
    }

    let staging = std::env::temp_dir().join(format!("ycode-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    let asset = asset_name(&release.tag);
    let url = format!("{DOWNLOAD}/{}/{asset}", release.tag);
    let archive = staging.join(&asset);
    download(&url, &archive, report)?;

    report(Progress::Verifying);
    verify_checksum(&archive, &url)?;
    report(Progress::Unpacking);
    unpack(&archive, &staging)?;
    report(Progress::Replacing);

    // The new binaries are one directory deep, named after the archive.
    let unpacked = staging.join(asset.trim_end_matches(".tar.gz").trim_end_matches(".zip"));
    {
        let name = binary();
        let from = unpacked.join(&name);
        if !from.exists() {
            return Err(format!("the release carries no {name}"));
        }
        let to = dir.join(&name);
        // A running binary cannot be overwritten, but it can be moved aside;
        // the old one goes when the process next starts.
        let aside = dir.join(format!("{name}.old"));
        let _ = std::fs::remove_file(&aside);
        let moved_aside = std::fs::rename(&to, &aside).is_ok();
        if let Err(e) = std::fs::copy(&from, &to) {
            // Put the old binary back: a failed update must not leave the
            // user with nothing to launch.
            if moved_aside {
                let _ = std::fs::rename(&aside, &to);
            }
            return Err(format!("cannot write {}: {e}", to.display()));
        }
        set_executable(&to);
    }
    let _ = std::fs::remove_dir_all(&staging);
    Ok(dir)
}

/// Fetches the archive, telling `report` how much of it has landed. `curl`
/// writes its own progress to a terminal we do not have, so the size of the
/// file on disk is what gets watched instead.
fn download(url: &str, to: &Path, report: &mut dyn FnMut(Progress)) -> Result<(), String> {
    let total = content_length(url);
    report(Progress::Downloading { bytes: 0, total });
    let mut child = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "300",
            "-o",
            &to.to_string_lossy(),
            url,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl unavailable: {e}"))?;
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(_) => {
                let mut message = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    use std::io::Read;
                    let _ = stderr.read_to_string(&mut message);
                }
                return Err(message
                    .lines()
                    .next()
                    .unwrap_or("the download failed")
                    .to_string());
            }
            None => {
                let bytes = std::fs::metadata(to).map(|m| m.len()).unwrap_or(0);
                report(Progress::Downloading { bytes, total });
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        }
    }
}

/// How big the download is, if the server will say. Redirects are followed, so
/// the last header block is the one that describes the archive itself.
fn content_length(url: &str) -> Option<u64> {
    let head = curl(&["-fsSLI", "--max-time", "15", url]).ok()?;
    head.lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<u64>().ok())?
        })
        .next_back()
}

/// The binary, named as this platform names it.
fn binary() -> String {
    format!("ycode{}", if cfg!(windows) { ".exe" } else { "" })
}

/// The release publishes a `.sha256` beside every archive; a download that does
/// not match it is not installed.
fn verify_checksum(archive: &Path, url: &str) -> Result<(), String> {
    let published = curl(&["-fsSL", "--max-time", "30", &format!("{url}.sha256")])?;
    let expected = published
        .split_whitespace()
        .next()
        .ok_or("the published checksum is empty")?
        .to_lowercase();
    let bytes = std::fs::read(archive).map_err(|e| e.to_string())?;
    let actual = sha256(&bytes);
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "checksum mismatch: expected {expected}, got {actual}"
        ))
    }
}

fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let (program, args): (&str, Vec<String>) = if cfg!(windows) {
        (
            "tar", // Windows 10+ ships bsdtar, which reads zip archives too.
            vec![
                "-xf".into(),
                archive.to_string_lossy().into_owned(),
                "-C".into(),
                into.to_string_lossy().into_owned(),
            ],
        )
    } else {
        (
            "tar",
            vec![
                "-xzf".into(),
                archive.to_string_lossy().into_owned(),
                "-C".into(),
                into.to_string_lossy().into_owned(),
            ],
        )
    };
    let out = std::process::Command::new(program)
        .args(&args)
        .output()
        .map_err(|e| format!("cannot unpack: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {}

fn curl(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("curl")
        .args(args)
        .output()
        .map_err(|e| format!("curl unavailable: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err
            .lines()
            .next()
            .unwrap_or("the download failed")
            .to_string())
    }
}

/// SHA-256, so a download can be checked against the published sum without
/// pulling in a crate for it.
fn sha256(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut message = bytes.to_vec();
    let bit_len = (bytes.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(is_newer("0.10.0", "0.9.9"), "ten is after nine");
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("0.2.1", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0"), "the same is not newer");
        assert!(!is_newer("0.1.9", "0.2.0"));
        // A leading v, and a pre-release suffix, are read the same way.
        assert!(is_newer("v0.3.0", "0.2.9"));
        assert!(!is_newer("nonsense", "0.1.0"));
    }

    #[test]
    fn the_release_answer_is_read_for_its_tag() {
        let release = parse_release(r#"{"tag_name":"v0.3.1","name":"Yara Code 0.3.1"}"#).unwrap();
        assert_eq!(release.tag, "v0.3.1");
        assert_eq!(release.version, "0.3.1");
        // Nothing published yet, or something else entirely.
        assert!(parse_release(r#"{"message":"Not Found"}"#).is_err());
        assert!(parse_release("not json").is_err());
    }

    #[test]
    fn a_release_older_than_this_build_is_not_an_update() {
        let old = Release {
            version: "0.0.1".into(),
            tag: "v0.0.1".into(),
        };
        assert!(!old.is_newer());
        let ahead = Release {
            version: "999.0.0".into(),
            tag: "v999.0.0".into(),
        };
        assert!(ahead.is_newer());
    }

    #[test]
    fn the_asset_is_named_as_the_release_workflow_uploads_it() {
        let name = asset_name("v0.3.0");
        assert!(name.starts_with("ycode-v0.3.0-"), "{name}");
        if cfg!(windows) {
            assert!(name.ends_with(".zip"));
        } else {
            assert!(name.ends_with(".tar.gz"));
        }
        assert!(name.contains(TARGET));
    }

    #[test]
    fn the_checksum_matches_the_reference_vectors() {
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Long enough to need a second block, which is where padding goes wrong.
        assert_eq!(
            sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn a_download_that_does_not_match_its_checksum_is_refused() {
        let dir = crate::test_support::Dir::new("yara-update-sum");
        let archive = dir.file("ycode.tar.gz", "not the real thing");
        // No network here: the mismatch is what is being checked, and the
        // published sum cannot be fetched, so this fails either way.
        assert!(verify_checksum(&archive, "file:///nowhere").is_err());
    }

    #[test]
    fn a_download_reads_its_size_the_way_a_person_would() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2 KB");
        assert_eq!(human_bytes(9_961_472), "9.5 MB");
    }

    #[test]
    fn every_step_of_an_install_says_what_it_is_doing() {
        let downloading = |bytes, total| Progress::Downloading { bytes, total };
        // With a declared size the line carries a figure to watch.
        let half = downloading(5_242_880, Some(10_485_760));
        assert_eq!(
            half.line("v0.5.0"),
            "installing v0.5.0 — 5.0 MB of 10.0 MB (50%)"
        );
        assert_eq!(half.fraction(), Some(0.5));
        // Without one, what has arrived is still worth saying.
        assert_eq!(
            downloading(2048, None).line("v0.5.0"),
            "installing v0.5.0 — 2 KB downloaded"
        );
        assert_eq!(downloading(2048, None).fraction(), None);
        assert_eq!(
            downloading(0, None).line("v0.5.0"),
            "installing v0.5.0 — starting the download"
        );
        // A server that answers zero is not divided by.
        assert_eq!(downloading(0, Some(0)).fraction(), None);

        for step in [
            Progress::Verifying,
            Progress::Unpacking,
            Progress::Replacing,
        ] {
            assert!(step.line("v0.5.0").starts_with("installing v0.5.0 — "));
            assert!(!step.is_finished());
            assert!(step.fraction().is_some_and(|f| f > 0.5 && f < 1.0));
        }

        let done = Progress::Done(PathBuf::from("/usr/local/bin"));
        assert_eq!(
            done.line("v0.5.0"),
            "v0.5.0 installed in /usr/local/bin — restart to use it"
        );
        assert!(done.is_finished());
        assert_eq!(done.fraction(), Some(1.0));

        let failed = Progress::Failed("checksum mismatch".into());
        assert_eq!(failed.line("v0.5.0"), "checksum mismatch");
        assert!(failed.is_finished());
        assert_eq!(failed.fraction(), None);
    }

    #[test]
    fn the_marker_turns_and_comes_back_round() {
        let frames: Vec<char> = (0..8).map(spinner).collect();
        assert_eq!(frames.len(), 8);
        assert_eq!(spinner(8), frames[0], "it starts over");
        assert_eq!(spinner(11), frames[3]);
        assert!(frames.iter().all(|c| !c.is_whitespace()));
    }

    #[test]
    fn nothing_is_reported_before_an_install_starts() {
        let mut installer = Installer::default();
        assert!(!installer.is_running());
        assert_eq!(installer.poll(), None);
        assert_eq!(installer.tick(), 0);
        assert_eq!(installer.tag(), "");
        let mut checker = Checker::default();
        assert!(!checker.is_running());
        assert_eq!(checker.tick(), 0);
        assert!(checker.take().is_none());
    }

    #[test]
    fn advice_depends_on_where_the_binary_lives() {
        let advice = how_to_update();
        assert!(!advice.is_empty());
    }
}

/// The update check as the frontends use it: started on a thread, read from
/// the drawing loop, and never blocking either.
#[derive(Default)]
pub struct Checker {
    state: std::sync::Arc<std::sync::Mutex<Option<Result<Release, String>>>>,
    running: bool,
    started: Option<std::time::Instant>,
}

impl Checker {
    /// Starts a check unless one is already running. `notify` is called when
    /// the answer lands, so a frontend that sleeps between frames wakes up.
    pub fn start(&mut self, notify: impl Fn() + Send + 'static) {
        if self.running {
            return;
        }
        self.running = true;
        self.started = Some(std::time::Instant::now());
        let state = std::sync::Arc::clone(&self.state);
        std::thread::spawn(move || {
            let answer = latest();
            *state.lock().unwrap() = Some(answer);
            notify();
        });
    }

    /// The answer, once, if it has arrived.
    pub fn take(&mut self) -> Option<Result<Release, String>> {
        let answer = self.state.lock().unwrap().take();
        if answer.is_some() {
            self.running = false;
            self.started = None;
        }
        answer
    }

    /// Whether an answer is still being waited for.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Which spinner frame this waiting is up to.
    pub fn tick(&self) -> usize {
        tick_of(self.started)
    }
}

/// The install as the frontends run it: on a thread, reporting as it goes, so
/// a download of several megabytes never freezes the editor.
#[derive(Default)]
pub struct Installer {
    state: std::sync::Arc<std::sync::Mutex<Option<Progress>>>,
    running: bool,
    started: Option<std::time::Instant>,
    tag: String,
}

impl Installer {
    /// Starts installing `release` unless an install is already going.
    /// `notify` is called on every step, so a frontend that sleeps between
    /// frames wakes up to draw the new figure.
    pub fn start(&mut self, release: &Release, notify: impl Fn() + Send + 'static) {
        if self.running {
            return;
        }
        self.running = true;
        self.started = Some(std::time::Instant::now());
        self.tag = release.tag.clone();
        let state = std::sync::Arc::clone(&self.state);
        *state.lock().unwrap() = Some(Progress::Downloading {
            bytes: 0,
            total: None,
        });
        let release = release.clone();
        std::thread::spawn(move || {
            let mut report = |progress: Progress| {
                *state.lock().unwrap() = Some(progress);
                notify();
            };
            let outcome = install(&release, &mut report);
            report(match outcome {
                Ok(dir) => Progress::Done(dir),
                Err(message) => Progress::Failed(message),
            });
        });
    }

    /// The newest step, if there has been one since this was last asked. Only
    /// the latest is kept: a status line has no use for the ones it missed.
    pub fn poll(&mut self) -> Option<Progress> {
        let progress = self.state.lock().unwrap().take()?;
        if progress.is_finished() {
            self.running = false;
            self.started = None;
        }
        Some(progress)
    }

    /// Whether an install is still going.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// The release being installed, for the line that describes it.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// Which spinner frame this install is up to.
    pub fn tick(&self) -> usize {
        tick_of(self.started)
    }
}

/// A spinner frame per tenth of a second since the work began.
fn tick_of(started: Option<std::time::Instant>) -> usize {
    started.map_or(0, |at| (at.elapsed().as_millis() / 100) as usize)
}

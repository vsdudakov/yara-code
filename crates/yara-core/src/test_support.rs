//! A scratch directory for tests, removed when it goes out of scope.
//!
//! The tests here touch the filesystem on purpose — settings files, project
//! folders, themes on disk — so they need somewhere of their own to work, one
//! per test, never the user's own files.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// `YARA_CONFIG_DIR` is one variable for the whole test binary; a test that
/// sets it holds this for as long as it relies on it.
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct Dir(PathBuf);

impl Dir {
    /// A fresh directory named after the test, the process and a counter, so
    /// tests running side by side never share one.
    pub fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        // Symlinked temp directories (macOS puts /tmp behind /private) would
        // otherwise make canonicalised paths compare unequal, and Windows
        // spells a canonical path with a `\\?\` prefix that git will not
        // take on a command line.
        Self(crate::git::canonical(&path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes a file, creating parents, and returns its path.
    pub fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

//! File operations the navigator performs, kept out of the UI layer.

use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum FsError {
    Exists,
    Invalid,
    Io(std::io::Error),
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exists => write!(f, "already exists"),
            Self::Invalid => write!(f, "invalid path"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Creates an empty file, making any missing parent directories.
pub fn create_file(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(FsError::Invalid);
    }
    let path = parent.join(name);
    if path.exists() {
        return Err(FsError::Exists);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(FsError::Io)?;
    }
    std::fs::write(&path, "").map_err(FsError::Io)?;
    Ok(path)
}

pub fn create_dir(parent: &Path, name: &str) -> Result<PathBuf, FsError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(FsError::Invalid);
    }
    let path = parent.join(name);
    if path.exists() {
        return Err(FsError::Exists);
    }
    std::fs::create_dir_all(&path).map_err(FsError::Io)?;
    Ok(path)
}

pub fn delete(path: &Path) -> Result<(), FsError> {
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(FsError::Io)
    } else {
        std::fs::remove_file(path).map_err(FsError::Io)
    }
}

pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, FsError> {
    let new_name = new_name.trim();
    if new_name.is_empty() || new_name.contains('/') {
        return Err(FsError::Invalid);
    }
    let parent = path.parent().ok_or(FsError::Invalid)?;
    let to = parent.join(new_name);
    if to == path {
        return Ok(to);
    }
    if to.exists() {
        return Err(FsError::Exists);
    }
    std::fs::rename(path, &to).map_err(FsError::Io)?;
    Ok(to)
}

/// Moves `src` into `dest_dir`. Refuses no-op moves and moving a directory into
/// itself, either of which would corrupt the tree.
pub fn move_into(src: &Path, dest_dir: &Path) -> Result<PathBuf, FsError> {
    if src.parent() == Some(dest_dir) || dest_dir.starts_with(src) {
        return Err(FsError::Invalid);
    }
    let name = src.file_name().ok_or(FsError::Invalid)?;
    let to = dest_dir.join(name);
    if to.exists() {
        return Err(FsError::Exists);
    }
    std::fs::rename(src, &to).map_err(FsError::Io)?;
    Ok(to)
}

/// Directory listing for the navigator: folders first, then files, each
/// alphabetical, with `.git` hidden.
pub fn list_dir(dir: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<(PathBuf, bool)> = rd
        .filter_map(|e| e.ok())
        .map(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            (e.path(), is_dir)
        })
        .filter(|(p, _)| p.file_name().is_none_or(|n| n != ".git"))
        .collect();
    entries.sort_by(|(pa, da), (pb, db)| {
        db.cmp(da).then_with(|| {
            let na = pa.file_name().unwrap_or_default().to_ascii_lowercase();
            let nb = pb.file_name().unwrap_or_default().to_ascii_lowercase();
            na.cmp(&nb)
        })
    });
    entries
}

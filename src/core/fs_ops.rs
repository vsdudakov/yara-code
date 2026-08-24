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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_support::Dir;

    #[test]
    fn a_new_file_is_empty_and_refuses_to_overwrite() {
        let dir = Dir::new("yara-fs-new");
        let path = create_file(dir.path(), "notes.txt").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        assert!(
            matches!(create_file(dir.path(), "notes.txt"), Err(FsError::Exists)),
            "a second file of the same name would silently replace the first"
        );
        assert!(matches!(
            create_file(dir.path(), "   "),
            Err(FsError::Invalid)
        ));
    }

    #[test]
    fn a_new_file_brings_its_parents_with_it() {
        let dir = Dir::new("yara-fs-parents");
        let path = create_file(dir.path(), "a/b/c.txt").unwrap();
        assert!(path.is_file());
        assert!(dir.path().join("a").join("b").is_dir());
    }

    #[test]
    fn folders_are_created_and_deleted_whole() {
        let dir = Dir::new("yara-fs-dir");
        let nested = create_dir(dir.path(), "pkg/inner").unwrap();
        assert!(nested.is_dir());
        assert!(matches!(
            create_dir(dir.path(), "pkg"),
            Err(FsError::Exists)
        ));
        create_file(&nested, "file.txt").unwrap();
        delete(&dir.path().join("pkg")).unwrap();
        assert!(!dir.path().join("pkg").exists(), "the whole tree goes");
    }

    #[test]
    fn renaming_keeps_the_file_in_place() {
        let dir = Dir::new("yara-fs-rename");
        let path = dir.file("before.txt", "body");
        let renamed = rename(&path, "after.txt").unwrap();
        assert_eq!(renamed, dir.path().join("after.txt"));
        assert_eq!(std::fs::read_to_string(&renamed).unwrap(), "body");
        // A name that is really a path would move the file somewhere else.
        assert!(matches!(
            rename(&renamed, "sub/x.txt"),
            Err(FsError::Invalid)
        ));
        assert!(matches!(rename(&renamed, ""), Err(FsError::Invalid)));
        // Renaming to itself is a no-op, not a failure.
        assert_eq!(rename(&renamed, "after.txt").unwrap(), renamed);
        dir.file("taken.txt", "");
        assert!(matches!(
            rename(&renamed, "taken.txt"),
            Err(FsError::Exists)
        ));
    }

    #[test]
    fn moving_refuses_what_would_corrupt_the_tree() {
        let dir = Dir::new("yara-fs-move");
        let file = dir.file("file.txt", "x");
        let target = create_dir(dir.path(), "target").unwrap();
        let moved = move_into(&file, &target).unwrap();
        assert_eq!(moved, target.join("file.txt"));

        // Already there.
        assert!(matches!(move_into(&moved, &target), Err(FsError::Invalid)));
        // A folder into itself.
        let inner = create_dir(&target, "inner").unwrap();
        assert!(matches!(move_into(&target, &inner), Err(FsError::Invalid)));
        // Onto a name that is taken.
        let other = create_dir(dir.path(), "other").unwrap();
        std::fs::write(other.join("file.txt"), "").unwrap();
        assert!(matches!(move_into(&moved, &other), Err(FsError::Exists)));
    }

    #[test]
    fn a_listing_puts_folders_first_and_hides_the_repository() {
        let dir = Dir::new("yara-fs-list");
        dir.file("beta.txt", "");
        dir.file("Alpha.txt", "");
        create_dir(dir.path(), "zeta").unwrap();
        create_dir(dir.path(), "Middle").unwrap();
        create_dir(dir.path(), ".git").unwrap();

        let names: Vec<String> = list_dir(dir.path())
            .into_iter()
            .map(|(path, is_dir)| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if is_dir {
                    format!("{name}/")
                } else {
                    name
                }
            })
            .collect();
        assert_eq!(names, ["Middle/", "zeta/", "Alpha.txt", "beta.txt"]);
        assert!(list_dir(&dir.path().join("nowhere")).is_empty());
    }

    #[test]
    fn errors_say_what_went_wrong() {
        assert_eq!(FsError::Exists.to_string(), "already exists");
        assert_eq!(FsError::Invalid.to_string(), "invalid path");
        let io = FsError::Io(std::io::Error::other("disk gone"));
        assert!(io.to_string().contains("disk gone"));
    }
}

//! Shared helpers for tests: uniquely named temporary directories that are
//! removed when dropped, including when a test fails part-way.

use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Set to keep every test directory for inspection instead of removing it.
pub const KEEP_ENV: &str = "FERRULE_KEEP_TEST_DIRS";

/// A new directory under [`std::env::temp_dir`] (which honours `TMPDIR`),
/// removed with its contents on drop unless [`TempDir::keep`] was called or
/// [`KEEP_ENV`] is set.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
    armed: bool,
}

impl TempDir {
    /// Creates `ferrule-<label>-<pid>-<nanos>-<counter>`; the name is unique
    /// within the process and across concurrent test processes.
    pub fn new(label: &str) -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ferrule-{label}-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self { path, armed: true })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `contents` to `relative`, creating parent directories.
    pub fn write(
        &self,
        relative: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> io::Result<PathBuf> {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, contents)?;
        Ok(path)
    }

    /// Retains the directory and returns its path.
    pub fn keep(mut self) -> PathBuf {
        self.armed = false;
        std::mem::take(&mut self.path)
    }

    /// Removes the directory and everything in it, reporting failure.
    pub fn close(mut self) -> io::Result<()> {
        self.armed = false;
        std::fs::remove_dir_all(&self.path)
    }

    /// Removes the directory, failing if the test left anything in it.
    /// On failure the directory is still removed when the guard drops.
    pub fn remove_empty(mut self) -> io::Result<()> {
        std::fs::remove_dir(&self.path)?;
        self.armed = false;
        Ok(())
    }
}

impl Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if std::env::var_os(KEEP_ENV).is_some() {
            eprintln!("kept test directory: {}", self.path.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_removes_the_directory_and_its_contents() {
        let directory = TempDir::new("drop").unwrap();
        let file = directory.write("nested/file.txt", "text").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "text");
        let path = directory.path().to_path_buf();
        drop(directory);
        assert!(!path.exists());
    }

    #[test]
    fn a_failing_test_still_removes_its_directory() {
        let path = std::panic::catch_unwind(|| {
            let directory = TempDir::new("panic").unwrap();
            let path = directory.path().to_path_buf();
            std::panic::panic_any(path);
        })
        .unwrap_err()
        .downcast::<PathBuf>()
        .unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn names_are_unique_and_keep_retains_the_directory() {
        let first = TempDir::new("same").unwrap();
        let second = TempDir::new("same").unwrap();
        assert_ne!(first.path(), second.path());
        let kept = first.keep();
        assert!(kept.is_dir());
        std::fs::remove_dir(&kept).unwrap();
    }

    #[test]
    fn remove_empty_refuses_leftovers_and_drop_still_cleans_up() {
        let directory = TempDir::new("leftover").unwrap();
        directory.write("left.txt", "x").unwrap();
        let path = directory.path().to_path_buf();
        assert!(directory.remove_empty().is_err());
        assert!(!path.exists());

        let empty = TempDir::new("empty").unwrap();
        let path = empty.path().to_path_buf();
        empty.remove_empty().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn close_removes_contents() {
        let directory = TempDir::new("close").unwrap();
        directory.write("a/b.txt", "x").unwrap();
        let path = directory.path().to_path_buf();
        directory.close().unwrap();
        assert!(!path.exists());
    }
}

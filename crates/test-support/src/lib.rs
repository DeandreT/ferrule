//! Shared helpers for tests: uniquely named temporary directories that are
//! removed when dropped, including when a test fails part-way.

use std::io;
use std::ops::Deref;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Set to keep every test directory for inspection instead of removing it.
pub const KEEP_ENV: &str = "FERRULE_KEEP_TEST_DIRS";

/// A new directory under [`std::env::temp_dir`] (which honours `TMPDIR`),
/// removed with its contents on drop unless [`TempDir::keep`] was called or
/// [`KEEP_ENV`] was set when the guard was created.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
    armed: bool,
    retain: bool,
}

impl TempDir {
    /// Creates `ferrule-<label>-<pid>-<nanos>-<counter>`; the name is unique
    /// within the process and across concurrent test processes.
    pub fn new(label: &str) -> io::Result<Self> {
        Self::with_retention(label, std::env::var_os(KEEP_ENV).is_some())
    }

    fn with_retention(label: &str, retain: bool) -> io::Result<Self> {
        if label.contains(['/', '\\']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "temporary directory labels must not contain path separators",
            ));
        }
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
        Ok(Self {
            path,
            armed: true,
            retain,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `contents` to `relative`, creating parent directories.
    /// Absolute paths, parent components and symlinks are refused.
    pub fn write(
        &self,
        relative: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> io::Result<PathBuf> {
        let relative = relative.as_ref();
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fixture paths must stay inside the temporary directory",
            ));
        }
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            let mut directory = self.path.clone();
            for part in parent.strip_prefix(&self.path).map_err(io::Error::other)? {
                directory.push(part);
                match std::fs::symlink_metadata(&directory) {
                    Ok(metadata) if metadata.is_dir() => {}
                    Ok(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "fixture parents must be directories, not symlinks",
                        ));
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        std::fs::create_dir(&directory)?;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_symlink()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fixture files must not be symlinks",
            ));
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
    /// When retention was requested, leaves it intact instead.
    pub fn close(mut self) -> io::Result<()> {
        if self.retain {
            return Ok(());
        }
        std::fs::remove_dir_all(&self.path)?;
        self.armed = false;
        Ok(())
    }

    /// Removes the directory, failing if the test left anything in it.
    /// On failure the directory is still removed when the guard drops.
    /// Retention keeps the directory but still checks that it is empty.
    pub fn remove_empty(mut self) -> io::Result<()> {
        if self.retain {
            if std::fs::read_dir(&self.path)?.next().transpose()?.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::DirectoryNotEmpty,
                    "test directory is not empty",
                ));
            }
            return Ok(());
        }
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
        if self.retain {
            eprintln!("kept test directory: {}", self.path.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cleanup tests are independent of retention selected for the surrounding
    // test run. The environment contract is exercised in child processes below.
    fn temporary(label: &str) -> TempDir {
        TempDir::with_retention(label, false).unwrap()
    }

    #[test]
    fn drop_removes_the_directory_and_its_contents() {
        let directory = temporary("drop");
        let file = directory.write("nested/file.txt", "text").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "text");
        let path = directory.path().to_path_buf();
        drop(directory);
        assert!(!path.exists());
    }

    #[test]
    fn a_failing_test_still_removes_its_directory() {
        let path = std::panic::catch_unwind(|| {
            let directory = temporary("panic");
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
        let first = temporary("same");
        let second = temporary("same");
        assert_ne!(first.path(), second.path());
        let kept = first.keep();
        assert!(kept.is_dir());
        std::fs::remove_dir(&kept).unwrap();
    }

    #[test]
    fn remove_empty_refuses_leftovers_and_drop_still_cleans_up() {
        let directory = temporary("leftover");
        directory.write("left.txt", "x").unwrap();
        let path = directory.path().to_path_buf();
        assert!(directory.remove_empty().is_err());
        assert!(!path.exists());

        let empty = temporary("empty");
        let path = empty.path().to_path_buf();
        empty.remove_empty().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn close_removes_contents() {
        let directory = temporary("close");
        directory.write("a/b.txt", "x").unwrap();
        let path = directory.path().to_path_buf();
        directory.close().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn fixture_paths_cannot_escape_the_directory() {
        let directory = temporary("paths");
        let outside = temporary("outside");
        let absolute = outside.write("existing.txt", "original").unwrap();
        let escaped = directory.path().with_extension("escaped");
        let traversal = Path::new("..").join(escaped.file_name().unwrap());
        for path in [traversal.as_path(), absolute.as_path()] {
            assert_eq!(
                directory.write(path, "changed").unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert_eq!(std::fs::read_to_string(&absolute).unwrap(), "original");
        assert!(!escaped.exists());
        assert_eq!(
            TempDir::new("parent/nested").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[cfg(unix)]
    #[test]
    fn fixture_paths_refuse_symlink_parents_and_files() {
        use std::os::unix::fs::symlink;

        let directory = temporary("symlinks");
        let outside = temporary("outside");
        let file = outside.write("original.txt", "original").unwrap();
        symlink(outside.path(), directory.join("parent")).unwrap();
        symlink(&file, directory.join("file")).unwrap();
        for path in ["parent/nested/new.txt", "file"] {
            assert_eq!(
                directory.write(path, "changed").unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert!(!outside.join("nested").exists());
        assert_eq!(std::fs::read_to_string(file).unwrap(), "original");
    }

    #[test]
    fn retention_is_consistent_for_drop_close_and_remove_empty() {
        for retain in [false, true] {
            let report_directory = temporary("retention-report");
            let report = report_directory.join("paths");
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "tests::retention_child"])
                .env("FERRULE_TEST_SUPPORT_RETENTION_REPORT", &report);
            if retain {
                command.env(KEEP_ENV, "1");
            } else {
                command.env_remove(KEEP_ENV);
            }
            let output = command.output().unwrap();
            assert!(output.status.success(), "{output:?}");
            let paths = std::fs::read_to_string(report).unwrap();
            assert_eq!(paths.lines().count(), 4);
            for path in paths.lines().map(Path::new) {
                assert_eq!(path.exists(), retain, "{}", path.display());
                if retain {
                    std::fs::remove_dir_all(path).unwrap();
                }
            }
        }
    }

    #[test]
    fn retention_child() {
        let Some(report) = std::env::var_os("FERRULE_TEST_SUPPORT_RETENTION_REPORT") else {
            return;
        };
        let mut paths = Vec::new();
        let drop_directory = TempDir::new("retention-drop").unwrap();
        drop_directory.write("file", "kept").unwrap();
        paths.push(drop_directory.path().to_owned());
        drop(drop_directory);
        let close_directory = TempDir::new("retention-close").unwrap();
        close_directory.write("file", "kept").unwrap();
        paths.push(close_directory.path().to_owned());
        close_directory.close().unwrap();
        let empty_directory = TempDir::new("retention-empty").unwrap();
        paths.push(empty_directory.path().to_owned());
        empty_directory.remove_empty().unwrap();
        let leftover_directory = TempDir::new("retention-leftover").unwrap();
        leftover_directory.write("file", "kept").unwrap();
        paths.push(leftover_directory.path().to_owned());
        assert_eq!(
            leftover_directory.remove_empty().unwrap_err().kind(),
            io::ErrorKind::DirectoryNotEmpty
        );
        std::fs::write(
            report,
            paths
                .iter()
                .map(|path| format!("{}\n", path.display()))
                .collect::<String>(),
        )
        .unwrap();
    }
}

//! Staged replacement for files the editor owns.
//!
//! A document is written completely to a new sibling file, synced, and only
//! then renamed over its destination, so a failed save leaves either the
//! previous complete bytes or the new complete bytes on disk. Callers update
//! their save point only after [`replace`] returns.
//!
//! Destination policy:
//! - A symbolic link is resolved and its final target is replaced, so the link
//!   itself survives, as it did with a direct write. Dangling links are refused.
//! - Existing read-only files and non-file destinations are refused rather than
//!   silently replaced by the rename.
//! - The replacement adopts the existing file's permissions. Ownership and
//!   extended attributes are not carried over.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context as _, bail};

static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

pub fn replace(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let destination = resolve_destination(path)?;
    let existing = match fs::metadata(&destination) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => bail!("could not inspect {}: {error}", path.display()),
    };
    if let Some(metadata) = &existing {
        if !metadata.is_file() {
            bail!("{} is not a regular file", path.display());
        }
        if metadata.permissions().readonly() {
            bail!("{} is read-only", path.display());
        }
    }
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = destination
        .file_name()
        .with_context(|| format!("{} has no file name", path.display()))?
        .to_string_lossy();
    for _ in 0..32 {
        let nonce = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
        let staged = parent.join(format!(".{name}.{}-{nonce}.tmp", std::process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => bail!("could not stage {}: {error}", path.display()),
        };
        let result = (|| -> std::io::Result<()> {
            write_staged(&mut file, bytes, &destination)?;
            if let Some(metadata) = &existing {
                file.set_permissions(metadata.permissions())?;
            }
            file.sync_all()?;
            drop(file);
            fault::check(&destination, fault::Fault::BeforePublish)?;
            fs::rename(&staged, &destination)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&staged);
            bail!("could not save {}: {error}", path.display());
        }
        sync_directory(parent);
        return Ok(());
    }
    bail!("could not reserve a temporary file for {}", path.display())
}

fn resolve_destination(path: &Path) -> anyhow::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            fs::canonicalize(path).map_err(|error| {
                anyhow::anyhow!(
                    "could not resolve symbolic link {}: {error}",
                    path.display()
                )
            })
        }
        _ => Ok(path.to_path_buf()),
    }
}

fn write_staged(file: &mut fs::File, bytes: &[u8], destination: &Path) -> std::io::Result<()> {
    if fault::armed(destination, fault::Fault::PartialWrite) {
        file.write_all(&bytes[..bytes.len() / 2])?;
        return Err(std::io::Error::other("injected write failure"));
    }
    file.write_all(bytes)
}

/// The rename is already published; a failed directory sync only weakens
/// durability across power loss, so it is not reported as a failed save.
fn sync_directory(directory: &Path) {
    #[cfg(unix)]
    if let Ok(directory) = fs::File::open(directory) {
        let _ = directory.sync_all();
    }
    #[cfg(not(unix))]
    let _ = directory;
}

#[cfg(not(test))]
mod fault {
    use std::path::Path;

    #[derive(Clone, Copy)]
    pub enum Fault {
        PartialWrite,
        BeforePublish,
    }

    pub fn armed(_: &Path, _: Fault) -> bool {
        false
    }

    pub fn check(_: &Path, _: Fault) -> std::io::Result<()> {
        Ok(())
    }
}

/// Per-thread storage faults for tests; each armed fault fires once for its
/// exact destination path.
#[cfg(test)]
pub(crate) mod fault {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Fault {
        /// Writes half of the document to the staged file, then fails.
        PartialWrite,
        /// Fails after the staged file is complete but before the rename.
        BeforePublish,
    }

    thread_local! {
        static ARMED: RefCell<Vec<(PathBuf, Fault)>> = const { RefCell::new(Vec::new()) };
    }

    pub fn arm(destination: &Path, fault: Fault) {
        ARMED.with(|armed| armed.borrow_mut().push((destination.to_path_buf(), fault)));
    }

    pub fn armed(destination: &Path, fault: Fault) -> bool {
        ARMED.with(|armed| {
            let mut armed = armed.borrow_mut();
            let position = armed
                .iter()
                .position(|(path, armed)| path == destination && *armed == fault);
            position.map(|index| armed.remove(index)).is_some()
        })
    }

    pub fn check(destination: &Path, fault: Fault) -> std::io::Result<()> {
        if armed(destination, fault) {
            Err(std::io::Error::other(format!("injected {fault:?} failure")))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::fault::{Fault, arm};
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> std::io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule-gui-document-file-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path)?;
            Ok(Self(path))
        }

        fn entries(&self) -> std::io::Result<Vec<String>> {
            let mut names = fs::read_dir(&self.0)?
                .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
                .collect::<std::io::Result<Vec<_>>>()?;
            names.sort();
            Ok(names)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn replace_creates_and_replaces_complete_documents() -> anyhow::Result<()> {
        let temp = TestDir::new()?;
        let path = temp.0.join("project.json");
        replace(&path, b"first")?;
        assert_eq!(fs::read(&path)?, b"first");
        replace(&path, b"second document")?;
        assert_eq!(fs::read(&path)?, b"second document");
        assert_eq!(temp.entries()?, ["project.json"]);
        Ok(())
    }

    #[test]
    fn injected_failures_keep_the_previous_complete_bytes() -> anyhow::Result<()> {
        let temp = TestDir::new()?;
        let path = temp.0.join("project.json");
        fs::write(&path, b"previous complete document")?;
        for fault in [Fault::PartialWrite, Fault::BeforePublish] {
            arm(&path, fault);
            let error = replace(&path, b"replacement document").expect_err("fault is injected");
            assert!(error.to_string().contains("injected"), "{error:#}");
            assert_eq!(fs::read(&path)?, b"previous complete document");
            assert_eq!(temp.entries()?, ["project.json"]);
        }
        replace(&path, b"replacement document")?;
        assert_eq!(fs::read(&path)?, b"replacement document");
        Ok(())
    }

    #[test]
    fn read_only_and_directory_destinations_are_refused() -> anyhow::Result<()> {
        let temp = TestDir::new()?;
        let path = temp.0.join("locked.json");
        fs::write(&path, b"locked")?;
        let writable = fs::metadata(&path)?.permissions();
        let mut locked = writable.clone();
        locked.set_readonly(true);
        fs::set_permissions(&path, locked)?;
        let error = replace(&path, b"changed").expect_err("read-only file is refused");
        assert!(error.to_string().contains("read-only"), "{error:#}");
        assert_eq!(fs::read(&path)?, b"locked");
        fs::set_permissions(&path, writable)?;

        let directory = temp.0.join("folder.json");
        fs::create_dir(&directory)?;
        let error = replace(&directory, b"changed").expect_err("directory is refused");
        assert!(
            error.to_string().contains("not a regular file"),
            "{error:#}"
        );
        assert_eq!(temp.entries()?, ["folder.json", "locked.json"]);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn replacement_keeps_permissions_and_symbolic_links() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = TestDir::new()?;
        let target = temp.0.join("target.json");
        fs::write(&target, b"old")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
        let link = temp.0.join("link.json");
        std::os::unix::fs::symlink("target.json", &link)?;

        replace(&link, b"new")?;

        assert!(fs::symlink_metadata(&link)?.file_type().is_symlink());
        assert_eq!(fs::read(&target)?, b"new");
        assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(temp.entries()?, ["link.json", "target.json"]);

        let dangling = temp.0.join("dangling.json");
        std::os::unix::fs::symlink("missing.json", &dangling)?;
        assert!(replace(&dangling, b"new").is_err());
        assert!(!temp.0.join("missing.json").exists());
        Ok(())
    }
}

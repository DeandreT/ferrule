//! Resolve installed tool aliases without relaxing original-file custody.
use std::{
    io,
    path::{Path, PathBuf},
};

pub(super) fn resolve(path: &Path, name: &str) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(io::Error::other(format!(
            "an absolute {name} executable is required"
        )));
    }
    let resolved = path.canonicalize()?;
    if !std::fs::symlink_metadata(&resolved)?.file_type().is_file() {
        return Err(io::Error::other(format!(
            "a regular {name} executable is required"
        )));
    }
    Ok(resolved)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::{fs::symlink, net::UnixListener};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "gnu-tool-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn gnu_tool_aliases_resolve_to_the_same_regular_target() {
        let root = Fixture::new();
        let target = root.0.join("real tool");
        std::fs::write(&target, "fixture").unwrap();
        let first = root.0.join("first");
        let second = root.0.join("second");
        symlink(&target, &first).unwrap();
        symlink(&first, &second).unwrap();
        let expected = target.canonicalize().unwrap();
        for path in [&target, &first, &second] {
            assert_eq!(resolve(path, "GNU timeout").unwrap(), expected);
        }
        // Tool resolution must not rewrite aliases or change lstat semantics.
        assert!(
            std::fs::symlink_metadata(&first)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            std::fs::symlink_metadata(&second)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn gnu_tool_resolution_rejects_relative_missing_and_nonfiles() {
        let root = Fixture::new();
        assert!(resolve(Path::new("relative-tool"), "GNU time").is_err());
        assert!(resolve(&root.0.join("missing"), "GNU time").is_err());
        assert!(resolve(&root.0, "GNU time").is_err());
        let dangling = root.0.join("dangling");
        symlink(root.0.join("missing"), &dangling).unwrap();
        assert!(resolve(&dangling, "GNU time").is_err());
        let socket = root.0.join("socket");
        let _listener = UnixListener::bind(&socket).unwrap();
        assert!(resolve(&socket, "GNU time").is_err());
        let cycle = root.0.join("cycle");
        symlink(&cycle, &cycle).unwrap();
        assert!(resolve(&cycle, "GNU time").is_err());
    }
}

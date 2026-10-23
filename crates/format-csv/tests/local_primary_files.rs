use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use format_csv::{LocalFileError, LocalPrimaryFiles, PrimarySourceHost};
use mapping::{EmptyFileSelection, PrimaryFileSelection};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-local-primary-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn write(&self, path: &str, contents: &str) -> std::io::Result<()> {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(path, contents)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const WILDCARD: PrimaryFileSelection = PrimaryFileSelection::FileNameWildcard {
    empty: EmptyFileSelection::Refuse,
};

#[test]
fn wildcards_select_regular_files_in_scalar_order_and_direct_requests_are_lazy()
-> Result<(), Box<dyn std::error::Error>> {
    let root = TempDir::new()?;
    for name in ["in/b.csv", "in/B.csv", "in/雪.csv", "in/a.csv", "in/a.txt"] {
        root.write(name, "Text\nx\n")?;
    }
    std::fs::create_dir(root.0.join("in/dir.csv"))?;
    let files = LocalPrimaryFiles::new(&root.0)?;
    assert_eq!(
        files.select("./in\\*.csv", WILDCARD)?,
        ["in/B.csv", "in/a.csv", "in/b.csv", "in/雪.csv"]
    );
    assert_eq!(
        files.select("in/?.csv", WILDCARD)?,
        ["in/B.csv", "in/a.csv", "in/b.csv", "in/雪.csv"]
    );
    // Direct selection performs no filesystem access, even for a missing file.
    assert_eq!(
        files.select("in//absent.csv", PrimaryFileSelection::Direct)?,
        ["in/absent.csv"]
    );
    assert!(matches!(
        files.load("in/absent.csv"),
        Err(LocalFileError::Missing { path }) if path == "in/absent.csv"
    ));
    let loaded = files.load("in/a.csv")?;
    assert_eq!(loaded.bytes, b"Text\nx\n");
    assert_eq!(
        loaded.resolved_path.as_deref().map(Path::new),
        Some(files.root().join("in").join("a.csv").as_path())
    );
    assert!(matches!(
        files.clone().with_max_document_bytes(6).load("in/a.csv"),
        Err(LocalFileError::TooLarge { limit: 6, .. })
    ));
    assert!(matches!(
        files.load("in/dir.csv"),
        Err(LocalFileError::NotRegular { .. })
    ));
    assert!(matches!(
        files.select("out/*.csv", WILDCARD),
        Err(LocalFileError::Missing { path }) if path == "out"
    ));
    assert!(matches!(
        files.select("in/*.json", WILDCARD),
        Err(LocalFileError::NoMatches { .. })
    ));
    assert!(
        files
            .select(
                "in/*.json",
                PrimaryFileSelection::FileNameWildcard {
                    empty: EmptyFileSelection::MapNoDocuments
                }
            )?
            .is_empty()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_refused_and_direct_wildcard_characters_are_literal()
-> Result<(), Box<dyn std::error::Error>> {
    let root = TempDir::new()?;
    let outside = TempDir::new()?;
    outside.write("secret.csv", "Text\nsecret\n")?;
    root.write("in/a.csv", "Text\na\n")?;
    root.write("in/*.csv", "Text\nliteral\n")?;
    std::os::unix::fs::symlink(outside.0.join("secret.csv"), root.0.join("link.csv"))?;
    std::os::unix::fs::symlink(&outside.0, root.0.join("away"))?;
    let files = LocalPrimaryFiles::new(&root.0)?;
    let direct = files.select("in/*.csv", PrimaryFileSelection::Direct)?;
    assert_eq!(direct, ["in/*.csv"]);
    assert_eq!(files.load(&direct[0])?.bytes, b"Text\nliteral\n");
    assert!(matches!(
        files.load("link.csv"),
        Err(LocalFileError::Symlink { path }) if path == "link.csv"
    ));
    assert!(matches!(
        files.load("away/secret.csv"),
        Err(LocalFileError::Symlink { .. })
    ));
    assert!(matches!(
        files.select("away/*.csv", WILDCARD),
        Err(LocalFileError::Symlink { .. })
    ));
    assert!(matches!(
        files.select("*.csv", WILDCARD),
        Err(LocalFileError::Symlink { path }) if path == "link.csv"
    ));
    Ok(())
}

#[test]
fn a_missing_root_is_refused_before_any_selection() {
    assert!(matches!(
        LocalPrimaryFiles::new(Path::new("/definitely/not/a/ferrule/root")),
        Err(LocalFileError::Root { .. })
    ));
}

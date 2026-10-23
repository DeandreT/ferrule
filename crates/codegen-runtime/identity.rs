//! Source identity of the `codegen-runtime` package.
//!
//! Shared by this package's build script, which exports the identity of the
//! runtime being compiled, and by `codegen-rust`, which records the identity
//! generated mappings require and checks a declared runtime directory. The
//! encoding matches `codegen::RuntimeIdentity::of_sources`.

use std::fs;
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Package files outside `src/` that belong to the identity.
const ROOT_FILES: [&str; 3] = ["Cargo.toml", "build.rs", "identity.rs"];

/// `sha256:` and the lowercase hex digest of the package sources: the root
/// files above and every file under `src/`, in relative-path order, each as
/// its `/`-separated path, a NUL, its little-endian 64-bit length and bytes.
pub fn runtime_source_identity(root: &Path) -> io::Result<String> {
    let mut files = Vec::new();
    for name in ROOT_FILES {
        files.push((name.to_owned(), fs::read(root.join(name))?));
    }
    collect(&root.join("src"), "src", &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (path, contents) in &files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update((contents.len() as u64).to_le_bytes());
        hasher.update(contents);
    }
    let mut identity = String::from("sha256:");
    for byte in hasher.finalize() {
        identity.push_str(&format!("{byte:02x}"));
    }
    Ok(identity)
}

fn collect(directory: &Path, relative: &str, files: &mut Vec<(String, Vec<u8>)>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().into_string().map_err(|name| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("runtime source name {name:?} is not UTF-8"),
            )
        })?;
        let path = format!("{relative}/{name}");
        let kind = fs::metadata(entry.path())?;
        if kind.is_dir() {
            collect(&entry.path(), &path, files)?;
        } else {
            files.push((path, fs::read(entry.path())?));
        }
    }
    Ok(())
}

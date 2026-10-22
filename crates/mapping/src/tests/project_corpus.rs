//! Serialized Projects checked into the repository, shared by tests that run
//! analyses over every authored fixture and conformance model.

use std::path::{Path, PathBuf};

/// Directories under the workspace root that hold authored Project files.
const ROOTS: &[&str] = &["conformance", "crates", "examples"];
/// Generated, vendored, or build output trees never hold authored fixtures.
const SKIPPED: &[&str] = &["target", "node_modules", ".git", "bin", "obj"];
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Every JSON file below the workspace that `decode` accepts, in path order.
pub(crate) fn projects<T>(decode: impl Fn(&str) -> Option<T>) -> Vec<(PathBuf, T)> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for root in ROOTS {
        collect(&workspace.join(root), &mut files);
    }
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            // Cheap prefilter: every Project carries a graph and a root scope.
            if !text.contains("\"graph\"") || !text.contains("\"root\"") {
                return None;
            }
            decode(&text).map(|project| (path, project))
        })
        .collect()
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let skipped = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| SKIPPED.contains(&name));
            if !skipped {
                collect(&path, files);
            }
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
            && entry
                .metadata()
                .is_ok_and(|metadata| metadata.len() <= MAX_BYTES)
        {
            files.push(path);
        }
    }
}

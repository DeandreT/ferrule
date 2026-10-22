//! Serialized Projects checked into the repository, shared by the tests that
//! compare an analysis with the derivation it replaced over every authored
//! fixture and conformance model.

use std::path::{Path, PathBuf};

use mapping::Project;

/// Directories under the workspace root that hold authored Project files.
const ROOTS: &[&str] = &["conformance", "crates", "examples"];
/// Generated, vendored, or build output trees never hold authored fixtures.
const SKIPPED: &[&str] = &["target", "node_modules", ".git", "bin", "obj"];

/// Every JSON file below the authored roots that decodes as a Project, in
/// path order.
pub(crate) fn projects() -> Vec<Project> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for root in ROOTS {
        collect(&workspace.join(root), &mut files);
    }
    files.sort();
    files
        .into_iter()
        .filter_map(|file| {
            let text = std::fs::read_to_string(file).ok()?;
            // Cheap prefilter: every Project carries a graph and a root scope.
            if !text.contains("\"graph\"") || !text.contains("\"root\"") {
                return None;
            }
            serde_json::from_str(&text).ok()
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
        {
            files.push(path);
        }
    }
}

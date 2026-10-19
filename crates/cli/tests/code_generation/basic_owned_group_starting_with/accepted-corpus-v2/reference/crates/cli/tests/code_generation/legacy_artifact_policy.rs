//! Explicit conservation of historical files beside reviewed runtime updates.
//! Candidate runtime expectations come from source snapshots, never emitted files.
use sha2::{Digest as _, Sha256};
use std::{fs, io, path::Path};

pub type ManifestRow = (String, usize, String);
pub type SourceSnapshot = Vec<(String, Vec<u8>)>;

pub const ADJACENT_RUNTIME_FILES: [(&str, &str); 3] = [
    (
        "Runtime/FerruleBasicGroupBy.cs",
        "runtime/csharp/Ferrule.Runtime/FerruleBasicGroupBy.cs",
    ),
    (
        "Runtime/FerruleInstance.cs",
        "runtime/csharp/Ferrule.Runtime/FerruleInstance.cs",
    ),
    (
        "Runtime/ScopeContext.cs",
        "runtime/csharp/Ferrule.Runtime/ScopeContext.cs",
    ),
];

#[derive(Clone, Copy, Debug)]
pub enum Policy {
    ExactHistorical,
    ReviewedAdjacentRuntime,
}

#[derive(Debug)]
pub struct Comparison {
    pub historical_exact: bool,
    pub paths_exact: bool,
    pub stable_files_exact: bool,
    pub runtime_sources_exact: bool,
}

impl Comparison {
    pub fn accepts(&self, policy: Policy) -> bool {
        match policy {
            Policy::ExactHistorical => self.historical_exact,
            Policy::ReviewedAdjacentRuntime => {
                self.paths_exact && self.stable_files_exact && self.runtime_sources_exact
            }
        }
    }
}

pub fn row(path: &str, bytes: &[u8]) -> ManifestRow {
    (
        path.into(),
        bytes.len(),
        format!("{:x}", Sha256::digest(bytes)),
    )
}

pub fn snapshot(root: &Path) -> io::Result<SourceSnapshot> {
    ADJACENT_RUNTIME_FILES
        .iter()
        .map(|(artifact, source)| Ok(((*artifact).into(), fs::read(root.join(source))?)))
        .collect()
}

pub fn snapshot_unchanged(root: &Path, original: &SourceSnapshot) -> io::Result<bool> {
    Ok(snapshot(root)? == *original)
}

/// Both inventories must be complete, ordered and unique. Only the three named
/// runtime bodies have an alternate expected source; every other row is historical.
pub fn compare(
    historical: &[ManifestRow],
    actual: &[ManifestRow],
    sources: &SourceSnapshot,
) -> Comparison {
    let paths_exact = historical.len() == actual.len()
        && historical.windows(2).all(|pair| pair[0].0 < pair[1].0)
        && historical
            .iter()
            .zip(actual)
            .all(|(old, new)| old.0 == new.0);
    let sources_valid = sources.len() == ADJACENT_RUNTIME_FILES.len()
        && sources
            .iter()
            .zip(ADJACENT_RUNTIME_FILES)
            .all(|((path, _), (artifact, _))| path == artifact)
        && ADJACENT_RUNTIME_FILES
            .iter()
            .all(|(path, _)| historical.iter().any(|entry| entry.0 == *path));
    let stable_files_exact = paths_exact
        && historical.iter().zip(actual).all(|(old, new)| {
            ADJACENT_RUNTIME_FILES
                .iter()
                .any(|(path, _)| old.0 == *path)
                || old == new
        });
    let runtime_sources_exact = paths_exact
        && sources_valid
        && sources.iter().all(|(path, bytes)| {
            actual
                .iter()
                .find(|entry| entry.0 == *path)
                .is_some_and(|entry| *entry == row(path, bytes))
        });
    Comparison {
        historical_exact: paths_exact && historical == actual,
        paths_exact,
        stable_files_exact,
        runtime_sources_exact,
    }
}

#[test]
fn reviewed_runtime_updates_do_not_forgive_other_artifact_changes() {
    let mut historical = vec![row("GeneratedMapping.cs", b"stable mapping")];
    let sources: SourceSnapshot = ADJACENT_RUNTIME_FILES
        .iter()
        .map(|(path, _)| ((*path).into(), format!("reviewed {path}").into_bytes()))
        .collect();
    for (path, _) in ADJACENT_RUNTIME_FILES {
        historical.push(row(path, b"historical runtime"));
    }
    let mut actual = vec![historical[0].clone()];
    actual.extend(sources.iter().map(|(path, bytes)| row(path, bytes)));
    let result = compare(&historical, &actual, &sources);
    assert!(result.accepts(Policy::ReviewedAdjacentRuntime));
    assert!(!result.accepts(Policy::ExactHistorical));
    assert!(compare(&historical, &historical, &sources).accepts(Policy::ExactHistorical));

    let mut changed_mapping = actual.clone();
    changed_mapping[0] = row("GeneratedMapping.cs", b"changed mapping");
    let mut wrong_runtime = actual.clone();
    wrong_runtime[1] = row(&wrong_runtime[1].0, b"unreviewed runtime");
    let mut added = actual.clone();
    added.push(row("unexpected.cs", b"extra"));
    let mut missing = actual.clone();
    missing.remove(1);
    for invalid in [changed_mapping, wrong_runtime, added, missing] {
        assert!(!compare(&historical, &invalid, &sources).accepts(Policy::ReviewedAdjacentRuntime));
    }
    let mut changed_snapshot = sources.clone();
    changed_snapshot[0].1.push(b'!');
    assert!(
        !compare(&historical, &actual, &changed_snapshot).accepts(Policy::ReviewedAdjacentRuntime)
    );
    let mut unknown_source = sources.clone();
    unknown_source[0].0 = "Runtime/Unreviewed.cs".into();
    assert!(
        !compare(&historical, &actual, &unknown_source).accepts(Policy::ReviewedAdjacentRuntime)
    );
}

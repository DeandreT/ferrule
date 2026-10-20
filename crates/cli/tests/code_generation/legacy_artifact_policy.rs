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

pub const ADDED_RUNTIME_FILES: [(&str, &str); 2] = [
    (
        "Runtime/Json/FerruleJsonSourceDocument.cs",
        "runtime/csharp/Ferrule.Runtime/Json/FerruleJsonSourceDocument.cs",
    ),
    (
        "Runtime/Json/FerruleJsonSourceOccurrence.cs",
        "runtime/csharp/Ferrule.Runtime/Json/FerruleJsonSourceOccurrence.cs",
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
        .chain(ADDED_RUNTIME_FILES.iter())
        .map(|(artifact, source)| Ok(((*artifact).into(), fs::read(root.join(source))?)))
        .collect()
}

pub fn snapshot_unchanged(root: &Path, original: &SourceSnapshot) -> io::Result<bool> {
    Ok(snapshot(root)? == *original)
}

/// Inventories must be complete, ordered and unique. The three reviewed replacement
/// bodies and two added source files must match independently captured source bytes;
/// every other row remains historical. ExactHistorical never admits additions.
pub fn compare(
    historical: &[ManifestRow],
    actual: &[ManifestRow],
    sources: &SourceSnapshot,
) -> Comparison {
    let historical_ordered = historical.windows(2).all(|pair| pair[0].0 < pair[1].0);
    let mut expected_paths = historical
        .iter()
        .map(|entry| entry.0.as_str())
        .collect::<Vec<_>>();
    expected_paths.extend(ADDED_RUNTIME_FILES.iter().map(|(path, _)| *path));
    expected_paths.sort_unstable();
    let paths_exact = historical_ordered
        && expected_paths.windows(2).all(|pair| pair[0] < pair[1])
        && expected_paths
            .iter()
            .copied()
            .eq(actual.iter().map(|entry| entry.0.as_str()));
    let sources_valid = sources.len() == ADJACENT_RUNTIME_FILES.len() + ADDED_RUNTIME_FILES.len()
        && sources
            .iter()
            .zip(
                ADJACENT_RUNTIME_FILES
                    .into_iter()
                    .chain(ADDED_RUNTIME_FILES),
            )
            .all(|((path, _), (artifact, _))| path == artifact)
        && ADJACENT_RUNTIME_FILES
            .iter()
            .all(|(path, _)| historical.iter().any(|entry| entry.0 == *path));
    let stable_files_exact = paths_exact
        && historical.iter().all(|old| {
            ADJACENT_RUNTIME_FILES
                .iter()
                .any(|(path, _)| old.0 == *path)
                || actual.iter().any(|new| old == new)
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
        historical_exact: historical_ordered && historical == actual,
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
        .chain(ADDED_RUNTIME_FILES.iter())
        .map(|(path, _)| ((*path).into(), format!("reviewed {path}").into_bytes()))
        .collect();
    for (path, _) in ADJACENT_RUNTIME_FILES {
        historical.push(row(path, b"historical runtime"));
    }
    let mut actual = vec![historical[0].clone()];
    actual.extend(sources.iter().map(|(path, bytes)| row(path, bytes)));
    actual.sort_by(|a, b| a.0.cmp(&b.0));
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
    let mut wrong_added = actual.clone();
    let index = wrong_added
        .iter()
        .position(|entry| entry.0 == ADDED_RUNTIME_FILES[0].0)
        .unwrap();
    wrong_added[index] = row(ADDED_RUNTIME_FILES[0].0, b"unreviewed addition");
    let mut missing_added = actual.clone();
    missing_added.remove(index);
    let mut duplicate = actual.clone();
    duplicate.insert(index, actual[index].clone());
    let mut reordered = actual.clone();
    reordered.swap(0, 1);
    for invalid in [
        changed_mapping,
        wrong_runtime,
        added,
        missing,
        wrong_added,
        missing_added,
        duplicate,
        reordered,
    ] {
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

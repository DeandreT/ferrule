//! Conservation of frozen historical artifacts beside the current embedded runtime.
//! Runtime expectations come from the runtime source tree, never emitted files.
use sha2::{Digest as _, Sha256};
use std::path::Path;

pub type ManifestRow = (String, usize, String);
pub type SourceSnapshot = Vec<(String, Vec<u8>)>;

/// The C# emitter embeds `runtime/csharp/Ferrule.Runtime/<path>` as `Runtime/<path>`.
pub const RUNTIME_ARTIFACT_PREFIX: &str = "Runtime/";
pub const RUNTIME_SOURCE_ROOT: &str = "runtime/csharp/Ferrule.Runtime";

#[derive(Clone, Copy, Debug)]
pub enum Policy {
    ExactHistorical,
    CurrentRuntimeSources,
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
            Policy::CurrentRuntimeSources => {
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

/// Every current runtime source file, keyed by the artifact path it is emitted at.
pub fn snapshot(root: &Path) -> super::TestResult<SourceSnapshot> {
    Ok(super::artifact_files(&root.join(RUNTIME_SOURCE_ROOT))?
        .into_iter()
        .map(|(path, bytes)| (format!("{RUNTIME_ARTIFACT_PREFIX}{path}"), bytes))
        .collect())
}

pub fn snapshot_unchanged(root: &Path, original: &SourceSnapshot) -> super::TestResult<bool> {
    Ok(snapshot(root)? == *original)
}

fn is_runtime(path: &str) -> bool {
    path.starts_with(RUNTIME_ARTIFACT_PREFIX)
}

fn strictly_ordered<'a>(paths: impl IntoIterator<Item = &'a str>) -> bool {
    let paths = paths.into_iter().collect::<Vec<_>>();
    paths.windows(2).all(|pair| pair[0] < pair[1])
}

/// Inventories must be ordered and unique. Rows outside `Runtime/` must equal
/// the historical rows exactly; every emitted `Runtime/` row must equal its
/// source snapshot. ExactHistorical additionally requires the complete
/// historical inventory, including its historical runtime rows.
pub fn compare(
    historical: &[ManifestRow],
    actual: &[ManifestRow],
    sources: &SourceSnapshot,
) -> Comparison {
    let historical_ordered = strictly_ordered(historical.iter().map(|entry| entry.0.as_str()));
    let actual_ordered = strictly_ordered(actual.iter().map(|entry| entry.0.as_str()));
    let stable = |rows: &[ManifestRow]| {
        rows.iter()
            .filter(|entry| !is_runtime(&entry.0))
            .cloned()
            .collect::<Vec<_>>()
    };
    let (historical_stable, actual_stable) = (stable(historical), stable(actual));
    let paths_exact = historical_ordered
        && actual_ordered
        && historical_stable
            .iter()
            .map(|entry| &entry.0)
            .eq(actual_stable.iter().map(|entry| &entry.0));
    let sources_valid = strictly_ordered(sources.iter().map(|(path, _)| path.as_str()))
        && sources.iter().all(|(path, _)| is_runtime(path));
    let runtime_sources_exact = actual_ordered
        && sources_valid
        && actual
            .iter()
            .filter(|entry| is_runtime(&entry.0))
            .all(|entry| {
                sources
                    .iter()
                    .find(|(path, _)| *path == entry.0)
                    .is_some_and(|(path, bytes)| *entry == row(path, bytes))
            });
    Comparison {
        historical_exact: historical_ordered && historical == actual,
        paths_exact,
        stable_files_exact: paths_exact && historical_stable == actual_stable,
        runtime_sources_exact,
    }
}

#[test]
fn current_runtime_sources_do_not_forgive_other_artifact_changes() {
    let sources: SourceSnapshot = ["Runtime/A.cs", "Runtime/B.cs", "Runtime/Json/C.cs"]
        .into_iter()
        .map(|path| (path.into(), format!("current {path}").into_bytes()))
        .collect();
    let historical = vec![
        row("Ferrule.Generated.csproj", b"stable project"),
        row("GeneratedMapping.cs", b"stable mapping"),
        row("Runtime/A.cs", b"historical runtime"),
        row("Runtime/B.cs", b"historical runtime"),
    ];
    let mut actual = historical[..2].to_vec();
    actual.extend(sources.iter().map(|(path, bytes)| row(path, bytes)));
    let result = compare(&historical, &actual, &sources);
    assert!(result.accepts(Policy::CurrentRuntimeSources), "{result:?}");
    assert!(!result.accepts(Policy::ExactHistorical));
    let historical_result = compare(&historical, &historical, &sources);
    assert!(historical_result.accepts(Policy::ExactHistorical));
    assert!(!historical_result.accepts(Policy::CurrentRuntimeSources));
    // A source the emitter does not embed imposes no expectation.
    let mut unused = actual.clone();
    unused.retain(|entry| entry.0 != "Runtime/Json/C.cs");
    assert!(compare(&historical, &unused, &sources).accepts(Policy::CurrentRuntimeSources));

    let mut changed_mapping = actual.clone();
    changed_mapping[1] = row("GeneratedMapping.cs", b"changed mapping");
    let mut changed_runtime = actual.clone();
    changed_runtime[2] = row("Runtime/A.cs", b"differs from source");
    let mut unsourced = actual.clone();
    unsourced.push(row("Runtime/Unsourced.cs", b"no source"));
    let mut extra = actual.clone();
    extra.insert(2, row("GeneratedOther.cs", b"extra"));
    let mut missing = actual.clone();
    missing.remove(0);
    let mut duplicate = actual.clone();
    duplicate.insert(2, actual[2].clone());
    let mut duplicate_stable = actual.clone();
    duplicate_stable.insert(1, actual[1].clone());
    let mut reordered = actual.clone();
    reordered.swap(0, 1);
    let mut reordered_runtime = actual.clone();
    reordered_runtime.swap(2, 3);
    for invalid in [
        changed_mapping,
        changed_runtime,
        unsourced,
        extra,
        missing,
        duplicate,
        duplicate_stable,
        reordered,
        reordered_runtime,
    ] {
        assert!(
            !compare(&historical, &invalid, &sources).accepts(Policy::CurrentRuntimeSources),
            "{invalid:?}"
        );
    }
    let mut changed_source = sources.clone();
    changed_source[0].1.push(b'!');
    assert!(!compare(&historical, &actual, &changed_source).accepts(Policy::CurrentRuntimeSources));
    let mut misplaced_source = sources.clone();
    misplaced_source[0].0 = "GeneratedMapping.cs".into();
    assert!(
        !compare(&historical, &actual, &misplaced_source).accepts(Policy::CurrentRuntimeSources)
    );
    let mut reordered_historical = historical.clone();
    reordered_historical.swap(0, 1);
    assert!(
        !compare(&reordered_historical, &actual, &sources).accepts(Policy::CurrentRuntimeSources)
    );
}

#[test]
fn runtime_snapshot_maps_the_emitted_runtime_to_its_source_tree() -> super::TestResult<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace")?;
    let sources = snapshot(root)?;
    let project = mapping::project_file::decode_str(include_str!(
        "../../../../conformance/basic-owned-group-adjacent-by/legacy-project.json"
    ))?;
    let artifacts = codegen_csharp::emit(&codegen::lower(&project)?)?;
    let mut runtime = 0;
    for file in artifacts.files() {
        if let Some(relative) = file.path.as_str().strip_prefix(RUNTIME_ARTIFACT_PREFIX) {
            runtime += 1;
            let source = std::fs::read(root.join(RUNTIME_SOURCE_ROOT).join(relative))?;
            assert_eq!(file.contents, source, "{}", file.path.as_str());
        }
    }
    assert!(runtime > 0);
    let actual = artifacts
        .files()
        .iter()
        .map(|file| row(file.path.as_str(), &file.contents))
        .collect::<Vec<_>>();
    assert!(compare(&actual, &actual, &sources).accepts(Policy::CurrentRuntimeSources));
    assert!(snapshot_unchanged(root, &sources)?);
    Ok(())
}

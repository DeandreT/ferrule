// Starting remains separately typed: an Adjacent Comparison cannot be retagged.
pub const STARTING_RUNTIME_FILES: [(&str, &str); 3] = [
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
#[derive(Debug)]
pub struct ReviewedStartingSnapshot(SourceSnapshot);
#[derive(Debug)]
pub struct StartingComparison {
    pub historical_exact: bool,
    pub paths_exact: bool,
    pub stable_files_exact: bool,
    pub runtime_sources_exact: bool,
    _starting: (),
}
impl StartingComparison {
    pub fn accepts(&self, policy: Policy) -> bool {
        match policy {
            Policy::ExactHistorical => self.historical_exact,
            Policy::ReviewedStartingRuntime => {
                self.paths_exact && self.stable_files_exact && self.runtime_sources_exact
            }
            Policy::ReviewedAdjacentRuntime
            | Policy::ReviewedStartingRuntimeWithMergedJson42686c55 => false,
        }
    }
}
pub fn starting_snapshot(root: &Path) -> io::Result<SourceSnapshot> {
    STARTING_RUNTIME_FILES
        .iter()
        .map(|(artifact, source)| Ok(((*artifact).into(), fs::read(root.join(source))?)))
        .collect()
}
pub fn bind_starting_snapshot(
    sources: SourceSnapshot,
    accepted_rows: &[ManifestRow],
) -> io::Result<ReviewedStartingSnapshot> {
    let paths_valid = sources.len() == STARTING_RUNTIME_FILES.len()
        && sources
            .iter()
            .zip(STARTING_RUNTIME_FILES)
            .all(|((path, _), (artifact, _))| path == artifact);
    let rows: Vec<_> = sources
        .iter()
        .map(|(path, bytes)| row(path, bytes))
        .collect();
    if !paths_valid || rows != accepted_rows {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Starting requires exact ordered unique independently reviewed source rows",
        ));
    }
    Ok(ReviewedStartingSnapshot(sources))
}
pub fn starting_snapshot_unchanged(
    root: &Path,
    original: &ReviewedStartingSnapshot,
) -> io::Result<bool> {
    Ok(starting_snapshot(root)? == original.0)
}
pub fn compare_starting(
    historical: &[ManifestRow],
    actual: &[ManifestRow],
    sources: &ReviewedStartingSnapshot,
) -> StartingComparison {
    let historical_bound = historical_m78_rows().is_ok_and(|rows| rows == historical);
    let paths_exact = historical_bound
        && historical.len() == actual.len()
        && historical.windows(2).all(|pair| pair[0].0 < pair[1].0)
        && historical
            .iter()
            .zip(actual)
            .all(|(old, new)| old.0 == new.0);
    let sources_valid = sources.0.len() == STARTING_RUNTIME_FILES.len()
        && sources
            .0
            .iter()
            .zip(STARTING_RUNTIME_FILES)
            .all(|((path, _), (artifact, _))| path == artifact)
        && STARTING_RUNTIME_FILES
            .iter()
            .all(|(path, _)| historical.iter().any(|entry| entry.0 == *path));
    StartingComparison {
        historical_exact: paths_exact && historical == actual,
        paths_exact,
        _starting: (),
        stable_files_exact: paths_exact
            && historical.iter().zip(actual).all(|(old, new)| {
                STARTING_RUNTIME_FILES
                    .iter()
                    .any(|(path, _)| old.0 == *path)
                    || old == new
            }),
        runtime_sources_exact: paths_exact
            && sources_valid
            && sources.0.iter().all(|(path, bytes)| {
                actual
                    .iter()
                    .find(|entry| entry.0 == *path)
                    .is_some_and(|entry| *entry == row(path, bytes))
            }),
    }
}

pub const HISTORICAL_M_COMMIT: &str = "f30f6ca1c56a533bd4ccb4e4f4bfb821d939e971";
pub const HISTORICAL_M_TREE: &str = "2748a46873ef858c9c498d49f8d865e005b3366a";
pub const MERGED_JSON_N_COMMIT: &str = "42686c55fd8e3d3fa2ba18eaed4d2bdef3f82818";
pub const MERGED_JSON_N_TREE: &str = "77fdfad36d9c19777365d1896515abe867b6f634";
pub const MERGED_JSON42686C55_FILES: [(&str, &str, &str, usize, &str); 2] = [
    (
        "Runtime/Json/FerruleJsonSourceDocument.cs",
        "runtime/csharp/Ferrule.Runtime/Json/FerruleJsonSourceDocument.cs",
        "d2421811e0e71efae27b82ece3293f4b5d5223e1",
        8706,
        "5ee2ed77d6b509005b511a6be396779b46220da37c62c5423122257870f08a74",
    ),
    (
        "Runtime/Json/FerruleJsonSourceOccurrence.cs",
        "runtime/csharp/Ferrule.Runtime/Json/FerruleJsonSourceOccurrence.cs",
        "189130301b30e9149d0a5f921ac89acf02c96ebd",
        3447,
        "d35dbdce7c955f6064ff5896e2d94e333f2bc47beada7d4b964308d35c961768",
    ),
];
pub fn merged_json42686c55_rows() -> Vec<ManifestRow> {
    MERGED_JSON42686C55_FILES
        .iter()
        .map(|(path, _, _, len, sha)| ((*path).into(), *len, (*sha).into()))
        .collect()
}
pub fn merged_json42686c55_fixed_bodies() -> io::Result<SourceSnapshot> {
    let bodies:SourceSnapshot=vec![
        (MERGED_JSON42686C55_FILES[0].0.into(),include_bytes!("basic_owned_group_starting_with/merged-json-42686c55/FerruleJsonSourceDocument.cs").to_vec()),
        (MERGED_JSON42686C55_FILES[1].0.into(),include_bytes!("basic_owned_group_starting_with/merged-json-42686c55/FerruleJsonSourceOccurrence.cs").to_vec()),
    ];
    if bodies.iter().map(|(p, b)| row(p, b)).collect::<Vec<_>>() != merged_json42686c55_rows() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fixed N Git bodies changed; never sample current emission",
        ));
    }
    Ok(bodies)
}
fn historical_m78_rows() -> io::Result<Vec<ManifestRow>> {
    let bytes = include_bytes!(
        "basic_owned_group_starting_with/historical-package/legacy-baseline/manifest.json"
    );
    if format!("{:x}", Sha256::digest(bytes))
        != "8fceace43e25a4c66282e369be054f26e029b62567b75505bfe0415b354e03a0"
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "original M manifest changed",
        ));
    }
    let manifest: serde_json::Value = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    let rows: Vec<ManifestRow> =
        serde_json::from_value(manifest["csharp"].clone()).map_err(io::Error::other)?;
    if manifest["provenance"]["source_commit"] != HISTORICAL_M_COMMIT
        || manifest["provenance"]["source_tree"] != HISTORICAL_M_TREE
        || rows.len() != 78
        || !rows.windows(2).all(|p| p[0].0 < p[1].0)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "exact ordered M78 required",
        ));
    }
    Ok(rows)
}
#[derive(Debug)]
pub struct ReviewedStartingWithMergedJson42686c55Snapshot {
    starting: ReviewedStartingSnapshot,
    historical: Vec<ManifestRow>,
}
pub fn bind_starting_with_merged_json42686c55(
    sources: SourceSnapshot,
    accepted_three: &[ManifestRow],
    fixed_two: &[ManifestRow],
    implementation_commit: &str,
    implementation_tree: &str,
) -> io::Result<ReviewedStartingWithMergedJson42686c55Snapshot> {
    if implementation_commit != MERGED_JSON_N_COMMIT
        || implementation_tree != MERGED_JSON_N_TREE
        || fixed_two != merged_json42686c55_rows()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "distinct N commit/tree and exact two immutable addition rows required",
        ));
    }
    merged_json42686c55_fixed_bodies()?;
    Ok(ReviewedStartingWithMergedJson42686c55Snapshot {
        starting: bind_starting_snapshot(sources, accepted_three)?,
        historical: historical_m78_rows()?,
    })
}
pub fn starting_merged_json42686c55_snapshot_unchanged(
    root: &Path,
    reviewed: &ReviewedStartingWithMergedJson42686c55Snapshot,
) -> io::Result<bool> {
    let fixed = merged_json42686c55_fixed_bodies()?;
    Ok(starting_snapshot_unchanged(root, &reviewed.starting)?
        && MERGED_JSON42686C55_FILES
            .iter()
            .zip(&fixed)
            .map(|((_, source, _, _, _), (_, bytes))| Ok(fs::read(root.join(source))? == *bytes))
            .collect::<io::Result<Vec<_>>>()?
            .into_iter()
            .all(|equal| equal))
}
#[derive(Debug)]
pub struct MergedStartingComparison {
    pub historical_exact: bool,
    pub paths_exact: bool,
    pub historical_runtime_conserved: bool,
    pub original_78_conserved: Option<bool>,
    pub stable_files_exact: bool,
    pub fixed_merged_additions_exact: bool,
    pub runtime_sources_exact: bool,
    pub ordinary_mapping_bytes_exact: Option<bool>,
    ordinary: bool,
}
impl MergedStartingComparison {
    pub fn accepts(&self, policy: Policy) -> bool {
        match policy {
            Policy::ExactHistorical => self.historical_exact,
            Policy::ReviewedStartingRuntimeWithMergedJson42686c55 => {
                self.ordinary
                    && self.original_78_conserved == Some(true)
                    && self.paths_exact
                    && self.fixed_merged_additions_exact
                    && self.runtime_sources_exact
            }
            Policy::ReviewedStartingRuntime | Policy::ReviewedAdjacentRuntime => false,
        }
    }
    pub fn accepts_model_runtime(&self, policy: Policy) -> bool {
        matches!(
            policy,
            Policy::ReviewedStartingRuntimeWithMergedJson42686c55
        ) && !self.ordinary
            && self.paths_exact
            && self.historical_runtime_conserved
            && self.fixed_merged_additions_exact
            && self.runtime_sources_exact
    }
}
fn compare_starting_merged_impl(
    historical: &[ManifestRow],
    actual: &[ManifestRow],
    reviewed: &ReviewedStartingWithMergedJson42686c55Snapshot,
    ordinary: bool,
) -> MergedStartingComparison {
    // Validate the ENTIRE unmodified actual manifest. No filtering/projecting
    // actuals to 78 paths or Runtime/** before path/order/duplicate admission.
    let historical_bound = historical == reviewed.historical;
    let mut expected_paths = reviewed
        .historical
        .iter()
        .map(|r| r.0.as_str())
        .collect::<Vec<_>>();
    expected_paths.extend(MERGED_JSON42686C55_FILES.iter().map(|r| r.0));
    expected_paths.sort_unstable();
    let paths_exact = historical_bound
        && actual.len() == 80
        && expected_paths.windows(2).all(|p| p[0] < p[1])
        && expected_paths
            .into_iter()
            .eq(actual.iter().map(|r| r.0.as_str()));
    let mutable = |path: &str| STARTING_RUNTIME_FILES.iter().any(|(p, _)| *p == path);
    let generated = |path: &str| {
        matches!(
            path,
            "Ferrule.Generated.csproj" | "GeneratedMapping.cs" | "GeneratedTargetBuilder.cs"
        )
    };
    let runtime_sources_exact = paths_exact
        && reviewed
            .starting
            .0
            .iter()
            .all(|(p, b)| actual.iter().any(|r| *r == row(p, b)));
    let fixed_merged_additions_exact = paths_exact
        && merged_json42686c55_rows()
            .iter()
            .all(|r| actual.contains(r));
    let stable_runtime = paths_exact
        && historical
            .iter()
            .all(|r| generated(&r.0) || mutable(&r.0) || actual.contains(r));
    let mapping_exact = paths_exact
        && historical
            .iter()
            .all(|r| !generated(&r.0) || actual.contains(r));
    let stable_files_exact = paths_exact
        && historical
            .iter()
            .all(|r| mutable(&r.0) || (!ordinary && generated(&r.0)) || actual.contains(r));
    let historical_runtime_conserved = stable_runtime && runtime_sources_exact;
    MergedStartingComparison {
        historical_exact: historical_bound && historical == actual,
        paths_exact,
        historical_runtime_conserved,
        original_78_conserved: ordinary.then_some(stable_files_exact && runtime_sources_exact),
        stable_files_exact,
        fixed_merged_additions_exact,
        runtime_sources_exact,
        ordinary_mapping_bytes_exact: ordinary.then_some(mapping_exact),
        ordinary,
    }
}
pub fn compare_starting_merged_json42686c55(
    historical: &[ManifestRow],
    actual: &[ManifestRow],
    reviewed: &ReviewedStartingWithMergedJson42686c55Snapshot,
) -> MergedStartingComparison {
    compare_starting_merged_impl(historical, actual, reviewed, true)
}
pub fn compare_starting_model_merged_json42686c55(
    historical: &[ManifestRow],
    actual: &[ManifestRow],
    reviewed: &ReviewedStartingWithMergedJson42686c55Snapshot,
) -> MergedStartingComparison {
    compare_starting_merged_impl(historical, actual, reviewed, false)
}

#[test]
fn starting_strict_and_merged42686c55_cannot_cross_select_or_borrow_adjacent_authority() {
    let historical = historical_m78_rows().unwrap();
    let sources: SourceSnapshot = STARTING_RUNTIME_FILES
        .iter()
        .map(|(p, _)| ((*p).into(), format!("literal reviewed {p}").into_bytes()))
        .collect();
    let accepted: Vec<_> = sources.iter().map(|(p, b)| row(p, b)).collect();
    let strict = bind_starting_snapshot(sources.clone(), &accepted).unwrap();
    let reviewed = bind_starting_with_merged_json42686c55(
        sources.clone(),
        &accepted,
        &merged_json42686c55_rows(),
        MERGED_JSON_N_COMMIT,
        MERGED_JSON_N_TREE,
    )
    .unwrap();
    let mut strict_actual = historical.clone();
    for replacement in &accepted {
        *strict_actual
            .iter_mut()
            .find(|r| r.0 == replacement.0)
            .unwrap() = replacement.clone();
    }
    assert!(
        compare_starting(&historical, &strict_actual, &strict)
            .accepts(Policy::ReviewedStartingRuntime)
    );
    let mut actual = strict_actual.clone();
    actual.extend(merged_json42686c55_rows());
    actual.sort_by(|a, b| a.0.cmp(&b.0));
    let composition = compare_starting_merged_json42686c55(&historical, &actual, &reviewed);
    assert!(composition.accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55));
    assert!(!composition.accepts(Policy::ExactHistorical));
    assert!(!composition.accepts(Policy::ReviewedStartingRuntime));
    assert!(!composition.accepts(Policy::ReviewedAdjacentRuntime));
    assert!(
        !compare_starting(&historical, &actual, &strict).accepts(Policy::ReviewedStartingRuntime)
    );
    assert!(
        !compare_starting(&actual, &actual, &strict).accepts(Policy::ReviewedStartingRuntime),
        "an 80-row baseline cannot widen strict M78"
    );
    assert!(
        !compare_starting(&historical, &strict_actual, &strict)
            .accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55)
    );
    assert!(
        !compare_starting_merged_json42686c55(&historical, &strict_actual, &reviewed)
            .accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55)
    );
    let mut adjacent_sources = sources.clone();
    adjacent_sources.extend(merged_json42686c55_fixed_bodies().unwrap());
    let adjacent = compare(&historical, &actual, &adjacent_sources);
    assert!(adjacent.accepts(Policy::ReviewedAdjacentRuntime));
    assert!(!adjacent.accepts(Policy::ReviewedStartingRuntime));
    assert!(!adjacent.accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55));
    let mut changed_adjacent_sources = adjacent_sources.clone();
    changed_adjacent_sources[3].1.push(b'!');
    let mut changed_addition = actual.clone();
    *changed_addition
        .iter_mut()
        .find(|r| r.0 == MERGED_JSON42686C55_FILES[0].0)
        .unwrap() = row(
        &changed_adjacent_sources[3].0,
        &changed_adjacent_sources[3].1,
    );
    assert!(
        compare(&historical, &changed_addition, &changed_adjacent_sources)
            .accepts(Policy::ReviewedAdjacentRuntime)
    );
    assert!(
        !compare_starting_merged_json42686c55(&historical, &changed_addition, &reviewed)
            .accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55),
        "an updated live Adjacent snapshot cannot make an immutable N addition mutable for Starting"
    );
    let mut model = actual.clone();
    *model
        .iter_mut()
        .find(|r| r.0 == "GeneratedMapping.cs")
        .unwrap() = row(
        "GeneratedMapping.cs",
        b"distinct literal model, not the historical ordinary mapping",
    );
    let runtime = compare_starting_model_merged_json42686c55(&historical, &model, &reviewed);
    assert!(runtime.accepts_model_runtime(Policy::ReviewedStartingRuntimeWithMergedJson42686c55));
    assert!(!runtime.accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55));
    assert!(
        !compare_starting_merged_json42686c55(&historical, &model, &reviewed)
            .accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55)
    );
    let mut wrong_model_path = actual.clone();
    wrong_model_path[0].0 = "Wrong.cs".into();
    wrong_model_path.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(
        !compare_starting_model_merged_json42686c55(&historical, &wrong_model_path, &reviewed)
            .accepts_model_runtime(Policy::ReviewedStartingRuntimeWithMergedJson42686c55)
    );
    let mut changed_history = historical.clone();
    changed_history[0].2 = "0".repeat(64);
    assert!(
        !compare_starting_merged_json42686c55(&changed_history, &actual, &reviewed)
            .accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55)
    );
    let mut invalids = Vec::new();
    for path in [
        "GeneratedMapping.cs",
        "Runtime/FerruleFailures.cs",
        MERGED_JSON42686C55_FILES[0].0,
        STARTING_RUNTIME_FILES[0].0,
    ] {
        let mut changed = actual.clone();
        *changed.iter_mut().find(|r| r.0 == path).unwrap() =
            row(path, b"wrong fixed/stable/reviewed bytes");
        invalids.push(changed);
    }
    let mut missing = actual.clone();
    missing.retain(|r| r.0 != MERGED_JSON42686C55_FILES[0].0);
    invalids.push(missing);
    let mut extra = actual.clone();
    extra.push(row("Runtime/Extra.cs", b"not approved"));
    extra.sort_by(|a, b| a.0.cmp(&b.0));
    invalids.push(extra);
    let mut duplicate = actual.clone();
    duplicate.insert(1, actual[1].clone());
    invalids.push(duplicate);
    let mut reordered = actual.clone();
    reordered.swap(0, 1);
    invalids.push(reordered);
    let mut wrong_path = actual.clone();
    wrong_path[0].0 = "Wrong.cs".into();
    wrong_path.sort_by(|a, b| a.0.cmp(&b.0));
    invalids.push(wrong_path);
    for invalid in invalids {
        assert!(
            !compare_starting_merged_json42686c55(&historical, &invalid, &reviewed)
                .accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55)
        );
        if invalid.iter().find(|r| r.0 == "GeneratedMapping.cs")
            == actual.iter().find(|r| r.0 == "GeneratedMapping.cs")
        {
            assert!(
                !compare_starting_model_merged_json42686c55(&historical, &invalid, &reviewed)
                    .accepts_model_runtime(Policy::ReviewedStartingRuntimeWithMergedJson42686c55)
            );
        }
    }
    for defect in [
        "wrong-path",
        "wrong-bytes",
        "missing",
        "extra",
        "duplicate",
        "reordered",
    ] {
        let mut rows = merged_json42686c55_rows();
        match defect {
            "wrong-path" => rows[0].0 = "Runtime/Json/Wrong.cs".into(),
            "wrong-bytes" => rows[0].2 = "0".repeat(64),
            "missing" => {
                rows.pop();
            }
            "extra" => rows.push(row("Runtime/Extra.cs", b"x")),
            "duplicate" => rows[1] = rows[0].clone(),
            _ => rows.swap(0, 1),
        }
        assert!(
            bind_starting_with_merged_json42686c55(
                sources.clone(),
                &accepted,
                &rows,
                MERGED_JSON_N_COMMIT,
                MERGED_JSON_N_TREE
            )
            .is_err()
        );
    }
    assert!(
        bind_starting_with_merged_json42686c55(
            sources,
            &accepted,
            &merged_json42686c55_rows(),
            HISTORICAL_M_COMMIT,
            HISTORICAL_M_TREE
        )
        .is_err()
    );
}

#[test]
fn starting_policy_requires_exact_reviewed_paths_bodies_and_stable_artifacts() {
    let sources: SourceSnapshot = STARTING_RUNTIME_FILES
        .iter()
        .map(|(path, _)| {
            (
                (*path).into(),
                format!("reviewed Starting {path}").into_bytes(),
            )
        })
        .collect();
    let accepted: Vec<_> = sources
        .iter()
        .map(|(path, bytes)| row(path, bytes))
        .collect();
    let reviewed = bind_starting_snapshot(sources.clone(), &accepted).unwrap();
    let historical = historical_m78_rows().unwrap();
    let mut actual = historical.clone();
    for replacement in &accepted {
        *actual.iter_mut().find(|r| r.0 == replacement.0).unwrap() = replacement.clone();
    }
    assert!(
        compare_starting(&historical, &actual, &reviewed).accepts(Policy::ReviewedStartingRuntime)
    );
    assert!(!compare_starting(&historical, &actual, &reviewed).accepts(Policy::ExactHistorical));
    let mut wrong = sources.clone();
    wrong[0].0 = "Runtime/Unknown.cs".into();
    let mut missing = sources.clone();
    missing.pop();
    let mut extra = sources.clone();
    extra.push(("Runtime/Unknown.cs".into(), vec![]));
    let mut duplicate = sources.clone();
    duplicate[1] = duplicate[0].clone();
    let mut unreviewed = sources.clone();
    unreviewed[0].1.push(b'!');
    for invalid in [wrong, missing, extra, duplicate, unreviewed] {
        assert!(bind_starting_snapshot(invalid, &accepted).is_err());
    }
    let mut missing_row = accepted.clone();
    missing_row.pop();
    let mut extra_row = accepted.clone();
    extra_row.push(row("Runtime/Unknown.cs", b"x"));
    let mut duplicate_row = accepted.clone();
    duplicate_row[1] = duplicate_row[0].clone();
    for invalid in [missing_row, extra_row, duplicate_row] {
        assert!(bind_starting_snapshot(sources.clone(), &invalid).is_err());
    }
    let mut stable_drift = actual.clone();
    *stable_drift
        .iter_mut()
        .find(|r| r.0 == "GeneratedMapping.cs")
        .unwrap() = row("GeneratedMapping.cs", b"drift");
    let mut stable_runtime_drift = actual.clone();
    *stable_runtime_drift
        .iter_mut()
        .find(|r| r.0 == "Runtime/FerruleFailures.cs")
        .unwrap() = row("Runtime/FerruleFailures.cs", b"runtime drift");
    let mut body_drift = actual.clone();
    *body_drift
        .iter_mut()
        .find(|r| r.0 == STARTING_RUNTIME_FILES[0].0)
        .unwrap() = row(STARTING_RUNTIME_FILES[0].0, b"unreviewed");
    let mut added = actual.clone();
    added.push(row("extra.cs", b"x"));
    let mut removed = actual.clone();
    removed.remove(1);
    let mut duplicate_artifact = actual.clone();
    duplicate_artifact[2] = duplicate_artifact[1].clone();
    for invalid in [
        stable_drift,
        stable_runtime_drift,
        body_drift,
        added,
        removed,
        duplicate_artifact,
    ] {
        assert!(
            !compare_starting(&historical, &invalid, &reviewed)
                .accepts(Policy::ReviewedStartingRuntime)
        );
    }
}
#[test]
fn starting_reviewed_source_postfence_refuses_later_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = super::TempDir::new("starting443-policy-postfence")?;
    for (_, source) in STARTING_RUNTIME_FILES {
        let path = temp.0.join(source);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, b"independently reviewed body")?;
    }
    let sources = starting_snapshot(&temp.0)?;
    let accepted: Vec<_> = sources
        .iter()
        .map(|(path, bytes)| row(path, bytes))
        .collect();
    let reviewed = bind_starting_snapshot(sources, &accepted)?;
    assert!(starting_snapshot_unchanged(&temp.0, &reviewed)?);
    fs::write(
        temp.0.join(STARTING_RUNTIME_FILES[0].1),
        b"later source drift",
    )?;
    assert!(!starting_snapshot_unchanged(&temp.0, &reviewed)?);
    Ok(())
}

// P is an implementation target, never the origin of the immutable N inputs.
pub const IMPLEMENTATION_P_COMMIT: &str = "c9f9e0834933885edfb5790d5cbc43358d331ca8";
pub const IMPLEMENTATION_P_TREE: &str = "45c78e234da346ed06bf45d9ee27d8370ecedf0f";
pub fn bind_starting_with_merged_json42686c55_for_p(
    sources: SourceSnapshot,
    accepted_three: &[ManifestRow],
    fixed_two: &[ManifestRow],
    fixed_input_origin_commit: &str,
    fixed_input_origin_tree: &str,
    implementation_commit: &str,
    implementation_tree: &str,
) -> io::Result<ReviewedStartingWithMergedJson42686c55Snapshot> {
    if fixed_input_origin_commit != MERGED_JSON_N_COMMIT
        || fixed_input_origin_tree != MERGED_JSON_N_TREE
        || implementation_commit != IMPLEMENTATION_P_COMMIT
        || implementation_tree != IMPLEMENTATION_P_TREE
        || fixed_two != merged_json42686c55_rows()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "P target and immutable N origin must both be independently bound",
        ));
    }
    // Same private M78/three-reviewed/fixed-N mechanics, without calling the
    // N-specific implementation binder with a misleading N-as-P argument.
    merged_json42686c55_fixed_bodies()?;
    Ok(ReviewedStartingWithMergedJson42686c55Snapshot {
        starting: bind_starting_snapshot(sources, accepted_three)?,
        historical: historical_m78_rows()?,
    })
}

#[test]
fn starting_p_target_binding_keeps_n_origin_and_all_existing_policy_refusals() {
    let historical = historical_m78_rows().unwrap();
    let sources: SourceSnapshot = STARTING_RUNTIME_FILES
        .iter()
        .map(|(path, _)| {
            (
                (*path).into(),
                format!("literal P candidate {path}").into_bytes(),
            )
        })
        .collect();
    let accepted: Vec<_> = sources
        .iter()
        .map(|(path, bytes)| row(path, bytes))
        .collect();
    let fixed = merged_json42686c55_rows();
    let p = bind_starting_with_merged_json42686c55_for_p(
        sources.clone(),
        &accepted,
        &fixed,
        MERGED_JSON_N_COMMIT,
        MERGED_JSON_N_TREE,
        IMPLEMENTATION_P_COMMIT,
        IMPLEMENTATION_P_TREE,
    )
    .unwrap();
    assert!(
        bind_starting_with_merged_json42686c55(
            sources.clone(),
            &accepted,
            &fixed,
            MERGED_JSON_N_COMMIT,
            MERGED_JSON_N_TREE
        )
        .is_ok()
    );
    assert!(
        bind_starting_with_merged_json42686c55(
            sources.clone(),
            &accepted,
            &fixed,
            IMPLEMENTATION_P_COMMIT,
            IMPLEMENTATION_P_TREE
        )
        .is_err(),
        "old N binder cannot become P authority"
    );
    for (origin, origin_tree, target, target_tree) in [
        (
            IMPLEMENTATION_P_COMMIT,
            IMPLEMENTATION_P_TREE,
            IMPLEMENTATION_P_COMMIT,
            IMPLEMENTATION_P_TREE,
        ),
        (
            MERGED_JSON_N_COMMIT,
            MERGED_JSON_N_TREE,
            MERGED_JSON_N_COMMIT,
            MERGED_JSON_N_TREE,
        ),
        (
            HISTORICAL_M_COMMIT,
            HISTORICAL_M_TREE,
            IMPLEMENTATION_P_COMMIT,
            IMPLEMENTATION_P_TREE,
        ),
        (
            MERGED_JSON_N_COMMIT,
            IMPLEMENTATION_P_TREE,
            IMPLEMENTATION_P_COMMIT,
            IMPLEMENTATION_P_TREE,
        ),
        (
            MERGED_JSON_N_COMMIT,
            MERGED_JSON_N_TREE,
            IMPLEMENTATION_P_COMMIT,
            MERGED_JSON_N_TREE,
        ),
    ] {
        assert!(
            bind_starting_with_merged_json42686c55_for_p(
                sources.clone(),
                &accepted,
                &fixed,
                origin,
                origin_tree,
                target,
                target_tree
            )
            .is_err()
        );
    }
    let mut wrong_fixed = fixed.clone();
    wrong_fixed[0].2 = "0".repeat(64);
    assert!(
        bind_starting_with_merged_json42686c55_for_p(
            sources.clone(),
            &accepted,
            &wrong_fixed,
            MERGED_JSON_N_COMMIT,
            MERGED_JSON_N_TREE,
            IMPLEMENTATION_P_COMMIT,
            IMPLEMENTATION_P_TREE
        )
        .is_err()
    );
    let mut actual = historical.clone();
    for replacement in &accepted {
        *actual
            .iter_mut()
            .find(|row| row.0 == replacement.0)
            .unwrap() = replacement.clone();
    }
    actual.extend(fixed);
    actual.sort_by(|a, b| a.0.cmp(&b.0));
    let result = compare_starting_merged_json42686c55(&historical, &actual, &p);
    assert!(result.accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55));
    for policy in [
        Policy::ExactHistorical,
        Policy::ReviewedAdjacentRuntime,
        Policy::ReviewedStartingRuntime,
    ] {
        assert!(!result.accepts(policy));
    }
    let model = compare_starting_model_merged_json42686c55(&historical, &actual, &p);
    assert!(model.accepts_model_runtime(Policy::ReviewedStartingRuntimeWithMergedJson42686c55));
    assert!(!model.accepts(Policy::ReviewedStartingRuntimeWithMergedJson42686c55));
    let strict = bind_starting_snapshot(sources, &accepted).unwrap();
    assert!(
        !compare_starting(&historical, &actual, &strict).accepts(Policy::ReviewedStartingRuntime)
    );
}

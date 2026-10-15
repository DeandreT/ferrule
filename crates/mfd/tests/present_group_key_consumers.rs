use std::path::Path;

#[path = "fixtures/present-string-group-keys/consumer_project.rs"]
mod oracle;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/present-string-group-keys")
        .join(name)
}

fn assert_complete(actual: &mapping::Project, expected: &mapping::Project) {
    assert_eq!(
        mapping::project_file::encode_pretty(actual).unwrap(),
        mapping::project_file::encode_pretty(expected).unwrap()
    );
}

#[test]
fn composed_control_keeps_complete_legacy_project_warning_and_executable_refusal() {
    let path = fixture("integer-distinct.mfd");
    let expected = oracle::composed_distinct();
    let warning = "sequence into `Group` applies group-by before distinct-values, which cannot be represented exactly; imported using ferrule's sequence order";
    let validation = vec![engine::ValidationIssue {
        location: "graph node 0".into(),
        message: "source field `Rows` matches no scalar".into(),
        owner: Some(engine::ValidationOwner::GraphNode {
            function: None,
            node: 0,
        }),
    }];
    let report = mfd::ImportReport {
        executable: false,
        issues: vec![
            mfd::ImportIssue {
                kind: mfd::ImportIssueKind::ImportWarning,
                message: warning.into(),
            },
            mfd::ImportIssue {
                kind: mfd::ImportIssueKind::Validation,
                message: "graph node 0: source field `Rows` matches no scalar".into(),
            },
        ],
    };
    let imported = mfd::import(&path).unwrap();
    assert_complete(&imported.project, &expected);
    assert_eq!(imported.warnings, vec![warning.to_string()]);
    assert_eq!(engine::validate(&expected), validation);
    assert_eq!(engine::validate(&imported.project), validation);
    assert_eq!(mfd::assess_import(&imported), report);
    let ordinary = mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::BestEffort,
    )
    .unwrap();
    assert_complete(&ordinary.imported.project, &expected);
    assert_eq!(ordinary.imported.warnings, vec![warning.to_string()]);
    assert_eq!(ordinary.report, report);
    match mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    ) {
        Err(mfd::MfdError::IncompatibleImport(actual)) => assert_eq!(*actual, report),
        _ => panic!("the existing exact sequence-order finding must refuse Executable import"),
    }
    let group = &imported.project.root.children[0];
    let exposed = group.bindings[0].node;
    assert_eq!(group.group_by, Some(exposed));
    assert!(
        matches!(imported.project.graph.nodes.get(&exposed), Some(mapping::Node::SourceField { path, frame: Some(frame) }) if path == &["Key"] && frame == &["Rows"])
    );
    assert!(
        !imported.project.graph.nodes.values().any(
            |node| matches!(node, mapping::Node::Call { function, .. } if function == "string")
        )
    );
}

#[test]
fn shadowed_parent_consumer_keeps_complete_legacy_project_and_no_new_diagnostic() {
    let path = fixture("integer-hidden-consumer.mfd");
    let expected = oracle::hidden_consumer();
    let imported = mfd::import(&path).unwrap();
    assert_complete(&imported.project, &expected);
    assert!(imported.warnings.is_empty());
    assert!(engine::validate(&expected).is_empty());
    assert_eq!(
        mfd::assess_import(&imported),
        mfd::ImportReport {
            executable: true,
            issues: vec![]
        }
    );
    let executable = mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert_complete(&executable.imported.project, &expected);
    assert!(executable.imported.warnings.is_empty());
    assert_eq!(
        executable.report,
        mfd::ImportReport {
            executable: true,
            issues: vec![]
        }
    );
    let group = &imported.project.root.children[0];
    assert_eq!(group.group_by, Some(group.bindings[0].node));
    assert!(
        !imported.project.graph.nodes.values().any(
            |node| matches!(node, mapping::Node::Call { function, .. } if function == "string")
        )
    );
}

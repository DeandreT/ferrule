//! #471 owned failure walks render only the selected endpoint cohort.
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn owned(owner: mapping::SourceOwner, parts: &[&str]) -> mapping::Node {
    mapping::Node::OwnedSourceField(mapping::OwnedSourceField {
        source: mapping::OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: None,
    })
}
fn named() -> mapping::SourceOwner {
    mapping::SourceOwner::Named { name: "Ref".into() }
}
/// Rule 1 is a legacy primary-row rule; rule 2 walks only the named endpoint.
fn owned_failure_program() -> codegen::Program {
    let source = ir::SchemaNode::group(
        "Input",
        vec![
            ir::SchemaNode::group(
                "Rows",
                vec![
                    ir::SchemaNode::scalar("Flag", ir::ScalarType::Bool),
                    ir::SchemaNode::scalar("Message", ir::ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let rule = |iteration, predicate, message| mapping::FailureRule {
        iteration,
        selection: mapping::FailureSelection::WhenTrue { predicate },
        message: Some(message),
    };
    let project = mapping::Project {
        source: source.clone(),
        target: ir::SchemaNode::group("Output", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        user_functions: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Ref".into(),
            path: "ref.json".into(),
            schema: source,
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        root: mapping::Scope::default(),
        graph: mapping::Graph {
            nodes: [
                (1, owned(mapping::SourceOwner::Primary, &["Rows", "Flag"])),
                (
                    2,
                    owned(mapping::SourceOwner::Primary, &["Rows", "Message"]),
                ),
                (3, owned(named(), &["Rows", "Flag"])),
                (4, owned(named(), &["Rows", "Message"])),
            ]
            .into_iter()
            .collect(),
        },
        failure_rules: vec![
            rule(
                mapping::FailureIteration::Source {
                    collection: path(&["Rows"]),
                },
                1,
                2,
            ),
            rule(
                mapping::FailureIteration::OwnedSource {
                    source: mapping::OwnedSourceRef {
                        owner: named(),
                        path: path(&["Rows"]),
                    },
                },
                3,
                4,
            ),
        ],
    };
    codegen::lower_with_basic_owned_failure_sources(&project).expect("owned failure walk lowers")
}
fn closed_walk() -> codegen::ProgramValidationError {
    codegen::ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::FailureIteration { index: 1 },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
                owner: codegen::OwnedSourceDiagnosticOwner::Named { name: "Ref".into() },
                path: path(&["Rows"]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::FailureRule,
            origin: codegen::OwnedSourceCapabilityOrigin::Failure { index: 1 },
        }),
    }
}

#[test]
fn owned_failure_walks_render_beside_primary_rows_and_older_emitters_refuse() {
    let program = owned_failure_program();
    let artifacts = crate::emit_with_basic_owned_failure_sources(&program)
        .expect("explicit owned failure walks emit");
    let source = artifacts
        .files()
        .iter()
        .filter(|file| file.path.as_str().ends_with("GeneratedMapping.cs"))
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .collect::<String>();
    assert_eq!(
        source
            .matches("context.WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"), ")
            .count(),
        1
    );
    assert_eq!(
        source.matches("context.VisitPrimaryFailureRows(").count(),
        1
    );
    assert!(source.contains("finally { global::System.GC.KeepAlive(cohort_failure_1); }"));
    // Earlier owned emitters admit rule 1 and still refuse the owned walk.
    for result in [
        crate::emit_with_basic_owned_row_failures(&program),
        crate::emit_with_basic_owned_group_starting_with(&program),
    ] {
        assert!(
            matches!(&result, Err(crate::EmitError::ProgramValidation(error)) if *error == closed_walk()),
            "{result:?}"
        );
    }
}

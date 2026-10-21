//! #449 owned join scopes render one owned join and member reads by identity.
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn rows(owner: mapping::SourceOwner) -> mapping::OwnedSourceRef {
    mapping::OwnedSourceRef {
        owner,
        path: path(&["Rows"]),
    }
}
fn named() -> mapping::SourceOwner {
    mapping::SourceOwner::Named { name: "Ref".into() }
}

/// Primary `Rows` joined to named `Rows` by id, filtered, sorted and windowed.
fn owned_join_project() -> mapping::Project {
    let source = ir::SchemaNode::group(
        "Input",
        vec![
            ir::SchemaNode::group(
                "Rows",
                vec![
                    ir::SchemaNode::scalar("id", ir::ScalarType::Int),
                    ir::SchemaNode::scalar("code", ir::ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let join = mapping::JoinId::new(4);
    let plan = mapping::OwnedJoinPlan::new(
        mapping::OwnedJoinSource::new(rows(mapping::SourceOwner::Primary)),
        mapping::OwnedJoinSource::new(rows(named())),
        mapping::OwnedJoinConditions::new(mapping::OwnedJoinKey::new(
            rows(mapping::SourceOwner::Primary),
            path(&["id"]),
            path(&["id"]),
        )),
    )
    .expect("authored owned plan");
    let bind = |target_field: &str, node| mapping::Binding {
        target_field: target_field.into(),
        node,
    };
    mapping::Project {
        source: source.clone(),
        target: ir::SchemaNode::group(
            "Output",
            vec![
                ir::SchemaNode::group(
                    "Row",
                    vec![
                        ir::SchemaNode::scalar("code", ir::ScalarType::String),
                        ir::SchemaNode::scalar("at", ir::ScalarType::Int),
                        ir::SchemaNode::scalar("tuple", ir::ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
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
        root: mapping::Scope {
            children: vec![mapping::Scope {
                target_field: "Row".into(),
                iteration: mapping::ScopeIteration::OwnedInnerJoin { id: join, plan },
                bindings: vec![bind("code", 1), bind("at", 2), bind("tuple", 3)],
                filter: Some(4),
                sort_by: Some(2),
                windows: vec![mapping::SequenceWindow::First { count: 5 }],
                ..Default::default()
            }],
            ..Default::default()
        },
        graph: mapping::Graph {
            nodes: [
                (
                    1,
                    mapping::Node::OwnedJoinField(mapping::OwnedJoinField {
                        join,
                        collection: rows(named()),
                        path: path(&["code"]),
                    }),
                ),
                (
                    2,
                    mapping::Node::OwnedJoinSourcePosition(mapping::OwnedJoinSourcePosition {
                        join,
                        collection: rows(named()),
                    }),
                ),
                (3, mapping::Node::JoinPosition { join }),
                (
                    4,
                    mapping::Node::Const {
                        value: ir::Value::Bool(true),
                    },
                ),
                (
                    5,
                    mapping::Node::Const {
                        value: ir::Value::Int(2),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        failure_rules: Vec::new(),
    }
}

fn options() -> crate::Options {
    crate::Options {
        package_name: "owned-joins".into(),
        runtime_dependency: crate::RuntimeDependency::Path("../codegen-runtime".into()),
    }
}

#[test]
fn owned_join_scopes_render_the_owned_pipeline_and_exact_member_reads() {
    let program = codegen::lower_with_basic_owned_joins(&owned_join_project()).unwrap();
    let artifacts = crate::emit_with_basic_owned_joins(&program, &options()).unwrap();
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .unwrap();
    for expected in [
        "context.owned_inner_join(4, codegen_runtime::OwnedJoinSource { owner: SourceOwnerRef::Primary, collection: &[\"Rows\"], singleton: false }, &[codegen_runtime::OwnedJoinStage { source: codegen_runtime::OwnedJoinSource { owner: SourceOwnerRef::Named(\"Ref\"), collection: &[\"Rows\"], singleton: false }, keys: &[codegen_runtime::OwnedJoinKey { left: 0, left_path: &[\"id\"], right_path: &[\"id\"] }] }])?",
        "context.owned_join_field(4, SourceOwnerRef::Named(\"Ref\"), &[\"Rows\"], &[\"code\"])",
        "context.owned_join_source_position(4, SourceOwnerRef::Named(\"Ref\"), &[\"Rows\"])",
        "Ok(Value::Int(context.owned_join_position(4)? as i64))",
        ".map(|(index, candidate)| candidate.with_owned_join_position(4, index + 1))",
        "candidate.with_owned_join_position(4, outputs.len() + 1)?",
        "codegen_runtime::apply_sequence_windows(candidates, &windows)",
    ] {
        assert!(source.contains(expected), "{expected}\n{source}");
    }
    assert_eq!(source.matches("context.owned_inner_join(").count(), 1);
}

#[test]
fn older_rust_emitters_refuse_owned_joins() {
    let program = codegen::lower_with_basic_owned_joins(&owned_join_project()).unwrap();
    for result in [
        crate::emit(&program, &options()),
        crate::emit_with_basic_owned_sources(&program, &options()),
        crate::emit_with_basic_owned_lookup_find(&program, &options()),
        crate::emit_with_basic_owned_recursive_filter(&program, &options()),
    ] {
        assert!(
            matches!(&result, Err(crate::EmitError::InvalidProgram(
                codegen::ProgramValidationError::OwnedSource { diagnostic }))
                if matches!(diagnostic.as_ref(), codegen::OwnedSourceDiagnostic::Capability {
                    reason: codegen::OwnedSourceCapabilityReason::Join, .. })),
            "{result:?}"
        );
    }
}

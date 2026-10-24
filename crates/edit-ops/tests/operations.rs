use edit_ops::*;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope};

fn fixture() -> Project {
    let mut graph = Graph::default();
    graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Int(9007199254740993),
        },
    );
    graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Float(-0.0),
        },
    );
    graph.nodes.insert(
        3,
        Node::Call {
            function: "concat".into(),
            args: vec![1, 2],
        },
    );
    graph.nodes.insert(
        4,
        Node::SourceField {
            path: vec!["input".into()],
            frame: None,
        },
    );
    graph.nodes.insert(
        5,
        Node::ValueMap {
            input: 1,
            input_type: None,
            table: vec![],
            default: None,
        },
    );
    graph.nodes.insert(6, Node::Unconnected);
    graph.nodes.insert(
        7,
        Node::Call {
            function: "upper".into(),
            args: vec![6],
        },
    );
    Project {
        source: SchemaNode::group(
            "source",
            vec![SchemaNode::scalar("input", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "target",
            vec![
                SchemaNode::scalar("a", ScalarType::String),
                SchemaNode::scalar("b", ScalarType::String),
                SchemaNode::group("child", vec![]),
                SchemaNode::group("other", vec![]),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "a".into(),
                node: 4,
            }],
            children: vec![Scope {
                target_field: "child".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}
fn root() -> ScopeRef {
    ScopeRef {
        target: Target::Primary {},
        children: vec![],
    }
}
fn field(name: &str) -> BindingRef {
    BindingRef {
        scope: root(),
        field: name.into(),
    }
}
fn parse(text: &str) -> EditOperation {
    serde_json::from_str(text).unwrap()
}
fn key(project: &Project) -> String {
    mapping::project_file::encode_pretty(project).unwrap()
}

fn round_trip(project: Project, operation: EditOperation) {
    let encoded = serde_json::to_string(&operation).unwrap();
    let decoded: EditOperation = serde_json::from_str(&encoded).unwrap();
    assert_eq!(operation, decoded);
    assert_closed_objects(&operation);
    let before = key(&project);
    let mut session = EditSession::new(project.clone());
    assert!(session.apply(operation.clone()).unwrap(), "{operation:?}");
    let after = key(session.project());
    assert_ne!(before, after, "{operation:?}");
    assert_eq!(
        after,
        key(EditSession::replay(project, &[operation])
            .unwrap()
            .project())
    );
    assert!(session.undo().unwrap());
    assert_eq!(before, key(session.project()));
    assert!(!session.revisions().is_dirty());
    assert!(session.redo().unwrap());
    assert_eq!(after, key(session.project()));
}

fn assert_closed_objects(operation: &EditOperation) {
    fn objects(value: &serde_json::Value, path: String, found: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                found.push(path.clone());
                for (key, value) in map {
                    objects(
                        value,
                        format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                        found,
                    );
                }
            }
            serde_json::Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    objects(value, format!("{path}/{index}"), found);
                }
            }
            _ => {}
        }
    }
    let original = serde_json::to_value(operation).unwrap();
    let mut paths = Vec::new();
    objects(&original, String::new(), &mut paths);
    for path in paths {
        let mut changed = original.clone();
        changed
            .pointer_mut(&path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown_member".into(), serde_json::Value::Bool(true));
        assert!(
            serde_json::from_value::<EditOperation>(changed).is_err(),
            "unknown member accepted at {path}: {operation:?}"
        );
    }
}

#[test]
fn clearing_controls_and_all_grouping_modes_restore_exact_state() {
    for grouping in [
        Grouping::AdjacentBy { node: NodeId(1) },
        Grouping::StartingWith { node: NodeId(1) },
        Grouping::EndingWith { node: NodeId(1) },
        Grouping::IntoBlocks { node: NodeId(1) },
    ] {
        round_trip(
            fixture(),
            EditOperation::SetGrouping {
                scope: root(),
                grouping,
            },
        );
    }
    let mut project = fixture();
    project.root.filter = Some(1);
    project.root.group_by = Some(1);
    project.root.post_group_filter = Some(2);
    project.root.sort_by = Some(1);
    project.root.sort_descending = true;
    project.root.iteration = mapping::ScopeIteration::Source(vec![]);
    round_trip(
        project.clone(),
        EditOperation::SetFilter {
            scope: root(),
            node: None,
        },
    );
    round_trip(
        project.clone(),
        EditOperation::SetGrouping {
            scope: root(),
            grouping: Grouping::None {},
        },
    );
    round_trip(
        project.clone(),
        EditOperation::SetSort {
            scope: root(),
            keys: vec![],
        },
    );
    round_trip(
        project,
        EditOperation::SetIteration {
            scope: root(),
            source: None,
        },
    );
    round_trip(
        fixture(),
        EditOperation::SetIteration {
            scope: root(),
            source: Some(SourceCollection::Primary { path: vec![] }),
        },
    );
}

#[test]
fn existing_unsupported_content_and_shared_references_are_preserved() {
    let mut project = fixture();
    project.graph.nodes.insert(
        80,
        Node::UserFunctionCall {
            function: mapping::FunctionId::new(u64::MAX),
            args: vec![4],
        },
    );
    project.extra_targets.push(mapping::NamedTarget {
        name: "other".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: project.root.clone(),
    });
    let before = key(&project);
    let mut session = EditSession::new(project);
    session
        .apply(EditOperation::RemoveBinding {
            binding: field("a"),
        })
        .unwrap();
    assert!(
        session.project().graph.nodes.contains_key(&4),
        "another target and unsupported call still own the source field"
    );
    assert!(
        session
            .apply(EditOperation::RemoveNodes {
                scope: root(),
                nodes: vec![NodeId(4)],
                replacements: vec![NodeId(81)]
            })
            .is_err(),
        "inactive target is a protected owner"
    );
    session.undo().unwrap();
    assert_eq!(before, key(session.project()));
}

#[test]
fn orphan_cleanup_retains_unrecognized_source_reads() {
    let mut project = fixture();
    project.graph.nodes.insert(
        4,
        Node::SourceField {
            path: vec!["unrecognized".into()],
            frame: None,
        },
    );
    let mut session = EditSession::new(project);
    session
        .apply(EditOperation::Bind {
            binding: field("a"),
            expression: Expression::Constant {
                id: NodeId(20),
                value: Scalar::Null {},
            },
        })
        .unwrap();
    assert!(session.project().graph.nodes.contains_key(&4));
}

#[test]
fn reserved_identity_reuse_and_redo_branch_are_checked() {
    let mut project = fixture();
    project.graph.nodes.insert(
        80,
        Node::Call {
            function: "upper".into(),
            args: vec![90],
        },
    );
    let op = EditOperation::Bind {
        binding: field("b"),
        expression: Expression::Constant {
            id: NodeId(90),
            value: Scalar::Null {},
        },
    };
    assert!(matches!(
        plan(&project, &op),
        Err(EditError::OccupiedNode(NodeId(90)))
    ));
    let mut session = EditSession::new(fixture());
    session
        .apply(EditOperation::EditConstant {
            node: NodeId(1),
            value: Scalar::Int { text: "2".into() },
        })
        .unwrap();
    session.undo().unwrap();
    session
        .apply(EditOperation::EditConstant {
            node: NodeId(1),
            value: Scalar::Int { text: "3".into() },
        })
        .unwrap();
    assert!(!session.can_redo());
    assert_eq!(session.operations().len(), 1);
}

#[test]
fn generated_failure_rules_have_explicit_private_ids_and_exact_history() {
    let constant = |id, text: &str| Expression::Constant {
        id: NodeId(id),
        value: Scalar::Int { text: text.into() },
    };
    let sequences = [
        GeneratedSequence::Generate {
            from: Some(constant(21, "1")),
            to: constant(22, "3"),
            item: NodeId(20),
        },
        GeneratedSequence::Tokenize {
            input: Expression::Existing { node: NodeId(1) },
            delimiter: constant(22, "3"),
            item: NodeId(20),
        },
        GeneratedSequence::TokenizeByLength {
            input: Expression::Existing { node: NodeId(1) },
            length: constant(22, "3"),
            item: NodeId(20),
        },
        GeneratedSequence::TokenizeRegex {
            input: Expression::Existing { node: NodeId(1) },
            pattern: Expression::Existing { node: NodeId(2) },
            flags: None,
            item: NodeId(20),
        },
    ];
    for sequence in sequences {
        round_trip(
            fixture(),
            EditOperation::AddFailureRule {
                rule: FailureRule {
                    iteration: FailureIteration::Generated { sequence },
                    selection: FailureSelection::WhenTrue {
                        expression: Expression::Call {
                            id: NodeId(23),
                            function: "exists".into(),
                            args: vec![NodeId(20)],
                        },
                    },
                    message: Some(Expression::Existing { node: NodeId(2) }),
                },
            },
        );
    }
}

#[test]
fn generated_failure_edits_and_retirement_preserve_ownership() {
    let mut project = fixture();
    project.graph.nodes.insert(
        20,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    project.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Sequence {
            sequence: mapping::SequenceExpr::Generate {
                from: Some(1),
                to: 2,
                item: 20,
            },
        },
        selection: mapping::FailureSelection::All,
        message: None,
    });
    round_trip(
        project.clone(),
        EditOperation::RemoveFailureRule { index: Index(0) },
    );
    round_trip(
        project.clone(),
        EditOperation::EditFailureRule {
            index: Index(0),
            rule: FailureRule {
                iteration: FailureIteration::Generated {
                    sequence: GeneratedSequence::Generate {
                        from: Some(Expression::Existing { node: NodeId(1) }),
                        to: Expression::Existing { node: NodeId(2) },
                        item: NodeId(20),
                    },
                },
                selection: FailureSelection::WhenFalse {
                    expression: Expression::Call {
                        id: NodeId(21),
                        function: "exists".into(),
                        args: vec![NodeId(20)],
                    },
                },
                message: None,
            },
        },
    );
    project.root.bindings.push(Binding {
        target_field: "b".into(),
        node: 20,
    });
    assert!(
        plan(
            &project,
            &EditOperation::RemoveFailureRule { index: Index(0) }
        )
        .is_err()
    );
}

#[test]
fn a_scalar_source_root_is_a_bindable_field() {
    let mut project = fixture();
    project.source = SchemaNode::scalar("value", ScalarType::String);
    round_trip(
        project,
        EditOperation::Bind {
            binding: field("b"),
            expression: Expression::Source {
                id: NodeId(20),
                path: vec![],
                frame: None,
            },
        },
    );
}

#[test]
fn optional_messages_disconnect_without_allocating_an_unused_identity() {
    let mut project = fixture();
    project
        .graph
        .nodes
        .insert(8, Node::Raise { message: Some(1) });
    round_trip(
        project.clone(),
        EditOperation::Disconnect {
            node: NodeId(8),
            input: Index(0),
            replacement: None,
        },
    );
    assert!(
        plan(
            &project,
            &EditOperation::Disconnect {
                node: NodeId(8),
                input: Index(0),
                replacement: Some(NodeId(20))
            }
        )
        .is_err()
    );
}

#[test]
fn explicit_source_owners_resolve_alias_collisions_without_rewriting_boundaries() {
    let mut project = fixture();
    project.extra_sources.push(mapping::NamedSource {
        name: "input".into(),
        path: "lookup.json".into(),
        schema: SchemaNode::group(
            "lookup",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        ),
        options: Default::default(),
        dynamic_path: None,
    });
    round_trip(
        project.clone(),
        EditOperation::Bind {
            binding: field("b"),
            expression: Expression::OwnedSource {
                id: NodeId(20),
                owner: SourceOwner::Named {
                    name: "input".into(),
                },
                path: vec!["value".into()],
                frame: None,
            },
        },
    );
    round_trip(
        project,
        EditOperation::Bind {
            binding: field("b"),
            expression: Expression::OwnedSource {
                id: NodeId(20),
                owner: SourceOwner::Primary {},
                path: vec!["input".into()],
                frame: None,
            },
        },
    );
}

#[test]
fn later_operations_can_refer_to_created_nodes_and_undo_the_whole_script() {
    let script = EditScript {
        format: Format::V1,
        operations: vec![
            EditOperation::Bind {
                binding: field("b"),
                expression: Expression::Constant {
                    id: NodeId(20),
                    value: Scalar::String {
                        text: "value".into(),
                    },
                },
            },
            EditOperation::Bind {
                binding: field("a"),
                expression: Expression::Call {
                    id: NodeId(21),
                    function: "upper".into(),
                    args: vec![NodeId(20)],
                },
            },
            EditOperation::Connect {
                node: NodeId(3),
                input: Index(0),
                expression: Expression::Existing { node: NodeId(21) },
            },
        ],
    };
    let encoded = serde_json::to_string(&script).unwrap();
    let script: EditScript = serde_json::from_str(&encoded).unwrap();
    let base = fixture();
    let original = key(&base);
    let mut first = EditSession::replay(base.clone(), &script.operations).unwrap();
    let second = EditSession::replay(base, &script.operations).unwrap();
    assert_eq!(key(first.project()), key(second.project()));
    for _ in &script.operations {
        assert!(first.undo().unwrap());
    }
    assert_eq!(key(first.project()), original);
}

#[test]
fn every_v1_operation_replays_and_undoes_exactly() {
    let operations = vec![
        EditOperation::Bind {
            binding: field("b"),
            expression: Expression::Source {
                id: NodeId(20),
                path: vec!["input".into()],
                frame: None,
            },
        },
        EditOperation::Bind {
            binding: field("a"),
            expression: Expression::Constant {
                id: NodeId(20),
                value: Scalar::Float {
                    text: "-0.0".into(),
                },
            },
        },
        EditOperation::Bind {
            binding: field("b"),
            expression: Expression::Call {
                id: NodeId(20),
                function: "upper".into(),
                args: vec![NodeId(1)],
            },
        },
        EditOperation::RetargetBinding {
            binding: field("a"),
            to: field("b"),
            node: NodeId(1),
        },
        EditOperation::RemoveBinding {
            binding: field("a"),
        },
        EditOperation::Connect {
            node: NodeId(7),
            input: Index(0),
            expression: Expression::Existing { node: NodeId(1) },
        },
        EditOperation::Disconnect {
            node: NodeId(3),
            input: Index(0),
            replacement: Some(NodeId(20)),
        },
        EditOperation::EditConstant {
            node: NodeId(2),
            value: Scalar::Float { text: "0.0".into() },
        },
        EditOperation::EditCall {
            node: NodeId(3),
            function: "upper".into(),
            args: vec![NodeId(1)],
        },
        EditOperation::EditValueMap {
            node: NodeId(5),
            value: ValueMap {
                input_type: Some(edit_ops::ScalarType::Int {}),
                table: vec![ValueMapEntry {
                    from: Scalar::Int {
                        text: "9007199254740993".into(),
                    },
                    to: Scalar::Float {
                        text: "-0.0".into(),
                    },
                }],
                default: Some(Scalar::Null {}),
            },
        },
        EditOperation::RemoveNodes {
            scope: root(),
            nodes: vec![NodeId(1)],
            replacements: vec![NodeId(20), NodeId(21)],
        },
        EditOperation::AddScope {
            parent: root(),
            field: "other".into(),
        },
        EditOperation::RemoveScope {
            scope: ScopeRef {
                target: Target::Primary {},
                children: vec![Index(0)],
            },
        },
        EditOperation::SetIteration {
            scope: root(),
            source: Some(SourceCollection::Legacy { path: vec![] }),
        },
        EditOperation::SetFilter {
            scope: root(),
            node: Some(NodeId(1)),
        },
        EditOperation::SetGrouping {
            scope: root(),
            grouping: Grouping::By { node: NodeId(1) },
        },
        EditOperation::SetSort {
            scope: root(),
            keys: vec![
                SortKey {
                    node: NodeId(1),
                    descending: true,
                },
                SortKey {
                    node: NodeId(2),
                    descending: false,
                },
            ],
        },
        EditOperation::AddFailureRule {
            rule: FailureRule {
                iteration: FailureIteration::Source {
                    collection: SourceCollection::Legacy { path: vec![] },
                },
                selection: FailureSelection::WhenTrue {
                    expression: Expression::Existing { node: NodeId(1) },
                },
                message: Some(Expression::Existing { node: NodeId(2) }),
            },
        },
    ];
    for operation in operations {
        round_trip(fixture(), operation);
    }
    let mut project = fixture();
    project.failure_rules = (0..3)
        .map(|_| mapping::FailureRule {
            iteration: mapping::FailureIteration::Source { collection: vec![] },
            selection: mapping::FailureSelection::All,
            message: None,
        })
        .collect();
    project.failure_rules[1].message = Some(1);
    round_trip(
        project.clone(),
        EditOperation::RemoveFailureRule { index: Index(1) },
    );
    round_trip(
        project.clone(),
        EditOperation::MoveFailureRule {
            from: Index(0),
            to: Index(2),
        },
    );
    round_trip(
        project,
        EditOperation::EditFailureRule {
            index: Index(1),
            rule: FailureRule {
                iteration: FailureIteration::Source {
                    collection: SourceCollection::Primary { path: vec![] },
                },
                selection: FailureSelection::All {},
                message: Some(Expression::Existing { node: NodeId(2) }),
            },
        },
    );
}

#[test]
fn wire_contract_refuses_unknown_fields_and_numeric_or_noncanonical_ids() {
    for text in [
        r#"{"format":{"ferrule.edit.v1":null},"operations":[]}"#,
        r#"{"format":"ferrule.edit.v2","operations":[]}"#,
        r#"{"format":"ferrule.edit.v1","operations":[],"extra":1}"#,
    ] {
        assert!(serde_json::from_str::<EditScript>(text).is_err());
    }
    for text in [
        r#"{"op":"edit_constant","node":1,"value":{"type":"null"}}"#,
        r#"{"op":"edit_constant","node":"01","value":{"type":"null"}}"#,
        r#"{"op":"edit_constant","node":"+1","value":{"type":"null"}}"#,
        r#"{"op":"edit_constant","node":"1","value":{"type":"null"},"extra":true}"#,
        r#"{"op":"edit_constant","node":"1","value":{"type":"null","text":""}}"#,
        r#"{"op":"remove_scope","scope":{"target":{"kind":"primary","extra":1},"children":[]}}"#,
    ] {
        assert!(
            serde_json::from_str::<EditOperation>(text).is_err(),
            "{text}"
        );
    }
    let operation = parse(
        r#"{"op":"edit_constant","node":"1","value":{"type":"int","text":"9223372036854775807"}}"#,
    );
    assert_eq!(serde_json::to_value(&operation).unwrap()["node"], "1");
    for text in ["0", "18446744073709551615"] {
        let id: FunctionId = serde_json::from_str(&format!("\"{text}\"")).unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{text}\""));
    }
}

#[test]
fn scalar_text_is_typed_and_lossless() {
    assert_eq!(
        Scalar::Int {
            text: "9007199254740993".into()
        }
        .value()
        .unwrap(),
        Value::Int(9007199254740993)
    );
    let Value::Float(value) = (Scalar::Float {
        text: "-0.0".into(),
    })
    .value()
    .unwrap() else {
        panic!()
    };
    assert_eq!(value.to_bits(), (-0.0f64).to_bits());
    for text in ["NaN", "inf", "1e999", " 1.0", "+1.0"] {
        assert!((Scalar::Float { text: text.into() }).value().is_err());
    }
    assert!((Scalar::Int { text: "01".into() }).value().is_err());
    assert!(
        (Scalar::Bool {
            text: "True".into()
        })
        .value()
        .is_err()
    );
}

#[test]
fn failed_operations_leave_project_and_history_unchanged() {
    let project = fixture();
    let before = key(&project);
    let mut session = EditSession::new(project);
    let operations = vec![
        EditOperation::Bind {
            binding: field("b"),
            expression: Expression::Constant {
                id: NodeId(1),
                value: Scalar::Null {},
            },
        },
        EditOperation::Connect {
            node: NodeId(3),
            input: Index(0),
            expression: Expression::Existing { node: NodeId(3) },
        },
        EditOperation::RemoveNodes {
            scope: root(),
            nodes: vec![NodeId(1)],
            replacements: vec![NodeId(20), NodeId(20)],
        },
        EditOperation::Bind {
            binding: field("missing"),
            expression: Expression::Constant {
                id: NodeId(20),
                value: Scalar::Null {},
            },
        },
    ];
    for operation in operations {
        assert!(session.apply(operation).is_err());
        assert_eq!(before, key(session.project()));
        assert!(session.operations().is_empty());
    }
    assert!(!session.undo().unwrap());
}

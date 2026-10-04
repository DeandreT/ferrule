use super::*;
use ir::{GroupAlternative, Instance, ScalarType};
use mapping::{FormatOptions, NamedTarget};

pub(crate) const BASE: &str = "{urn:writer-types}Base";
pub(crate) const DERIVED: &str = "{urn:writer-types}Derived";

pub(crate) fn typed_schema(name: &str) -> SchemaNode {
    let mut schema = SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::scalar("Extra", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: BASE.into(),
            members: vec!["Code".into()],
            required: vec![],
            constraints: vec![],
        },
        GroupAlternative {
            name: DERIVED.into(),
            members: vec!["Code".into(), "Extra".into()],
            required: vec![],
            constraints: vec![],
        },
    ])
    .unwrap();
    schema.xml_type_alternatives = true;
    schema.xml_default_type = Some(BASE.into());
    assert!(schema.metadata_is_valid());
    schema
}

pub(crate) fn project() -> Project {
    let mut project = Project {
        source: SchemaNode::group("Input", vec![]),
        target: SchemaNode::group("Output", vec![]),
        source_path: None,
        target_path: None,
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    };
    project.source = SchemaNode::group("Input", vec![]);
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Audit", ScalarType::String),
            typed_schema("Typed"),
        ],
    );
    project.source_options = FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    };
    project.target_options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    project.source_path = Some("input.json".into());
    project.target_path = Some("output.xml".into());
    project.graph.nodes.clear();
    project.graph.nodes.extend([
        (
            0,
            Node::Const {
                value: Value::String(BASE.into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("code".into()),
            },
        ),
    ]);
    let typed_scope = Scope {
        bindings: vec![
            Binding {
                target_field: "Code".into(),
                node: 1,
            },
            Binding {
                target_field: XML_TYPE_FIELD.into(),
                node: 0,
            },
        ],
        ..Scope::default()
    };
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Audit".into(),
            node: 0,
        }],
        children: vec![Scope {
            target_field: "Typed".into(),
            ..typed_scope.clone()
        }],
        ..Scope::default()
    };
    project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: Some("other.xml".into()),
        schema: typed_schema("Other"),
        options: project.target_options.clone(),
        root: typed_scope,
    });
    assert!(cli::validate(&project).is_empty());
    project
}

fn state(project: &Project) -> String {
    mapping::project_file::encode_pretty(project).unwrap()
}

pub(crate) fn marker(scope: &Scope) -> NodeId {
    scope
        .bindings
        .iter()
        .find(|binding| binding.target_field == XML_TYPE_FIELD)
        .unwrap()
        .node
}

pub(crate) fn assert_outputs(project: &Project, primary: &str, named: &str) {
    assert!(cli::validate(project).is_empty());
    let outputs = engine::run_outputs(project, &Instance::Group(vec![].into())).unwrap();
    assert_eq!(
        outputs
            .primary
            .field("Typed")
            .unwrap()
            .field(XML_TYPE_FIELD)
            .unwrap()
            .as_scalar(),
        Some(&Value::String(primary.into()))
    );
    assert_eq!(
        outputs.extras[0]
            .instance
            .field(XML_TYPE_FIELD)
            .unwrap()
            .as_scalar(),
        Some(&Value::String(named.into()))
    );
    assert_eq!(
        outputs.primary.field("Audit").unwrap().as_scalar(),
        Some(&Value::String(BASE.into()))
    );
    let project_path = std::env::temp_dir().join("ferrule-target-xml-type-payload/project.json");
    let payloads = cli::run_project_value_payloads(
        project,
        &project_path,
        &cli::PayloadRunOptions::new(
            cli::PayloadDocument::new(std::path::Path::new("input.json"), b"{}").unwrap(),
        ),
    )
    .unwrap();
    assert_eq!(payloads.artifacts.len(), 2);
    for (artifact, identity) in payloads.artifacts.iter().zip([primary, named]) {
        let xml = std::str::from_utf8(&artifact.bytes).unwrap();
        assert!(
            xml.contains("http://www.w3.org/2001/XMLSchema-instance"),
            "{xml}"
        );
        assert!(xml.contains("urn:writer-types"), "{xml}");
        let local = identity.rsplit('}').next().unwrap();
        assert!(
            xml.contains(&format!(":{local}\"")),
            "qualified selected type: {xml}"
        );
        assert!(xml.contains("<Code>code</Code>"), "{xml}");
    }
}

#[test]
fn named_and_nested_type_selection_is_exact_and_does_not_mutate_shared_expression() {
    for (target, path) in [(None, vec![0]), (Some(0), vec![])] {
        let mut project = project();
        let original_schema =
            serde_json::to_value((&project.target, &project.extra_targets[0].schema)).unwrap();
        assert!(
            apply(
                &mut project,
                target,
                &path,
                &Action::Declared(DERIVED.into())
            )
            .unwrap()
        );
        assert!(
            matches!(project.graph.nodes.get(&0), Some(Node::Const { value: Value::String(value) }) if value == BASE)
        );
        assert_eq!(project.root.bindings[0].node, 0);
        assert_eq!(
            serde_json::to_value((&project.target, &project.extra_targets[0].schema)).unwrap(),
            original_schema
        );
        if target.is_some() {
            assert_eq!(marker(&project.root.children[0]), 0);
            assert_outputs(&project, BASE, DERIVED);
        } else {
            assert_eq!(marker(&project.extra_targets[0].root), 0);
            assert_outputs(&project, DERIVED, BASE);
        }
        let selected = state(&project);
        assert!(
            !apply(
                &mut project,
                target,
                &path,
                &Action::Declared(DERIVED.into())
            )
            .unwrap()
        );
        assert_eq!(state(&project), selected, "same choice is not another edit");
    }
}

#[test]
fn explicit_inference_removes_only_marker_bindings_and_writer_uses_default() {
    let mut project = project();
    assert!(apply(&mut project, None, &[0], &Action::Declared(DERIVED.into())).unwrap());
    let graph = serde_json::to_value(&project.graph).unwrap();
    assert!(apply(&mut project, None, &[0], &Action::Infer).unwrap());
    assert_eq!(serde_json::to_value(&project.graph).unwrap(), graph);
    assert_eq!(project.root.children[0].bindings.len(), 1);
    assert_eq!(project.root.children[0].bindings[0].target_field, "Code");
    assert!(!apply(&mut project, None, &[0], &Action::Infer).unwrap());
    let output = engine::run_outputs(&project, &Instance::Group(vec![].into())).unwrap();
    assert!(
        output
            .primary
            .field("Typed")
            .unwrap()
            .field(XML_TYPE_FIELD)
            .is_none()
    );
    let payloads = cli::run_project_value_payloads(
        &project,
        &std::env::temp_dir().join("ferrule-target-xml-inference/project.json"),
        &cli::PayloadRunOptions::new(
            cli::PayloadDocument::new(std::path::Path::new("input.json"), b"{}").unwrap(),
        ),
    )
    .unwrap();
    let xml = std::str::from_utf8(&payloads.artifacts[0].bytes).unwrap();
    assert!(
        !xml.contains("xsi:type="),
        "matching schema default is implicit: {xml}"
    );
    assert!(xml.contains("<Code>code</Code>"));
}

#[test]
fn invalid_imported_metadata_and_values_remain_atomic_and_repairable() {
    for invalid in [
        Node::Const {
            value: Value::Int(7),
        },
        Node::Const {
            value: Value::String("{urn:wrong}Derived".into()),
        },
    ] {
        let mut project = project();
        project.graph.nodes.insert(0, invalid.clone());
        assert!(apply(&mut project, None, &[0], &Action::Declared(DERIVED.into())).unwrap());
        assert_eq!(
            serde_json::to_value(project.graph.nodes.get(&0)).unwrap(),
            serde_json::to_value(Some(&invalid)).unwrap(),
            "other graph owners preserved"
        );
        assert_eq!(marker(&project.extra_targets[0].root), 0);
    }
    let mut project = project();
    project.root.children[0].bindings[1].node = 41;
    if let SchemaKind::Group { children, .. } = &mut project.target.kind {
        children[1].xml_default_type = Some("unknown".into());
        assert!(choices(&children[1]).is_empty());
    }
    assert!(mapping::project_file::encode_pretty(&project).is_err());
    let original = serde_json::to_value(&project).unwrap();
    assert!(apply(&mut project, None, &[0], &Action::Declared(DERIVED.into())).is_err());
    assert_eq!(serde_json::to_value(&project).unwrap(), original);
    assert!(apply(&mut project, None, &[0], &Action::Infer).unwrap());
    assert_eq!(project.root.children[0].bindings.len(), 1);
    assert_eq!(project.root.children[0].bindings[0].target_field, "Code");
    assert_eq!(project.root.children[0].bindings[0].node, 1);
    assert_eq!(marker(&project.extra_targets[0].root), 0);
    assert!(!project.graph.nodes.contains_key(&41));
}

#[test]
fn allocation_does_not_fill_dangling_owners_and_exhaustion_is_atomic() {
    let mut project = project();
    project.root.bindings[0].node = 2;
    assert!(apply(&mut project, None, &[0], &Action::Declared(DERIVED.into())).unwrap());
    assert_eq!(marker(&project.root.children[0]), 3);
    assert_eq!(project.root.bindings[0].node, 2);
    assert!(!project.graph.nodes.contains_key(&2));
    let mut owned = self::project();
    owned.root.children[0].iteration =
        mapping::ScopeIteration::Sequence(mapping::SequenceExpr::Tokenize {
            input: 1,
            delimiter: 1,
            item: 2,
        });
    assert!(apply(&mut owned, Some(0), &[], &Action::Declared(DERIVED.into())).unwrap());
    assert_eq!(marker(&owned.extra_targets[0].root), 3);
    assert!(
        !owned.graph.nodes.contains_key(&2),
        "generated item ownership is preserved"
    );
    let mut near_limit = self::project();
    near_limit
        .graph
        .nodes
        .insert(NodeId::MAX - 1, Node::Const { value: Value::Null });
    near_limit.root.bindings[0].node = NodeId::MAX;
    let original = state(&near_limit);
    assert!(
        apply(
            &mut near_limit,
            Some(0),
            &[],
            &Action::Declared(DERIVED.into())
        )
        .is_err()
    );
    assert_eq!(
        state(&near_limit),
        original,
        "a dangling final ID cannot be filled"
    );
    project
        .graph
        .nodes
        .insert(NodeId::MAX, Node::Const { value: Value::Null });
    let original = state(&project);
    assert!(
        apply(
            &mut project,
            Some(0),
            &[],
            &Action::Declared(DERIVED.into())
        )
        .is_err()
    );
    assert_eq!(state(&project), original);
}

#[test]
fn contiguous_dangling_ids_across_project_owners_are_reserved_once_and_preserved() {
    use mapping::{
        AdjacencyTreePlan, DynamicBinding, DynamicChild, DynamicSourcePath, FailureIteration,
        FailureRule, FailureSelection, NamedSource, RecursiveFilterPlan, ScopeIteration,
        ScopeSequence, SequenceExpr, SequenceWindow, SortKey,
    };
    let mut project = project();
    // This imported project is intentionally invalid. Every ID from 2 to
    // 1059 has one distinct owner, so omitting any owner fills its missing
    // expression instead of allocating the expected fresh constant.
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![2],
        },
    );
    project.root.filter = Some(3);
    project.root.post_group_filter = Some(4);
    project.root.group_by = Some(5);
    project.root.group_adjacent_by = Some(6);
    project.root.group_starting_with = Some(7);
    project.root.group_ending_with = Some(8);
    project.root.group_into_blocks = Some(9);
    project.root.sort_by = Some(10);
    project.root.sort_then_by = vec![SortKey {
        node: 11,
        descending: true,
    }];
    project.root.windows = vec![
        SequenceWindow::SkipFirst { count: 12 },
        SequenceWindow::First { count: 13 },
        SequenceWindow::From { position: 14 },
        SequenceWindow::FromTo {
            first: 15,
            last: 16,
        },
        SequenceWindow::Last { count: 17 },
    ];
    project.root.children.push(Scope {
        target_field: "Sequence".into(),
        iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
            input: 18,
            delimiter: 19,
            item: 20,
        }),
        ..Scope::default()
    });
    project.root.bindings[0].node = 21;
    project.root.dynamic_bindings = vec![DynamicBinding { key: 22, value: 23 }];
    project.root.dynamic_children = vec![DynamicChild {
        key: 24,
        scope: Scope {
            construction: ScopeConstruction::Scalar { value: 25 },
            ..Scope::default()
        },
    }];
    project.root.children.push(Scope {
        target_field: "Recursive".into(),
        construction: ScopeConstruction::RecursiveFilter {
            plan: RecursiveFilterPlan::new("Children".into(), "Items".into(), 26).unwrap(),
        },
        ..Scope::default()
    });
    project.root.children.push(Scope {
        target_field: "Tree".into(),
        construction: ScopeConstruction::AdjacencyTree {
            plan: AdjacencyTreePlan::new(
                vec!["Rows".into()],
                vec!["Key".into()],
                vec!["Parent".into()],
                "Key".into(),
                "Children".into(),
                Some(27),
            )
            .unwrap(),
        },
        ..Scope::default()
    });
    project.root.children.push(Scope {
        target_field: "Segments".into(),
        iteration: ScopeIteration::Concatenate(ScopeSequence::new(
            Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 28,
                }],
                children: vec![Scope {
                    target_field: "Documents".into(),
                    iteration: ScopeIteration::DynamicDocuments {
                        source: vec!["Rows".into()],
                        output_path: 29,
                    },
                    ..Scope::default()
                }],
                ..Scope::default()
            },
            vec![],
        )),
        ..Scope::default()
    });
    project.failure_rules = vec![FailureRule {
        selection: FailureSelection::WhenTrue { predicate: 30 },
        message: Some(31),
        iteration: FailureIteration::Sequence {
            sequence: SequenceExpr::Tokenize {
                input: 32,
                delimiter: 33,
                item: 34,
            },
        },
    }];
    project.extra_sources = vec![NamedSource {
        name: "Secondary".into(),
        path: "secondary.json".into(),
        schema: SchemaNode::group("Secondary", vec![]),
        options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        dynamic_path: Some(DynamicSourcePath {
            node: 35,
            iteration: vec!["Rows".into()],
        }),
    }];
    project.extra_targets[0]
        .root
        .bindings
        .extend((36..=1059).map(|node| Binding {
            target_field: format!("Missing{node}"),
            node,
        }));
    let original = state(&project);
    assert!(
        apply(
            &mut project,
            Some(0),
            &[],
            &Action::Declared(DERIVED.into())
        )
        .unwrap()
    );
    let fresh = marker(&project.extra_targets[0].root);
    assert_eq!(fresh, 1060);
    assert_eq!(project.graph.nodes.len(), 3);
    assert!(
        matches!(project.graph.nodes.remove(&fresh), Some(Node::Const { value: Value::String(value) }) if value == DERIVED)
    );
    project.extra_targets[0].root.bindings[1].node = 0;
    assert_eq!(
        state(&project),
        original,
        "all imported owners retain their exact state"
    );
}

#[test]
fn isolated_function_graph_ids_do_not_reserve_project_expression_ids() {
    let mut project = project();
    let mut body = Graph::default();
    body.nodes.insert(
        2,
        Node::Const {
            value: Value::String("isolated".into()),
        },
    );
    project.user_functions.insert(
        mapping::FunctionId::new(1),
        mapping::UserFunction {
            library: "local".into(),
            name: "isolated".into(),
            description: None,
            parameters: vec![],
            output_name: "Value".into(),
            output_type: ScalarType::String,
            body,
            output: 2,
        },
    );
    let functions = serde_json::to_value(&project.user_functions).unwrap();
    assert!(
        apply(
            &mut project,
            Some(0),
            &[],
            &Action::Declared(DERIVED.into())
        )
        .unwrap()
    );
    assert_eq!(marker(&project.extra_targets[0].root), 2);
    assert_eq!(
        serde_json::to_value(&project.user_functions).unwrap(),
        functions
    );
}

#[test]
fn duplicate_and_copied_scope_bindings_can_be_removed_without_implicit_repair() {
    let mut project = project();
    project.root.children[0].bindings.push(Binding {
        target_field: XML_TYPE_FIELD.into(),
        node: 1,
    });
    let original = state(&project);
    assert!(apply(&mut project, None, &[0], &Action::Declared(DERIVED.into())).is_err());
    assert_eq!(state(&project), original);
    project.root.construction = ScopeConstruction::CopyCurrentSource;
    let original = state(&project);
    assert!(apply(&mut project, None, &[0], &Action::Declared(DERIVED.into())).is_err());
    assert_eq!(state(&project), original);
    assert!(apply(&mut project, None, &[0], &Action::Infer).unwrap());
    assert_eq!(project.root.children[0].bindings.len(), 1);
    assert_eq!(
        project.root.construction,
        ScopeConstruction::CopyCurrentSource
    );
    assert_eq!(project.graph.nodes.len(), 2);
}

#[test]
fn virtual_endpoints_preserve_physical_indices_and_do_not_auto_connect() {
    let schema = typed_schema("Root");
    let leaves = crate::canvas::target_leaves(&schema);
    assert_eq!(
        leaves
            .iter()
            .map(|leaf| leaf.field.as_str())
            .collect::<Vec<_>>(),
        ["Code", "Extra", XML_TYPE_FIELD]
    );
    assert!(leaves[2].chain.is_empty());
    let blocks = crate::canvas::target_blocks(&schema);
    assert_eq!(blocks[0].pin_labels, ["Code", "Extra", "XML type"]);
    let nested = project().target;
    let leaves = crate::canvas::target_leaves(&nested);
    assert_eq!(
        leaves
            .iter()
            .map(|leaf| leaf.label.as_str())
            .collect::<Vec<_>>(),
        ["Audit", "Typed/Code", "Typed/Extra", "Typed/XML type"]
    );
    assert_eq!(leaves[3].chain, ["Typed"]);
    assert_eq!(
        crate::scope_editor::binding_target_fields(&nested, &["Typed".into()]),
        ["Code", "Extra", XML_TYPE_FIELD]
    );
    let source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::scalar("Extra", ScalarType::String),
        ],
    );
    let plan =
        crate::auto_connect::plan_auto_connect(&source, &schema, &Scope::default(), &[], None);
    assert_eq!(
        plan.connections
            .iter()
            .map(|connection| connection.target_field.as_str())
            .collect::<Vec<_>>(),
        ["Code", "Extra"]
    );
    assert_eq!(plan.skipped_incompatible, 0);
    assert_eq!(plan.skipped_ambiguous, 0);
    let mut ordinary = schema.clone();
    ordinary.xml_type_alternatives = false;
    ordinary.xml_default_type = None;
    assert!(choices(&ordinary).is_empty());
    assert_eq!(crate::canvas::target_leaves(&ordinary).len(), 2);
    let mut malformed = schema;
    if let SchemaKind::Group { alternatives, .. } = &mut malformed.kind {
        alternatives[1].name = "prefix:Derived".into();
    }
    assert!(
        choices(&malformed).is_empty(),
        "undeclared lexical prefix cannot stand in for an expanded namespace"
    );
}

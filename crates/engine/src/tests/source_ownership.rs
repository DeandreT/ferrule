use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, FailureIteration, FailureRule, FailureSelection, FunctionId, Graph,
    NamedSource, NamedTarget, Node, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project,
    Scope, SourceOwner, UserFunction,
};

use crate::{
    DynamicSourceLoader, EngineError, ExecutionContext, TargetSelection, ValidationOwner,
    check_source_ownership_capability, required_sources_for_target, run, run_outputs,
    run_selected_target, run_with_context, validate,
};

fn scalar(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn owned(owner: SourceOwner, path: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path.iter().map(|part| (*part).into()).collect(),
        },
        frame: frame.map(|path| path.iter().map(|part| (*part).into()).collect()),
    })
}
fn scope(node: u32) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}
fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Primary",
            vec![
                scalar("Value"),
                SchemaNode::group("Rows", vec![scalar("Value")]).repeating(),
            ],
        ),
        target: SchemaNode::group("Output", vec![scalar("Value")]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: " Ref 雪 ".into(),
            path: "ref.json".into(),
            schema: SchemaNode::group("Other", vec![scalar("Value"), scalar("OnlyNamed")]),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::Const {
                        value: Value::String("legacy".into()),
                    },
                ),
                (2, owned(SourceOwner::Primary, &["Value"], None)),
            ]),
        },
        root: scope(1),
    }
}
fn refuse(
    project: &Project,
    selection: Option<TargetSelection<'_>>,
    node: u32,
    function: Option<FunctionId>,
    kind: &'static str,
) {
    assert_eq!(
        check_source_ownership_capability(project, selection),
        Err(EngineError::UnsupportedSourceOwnership {
            node,
            function,
            kind
        })
    );
}
fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}

#[test]
fn exact_owner_and_endpoint_local_absolute_path_validation() {
    let mut project = project();
    project
        .graph
        .nodes
        .insert(2, owned(named(" Ref 雪 "), &["Value"], None));
    assert!(validate(&project).is_empty());
    for name in ["Ref 雪", " ref 雪 ", "Unknown", ""] {
        project
            .graph
            .nodes
            .insert(2, owned(named(name), &["Value"], None));
        let issues = validate(&project);
        assert!(
            issues.iter().any(|issue| issue.owner
                == Some(ValidationOwner::GraphNode {
                    function: None,
                    node: 2
                })),
            "{name:?}: {issues:?}"
        );
    }
    project
        .graph
        .nodes
        .insert(2, owned(named(" Ref 雪 "), &[" Ref 雪 ", "Value"], None));
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.message.contains("matches no scalar in its owner"))
    );
    project.extra_sources.push(project.extra_sources[0].clone());
    project
        .graph
        .nodes
        .insert(2, owned(named(" Ref 雪 "), &["Value"], None));
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.message.contains("is ambiguous"))
    );
}

#[test]
fn no_other_owner_or_frame_prefix_rescues_an_invalid_scalar() {
    let mut project = project();
    for node in [
        owned(SourceOwner::Primary, &["OnlyNamed"], None),
        owned(SourceOwner::Primary, &["Missing"], Some(&["Rows"])),
    ] {
        project.graph.nodes.insert(2, node);
        assert!(
            validate(&project)
                .iter()
                .any(|issue| issue.message.contains("matches no scalar in its owner"))
        );
    }
    project.source = SchemaNode::group(
        "Primary",
        vec![SchemaNode::group("Rows", vec![scalar("Value")]).repeating()],
    );
    project
        .graph
        .nodes
        .insert(2, owned(SourceOwner::Primary, &["Value"], Some(&["Rows"])));
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.message.contains("matches no scalar in its owner"))
    );
    project.graph.nodes.insert(
        2,
        owned(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
    );
    assert!(validate(&project).is_empty());
    // Intrinsic validation does not decide whether a frame is active at execution.
    project.source = SchemaNode::group(
        "Primary",
        vec![
            scalar("Value"),
            SchemaNode::group("Rows", vec![scalar("Other")]).repeating(),
        ],
    );
    project
        .graph
        .nodes
        .insert(2, owned(SourceOwner::Primary, &["Value"], Some(&["Rows"])));
    assert!(validate(&project).is_empty());
    project.root = scope(2);
    refuse(&project, None, 2, None, "owned_source_field");
}

#[test]
fn scalar_root_and_flat_group_root_collection_are_present_and_valid() {
    let mut project = project();
    project.source = scalar("ScalarRoot");
    project
        .graph
        .nodes
        .insert(2, owned(SourceOwner::Primary, &[], None));
    assert!(validate(&project).is_empty());
    project.source = SchemaNode::group("FlatRows", vec![scalar("Value")]);
    for collection in [None, Some(vec![])] {
        project.graph.nodes.insert(
            2,
            Node::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection,
            }),
        );
        assert!(validate(&project).is_empty());
        project.root = scope(2);
        refuse(&project, None, 2, None, "owned_position");
    }
}

#[test]
fn disconnected_and_unselected_nodes_do_not_close_legacy_execution() {
    let mut project = project();
    assert!(validate(&project).is_empty());
    let input = Instance::Group(Default::default());
    assert!(run(&project, &input).is_ok());
    assert!(required_sources_for_target(&project, TargetSelection::Primary).is_ok());
    project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: scope(2),
    });
    assert!(run_selected_target(&project, &input, TargetSelection::Primary).is_ok());
    refuse(
        &project,
        Some(TargetSelection::Named("Other")),
        2,
        None,
        "owned_source_field",
    );
    assert!(matches!(
        run_outputs(&project, &input),
        Err(EngineError::UnsupportedSourceOwnership { node: 2, .. })
    ));
    assert!(matches!(
        required_sources_for_target(&project, TargetSelection::Named("Missing")),
        Err(EngineError::UnknownTarget { .. })
    ));
}

#[test]
fn failure_rules_and_demanded_dynamic_paths_are_admitted_before_loading() {
    let mut project = project();
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source { collection: vec![] },
        selection: FailureSelection::WhenTrue { predicate: 2 },
        message: None,
    });
    refuse(
        &project,
        Some(TargetSelection::Primary),
        2,
        None,
        "owned_source_field",
    );
    project.failure_rules[0].selection = FailureSelection::All;
    project.failure_rules[0].message = Some(2);
    refuse(
        &project,
        Some(TargetSelection::Primary),
        2,
        None,
        "owned_source_field",
    );
    project.failure_rules.clear();
    project.extra_sources[0].dynamic_path = Some(DynamicSourcePath {
        node: 2,
        iteration: vec![],
    });
    assert!(check_source_ownership_capability(&project, None).is_ok());
    project.graph.nodes.insert(
        3,
        Node::SourceField {
            path: vec![" Ref 雪 ".into(), "Value".into()],
            frame: None,
        },
    );
    project.root = scope(3);
    refuse(&project, None, 2, None, "owned_source_field");
    struct Loader(Cell<usize>);
    impl DynamicSourceLoader for Loader {
        fn load(&self, _: &str, _: &str) -> Result<Arc<Instance>, String> {
            self.0.set(self.0.get() + 1);
            Err("unexpected acquisition".into())
        }
    }
    let loader = Loader(Cell::new(0));
    let execution =
        ExecutionContext::new(Path::new("project.json")).with_dynamic_source_loader(&loader);
    assert!(matches!(
        run_with_context(&project, &Instance::Group(Default::default()), &execution),
        Err(EngineError::UnsupportedSourceOwnership { .. })
    ));
    assert_eq!(loader.0.get(), 0);
}

fn function(output: u32) -> UserFunction {
    UserFunction {
        library: "tests".into(),
        name: format!("function_{output}"),
        description: None,
        parameters: vec![],
        output_name: "value".into(),
        output_type: ScalarType::String,
        body: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::Const {
                        value: Value::String("function".into()),
                    },
                ),
                (2, owned(SourceOwner::Primary, &["Value"], None)),
            ]),
        },
        output,
    }
}

#[test]
fn function_and_main_node_ids_remain_separate_and_dead_nodes_stay_dead() {
    let mut project = project();
    let id = FunctionId::new(7);
    project.user_functions.insert(id, function(1));
    project.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: id,
            args: vec![],
        },
    );
    project.root = scope(3);
    assert!(validate(&project).is_empty());
    assert!(check_source_ownership_capability(&project, None).is_ok());
    project.user_functions.get_mut(&id).unwrap().output = 2;
    refuse(&project, None, 2, Some(id), "owned_source_field");
    // The main graph's 2 must not suppress function 2 through an ID-only visit set.
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("main".into()),
        },
    );
    project.graph.nodes.insert(
        4,
        Node::Call {
            function: "concat".into(),
            args: vec![3, 2],
        },
    );
    project.root = scope(4);
    refuse(&project, None, 2, Some(id), "owned_source_field");
    project.root = scope(1);
    assert!(check_source_ownership_capability(&project, None).is_ok());
}

#[test]
fn potential_if_admission_does_not_change_legacy_lazy_evaluation() {
    let mut project = project();
    project.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    project.graph.nodes.insert(
        4,
        Node::If {
            condition: 3,
            then: 1,
            else_: 2,
        },
    );
    project.root = scope(4);
    refuse(&project, None, 2, None, "owned_source_field");
    project.graph.nodes.insert(2, Node::Raise { message: None });
    assert_eq!(
        run(&project, &Instance::Group(Default::default()))
            .unwrap()
            .field("Value")
            .unwrap()
            .as_scalar(),
        Some(&Value::String("legacy".into()))
    );
}

#[test]
fn selected_scope_children_dynamic_children_and_concat_segments_are_roots() {
    let mut project = project();
    project.root.children.push(scope(2));
    refuse(
        &project,
        Some(TargetSelection::Primary),
        2,
        None,
        "owned_source_field",
    );
    project.root.children.clear();
    project.root.dynamic_children.push(mapping::DynamicChild {
        key: 1,
        scope: scope(2),
    });
    refuse(
        &project,
        Some(TargetSelection::Primary),
        2,
        None,
        "owned_source_field",
    );
    project.root.dynamic_children.clear();
    project.root.iteration =
        mapping::ScopeIteration::Concatenate(mapping::ScopeSequence::new(scope(1), vec![scope(2)]));
    refuse(
        &project,
        Some(TargetSelection::Primary),
        2,
        None,
        "owned_source_field",
    );
}

#[test]
fn filter_map_stage_functions_are_roots_with_distinct_body_namespaces() {
    let mut project = project();
    let predicate = FunctionId::new(7);
    let mapper = FunctionId::new(8);
    project.user_functions.insert(predicate, function(1));
    project.user_functions.insert(mapper, function(2));
    project.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::Int(1),
        },
    );
    project.graph.nodes.insert(
        9,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    project.graph.nodes.insert(
        10,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    let composition = mapping::FilterMapV1 {
        source: Box::new(mapping::SequenceExpr::Generate {
            from: None,
            to: 8,
            item: 9,
        }),
        item: 10,
        predicate,
        mapper,
        output_type: ScalarType::String,
        captures: vec![],
    };
    project.root.iteration =
        mapping::ScopeIteration::Sequence(mapping::SequenceExpr::FilterMapV1(composition));
    refuse(&project, None, 2, Some(mapper), "owned_source_field");
    project.user_functions.get_mut(&mapper).unwrap().output = 1;
    assert!(check_source_ownership_capability(&project, None).is_ok());
    project.user_functions.get_mut(&predicate).unwrap().output = 2;
    refuse(&project, None, 2, Some(predicate), "owned_source_field");
}

#[test]
fn chained_function_outputs_and_demands_follow_only_called_bodies() {
    let mut project = project();
    let first = FunctionId::new(7);
    let second = FunctionId::new(8);
    project.user_functions.insert(first, function(1));
    project.user_functions.insert(second, function(2));
    project
        .user_functions
        .get_mut(&first)
        .unwrap()
        .body
        .nodes
        .insert(
            1,
            Node::UserFunctionCall {
                function: second,
                args: vec![],
            },
        );
    project.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: first,
            args: vec![],
        },
    );
    project.root = scope(3);
    refuse(&project, None, 2, Some(second), "owned_source_field");
    project.root = scope(1);
    assert!(check_source_ownership_capability(&project, None).is_ok());
}

#[test]
fn explicit_nonempty_selectors_require_repetition_but_not_group_shape() {
    let mut project = project();
    let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
        unreachable!();
    };
    children.push(SchemaNode::scalar("RepeatedScalar", ScalarType::String).repeating());
    children.push(SchemaNode::group("Singleton", vec![scalar("Value")]));
    for node in [
        owned(
            SourceOwner::Primary,
            &["RepeatedScalar"],
            Some(&["RepeatedScalar"]),
        ),
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(vec!["RepeatedScalar".into()]),
        }),
    ] {
        project.graph.nodes.insert(2, node);
        assert!(validate(&project).is_empty());
    }
    for path in ["Value", "Singleton"] {
        for node in [
            owned(SourceOwner::Primary, &["Value"], Some(&[path])),
            Node::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection: Some(vec![path.into()]),
            }),
        ] {
            project.graph.nodes.insert(2, node);
            let issues = validate(&project);
            assert!(
                issues.iter().any(|issue| issue
                    .message
                    .contains("is not a repeating path in its owner")
                    && issue.owner
                        == Some(ValidationOwner::GraphNode {
                            function: None,
                            node: 2
                        })),
                "{issues:?}"
            );
        }
    }

    // Repetition belongs to the selected occurrence, not its recursive anchor.
    let mut children = SchemaNode::group("Children", vec![]).repeating();
    children.recursive_ref = Some("Tree".into());
    project.source = SchemaNode::group(
        "Primary",
        vec![SchemaNode::group("Tree", vec![scalar("Value"), children])],
    );
    for node in [
        owned(
            SourceOwner::Primary,
            &["Tree", "Children", "Value"],
            Some(&["Tree", "Children"]),
        ),
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(vec!["Tree".into(), "Children".into()]),
        }),
    ] {
        project.graph.nodes.insert(2, node);
        assert!(validate(&project).is_empty());
    }
}

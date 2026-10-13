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
fn refuse_context(
    project: &Project,
    selection: Option<TargetSelection<'_>>,
    node: u32,
    context: crate::OwnedCapabilityContext,
) {
    assert!(
        matches!(check_source_ownership_capability(project, selection),
        Err(EngineError::UnsupportedOwnedContext { node: Some(actual), context: actual_context, .. }) if actual == node && actual_context == context)
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
    assert!(matches!(
        check_source_ownership_capability(&project, None),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            source: crate::OwnedSourceError::UnsupportedFramePathCombination { .. },
            ..
        })
    ));
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
        assert!(check_source_ownership_capability(&project, None).is_ok());
        assert!(matches!(
            run(&project, &Instance::Group(Default::default())),
            Err(EngineError::OwnedSourceAccess {
                source: crate::OwnedSourceError::InactiveSourceFrame { .. },
                ..
            })
        ));
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
    assert!(
        check_source_ownership_capability(&project, Some(TargetSelection::Named("Other"))).is_ok()
    );
    assert!(run_outputs(&project, &input).is_ok());
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
    refuse_context(
        &project,
        Some(TargetSelection::Primary),
        2,
        crate::OwnedCapabilityContext::FailureRule,
    );
    project.failure_rules[0].selection = FailureSelection::All;
    project.failure_rules[0].message = Some(2);
    refuse_context(
        &project,
        Some(TargetSelection::Primary),
        2,
        crate::OwnedCapabilityContext::FailureRule,
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
    refuse_context(
        &project,
        None,
        2,
        crate::OwnedCapabilityContext::DynamicLoader,
    );
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
        Err(EngineError::UnsupportedOwnedContext {
            context: crate::OwnedCapabilityContext::DynamicLoader,
            ..
        })
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
    assert!(check_source_ownership_capability(&project, None).is_ok());
    assert_eq!(
        run(&project, &Instance::Group(Default::default()))
            .unwrap()
            .field("Value")
            .unwrap()
            .as_scalar(),
        Some(&Value::String("legacy".into()))
    );
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
    assert!(check_source_ownership_capability(&project, Some(TargetSelection::Primary)).is_ok());
    project.root.children.clear();
    project.root.dynamic_children.push(mapping::DynamicChild {
        key: 1,
        scope: scope(2),
    });
    refuse_context(
        &project,
        Some(TargetSelection::Primary),
        2,
        crate::OwnedCapabilityContext::DynamicTarget,
    );
    project.root.dynamic_children.clear();
    project.root.iteration =
        mapping::ScopeIteration::Concatenate(mapping::ScopeSequence::new(scope(1), vec![scope(2)]));
    refuse_context(
        &project,
        Some(TargetSelection::Primary),
        2,
        crate::OwnedCapabilityContext::Concatenate,
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

fn udf_identity(name: &str, ty: ScalarType) -> UserFunction {
    let parameter = mapping::FunctionParameterId::new(1);
    UserFunction {
        library: "tests".into(),
        name: name.into(),
        description: None,
        parameters: vec![mapping::FunctionParameter {
            id: parameter,
            name: "argument".into(),
            ty,
        }],
        output_name: "value".into(),
        output_type: ty,
        body: Graph {
            nodes: BTreeMap::from([(2, Node::FunctionParameter { parameter })]),
        },
        output: 2,
    }
}
fn udf_unused_argument(ty: ScalarType) -> UserFunction {
    let mut body = function(1);
    body.parameters.push(mapping::FunctionParameter {
        id: mapping::FunctionParameterId::new(1),
        name: "unused".into(),
        ty,
    });
    body
}
fn udf_text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn udf_group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

#[test]
fn public_udf_scalar_arguments_keep_exact_root_owners_and_source_discovery() {
    let mut p = project();
    let id = FunctionId::new(7);
    p.user_functions
        .insert(id, udf_identity("root_identity", ScalarType::String));
    let ir::SchemaKind::Group { children, .. } = &mut p.source.kind else {
        unreachable!();
    };
    children.push(SchemaNode::group(" Ref 雪 ", vec![scalar("Value")]));
    p.graph
        .nodes
        .insert(4, owned(named(" Ref 雪 "), &["Value"], None));
    for (node, argument) in [(3, 2), (5, 4)] {
        p.graph.nodes.insert(
            node,
            Node::UserFunctionCall {
                function: id,
                args: vec![argument],
            },
        );
    }
    p.graph.nodes.insert(
        6,
        Node::Call {
            function: "concat".into(),
            args: vec![3, 5],
        },
    );
    p.root = scope(6);
    assert!(validate(&p).is_empty());
    assert!(check_source_ownership_capability(&p, None).is_ok());
    let required = required_sources_for_target(&p, TargetSelection::Primary).unwrap();
    assert_eq!(required.static_sources.len(), 1);
    assert!(std::ptr::eq(
        required.static_sources[0],
        &p.extra_sources[0]
    ));
    assert!(required.dynamic_sources.is_empty());
    let primary = udf_group(vec![
        ("Value", udf_text("P.root")),
        (" Ref 雪 ", udf_group(vec![("Value", udf_text("shadow"))])),
    ]);
    let output = crate::run_with_sources(
        &p,
        &primary,
        vec![(
            " Ref 雪 ".into(),
            udf_group(vec![("Value", udf_text("N.root"))]),
        )],
    )
    .unwrap();
    assert_eq!(output, udf_group(vec![("Value", udf_text("P.rootN.root"))]));
}

#[test]
fn public_udf_row_arguments_keep_owned_ancestry_frames_and_positions() {
    let mut p = project();
    let identity = FunctionId::new(7);
    let position = FunctionId::new(8);
    p.user_functions
        .insert(identity, udf_identity("row_identity", ScalarType::String));
    p.user_functions
        .insert(position, udf_identity("position_identity", ScalarType::Int));
    let ir::SchemaKind::Group { children, .. } = &mut p.source.kind else {
        unreachable!();
    };
    children.push(SchemaNode::group(
        " Ref 雪 ",
        vec![SchemaNode::group("Rows", vec![scalar("Value")]).repeating()],
    ));
    let ir::SchemaKind::Group { children, .. } = &mut p.extra_sources[0].schema.kind else {
        unreachable!();
    };
    children.push(SchemaNode::group("Rows", vec![scalar("Value")]).repeating());
    let ir::SchemaKind::Group { children, .. } = &mut p.target.kind else {
        unreachable!();
    };
    children.push(SchemaNode::scalar("Position", ScalarType::Int));
    p.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: identity,
            args: vec![2],
        },
    );
    p.graph.nodes.insert(
        7,
        Node::UserFunctionCall {
            function: position,
            args: vec![6],
        },
    );
    let primary = udf_group(vec![
        ("Value", udf_text("P.root")),
        (
            "Rows",
            Instance::Repeated(vec![
                udf_group(vec![("Value", udf_text("P1"))]),
                udf_group(vec![("Value", udf_text("P2"))]),
            ]),
        ),
        (
            " Ref 雪 ",
            udf_group(vec![(
                "Rows",
                Instance::Repeated(vec![udf_group(vec![("Value", udf_text("shadow"))])]),
            )]),
        ),
    ]);
    let supplied = udf_group(vec![(
        "Rows",
        Instance::Repeated(vec![
            udf_group(vec![("Value", udf_text("N1"))]),
            udf_group(vec![("Value", udf_text("N2"))]),
            udf_group(vec![("Value", udf_text("N3"))]),
        ]),
    )]);
    for (owner, expected) in [
        (SourceOwner::Primary, vec!["P1", "P2"]),
        (named(" Ref 雪 "), vec!["N1", "N2", "N3"]),
    ] {
        p.graph
            .nodes
            .insert(2, owned(owner.clone(), &["Rows", "Value"], Some(&["Rows"])));
        p.graph.nodes.insert(
            6,
            Node::OwnedPosition(OwnedPosition {
                owner: owner.clone(),
                collection: Some(vec!["Rows".into()]),
            }),
        );
        p.root = scope(3);
        p.root.bindings.push(Binding {
            target_field: "Position".into(),
            node: 7,
        });
        p.root.iteration = mapping::ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner,
                path: vec!["Rows".into()],
            },
            output_path: None,
        };
        assert!(validate(&p).is_empty());
        assert!(check_source_ownership_capability(&p, None).is_ok());
        let output =
            crate::run_with_sources(&p, &primary, vec![(" Ref 雪 ".into(), supplied.clone())])
                .unwrap();
        let expected = Instance::Repeated(
            expected
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    udf_group(vec![
                        ("Value", udf_text(value)),
                        (
                            "Position",
                            Instance::Scalar(Value::Int(i64::try_from(index + 1).unwrap())),
                        ),
                    ])
                })
                .collect(),
        );
        assert_eq!(output, expected);
    }
}

#[test]
fn public_udf_unused_scalar_arguments_are_eager_but_if_branches_stay_lazy() {
    let input = udf_group(vec![
        ("Value", udf_text("root")),
        (
            "Rows",
            Instance::Repeated(vec![udf_group(vec![("Value", udf_text("row"))])]),
        ),
    ]);
    for (argument, ty, safe) in [
        (
            owned(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ScalarType::String,
            Value::String("safe".into()),
        ),
        (
            Node::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection: Some(vec!["Rows".into()]),
            }),
            ScalarType::Int,
            Value::Int(7),
        ),
    ] {
        let mut p = project();
        let id = FunctionId::new(7);
        p.user_functions.insert(id, udf_unused_argument(ty));
        p.graph.nodes.insert(2, argument);
        p.graph.nodes.insert(
            3,
            Node::UserFunctionCall {
                function: id,
                args: vec![2],
            },
        );
        p.root = scope(3);
        let inactive = || EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: crate::OwnedSourceError::InactiveSourceFrame {
                owner: SourceOwner::Primary,
                collection: Some(vec!["Rows".into()]),
            },
        };
        assert!(validate(&p).is_empty());
        assert!(check_source_ownership_capability(&p, None).is_ok());
        assert_eq!(run(&p, &input), Err(inactive()));
        p.graph.nodes.insert(
            4,
            Node::Const {
                value: Value::Bool(true),
            },
        );
        p.graph.nodes.insert(5, Node::Const { value: safe });
        p.graph.nodes.insert(
            6,
            Node::If {
                condition: 4,
                then: 5,
                else_: 2,
            },
        );
        p.graph.nodes.insert(
            3,
            Node::UserFunctionCall {
                function: id,
                args: vec![6],
            },
        );
        assert!(check_source_ownership_capability(&p, None).is_ok());
        assert_eq!(
            run(&p, &input),
            Ok(udf_group(vec![("Value", udf_text("function"))]))
        );
        p.graph.nodes.insert(
            4,
            Node::Const {
                value: Value::Bool(false),
            },
        );
        assert_eq!(run(&p, &input), Err(inactive()));
    }
}

#[test]
fn public_udf_nested_caller_arguments_keep_body_namespaces_isolated() {
    let mut p = project();
    let inner = FunctionId::new(7);
    let outer = FunctionId::new(8);
    p.user_functions
        .insert(inner, udf_identity("inner_identity", ScalarType::String));
    p.user_functions
        .insert(outer, udf_identity("outer_identity", ScalarType::String));
    p.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: inner,
            args: vec![2],
        },
    );
    p.graph.nodes.insert(
        4,
        Node::UserFunctionCall {
            function: outer,
            args: vec![3],
        },
    );
    p.root = scope(4);
    let input = udf_group(vec![("Value", udf_text("caller"))]);
    assert!(validate(&p).is_empty());
    assert!(check_source_ownership_capability(&p, None).is_ok());
    assert_eq!(
        run(&p, &input),
        Ok(udf_group(vec![("Value", udf_text("caller"))]))
    );
    let mut body = function(1);
    body.body.nodes.insert(
        1,
        Node::UserFunctionCall {
            function: inner,
            args: vec![2],
        },
    );
    p.user_functions.insert(outer, body);
    p.graph.nodes.insert(
        4,
        Node::UserFunctionCall {
            function: outer,
            args: vec![],
        },
    );
    refuse(&p, None, 2, Some(outer), "owned_source_field");
    // A dead body-owned 2 must neither be evaluated nor hide main argument 2.
    p.user_functions
        .insert(outer, udf_unused_argument(ScalarType::String));
    p.graph.nodes.insert(
        4,
        Node::UserFunctionCall {
            function: outer,
            args: vec![2],
        },
    );
    assert!(check_source_ownership_capability(&p, None).is_ok());
    assert_eq!(
        run(&p, &input),
        Ok(udf_group(vec![("Value", udf_text("function"))]))
    );
}

#[test]
fn public_udf_unknown_functions_keep_argument_errors_before_callee_lookup() {
    let mut p = project();
    let missing = FunctionId::new(999);
    p.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: missing,
            args: vec![2],
        },
    );
    p.root = scope(3);
    let input = udf_group(vec![("Value", udf_text("caller"))]);
    assert!(check_source_ownership_capability(&p, None).is_ok());
    assert!(validate(&p).iter().any(|issue| issue.owner
        == Some(ValidationOwner::GraphNode {
            function: None,
            node: 3,
        })));
    assert_eq!(
        run(&p, &input),
        Err(EngineError::MissingUserFunction { function: missing })
    );
    p.graph.nodes.insert(4, Node::Raise { message: None });
    p.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: missing,
            args: vec![2, 4],
        },
    );
    assert_eq!(
        run(&p, &input),
        Err(EngineError::MappingException {
            node: 4,
            message: None,
        })
    );
    p.graph.nodes.insert(
        2,
        owned(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
    );
    p.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: missing,
            args: vec![2],
        },
    );
    assert_eq!(
        run(&p, &input),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: crate::OwnedSourceError::InactiveSourceFrame {
                owner: SourceOwner::Primary,
                collection: Some(vec!["Rows".into()]),
            },
        })
    );
}

#[test]
fn public_udf_scalar_arguments_keep_specialized_contexts_and_visit_keys() {
    let mut p = project();
    let id = FunctionId::new(7);
    p.user_functions
        .insert(id, udf_identity("context_identity", ScalarType::String));
    p.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: id,
            args: vec![2],
        },
    );
    p.root = scope(3);
    use crate::OwnedCapabilityContext as Domain;
    for context in [
        Domain::IsolatedFunction,
        Domain::FailureRule,
        Domain::GeneratedItem,
        Domain::Grouping,
        Domain::Join,
        Domain::AggregateLookup,
        Domain::XmlOperator,
        Domain::TreeOperator,
        Domain::DynamicOwner,
        Domain::DynamicLoader,
        Domain::Document,
        Domain::DynamicTarget,
        Domain::Concatenate,
        Domain::SpecialConstruction,
    ] {
        assert_eq!(
            crate::source_ownership::check_context_reuse_for_test(&p, 3, context),
            Err(EngineError::UnsupportedOwnedContext {
                node: Some(2),
                function: None,
                scope: None,
                context,
            })
        );
    }
}

#[test]
fn public_udf_failure_and_dynamic_loader_arguments_stay_closed_before_loading() {
    let mut p = project();
    let id = FunctionId::new(7);
    p.user_functions
        .insert(id, udf_identity("loader_identity", ScalarType::String));
    p.graph.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: id,
            args: vec![2],
        },
    );
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source { collection: vec![] },
        selection: FailureSelection::WhenTrue { predicate: 3 },
        message: None,
    });
    refuse_context(&p, None, 2, crate::OwnedCapabilityContext::FailureRule);
    p.failure_rules.clear();
    p.extra_sources[0].dynamic_path = Some(DynamicSourcePath {
        node: 3,
        iteration: vec![],
    });
    p.graph.nodes.insert(
        4,
        Node::SourceField {
            path: vec![" Ref 雪 ".into(), "Value".into()],
            frame: None,
        },
    );
    p.root = scope(4);
    refuse_context(&p, None, 2, crate::OwnedCapabilityContext::DynamicLoader);
    struct Loader(Cell<usize>);
    impl DynamicSourceLoader for Loader {
        fn load(&self, _: &str, _: &str) -> Result<Arc<Instance>, String> {
            self.0.set(self.0.get() + 1);
            Err("unexpected acquisition".into())
        }
    }
    let loader = Loader(Cell::new(0));
    let execution =
        ExecutionContext::new(Path::new("test.json")).with_dynamic_source_loader(&loader);
    assert!(matches!(
        run_with_context(&p, &Instance::Group(Default::default()), &execution),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(2),
            context: crate::OwnedCapabilityContext::DynamicLoader,
            ..
        })
    ));
    assert_eq!(loader.0.get(), 0);
}

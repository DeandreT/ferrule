//! Literal prospective controls. Invalid namespace declarations are helper-only controls.
use crate::owned_context::{OwnedContext, OwnedLimits, SourceRoots};
use crate::source_iteration::{ObservedExtension, PositionFrame, walk_observed};
use crate::{
    EngineError, OwnedCapabilityContext as Domain, OwnedSourceError as Access, TargetSelection,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, NamedSource, NamedTarget, Node, OwnedPosition, OwnedSourceField,
    OwnedSourceRef, Project, Scope, ScopeIteration, SourceOwner,
};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::Path;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn row(value: &str) -> Instance {
    group(vec![("Value", text(value))])
}
fn scalar(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn field(owner: SourceOwner, parts: &[&str], pin: Option<&[&str]>) -> OwnedSourceField {
    OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: pin.map(path),
    }
}
fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            scalar("Value"),
            SchemaNode::group(
                "Ref",
                vec![
                    scalar("Value"),
                    SchemaNode::group("Rows", vec![scalar("Value")]).repeating(),
                ],
            ),
            SchemaNode::group(
                "Details",
                vec![SchemaNode::scalar("Counter", ScalarType::Int)],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    scalar("Value"),
                    SchemaNode::group("Rows", vec![scalar("Value")]).repeating(),
                    SchemaNode::group(
                        "Details",
                        vec![SchemaNode::scalar("Counter", ScalarType::Int)],
                    ),
                ],
            )
            .repeating(),
        ],
    )
}
fn project() -> Project {
    Project {
        source: schema(),
        target: SchemaNode::group(
            "Output",
            vec![
                scalar("Value"),
                SchemaNode::scalar("Position", ScalarType::Int),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Ref", "primary", " Ref 雪 ", "ref"]
            .into_iter()
            .map(|name| NamedSource {
                name: name.into(),
                path: format!("{name}.json"),
                schema: schema(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}
fn fixture() -> (Instance, Instance) {
    let primary = group(vec![
        ("Value", text("P.root")),
        (
            "Ref",
            group(vec![
                ("Value", text("P.Ref")),
                ("Rows", Instance::Repeated(vec![row("P.Ref.1")])),
            ]),
        ),
        (
            "Details",
            group(vec![("Counter", Instance::Scalar(Value::Int(99)))]),
        ),
        (
            "Rows",
            Instance::Repeated(vec![
                group(vec![
                    ("Value", text("P1")),
                    (
                        "Rows",
                        Instance::Repeated(vec![row("P.inner11"), row("P.inner12")]),
                    ),
                    (
                        "Details",
                        group(vec![("Counter", Instance::Scalar(Value::Int(7)))]),
                    ),
                ]),
                group(vec![
                    ("Value", text("P2")),
                    ("Rows", Instance::Repeated(vec![row("P.inner21")])),
                    ("Details", Instance::Scalar(Value::Null)),
                ]),
                group(vec![
                    ("Value", text("P3")),
                    ("Rows", Instance::Repeated(vec![])),
                ]),
            ]),
        ),
    ]);
    let extras = group(vec![
        (
            "Ref",
            group(vec![
                ("Value", text("N.root")),
                (
                    "Rows",
                    Instance::Repeated(vec![row("N1"), row("N2"), row("N3"), row("N4")]),
                ),
            ]),
        ),
        ("primary", row("named.literal.primary")),
        (" Ref 雪 ", row("named.literal.whitespace.unicode")),
        ("ref", row("named.literal.lowercase")),
    ]);
    (primary, extras)
}
fn walk<'a>(
    context: &OwnedContext<'a>,
    owner: SourceOwner,
    parts: &[&str],
) -> Result<Vec<ObservedExtension<'a>>, Access> {
    let source = OwnedSourceRef {
        owner,
        path: path(parts),
    };
    let (key, item, prefix, tail, budget) = context.select(&source)?;
    let offset = context
        .frames
        .iter()
        .map(|frame| frame.position_slot + 1)
        .max()
        .unwrap_or(0);
    walk_observed(
        item,
        tail,
        prefix,
        prefix,
        Some(key),
        context,
        offset,
        true,
        &budget,
    )
}
fn child<'a>(parent: &OwnedContext<'a>, extension: &ObservedExtension<'a>) -> OwnedContext<'a> {
    OwnedContext {
        roots: parent.roots,
        frames: extension.frames.clone(),
        origins: extension.origins.clone(),
        owned_ancestry: true,
    }
}
fn position(
    context: &OwnedContext<'_>,
    owner: &SourceOwner,
    pin: Option<&[&str]>,
    positions: &[PositionFrame],
) -> usize {
    let pin = pin.map(path);
    positions[context.position_slot(owner, pin.as_deref()).unwrap()].index
}
fn extras(extras: Instance) -> Vec<(String, Instance)> {
    let Instance::Group(fields) = extras else {
        panic!("literal extras wrapper")
    };
    fields.iter().cloned().collect()
}
fn owned_scope(owner: SourceOwner, parts: &[&str]) -> Scope {
    Scope {
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner,
                path: path(parts),
            },
            output_path: None,
        },
        ..Scope::default()
    }
}
fn value_position_project(owner: SourceOwner, parts: &[&str]) -> Project {
    let mut p = project();
    let mut value = path(parts);
    value.push("Value".into());
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: owner.clone(),
                path: value,
            },
            frame: None,
        }),
    );
    p.graph.nodes.insert(
        2,
        Node::OwnedPosition(OwnedPosition {
            owner: owner.clone(),
            collection: Some(path(parts)),
        }),
    );
    p.root = owned_scope(owner, parts);
    p.root.bindings = vec![
        Binding {
            target_field: "Value".into(),
            node: 1,
        },
        Binding {
            target_field: "Position".into(),
            node: 2,
        },
    ];
    p
}
fn pairs(output: &Instance) -> Vec<(Value, Value)> {
    output
        .as_repeated()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item.field("Value").unwrap().as_scalar().unwrap().clone(),
                item.field("Position").unwrap().as_scalar().unwrap().clone(),
            )
        })
        .collect()
}

#[test]
fn s01_s03_literal_owner_collisions_are_exact() {
    let p = project();
    let (primary, extras) = fixture();
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    assert_eq!(
        context
            .field(&field(SourceOwner::Primary, &["Ref", "Value"], None))
            .unwrap(),
        Value::String("P.Ref".into())
    );
    for (name, expected) in [
        ("Ref", "N.root"),
        ("primary", "named.literal.primary"),
        (" Ref 雪 ", "named.literal.whitespace.unicode"),
        ("ref", "named.literal.lowercase"),
    ] {
        assert_eq!(
            context
                .field(&field(named(name), &["Value"], None))
                .unwrap(),
            Value::String(expected.into())
        );
    }
    assert!(matches!(
        context.field(&field(named("Ref 雪"), &["Value"], None)),
        Err(Access::UnknownOwner { .. })
    ));
}

#[test]
fn s04_s05_public_collections_and_counts_never_cross_owners() {
    let (primary, supplied) = fixture();
    for (owner, parts, expected) in [
        (SourceOwner::Primary, vec!["Rows"], vec!["P1", "P2", "P3"]),
        (named("Ref"), vec!["Rows"], vec!["N1", "N2", "N3", "N4"]),
        (SourceOwner::Primary, vec!["Ref", "Rows"], vec!["P.Ref.1"]),
    ] {
        let p = value_position_project(owner, &parts);
        assert!(crate::validate(&p).is_empty());
        let result = crate::run_with_sources(&p, &primary, extras(supplied.clone())).unwrap();
        let expected = expected
            .iter()
            .enumerate()
            .map(|(i, value)| (Value::String((*value).into()), Value::Int((i + 1) as i64)))
            .collect::<Vec<_>>();
        assert_eq!(pairs(&result), expected);
    }
}

#[test]
fn s06_s15_nested_pins_positions_and_identity_keep_descendant_frames() {
    let p = project();
    let (primary, extras) = fixture();
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    let outer = walk(&context, SourceOwner::Primary, &["Rows"]).unwrap();
    let outer_state = child(&context, &outer[0]);
    let inner = walk(&outer_state, SourceOwner::Primary, &["Rows", "Rows"]).unwrap();
    let inner_state = child(&outer_state, &inner[1]);
    let mut positions = outer[0].extension.positions.clone();
    positions.extend(inner[1].extension.positions.iter().cloned());
    assert_eq!(
        inner_state
            .field(&field(
                SourceOwner::Primary,
                &["Rows", "Rows", "Value"],
                None
            ))
            .unwrap(),
        Value::String("P.inner12".into())
    );
    assert_eq!(
        inner_state
            .field(&field(
                SourceOwner::Primary,
                &["Rows", "Value"],
                Some(&["Rows"])
            ))
            .unwrap(),
        Value::String("P1".into())
    );
    assert_eq!(
        position(
            &inner_state,
            &SourceOwner::Primary,
            Some(&["Rows"]),
            &positions
        ),
        1
    );
    assert_eq!(
        position(
            &inner_state,
            &SourceOwner::Primary,
            Some(&["Rows", "Rows"]),
            &positions
        ),
        2
    );
    assert_eq!(
        position(&inner_state, &SourceOwner::Primary, None, &positions),
        2
    );
    let identity = walk(&inner_state, SourceOwner::Primary, &["Rows"]).unwrap();
    assert_eq!(identity.len(), 1);
    assert!(identity[0].extension.instances.is_empty());
    assert!(identity[0].extension.positions.is_empty());
    let identity_state = child(&inner_state, &identity[0]);
    assert_eq!(identity_state.frames.len(), 2);
    assert_eq!(
        position(&identity_state, &SourceOwner::Primary, None, &positions),
        2
    );
    assert_eq!(
        position(
            &identity_state,
            &SourceOwner::Primary,
            Some(&["Rows"]),
            &positions
        ),
        1
    );
}

#[test]
fn s07_s09_absence_null_and_malformed_do_not_rescue() {
    let p = project();
    let (primary, extras) = fixture();
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    let rows = walk(&context, SourceOwner::Primary, &["Rows"]).unwrap();
    for index in [1, 2] {
        let current = child(&context, &rows[index]);
        for pin in [None, Some(&["Rows"][..])] {
            assert_eq!(
                current
                    .field(&field(
                        SourceOwner::Primary,
                        &["Rows", "Details", "Counter"],
                        pin
                    ))
                    .unwrap(),
                Value::Null
            );
        }
    }
    let malformed = group(vec![(
        "Rows",
        Instance::Repeated(vec![group(vec![(
            "Details",
            Instance::Scalar(Value::Int(7)),
        )])]),
    )]);
    let roots = SourceRoots::new(&p, &malformed, &extras, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    let rows = walk(&context, SourceOwner::Primary, &["Rows"]).unwrap();
    let current = child(&context, &rows[0]);
    assert!(matches!(
        current.field(&field(
            SourceOwner::Primary,
            &["Rows", "Details", "Counter"],
            None
        )),
        Err(Access::MalformedOwnedTraversal {
            found: "a non-null scalar",
            ..
        })
    ));
}

#[test]
fn s10_s11_pin_and_namespace_error_order_is_typed() {
    let p = project();
    let (primary, extras) = fixture();
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    assert!(matches!(
        context.field(&field(
            SourceOwner::Primary,
            &["Rows", "Value"],
            Some(&["Rows"])
        )),
        Err(Access::InactiveSourceFrame { .. })
    ));
    assert!(matches!(
        context.field(&field(SourceOwner::Primary, &["Value"], Some(&["Rows"]))),
        Err(Access::UnsupportedFramePathCombination { .. })
    ));
    assert!(matches!(
        context.field(&field(named("Missing"), &["Value"], Some(&["Rows"]))),
        Err(Access::UnknownOwner { .. })
    ));
    for pin in [None, Some(path(&["Rows"]))] {
        assert!(matches!(
            context.position_slot(&SourceOwner::Primary, pin.as_deref()),
            Err(Access::InactiveSourceFrame { .. })
        ));
    }
}

#[test]
fn s12_s14_flat_root_and_empty_root_identity_are_distinct() {
    let mut p = value_position_project(SourceOwner::Primary, &[]);
    p.extra_sources.clear();
    p.source = SchemaNode::group("Flat", vec![scalar("Value")]);
    let input = Instance::Repeated(vec![row("flat1"), row("flat2"), row("flat3")]);
    assert!(!p.source.repeating);
    assert!(crate::validate(&p).is_empty());
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(field(SourceOwner::Primary, &["Value"], Some(&[]))),
    );
    assert_eq!(
        pairs(&crate::run(&p, &input).unwrap()),
        vec![
            (Value::String("flat1".into()), Value::Int(1)),
            (Value::String("flat2".into()), Value::Int(2)),
            (Value::String("flat3".into()), Value::Int(3))
        ]
    );
    let singleton = row("P.root");
    let empty = group(vec![]);
    let roots = SourceRoots::new(&p, &singleton, &empty, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    let walked = walk(&context, SourceOwner::Primary, &[]).unwrap();
    assert_eq!(walked.len(), 1);
    assert!(walked[0].frames.is_empty());
    let identity = child(&context, &walked[0]);
    assert_eq!(
        identity
            .field(&field(SourceOwner::Primary, &["Value"], None))
            .unwrap(),
        Value::String("P.root".into())
    );
    assert!(matches!(
        identity.position_slot(&SourceOwner::Primary, Some(&[])),
        Err(Access::InactiveSourceFrame { .. })
    ));
}

#[test]
fn s13_all_uniterated_repetitions_refuse_including_empty() {
    let mut p = project();
    p.extra_sources.clear();
    let empty = group(vec![]);
    for input in [
        Instance::Repeated(vec![]),
        Instance::Repeated(vec![row("first")]),
    ] {
        let roots = SourceRoots::new(&p, &input, &empty, OwnedLimits::default()).unwrap();
        let context = OwnedContext::new(&roots);
        assert!(matches!(
            context.field(&field(SourceOwner::Primary, &["Value"], None)),
            Err(Access::UniteratedOwnedRepetition { .. })
        ));
        assert_eq!(
            walk(&context, SourceOwner::Primary, &[]).unwrap().len(),
            input.as_repeated().unwrap().len()
        );
    }
}

#[test]
fn s16_cross_owner_nested_walk_preserves_both_positions() {
    let p = project();
    let (primary, extras) = fixture();
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    let outer = walk(&context, SourceOwner::Primary, &["Rows"]).unwrap();
    let current = child(&context, &outer[1]);
    let inner = walk(&current, named("Ref"), &["Rows"]).unwrap();
    let current = child(&current, &inner[0]);
    let mut positions = outer[1].extension.positions.clone();
    positions.extend(inner[0].extension.positions.iter().cloned());
    assert_eq!(
        current
            .field(&field(
                SourceOwner::Primary,
                &["Rows", "Value"],
                Some(&["Rows"])
            ))
            .unwrap(),
        Value::String("P2".into())
    );
    assert_eq!(
        current
            .field(&field(named("Ref"), &["Rows", "Value"], Some(&["Rows"])))
            .unwrap(),
        Value::String("N1".into())
    );
    assert_eq!(
        position(&current, &SourceOwner::Primary, None, &positions),
        2
    );
    assert_eq!(position(&current, &named("Ref"), None, &positions), 1);
}

#[test]
fn s17_s18_invalid_namespace_controls_are_private_registry_only() {
    let mut p = project();
    p.extra_sources.truncate(1);
    p.extra_sources.push(p.extra_sources[0].clone());
    let mut unavailable = p.extra_sources[0].clone();
    unavailable.name = "Unavailable".into();
    p.extra_sources.push(unavailable);
    let (primary, _) = fixture();
    let empty = group(vec![]);
    let roots = SourceRoots::new(&p, &primary, &empty, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    assert_eq!(
        context
            .field(&field(SourceOwner::Primary, &["Value"], None))
            .unwrap(),
        Value::String("P.root".into())
    );
    for (name, kind) in [
        ("Missing", "unknown"),
        ("Ref", "ambiguous"),
        ("Unavailable", "unavailable"),
        ("", "invalid"),
    ] {
        let result = context.field(&field(named(name), &["Value"], None));
        assert!(matches!(
            (kind, result),
            ("unknown", Err(Access::UnknownOwner { .. }))
                | ("ambiguous", Err(Access::AmbiguousOwner { .. }))
                | ("unavailable", Err(Access::SourceUnavailable { .. }))
                | ("invalid", Err(Access::InvalidNamedOwner))
        ));
    }
    assert!(!crate::validate(&p).is_empty()); // Does not alter the explicit structural validator.
}

fn phase_project(order: mapping::SortFilterOrder) -> Project {
    let mut p = value_position_project(SourceOwner::Primary, &["Rows"]);
    p.extra_sources.clear();
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Value", ScalarType::Int),
            SchemaNode::scalar("Position", ScalarType::Int),
        ],
    );
    p.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Int(1),
        },
    );
    p.graph.nodes.insert(
        4,
        Node::Call {
            function: "greater_than".into(),
            args: vec![2, 3],
        },
    );
    p.root.sort_by = Some(1);
    p.root.filter = Some(4);
    p.root.sort_filter_order = order;
    p
}
fn phase_input() -> Instance {
    group(vec![(
        "Rows",
        Instance::Repeated(
            [30, 10, 20]
                .into_iter()
                .map(|n| group(vec![("Value", Instance::Scalar(Value::Int(n)))]))
                .collect(),
        ),
    )])
}
#[test]
fn s19_s21_candidate_sort_filter_window_positions_are_phase_specific() {
    for (order, values) in [
        (mapping::SortFilterOrder::SortThenFilter, vec![20, 30]),
        (mapping::SortFilterOrder::FilterThenSort, vec![10, 20]),
    ] {
        let p = phase_project(order);
        assert_eq!(
            pairs(&crate::run(&p, &phase_input()).unwrap()),
            values
                .into_iter()
                .enumerate()
                .map(|(i, n)| (Value::Int(n), Value::Int((i + 1) as i64)))
                .collect::<Vec<_>>()
        );
    }
    let mut p = phase_project(mapping::SortFilterOrder::SortThenFilter);
    p.root.filter = None;
    p.root.windows = vec![
        mapping::SequenceWindow::SkipFirst { count: 3 },
        mapping::SequenceWindow::First { count: 3 },
    ];
    assert_eq!(
        pairs(&crate::run(&p, &phase_input()).unwrap()),
        vec![(Value::Int(20), Value::Int(1))]
    );
}

#[test]
fn s22_supported_owned_if_is_actually_lazy() {
    let mut p = project();
    let (input, _) = fixture();
    p.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("selected".into()),
        },
    );
    p.graph.nodes.insert(
        2,
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(path(&["Rows"])),
        }),
    );
    p.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.graph.nodes.insert(
        4,
        Node::If {
            condition: 3,
            then: 1,
            else_: 2,
        },
    );
    p.root.bindings = vec![Binding {
        target_field: "Value".into(),
        node: 4,
    }];
    assert!(crate::check_source_ownership_capability(&p, None).is_ok());
    assert_eq!(
        crate::run(&p, &input)
            .unwrap()
            .field("Value")
            .unwrap()
            .as_scalar(),
        Some(&Value::String("selected".into()))
    );
    p.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    assert!(matches!(
        crate::run(&p, &input),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            source: Access::InactiveSourceFrame { .. },
            ..
        })
    ));
}

#[test]
fn c02_c18_context_key_reuse_retains_actual_closed_domain_errors() {
    let mut p = project();
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(field(SourceOwner::Primary, &["Value"], None)),
    );
    // C16 is a traversal-order control, not a fabricated diagnostic reason.
    for context in [
        Domain::FailureRule,
        Domain::GeneratedItem,
        Domain::Grouping,
        Domain::Join,
        Domain::AggregateLookup,
        Domain::XmlOperator,
        Domain::TreeOperator,
        Domain::DynamicLoader,
        Domain::Document,
        Domain::DynamicTarget,
        Domain::Concatenate,
        Domain::SpecialConstruction,
    ] {
        assert!(
            matches!(crate::source_ownership::check_context_reuse_for_test(&p, 1, context),
            Err(EngineError::UnsupportedOwnedContext { node: Some(1), context: actual, .. }) if actual == context)
        );
    }
    p.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::String("safe".into()),
        },
    );
    p.graph.nodes.insert(
        4,
        Node::If {
            condition: 2,
            then: 3,
            else_: 1,
        },
    );
    assert!(matches!(
        crate::source_ownership::check_context_reuse_for_test(&p, 4, Domain::GeneratedItem),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(1),
            context: Domain::GeneratedItem,
            ..
        })
    ));
}

#[test]
fn c04_c11_owned_special_scope_and_runtime_documents_remain_closed() {
    let (input, supplied) = fixture();
    let mut p = value_position_project(SourceOwner::Primary, &["Rows"]);
    p.root.group_adjacent_by = Some(1);
    assert_eq!(
        crate::run(&p, &input),
        Ok(Instance::Repeated(vec![
            group(vec![
                ("Value", text("P1")),
                ("Position", Instance::Scalar(Value::Int(1)))
            ]),
            group(vec![
                ("Value", text("P2")),
                ("Position", Instance::Scalar(Value::Int(2)))
            ]),
            group(vec![
                ("Value", text("P3")),
                ("Position", Instance::Scalar(Value::Int(3)))
            ]),
        ]))
    );
    p.root.group_adjacent_by = None;
    p.root.group_starting_with = Some(1);
    assert!(matches!(
        crate::run(&p, &input),
        Err(EngineError::UnsupportedOwnedContext {
            context: Domain::Grouping,
            scope: Some(_),
            ..
        })
    ));
    p.root.group_starting_with = None;
    p.root.set_output_path(Some(1));
    assert!(matches!(
        crate::run(&p, &input),
        Err(EngineError::UnsupportedOwnedContext {
            context: Domain::Document,
            ..
        })
    ));
    let p = value_position_project(SourceOwner::Primary, &[]);
    let documents = Instance::DocumentSet(vec![ir::DocumentMember::new("x.json", input).unwrap()]);
    assert!(matches!(
        crate::run_with_sources(&p, &documents, extras(supplied)),
        Err(EngineError::UnsupportedOwnedContext {
            context: Domain::Document,
            ..
        })
    ));
}

#[test]
fn c14_c15_trace_debug_activation_is_selected_not_stored() {
    struct Sink(Cell<usize>);
    impl crate::TraceSink for Sink {
        fn record(&self, _: crate::TraceEvent) {
            self.0.set(self.0.get() + 1);
        }
    }
    let (input, _) = fixture();
    let mut p = project();
    p.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("legacy".into()),
        },
    );
    p.graph.nodes.insert(
        2,
        Node::OwnedSourceField(field(SourceOwner::Primary, &["Value"], None)),
    );
    p.root.bindings = vec![Binding {
        target_field: "Value".into(),
        node: 1,
    }];
    p.extra_targets.push(NamedTarget {
        name: "primary".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 2,
            }],
            ..Scope::default()
        },
    });
    let sink = Sink(Cell::new(0));
    let execution = crate::ExecutionContext::new(Path::new("test.json")).with_trace_sink(&sink);
    assert!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input,
            vec![],
            &execution,
            TargetSelection::Primary
        )
        .is_ok()
    );
    assert!(sink.0.get() > 0);
    sink.0.set(0);
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input,
            vec![],
            &execution,
            TargetSelection::Named("primary")
        ),
        Ok(crate::SelectedTargetOutput::Named(crate::NamedOutput {
            name: "primary".into(),
            instance: row("P.root"),
        }))
    );
    assert!(sink.0.get() > 0);
    struct Hook(Cell<usize>);
    impl crate::DebugHook for Hook {
        fn before_target_write(&self, _: &crate::PendingTargetWrite) -> crate::DebugDecision {
            self.0.set(self.0.get() + 1);
            crate::DebugDecision::Resume
        }
    }
    let hook = Hook(Cell::new(0));
    let execution = crate::ExecutionContext::new(Path::new("test.json")).with_debug_hook(&hook);
    assert!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input,
            vec![],
            &execution,
            TargetSelection::Primary
        )
        .is_ok()
    );
    assert!(hook.0.get() > 0);
    hook.0.set(0);
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input,
            vec![],
            &execution,
            TargetSelection::Named("primary")
        ),
        Ok(crate::SelectedTargetOutput::Named(crate::NamedOutput {
            name: "primary".into(),
            instance: row("P.root"),
        }))
    );
    assert!(hook.0.get() > 0);
    let admission =
        crate::source_ownership::admit_project(&p, Some(TargetSelection::Primary)).unwrap();
    assert!(!admission.used_owned);
    let admission =
        crate::source_ownership::admit_project(&p, Some(TargetSelection::Named("primary")))
            .unwrap();
    assert!(admission.used_owned);
    // An unsupported sibling is still excluded by isolated Primary evaluation,
    // but all-target and selected-named runs must refuse before any callback.
    p.extra_targets[0].root.group_adjacent_by = Some(2);
    sink.0.set(0);
    hook.0.set(0);
    let execution = crate::ExecutionContext::new(Path::new("test.json"))
        .with_trace_sink(&sink)
        .with_debug_hook(&hook);
    for result in [
        crate::run_outputs_with_sources_and_context(&p, &input, vec![], &execution).map(|_| ()),
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input,
            vec![],
            &execution,
            TargetSelection::Named("primary"),
        )
        .map(|_| ()),
    ] {
        assert_eq!(
            result,
            Err(EngineError::UnsupportedOwnedContext {
                node: Some(2),
                function: None,
                scope: None,
                context: Domain::Grouping,
            })
        );
    }
    assert_eq!(sink.0.get(), 0);
    assert_eq!(hook.0.get(), 0);
    crate::run_selected_target_with_sources_and_context(
        &p,
        &input,
        vec![],
        &execution,
        TargetSelection::Primary,
    )
    .unwrap();
    assert!(sink.0.get() > 0);
    assert!(hook.0.get() > 0);
}

#[test]
fn public_run_observers_borrow_real_roots_and_preserve_debug_errors_and_cancellation() {
    #[derive(Debug, PartialEq)]
    enum Callback {
        // Index into the current run's complete event stream.
        Trace(usize),
        Value(mapping::NodeId),
        Input(mapping::NodeId, mapping::NodeId, usize),
        Write,
        Failure(mapping::NodeId),
    }
    struct Sink<'a> {
        source: &'a Instance,
        events: RefCell<Vec<crate::TraceEvent>>,
        contexts: RefCell<Vec<crate::TraceOwnedSnapshot>>,
        callbacks: &'a RefCell<Vec<Callback>>,
    }
    impl crate::TraceSink for Sink<'_> {
        fn record(&self, event: crate::TraceEvent) {
            self.callbacks
                .borrow_mut()
                .push(Callback::Trace(self.events.borrow().len()));
            self.events.borrow_mut().push(event);
        }
        fn wants_owned_context_for(&self, event: &crate::TraceEvent) -> bool {
            matches!(
                event,
                crate::TraceEvent::NodeValue { .. }
                    | crate::TraceEvent::NodeInputValue { .. }
                    | crate::TraceEvent::TargetFieldWritten { .. }
                    | crate::TraceEvent::TargetProduced { .. }
            )
        }
        fn record_with_owned_context(
            &self,
            event: crate::TraceEvent,
            context: Result<crate::TraceOwnedWitness<'_>, &crate::TraceOwnedContextError>,
        ) {
            let witness = context.unwrap();
            assert!(std::ptr::eq(witness.roots[0].input.unwrap(), self.source));
            assert_eq!(witness.roots[0].owner, crate::TraceOwnedOwnerRef::Primary);
            assert_eq!(witness.phase, crate::TraceOwnedPhase::Output);
            self.contexts.borrow_mut().push(witness.snapshot().unwrap());
            self.record(event);
        }
    }
    struct Hook<'a> {
        source: &'a Instance,
        values: Cell<usize>,
        inputs: Cell<usize>,
        writes: Cell<usize>,
        failures: RefCell<Vec<(EngineError, crate::TraceOwnedSnapshot)>>,
        cancel: Cell<bool>,
        callbacks: &'a RefCell<Vec<Callback>>,
    }
    impl crate::DebugHook for Hook<'_> {
        fn before_target_write(&self, write: &crate::PendingTargetWrite) -> crate::DebugDecision {
            assert_eq!(write.field, "Value");
            self.callbacks.borrow_mut().push(Callback::Write);
            self.writes.set(self.writes.get() + 1);
            crate::DebugDecision::Resume
        }
        fn after_node_value(&self, value: &crate::PendingNodeValue) -> crate::DebugDecision {
            self.callbacks
                .borrow_mut()
                .push(Callback::Value(value.node));
            self.values.set(self.values.get() + 1);
            crate::DebugDecision::Resume
        }
        fn after_node_input(&self, input: &crate::PendingNodeInput) -> crate::DebugDecision {
            self.callbacks.borrow_mut().push(Callback::Input(
                input.consumer,
                input.input,
                input.input_index,
            ));
            self.inputs.set(self.inputs.get() + 1);
            crate::DebugDecision::Resume
        }
        fn wants_node_failures(&self) -> bool {
            true
        }
        fn wants_owned_context_for_node_failure(&self, _: &crate::PendingNodeFailure) -> bool {
            true
        }
        fn after_node_failure_with_owned_context(
            &self,
            failure: &crate::PendingNodeFailure,
            error: &EngineError,
            context: Result<crate::TraceOwnedWitness<'_>, &crate::TraceOwnedContextError>,
        ) -> crate::DebugDecision {
            assert_eq!(failure.node, 3);
            self.callbacks
                .borrow_mut()
                .push(Callback::Failure(failure.node));
            let witness = context.unwrap();
            assert!(std::ptr::eq(witness.roots[0].input.unwrap(), self.source));
            assert_eq!(witness.phase, crate::TraceOwnedPhase::Output);
            // Compare the complete typed error while it is borrowed, rather than
            // accepting the bounded UI error preview as an execution oracle.
            assert_eq!(
                error,
                &EngineError::MappingException {
                    node: 3,
                    message: Some("P.root".into()),
                }
            );
            self.failures.borrow_mut().push((
                EngineError::MappingException {
                    node: 3,
                    message: Some("P.root".into()),
                },
                witness.snapshot().unwrap(),
            ));
            if self.cancel.get() {
                crate::DebugDecision::Cancel
            } else {
                crate::DebugDecision::Resume
            }
        }
    }
    let (input, _) = fixture();
    let mut p = project();
    p.extra_sources.clear();
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(field(SourceOwner::Primary, &["Value"], None)),
    );
    p.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("!".into()),
        },
    );
    p.graph.nodes.insert(
        3,
        Node::Call {
            function: "concat".into(),
            args: vec![1, 2],
        },
    );
    p.root.bindings = vec![Binding {
        target_field: "Value".into(),
        node: 3,
    }];
    p.extra_targets.push(NamedTarget {
        name: "primary".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: p.root.clone(),
    });
    let callbacks = RefCell::new(Vec::new());
    let sink = Sink {
        source: &input,
        events: RefCell::new(Vec::new()),
        contexts: RefCell::new(Vec::new()),
        callbacks: &callbacks,
    };
    let hook = Hook {
        source: &input,
        values: Cell::new(0),
        inputs: Cell::new(0),
        writes: Cell::new(0),
        failures: RefCell::new(Vec::new()),
        cancel: Cell::new(false),
        callbacks: &callbacks,
    };
    let execution = crate::ExecutionContext::new(Path::new("test.json"))
        .with_trace_sink(&sink)
        .with_debug_hook(&hook);
    assert_eq!(
        crate::run_with_context(&p, &input, &execution),
        Ok(row("P.root!"))
    );
    assert_eq!(hook.writes.get(), 2);
    assert_eq!(hook.inputs.get(), 4);
    assert_eq!(hook.values.get(), 6);
    assert_eq!(sink.contexts.borrow().len(), 14);
    assert!(hook.failures.borrow().is_empty());
    sink.events.borrow_mut().clear();
    sink.contexts.borrow_mut().clear();
    assert_eq!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input,
            vec![],
            &execution,
            TargetSelection::Named("primary"),
        ),
        Ok(crate::SelectedTargetOutput::Named(crate::NamedOutput {
            name: "primary".into(),
            instance: row("P.root!"),
        }))
    );
    let targets = sink
        .events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            crate::TraceEvent::TargetProduced { scope, .. } => Some(scope.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 1);
    assert_eq!(
        targets[0].target,
        crate::TraceTarget::Named("primary".into())
    );
    assert_eq!(
        targets[0].source_scope,
        mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Named {
            index: 0,
            name: "primary".into()
        },)
    );
    assert_eq!(sink.contexts.borrow().len(), 7);
    p.graph.nodes.insert(3, Node::Raise { message: Some(1) });
    // Literal source-derived oracle: the once scope starts and admits its one
    // candidate, then Raise's message succeeds and reaches input zero. Raise
    // itself never succeeds; no target write, output or scope finish follows.
    let scope = crate::TraceScope {
        target: crate::TraceTarget::Primary,
        target_path: Vec::new(),
        structural_path: Vec::new(),
        source_scope: mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary),
    };
    let expected_prefix = vec![
        crate::TraceEvent::ScopeStarted {
            scope: scope.clone(),
            iteration: crate::TraceIteration::Once,
            positions: Vec::new(),
        },
        crate::TraceEvent::IterationCandidate {
            scope,
            ordinal: 1,
            positions: Vec::new(),
            source_row: None,
        },
        crate::TraceEvent::NodeValue {
            node: 1,
            positions: Vec::new(),
            value: crate::TraceValue {
                value_type: "string",
                preview: "P.root".into(),
                truncated: false,
            },
        },
        crate::TraceEvent::NodeInputValue {
            consumer: 3,
            input: 1,
            input_index: 0,
            positions: Vec::new(),
            value: crate::TraceValue {
                value_type: "string",
                preview: "P.root".into(),
                truncated: false,
            },
        },
    ];
    sink.events.borrow_mut().clear();
    sink.contexts.borrow_mut().clear();
    callbacks.borrow_mut().clear();
    hook.values.set(0);
    hook.inputs.set(0);
    hook.writes.set(0);
    let trace_only = crate::ExecutionContext::new(Path::new("test.json")).with_trace_sink(&sink);
    assert_eq!(
        crate::run_with_context(&p, &input, &trace_only),
        Err(EngineError::MappingException {
            node: 3,
            message: Some("P.root".into()),
        })
    );
    // Validate the actual public trace-only baseline independently before using
    // it for the complete Resume/Cancel comparisons below.
    let trace_only_prefix = sink.events.borrow().clone();
    assert_eq!(trace_only_prefix, expected_prefix);
    assert_eq!(
        *callbacks.borrow(),
        vec![
            Callback::Trace(0),
            Callback::Trace(1),
            Callback::Trace(2),
            Callback::Trace(3)
        ]
    );
    assert_eq!(sink.contexts.borrow().len(), 2);
    assert_eq!(
        (hook.values.get(), hook.inputs.get(), hook.writes.get()),
        (0, 0, 0)
    );
    assert!(hook.failures.borrow().is_empty());
    let expected_callbacks = vec![
        Callback::Trace(0),
        Callback::Trace(1),
        Callback::Trace(2),
        Callback::Value(1),
        Callback::Trace(3),
        Callback::Input(3, 1, 0),
        Callback::Failure(3),
    ];
    for (cancel, error, failure_count) in [
        (
            false,
            EngineError::MappingException {
                node: 3,
                message: Some("P.root".into()),
            },
            1,
        ),
        (true, EngineError::DebugCancelled, 2),
    ] {
        sink.events.borrow_mut().clear();
        sink.contexts.borrow_mut().clear();
        callbacks.borrow_mut().clear();
        hook.values.set(0);
        hook.inputs.set(0);
        hook.writes.set(0);
        hook.cancel.set(cancel);
        assert_eq!(crate::run_with_context(&p, &input, &execution), Err(error));
        assert_eq!(*sink.events.borrow(), trace_only_prefix);
        assert!(sink.events.borrow().iter().all(|event| !matches!(
            event,
            crate::TraceEvent::NodeValue { node: 3, .. }
                | crate::TraceEvent::TargetFieldWritten { .. }
                | crate::TraceEvent::TargetProduced { .. }
                | crate::TraceEvent::ScopeFinished { .. }
        )));
        // Successful value/input trace records precede their debug callbacks;
        // the failure callback is last, with no later event or debug callback.
        assert_eq!(*callbacks.borrow(), expected_callbacks);
        assert_eq!(sink.contexts.borrow().len(), 2);
        assert_eq!(
            (hook.values.get(), hook.inputs.get(), hook.writes.get()),
            (1, 1, 0)
        );
        assert_eq!(hook.failures.borrow().len(), failure_count);
    }
}

#[test]
fn legacy_walk_provenance_follows_actual_base_not_name_prefix() {
    let (input, supplied) = fixture();
    let mut p = value_position_project(SourceOwner::Primary, &["Ref", "Rows"]);
    p.root.iteration = ScopeIteration::Source(path(&["Ref", "Rows"]));
    assert_eq!(
        pairs(&crate::run_with_sources(&p, &input, extras(supplied.clone())).unwrap()),
        vec![(Value::String("P.Ref.1".into()), Value::Int(1))]
    );
    let mut p = value_position_project(named("Ref"), &["Rows"]);
    p.root.iteration = ScopeIteration::Source(path(&["Ref", "Rows"]));
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(field(named("Ref"), &["Rows", "Value"], Some(&["Rows"]))),
    );
    // Actual primary Ref wins legacy iteration; it must not fabricate a named occurrence.
    assert!(matches!(
        crate::run_with_sources(&p, &input, extras(supplied.clone())),
        Err(EngineError::OwnedSourceAccess {
            source: Access::InactiveSourceFrame { .. },
            ..
        })
    ));
    let no_collision = group(vec![("Value", text("primary"))]);
    assert_eq!(
        pairs(&crate::run_with_sources(&p, &no_collision, extras(supplied)).unwrap()),
        vec![
            (Value::String("N1".into()), Value::Int(1)),
            (Value::String("N2".into()), Value::Int(2)),
            (Value::String("N3".into()), Value::Int(3)),
            (Value::String("N4".into()), Value::Int(4))
        ]
    );
}

#[test]
fn legacy_expressions_coexist_with_named_owned_occurrences() {
    let mut p = value_position_project(named("Ref"), &["Rows"]);
    p.graph.nodes.insert(
        1,
        Node::SourceField {
            path: path(&["Value"]),
            frame: None,
        },
    );
    p.graph.nodes.insert(
        2,
        Node::Position {
            collection: path(&["Rows"]),
        },
    );
    let (input, supplied) = fixture();
    assert_eq!(
        pairs(&crate::run_with_sources(&p, &input, extras(supplied)).unwrap()),
        vec![
            (Value::String("N1".into()), Value::Int(1)),
            (Value::String("N2".into()), Value::Int(2)),
            (Value::String("N3".into()), Value::Int(3)),
            (Value::String("N4".into()), Value::Int(4))
        ]
    );
}

#[test]
fn owned_named_discovery_uses_owner_not_first_path_segment() {
    let mut p = project();
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(field(named("Ref"), &["Value"], None)),
    );
    p.root.bindings = vec![Binding {
        target_field: "Value".into(),
        node: 1,
    }];
    let required = crate::required_sources_for_target(&p, TargetSelection::Primary).unwrap();
    assert_eq!(
        required
            .static_sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Ref"]
    );
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(field(SourceOwner::Primary, &["Ref", "Value"], None)),
    );
    assert!(
        crate::required_sources_for_target(&p, TargetSelection::Primary)
            .unwrap()
            .static_sources
            .is_empty()
    );
}

#[test]
fn b01_b05_count_payload_and_walk_limits_are_checked() {
    let mut p = project();
    let (input, supplied) = fixture();
    let limits = OwnedLimits {
        roots: 0,
        ..OwnedLimits::default()
    };
    assert!(matches!(
        SourceRoots::new(&p, &input, &supplied, limits),
        Err(Access::ResourceLimit {
            resource: "root entries",
            ..
        })
    ));
    let limits = OwnedLimits {
        metadata_bytes: 0,
        ..OwnedLimits::default()
    };
    assert!(matches!(
        SourceRoots::new(&p, &input, &supplied, limits),
        Err(Access::ResourceLimit {
            resource: "metadata bytes",
            ..
        })
    ));
    // B04 successor: a nonempty named declaration costs bytes. Primary itself does not.
    p.extra_sources.clear();
    let roots = SourceRoots::new(&p, &input, &supplied, limits).unwrap();
    let context = OwnedContext::new(&roots);
    assert!(matches!(
        context.field(&field(SourceOwner::Primary, &["Value"], None)),
        Err(Access::ResourceLimit {
            resource: "metadata bytes",
            ..
        })
    ));
    let empty = group(vec![]);
    let roots = SourceRoots::new(&p, &empty, &empty, limits).unwrap();
    assert_eq!(
        walk(&OwnedContext::new(&roots), SourceOwner::Primary, &[])
            .unwrap()
            .len(),
        1
    );
    let limits = OwnedLimits {
        path_segments: 1,
        ..OwnedLimits::default()
    };
    let roots = SourceRoots::new(&p, &input, &supplied, limits).unwrap();
    assert!(matches!(
        walk(
            &OwnedContext::new(&roots),
            SourceOwner::Primary,
            &["Rows", "Rows"]
        ),
        Err(Access::ResourceLimit {
            resource: "path segments",
            ..
        })
    ));
    let limits = OwnedLimits {
        walk_contexts: 1,
        ..OwnedLimits::default()
    };
    let roots = SourceRoots::new(&p, &input, &supplied, limits).unwrap();
    assert!(matches!(
        walk(&OwnedContext::new(&roots), SourceOwner::Primary, &["Rows"]),
        Err(Access::ResourceLimit {
            resource: "walk contexts",
            ..
        })
    ));
    let limits = OwnedLimits {
        depth: 1,
        ..OwnedLimits::default()
    };
    let roots = SourceRoots::new(&p, &input, &supplied, limits).unwrap();
    let context = OwnedContext::new(&roots);
    let outer = walk(&context, SourceOwner::Primary, &["Rows"]).unwrap();
    let current = child(&context, &outer[0]);
    assert!(matches!(
        walk(&current, SourceOwner::Primary, &["Rows", "Rows"]),
        Err(Access::ResourceLimit {
            resource: "frame depth",
            ..
        })
    ));
}

#[test]
fn metadata_and_derived_slot_overflow_refuse_before_walk_allocations() {
    let mut p = project();
    p.extra_sources.clear();
    let empty = group(vec![]);
    let limits = OwnedLimits {
        depth: usize::MAX,
        walk_contexts: usize::MAX,
        ..OwnedLimits::default()
    };
    assert!(matches!(
        SourceRoots::new(&p, &empty, &empty, limits),
        Err(Access::ResourceLimit {
            resource: "derived metadata slots",
            ..
        })
    ));
    let roots = SourceRoots::new(&p, &empty, &empty, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    let budget = roots
        .budget
        .work(None, &[], None, &context.frames, &context.origins)
        .unwrap();
    assert!(matches!(
        budget.charge(usize::MAX),
        Err(Access::ResourceLimit {
            resource: "metadata bytes",
            ..
        })
    ));
    assert!(matches!(
        budget.reserve(usize::MAX),
        Err(Access::ResourceLimit {
            resource: "derived metadata slots",
            ..
        })
    ));
}

#[test]
fn oversized_selected_owner_and_path_refuse_before_diagnostic_copies() {
    let mut p = project();
    p.extra_sources.clear();
    let limit = OwnedLimits::default().metadata_bytes;
    for node in [
        Node::OwnedSourceField(field(named(&"x".repeat(limit + 1)), &["Value"], None)),
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["x".repeat(limit + 1)],
            },
            frame: None,
        }),
    ] {
        p.graph.nodes.insert(1, node);
        p.root.bindings = vec![Binding {
            target_field: "Value".into(),
            node: 1,
        }];
        assert!(matches!(
            crate::check_source_ownership_capability(&p, None),
            Err(EngineError::OwnedSourceAccess {
                source: Access::ResourceLimit {
                    resource: "metadata bytes",
                    ..
                },
                ..
            })
        ));
    }
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["x".into(); 257],
            },
            frame: None,
        }),
    );
    assert!(matches!(
        crate::check_source_ownership_capability(&p, None),
        Err(EngineError::OwnedSourceAccess {
            source: Access::ResourceLimit {
                resource: "path segments",
                ..
            },
            ..
        })
    ));
}

#[test]
fn dead_owned_nodes_and_unselected_targets_do_not_activate_registry_limits() {
    let mut p = project();
    let (input, _) = fixture();
    p.extra_sources = (0..5000)
        .map(|n| NamedSource {
            name: format!("unused_{n}"),
            path: "unused.json".into(),
            schema: schema(),
            options: Default::default(),
            dynamic_path: None,
        })
        .collect();
    p.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("legacy".into()),
        },
    );
    p.graph.nodes.insert(
        2,
        Node::OwnedSourceField(field(SourceOwner::Primary, &["Value"], None)),
    );
    p.root.bindings = vec![Binding {
        target_field: "Value".into(),
        node: 1,
    }];
    p.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: None,
        schema: p.target.clone(),
        options: Default::default(),
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 2,
            }],
            ..Scope::default()
        },
    });
    assert!(crate::run_selected_target(&p, &input, TargetSelection::Primary).is_ok());
    assert!(
        !crate::source_ownership::admit_project(&p, Some(TargetSelection::Primary))
            .unwrap()
            .used_owned
    );
    assert!(matches!(
        crate::check_source_ownership_capability(&p, Some(TargetSelection::Named("Other"))),
        Err(EngineError::OwnedSourceAccess {
            source: Access::ResourceLimit {
                resource: "root entries",
                ..
            },
            ..
        })
    ));
}

#[test]
fn owned_terminal_null_pruning_preserves_source_ordinals_and_legacy_candidates() {
    let mut p = value_position_project(SourceOwner::Primary, &[]);
    p.extra_sources.clear();
    p.source = SchemaNode::group("Flat", vec![scalar("Value")]);
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(field(SourceOwner::Primary, &["Value"], Some(&[]))),
    );
    let input = Instance::Repeated(vec![
        Instance::Scalar(Value::Null),
        row("kept1"),
        Instance::Scalar(Value::JsonNull(ir::JsonNull)),
        row("kept2"),
    ]);
    assert_eq!(
        pairs(&crate::run(&p, &input).unwrap()),
        vec![
            (Value::String("kept1".into()), Value::Int(2)),
            (Value::String("kept2".into()), Value::Int(4))
        ]
    );
    let old = crate::source_iteration::walk(&input, &[], &[], &[], &[]);
    assert_eq!(old.len(), 4);
    p.root.iteration = ScopeIteration::Source(vec![]);
    assert_eq!(
        pairs(&crate::run(&p, &input).unwrap()),
        vec![
            (Value::Null, Value::Int(1)),
            (Value::String("kept1".into()), Value::Int(2)),
            (Value::Null, Value::Int(3)),
            (Value::String("kept2".into()), Value::Int(4))
        ]
    );
    let empty = group(vec![]);
    for null in [
        Instance::Scalar(Value::Null),
        Instance::Scalar(Value::JsonNull(ir::JsonNull)),
    ] {
        let roots = SourceRoots::new(&p, &null, &empty, OwnedLimits::default()).unwrap();
        assert!(
            walk(&OwnedContext::new(&roots), SourceOwner::Primary, &[])
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn owned_access_error_source_keeps_concrete_cause_and_exact_scope_metadata() {
    let mut p = project();
    p.extra_sources.clear();
    p.graph.nodes.insert(
        1,
        Node::OwnedSourceField(field(SourceOwner::Primary, &["Value"], Some(&[]))),
    );
    p.root.bindings = vec![Binding {
        target_field: "Value".into(),
        node: 1,
    }];
    let input = Instance::Repeated(vec![row("first"), row("second")]);
    let error = crate::run(&p, &input).expect_err("a repeated root pin requires a real occurrence");
    let expected = Access::InactiveSourceFrame {
        owner: SourceOwner::Primary,
        collection: Some(vec![]),
    };
    match &error {
        EngineError::OwnedSourceAccess {
            node,
            scope,
            source,
        } => {
            assert_eq!(*node, Some(1));
            assert!(scope.is_none());
            assert_eq!(source, &expected);
        }
        other => panic!("expected actual owned field failure, got {other:?}"),
    }
    let cause = std::error::Error::source(&error)
        .expect("an owned access error retains its typed source")
        .downcast_ref::<Access>()
        .expect("the direct source must be OwnedSourceError");
    assert_eq!(cause, &expected);

    p.graph.nodes.clear();
    p.extra_sources.push(NamedSource {
        name: "Absent".into(),
        path: "absent.json".into(),
        schema: p.source.clone(),
        options: Default::default(),
        dynamic_path: None,
    });
    p.root = Scope {
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: named("Absent"),
                path: vec![],
            },
            output_path: None,
        },
        ..Scope::default()
    };
    let error = crate::run(&p, &input).expect_err("the declared named root has no supplied input");
    let expected = Access::SourceUnavailable {
        owner: named("Absent"),
    };
    let location = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary);
    match &error {
        EngineError::OwnedSourceAccess {
            node,
            scope,
            source,
        } => {
            assert_eq!(*node, None);
            assert_eq!(scope.as_deref(), Some(&location));
            assert_eq!(source, &expected);
        }
        other => panic!("expected actual owned scope failure, got {other:?}"),
    }
    let cause = std::error::Error::source(&error)
        .expect("a scoped owned access error retains its typed source")
        .downcast_ref::<Access>()
        .expect("scope boxing must not change the direct source type");
    assert_eq!(cause, &expected);
}

fn empty_root_pin_project(owner: SourceOwner, parts: &[&str]) -> Project {
    let mut p = project();
    p.graph
        .nodes
        .insert(1, Node::OwnedSourceField(field(owner, parts, Some(&[]))));
    p.root.bindings = vec![Binding {
        target_field: "Value".into(),
        node: 1,
    }];
    p
}

#[test]
fn empty_root_pins_read_exact_public_primary_named_and_literal_roots() {
    let (primary, supplied) = fixture();
    for (owner, parts, expected) in [
        (SourceOwner::Primary, vec!["Value"], "P.root"),
        (SourceOwner::Primary, vec!["Ref", "Value"], "P.Ref"),
        (named("Ref"), vec!["Value"], "N.root"),
        (named("primary"), vec!["Value"], "named.literal.primary"),
        (
            named(" Ref 雪 "),
            vec!["Value"],
            "named.literal.whitespace.unicode",
        ),
        (named("ref"), vec!["Value"], "named.literal.lowercase"),
    ] {
        let mut p = empty_root_pin_project(owner.clone(), &parts);
        let expected = group(vec![("Value", text(expected))]);
        assert_eq!(
            crate::run_with_sources(&p, &primary, extras(supplied.clone())),
            Ok(expected.clone()),
        );
        p.graph
            .nodes
            .insert(1, Node::OwnedSourceField(field(owner, &parts, None)));
        assert_eq!(
            crate::run_with_sources(&p, &primary, extras(supplied.clone())),
            Ok(expected),
        );
    }
}

#[test]
fn empty_root_pins_read_public_scalar_roots_and_preserve_null() {
    for owner in [SourceOwner::Primary, named("Ref")] {
        for value in [
            Value::String("scalar root".into()),
            Value::Null,
            Value::JsonNull(ir::JsonNull),
        ] {
            let mut p = empty_root_pin_project(owner.clone(), &[]);
            p.source = scalar("Input");
            p.extra_sources.truncate(1);
            p.extra_sources[0].schema = scalar("Input");
            let input = Instance::Scalar(value.clone());
            let expected = group(vec![("Value", Instance::Scalar(value.clone()))]);
            assert_eq!(
                crate::run_with_sources(&p, &input, vec![("Ref".into(), input.clone())]),
                Ok(expected.clone()),
            );
            p.graph
                .nodes
                .insert(1, Node::OwnedSourceField(field(owner.clone(), &[], None)));
            assert_eq!(
                crate::run_with_sources(&p, &input, vec![("Ref".into(), input.clone())]),
                Ok(expected),
            );
        }
    }
}

#[test]
fn empty_root_pins_ignore_unrelated_row_frames_and_preserve_positions() {
    let (input, supplied) = fixture();
    for (iteration_owner, count) in [(SourceOwner::Primary, 3usize), (named("Ref"), 4)] {
        for (read_owner, value) in [
            (SourceOwner::Primary, "P.root"),
            (named("Ref"), "N.root"),
            (named("primary"), "named.literal.primary"),
        ] {
            let mut p = value_position_project(iteration_owner.clone(), &["Rows"]);
            p.graph.nodes.insert(
                1,
                Node::OwnedSourceField(field(read_owner, &["Value"], Some(&[]))),
            );
            let expected = Instance::Repeated(
                (1..=count)
                    .map(|position| {
                        group(vec![
                            ("Value", text(value)),
                            ("Position", Instance::Scalar(Value::Int(position as i64))),
                        ])
                    })
                    .collect(),
            );
            assert_eq!(
                crate::run_with_sources(&p, &input, extras(supplied.clone())),
                Ok(expected),
            );
        }
    }
}

#[test]
fn empty_root_pins_do_not_create_positions_or_unwrap_active_collections() {
    let (input, supplied) = fixture();
    for owner in [SourceOwner::Primary, named("Ref")] {
        let mut p = empty_root_pin_project(owner.clone(), &["Value"]);
        p.graph.nodes.insert(
            2,
            Node::OwnedPosition(OwnedPosition {
                owner: owner.clone(),
                collection: Some(vec![]),
            }),
        );
        p.root.bindings.push(Binding {
            target_field: "Position".into(),
            node: 2,
        });
        assert_eq!(
            crate::run_with_sources(&p, &input, extras(supplied.clone())),
            Err(EngineError::OwnedSourceAccess {
                node: Some(2),
                scope: None,
                source: Access::InactiveSourceFrame {
                    owner: owner.clone(),
                    collection: Some(vec![])
                },
            }),
        );
        let mut p = value_position_project(owner.clone(), &["Rows"]);
        p.graph.nodes.insert(
            1,
            Node::OwnedSourceField(field(owner.clone(), &["Rows", "Value"], Some(&[]))),
        );
        assert_eq!(
            crate::run_with_sources(&p, &input, extras(supplied.clone())),
            Err(EngineError::OwnedSourceAccess {
                node: Some(1),
                scope: None,
                source: Access::UniteratedOwnedRepetition {
                    path: path(&["Rows", "Value"])
                },
            }),
        );
        p.graph.nodes.insert(
            1,
            Node::OwnedSourceField(field(owner.clone(), &["Rows", "Value"], None)),
        );
        let values = if owner == SourceOwner::Primary {
            vec!["P1", "P2", "P3"]
        } else {
            vec!["N1", "N2", "N3", "N4"]
        };
        let expected = Instance::Repeated(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    group(vec![
                        ("Value", text(value)),
                        ("Position", Instance::Scalar(Value::Int((index + 1) as i64))),
                    ])
                })
                .collect(),
        );
        assert_eq!(
            crate::run_with_sources(&p, &input, extras(supplied.clone())),
            Ok(expected)
        );
    }
}

#[test]
fn empty_root_pins_require_declared_or_actual_repeated_root_occurrences() {
    for owner in [SourceOwner::Primary, named("Ref")] {
        for (declared_repeating, input) in [
            (false, Instance::Repeated(vec![])),
            (false, Instance::Repeated(vec![row("first"), row("second")])),
            (true, row("singleton is not an occurrence")),
            (true, Instance::Repeated(vec![row("first"), row("second")])),
        ] {
            let mut p = empty_root_pin_project(owner.clone(), &["Value"]);
            p.source = SchemaNode::group("Input", vec![scalar("Value")]);
            p.extra_sources.truncate(1);
            p.extra_sources[0].schema = p.source.clone();
            p.source.repeating = if owner == SourceOwner::Primary {
                declared_repeating
            } else {
                !declared_repeating
            };
            p.extra_sources[0].schema.repeating = if owner == SourceOwner::Primary {
                !declared_repeating
            } else {
                declared_repeating
            };
            assert_eq!(
                crate::run_with_sources(&p, &input, vec![("Ref".into(), input.clone())]),
                Err(EngineError::OwnedSourceAccess {
                    node: Some(1),
                    scope: None,
                    source: Access::InactiveSourceFrame {
                        owner: owner.clone(),
                        collection: Some(vec![])
                    },
                }),
            );
        }
        // The other endpoint's schema must never decide this endpoint's root pin.
        let mut p = empty_root_pin_project(owner.clone(), &["Value"]);
        p.source = SchemaNode::group("Input", vec![scalar("Value")]);
        p.extra_sources.truncate(1);
        p.extra_sources[0].schema = p.source.clone();
        p.source.repeating = owner != SourceOwner::Primary;
        p.extra_sources[0].schema.repeating = owner == SourceOwner::Primary;
        let input = row("selected nonrepeating root");
        assert_eq!(
            crate::run_with_sources(&p, &input, vec![("Ref".into(), input.clone())]),
            Ok(group(vec![("Value", text("selected nonrepeating root"))])),
        );
        for declared_repeating in [false, true] {
            let mut p = value_position_project(owner.clone(), &[]);
            p.source = SchemaNode::group("Input", vec![scalar("Value")]);
            p.extra_sources.truncate(1);
            p.extra_sources[0].schema = p.source.clone();
            p.source.repeating = if owner == SourceOwner::Primary {
                declared_repeating
            } else {
                !declared_repeating
            };
            p.extra_sources[0].schema.repeating = if owner == SourceOwner::Primary {
                !declared_repeating
            } else {
                declared_repeating
            };
            p.graph.nodes.insert(
                1,
                Node::OwnedSourceField(field(owner.clone(), &["Value"], Some(&[]))),
            );
            let input = Instance::Repeated(vec![row("first"), row("second")]);
            let expected = Instance::Repeated(vec![
                group(vec![
                    ("Value", text("first")),
                    ("Position", Instance::Scalar(Value::Int(1))),
                ]),
                group(vec![
                    ("Value", text("second")),
                    ("Position", Instance::Scalar(Value::Int(2))),
                ]),
            ]);
            assert_eq!(
                crate::run_with_sources(&p, &input, vec![("Ref".into(), input.clone())]),
                Ok(expected)
            );
        }
    }
}

#[test]
fn empty_root_pins_cannot_borrow_another_owners_root_occurrence() {
    for (iteration_owner, read_owner) in [
        (SourceOwner::Primary, named("Ref")),
        (named("Ref"), SourceOwner::Primary),
    ] {
        let mut p = value_position_project(iteration_owner, &[]);
        p.source = SchemaNode::group("Input", vec![scalar("Value")]);
        p.extra_sources.truncate(1);
        p.extra_sources[0].schema = p.source.clone();
        p.graph.nodes.insert(
            1,
            Node::OwnedSourceField(field(read_owner.clone(), &["Value"], Some(&[]))),
        );
        let input = Instance::Repeated(vec![row("same value"), row("same value")]);
        assert_eq!(
            crate::run_with_sources(&p, &input, vec![("Ref".into(), input.clone())]),
            Err(EngineError::OwnedSourceAccess {
                node: Some(1),
                scope: None,
                source: Access::InactiveSourceFrame {
                    owner: read_owner,
                    collection: Some(vec![])
                },
            }),
        );
    }
}

#[test]
fn empty_root_pins_preserve_public_missing_malformed_and_unsupported_outcomes() {
    let input = row("primary cannot supply named absence");
    let p = empty_root_pin_project(named("Ref"), &["Value"]);
    assert_eq!(
        crate::run(&p, &input),
        Err(EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: Access::SourceUnavailable {
                owner: named("Ref")
            },
        })
    );
    for supplied in [
        group(vec![]),
        Instance::Scalar(Value::Null),
        Instance::Scalar(Value::JsonNull(ir::JsonNull)),
    ] {
        assert_eq!(
            crate::run_with_sources(&p, &input, vec![("Ref".into(), supplied)]),
            Ok(group(vec![("Value", Instance::Scalar(Value::Null))])),
        );
    }
    let p = empty_root_pin_project(SourceOwner::Primary, &["Value"]);
    assert_eq!(
        crate::run(&p, &Instance::Scalar(Value::Int(7))),
        Err(EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: Access::MalformedOwnedTraversal {
                path: path(&["Value"]),
                found: "a non-null scalar"
            },
        })
    );
    let p = empty_root_pin_project(SourceOwner::Primary, &[]);
    assert_eq!(
        crate::run(&p, &group(vec![])),
        Err(EngineError::OwnedSourceAccess {
            node: Some(1),
            scope: None,
            source: Access::MalformedOwnedTraversal {
                path: vec![],
                found: "a group"
            },
        })
    );
    for (input, domain) in [
        (Instance::DocumentSet(vec![]), Domain::Document),
        (Instance::MappedSequence(vec![]), Domain::GeneratedItem),
    ] {
        assert_eq!(
            crate::run(&p, &input),
            Err(EngineError::UnsupportedOwnedContext {
                node: Some(1),
                function: None,
                scope: None,
                context: domain,
            })
        );
    }
    let p = empty_root_pin_project(SourceOwner::Primary, &["Rows", "Value"]);
    assert_eq!(
        crate::run(&p, &row("root")),
        Ok(group(vec![("Value", Instance::Scalar(Value::Null))]))
    );
}

#[test]
fn empty_root_pins_keep_metadata_path_and_inactive_pin_bounds() {
    let mut p = project();
    p.extra_sources.clear();
    let (input, _) = fixture();
    let empty = group(vec![]);
    for (limits, request, expected) in [
        (
            OwnedLimits {
                metadata_bytes: 0,
                ..OwnedLimits::default()
            },
            field(SourceOwner::Primary, &["Value"], Some(&[])),
            Access::ResourceLimit {
                resource: "metadata bytes",
                limit: 0,
            },
        ),
        (
            OwnedLimits {
                path_segments: 1,
                ..OwnedLimits::default()
            },
            field(SourceOwner::Primary, &["Ref", "Value"], Some(&[])),
            Access::ResourceLimit {
                resource: "path segments",
                limit: 1,
            },
        ),
        (
            OwnedLimits::default(),
            field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            Access::InactiveSourceFrame {
                owner: SourceOwner::Primary,
                collection: Some(path(&["Rows"])),
            },
        ),
        (
            OwnedLimits::default(),
            field(SourceOwner::Primary, &["Value"], Some(&["Rows"])),
            Access::UnsupportedFramePathCombination {
                path: path(&["Value"]),
                frame: path(&["Rows"]),
            },
        ),
    ] {
        let roots = SourceRoots::new(&p, &input, &empty, limits).unwrap();
        assert_eq!(OwnedContext::new(&roots).field(&request), Err(expected));
    }
}

fn first_selection<'a, 'query>(
    context: &'query OwnedContext<'a>,
    source: &'query OwnedSourceRef,
    retain_first: bool,
) -> Result<
    (
        crate::source_iteration::ObservedSelection<'a, 'query>,
        crate::owned_context::WorkBudget,
    ),
    Access,
> {
    let (owner, base, prefix, tail, budget) = context.select(source)?;
    let offset = context
        .frames
        .iter()
        .map(|frame| frame.position_slot + 1)
        .max()
        .unwrap_or(0);
    let selected = crate::source_iteration::select_observed(
        crate::source_iteration::ObservedWalkRequest {
            base,
            path: tail,
            raw_prefix: prefix,
            canonical_prefix: prefix,
            owner: Some(owner),
            context,
            position_offset: offset,
            strict_collection: true,
        },
        &budget,
        retain_first,
    )?;
    Ok((selected, budget))
}

fn assert_same_first_extension(left: &ObservedExtension<'_>, right: &ObservedExtension<'_>) {
    assert_eq!(
        left.extension.instances.len(),
        right.extension.instances.len()
    );
    for (a, b) in left
        .extension
        .instances
        .iter()
        .zip(&right.extension.instances)
    {
        assert!(std::ptr::eq(*a, *b));
    }
    let positions = |extension: &ObservedExtension<'_>| {
        extension
            .extension
            .positions
            .iter()
            .map(|frame| {
                (
                    frame.collection.clone(),
                    frame.index,
                    frame.grouped,
                    frame.join,
                    frame.join_position,
                    frame.document_path.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(positions(left), positions(right));
    assert_eq!(left.frames.len(), right.frames.len());
    for (a, b) in left.frames.iter().zip(&right.frames) {
        assert_eq!(
            (a.owner, &a.collection, a.position_slot),
            (b.owner, &b.collection, b.position_slot)
        );
        assert!(std::ptr::eq(a.item, b.item));
    }
    assert_eq!(left.origins.len(), right.origins.len());
    for (a, b) in left.origins.iter().zip(&right.origins) {
        assert_eq!((a.owner, &a.path), (b.owner, &b.path));
        assert!(std::ptr::eq(a.item, b.item));
    }
}

fn charge_first_preselection(
    selected: &crate::source_iteration::ObservedSelection<'_, '_>,
    budget: &crate::owned_context::WorkBudget,
    positions: &[PositionFrame],
    context_len: usize,
) -> Result<(), Access> {
    use crate::source_iteration::reserve_observed_copy;
    budget.reserve(0)?; // Empty sort-key vector.
    budget.reserve(1)?; // Implicit First window vector.
    budget.reserve(selected.candidates)?; // No-op filter result vector.
    let mut copy = |extension: ObservedExtension<'_>| {
        reserve_observed_copy(
            budget,
            &extension.frames,
            &extension.origins,
            positions,
            context_len,
        )?;
        reserve_observed_copy(
            budget,
            &[],
            &[],
            &extension.extension.positions,
            extension.extension.instances.len(),
        )
    };
    if let Some(replay) = &selected.replay {
        replay.request.visit(&replay.budget, &mut copy)?;
    } else {
        for extension in &selected.extensions {
            reserve_observed_copy(
                budget,
                &extension.frames,
                &extension.origins,
                positions,
                context_len,
            )?;
            reserve_observed_copy(
                budget,
                &[],
                &[],
                &extension.extension.positions,
                extension.extension.instances.len(),
            )?;
        }
    }
    budget.reserve(selected.candidates)?; // First(1) window's input vector.
    budget.reserve(usize::from(selected.candidates != 0))?; // Selected output vector.
    Ok(())
}

#[test]
fn first_retention_preserves_complete_counts_frames_and_preselection_ledger() {
    let p = project();
    let (input, supplied) = fixture();
    let roots = SourceRoots::new(&p, &input, &supplied, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    for (owner, parts) in [
        (SourceOwner::Primary, vec!["Rows"]),
        (SourceOwner::Primary, vec!["Rows", "Rows"]),
        (named("Ref"), vec!["Rows"]),
        (SourceOwner::Primary, vec![]),
    ] {
        let source = OwnedSourceRef {
            owner,
            path: path(&parts),
        };
        let (all, all_budget) = first_selection(&context, &source, false).unwrap();
        let (first, first_budget) = first_selection(&context, &source, true).unwrap();
        assert_eq!(all.candidates, all.extensions.len());
        assert_eq!(first.candidates, all.candidates);
        assert_eq!(first.extensions.len(), usize::from(all.candidates != 0));
        assert_eq!(all_budget.usage(), first_budget.usage());
        if let (Some(a), Some(b)) = (all.extensions.first(), first.extensions.first()) {
            assert_same_first_extension(a, b);
        }
        charge_first_preselection(&all, &all_budget, &[], 1).unwrap();
        charge_first_preselection(&first, &first_budget, &[], 1).unwrap();
        assert_eq!(all_budget.usage(), first_budget.usage());
    }
    let outer = walk(&context, SourceOwner::Primary, &["Rows"]).unwrap();
    let nested = child(&context, &outer[0]);
    // Identity re-entry preserves the selected occurrence and every outer frame.
    let source = OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(&["Rows"]),
    };
    let (all, all_budget) = first_selection(&nested, &source, false).unwrap();
    let (first, first_budget) = first_selection(&nested, &source, true).unwrap();
    assert_eq!(first.candidates, 1);
    assert!(first.extensions[0].extension.positions.is_empty());
    assert_same_first_extension(&all.extensions[0], &first.extensions[0]);
    charge_first_preselection(&all, &all_budget, &outer[0].extension.positions, 2).unwrap();
    charge_first_preselection(&first, &first_budget, &outer[0].extension.positions, 2).unwrap();
    assert_eq!(all_budget.usage(), first_budget.usage());
}

#[test]
fn first_retention_does_not_hide_later_walk_errors_or_tighten_terminal_items() {
    let mut p = project();
    p.extra_sources.clear();
    let empty = group(vec![]);
    for bad in [
        Instance::Scalar(Value::Int(9)),
        Instance::DocumentSet(vec![]),
        Instance::MappedSequence(vec![]),
    ] {
        let input = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                group(vec![("Rows", Instance::Repeated(vec![row("first")]))]),
                group(vec![("Rows", bad)]),
            ]),
        )]);
        let roots = SourceRoots::new(&p, &input, &empty, OwnedLimits::default()).unwrap();
        let context = OwnedContext::new(&roots);
        let source = OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows", "Rows"]),
        };
        let all_error = first_selection(&context, &source, false)
            .err()
            .expect("later traversal fails");
        let first_error = first_selection(&context, &source, true)
            .err()
            .expect("later traversal still fails");
        assert_eq!(first_error, all_error);
    }
    // Terminal repeated members retain the original permissive shape contract.
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            Instance::DocumentSet(vec![]),
            Instance::MappedSequence(vec![]),
            Instance::Scalar(Value::Int(9)),
            row("last"),
        ]),
    )]);
    let roots = SourceRoots::new(&p, &input, &empty, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    let source = OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(&["Rows"]),
    };
    let (all, all_budget) = first_selection(&context, &source, false).unwrap();
    let (first, first_budget) = first_selection(&context, &source, true).unwrap();
    assert_eq!(first.candidates, 4);
    assert_same_first_extension(&all.extensions[0], &first.extensions[0]);
    assert_eq!(first_budget.usage(), all_budget.usage());
}

#[test]
fn first_retention_preserves_lowered_walk_resource_errors() {
    let mut p = project();
    p.extra_sources.clear();
    let (input, supplied) = fixture();
    for limits in [
        OwnedLimits {
            walk_contexts: 1,
            ..OwnedLimits::default()
        },
        OwnedLimits {
            depth: 1,
            ..OwnedLimits::default()
        },
        OwnedLimits {
            path_segments: 1,
            ..OwnedLimits::default()
        },
        OwnedLimits {
            metadata_bytes: 64,
            ..OwnedLimits::default()
        },
    ] {
        let roots = SourceRoots::new(&p, &input, &supplied, limits).unwrap();
        let context = OwnedContext::new(&roots);
        let source = OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows", "Rows"]),
        };
        let all = first_selection(&context, &source, false)
            .err()
            .expect("limited full walk fails");
        let first = first_selection(&context, &source, true)
            .err()
            .expect("limited retained walk fails");
        assert_eq!(first, all);
    }
}

#[test]
fn first_retention_guard_keeps_explicit_sequence_controls_on_the_full_path() {
    use crate::eval_scope::retain_first_source_candidate as eligible;
    let mut scope = owned_scope(SourceOwner::Primary, &["Rows"]);
    scope.iteration_output = mapping::IterationOutput::First;
    assert!(eligible(&scope));
    let mut changed = scope.clone();
    changed.filter = Some(99);
    assert!(!eligible(&changed));
    let mut changed = scope.clone();
    changed.sort_by = Some(99);
    assert!(!eligible(&changed));
    let mut changed = scope.clone();
    changed.sort_then_by.push(mapping::SortKey {
        node: 99,
        descending: false,
    });
    assert!(!eligible(&changed));
    let mut changed = scope.clone();
    changed
        .windows
        .push(mapping::SequenceWindow::First { count: 99 });
    assert!(!eligible(&changed));
    let mut changed = scope.clone();
    changed.group_by = Some(99);
    assert!(!eligible(&changed));
    let mut changed = scope.clone();
    changed.post_group_filter = Some(99);
    assert!(!eligible(&changed));
    let mut changed = scope.clone();
    changed.construction = mapping::ScopeConstruction::CopyCurrentSource;
    assert!(!eligible(&changed));
    let mut changed = scope.clone();
    changed.iteration = ScopeIteration::None;
    assert!(!eligible(&changed));
    let mut changed = scope;
    changed.iteration = ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        },
        output_path: Some(99),
    };
    assert!(!eligible(&changed));
}

#[test]
fn public_first_retention_keeps_null_raw_ordinals_nested_pins_and_identity() {
    let mut p = value_position_project(SourceOwner::Primary, &["Rows"]);
    p.extra_sources.clear();
    p.root.iteration_output = mapping::IterationOutput::First;
    p.graph.nodes.extend([
        (
            3,
            Node::OwnedSourceField(field(
                SourceOwner::Primary,
                &["Rows", "Rows", "Value"],
                Some(&["Rows", "Rows"]),
            )),
        ),
        (
            4,
            Node::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection: Some(path(&["Rows", "Rows"])),
            }),
        ),
    ]);
    p.root.children = vec![
        Scope {
            target_field: "Selected".into(),
            iteration_output: mapping::IterationOutput::First,
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    node: 3,
                },
                Binding {
                    target_field: "Position".into(),
                    node: 4,
                },
                Binding {
                    target_field: "Outer".into(),
                    node: 2,
                },
            ],
            ..owned_scope(SourceOwner::Primary, &["Rows", "Rows"])
        },
        Scope {
            target_field: "Identity".into(),
            iteration_output: mapping::IterationOutput::First,
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Position".into(),
                    node: 2,
                },
            ],
            ..owned_scope(SourceOwner::Primary, &["Rows"])
        },
    ];
    p.target = SchemaNode::group(
        "Output",
        vec![
            scalar("Value"),
            SchemaNode::scalar("Position", ScalarType::Int),
            SchemaNode::group(
                "Selected",
                vec![
                    scalar("Value"),
                    SchemaNode::scalar("Position", ScalarType::Int),
                    SchemaNode::scalar("Outer", ScalarType::Int),
                ],
            ),
            SchemaNode::group(
                "Identity",
                vec![
                    scalar("Value"),
                    SchemaNode::scalar("Position", ScalarType::Int),
                ],
            ),
        ],
    );
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            Instance::Scalar(Value::Null),
            group(vec![
                ("Value", text("parent")),
                (
                    "Rows",
                    Instance::Repeated(vec![
                        Instance::Scalar(Value::json_null()),
                        row("child"),
                        row("unused"),
                    ]),
                ),
            ]),
            group(vec![
                ("Value", text("unused-parent")),
                ("Rows", Instance::DocumentSet(vec![])),
            ]),
        ]),
    )]);
    assert_eq!(
        crate::run(&p, &input),
        Ok(group(vec![
            ("Value", text("parent")),
            ("Position", Instance::Scalar(Value::Int(1))),
            (
                "Selected",
                group(vec![
                    ("Value", text("child")),
                    ("Position", Instance::Scalar(Value::Int(1))),
                    ("Outer", Instance::Scalar(Value::Int(1)))
                ])
            ),
            (
                "Identity",
                group(vec![
                    ("Value", text("parent")),
                    ("Position", Instance::Scalar(Value::Int(1)))
                ])
            ),
        ]))
    );
    let empty = group(vec![(
        "Rows",
        Instance::Repeated(vec![Instance::Scalar(Value::Null)]),
    )]);
    assert_eq!(crate::run(&p, &empty), Ok(group(vec![])));
}

#[test]
fn first_retention_bounds_the_retained_vector_without_losing_large_walk_count() {
    let mut p = project();
    p.extra_sources.clear();
    let input = group(vec![(
        "Rows",
        Instance::Repeated((0..1_000).map(|_| row("same")).collect()),
    )]);
    let empty = group(vec![]);
    let roots = SourceRoots::new(&p, &input, &empty, OwnedLimits::default()).unwrap();
    let context = OwnedContext::new(&roots);
    let source = OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(&["Rows"]),
    };
    let (selected, budget) = first_selection(&context, &source, true).unwrap();
    assert_eq!(selected.candidates, 1_000);
    assert_eq!(selected.extensions.len(), 1);
    assert_eq!(budget.usage().2, 1_000);
    assert_eq!(selected.extensions[0].extension.positions[0].index, 1);
}

#[test]
fn first_retention_preserves_late_domain_priority_over_noop_filter_budget() {
    let mut p = project();
    p.extra_sources.clear();
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            group(vec![("Rows", Instance::Repeated(vec![row("first")]))]),
            group(vec![("Rows", Instance::DocumentSet(vec![]))]),
        ]),
    )]);
    let empty = group(vec![]);
    // Traversal reaches the later document at exactly 120 charged UTF-8 bytes.
    // Premature no-op filter copies of the first row would exceed this budget.
    let limits = OwnedLimits {
        metadata_bytes: 120,
        ..OwnedLimits::default()
    };
    let roots = SourceRoots::new(&p, &input, &empty, limits).unwrap();
    let context = OwnedContext::new(&roots);
    let source = OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(&["Rows", "Rows"]),
    };
    let expected = Access::UnsupportedSourceDomain {
        found: "a document set",
    };
    assert_eq!(
        first_selection(&context, &source, false).err(),
        Some(expected.clone())
    );
    assert_eq!(
        first_selection(&context, &source, true).err(),
        Some(expected)
    );
}

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
use std::cell::Cell;
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
    p.root.group_by = Some(1);
    assert!(matches!(
        crate::run(&p, &input),
        Err(EngineError::UnsupportedOwnedContext {
            context: Domain::Grouping,
            scope: Some(_),
            ..
        })
    ));
    p.root.group_by = None;
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
    assert!(matches!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input,
            vec![],
            &execution,
            TargetSelection::Named("Other")
        ),
        Err(EngineError::UnsupportedOwnedContext {
            context: Domain::Tracing,
            ..
        })
    ));
    assert_eq!(sink.0.get(), 0);
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
    assert!(matches!(
        crate::run_selected_target_with_sources_and_context(
            &p,
            &input,
            vec![],
            &execution,
            TargetSelection::Named("Other")
        ),
        Err(EngineError::UnsupportedOwnedContext {
            context: Domain::Debugging,
            ..
        })
    ));
    assert_eq!(hook.0.get(), 0);
    let admission =
        crate::source_ownership::admit_project(&p, Some(TargetSelection::Primary)).unwrap();
    assert!(!admission.used_owned);
    let admission =
        crate::source_ownership::admit_project(&p, Some(TargetSelection::Named("Other"))).unwrap();
    assert!(admission.used_owned);
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
    let input = group(vec![("Value", text("root"))]);
    let error = crate::run(&p, &input).expect_err("a root pin requires a real repeated occurrence");
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

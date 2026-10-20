//! Hand-derived native By controls; expected trees do not come from an evaluator.
use std::collections::BTreeMap;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, IterationOutput, NamedSource, Node, OwnedPosition, OwnedSourceField,
    OwnedSourceRef, Project, Scope, ScopeIteration, SequenceWindow, SortFilterOrder, SourceOwner,
};

use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}
fn scalar(value: impl Into<Value>) -> Instance {
    Instance::Scalar(value.into())
}
fn text(value: &str) -> Instance {
    scalar(Value::String(value.into()))
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
fn binding(name: &str, node: mapping::NodeId) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}
fn field(owner: SourceOwner, parts: &[&str], pin: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: pin.map(path),
    })
}
fn position(owner: SourceOwner, parts: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner,
        collection: Some(path(parts)),
    })
}
fn owned(owner: SourceOwner, parts: &[&str]) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        output_path: None,
    }
}
fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Marker", ScalarType::String),
            SchemaNode::scalar("Limit", ScalarType::Int),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Label", ScalarType::String),
                    SchemaNode::scalar("Keep", ScalarType::Bool),
                    SchemaNode::scalar("Rank", ScalarType::Int),
                    SchemaNode::scalar("Limit", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    )
}
fn output_schema() -> SchemaNode {
    SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("First", ScalarType::String),
            SchemaNode::scalar("GroupPosition", ScalarType::Int),
            SchemaNode::scalar("RootMarker", ScalarType::String),
            SchemaNode::group(
                "Members",
                vec![
                    SchemaNode::scalar("Label", ScalarType::String),
                    SchemaNode::scalar("MemberPosition", ScalarType::Int),
                    SchemaNode::scalar("RootMarker", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
    .repeating()
}
fn batch_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Batch",
                vec![
                    SchemaNode::scalar("Id", ScalarType::String),
                    SchemaNode::scalar("Limit", ScalarType::Int),
                    source_schema().child("Rows").unwrap().clone(),
                ],
            )
            .repeating(),
        ],
    )
}
fn project(owner: SourceOwner) -> Project {
    let nodes = BTreeMap::from([
        (1, field(owner.clone(), &["Rows", "Key"], None)),
        (2, field(owner.clone(), &["Rows", "Label"], Some(&["Rows"]))),
        (3, position(owner.clone(), &["Rows"])),
        (4, field(owner.clone(), &["Marker"], Some(&[]))),
        (5, field(owner.clone(), &["Rows", "Keep"], None)),
        (6, field(owner.clone(), &["Rows", "Rank"], None)),
        (7, field(owner.clone(), &["Limit"], Some(&[]))),
        (
            8,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            9,
            Node::Const {
                value: Value::Int(3),
            },
        ),
    ]);
    Project {
        source: source_schema(),
        target: output_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Rows", " Røws💠 "]
            .into_iter()
            .map(|name| NamedSource {
                name: name.into(),
                path: format!("{name}.json"),
                schema: source_schema(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph { nodes },
        root: Scope {
            iteration: owned(owner.clone(), &["Rows"]),
            group_by: Some(1),
            bindings: vec![
                binding("First", 2),
                binding("GroupPosition", 3),
                binding("RootMarker", 4),
            ],
            children: vec![Scope {
                target_field: "Members".into(),
                iteration: owned(owner, &["Rows"]),
                bindings: vec![
                    binding("Label", 2),
                    binding("MemberPosition", 3),
                    binding("RootMarker", 4),
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}
fn input(prefix: &str) -> Instance {
    let rows = [
        ("A", "a1", false, 30),
        ("B", "b1", true, 10),
        ("A", "a2", true, 20),
        ("C", "c1", false, 40),
        ("B", "b2", false, 15),
    ];
    group(vec![
        ("Marker", text(&format!("{prefix}.root"))),
        ("Limit", scalar(Value::Int(1))),
        (
            "Rows",
            Instance::Repeated(
                rows.into_iter()
                    .map(|(key, label, keep, rank)| {
                        group(vec![
                            ("Key", text(key)),
                            ("Label", text(&format!("{prefix}-{label}"))),
                            ("Keep", scalar(Value::Bool(keep))),
                            ("Rank", scalar(Value::Int(rank))),
                            ("Limit", scalar(Value::Int(0))),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}
fn expected(prefix: &str, groups: &[(&str, &[&str])]) -> Instance {
    Instance::Repeated(
        groups
            .iter()
            .enumerate()
            .map(|(index, (first, members))| {
                group(vec![
                    ("First", text(&format!("{prefix}-{first}"))),
                    ("GroupPosition", scalar(Value::Int(index as i64 + 1))),
                    ("RootMarker", text(&format!("{prefix}.root"))),
                    (
                        "Members",
                        Instance::Repeated(
                            members
                                .iter()
                                .enumerate()
                                .map(|(index, label)| {
                                    group(vec![
                                        ("Label", text(&format!("{prefix}-{label}"))),
                                        ("MemberPosition", scalar(Value::Int(index as i64 + 1))),
                                        ("RootMarker", text(&format!("{prefix}.root"))),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    )
}
fn run(p: &Project) -> Result<Instance, EngineError> {
    crate::run_with_sources(
        p,
        &input("P"),
        vec![("Rows".into(), input("N")), (" Røws💠 ".into(), input("U"))],
    )
}

#[test]
fn exact_owner_representative_complete_members_and_positions() {
    for (owner, prefix) in [
        (SourceOwner::Primary, "P"),
        (named("Rows"), "N"),
        (named(" Røws💠 "), "U"),
    ] {
        let p = project(owner);
        assert_eq!(
            run(&p),
            Ok(expected(
                prefix,
                &[
                    ("a1", &["a1", "a2"]),
                    ("b1", &["b1", "b2"]),
                    ("c1", &["c1"])
                ]
            ))
        );
    }
}

#[test]
fn retention_is_any_member_not_a_member_filter_and_zero_groups_stay_empty() {
    let mut p = project(named("Rows"));
    p.root.post_group_filter = Some(5);
    assert_eq!(
        run(&p),
        Ok(expected(
            "N",
            &[("a1", &["a1", "a2"]), ("b1", &["b1", "b2"])]
        ))
    );
    p.graph.nodes.insert(
        5,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    assert_eq!(run(&p), Ok(Instance::Repeated(vec![])));
    let empty = group(vec![
        ("Marker", text("empty")),
        ("Rows", Instance::Repeated(vec![])),
    ]);
    assert_eq!(
        crate::run_with_sources(&p, &input("P"), vec![("Rows".into(), empty)]),
        Ok(Instance::Repeated(vec![]))
    );
}

#[test]
fn legacy_empty_path_members_keep_canonical_collection_and_raw_prefix() {
    for (owner, prefix, legacy) in [
        (SourceOwner::Primary, "P", true),
        (named("Rows"), "N", false),
    ] {
        let mut p = project(owner);
        if legacy {
            p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
        }
        p.root.children[0].iteration = ScopeIteration::Source(vec![]);
        assert_eq!(
            run(&p),
            Ok(expected(
                prefix,
                &[
                    ("a1", &["a1", "a2"]),
                    ("b1", &["b1", "b2"]),
                    ("c1", &["c1"])
                ]
            ))
        );
    }
}

#[test]
fn sort_ranks_feed_post_then_windows_compact_group_ordinals() {
    let mut p = project(SourceOwner::Primary);
    p.root.sort_by = Some(6);
    p.graph.nodes.insert(
        10,
        Node::Call {
            function: "greater_than".into(),
            args: vec![3, 9],
        },
    );
    p.root.post_group_filter = Some(10);
    assert_eq!(
        run(&p),
        Ok(expected("P", &[("a2", &["a2", "a1"]), ("c1", &["c1"])]))
    );
    p.root.windows.push(SequenceWindow::Last { count: 7 });
    assert_eq!(run(&p), Ok(expected("P", &[("c1", &["c1"])])));
}

#[test]
fn filter_then_sort_and_sort_then_filter_are_distinct_before_grouping() {
    let mut p = project(SourceOwner::Primary);
    p.root.sort_by = Some(6);
    p.graph.nodes.insert(
        10,
        Node::Call {
            function: "greater_than".into(),
            args: vec![3, 8],
        },
    );
    p.root.filter = Some(10);
    p.root.post_group_filter = Some(5);
    p.root.sort_filter_order = SortFilterOrder::FilterThenSort;
    assert_eq!(
        run(&p),
        Ok(expected("P", &[("b1", &["b1", "b2"]), ("a2", &["a2"])]))
    );
    p.root.sort_filter_order = SortFilterOrder::SortThenFilter;
    assert_eq!(run(&p), Ok(expected("P", &[("a2", &["a2", "a1"])])));
}

#[test]
fn same_owner_regroup_enumerates_members_and_preserves_outer_ordinal() {
    let mut p = project(SourceOwner::Primary);
    p.root.children[0].group_by = Some(1);
    // Grouping members by their original key leaves one inner group per outer
    // group, whose position is 1 even when the enclosing group position is 2/3.
    let expected = Instance::Repeated(
        [("a1", 1), ("b1", 2), ("c1", 3)]
            .into_iter()
            .map(|(label, ordinal)| {
                group(vec![
                    ("First", text(&format!("P-{label}"))),
                    ("GroupPosition", scalar(Value::Int(ordinal))),
                    ("RootMarker", text("P.root")),
                    (
                        "Members",
                        Instance::Repeated(vec![group(vec![
                            ("Label", text(&format!("P-{label}"))),
                            ("MemberPosition", scalar(Value::Int(1))),
                            ("RootMarker", text("P.root")),
                        ])]),
                    ),
                ])
            })
            .collect(),
    );
    assert_eq!(run(&p), Ok(expected));
}

#[test]
fn grouping_an_already_active_item_introduces_a_new_group_cursor() {
    let mut p = project(SourceOwner::Primary);
    p.root.group_by = None;
    p.root.children[0].group_by = Some(1);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[
                ("a1", &["a1"]),
                ("b1", &["b1"]),
                ("a2", &["a2"]),
                ("c1", &["c1"]),
                ("b2", &["b2"]),
            ]
        ))
    );
}

#[test]
fn singleton_root_gets_group_and_member_positions_but_pin_remains_immutable_root() {
    let mut p = project(SourceOwner::Primary);
    p.root.iteration = owned(SourceOwner::Primary, &[]);
    p.root.children[0].iteration = owned(SourceOwner::Primary, &[]);
    p.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Int(0),
        },
    );
    p.graph
        .nodes
        .insert(2, field(SourceOwner::Primary, &["Marker"], None));
    p.graph.nodes.insert(3, position(SourceOwner::Primary, &[]));
    let expected = Instance::Repeated(vec![group(vec![
        ("First", text("P.root")),
        ("GroupPosition", scalar(Value::Int(1))),
        ("RootMarker", text("P.root")),
        (
            "Members",
            Instance::Repeated(vec![group(vec![
                ("Label", text("P.root")),
                ("MemberPosition", scalar(Value::Int(1))),
                ("RootMarker", text("P.root")),
            ])]),
        ),
    ])]);
    assert_eq!(run(&p), Ok(expected));
}

#[test]
fn tagged_scalar_keys_and_anonymous_root_members_use_exact_equality() {
    let mut p = project(SourceOwner::Primary);
    p.source = SchemaNode::scalar("Input", ScalarType::String).repeating();
    p.root.iteration = owned(SourceOwner::Primary, &[]);
    p.root.children.clear();
    p.root.bindings = vec![binding("First", 1), binding("GroupPosition", 3)];
    p.graph
        .nodes
        .insert(1, field(SourceOwner::Primary, &[], None));
    p.graph.nodes.insert(3, position(SourceOwner::Primary, &[]));
    let input = Instance::Repeated(vec![
        scalar(Value::Int(1)),
        text("1"),
        scalar(Value::Float(1.0)),
        scalar(Value::Int(1)),
        scalar(Value::Float(0.0)),
        scalar(Value::Float(-0.0)),
    ]);
    let expected = Instance::Repeated(
        [
            Value::Int(1),
            Value::String("1".into()),
            Value::Float(1.0),
            Value::Float(0.0),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            group(vec![
                ("First", scalar(value)),
                ("GroupPosition", scalar(Value::Int(index as i64 + 1))),
            ])
        })
        .collect(),
    );
    // Do not adapt the representative value through a String target field.
    p.target = SchemaNode::group("Output", vec![]).repeating();
    assert_eq!(crate::run(&p, &input), Ok(expected));
}

#[test]
fn key_and_post_only_owned_demands_on_legacy_source_are_discovered() {
    for key_owned in [false, true] {
        let mut p = project(SourceOwner::Primary);
        p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
        p.root.children.clear();
        p.root.bindings = vec![binding("First", 20)];
        p.graph.nodes.insert(
            20,
            Node::SourceField {
                path: path(&["Label"]),
                frame: None,
            },
        );
        if !key_owned {
            p.graph.nodes.insert(
                1,
                Node::SourceField {
                    path: path(&["Key"]),
                    frame: None,
                },
            );
            p.root.post_group_filter = Some(5);
        }
        let labels: &[&str] = if key_owned {
            &["P-a1", "P-b1", "P-c1"]
        } else {
            &["P-a1", "P-b1"]
        };
        assert_eq!(
            run(&p),
            Ok(Instance::Repeated(
                labels
                    .iter()
                    .map(|label| group(vec![("First", text(label))]))
                    .collect()
            ))
        );
    }
}

#[test]
fn uniterated_descendant_repetition_is_not_representative_permission() {
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.insert(
        2,
        field(SourceOwner::Primary, &["Rows", "Nested", "Value"], None),
    );
    let row = group(vec![
        ("Key", text("A")),
        (
            "Nested",
            Instance::Repeated(vec![group(vec![("Value", text("no"))])]),
        ),
    ]);
    let input = group(vec![("Rows", Instance::Repeated(vec![row]))]);
    assert!(
        matches!(crate::run(&p, &input), Err(EngineError::OwnedSourceAccess {
        node: Some(2), source: OwnedSourceError::UniteratedOwnedRepetition { path: actual }, ..
    }) if actual == path(&["Nested", "Value"]))
    );
}

#[test]
fn neighboring_grouping_modes_and_specialized_consumers_stay_closed() {
    for mode in 0..4 {
        let mut p = project(SourceOwner::Primary);
        p.root.group_by = None;
        match mode {
            0 => p.root.group_adjacent_by = Some(1),
            1 => p.root.group_starting_with = Some(5),
            2 => p.root.group_ending_with = Some(5),
            _ => {
                p.graph.nodes.insert(
                    8,
                    Node::Const {
                        value: Value::Int(2),
                    },
                );
                p.root.group_into_blocks = Some(8);
            }
        }
        if mode == 0 {
            // #442 adds this one neighboring family; the original fixture is
            // retained as a literal adjacent control, not a new By baseline.
            assert_eq!(
                run(&p),
                Ok(expected(
                    "P",
                    &[
                        ("a1", &["a1"]),
                        ("b1", &["b1"]),
                        ("a2", &["a2"]),
                        ("c1", &["c1"]),
                        ("b2", &["b2"])
                    ]
                ))
            );
            continue;
        }
        if mode == 1 {
            assert_eq!(
                run(&p),
                Ok(expected(
                    "P",
                    &[
                        ("a1", &["a1"]),
                        ("b1", &["b1"]),
                        ("a2", &["a2", "c1", "b2"])
                    ]
                ))
            );
            continue;
        }
        if mode == 2 {
            // #444: each true boundary closes its group; the false suffix trails.
            assert_eq!(
                run(&p),
                Ok(expected(
                    "P",
                    &[
                        ("a1", &["a1", "b1"]),
                        ("a2", &["a2"]),
                        ("c1", &["c1", "b2"])
                    ]
                ))
            );
            continue;
        }
        // #445: fixed blocks of two leave a trailing partial block.
        assert_eq!(
            run(&p),
            Ok(expected(
                "P",
                &[
                    ("a1", &["a1", "b1"]),
                    ("a2", &["a2", "c1"]),
                    ("b2", &["b2"])
                ]
            ))
        );
    }
    let mut p = project(SourceOwner::Primary);
    p.root.group_by = None;
    p.root.post_group_filter = Some(5);
    assert!(matches!(
        run(&p),
        Err(EngineError::UnsupportedOwnedContext {
            context: OwnedCapabilityContext::Grouping,
            ..
        })
    ));
    for legacy_source in [false, true] {
        let mut p = project(SourceOwner::Primary);
        if legacy_source {
            p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
        }
        p.graph.nodes.insert(
            2,
            Node::Lookup {
                collection: path(&["Rows"]),
                key: path(&["Key"]),
                matches: 1,
                value: path(&["Label"]),
            },
        );
        assert!(matches!(
            run(&p),
            Err(EngineError::UnsupportedOwnedContext {
                context: OwnedCapabilityContext::AggregateLookup,
                node: Some(2),
                ..
            })
        ));
    }
}

#[test]
fn closed_legacy_by_scopes_preserve_owned_node_refusal_provenance() {
    for (case, domain) in [
        (0, OwnedCapabilityContext::Grouping),
        (1, OwnedCapabilityContext::Grouping),
        (2, OwnedCapabilityContext::SpecialConstruction),
        (3, OwnedCapabilityContext::DynamicTarget),
        (4, OwnedCapabilityContext::XmlOperator),
        (5, OwnedCapabilityContext::Document),
    ] {
        let mut p = project(SourceOwner::Primary);
        p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
        p.root.children.clear();
        p.root.bindings = vec![binding("First", 2)];
        p.graph.nodes.insert(
            1,
            Node::Const {
                value: Value::String("closed".into()),
            },
        );
        match case {
            0 => p.root.iteration = ScopeIteration::None,
            1 => p.root.group_adjacent_by = Some(1),
            2 => p.root.construction = mapping::ScopeConstruction::CopyCurrentSource,
            3 => p.root.merge_dynamic_fields = true,
            4 => p.root.iteration_output = IterationOutput::MappedSequence,
            5 => assert!(p.root.set_output_path(Some(1))),
            _ => unreachable!("fixed closed-domain fixture"),
        }
        assert_eq!(crate::source_ownership::scope_context(&p.root), domain);
        assert_eq!(
            run(&p),
            Err(EngineError::UnsupportedOwnedContext {
                node: Some(2),
                function: None,
                scope: None,
                context: domain,
            })
        );
    }
}

#[test]
fn saved_project_roundtrip_adds_no_new_group_wire_form() {
    let p = project(SourceOwner::Primary);
    let bytes = serde_json::to_vec(&p).unwrap();
    let restored: Project = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
    assert_eq!(
        run(&restored),
        Ok(expected(
            "P",
            &[
                ("a1", &["a1", "a2"]),
                ("b1", &["b1", "b2"]),
                ("c1", &["c1"])
            ]
        ))
    );
}

#[test]
fn grouped_owned_witness_is_never_mislabeled_basic_even_for_anonymous_root() {
    struct Sink(std::cell::Cell<usize>);
    impl crate::TraceSink for Sink {
        fn record(&self, _: crate::TraceEvent) {}
        fn wants_owned_context(&self) -> bool {
            true
        }
        fn wants_owned_context_for(&self, _: &crate::TraceEvent) -> bool {
            true
        }
        fn record_with_owned_context(
            &self,
            _: crate::TraceEvent,
            witness: Result<crate::TraceOwnedWitness<'_>, &crate::TraceOwnedContextError>,
        ) {
            if let Ok(witness) = witness
                && witness.domain == crate::TraceOwnedDomain::Grouped
            {
                self.0.set(self.0.get() + 1);
                assert!(matches!(
                    witness.snapshot(),
                    Err(crate::TraceOwnedWitnessError::UnsupportedDomain {
                        domain: "grouped source",
                        ..
                    })
                ));
            }
        }
    }
    let sink = Sink(std::cell::Cell::new(0));
    let p = project(SourceOwner::Primary);
    let execution = crate::ExecutionContext::new(Path::new("group.json")).with_trace_sink(&sink);
    crate::run_with_sources_and_context(&p, &input("P"), vec![], &execution).unwrap();
    assert!(sink.0.get() > 0);
    sink.0.set(0);
    let mut p = p;
    p.root.iteration = owned(SourceOwner::Primary, &[]);
    p.root.children.clear();
    p.root.bindings.clear();
    p.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Int(0),
        },
    );
    crate::run_with_sources_and_context(&p, &input("P"), vec![], &execution).unwrap();
    assert!(sink.0.get() > 0);
}

fn raises(p: &mut Project, node: mapping::NodeId, message: &str) {
    p.graph.nodes.insert(
        node + 100,
        Node::Const {
            value: Value::String(message.into()),
        },
    );
    p.graph.nodes.insert(
        node,
        Node::Raise {
            message: Some(node + 100),
        },
    );
}
fn exception(node: mapping::NodeId, message: &str) -> EngineError {
    EngineError::MappingException {
        node,
        message: Some(message.into()),
    }
}

#[test]
fn keys_post_and_later_filters_have_the_original_fused_error_order() {
    let mut p = project(SourceOwner::Primary);
    raises(&mut p, 30, "key");
    raises(&mut p, 31, "post");
    raises(&mut p, 32, "later filter");
    p.graph.nodes.insert(
        33,
        Node::Call {
            function: "greater_than".into(),
            args: vec![3, 8],
        },
    );
    p.graph.nodes.insert(
        34,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.graph.nodes.insert(
        35,
        Node::If {
            condition: 33,
            then: 32,
            else_: 34,
        },
    );
    p.root.filter = Some(35);
    p.root.group_by = Some(30);
    p.root.post_group_filter = Some(31);
    assert_eq!(run(&p), Err(exception(30, "key")));
    p.root.group_by = Some(1);
    assert_eq!(run(&p), Err(exception(31, "post")));
    p.root.sort_by = Some(6);
    p.root.sort_filter_order = SortFilterOrder::FilterThenSort;
    // Complete predicates precede every sort key, bound, key and post predicate.
    assert_eq!(run(&p), Err(exception(32, "later filter")));
}

#[test]
fn sort_errors_precede_bounds_and_bounds_precede_keys_even_with_no_output() {
    let mut p = project(SourceOwner::Primary);
    raises(&mut p, 30, "key");
    raises(&mut p, 31, "bound");
    raises(&mut p, 32, "sort");
    p.root.group_by = Some(30);
    p.root.windows = vec![SequenceWindow::First { count: 31 }];
    p.root.sort_by = Some(32);
    assert_eq!(run(&p), Err(exception(32, "sort")));
    p.root.sort_by = None;
    assert_eq!(run(&p), Err(exception(31, "bound")));
    p.graph.nodes.insert(
        31,
        Node::Const {
            value: Value::Int(0),
        },
    );
    assert_eq!(run(&p), Err(exception(30, "key")));
    p.root.iteration_output = IterationOutput::First;
    assert_eq!(run(&p), Err(exception(30, "key")));
}

#[test]
fn every_post_predicate_runs_even_after_the_same_bucket_is_already_retained() {
    let mut p = project(SourceOwner::Primary);
    raises(&mut p, 30, "third member");
    p.graph.nodes.insert(
        31,
        Node::Call {
            function: "equal".into(),
            args: vec![3, 9],
        },
    );
    p.graph.nodes.insert(
        32,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    p.graph.nodes.insert(
        33,
        Node::If {
            condition: 31,
            then: 30,
            else_: 32,
        },
    );
    p.root.post_group_filter = Some(33);
    assert_eq!(run(&p), Err(exception(30, "third member")));
    // A false source predicate skips both key and post, but bounds still run.
    p.graph.nodes.insert(
        34,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    p.root.filter = Some(34);
    assert_eq!(run(&p), Ok(Instance::Repeated(vec![])));
}

#[test]
fn typed_post_and_zero_candidate_bound_failures_keep_their_node() {
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.insert(
        30,
        Node::Const {
            value: Value::String("not bool".into()),
        },
    );
    p.root.post_group_filter = Some(30);
    assert_eq!(
        run(&p),
        Err(EngineError::NotABool {
            node: 30,
            found: "string"
        })
    );
    p.root.windows = vec![SequenceWindow::First { count: 30 }];
    let empty = group(vec![("Rows", Instance::Repeated(vec![]))]);
    assert_eq!(
        crate::run(&p, &empty),
        Err(EngineError::NotAnItemCount {
            node: 30,
            found: "string"
        })
    );
}

#[test]
fn dropped_and_window_excluded_group_targets_are_never_evaluated() {
    let mut p = project(SourceOwner::Primary);
    raises(&mut p, 30, "target");
    p.root.bindings[0].node = 30;
    p.root.windows = vec![SequenceWindow::First { count: 31 }];
    p.graph.nodes.insert(
        31,
        Node::Const {
            value: Value::Int(0),
        },
    );
    assert_eq!(run(&p), Ok(Instance::Repeated(vec![])));
    p.root.windows.clear();
    p.graph.nodes.insert(
        31,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    p.root.post_group_filter = Some(31);
    assert_eq!(run(&p), Ok(Instance::Repeated(vec![])));
}

#[test]
fn different_owner_nested_groups_do_not_replace_outer_owner_frames() {
    let mut p = project(SourceOwner::Primary);
    p.graph
        .nodes
        .insert(20, field(named("Rows"), &["Rows", "Key"], None));
    p.graph
        .nodes
        .insert(21, field(named("Rows"), &["Rows", "Label"], None));
    p.graph.nodes.insert(22, position(named("Rows"), &["Rows"]));
    let inner = &mut p.root.children[0];
    inner.iteration = owned(named("Rows"), &["Rows"]);
    inner.group_by = Some(20);
    inner.bindings = vec![
        binding("Label", 21),
        binding("MemberPosition", 22),
        binding("RootMarker", 2),
    ];
    let expected = Instance::Repeated(
        [("a1", 1), ("b1", 2), ("c1", 3)]
            .into_iter()
            .map(|(first, index)| {
                group(vec![
                    ("First", text(&format!("P-{first}"))),
                    ("GroupPosition", scalar(Value::Int(index))),
                    ("RootMarker", text("P.root")),
                    (
                        "Members",
                        Instance::Repeated(
                            [("a1", 1), ("b1", 2), ("c1", 3)]
                                .into_iter()
                                .map(|(label, ordinal)| {
                                    group(vec![
                                        ("Label", text(&format!("N-{label}"))),
                                        ("MemberPosition", scalar(Value::Int(ordinal))),
                                        ("RootMarker", text(&format!("P-{first}"))),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    );
    assert_eq!(run(&p), Ok(expected));
}

#[test]
fn flattened_multihop_groups_keep_first_members_intermediate_prefix() {
    let mut p = project(SourceOwner::Primary);
    p.source = batch_schema();
    p.root.iteration = owned(SourceOwner::Primary, &["Batch", "Rows"]);
    p.root.children[0].iteration = owned(SourceOwner::Primary, &["Batch", "Rows"]);
    for (id, leaf) in [(1, "Key"), (2, "Label")] {
        p.graph.nodes.insert(
            id,
            field(SourceOwner::Primary, &["Batch", "Rows", leaf], None),
        );
    }
    p.graph
        .nodes
        .insert(3, position(SourceOwner::Primary, &["Batch", "Rows"]));
    p.graph.nodes.insert(
        4,
        field(SourceOwner::Primary, &["Batch", "Id"], Some(&["Batch"])),
    );
    let row = |key: &str, label: &str| group(vec![("Key", text(key)), ("Label", text(label))]);
    let batch = |id: &str, rows| group(vec![("Id", text(id)), ("Rows", Instance::Repeated(rows))]);
    let input = group(vec![(
        "Batch",
        Instance::Repeated(vec![
            batch("first", vec![row("A", "a1"), row("B", "b1")]),
            batch("second", vec![row("A", "a2")]),
        ]),
    )]);
    let expected = Instance::Repeated(
        [("a1", &["a1", "a2"][..]), ("b1", &["b1"][..])]
            .into_iter()
            .enumerate()
            .map(|(index, (first, labels))| {
                group(vec![
                    ("First", text(first)),
                    ("GroupPosition", scalar(Value::Int(index as i64 + 1))),
                    ("RootMarker", text("first")),
                    (
                        "Members",
                        Instance::Repeated(
                            labels
                                .iter()
                                .enumerate()
                                .map(|(index, label)| {
                                    group(vec![
                                        ("Label", text(label)),
                                        ("MemberPosition", scalar(Value::Int(index as i64 + 1))),
                                        ("RootMarker", text("first")),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    );
    assert_eq!(crate::run(&p, &input), Ok(expected));
}

#[test]
fn parent_bounds_use_the_incoming_batch_not_candidate_row_values() {
    let mut p = project(SourceOwner::Primary);
    p.source = batch_schema();
    p.graph.nodes.insert(
        1,
        field(SourceOwner::Primary, &["Batch", "Rows", "Key"], None),
    );
    p.graph.nodes.insert(
        2,
        field(SourceOwner::Primary, &["Batch", "Rows", "Label"], None),
    );
    p.graph
        .nodes
        .insert(3, position(SourceOwner::Primary, &["Batch", "Rows"]));
    p.graph.nodes.insert(
        4,
        field(SourceOwner::Primary, &["Batch", "Id"], Some(&["Batch"])),
    );
    p.graph
        .nodes
        .insert(7, field(SourceOwner::Primary, &["Batch", "Limit"], None));
    p.root.iteration = owned(SourceOwner::Primary, &["Batch", "Rows"]);
    p.root.windows.push(SequenceWindow::First { count: 7 });
    p.root.children[0].iteration = owned(SourceOwner::Primary, &["Batch", "Rows"]);
    let grouped = p.root.clone();
    p.root = Scope {
        iteration: owned(SourceOwner::Primary, &["Batch"]),
        children: vec![Scope {
            target_field: "Groups".into(),
            ..grouped
        }],
        ..Scope::default()
    };
    let mut groups_schema = output_schema();
    groups_schema.name = "Groups".into();
    p.target = SchemaNode::group("Output", vec![groups_schema]).repeating();
    let row = |key: &str, label: &str| {
        group(vec![
            ("Key", text(key)),
            ("Label", text(label)),
            ("Limit", scalar(Value::Int(0))),
        ])
    };
    let batch = |id: &str, limit| {
        group(vec![
            ("Id", text(id)),
            ("Limit", scalar(Value::Int(limit))),
            (
                "Rows",
                Instance::Repeated(vec![row("A", "a"), row("B", "b")]),
            ),
        ])
    };
    let input = group(vec![(
        "Batch",
        Instance::Repeated(vec![batch("one", 1), batch("two", 2)]),
    )]);
    let expected = Instance::Repeated(
        [("one", &["a"][..]), ("two", &["a", "b"][..])]
            .into_iter()
            .map(|(id, labels)| {
                group(vec![(
                    "Groups",
                    Instance::Repeated(
                        labels
                            .iter()
                            .enumerate()
                            .map(|(index, label)| {
                                group(vec![
                                    ("First", text(label)),
                                    ("GroupPosition", scalar(Value::Int(index as i64 + 1))),
                                    ("RootMarker", text(id)),
                                    (
                                        "Members",
                                        Instance::Repeated(vec![group(vec![
                                            ("Label", text(label)),
                                            ("MemberPosition", scalar(Value::Int(1))),
                                            ("RootMarker", text(id)),
                                        ])]),
                                    ),
                                ])
                            })
                            .collect(),
                    ),
                )])
            })
            .collect(),
    );
    assert_eq!(crate::run(&p, &input), Ok(expected));
}

#[test]
fn exact_other_owner_pin_cannot_be_rescued_by_group_representative() {
    let mut p = project(SourceOwner::Primary);
    p.graph
        .nodes
        .insert(2, field(named("Rows"), &["Rows", "Label"], Some(&["Rows"])));
    assert_eq!(
        run(&p),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: OwnedSourceError::InactiveSourceFrame {
                owner: named("Rows"),
                collection: Some(path(&["Rows"]))
            },
        })
    );
    p.graph
        .nodes
        .insert(2, field(named("unknown"), &["Rows", "Label"], None));
    assert_eq!(
        run(&p),
        Err(EngineError::OwnedSourceAccess {
            node: Some(2),
            scope: None,
            source: OwnedSourceError::UnknownOwner {
                name: "unknown".into()
            },
        })
    );
}

#[test]
fn legacy_saved_group_project_roundtrip_keeps_original_dispatch_and_tree() {
    let mut p = project(SourceOwner::Primary);
    p.extra_sources.clear();
    p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
    p.root.children[0].iteration = ScopeIteration::Source(vec![]);
    p.graph.nodes = BTreeMap::from([
        (
            1,
            Node::SourceField {
                path: path(&["Key"]),
                frame: None,
            },
        ),
        (
            2,
            Node::SourceField {
                path: path(&["Label"]),
                frame: None,
            },
        ),
        (
            3,
            Node::Position {
                collection: path(&["Rows"]),
            },
        ),
        (
            4,
            Node::SourceField {
                path: path(&["Marker"]),
                frame: None,
            },
        ),
    ]);
    let bytes = serde_json::to_vec(&p).unwrap();
    let wire = String::from_utf8(bytes.clone()).unwrap();
    assert!(!wire.contains("owned_source"));
    let restored: Project = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
    assert_eq!(
        crate::run(&restored, &input("P")),
        Ok(expected(
            "P",
            &[
                ("a1", &["a1", "a2"]),
                ("b1", &["b1", "b2"]),
                ("c1", &["c1"])
            ]
        ))
    );
}

#[test]
fn primary_row_failure_permission_still_composes_with_grouped_output() {
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.insert(
        10,
        Node::Call {
            function: "greater_than".into(),
            args: vec![3, 11],
        },
    );
    p.graph.nodes.insert(
        11,
        Node::Const {
            value: Value::Int(0),
        },
    );
    p.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        selection: mapping::FailureSelection::WhenFalse { predicate: 10 },
        message: None,
    });
    assert_eq!(
        run(&p),
        Ok(expected(
            "P",
            &[
                ("a1", &["a1", "a2"]),
                ("b1", &["b1", "b2"]),
                ("c1", &["c1"])
            ]
        ))
    );
    p.graph.nodes.insert(
        11,
        Node::Const {
            value: Value::Int(1),
        },
    );
    assert_eq!(
        run(&p),
        Err(EngineError::MappingFailure {
            rule: 1,
            message: None
        })
    );
}

#[test]
fn trusted_empty_group_is_null_and_never_an_identity_or_another_owner_rescue() {
    use crate::owned_context::{
        OccurrenceKind, OwnedContext, OwnedLimits, OwnerKey, SourceFrame, SourceRoots,
    };
    let p = project(SourceOwner::Primary);
    let primary = input("P");
    let extras = group(vec![("Rows", input("N"))]);
    let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
    let empty = Instance::Repeated(vec![]);
    let context = OwnedContext {
        roots: &roots,
        frames: vec![SourceFrame {
            owner: OwnerKey::Primary,
            collection: path(&["Rows"]),
            item: &empty,
            position_slot: 0,
            kind: OccurrenceKind::GroupMembers,
        }],
        origins: vec![],
        owned_ancestry: true,
    };
    let Node::OwnedSourceField(field) = field(SourceOwner::Primary, &["Rows", "Label"], None)
    else {
        unreachable!()
    };
    assert_eq!(context.field(&field), Ok(Value::Null));
    assert!(!context.active_identity(OwnerKey::Primary, &path(&["Rows"]), &empty));
    let source = OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(&["Rows"]),
    };
    let (owner, base, prefix, tail, budget) = context.select(&source).unwrap();
    let members = crate::source_iteration::walk_observed(
        base,
        tail,
        prefix,
        prefix,
        Some(owner),
        &context,
        1,
        true,
        &budget,
    )
    .unwrap();
    assert!(members.is_empty());
}

#[test]
fn tagged_nulls_and_repeated_nans_do_not_share_buckets() {
    let mut p = project(SourceOwner::Primary);
    p.root.children.clear();
    p.root.bindings = vec![binding("First", 2), binding("GroupPosition", 3)];
    let row = |key, label: &str| group(vec![("Key", scalar(key)), ("Label", text(label))]);
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![
            row(Value::Null, "absent"),
            row(Value::JsonNull(ir::JsonNull), "json-null"),
            row(Value::Null, "same-absent"),
            row(Value::Float(f64::NAN), "nan-one"),
            row(Value::Float(f64::NAN), "nan-two"),
        ]),
    )]);
    let expected = Instance::Repeated(
        ["absent", "json-null", "nan-one", "nan-two"]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                group(vec![
                    ("First", text(label)),
                    ("GroupPosition", scalar(Value::Int(index as i64 + 1))),
                ])
            })
            .collect(),
    );
    assert_eq!(crate::run(&p, &input), Ok(expected));
}

#[test]
fn owned_null_pruning_preserves_raw_positions_before_post_and_group_compaction() {
    let mut p = project(SourceOwner::Primary);
    p.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::Int(2),
        },
    );
    p.graph.nodes.insert(
        10,
        Node::Call {
            function: "greater_than".into(),
            args: vec![3, 9],
        },
    );
    p.root.post_group_filter = Some(10);
    let row = |key: &str, label: &str| group(vec![("Key", text(key)), ("Label", text(label))]);
    let input = group(vec![
        ("Marker", text("P.root")),
        (
            "Rows",
            Instance::Repeated(vec![
                row("A", "P-a1"),
                scalar(Value::Null),
                row("B", "P-b1"),
                scalar(Value::JsonNull(ir::JsonNull)),
            ]),
        ),
    ]);
    assert_eq!(
        crate::run(&p, &input),
        Ok(expected("P", &[("b1", &["b1"])]))
    );
}

#[test]
fn legacy_named_root_alias_members_preserve_exact_group_bucket() {
    for iteration in [
        ScopeIteration::Source(path(&["Alias"])),
        ScopeIteration::Source(vec![]),
        owned(named("Alias"), &[]),
    ] {
        let mut p = project(named("Alias"));
        let mut root_schema = source_schema().child("Rows").unwrap().clone();
        root_schema.name = "Input".into();
        p.extra_sources = vec![NamedSource {
            name: "Alias".into(),
            path: "alias.json".into(),
            schema: root_schema,
            options: Default::default(),
            dynamic_path: None,
        }];
        p.root.iteration = ScopeIteration::Source(path(&["Alias"]));
        p.root.children[0].iteration = iteration;
        p.graph
            .nodes
            .insert(1, field(named("Alias"), &["Key"], None));
        p.graph
            .nodes
            .insert(2, field(named("Alias"), &["Label"], None));
        p.graph
            .nodes
            .insert(25, field(named("Alias"), &["Label"], Some(&[])));
        p.root.children[0].bindings[0].node = 25;
        p.graph.nodes.insert(3, position(named("Alias"), &[]));
        p.graph
            .nodes
            .insert(4, field(SourceOwner::Primary, &["Marker"], Some(&[])));
        let row = |key: &str, label: &str| {
            group(vec![
                ("Key", text(key)),
                ("Label", text(label)),
                ("Marker", text("not-the-primary-root")),
            ])
        };
        let supplied = Instance::Repeated(vec![row("A", "a1"), row("B", "b1"), row("A", "a2")]);
        let expected = Instance::Repeated(vec![
            group(vec![
                ("First", text("a1")),
                ("GroupPosition", scalar(Value::Int(1))),
                ("RootMarker", text("P.root")),
                (
                    "Members",
                    Instance::Repeated(vec![
                        group(vec![
                            ("Label", text("a1")),
                            ("MemberPosition", scalar(Value::Int(1))),
                            ("RootMarker", text("P.root")),
                        ]),
                        group(vec![
                            ("Label", text("a2")),
                            ("MemberPosition", scalar(Value::Int(2))),
                            ("RootMarker", text("P.root")),
                        ]),
                    ]),
                ),
            ]),
            group(vec![
                ("First", text("b1")),
                ("GroupPosition", scalar(Value::Int(2))),
                ("RootMarker", text("P.root")),
                (
                    "Members",
                    Instance::Repeated(vec![group(vec![
                        ("Label", text("b1")),
                        ("MemberPosition", scalar(Value::Int(1))),
                        ("RootMarker", text("P.root")),
                    ])]),
                ),
            ]),
        ]);
        assert_eq!(
            crate::run_with_sources(&p, &input("P"), vec![("Alias".into(), supplied)]),
            Ok(expected)
        );
    }
}

#[test]
fn legacy_named_alias_multihop_members_keep_representative_prefix_and_root_pins() {
    for iteration in [
        ScopeIteration::Source(path(&["Rows"])),
        ScopeIteration::Source(vec![]),
        owned(named("Alias"), &["Batch", "Rows"]),
    ] {
        let mut p = project(named("Alias"));
        let named_schema = SchemaNode::group(
            "Input",
            vec![
                SchemaNode::scalar("Marker", ScalarType::String),
                batch_schema().child("Batch").unwrap().clone(),
            ],
        );
        p.extra_sources = vec![NamedSource {
            name: "Alias".into(),
            path: "alias.json".into(),
            schema: named_schema,
            options: Default::default(),
            dynamic_path: None,
        }];
        p.root.iteration = ScopeIteration::Source(path(&["Alias", "Batch", "Rows"]));
        p.root.children[0].iteration = iteration;
        p.graph
            .nodes
            .insert(1, field(named("Alias"), &["Batch", "Rows", "Key"], None));
        p.graph.nodes.insert(
            2,
            field(
                named("Alias"),
                &["Batch", "Rows", "Label"],
                Some(&["Batch", "Rows"]),
            ),
        );
        p.graph
            .nodes
            .insert(3, position(named("Alias"), &["Batch", "Rows"]));
        p.graph
            .nodes
            .insert(4, field(named("Alias"), &["Batch", "Id"], Some(&["Batch"])));
        p.graph
            .nodes
            .insert(24, field(named("Alias"), &["Marker"], Some(&[])));
        p.root.bindings.push(binding("RootPin", 24));
        p.root.children[0].bindings.push(binding("RootPin", 24));
        p.target = SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("First", ScalarType::String),
                SchemaNode::scalar("GroupPosition", ScalarType::Int),
                SchemaNode::scalar("RootMarker", ScalarType::String),
                SchemaNode::scalar("RootPin", ScalarType::String),
                SchemaNode::group(
                    "Members",
                    vec![
                        SchemaNode::scalar("Label", ScalarType::String),
                        SchemaNode::scalar("MemberPosition", ScalarType::Int),
                        SchemaNode::scalar("RootMarker", ScalarType::String),
                        SchemaNode::scalar("RootPin", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        )
        .repeating();
        let row = |key: &str, label: &str| {
            group(vec![
                ("Key", text(key)),
                ("Label", text(label)),
                ("Marker", text("not-the-named-root")),
            ])
        };
        let batch =
            |id: &str, rows| group(vec![("Id", text(id)), ("Rows", Instance::Repeated(rows))]);
        let supplied = group(vec![
            ("Marker", text("N.root")),
            (
                "Batch",
                Instance::Repeated(vec![
                    batch("first-batch", vec![row("A", "a1"), row("B", "b1")]),
                    batch("second-batch", vec![row("A", "a2")]),
                ]),
            ),
        ]);
        let expected = Instance::Repeated(
            [("a1", &["a1", "a2"][..]), ("b1", &["b1"][..])]
                .into_iter()
                .enumerate()
                .map(|(index, (first, labels))| {
                    group(vec![
                        ("First", text(first)),
                        ("GroupPosition", scalar(Value::Int(index as i64 + 1))),
                        ("RootMarker", text("first-batch")),
                        ("RootPin", text("N.root")),
                        (
                            "Members",
                            Instance::Repeated(
                                labels
                                    .iter()
                                    .enumerate()
                                    .map(|(index, label)| {
                                        group(vec![
                                            ("Label", text(label)),
                                            (
                                                "MemberPosition",
                                                scalar(Value::Int(index as i64 + 1)),
                                            ),
                                            ("RootMarker", text("first-batch")),
                                            ("RootPin", text("N.root")),
                                        ])
                                    })
                                    .collect(),
                            ),
                        ),
                    ])
                })
                .collect(),
        );
        assert_eq!(
            crate::run_with_sources(&p, &input("P"), vec![("Alias".into(), supplied)]),
            Ok(expected)
        );
    }
}

fn repeated_root_by_fixture(
    owner: SourceOwner,
    legacy_alias: bool,
    declared_repeating: bool,
) -> (Project, Instance, Vec<(String, Instance)>) {
    let mut p = project(owner.clone());
    let mut schema = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar("Label", ScalarType::String),
        ],
    );
    schema.repeating = declared_repeating;
    let row = |key: &str, label: &str| group(vec![("Key", text(key)), ("Label", text(label))]);
    let rows = Instance::Repeated(vec![row("A", "a1"), row("B", "b1"), row("A", "a2")]);
    let (primary, supplied) = match &owner {
        SourceOwner::Primary => {
            p.source = schema;
            p.extra_sources.clear();
            (rows, vec![])
        }
        SourceOwner::Named { name } => {
            p.extra_sources = vec![NamedSource {
                name: name.clone(),
                path: "root.json".into(),
                schema,
                options: Default::default(),
                dynamic_path: None,
            }];
            (
                group(vec![("Marker", text("P.root"))]),
                vec![(name.clone(), rows)],
            )
        }
    };
    p.graph.nodes = BTreeMap::from([
        (1, field(owner.clone(), &["Key"], None)),
        (2, field(owner.clone(), &["Label"], None)),
        (3, position(owner.clone(), &[])),
        (4, field(owner.clone(), &["Label"], Some(&[]))),
        (41, field(owner.clone(), &["Key"], Some(&[]))),
    ]);
    p.root.iteration = if legacy_alias {
        let SourceOwner::Named { name } = &owner else {
            panic!("legacy alias fixture requires a named owner")
        };
        ScopeIteration::Source(vec![name.clone()])
    } else {
        owned(owner.clone(), &[])
    };
    p.root.bindings = vec![binding("First", 2), binding("GroupPosition", 3)];
    p.root.children[0].iteration = owned(owner, &[]);
    p.root.children[0].bindings = vec![binding("Label", 4), binding("MemberPosition", 3)];
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("First", ScalarType::String),
            SchemaNode::scalar("GroupPosition", ScalarType::Int),
            SchemaNode::group(
                "Members",
                vec![
                    SchemaNode::scalar("Label", ScalarType::String),
                    SchemaNode::scalar("MemberPosition", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    )
    .repeating();
    (p, primary, supplied)
}

#[test]
fn by_group_empty_root_pin_rejects_repeated_root_at_the_pinned_node() {
    for (owner, legacy_alias) in [
        (named("Alias"), true),
        (SourceOwner::Primary, false),
        (named("Alias"), false),
    ] {
        for declared_repeating in [true, false] {
            // Both are valid mapping roots ([] also admits a singleton Group).
            // The supplied repeated payload must not become a representative
            // through an explicit root pin, regardless of the declaration flag.
            let (mut p, primary, supplied) =
                repeated_root_by_fixture(owner.clone(), legacy_alias, declared_repeating);
            p.root.bindings = vec![binding("First", 41)];
            p.root.children.clear();
            assert_eq!(
                crate::run_with_sources(&p, &primary, supplied),
                Err(EngineError::OwnedSourceAccess {
                    node: Some(41),
                    scope: None,
                    source: OwnedSourceError::UniteratedOwnedRepetition {
                        path: path(&["Key"])
                    },
                }),
                "owner={owner:?}, legacy_alias={legacy_alias}, declared_repeating={declared_repeating}"
            );
        }
    }
}

#[test]
fn root_group_implicit_reads_representative_but_member_empty_pins_read_each_item() {
    let expected = Instance::Repeated(vec![
        group(vec![
            ("First", text("a1")),
            ("GroupPosition", scalar(Value::Int(1))),
            (
                "Members",
                Instance::Repeated(vec![
                    group(vec![
                        ("Label", text("a1")),
                        ("MemberPosition", scalar(Value::Int(1))),
                    ]),
                    group(vec![
                        ("Label", text("a2")),
                        ("MemberPosition", scalar(Value::Int(2))),
                    ]),
                ]),
            ),
        ]),
        group(vec![
            ("First", text("b1")),
            ("GroupPosition", scalar(Value::Int(2))),
            (
                "Members",
                Instance::Repeated(vec![group(vec![
                    ("Label", text("b1")),
                    ("MemberPosition", scalar(Value::Int(1))),
                ])]),
            ),
        ]),
    ]);
    for (owner, legacy_alias) in [
        (SourceOwner::Primary, false),
        (named("Alias"), false),
        (named("Alias"), true),
    ] {
        for declared_repeating in [false, true] {
            let (p, primary, supplied) =
                repeated_root_by_fixture(owner.clone(), legacy_alias, declared_repeating);
            assert_eq!(
                crate::run_with_sources(&p, &primary, supplied),
                Ok(expected.clone()),
                "owner={owner:?}, legacy_alias={legacy_alias}, declared_repeating={declared_repeating}"
            );
        }
    }
}

#[test]
fn ordinary_repeated_root_item_empty_pin_keeps_baseline_active_item_semantics() {
    let expected = Instance::Repeated(vec![
        group(vec![
            ("First", text("a1")),
            ("GroupPosition", scalar(Value::Int(1))),
        ]),
        group(vec![
            ("First", text("b1")),
            ("GroupPosition", scalar(Value::Int(2))),
        ]),
        group(vec![
            ("First", text("a2")),
            ("GroupPosition", scalar(Value::Int(3))),
        ]),
    ]);
    for (owner, legacy_alias) in [
        (SourceOwner::Primary, false),
        (named("Alias"), false),
        (named("Alias"), true),
    ] {
        for declared_repeating in [false, true] {
            let (mut p, primary, supplied) =
                repeated_root_by_fixture(owner.clone(), legacy_alias, declared_repeating);
            p.root.group_by = None;
            p.root.bindings[0].node = 4;
            p.root.children.clear();
            assert_eq!(
                crate::run_with_sources(&p, &primary, supplied),
                Ok(expected.clone()),
                "owner={owner:?}, legacy_alias={legacy_alias}, declared_repeating={declared_repeating}"
            );
        }
    }
}

#[test]
fn by_group_empty_pin_uses_latest_exact_owner_not_another_owners_item() {
    let (mut p, primary, _) = repeated_root_by_fixture(SourceOwner::Primary, false, true);
    p.extra_sources = vec![NamedSource {
        name: "Alias".into(),
        path: "alias.json".into(),
        schema: p.source.clone(),
        options: Default::default(),
        dynamic_path: None,
    }];
    p.root.children[0].iteration = owned(named("Alias"), &[]);
    p.root.children[0].bindings = vec![binding("Label", 41)];
    let supplied = Instance::Repeated(vec![group(vec![
        ("Key", text("A")),
        ("Label", text("other-owner")),
    ])]);
    assert_eq!(
        crate::run_with_sources(&p, &primary, vec![("Alias".into(), supplied)]),
        Err(EngineError::OwnedSourceAccess {
            node: Some(41),
            scope: None,
            source: OwnedSourceError::UniteratedOwnedRepetition {
                path: path(&["Key"])
            },
        })
    );
}

#[test]
fn nonrepeating_group_root_empty_pin_keeps_immutable_singleton_semantics() {
    let expected = Instance::Repeated(vec![group(vec![
        ("First", text("single")),
        ("GroupPosition", scalar(Value::Int(1))),
        (
            "Members",
            Instance::Repeated(vec![group(vec![
                ("Label", text("single")),
                ("MemberPosition", scalar(Value::Int(1))),
            ])]),
        ),
    ])]);
    for owner in [SourceOwner::Primary, named("Alias")] {
        let (mut p, _, _) = repeated_root_by_fixture(owner.clone(), false, false);
        p.root.bindings[0].node = 4;
        let singleton = group(vec![("Key", text("A")), ("Label", text("single"))]);
        let (primary, supplied) = match &owner {
            SourceOwner::Primary => (singleton, vec![]),
            SourceOwner::Named { name } => (
                group(vec![("Marker", text("P.root"))]),
                vec![(name.clone(), singleton)],
            ),
        };
        assert_eq!(
            crate::run_with_sources(&p, &primary, supplied),
            Ok(expected.clone())
        );
    }
}

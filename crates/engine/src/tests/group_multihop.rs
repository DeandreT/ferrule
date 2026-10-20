//! Legacy grouping over a source path that crosses two repeating levels.
//! Each group borrows the first member's intermediate ancestors, so these
//! fixtures pin complete outputs and provenance across order boundaries.

use std::cell::RefCell;
use std::path::Path;

use super::*;
use ir::SchemaNode;
use mapping::{AggregateOp, Binding, ScopeIteration};

use crate::{ExecutionContext, TraceEvent, TraceSink, run_with_context};

#[derive(Clone, Copy)]
enum Failure {
    None,
    // The key raises at candidate `a4`, before any group is produced.
    Key,
    // The member label raises at `a3`, inside an already produced group.
    Member,
}

#[derive(Clone, Copy)]
enum Mode {
    By,
    AdjacentBy,
    StartingWith,
    EndingWith,
    IntoBlocks,
}

fn field(frame: &[&str], name: &str) -> Node {
    Node::SourceField {
        frame: Some(frame.iter().map(|segment| (*segment).into()).collect()),
        path: vec![name.into()],
    }
}

fn binding(target_field: &str, node: NodeId) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}

const ITEM: &[&str] = &["Order", "Item"];

fn project(mode: Mode, failure: Failure) -> Project {
    let mut graph = Graph {
        nodes: [
            (0, field(ITEM, "Key")),
            (1, field(ITEM, "Label")),
            (2, field(&["Order"], "Id")),
            (
                3,
                Node::Position {
                    collection: ITEM.iter().map(|segment| (*segment).into()).collect(),
                },
            ),
            (
                4,
                Node::Aggregate {
                    function: AggregateOp::Count,
                    collection: vec!["Item".into()],
                    value: vec![],
                    expression: None,
                    arg: None,
                },
            ),
            (5, field(ITEM, "Start")),
            (6, field(ITEM, "End")),
            (
                7,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
            (
                8,
                Node::Call {
                    function: "not_equal".into(),
                    args: vec![1, 9],
                },
            ),
            (
                9,
                Node::Const {
                    value: Value::String("skip".into()),
                },
            ),
        ]
        .into_iter()
        .collect(),
    };
    // A raise message reads the order id, which resolves through the
    // intermediate ancestor active at the failing evaluation.
    let (key, label) = match failure {
        Failure::None => (0, 1),
        Failure::Key => (13, 1),
        Failure::Member => (0, 13),
    };
    let raised = match failure {
        Failure::None => None,
        Failure::Key => Some(("a4", 0)),
        Failure::Member => Some(("a3", 1)),
    };
    if let Some((target, otherwise)) = raised {
        graph.nodes.extend([
            (
                10,
                Node::Call {
                    function: "equal".into(),
                    args: vec![1, 11],
                },
            ),
            (
                11,
                Node::Const {
                    value: Value::String(target.into()),
                },
            ),
            (12, Node::Raise { message: Some(2) }),
            (
                13,
                Node::If {
                    condition: 10,
                    then: 12,
                    else_: otherwise,
                },
            ),
        ]);
    }
    let mut grouped = Scope {
        target_field: "Group".into(),
        iteration: ScopeIteration::Source(ITEM.iter().map(|segment| (*segment).into()).collect()),
        filter: Some(8),
        bindings: vec![
            binding("Key", 0),
            binding("First", 1),
            binding("OrderId", 2),
            binding("Position", 3),
            binding("Count", 4),
        ],
        children: vec![Scope {
            target_field: "Member".into(),
            iteration: ScopeIteration::Source(Vec::new()),
            bindings: vec![
                binding("Label", label),
                binding("OrderId", 2),
                binding("Position", 3),
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    match mode {
        Mode::By => grouped.group_by = Some(key),
        Mode::AdjacentBy => grouped.group_adjacent_by = Some(key),
        Mode::StartingWith => grouped.group_starting_with = Some(5),
        Mode::EndingWith => grouped.group_ending_with = Some(6),
        Mode::IntoBlocks => grouped.group_into_blocks = Some(7),
    }
    Project {
        source: SchemaNode::group("root", vec![]),
        target: SchemaNode::group("root", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![grouped],
            ..Scope::default()
        },
    }
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

/// Two orders whose items repeat keys within and across orders. The `skip`
/// item is filtered before partitioning and carries both boundary flags.
fn source() -> Instance {
    let item = |key: &str, label: &str, start: bool, end: bool| {
        group(vec![
            ("Key", text(key)),
            ("Label", text(label)),
            ("Start", Instance::Scalar(Value::Bool(start))),
            ("End", Instance::Scalar(Value::Bool(end))),
        ])
    };
    let order = |id: &str, items: Vec<Instance>| {
        group(vec![("Id", text(id)), ("Item", Instance::Repeated(items))])
    };
    group(vec![(
        "Order",
        Instance::Repeated(vec![
            order(
                "O1",
                vec![
                    item("A", "a1", true, false),
                    item("B", "b1", false, false),
                    item("X", "skip", true, true),
                    item("A", "a2", false, true),
                ],
            ),
            order(
                "O2",
                vec![
                    item("C", "c1", false, false),
                    item("A", "a3", true, false),
                    item("A", "a4", false, false),
                    item("B", "b2", false, true),
                ],
            ),
        ]),
    )])
}

#[derive(Default)]
struct Collector(RefCell<Vec<TraceEvent>>);

impl TraceSink for Collector {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

type GroupSpec<'s> = (&'s str, &'s str, &'s str, &'s [&'s str]);

/// Each group reports its first member's key, label and order. Every member
/// resolves the order id through that same first-member ancestor prefix.
fn expected(groups: &[GroupSpec<'_>]) -> Instance {
    let int = |value: usize| Instance::Scalar(Value::Int(value as i64));
    group(vec![(
        "Group",
        Instance::Repeated(
            groups
                .iter()
                .enumerate()
                .map(|(index, (key, first, order, members))| {
                    group(vec![
                        ("Key", text(key)),
                        ("First", text(first)),
                        ("OrderId", text(order)),
                        ("Position", int(index + 1)),
                        ("Count", int(members.len())),
                        (
                            "Member",
                            Instance::Repeated(
                                members
                                    .iter()
                                    .enumerate()
                                    .map(|(position, label)| {
                                        group(vec![
                                            ("Label", text(label)),
                                            ("OrderId", text(order)),
                                            ("Position", int(position + 1)),
                                        ])
                                    })
                                    .collect(),
                            ),
                        ),
                    ])
                })
                .collect(),
        ),
    )])
}

#[test]
fn multihop_groups_keep_complete_first_member_ancestry_in_every_mode() {
    let cases: [(Mode, &[GroupSpec<'_>]); 5] = [
        (
            Mode::By,
            &[
                ("A", "a1", "O1", &["a1", "a2", "a3", "a4"]),
                ("B", "b1", "O1", &["b1", "b2"]),
                ("C", "c1", "O2", &["c1"]),
            ],
        ),
        (
            Mode::AdjacentBy,
            &[
                ("A", "a1", "O1", &["a1"]),
                ("B", "b1", "O1", &["b1"]),
                ("A", "a2", "O1", &["a2"]),
                ("C", "c1", "O2", &["c1"]),
                ("A", "a3", "O2", &["a3", "a4"]),
                ("B", "b2", "O2", &["b2"]),
            ],
        ),
        (
            Mode::StartingWith,
            &[
                ("A", "a1", "O1", &["a1", "b1", "a2", "c1"]),
                ("A", "a3", "O2", &["a3", "a4", "b2"]),
            ],
        ),
        (
            Mode::EndingWith,
            &[
                ("A", "a1", "O1", &["a1", "b1", "a2"]),
                ("C", "c1", "O2", &["c1", "a3", "a4", "b2"]),
            ],
        ),
        (
            Mode::IntoBlocks,
            &[
                ("A", "a1", "O1", &["a1", "b1"]),
                ("A", "a2", "O1", &["a2", "c1"]),
                ("A", "a3", "O2", &["a3", "a4"]),
                ("B", "b2", "O2", &["b2"]),
            ],
        ),
    ];
    for (mode, groups) in cases {
        assert_eq!(
            run(&project(mode, Failure::None), &source()),
            Ok(expected(groups))
        );
    }
}

type Positions = Vec<(Vec<String>, usize, bool)>;

fn positions(positions: &[crate::TracePosition]) -> Positions {
    positions
        .iter()
        .map(|position| {
            (
                position.collection.clone(),
                position.index,
                position.grouped,
            )
        })
        .collect()
}

fn order_item(order: usize, item: usize, grouped: bool) -> Positions {
    vec![
        (vec!["Order".into()], order, false),
        (vec!["Order".into(), "Item".into()], item, grouped),
    ]
}

fn traced(project: &Project) -> (Result<Instance, EngineError>, Vec<TraceEvent>) {
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);
    let result = run_with_context(project, &source(), &execution);
    (result, collector.0.into_inner())
}

#[test]
fn multihop_group_trace_keeps_first_member_positions_and_ancestor_reads() {
    let (result, events) = traced(&project(Mode::By, Failure::None));
    assert!(result.is_ok());
    let groups = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::GroupProduced {
                group_index,
                member_count,
                key,
                retained,
                positions: at,
                ..
            } => Some((
                *group_index,
                *member_count,
                key.as_ref().map(|key| key.preview.clone()),
                *retained,
                positions(at),
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        groups,
        [
            (1, 4, Some("A".into()), true, order_item(1, 1, false)),
            (2, 2, Some("B".into()), true, order_item(1, 2, false)),
            (3, 1, Some("C".into()), true, order_item(2, 1, false)),
        ]
    );
    let member = |group: usize, order: usize, index: usize| {
        let mut at = order_item(order, group, true);
        at.push((vec!["Order".into(), "Item".into()], index, false));
        at
    };
    let order_reads = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::NodeValue {
                node: 2,
                positions: at,
                value,
            } => Some((positions(at), value.preview.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    let o1 = || "O1".to_owned();
    let o2 = || "O2".to_owned();
    assert_eq!(
        order_reads,
        [
            (order_item(1, 1, true), o1()),
            (member(1, 1, 1), o1()),
            (member(1, 1, 2), o1()),
            (member(1, 1, 3), o1()),
            (member(1, 1, 4), o1()),
            (order_item(1, 2, true), o1()),
            (member(2, 1, 1), o1()),
            (member(2, 1, 2), o1()),
            (order_item(2, 3, true), o2()),
            (member(3, 2, 1), o2()),
        ]
    );
}

#[test]
fn multihop_group_failures_keep_evaluation_order_and_ancestor_context() {
    let key_failure = project(Mode::By, Failure::Key);
    let (result, events) = traced(&key_failure);
    assert_eq!(
        result,
        Err(EngineError::MappingException {
            node: 12,
            message: Some("O2".into()),
        })
    );
    // Five earlier candidates (one filtered) are keyed before the failure;
    // partitioning stops there, so no group or target output is produced.
    let filtered = events
        .iter()
        .filter(|event| matches!(event, TraceEvent::FilterDecision { .. }))
        .count();
    assert_eq!(filtered, 7);
    assert!(!events.iter().any(|event| matches!(
        event,
        TraceEvent::GroupProduced { .. } | TraceEvent::TargetProduced { .. }
    )));

    let (result, events) = traced(&project(Mode::By, Failure::Member));
    assert_eq!(
        result,
        Err(EngineError::MappingException {
            node: 12,
            message: Some("O1".into()),
        })
    );
    // The first group's a1 and a2 members are produced before a3 fails it.
    let produced = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::TargetProduced { scope, .. } => Some(scope.target_path.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(produced, vec![vec!["Group", "Member"]; 2]);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, TraceEvent::GroupProduced { .. }))
            .count(),
        3
    );
}

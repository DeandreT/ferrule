//! Computed join-key sources are modeled but not yet executable: validation,
//! execution, tracing and named-input planning each refuse them.

use std::cell::RefCell;
use std::path::Path;

use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{
    AggregateOp, Binding, ComputedJoinSourceId, JoinConditions, JoinId, JoinKey, JoinPlan,
    JoinSource, NamedSource, ScopeIteration,
};

const JOIN: JoinId = JoinId::new(5);
const KEY: ComputedJoinSourceId = ComputedJoinSourceId::new(1);
const NOT_EXECUTABLE: &str = "computed join sources are not yet executable";

fn customers() -> JoinSource {
    JoinSource::new(vec!["Customer".into()])
}

/// The computed key as the first source, or as the stage's right source.
fn plan(key_first: bool) -> JoinPlan {
    if key_first {
        JoinPlan::new(
            JoinSource::computed(KEY, 1),
            customers(),
            JoinConditions::new(JoinKey::computed(KEY, vec!["Number".into()])),
        )
    } else {
        JoinPlan::new(
            customers(),
            JoinSource::computed(KEY, 1),
            JoinConditions::new(JoinKey::new(
                vec!["Customer".into()],
                vec!["Number".into()],
                Vec::new(),
            )),
        )
    }
    .unwrap()
}

fn project(root: Scope, graph: Graph) -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Customer",
                    vec![
                        SchemaNode::scalar("Number", ScalarType::String),
                        SchemaNode::scalar("Name", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Report",
            vec![
                SchemaNode::group("Hit", vec![SchemaNode::scalar("Name", ScalarType::String)])
                    .repeating(),
                SchemaNode::scalar("Count", ScalarType::Int),
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
        root,
    }
}

fn graph() -> Graph {
    Graph {
        nodes: BTreeMap::from([
            (
                1,
                Node::Const {
                    value: Value::String("C2".into()),
                },
            ),
            (
                2,
                Node::JoinField {
                    join: JOIN,
                    collection: vec!["Customer".into()],
                    path: vec!["Name".into()],
                },
            ),
        ]),
    }
}

fn scope_project(key_first: bool) -> Project {
    project(
        Scope {
            children: vec![Scope {
                target_field: "Hit".into(),
                iteration: ScopeIteration::InnerJoin {
                    id: JOIN,
                    plan: plan(key_first),
                },
                bindings: vec![Binding {
                    target_field: "Name".into(),
                    node: 2,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
        graph(),
    )
}

fn aggregate_project(key_first: bool) -> Project {
    let mut graph = graph();
    graph.nodes.insert(
        3,
        Node::JoinAggregate {
            function: AggregateOp::Count,
            join: JOIN,
            plan: plan(key_first),
            expression: None,
            arg: None,
        },
    );
    project(
        Scope {
            bindings: vec![Binding {
                target_field: "Count".into(),
                node: 3,
            }],
            ..Scope::default()
        },
        graph,
    )
}

fn source() -> Instance {
    let customer = |number: &str, name: &str| {
        Instance::Group(
            vec![
                (
                    "Number".into(),
                    Instance::Scalar(Value::String(number.into())),
                ),
                ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            ]
            .into(),
        )
    };
    Instance::Group(
        vec![(
            "Customer".into(),
            Instance::Repeated(vec![customer("C1", "Ada"), customer("C2", "Grace")]),
        )]
        .into(),
    )
}

fn projects() -> Vec<Project> {
    [true, false]
        .into_iter()
        .flat_map(|key_first| [scope_project(key_first), aggregate_project(key_first)])
        .collect()
}

#[test]
fn validation_refuses_computed_sources_in_scopes_and_aggregates() {
    for project in projects() {
        let issues = validate(&project);
        assert_eq!(
            issues
                .iter()
                .filter(|issue| issue.message.contains(NOT_EXECUTABLE))
                .count(),
            1,
            "{issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("join 5 computed source 1 (node 1)")),
            "{issues:?}"
        );
    }
}

#[test]
fn execution_refuses_computed_sources_with_a_typed_error() {
    for project in projects() {
        assert!(matches!(
            run(&project, &source()),
            Err(EngineError::ComputedJoinSource {
                join: JOIN,
                computed: KEY,
            })
        ));
    }
    assert_eq!(
        EngineError::ComputedJoinSource {
            join: JOIN,
            computed: KEY,
        }
        .to_string(),
        format!("join 5 computed source 1: {NOT_EXECUTABLE}")
    );
}

#[derive(Default)]
struct Collector(RefCell<Vec<TraceEvent>>);

impl TraceSink for Collector {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

#[test]
fn traced_runs_refuse_before_any_join_candidate() {
    for project in projects() {
        let collector = Collector::default();
        let execution =
            ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);
        assert!(matches!(
            run_with_context(&project, &source(), &execution),
            Err(EngineError::ComputedJoinSource { .. })
        ));
        assert!(!collector.0.into_inner().iter().any(|event| matches!(
            event,
            TraceEvent::IterationCandidate { positions, .. }
                if positions.iter().any(|position| position.join.is_some())
        )));
    }
}

#[test]
fn named_input_planning_refuses_computed_sources() {
    for mut project in projects() {
        project.extra_sources.push(NamedSource {
            name: "Lookup".into(),
            path: "lookup.json".into(),
            schema: SchemaNode::group("Lookup", vec![]),
            options: Default::default(),
            dynamic_path: None,
        });
        assert!(matches!(
            required_sources_for_target(&project, TargetSelection::Primary),
            Err(EngineError::ComputedJoinSource {
                join: JOIN,
                computed: KEY,
            })
        ));
    }
    // Plans without a computed source still plan normally.
    let mut plain = scope_project(true);
    plain.root.children[0].iteration = ScopeIteration::InnerJoin {
        id: JOIN,
        plan: JoinPlan::new(
            customers(),
            JoinSource::new(vec!["Order".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["Customer".into()],
                vec!["Number".into()],
                vec!["CustomerNumber".into()],
            )),
        )
        .unwrap(),
    };
    assert!(required_sources_for_target(&plain, TargetSelection::Primary).is_ok());
}

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FunctionId, FunctionParameter, FunctionParameterId, Graph, JoinConditions, JoinId,
    JoinKey, JoinPlan, JoinSource, Node, NodeId, Project, Scope, ScopeIteration, UserFunction,
};

use super::*;
use crate::{ExecutionContext, TraceEvent, TraceSink, run_with_context};

fn empty_source() -> Instance {
    Instance::Group(Vec::<(String, Instance)>::new().into())
}

fn scalar_project(nodes: impl IntoIterator<Item = (NodeId, Node)>, output: NodeId) -> Project {
    Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Out", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: nodes.into_iter().collect(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Out".into(),
                node: output,
            }],
            ..Scope::default()
        },
    }
}

fn restricted(work: u64, depth: u64, join_rows: u64) -> ExecutionPolicy {
    ExecutionPolicy::Restricted(RestrictedPolicy::new(ExecutionBudgets::new(
        work, depth, join_rows,
    )))
}

fn run_under(
    project: &Project,
    source: &Instance,
    policy: &ExecutionPolicy,
) -> Result<Instance, EngineError> {
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_policy(policy);
    run_with_context(project, source, &execution)
}

fn out(instance: &Instance) -> &Value {
    instance
        .field("Out")
        .and_then(Instance::as_scalar)
        .expect("scalar Out")
}

fn exceeded(kind: ExecutionBudgetKind, limit: u64) -> EngineError {
    EngineError::ExecutionBudgetExceeded { kind, limit }
}

/// Node 0 is a constant; node k concatenates node k-1 with itself. Without
/// memoization the root costs 2^(levels+1) - 1 evaluations.
fn doubling_dag(levels: NodeId) -> Vec<(NodeId, Node)> {
    let mut nodes = vec![(
        0,
        Node::Const {
            value: Value::String("x".into()),
        },
    )];
    nodes.extend((1..=levels).map(|level| {
        (
            level,
            Node::Call {
                function: "concat".into(),
                args: vec![level - 1, level - 1],
            },
        )
    }));
    nodes
}

#[test]
fn work_budget_admits_exactly_the_shared_dag_evaluation_count() {
    let project = scalar_project(doubling_dag(6), 6);
    let expected = Value::String("x".repeat(64));
    let trusted = run_under(&project, &empty_source(), &ExecutionPolicy::Trusted).unwrap();
    assert_eq!(out(&trusted), &expected);

    let admitted = run_under(&project, &empty_source(), &restricted(127, 64, 0)).unwrap();
    assert_eq!(out(&admitted), &expected);
    assert_eq!(
        run_under(&project, &empty_source(), &restricted(126, 64, 0)).unwrap_err(),
        exceeded(ExecutionBudgetKind::Work, 126)
    );
}

#[test]
fn work_budget_cuts_off_exponential_recursion_in_a_small_shared_dag() {
    // 49 nodes would need 2^49 - 1 evaluations; the budget stops after 10,000.
    let project = scalar_project(doubling_dag(48), 48);
    assert_eq!(
        run_under(&project, &empty_source(), &restricted(10_000, 64, 0)).unwrap_err(),
        exceeded(ExecutionBudgetKind::Work, 10_000)
    );
}

#[test]
fn depth_budget_admits_exactly_the_longest_active_chain() {
    let mut nodes = vec![(
        0,
        Node::Const {
            value: Value::String("a".into()),
        },
    )];
    nodes.extend((1..20).map(|node| {
        (
            node,
            Node::Call {
                function: "upper".into(),
                args: vec![node - 1],
            },
        )
    }));
    let project = scalar_project(nodes, 19);
    let admitted = run_under(&project, &empty_source(), &restricted(1_000, 20, 0)).unwrap();
    assert_eq!(out(&admitted), &Value::String("A".into()));
    assert_eq!(
        run_under(&project, &empty_source(), &restricted(1_000, 19, 0)).unwrap_err(),
        exceeded(ExecutionBudgetKind::Depth, 19)
    );
    // When both budgets would be exceeded at one entry, work is reported.
    assert_eq!(
        run_under(&project, &empty_source(), &restricted(0, 0, 0)).unwrap_err(),
        exceeded(ExecutionBudgetKind::Work, 0)
    );
}

#[test]
fn user_function_body_nodes_share_the_work_and_depth_budgets() {
    let function = FunctionId::new(1);
    let mut project = scalar_project(
        [
            (
                0,
                Node::Const {
                    value: Value::String("b".into()),
                },
            ),
            (
                1,
                Node::UserFunctionCall {
                    function,
                    args: vec![0],
                },
            ),
        ],
        1,
    );
    project.user_functions = BTreeMap::from([(
        function,
        UserFunction {
            library: "tests".into(),
            name: "shout".into(),
            description: None,
            parameters: vec![FunctionParameter {
                id: FunctionParameterId::new(1),
                name: "value".into(),
                ty: ScalarType::String,
            }],
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: Graph {
                nodes: BTreeMap::from([
                    (
                        0,
                        Node::FunctionParameter {
                            parameter: FunctionParameterId::new(1),
                        },
                    ),
                    (
                        1,
                        Node::Call {
                            function: "upper".into(),
                            args: vec![0],
                        },
                    ),
                ]),
            },
            output: 1,
        },
    )]);
    // Main call, its argument, then the body output and its parameter read.
    let admitted = run_under(&project, &empty_source(), &restricted(4, 3, 0)).unwrap();
    assert_eq!(out(&admitted), &Value::String("B".into()));
    assert_eq!(
        run_under(&project, &empty_source(), &restricted(3, 3, 0)).unwrap_err(),
        exceeded(ExecutionBudgetKind::Work, 3)
    );
    assert_eq!(
        run_under(&project, &empty_source(), &restricted(4, 2, 0)).unwrap_err(),
        exceeded(ExecutionBudgetKind::Depth, 2)
    );
}

fn join_project() -> (Project, Instance) {
    let row = |fields: &[(&str, Value)]| {
        Instance::Group(
            fields
                .iter()
                .map(|(name, value)| ((*name).to_string(), Instance::Scalar(value.clone())))
                .collect::<Vec<_>>()
                .into(),
        )
    };
    let source = Instance::Group(
        vec![
            (
                "A".to_string(),
                Instance::Repeated((1..=3).map(|id| row(&[("id", Value::Int(id))])).collect()),
            ),
            (
                "B".to_string(),
                Instance::Repeated(
                    [1, 1, 2, 9]
                        .into_iter()
                        .map(|id| row(&[("aid", Value::Int(id))]))
                        .collect(),
                ),
            ),
        ]
        .into(),
    );
    let plan = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["id".into()],
            vec!["aid".into()],
        )),
    )
    .unwrap();
    let project = Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("A", vec![SchemaNode::scalar("id", ScalarType::Int)]).repeating(),
                SchemaNode::group("B", vec![SchemaNode::scalar("aid", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Id", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::InnerJoin {
                    id: JoinId::new(7),
                    plan,
                },
                bindings: vec![Binding {
                    target_field: "Id".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
        ..scalar_project(
            [(
                0,
                Node::JoinField {
                    join: JoinId::new(7),
                    collection: vec!["A".into()],
                    path: vec!["id".into()],
                },
            )],
            0,
        )
    };
    (project, source)
}

#[test]
fn join_row_budget_counts_every_examined_candidate_tuple() {
    let (project, source) = join_project();
    let trusted = run_under(&project, &source, &ExecutionPolicy::Trusted).unwrap();
    let rows = |instance: &Instance| match instance.field("Row") {
        Some(Instance::Repeated(rows)) => rows.len(),
        other => panic!("unexpected rows {other:?}"),
    };
    assert_eq!(rows(&trusted), 3);

    // Three left rows times four right rows, matched or not.
    let admitted = run_under(&project, &source, &restricted(100, 10, 12)).unwrap();
    assert_eq!(admitted, trusted);
    assert_eq!(
        run_under(&project, &source, &restricted(100, 10, 11)).unwrap_err(),
        exceeded(ExecutionBudgetKind::JoinRows, 11)
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
fn restricted_runs_keep_trusted_trace_multiplicity() {
    let project = scalar_project(doubling_dag(3), 3);
    let trusted = Collector::default();
    let policy = restricted(1_000, 64, 0);
    let observed = Collector::default();
    run_with_context(
        &project,
        &empty_source(),
        &ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trusted),
    )
    .unwrap();
    run_with_context(
        &project,
        &empty_source(),
        &ExecutionContext::new(Path::new("mapping.json"))
            .with_trace_sink(&observed)
            .with_policy(&policy),
    )
    .unwrap();
    let trusted = trusted.0.into_inner();
    assert_eq!(trusted, observed.0.into_inner());
    let leaf_values = trusted
        .iter()
        .filter(|event| matches!(event, TraceEvent::NodeValue { node: 0, .. }))
        .count();
    assert_eq!(leaf_values, 8, "the shared leaf is evaluated once per path");
}

#[test]
fn lazy_branches_are_not_charged_and_earlier_errors_take_precedence() {
    let mut nodes = doubling_dag(20);
    nodes.extend([
        (
            100,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            101,
            Node::Const {
                value: Value::String("ok".into()),
            },
        ),
        (
            102,
            Node::If {
                condition: 100,
                then: 101,
                else_: 20,
            },
        ),
    ]);
    let project = scalar_project(nodes.clone(), 102);
    let admitted = run_under(&project, &empty_source(), &restricted(3, 3, 0)).unwrap();
    assert_eq!(out(&admitted), &Value::String("ok".into()));
    assert_eq!(
        run_under(&project, &empty_source(), &restricted(2, 3, 0)).unwrap_err(),
        exceeded(ExecutionBudgetKind::Work, 2)
    );

    // The first argument fails before the expensive second argument is reached.
    nodes.extend([
        (
            103,
            Node::FunctionParameter {
                parameter: FunctionParameterId::new(1),
            },
        ),
        (
            104,
            Node::Call {
                function: "concat".into(),
                args: vec![103, 20],
            },
        ),
    ]);
    let project = scalar_project(nodes, 104);
    let trusted = run_under(&project, &empty_source(), &ExecutionPolicy::Trusted).unwrap_err();
    assert_eq!(
        trusted,
        EngineError::FunctionParameterOutsideFunction { node: 103 }
    );
    assert_eq!(
        run_under(&project, &empty_source(), &restricted(2, 3, 0)).unwrap_err(),
        trusted
    );
    assert_eq!(
        run_under(&project, &empty_source(), &restricted(1, 3, 0)).unwrap_err(),
        exceeded(ExecutionBudgetKind::Work, 1)
    );
}

#[test]
fn budget_meter_releases_depth_when_an_evaluation_returns() {
    let meter = BudgetMeter::new(ExecutionBudgets::new(3, 2, 1));
    {
        let _outer = meter.enter_node().unwrap();
        let _inner = meter.enter_node().unwrap();
        assert_eq!(meter.counters(), (2, 2, 0));
        assert!(matches!(
            meter.enter_node(),
            Err(EngineError::ExecutionBudgetExceeded {
                kind: ExecutionBudgetKind::Depth,
                limit: 2
            })
        ));
    }
    assert_eq!(meter.counters(), (2, 0, 0));
    let _third = meter.enter_node().unwrap();
    assert!(meter.enter_node().is_err());
    meter.charge_join_row().unwrap();
    assert!(meter.charge_join_row().is_err());
    assert_eq!(meter.counters(), (3, 1, 1));
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_engine_policy_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        for directory in ["in", "out", "outside", "in-sibling"] {
            std::fs::create_dir_all(path.join(directory)).unwrap();
        }
        std::fs::write(path.join("in/a.xml"), "<a/>").unwrap();
        std::fs::write(path.join("outside/b.xml"), "<b/>").unwrap();
        std::fs::write(path.join("in-sibling/c.xml"), "<c/>").unwrap();
        Self(path)
    }

    fn policy(&self) -> RestrictedPolicy {
        RestrictedPolicy::new(ExecutionBudgets::new(1, 1, 1))
            .allow_root(self.0.join("in"), RootAccess::Read)
            .unwrap()
            .allow_root(self.0.join("out"), RootAccess::ReadWrite)
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn outside(result: Result<(), PolicyRefusal>) -> bool {
    matches!(result, Err(PolicyRefusal::PathOutsideRoots { .. }))
}

fn unresolvable(result: Result<(), PolicyRefusal>) -> bool {
    matches!(result, Err(PolicyRefusal::UnresolvablePath { .. }))
}

#[test]
fn filesystem_roots_admit_only_canonical_locations_beneath_them() {
    let fixture = Fixture::new();
    let policy = fixture.policy();
    let read = |path: PathBuf| policy.authorize_path(&path, PathAccess::Read);
    let write = |path: PathBuf| policy.authorize_path(&path, PathAccess::Write);

    assert_eq!(read(fixture.0.join("in/a.xml")), Ok(()));
    assert_eq!(read(fixture.0.join("in/../in/./a.xml")), Ok(()));
    assert!(outside(read(fixture.0.join("outside/b.xml"))));
    assert!(outside(read(fixture.0.join("in/../outside/b.xml"))));
    assert!(outside(read(fixture.0.join("in-sibling/c.xml"))));
    assert!(unresolvable(read(fixture.0.join("in/missing.xml"))));
    // Read-write roots admit reads too.
    std::fs::write(fixture.0.join("out/existing.xml"), "<e/>").unwrap();
    assert_eq!(read(fixture.0.join("out/existing.xml")), Ok(()));

    assert_eq!(write(fixture.0.join("out/new/nested/file.xml")), Ok(()));
    assert_eq!(write(fixture.0.join("out/existing.xml")), Ok(()));
    assert!(outside(write(fixture.0.join("in/a.xml"))));
    assert!(outside(write(fixture.0.join("in/new.xml"))));
    assert!(outside(write(fixture.0.join("out/../outside/new.xml"))));
    assert!(unresolvable(write(
        fixture.0.join("out/missing/../../outside/new.xml")
    )));

    let trusted = ExecutionPolicy::Trusted;
    assert_eq!(
        trusted.authorize_read(&fixture.0.join("outside/none")),
        Ok(())
    );
    assert_eq!(trusted.authorize_write(Path::new("/")), Ok(()));
}

#[cfg(unix)]
#[test]
fn symlinks_inside_a_root_cannot_reach_outside_it() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink(
        fixture.0.join("outside/b.xml"),
        fixture.0.join("in/link.xml"),
    )
    .unwrap();
    std::os::unix::fs::symlink(fixture.0.join("outside"), fixture.0.join("out/escape")).unwrap();
    let policy = fixture.policy();
    assert!(outside(policy.authorize_path(
        &fixture.0.join("in/link.xml"),
        PathAccess::Read
    )));
    assert!(outside(policy.authorize_path(
        &fixture.0.join("out/escape/new.xml"),
        PathAccess::Write
    )));
}

#[test]
fn http_origins_match_exact_scheme_host_and_port() {
    let policy = RestrictedPolicy::new(ExecutionBudgets::new(1, 1, 1))
        .allow_origin("http://127.0.0.1:8080")
        .unwrap()
        .allow_origin("https://Data.Example.com/")
        .unwrap()
        .allow_origin("http://[::1]:9")
        .unwrap();
    for admitted in [
        "http://127.0.0.1:8080/orders.xml?page=2",
        "HTTPS://data.example.COM:443/p",
        "https://data.example.com",
        "http://[::1]:9/x",
    ] {
        assert_eq!(policy.authorize_url(admitted), Ok(()), "{admitted}");
    }
    for denied in [
        "http://127.0.0.1:8081/",
        "https://127.0.0.1:8080/",
        "http://data.example.com/",
        "https://data.example.com.evil.net/",
        "https://data.example.com:8443/",
        "http://[::1]/",
    ] {
        assert!(
            matches!(
                policy.authorize_url(denied),
                Err(PolicyRefusal::HttpOriginDenied { .. })
            ),
            "{denied}"
        );
    }
    for invalid in [
        "http://user:secret@127.0.0.1:8080/",
        "http://127.0.0.1:8080\\@evil/",
        "http://127.0.0.1:/",
        "http://:8080/",
        "http:///x",
        "ftp://127.0.0.1:8080/",
        "http://[::1/",
    ] {
        let refusal = policy.authorize_url(invalid).unwrap_err();
        assert!(
            matches!(refusal, PolicyRefusal::InvalidHttpUrl { .. }),
            "{invalid}: {refusal:?}"
        );
        assert!(!refusal.to_string().contains("secret"));
    }

    let wrapped = ExecutionPolicy::Restricted(policy);
    assert_eq!(
        wrapped.authorize_read(Path::new("http://127.0.0.1:8080/a.xml")),
        Ok(())
    );
    assert!(matches!(
        wrapped.authorize_read(Path::new("https://elsewhere.test/a.xml")),
        Err(PolicyRefusal::HttpOriginDenied { .. })
    ));
}

#[test]
fn policy_configuration_and_capabilities_are_explicit() {
    let budgets = ExecutionBudgets::new(1, 1, 1);
    for origin in ["ftp://host", "http://host/path", "http://host:", "host:80"] {
        assert!(
            matches!(
                RestrictedPolicy::new(budgets).allow_origin(origin),
                Err(PolicyConfigError::InvalidOrigin { .. })
            ),
            "{origin}"
        );
    }
    let fixture = Fixture::new();
    assert!(matches!(
        RestrictedPolicy::new(budgets).allow_root(fixture.0.join("in/a.xml"), RootAccess::Read),
        Err(PolicyConfigError::RootNotDirectory { .. })
    ));
    assert!(matches!(
        RestrictedPolicy::new(budgets).allow_root(fixture.0.join("absent"), RootAccess::Read),
        Err(PolicyConfigError::RootIo { .. })
    ));

    // Deny by default: no roots, no origins.
    let empty = ExecutionPolicy::Restricted(RestrictedPolicy::new(budgets));
    assert!(outside(empty.authorize_read(&fixture.0.join("in/a.xml"))));
    assert!(matches!(
        empty.authorize_url("http://127.0.0.1/"),
        Err(PolicyRefusal::HttpOriginDenied { .. })
    ));
    assert_eq!(
        empty.authorize_capability(RestrictedCapability::HttpRedirect),
        Err(PolicyRefusal::UnsupportedCapability {
            capability: RestrictedCapability::HttpRedirect
        })
    );
    assert_eq!(
        ExecutionPolicy::Trusted.authorize_capability(RestrictedCapability::HttpRedirect),
        Ok(())
    );
}

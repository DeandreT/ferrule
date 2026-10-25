use std::cell::RefCell;
use std::collections::BTreeMap;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, IterationOutput, Node, Project, Scope, ScopeConstruction, ScopeIteration,
    ScopeSequence,
};

use crate::{
    EngineError, ExecutionContext, TraceEvent, TraceIteration, TraceSink, run, run_with_context,
    validate,
};

fn bind(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}

fn present(condition: u32, bindings: Vec<Binding>) -> Scope {
    Scope {
        iteration: ScopeIteration::OptionalPresence { condition },
        bindings,
        ..Default::default()
    }
}

/// `Rows/Item` concatenates two presence segments. The second reads the
/// parent row's position, so a missing frame would renumber it.
fn project(first_condition: Node) -> Project {
    let source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Keep", ScalarType::Bool),
                    SchemaNode::scalar("Name", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::group(
                        "Item",
                        vec![
                            SchemaNode::scalar("Name", ScalarType::String),
                            SchemaNode::scalar("Position", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let field = |name: &str| Node::SourceField {
        path: vec![name.into()],
        frame: Some(vec!["Rows".into()]),
    };
    Project {
        source,
        target,
        graph: Graph {
            nodes: BTreeMap::from([
                (0, first_condition),
                (1, field("Name")),
                (
                    2,
                    Node::Position {
                        collection: vec!["Rows".into()],
                    },
                ),
                (3, field("Keep")),
                (
                    4,
                    Node::Call {
                        function: "exists".into(),
                        args: vec![1],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![Scope {
                    target_field: "Item".into(),
                    iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                        present(0, vec![bind("Name", 1)]),
                        vec![present(4, vec![bind("Position", 2)])],
                    )),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

fn input(rows: &[(Option<bool>, Option<&str>)]) -> Instance {
    let rows = rows
        .iter()
        .map(|(keep, name)| {
            let mut fields = Vec::new();
            if let Some(keep) = keep {
                fields.push(("Keep".to_string(), Instance::Scalar(Value::Bool(*keep))));
            }
            if let Some(name) = name {
                fields.push((
                    "Name".to_string(),
                    Instance::Scalar(Value::String((*name).into())),
                ));
            }
            Instance::Group(fields.into())
        })
        .collect();
    Instance::Group(vec![("Rows".to_string(), Instance::Repeated(rows))].into())
}

fn items(output: &Instance) -> Vec<Vec<(String, Value)>> {
    let Some(Instance::Repeated(rows)) = output.field("Rows") else {
        panic!("rows: {output:?}");
    };
    rows.iter()
        .map(|row| match row.field("Item") {
            Some(Instance::Repeated(items)) => items
                .iter()
                .flat_map(|item| match item {
                    Instance::Group(fields) => fields
                        .iter()
                        .map(|(name, value)| match value {
                            Instance::Scalar(value) => (name.clone(), value.clone()),
                            other => panic!("item field: {other:?}"),
                        })
                        .collect::<Vec<_>>(),
                    other => panic!("item: {other:?}"),
                })
                .collect(),
            other => panic!("items: {other:?}"),
        })
        .collect()
}

#[test]
fn presence_constructs_zero_or_one_group_in_the_parent_frame() {
    let project = project(Node::SourceField {
        path: vec!["Keep".into()],
        frame: Some(vec!["Rows".into()]),
    });
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let output = run(
        &project,
        &input(&[
            (Some(true), Some("a")),
            (Some(false), Some("b")),
            (Some(false), None),
            (Some(true), Some("")),
        ]),
    )
    .unwrap();
    assert_eq!(
        items(&output),
        [
            vec![
                ("Name".into(), Value::String("a".into())),
                ("Position".into(), Value::Int(1)),
            ],
            vec![("Position".into(), Value::Int(2))],
            vec![],
            vec![
                ("Name".into(), Value::String(String::new())),
                ("Position".into(), Value::Int(4)),
            ],
        ]
    );
}

#[test]
fn presence_conditions_must_be_boolean() {
    let project = project(Node::SourceField {
        path: vec!["Keep".into()],
        frame: Some(vec!["Rows".into()]),
    });
    assert_eq!(
        run(&project, &input(&[(None, Some("a"))])),
        Err(EngineError::NotABool {
            node: 0,
            found: "null"
        })
    );
    let project = self::project(Node::Const {
        value: Value::String("yes".into()),
    });
    assert_eq!(
        run(&project, &input(&[(Some(true), None)])),
        Err(EngineError::NotABool {
            node: 0,
            found: "string"
        })
    );
}

fn first_segment(project: &mut Project) -> &mut Scope {
    let ScopeIteration::Concatenate(segments) = &mut project.root.children[0].children[0].iteration
    else {
        unreachable!("the fixture concatenates presence segments")
    };
    segments.iter_mut().next().unwrap()
}

#[test]
fn presence_validation_refuses_controls_constructions_and_singular_targets() {
    let base = project(Node::Const {
        value: Value::Bool(true),
    });
    let mut filtered = base.clone();
    first_segment(&mut filtered).filter = Some(3);
    let mut copied = base.clone();
    first_segment(&mut copied).construction = ScopeConstruction::CopyCurrentSource;
    first_segment(&mut copied).bindings.clear();
    let mut first = base.clone();
    first_segment(&mut first).iteration_output = IterationOutput::First;
    let mut missing = base.clone();
    first_segment(&mut missing).iteration = ScopeIteration::OptionalPresence { condition: 99 };
    let mut singular = base.clone();
    singular.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::OptionalPresence { condition: 0 },
            ..Default::default()
        }],
        ..Default::default()
    };
    if let ir::SchemaKind::Group { children, .. } = &mut singular.target.kind {
        children[0].repeating = false;
    }
    for (project, message) in [
        (
            filtered,
            "optional presence cannot use item controls or computed target properties",
        ),
        (
            copied,
            "optional presence requires constructed repeated output",
        ),
        (
            first,
            "optional presence requires constructed repeated output",
        ),
        (
            missing,
            "optional presence condition references missing node 99",
        ),
        (
            singular,
            "optional presence requires a repeating target group schema",
        ),
    ] {
        let issues = validate(&project);
        assert!(
            issues.iter().any(|issue| issue.message == message),
            "{message}: {issues:?}"
        );
    }
}

#[derive(Default)]
struct Collector(RefCell<Vec<TraceEvent>>);

impl TraceSink for Collector {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

#[test]
fn presence_traces_its_condition_and_candidate_count() {
    let project = project(Node::SourceField {
        path: vec!["Keep".into()],
        frame: Some(vec!["Rows".into()]),
    });
    let collector = Collector::default();
    let context =
        ExecutionContext::new(std::path::Path::new("mapping.json")).with_trace_sink(&collector);
    run_with_context(&project, &input(&[(Some(false), Some("a"))]), &context).unwrap();
    let events = collector.0.into_inner();
    let finished = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::ScopeFinished {
                scope,
                candidates,
                produced,
                ..
            } if scope.target_path == ["Rows", "Item"] && scope.structural_path.len() == 3 => {
                Some((*candidates, *produced))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(finished, [(0, 0), (1, 1)]);
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::ScopeStarted {
            iteration: TraceIteration::OptionalPresence { condition: 4 },
            ..
        }
    )));
}

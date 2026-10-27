use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::Path;

use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, IterationOutput, Node, Project, Scope, ScopeConstruction, ScopeIteration,
};

use crate::{
    EngineError, ExecutionContext, TraceEvent, TraceSink, run, run_with_context, validate,
};

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn people(names: &[&str]) -> Instance {
    Instance::Repeated(
        names
            .iter()
            .map(|name| group(vec![("Name", text(name))]))
            .collect(),
    )
}

fn input(empty_parent: bool) -> Instance {
    let department =
        |name: &str, names: &[&str]| group(vec![("Name", text(name)), ("Person", people(names))]);
    let mut departments = vec![department("Alpha", &["Aster", "Birch", "Cedar"])];
    if empty_parent {
        departments.push(department("Empty", &[]));
    }
    departments.push(department("Beta", &["Elm"]));
    departments.push(department("Alpha", &["Fir", "Grove"]));
    group(vec![("Dept", Instance::Repeated(departments))])
}

fn project() -> Project {
    let source_person = SchemaNode::group(
        "Person",
        vec![SchemaNode::scalar("Name", ScalarType::String)],
    )
    .repeating();
    let nodes: BTreeMap<_, _> = [
        (
            1,
            Node::SourceField {
                path: vec!["Name".into()],
                frame: Some(vec!["Dept".into()]),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String("-".into()),
            },
        ),
        (
            4,
            Node::SourceField {
                path: vec!["Name".into()],
                frame: None,
            },
        ),
        (
            5,
            Node::Position {
                collection: vec!["Dept".into(), "Person".into()],
            },
        ),
        (
            6,
            Node::Const {
                value: Value::String(".xml".into()),
            },
        ),
        (
            8,
            Node::Call {
                function: "concat".into(),
                args: vec![1, 3, 5, 6],
            },
        ),
    ]
    .into_iter()
    .collect();
    let leaf = Scope {
        iteration: ScopeIteration::DynamicDocuments {
            source: vec!["Dept".into(), "Person".into()],
            output_path: 8,
        },
        group_into_blocks: Some(2),
        children: vec![Scope {
            target_field: "Person".into(),
            iteration: ScopeIteration::Source(vec!["Person".into()]),
            bindings: vec![Binding {
                target_field: "Name".into(),
                node: 4,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Dept",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        source_person,
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Person",
                    vec![SchemaNode::scalar("Name", ScalarType::String)],
                )
                .repeating(),
            ],
        ),
        graph: Graph { nodes },
        root: Scope {
            construction: ScopeConstruction::CollectDocuments,
            iteration: ScopeIteration::Source(vec!["Dept".into()]),
            group_by: Some(1),
            children: vec![leaf],
            ..Scope::default()
        },
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
    }
}

fn document(path: &str, names: &[&str]) -> DocumentMember {
    DocumentMember::new(path, group(vec![("Person", people(names))])).unwrap()
}

#[test]
fn nested_grouped_document_collection_preserves_paths_members_and_order() {
    let project = project();
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    assert_eq!(
        run(&project, &input(false)).unwrap(),
        Instance::DocumentSet(vec![
            document("Alpha-1.xml", &["Aster", "Birch"]),
            document("Alpha-2.xml", &["Cedar", "Fir"]),
            document("Alpha-3.xml", &["Grove"]),
            document("Beta-1.xml", &["Elm"]),
        ])
    );
    let encoded = mapping::project_file::encode_pretty(&project).unwrap();
    let restored: Project = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        mapping::project_file::encode_pretty(&restored).unwrap(),
        encoded
    );
}

#[test]
fn container_positions_count_selected_contexts_not_flattened_documents() {
    #[derive(Default)]
    struct Observations {
        positions: RefCell<Vec<usize>>,
        produced: Cell<Option<usize>>,
    }
    impl TraceSink for Observations {
        fn record(&self, event: TraceEvent) {
            match event {
                TraceEvent::TargetProduced {
                    scope, positions, ..
                } if scope.structural_path.is_empty() => {
                    let position = positions.last().unwrap();
                    assert_eq!(position.collection, vec!["Dept".to_owned()]);
                    assert!(position.grouped);
                    self.positions.borrow_mut().push(position.index);
                }
                TraceEvent::ScopeFinished {
                    scope, produced, ..
                } if scope.structural_path.is_empty() => self.produced.set(Some(produced)),
                _ => {}
            }
        }
    }
    let project = project();
    let observed = Observations::default();
    let context =
        ExecutionContext::new(Path::new("document-collection-test")).with_trace_sink(&observed);
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    assert_eq!(
        run_with_context(&project, &input(true), &context).unwrap(),
        Instance::DocumentSet(vec![
            document("Alpha-1.xml", &["Aster", "Birch"]),
            document("Alpha-2.xml", &["Cedar", "Fir"]),
            document("Alpha-3.xml", &["Grove"]),
            document("Beta-1.xml", &["Elm"]),
        ])
    );
    assert_eq!(*observed.positions.borrow(), vec![1, 2, 3]);
    assert_eq!(observed.produced.get(), Some(3));
}

#[test]
fn empty_document_collection_is_an_empty_document_set() {
    let project = project();
    assert_eq!(
        run(&project, &group(vec![("Dept", Instance::Repeated(vec![]))])).unwrap(),
        Instance::DocumentSet(vec![])
    );
}

#[test]
fn late_child_failure_does_not_return_partial_documents() {
    let mut project = project();
    project.graph.nodes.extend([
        (
            9,
            Node::Const {
                value: Value::String("Beta".into()),
            },
        ),
        (
            10,
            Node::Call {
                function: "equal".into(),
                args: vec![1, 9],
            },
        ),
        (
            11,
            Node::Const {
                value: Value::String("late document failure".into()),
            },
        ),
        (12, Node::Raise { message: Some(11) }),
        (
            13,
            Node::If {
                condition: 10,
                then: 12,
                else_: 8,
            },
        ),
    ]);
    assert!(project.root.children[0].set_output_path(Some(13)));
    assert_eq!(
        run(&project, &input(false)),
        Err(EngineError::MappingException {
            node: 12,
            message: Some("late document failure".into())
        })
    );
}

#[test]
fn document_collection_rejects_ambiguous_field_and_cardinality_shapes() {
    let mutations: [fn(&mut Project); 7] = [
        |p: &mut Project| p.root.iteration_output = IterationOutput::First,
        |p: &mut Project| p.root.iteration_output = IterationOutput::MappedSequence,
        |p: &mut Project| p.root.children.clear(),
        |p: &mut Project| {
            p.root.bindings.push(Binding {
                target_field: "unused".into(),
                node: 1,
            })
        },
        |p: &mut Project| p.root.children[0].target_field = "Person".into(),
        |p: &mut Project| {
            p.root.children[0].set_output_path(None);
        },
        |p: &mut Project| p.target_path = Some("fixed.xml".into()),
    ];
    for mutate in mutations {
        let mut project = project();
        mutate(&mut project);
        assert!(
            !validate(&project).is_empty(),
            "ambiguous document collector was admitted"
        );
    }
    let mut project = project();
    let mut collector = project.root.clone();
    collector.target_field = "Person".into();
    project.root = Scope {
        children: vec![collector],
        ..Scope::default()
    };
    assert!(
        validate(&project)
            .iter()
            .any(|issue| issue.message.contains("explicit document-root context"))
    );
}

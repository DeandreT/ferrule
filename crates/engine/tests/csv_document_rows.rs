use engine::{EngineError, run, validate};
use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{Graph, Node, Project, Scope, ScopeConstruction, ScopeIteration};

fn project() -> Project {
    let schema = SchemaNode::group(
        "Row",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Detail", ScalarType::String),
        ],
    );
    Project {
        source: schema.clone(),
        target: schema,
        source_path: Some("input.csv".into()),
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("chunk-".into()),
                    },
                ),
                (3, Node::Position { collection: vec![] }),
                (
                    4,
                    Node::Const {
                        value: Value::String(".csv".into()),
                    },
                ),
                (
                    5,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![2, 3, 4],
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec![],
                output_path: 5,
            },
            construction: ScopeConstruction::CopyCurrentRows,
            group_into_blocks: Some(1),
            ..Scope::default()
        },
    }
}

fn row(name: &str, detail: &str) -> Instance {
    Instance::Group(
        vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            (
                "Detail".into(),
                Instance::Scalar(Value::String(detail.into())),
            ),
        ]
        .into(),
    )
}

#[test]
fn grouped_csv_documents_keep_whole_rows_paths_and_uneven_final_block() {
    let project = project();
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let rows = vec![
        row("A", "one"),
        row("B", "two"),
        row("C", "three"),
        row("D", "four"),
        row("E", "five"),
    ];
    let expected = Instance::DocumentSet(vec![
        DocumentMember::new("chunk-1.csv", Instance::Repeated(rows[..2].to_vec())).unwrap(),
        DocumentMember::new("chunk-2.csv", Instance::Repeated(rows[2..4].to_vec())).unwrap(),
        DocumentMember::new("chunk-3.csv", Instance::Repeated(rows[4..].to_vec())).unwrap(),
    ]);
    assert_eq!(run(&project, &Instance::Repeated(rows)).unwrap(), expected);
    assert_eq!(
        run(&project, &Instance::Repeated(vec![])).unwrap(),
        Instance::DocumentSet(vec![])
    );
}

#[test]
fn row_copy_does_not_relax_ordinary_group_copy_or_accept_unbound_contexts() {
    for mode in 0..6 {
        let mut project = project();
        match mode {
            0 => project.root.construction = ScopeConstruction::CopyCurrentSource,
            1 => project.root.group_into_blocks = None,
            2 => project.root.iteration = ScopeIteration::Source(vec![]),
            3 => project.root.group_by = Some(3),
            4 => {
                let ir::SchemaKind::Group { children, .. } = &mut project.target.kind else {
                    unreachable!()
                };
                children[0].repeating = true;
            }
            5 => project.root.children.push(Scope::default()),
            _ => unreachable!(),
        }
        assert!(!validate(&project).is_empty(), "mode {mode}");
    }
}

#[test]
fn invalid_rows_and_block_sizes_fail_instead_of_dropping_data() {
    let mut project = project();
    for source in [row("A", "one"), Instance::DocumentSet(vec![])] {
        assert!(matches!(
            run(&project, &source),
            Err(EngineError::InvalidCurrentRows)
        ));
    }
    let source = Instance::Repeated(vec![row("A", "one"), Instance::Scalar(Value::Null)]);
    assert!(matches!(
        run(&project, &source),
        Err(EngineError::InvalidCurrentRows)
    ));
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Int(0),
        },
    );
    assert!(run(&project, &Instance::Repeated(vec![row("A", "one")])).is_err());
}

use super::*;
use ir::SchemaNode;
use mapping::{AggregateOp, Binding};

fn project(block_size: Value) -> Project {
    let graph = Graph {
        nodes: [
            (0, Node::Const { value: block_size }),
            (
                1,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            ),
            (
                2,
                Node::Aggregate {
                    function: AggregateOp::Sum,
                    collection: vec!["Row".into()],
                    value: vec!["Value".into()],
                    expression: None,
                    arg: None,
                },
            ),
            (
                3,
                Node::Position {
                    collection: vec!["Row".into()],
                },
            ),
            (
                4,
                Node::Call {
                    function: "greater_than".into(),
                    args: vec![1, 5],
                },
            ),
            (
                5,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
        ]
        .into_iter()
        .collect(),
    };
    let member = Scope {
        target_field: "Member".into(),
        iteration: mapping::ScopeIteration::Source(vec!["Row".into()]),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 1,
        }],
        ..Scope::default()
    };
    let block = Scope {
        target_field: "Block".into(),
        iteration: mapping::ScopeIteration::Source(vec!["Row".into()]),
        filter: Some(4),
        group_into_blocks: Some(0),
        bindings: vec![
            Binding {
                target_field: "First".into(),
                node: 1,
            },
            Binding {
                target_field: "Sum".into(),
                node: 2,
            },
            Binding {
                target_field: "Position".into(),
                node: 3,
            },
        ],
        children: vec![member],
        ..Scope::default()
    };
    Project {
        source: SchemaNode::group("Source", vec![]),
        target: SchemaNode::group("Target", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph,
        root: Scope {
            children: vec![block],
            ..Scope::default()
        },
    }
}

fn source() -> Instance {
    Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(
            (1..=5)
                .map(|value| {
                    Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(value)))])
                })
                .collect(),
        ),
    )])
}

fn scalar(instance: &Instance, field: &str) -> Value {
    instance
        .field(field)
        .and_then(Instance::as_scalar)
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn group_into_blocks_chunks_filtered_items_and_exposes_members() {
    let output = run(&project(Value::String("2".into())), &source()).unwrap();
    let blocks = output
        .field("Block")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(scalar(&blocks[0], "First"), Value::Int(2));
    assert_eq!(scalar(&blocks[0], "Sum"), Value::Int(5));
    assert_eq!(scalar(&blocks[0], "Position"), Value::Int(1));
    assert_eq!(scalar(&blocks[1], "First"), Value::Int(4));
    assert_eq!(scalar(&blocks[1], "Sum"), Value::Int(9));
    assert_eq!(scalar(&blocks[1], "Position"), Value::Int(2));

    let members = blocks[0]
        .field("Member")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(scalar(&members[0], "Value"), Value::Int(2));
    assert_eq!(scalar(&members[1], "Value"), Value::Int(3));
}

#[test]
fn group_into_blocks_rejects_nonpositive_and_missing_sizes() {
    assert_eq!(
        run(&project(Value::Int(0)), &source()),
        Err(EngineError::InvalidBlockSize { node: 0 })
    );
    assert_eq!(
        run(&project(Value::Int(-2)), &source()),
        Err(EngineError::InvalidBlockSize { node: 0 })
    );
    assert_eq!(
        run(&project(Value::Null), &source()),
        Err(EngineError::NotAnItemCount {
            node: 0,
            found: "null",
        })
    );
}

#[test]
fn group_into_blocks_rejects_a_second_grouping_mode() {
    let mut project = project(Value::Int(2));
    project.root.children[0].group_by = Some(1);
    assert_eq!(
        run(&project, &source()),
        Err(EngineError::ConflictingGroupingModes)
    );
    let issues = validate(&project);
    assert!(issues.iter().any(|issue| {
        issue
            .to_string()
            .contains("scope grouping modes are mutually exclusive")
    }));
}

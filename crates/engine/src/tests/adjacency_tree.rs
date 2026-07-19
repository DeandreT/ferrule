use std::collections::BTreeMap;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{AdjacencyTreePlan, Graph, Node, Project, Scope, ScopeConstruction};

use crate::{EngineError, run, validate};

#[test]
fn builds_reachable_adjacency_tree_in_source_order_and_omits_unreachable_cycles() {
    let project = project(None);
    let source = rows(&[
        ("Root", None),
        ("Beta", Some("Root")),
        ("Alpha", Some("Root")),
        ("Leaf", Some("Beta")),
        ("Unreachable", Some("Unreachable")),
    ]);

    assert!(validate(&project).is_empty());
    let output = run(&project, &source).unwrap();
    assert_eq!(string(&output, "name"), "Root");
    let children = repeated(&output, "type");
    assert_eq!(children.len(), 2);
    assert_eq!(string(&children[0], "name"), "Beta");
    assert_eq!(string(&children[1], "name"), "Alpha");
    let grandchildren = repeated(&children[0], "type");
    assert_eq!(grandchildren.len(), 1);
    assert_eq!(string(&grandchildren[0], "name"), "Leaf");
}

#[test]
fn reports_a_reachable_adjacency_cycle() {
    let project = project(Some((7, Value::String("Loop".into()))));
    assert_eq!(
        run(&project, &rows(&[("Loop", Some("Loop"))])),
        Err(EngineError::AdjacencyCycle("Loop".into()))
    );
}

#[test]
fn reports_the_typed_adjacency_depth_limit() {
    let project = project(None);
    let mut input = Vec::with_capacity(257);
    input.push(("node-0".to_string(), None));
    for index in 1..=256 {
        input.push((format!("node-{index}"), Some(format!("node-{}", index - 1))));
    }
    let rows = input
        .iter()
        .map(|(key, parent)| (key.as_str(), parent.as_deref()))
        .collect::<Vec<_>>();

    assert_eq!(
        run(&project, &self::rows(&rows)),
        Err(EngineError::AdjacencyTreeDepth { limit: 256 })
    );
}

#[test]
fn validates_root_and_source_and_recursive_target_paths() {
    let mut project = project(Some((99, Value::String("Root".into()))));
    project.graph.nodes.clear();
    project.target = SchemaNode::group(
        "type",
        vec![
            SchemaNode::scalar("name", ScalarType::Int),
            SchemaNode::group("type", Vec::new()).repeating(),
        ],
    );
    let issues = validate(&project);
    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("root references missing node 99"))
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("target key `name`"))
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("child field `type`"))
    );
}

fn project(root: Option<(u32, Value)>) -> Project {
    let root_id = root.as_ref().map(|(id, _)| *id);
    let graph = Graph {
        nodes: root
            .into_iter()
            .map(|(id, value)| (id, Node::Const { value }))
            .collect::<BTreeMap<_, _>>(),
    };
    Project {
        source: source_schema(),
        target: target_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            construction: ScopeConstruction::AdjacencyTree {
                plan: AdjacencyTreePlan::new(
                    vec!["type".into()],
                    vec!["name".into()],
                    vec!["base".into()],
                    "name".into(),
                    "type".into(),
                    root_id,
                )
                .unwrap(),
            },
            ..Scope::default()
        },
    }
}

fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "schema-types",
        vec![
            SchemaNode::group(
                "type",
                vec![
                    SchemaNode::scalar("name", ScalarType::String),
                    SchemaNode::scalar("base", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}

fn target_schema() -> SchemaNode {
    SchemaNode::group(
        "type",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::recursive_group("type", "type").repeating(),
        ],
    )
}

fn rows(rows: &[(&str, Option<&str>)]) -> Instance {
    Instance::Group(vec![(
        "type".into(),
        Instance::Repeated(
            rows.iter()
                .map(|(key, parent)| {
                    Instance::Group(vec![
                        (
                            "name".into(),
                            Instance::Scalar(Value::String((*key).into())),
                        ),
                        (
                            "base".into(),
                            Instance::Scalar(
                                parent
                                    .map(|value| Value::String(value.into()))
                                    .unwrap_or(Value::Null),
                            ),
                        ),
                    ])
                })
                .collect(),
        ),
    )])
}

fn string<'a>(instance: &'a Instance, field: &str) -> &'a str {
    let Some(Instance::Scalar(Value::String(value))) = instance.field(field) else {
        panic!("missing string field {field}");
    };
    value
}

fn repeated<'a>(instance: &'a Instance, field: &str) -> &'a [Instance] {
    let Some(Instance::Repeated(items)) = instance.field(field) else {
        panic!("missing repeated field {field}");
    };
    items
}

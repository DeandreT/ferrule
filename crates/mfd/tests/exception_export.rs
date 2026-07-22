use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, Graph, Node, Project, Scope,
    ScopeIteration, SequenceExpr,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_exception_export_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn source_project(message: Option<u32>) -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group("Item", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Result", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Source {
                collection: vec!["Item".into()],
            },
            selection: FailureSelection::WhenFalse { predicate: 2 },
            message,
        }],
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Int(10),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![0, 1],
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("limit exceeded".into()),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                filter: Some(2),
                bindings: vec![Binding {
                    target_field: "Result".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn nested_project(message_collection: Vec<String>) -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Outer",
                    vec![
                        SchemaNode::group(
                            "Item",
                            vec![SchemaNode::scalar("Value", ScalarType::Int)],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "OuterRow",
                    vec![
                        SchemaNode::group(
                            "Row",
                            vec![SchemaNode::scalar("Result", ScalarType::Int)],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Source {
                collection: vec!["Outer".into(), "Item".into()],
            },
            selection: FailureSelection::WhenFalse { predicate: 2 },
            message: Some(3),
        }],
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["Outer".into(), "Item".into()]),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Int(10),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![0, 1],
                    },
                ),
                (
                    3,
                    Node::Position {
                        collection: message_collection,
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "OuterRow".into(),
                iteration: ScopeIteration::Source(vec!["Outer".into()]),
                children: vec![Scope {
                    target_field: "Row".into(),
                    iteration: ScopeIteration::Source(vec!["Item".into()]),
                    filter: Some(2),
                    bindings: vec![Binding {
                        target_field: "Result".into(),
                        node: 0,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn component<'a>(document: &'a roxmltree::Document<'a>, name: &str) -> roxmltree::Node<'a, 'a> {
    document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
        .unwrap()
}

fn pin_key(component: roxmltree::Node<'_, '_>, side: &str, position: usize) -> Option<String> {
    component
        .children()
        .find(|node| node.has_tag_name(side))?
        .children()
        .filter(|node| node.has_tag_name("datapoint"))
        .enumerate()
        .find(|(index, node)| {
            node.attribute("pos")
                .and_then(|pos| pos.parse::<usize>().ok())
                .unwrap_or(*index)
                == position
        })
        .and_then(|(_, node)| node.attribute("key"))
        .map(str::to_string)
}

fn has_edge(document: &roxmltree::Document<'_>, from: &str, to: &str) -> bool {
    document.descendants().any(|vertex| {
        vertex.has_tag_name("vertex")
            && vertex.attribute("vertexkey") == Some(from)
            && vertex
                .descendants()
                .any(|edge| edge.has_tag_name("edge") && edge.attribute("vertexkey") == Some(to))
    })
}

fn has_incoming_edge(document: &roxmltree::Document<'_>, to: &str) -> bool {
    document
        .descendants()
        .any(|edge| edge.has_tag_name("edge") && edge.attribute("vertexkey") == Some(to))
}

fn has_outgoing_edge(document: &roxmltree::Document<'_>, from: &str) -> bool {
    document.descendants().any(|vertex| {
        vertex.has_tag_name("vertex")
            && vertex.attribute("vertexkey") == Some(from)
            && vertex.descendants().any(|edge| edge.has_tag_name("edge"))
    })
}

#[test]
fn false_scope_branch_exports_and_reimports_canonically() {
    let dir = TempDir::new("false");
    let design = dir.0.join("failure.mfd");
    let warnings = mfd::export(&source_project(Some(3)), &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let xml = std::fs::read_to_string(&design).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let filter = component(&document, "filter");
    let exception = component(&document, "exception");
    assert_eq!(exception.attribute("library"), Some("core"));
    assert_eq!(exception.attribute("kind"), Some("18"));
    assert!(
        exception
            .descendants()
            .any(|node| node.has_tag_name("wsdl"))
    );
    assert!(
        exception
            .descendants()
            .any(|node| node.has_tag_name("exception"))
    );

    let kept = pin_key(filter, "targets", 0).unwrap();
    let failed = pin_key(filter, "targets", 1).unwrap();
    let trigger = pin_key(exception, "sources", 0).unwrap();
    let message = pin_key(exception, "sources", 1).unwrap();
    assert!(has_outgoing_edge(&document, &kept));
    assert!(has_edge(&document, &failed, &trigger));
    assert!(has_incoming_edge(&document, &message));

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(imported.project.failure_rules.len(), 1);
    let rule = &imported.project.failure_rules[0];
    let FailureSelection::WhenFalse { predicate } = rule.selection else {
        panic!("expected a false-branch failure, got {:?}", rule.selection);
    };
    assert_eq!(
        imported.project.root.children[0].filter,
        Some(predicate),
        "failure must retain the target scope's complementary filter branch"
    );
    assert!(matches!(
        &rule.iteration,
        FailureIteration::Source { collection } if collection == &["Item"]
    ));
    assert!(matches!(
        rule.message.and_then(|node| imported.project.graph.nodes.get(&node)),
        Some(Node::Const {
            value: Value::String(message)
        }) if message == "limit exceeded"
    ));

    let roundtrip = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &roundtrip).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&roundtrip).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.failure_rules.len(), 1);
}

#[test]
fn optional_message_pin_remains_disconnected() {
    let dir = TempDir::new("optional-message");
    let design = dir.0.join("failure.mfd");
    let project = source_project(None);
    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let xml = std::fs::read_to_string(&design).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let exception = component(&document, "exception");
    let message = pin_key(exception, "sources", 1).unwrap();
    assert!(!has_incoming_edge(&document, &message));

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let [rule] = imported.project.failure_rules.as_slice() else {
        panic!("expected one imported failure rule");
    };
    assert!(matches!(
        &rule.iteration,
        FailureIteration::Source { collection } if collection == &["Item"]
    ));
    assert!(matches!(rule.selection, FailureSelection::WhenFalse { .. }));
    assert_eq!(rule.message, None);
}

#[test]
fn multiple_failures_share_one_false_filter_branch() {
    let dir = TempDir::new("shared-branch");
    let design = dir.0.join("failure.mfd");
    let mut project = source_project(Some(3));
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String("second failure".into()),
        },
    );
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["Item".into()],
        },
        selection: FailureSelection::WhenFalse { predicate: 2 },
        message: Some(4),
    });

    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let failed = pin_key(component(&document, "filter"), "targets", 1).unwrap();
    let exceptions = document
        .descendants()
        .filter(|node| {
            node.has_tag_name("component") && node.attribute("name") == Some("exception")
        })
        .collect::<Vec<_>>();
    assert_eq!(exceptions.len(), 2);
    for exception in exceptions {
        let trigger = pin_key(exception, "sources", 0).unwrap();
        assert!(has_edge(&document, &failed, &trigger));
    }

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.project.failure_rules.len(), 2);
    let messages = imported
        .project
        .failure_rules
        .iter()
        .filter_map(|rule| rule.message)
        .filter_map(|node| imported.project.graph.nodes.get(&node))
        .filter_map(|node| match node {
            Node::Const {
                value: Value::String(message),
            } => Some(message.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(messages, ["limit exceeded", "second failure"]);
}

#[test]
fn nested_failure_message_uses_the_absolute_item_position_context() {
    let dir = TempDir::new("nested-position");
    let design = dir.0.join("failure.mfd");
    let project = nested_project(vec!["Outer".into(), "Item".into()]);

    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let position = component(&document, "position");
    let position_input = pin_key(position, "sources", 0).unwrap();
    let position_output = pin_key(position, "targets", 0).unwrap();
    let message_input = pin_key(component(&document, "exception"), "sources", 1).unwrap();
    assert!(has_incoming_edge(&document, &position_input));
    assert!(has_edge(&document, &position_output, &message_input));

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(matches!(
        &imported.project.failure_rules[0].iteration,
        FailureIteration::Source { collection }
            if collection == &["Outer", "Item"]
    ));
}

#[test]
fn restricted_ancestor_and_ambiguous_position_reject_atomically() {
    let dir = TempDir::new("nested-rejections");

    let restricted_path = dir.0.join("restricted.mfd");
    std::fs::write(&restricted_path, "sentinel").unwrap();
    let mut restricted = nested_project(vec!["Outer".into(), "Item".into()]);
    restricted.root.children[0].filter = Some(4);
    let error = mfd::export(&restricted, &restricted_path).unwrap_err();
    assert!(error.to_string().contains("found 0"), "{error}");
    assert_eq!(
        std::fs::read_to_string(&restricted_path).unwrap(),
        "sentinel"
    );

    let position_path = dir.0.join("position.mfd");
    std::fs::write(&position_path, "sentinel").unwrap();
    let ambiguous = nested_project(vec!["Outer".into()]);
    let error = mfd::export(&ambiguous, &position_path).unwrap_err();
    assert!(
        error.to_string().contains("no unambiguous failure-item"),
        "{error}"
    );
    assert_eq!(std::fs::read_to_string(&position_path).unwrap(), "sentinel");
}

#[test]
fn noncanonical_failure_shapes_are_rejected_atomically() {
    let dir = TempDir::new("unsupported");
    for (label, selection, iteration, expected) in [
        (
            "all",
            FailureSelection::All,
            FailureIteration::Source {
                collection: vec!["Item".into()],
            },
            "unconditional failures",
        ),
        (
            "true",
            FailureSelection::WhenTrue { predicate: 2 },
            FailureIteration::Source {
                collection: vec!["Item".into()],
            },
            "complementary false-branch",
        ),
        (
            "sequence",
            FailureSelection::WhenFalse { predicate: 2 },
            FailureIteration::Sequence {
                sequence: SequenceExpr::Tokenize {
                    input: 3,
                    delimiter: 3,
                    item: 0,
                },
            },
            "generated-sequence failures",
        ),
    ] {
        let design = dir.0.join(format!("{label}.mfd"));
        std::fs::write(&design, "sentinel").unwrap();
        let mut project = source_project(Some(3));
        project.failure_rules[0].selection = selection;
        project.failure_rules[0].iteration = iteration;
        let error = mfd::export(&project, &design).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        assert_eq!(std::fs::read_to_string(&design).unwrap(), "sentinel");
    }
}

#[test]
fn invalid_failure_reference_is_rejected_before_replacing_artifacts() {
    let dir = TempDir::new("invalid");
    let design = dir.0.join("failure.mfd");
    std::fs::write(&design, "sentinel").unwrap();
    let mut project = source_project(Some(999));
    project.source_path = Some("missing-source.xml".into());

    let error = mfd::export(&project, &design).unwrap_err();
    assert!(
        error.to_string().contains("missing message node 999"),
        "{error}"
    );
    assert_eq!(std::fs::read_to_string(&design).unwrap(), "sentinel");
    assert!(!dir.0.join("failure-source.xsd").exists());
    assert!(!dir.0.join("failure-target.xsd").exists());
}

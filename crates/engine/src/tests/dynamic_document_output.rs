use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

use crate::{EngineError, run, validate};

fn project(output_path: u32) -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("records-*.xml".into()),
        target_path: None,
        source_options: mapping::FormatOptions {
            xml_document: true,
            local_xml_file_set: true,
            ..mapping::FormatOptions::default()
        },
        target_options: mapping::FormatOptions {
            xml_document: true,
            ..mapping::FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: [
                (0, Node::SourceDocumentPath),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: Vec::new(),
                output_path,
            },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

fn member(path: &str, value: &str) -> DocumentMember {
    let instance = Instance::Group(vec![(
        "Value".into(),
        Instance::Scalar(Value::String(value.into())),
    )]);
    DocumentMember::new(path, instance).unwrap_or_else(|| panic!("valid member {path}"))
}

#[test]
fn pairs_each_document_with_its_path_in_the_same_item_context() {
    let project = project(0);
    assert!(validate(&project).is_empty());
    let source = Instance::DocumentSet(vec![member("a.xml", "A"), member("b.xml", "B")]);

    let output = run(&project, &source).unwrap_or_else(|error| panic!("run failed: {error}"));
    let Instance::DocumentSet(documents) = output else {
        panic!("expected document set output")
    };
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].path(), "a.xml");
    assert_eq!(
        documents[0]
            .value()
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("A".into()))
    );
    assert_eq!(documents[1].path(), "b.xml");
    assert_eq!(
        documents[1]
            .value()
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::String("B".into()))
    );
}

#[test]
fn rejects_missing_nested_and_static_path_combinations() {
    let mut missing = project(99);
    assert!(
        validate(&missing)
            .iter()
            .any(|issue| issue.message.contains("missing node 99"))
    );

    missing.root.iteration = ScopeIteration::Source(Vec::new());
    missing.root.children.push(Scope {
        target_field: "nested".into(),
        iteration: ScopeIteration::DynamicDocuments {
            source: Vec::new(),
            output_path: 0,
        },
        ..Scope::default()
    });
    assert!(
        validate(&missing)
            .iter()
            .any(|issue| { issue.message.contains("valid only on a project root scope") })
    );

    let mut static_path = project(0);
    static_path.target_path = Some("fixed.xml".into());
    assert!(validate(&static_path).iter().any(|issue| {
        issue
            .message
            .contains("cannot be combined with a stored target path")
    }));
}

#[test]
fn reports_a_typed_error_for_non_string_paths() {
    let mut project = project(2);
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Int(7),
        },
    );
    let source = Instance::DocumentSet(vec![member("a.xml", "A")]);

    assert_eq!(
        run(&project, &source),
        Err(EngineError::DynamicTargetPath {
            node: 2,
            found: "int",
        })
    );
}

#[test]
fn reports_a_typed_error_for_empty_paths() {
    let mut project = project(2);
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String(String::new()),
        },
    );
    let source = Instance::DocumentSet(vec![member("a.xml", "A")]);

    assert_eq!(
        run(&project, &source),
        Err(EngineError::EmptyDynamicTargetPath { node: 2 })
    );
}

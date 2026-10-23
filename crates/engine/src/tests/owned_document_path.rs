//! #451 owned document-set paths: members root their owner inside the scope
//! walking them, and a document path never comes from another owner.
use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, NamedSource, Node, OwnedPosition, OwnedSourceDocumentPath, OwnedSourceField,
    OwnedSourceRef, Project, Scope, ScopeIteration, SourceOwner,
};

use crate::{EngineError, OwnedSourceError};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn rows(values: &[&str]) -> Instance {
    Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(
                values
                    .iter()
                    .map(|value| Instance::Group(vec![("Value".into(), text(value))].into()))
                    .collect(),
            ),
        )]
        .into(),
    )
}

fn set(members: &[(&str, &[&str])]) -> Instance {
    Instance::DocumentSet(
        members
            .iter()
            .map(|(path, values)| DocumentMember::new(*path, rows(values)).expect("member"))
            .collect(),
    )
}

fn named() -> SourceOwner {
    SourceOwner::Named { name: "Ref".into() }
}

fn iterate(owner: SourceOwner, collection: &[&str]) -> ScopeIteration {
    ScopeIteration::OwnedSource {
        source: OwnedSourceRef {
            owner,
            path: path(collection),
        },
        output_path: None,
    }
}

fn bind(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}

fn endpoint() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    )
}

/// Docs walk primary documents; Rows nest a second walk of the same owner
/// and bind its document path, an owned value and the root position.
fn project(position_collection: Option<Vec<String>>) -> Project {
    let s = |name| SchemaNode::scalar(name, ScalarType::String);
    let mut graph = Graph::default();
    graph.nodes.insert(
        1,
        Node::OwnedSourceDocumentPath(OwnedSourceDocumentPath {
            owner: SourceOwner::Primary,
        }),
    );
    graph.nodes.insert(
        2,
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows", "Value"]),
            },
            frame: None,
        }),
    );
    graph.nodes.insert(
        3,
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: position_collection,
        }),
    );
    graph.nodes.insert(
        4,
        Node::OwnedSourceDocumentPath(OwnedSourceDocumentPath { owner: named() }),
    );
    Project {
        source: endpoint(),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Docs",
                    vec![
                        s("Path"),
                        SchemaNode::group(
                            "Rows",
                            vec![
                                s("Path"),
                                s("Value"),
                                SchemaNode::scalar("Position", ScalarType::Int),
                            ],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Ref".into(),
            path: "unused.xml".into(),
            schema: endpoint(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Docs".into(),
                iteration: iterate(SourceOwner::Primary, &[]),
                bindings: vec![bind("Path", 1)],
                children: vec![Scope {
                    target_field: "Rows".into(),
                    iteration: iterate(SourceOwner::Primary, &["Rows"]),
                    bindings: vec![bind("Path", 1), bind("Value", 2), bind("Position", 3)],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

/// One output row's document path, value and position.
type Row = (String, String, i64);

fn docs(output: &Instance) -> Vec<(String, Vec<Row>)> {
    let field = |instance: &Instance, name: &str| match instance.field(name) {
        Some(Instance::Scalar(Value::String(value))) => value.clone(),
        other => panic!("{name}: {other:?}"),
    };
    let Some(Instance::Repeated(docs)) = output.field("Docs") else {
        panic!("docs: {output:?}");
    };
    docs.iter()
        .map(|doc| {
            let Some(Instance::Repeated(rows)) = doc.field("Rows") else {
                panic!("rows: {doc:?}");
            };
            let rows = rows
                .iter()
                .map(|row| {
                    let Some(Instance::Scalar(Value::Int(position))) = row.field("Position") else {
                        panic!("position: {row:?}");
                    };
                    (field(row, "Path"), field(row, "Value"), *position)
                })
                .collect();
            (field(doc, "Path"), rows)
        })
        .collect()
}

#[test]
fn members_root_their_owner_and_nested_walks_never_reenter_the_set() {
    let project = project(Some(path(&["Rows"])));
    let source = set(&[("a.xml", &["a1", "a2"]), ("b.xml", &["b1"])]);
    let named_set = set(&[("n.xml", &["n1"])]);
    let output = crate::run_with_sources(&project, &source, vec![("Ref".into(), named_set)])
        .expect("document-set walk");
    let row = |path: &str, value: &str, position| (path.to_owned(), value.to_owned(), position);
    assert_eq!(
        docs(&output),
        vec![
            (
                "a.xml".to_owned(),
                vec![row("a.xml", "a1", 1), row("a.xml", "a2", 2)]
            ),
            ("b.xml".to_owned(), vec![row("b.xml", "b1", 1)]),
        ]
    );
}

#[test]
fn a_document_is_never_a_position() {
    let project = project(Some(vec![]));
    let source = set(&[("a.xml", &["a1"])]);
    assert_eq!(
        crate::run_with_sources(&project, &source, vec![]),
        Err(EngineError::OwnedSourceAccess {
            node: Some(3),
            scope: None,
            source: OwnedSourceError::InactiveSourceFrame {
                owner: SourceOwner::Primary,
                collection: Some(vec![]),
            },
        })
    );
}

#[test]
fn another_owners_document_never_answers() {
    let mut project = project(None);
    project.root.children[0].bindings = vec![bind("Path", 4)];
    let source = set(&[("a.xml", &["a1"])]);
    let named_set = set(&[("n.xml", &["n1"])]);
    assert_eq!(
        crate::run_with_sources(&project, &source, vec![("Ref".into(), named_set)]),
        Err(EngineError::OwnedSourceAccess {
            node: Some(4),
            scope: None,
            source: OwnedSourceError::MissingDocumentPath { owner: named() },
        })
    );
}

#[test]
fn the_legacy_document_path_keeps_its_first_member_fallback() {
    let mut project = project(None);
    project.graph.nodes.insert(1, Node::SourceDocumentPath);
    project.root.children.clear();
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Path", ScalarType::String)],
    );
    project.root.bindings = vec![bind("Path", 1)];
    let source = set(&[("a.xml", &["a1"]), ("b.xml", &["b1"])]);
    let output = crate::run(&project, &source).expect("legacy document path");
    assert_eq!(output.field("Path"), Some(&text("a.xml")));
}

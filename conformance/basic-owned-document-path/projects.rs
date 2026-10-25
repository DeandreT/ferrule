//! Genuine public Project definitions for owned document-set paths. Authored
//! source only: no mapping, document or path is evaluated here.
use super::*;
use mapping::{
    NamedSource, OwnedPosition, OwnedSourceDocumentPath, OwnedSourceField, OwnedSourceRef,
    SourceOwner,
};

pub const NAMED: &str = "Ref";

/// Model order is the `model` index the generated hosts receive.
pub const MODELS: [&str; 4] = ["nested", "cross", "root", "sorted"];

fn named() -> SourceOwner {
    SourceOwner::Named { name: NAMED.into() }
}

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

fn document_path(owner: SourceOwner) -> Node {
    Node::OwnedSourceDocumentPath(OwnedSourceDocumentPath { owner })
}

fn owned_value(owner: SourceOwner) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(&["Rows", "Value"]),
        },
        frame: None,
    })
}

fn owned_title(owner: SourceOwner, frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(&["Title"]),
        },
        frame: frame.map(path),
    })
}

fn owned_position(owner: SourceOwner) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner,
        collection: Some(path(&["Rows"])),
    })
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

/// Both endpoints share one schema, so every path collides across owners.
fn endpoint() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            string("Title"),
            SchemaNode::group("Rows", vec![string("Value")]).repeating(),
        ],
    )
}

fn repeating(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, children).repeating()
}

/// Graph nodes shared by every model: 1 and 2 read the primary and named
/// document paths, 3 to 6 the owned row values and positions, 7 the primary
/// title through an explicit root pin and 8 the named title without a pin.
fn graph() -> Graph {
    let mut graph = Graph::default();
    graph.nodes.insert(1, document_path(SourceOwner::Primary));
    graph.nodes.insert(2, document_path(named()));
    graph.nodes.insert(3, owned_value(SourceOwner::Primary));
    graph.nodes.insert(4, owned_position(SourceOwner::Primary));
    graph.nodes.insert(5, owned_value(named()));
    graph.nodes.insert(6, owned_position(named()));
    graph
        .nodes
        .insert(7, owned_title(SourceOwner::Primary, Some(&[])));
    graph.nodes.insert(8, owned_title(named(), None));
    graph
}

/// `nested` walks each primary document, its rows, and inside every primary
/// row each named document's rows. `cross` walks only named documents and
/// reads the uniterated primary path; `root` reads the primary path with no
/// active document; `sorted` walks the rows of every primary document as one
/// sorted collection.
pub fn project(model: &str) -> Project {
    let (target, root) = match model {
        "nested" => (
            SchemaNode::group(
                "Output",
                vec![repeating(
                    "Docs",
                    vec![
                        string("Path"),
                        string("Title"),
                        repeating(
                            "Rows",
                            vec![
                                string("Value"),
                                int("Position"),
                                string("Path"),
                                repeating(
                                    "Pairs",
                                    vec![
                                        string("Value"),
                                        int("Position"),
                                        string("NamedPath"),
                                        string("PrimaryPath"),
                                        string("NamedTitle"),
                                        string("PrimaryTitle"),
                                    ],
                                ),
                            ],
                        ),
                    ],
                )],
            ),
            Scope {
                children: vec![Scope {
                    target_field: "Docs".into(),
                    iteration: iterate(SourceOwner::Primary, &[]),
                    bindings: vec![bind("Path", 1), bind("Title", 7)],
                    children: vec![Scope {
                        target_field: "Rows".into(),
                        iteration: iterate(SourceOwner::Primary, &["Rows"]),
                        bindings: vec![bind("Value", 3), bind("Position", 4), bind("Path", 1)],
                        children: vec![Scope {
                            target_field: "Pairs".into(),
                            iteration: iterate(named(), &["Rows"]),
                            bindings: vec![
                                bind("Value", 5),
                                bind("Position", 6),
                                bind("NamedPath", 2),
                                bind("PrimaryPath", 1),
                                bind("NamedTitle", 8),
                                bind("PrimaryTitle", 7),
                            ],
                            ..Scope::default()
                        }],
                        ..Scope::default()
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            },
        ),
        "cross" => (
            SchemaNode::group(
                "Output",
                vec![repeating(
                    "NamedDocs",
                    vec![string("NamedPath"), string("PrimaryPath")],
                )],
            ),
            Scope {
                children: vec![Scope {
                    target_field: "NamedDocs".into(),
                    iteration: iterate(named(), &[]),
                    bindings: vec![bind("NamedPath", 2), bind("PrimaryPath", 1)],
                    ..Scope::default()
                }],
                ..Scope::default()
            },
        ),
        "root" => (
            SchemaNode::group("Output", vec![string("Path")]),
            Scope {
                bindings: vec![bind("Path", 1)],
                ..Scope::default()
            },
        ),
        "sorted" => (
            SchemaNode::group(
                "Output",
                vec![repeating(
                    "Rows",
                    vec![string("Value"), int("Position"), string("Path")],
                )],
            ),
            Scope {
                children: vec![Scope {
                    target_field: "Rows".into(),
                    iteration: iterate(SourceOwner::Primary, &["Rows"]),
                    sort_by: Some(3),
                    sort_descending: true,
                    bindings: vec![bind("Value", 3), bind("Position", 4), bind("Path", 1)],
                    ..Scope::default()
                }],
                ..Scope::default()
            },
        ),
        _ => panic!("authored model"),
    };
    Project {
        source: endpoint(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: NAMED.into(),
            path: "unused-basic-document-path-input.xml".into(),
            schema: endpoint(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: graph(),
        root,
    }
}

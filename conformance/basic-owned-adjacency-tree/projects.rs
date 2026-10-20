//! Models for the owned adjacency-tree corpus (#466). Each model is a complete
//! Project; `cases.json` refers to these keys.
use ir::{ScalarType, SchemaNode};
use mapping::{
    AdjacencyTreePlan, Graph, NamedSource, NamedTarget, Node, NodeId, OwnedAdjacencyTreePlan,
    OwnedSourceField, OwnedSourceRef, Project, Scope, ScopeConstruction, ScopeIteration,
    SourceOwner,
};

pub const MODELS: &[&str] = &[
    "A01-endpoints",
    "A02-root",
    "A03-nested",
    "A04-root-collection",
];

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

/// One adjacency row: a key and a parent nested one level below the row.
pub fn row_schema() -> SchemaNode {
    SchemaNode::group(
        "Row",
        vec![
            string("Key"),
            SchemaNode::group("Link", vec![string("Parent")]),
        ],
    )
    .repeating()
}

/// The same declared shape is used by the primary and the named endpoints so
/// that only the owner can distinguish their rows.
pub fn input_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            string("Root"),
            SchemaNode::group("Rows", vec![row_schema()]),
            SchemaNode::group("Batch", vec![string("Start"), row_schema()]).repeating(),
        ],
    )
}

pub fn node(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("name"),
            SchemaNode::recursive_group("children", name).repeating(),
        ],
    )
}

fn batches_schema() -> SchemaNode {
    SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Batch", vec![node("Tree"), node("Audit")]).repeating()],
    )
}

pub fn owner(name: Option<&str>) -> SourceOwner {
    name.map_or(SourceOwner::Primary, |name| SourceOwner::Named {
        name: name.into(),
    })
}

pub fn tree(owner: SourceOwner, rows: &[&str], root: Option<NodeId>) -> Scope {
    let plan = OwnedAdjacencyTreePlan::new(
        OwnedSourceRef {
            owner,
            path: path(rows),
        },
        path(&["Key"]),
        path(&["Link", "Parent"]),
        "name".into(),
        "children".into(),
        root,
    )
    .expect("authored owned plan");
    Scope {
        construction: ScopeConstruction::OwnedAdjacencyTree { plan },
        ..Scope::default()
    }
}

pub fn legacy_tree(rows: &[&str], root: Option<NodeId>) -> Scope {
    let plan = AdjacencyTreePlan::new(
        path(rows),
        path(&["Key"]),
        path(&["Link", "Parent"]),
        "name".into(),
        "children".into(),
        root,
    )
    .expect("authored legacy plan");
    Scope {
        construction: ScopeConstruction::AdjacencyTree { plan },
        ..Scope::default()
    }
}

/// An owned scalar read; `frame` pins it to an active same-owner batch.
pub fn owned_field(owner: SourceOwner, field: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(field),
        },
        frame: frame.map(path),
    })
}

fn named(name: &str, schema: SchemaNode) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema,
        options: Default::default(),
        dynamic_path: None,
    }
}

fn target(name: &str, schema: SchemaNode, root: Scope) -> NamedTarget {
    NamedTarget {
        name: name.into(),
        path: Some(format!("{name}.json")),
        schema,
        options: Default::default(),
        root,
    }
}

pub fn base(target: SchemaNode, root: Scope) -> Project {
    Project {
        source: input_schema(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            named("audit", input_schema()),
            named("Rows", input_schema()),
        ],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root,
    }
}

/// One owned iteration over `iterated`'s batches: a pinned same-owner tree
/// rooted at the batch's Start, and the other endpoint's complete tree.
fn batches(iterated: Option<&str>, other: Option<&str>, start: NodeId) -> Scope {
    Scope {
        children: vec![Scope {
            target_field: "Batch".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: owner(iterated),
                    path: path(&["Batch"]),
                },
                output_path: None,
            },
            children: vec![
                Scope {
                    target_field: "Tree".into(),
                    ..tree(owner(iterated), &["Batch", "Row"], Some(start))
                },
                Scope {
                    target_field: "Audit".into(),
                    ..tree(owner(other), &["Batch", "Row"], None)
                },
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    }
}

pub fn project(model: &str) -> Project {
    match model {
        "A01-endpoints" => {
            let mut project = base(node("node"), tree(owner(None), &["Rows", "Row"], None));
            project.extra_targets = vec![
                target(
                    "audit",
                    node("node"),
                    tree(owner(Some("audit")), &["Rows", "Row"], None),
                ),
                // The named endpoint `Rows` shares a primary field name.
                target(
                    "alias",
                    node("node"),
                    tree(owner(Some("Rows")), &["Rows", "Row"], None),
                ),
            ];
            project
        }
        // Each root reads the receiving context, never the selected rows.
        "A02-root" => {
            let mut project = base(
                node("node"),
                tree(owner(Some("audit")), &["Rows", "Row"], Some(1)),
            );
            project.graph.nodes.extend([
                (1, owned_field(owner(None), &["Root"], None)),
                (
                    2,
                    Node::SourceField {
                        path: path(&["Root"]),
                        frame: None,
                    },
                ),
                (3, owned_field(owner(Some("audit")), &["Root"], None)),
            ]);
            project.extra_targets = vec![
                target(
                    "legacy-root",
                    node("node"),
                    tree(owner(None), &["Rows", "Row"], Some(2)),
                ),
                target(
                    "named-root",
                    node("node"),
                    tree(owner(None), &["Rows", "Row"], Some(3)),
                ),
            ];
            project
        }
        "A03-nested" => {
            let mut project = base(batches_schema(), batches(None, Some("audit"), 1));
            project.graph.nodes.extend([
                (
                    1,
                    owned_field(owner(None), &["Batch", "Start"], Some(&["Batch"])),
                ),
                (
                    2,
                    owned_field(owner(Some("audit")), &["Batch", "Start"], Some(&["Batch"])),
                ),
            ]);
            project.extra_targets = vec![target(
                "audit",
                batches_schema(),
                batches(Some("audit"), None, 2),
            )];
            project
        }
        // A named endpoint whose root is itself the repeating row collection.
        "A04-root-collection" => {
            let mut project = base(node("node"), tree(owner(Some("rows")), &[], None));
            project.extra_sources.push(named("rows", row_schema()));
            project
        }
        _ => panic!("unknown owned adjacency-tree model {model}"),
    }
}

/// The ordinary saved control keeps the legacy plan and its outward fallback.
pub fn legacy() -> Project {
    base(node("node"), legacy_tree(&["Rows", "Row"], None))
}

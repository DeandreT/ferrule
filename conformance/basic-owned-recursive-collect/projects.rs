//! Genuine public Projects for owned recursive collection walks (#463).
//!
//! Primary and `Lookup` share one endpoint schema, so every owned root path
//! also exists under the other owner. `Tree` is a named endpoint whose root is
//! one directory group and `Forest` a named repeated root of directories.
//! Expected results live separately in `cases.rs`.
use super::*;
use mapping::{
    FailureIteration, FailureRule, FailureSelection, NamedSource, NamedTarget,
    OwnedAdjacencyTreePlan, OwnedRecursiveCollect, OwnedSourceField, OwnedSourceRef, SequenceExpr,
    SourceOwner,
};

pub const LOOKUP: &str = "Lookup";
pub const TREE: &str = "Tree";
pub const FOREST: &str = "Forest";
pub const AUDIT: &str = "audit";

pub const PROFILES: &[&str] = &[
    "R01-primary-dirs",
    "R02-named-dirs",
    "R03-named-root-tree",
    "R04-named-repeated-root",
    "R05-owned-parent-arguments",
    "R06-owned-index",
    "R07-nested-pinned",
    "R08-failure-and-scalar-producers",
    "R09-adjacency-tree-root",
];

/// The ItemAt node bound to the audit target's `First` field.
pub const AUDIT_NODE: u32 = 90;
/// The first ItemAt node bound in the Primary target.
pub const FIRST_NODE: u32 = 101;

pub fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
pub fn owner(name: &str) -> SourceOwner {
    if name == "Primary" {
        SourceOwner::Primary
    } else {
        SourceOwner::Named { name: name.into() }
    }
}
fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
/// One recursive directory: its descent `Note`, leaf files and child groups.
pub fn directory_schema() -> SchemaNode {
    SchemaNode::group(
        "Dir",
        vec![
            string("Note"),
            SchemaNode::group("file", vec![string("Note")]).repeating(),
            SchemaNode::recursive_group("Dir", "Dir").repeating(),
        ],
    )
}
pub fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("Note"),
            string("Sep"),
            SchemaNode::scalar("Pick", ScalarType::Int),
            directory_schema().repeating(),
            // Adjacency rows: a key and a parent key.
            SchemaNode::group("Rows", vec![string("Key"), string("Parent")]).repeating(),
            // Batch directories reference the top-level `Dir` anchor.
            SchemaNode::group(
                "Batches",
                vec![
                    string("Id"),
                    SchemaNode::recursive_group("Dir", "Dir").repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
fn owned_field(name: &str, parts: &[&str]) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: owner(name),
            path: path(parts),
        },
        frame: None,
    })
}
fn generated_item() -> Node {
    Node::SourceField {
        path: vec![],
        frame: None,
    }
}
fn walk(name: &str, parts: &[&str], prefix: u32, separator: u32, item: u32) -> SequenceExpr {
    SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
        collection: OwnedSourceRef {
            owner: owner(name),
            path: path(parts),
        },
        children: path(&["Dir"]),
        descent_value: path(&["Note"]),
        values: path(&["file"]),
        value: path(&["Note"]),
        prefix,
        separator,
        item,
    })
}

/// Shared nodes: prefix 1, separator 2 and indexes 3-7 (1 to 5). Each ItemAt
/// `id` owns the private generated item `id + 100`.
struct Builder {
    project: Project,
}
impl Builder {
    fn item_at(&mut self, id: u32, sequence: SequenceExpr, index: u32) -> u32 {
        let nodes = &mut self.project.graph.nodes;
        nodes.insert(sequence.item(), generated_item());
        nodes.insert(id, Node::SequenceItemAt { sequence, index });
        id
    }
    fn walk_at(&mut self, id: u32, name: &str, parts: &[&str], index: u32) -> u32 {
        self.item_at(id, walk(name, parts, 1, 2, id + 100), index)
    }
}
fn bindings(fields: &[(&str, u32)]) -> Vec<Binding> {
    fields
        .iter()
        .map(|(field, node)| Binding {
            target_field: (*field).into(),
            node: *node,
        })
        .collect()
}
fn output(fields: &[(&str, u32)]) -> (SchemaNode, Scope) {
    (
        SchemaNode::group("Out", fields.iter().map(|(name, _)| string(name)).collect()),
        Scope {
            bindings: bindings(fields),
            ..Scope::default()
        },
    )
}
fn named_source(name: &str, schema: SchemaNode) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema,
        options: Default::default(),
        dynamic_path: None,
    }
}

/// The audit target reads the first Lookup walk value in every Project.
pub fn base() -> Project {
    // Each named root is itself the recursive `Dir` anchor.
    let tree = directory_schema();
    let forest = directory_schema().repeating();
    let mut builder = Builder {
        project: Project {
            source: endpoint_schema("Input"),
            target: SchemaNode::group("Out", vec![]),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![
                named_source(LOOKUP, endpoint_schema(LOOKUP)),
                named_source(TREE, tree),
                named_source(FOREST, forest),
            ],
            extra_targets: vec![NamedTarget {
                name: AUDIT.into(),
                path: None,
                schema: SchemaNode::group("Audit", vec![string("First")]),
                options: Default::default(),
                root: Scope {
                    bindings: bindings(&[("First", AUDIT_NODE)]),
                    ..Scope::default()
                },
            }],
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph::default(),
            root: Scope::default(),
        },
    };
    let nodes = &mut builder.project.graph.nodes;
    nodes.insert(
        1,
        Node::Const {
            value: Value::String(String::new()),
        },
    );
    nodes.insert(
        2,
        Node::Const {
            value: Value::String("/".into()),
        },
    );
    for index in 1..=5 {
        nodes.insert(
            2 + index,
            Node::Const {
                value: Value::Int(i64::from(index)),
            },
        );
    }
    builder.walk_at(AUDIT_NODE, LOOKUP, &["Dir"], 3);
    builder.project
}

/// Five ItemAt bindings over one walk, so each output is the complete
/// sequence up to five values.
fn five(builder: &mut Builder, name: &str, parts: &[&str]) -> Vec<(&'static str, u32)> {
    ["V1", "V2", "V3", "V4", "V5"]
        .into_iter()
        .zip(0..)
        .map(|(field, offset)| {
            (
                field,
                builder.walk_at(FIRST_NODE + offset, name, parts, 3 + offset),
            )
        })
        .collect()
}

pub fn project(profile: &str) -> Project {
    let mut builder = Builder { project: base() };
    let fields = match profile {
        "R01-primary-dirs" => five(&mut builder, "Primary", &["Dir"]),
        "R02-named-dirs" => five(&mut builder, LOOKUP, &["Dir"]),
        "R03-named-root-tree" => five(&mut builder, TREE, &[]),
        "R04-named-repeated-root" => five(&mut builder, FOREST, &[]),
        "R05-owned-parent-arguments" => {
            // Both parent phases read colliding owners: Lookup's Note and
            // Primary's Sep, although each endpoint declares both fields.
            let nodes = &mut builder.project.graph.nodes;
            nodes.insert(10, owned_field(LOOKUP, &["Note"]));
            nodes.insert(11, owned_field("Primary", &["Sep"]));
            (0..3)
                .map(|offset| {
                    let id = FIRST_NODE + offset;
                    let sequence = walk("Primary", &["Dir"], 10, 11, id + 100);
                    (
                        ["V1", "V2", "V3"][offset as usize],
                        builder.item_at(id, sequence, 3 + offset),
                    )
                })
                .collect()
        }
        "R06-owned-index" => {
            let nodes = &mut builder.project.graph.nodes;
            nodes.insert(10, owned_field(LOOKUP, &["Pick"]));
            nodes.insert(11, owned_field("Primary", &["Pick"]));
            vec![
                ("V1", builder.walk_at(FIRST_NODE, "Primary", &["Dir"], 10)),
                ("V2", builder.walk_at(FIRST_NODE + 1, LOOKUP, &["Dir"], 11)),
            ]
        }
        "R07-nested-pinned" => {
            builder
                .project
                .graph
                .nodes
                .insert(10, owned_field("Primary", &["Batches", "Id"]));
            let batch = [
                ("Id", 10),
                // The active Primary batch pins the Primary root collection.
                (
                    "Own1",
                    builder.walk_at(FIRST_NODE, "Primary", &["Batches", "Dir"], 3),
                ),
                (
                    "Own2",
                    builder.walk_at(FIRST_NODE + 1, "Primary", &["Batches", "Dir"], 4),
                ),
                // No Lookup frame is active, so Lookup walks every batch.
                (
                    "Other1",
                    builder.walk_at(FIRST_NODE + 2, LOOKUP, &["Batches", "Dir"], 3),
                ),
                (
                    "Other2",
                    builder.walk_at(FIRST_NODE + 3, LOOKUP, &["Batches", "Dir"], 4),
                ),
                // A frame outside the root path never narrows the walk.
                (
                    "Top",
                    builder.walk_at(FIRST_NODE + 4, "Primary", &["Dir"], 3),
                ),
            ];
            let target = SchemaNode::group(
                "Out",
                vec![
                    SchemaNode::group(
                        "Batches",
                        batch.iter().map(|(name, _)| string(name)).collect(),
                    )
                    .repeating(),
                ],
            );
            builder.project.target = target;
            builder.project.root = Scope {
                children: vec![Scope {
                    target_field: "Batches".into(),
                    iteration: ScopeIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: owner("Primary"),
                            path: path(&["Batches"]),
                        },
                        output_path: None,
                    },
                    bindings: bindings(&batch),
                    ..Scope::default()
                }],
                ..Scope::default()
            };
            return builder.project;
        }
        "R08-failure-and-scalar-producers" => {
            // An owned failure walk and a Generate ItemAt beside the owned producer.
            let nodes = &mut builder.project.graph.nodes;
            nodes.insert(10, owned_field(LOOKUP, &["Pick"]));
            nodes.insert(11, owned_field(LOOKUP, &["Dir", "Note"]));
            nodes.insert(
                12,
                Node::Const {
                    value: Value::String("boom".into()),
                },
            );
            nodes.insert(
                13,
                Node::Call {
                    function: "equal".into(),
                    args: vec![11, 12],
                },
            );
            let range = SequenceExpr::Generate {
                from: None,
                to: 10,
                item: FIRST_NODE + 101,
            };
            let fields = vec![
                ("V1", builder.walk_at(FIRST_NODE, "Primary", &["Dir"], 3)),
                ("V2", builder.item_at(FIRST_NODE + 1, range, 4)),
            ];
            // The generated integer keeps its runtime tag in an integer field.
            builder.project.target = SchemaNode::group(
                "Out",
                vec![string("V1"), SchemaNode::scalar("V2", ScalarType::Int)],
            );
            builder.project.root = Scope {
                bindings: bindings(&fields),
                ..Scope::default()
            };
            builder.project.failure_rules = vec![FailureRule {
                iteration: FailureIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: owner(LOOKUP),
                        path: path(&["Dir"]),
                    },
                },
                selection: FailureSelection::WhenTrue { predicate: 13 },
                message: Some(11),
            }];
            return builder.project;
        }
        "R09-adjacency-tree-root" => {
            // An owned tree's parent-context root is an owned-walk ItemAt over
            // another endpoint; the rows come only from Primary.
            let root = builder.walk_at(FIRST_NODE, LOOKUP, &["Dir"], 3);
            let plan = OwnedAdjacencyTreePlan::new(
                OwnedSourceRef {
                    owner: owner("Primary"),
                    path: path(&["Rows"]),
                },
                path(&["Key"]),
                path(&["Parent"]),
                "name".into(),
                "children".into(),
                Some(root),
            )
            .expect("authored owned adjacency plan");
            builder.project.target = SchemaNode::group(
                "Out",
                vec![SchemaNode::group(
                    "Tree",
                    vec![
                        string("name"),
                        SchemaNode::recursive_group("children", "Tree").repeating(),
                    ],
                )],
            );
            builder.project.root = Scope {
                children: vec![Scope {
                    target_field: "Tree".into(),
                    construction: mapping::ScopeConstruction::OwnedAdjacencyTree { plan },
                    ..Scope::default()
                }],
                ..Scope::default()
            };
            return builder.project;
        }
        _ => panic!("authored profile {profile}"),
    };
    let (target, root) = output(&fields);
    builder.project.target = target;
    builder.project.root = root;
    builder.project
}

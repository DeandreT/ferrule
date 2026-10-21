//! Genuine public Projects for owned recursive filtering (#464).
//!
//! Primary and `Lookup` share one endpoint schema, so every owned collection
//! path also exists under the other owner. `Tree` is a named endpoint whose
//! root is one directory group and `Forest` a named repeated root of
//! directories. Expected results live separately in `cases.rs`.
use super::*;
use mapping::{
    FailureIteration, FailureRule, FailureSelection, NamedSource, NamedTarget,
    OwnedRecursiveCollect, OwnedSourceField, OwnedSourceRef, RecursiveFilterPlan,
    ScopeConstruction, SequenceExpr, SourceOwner,
};

pub const LOOKUP: &str = "Lookup";
pub const TREE: &str = "Tree";
pub const FOREST: &str = "Forest";
pub const AUDIT: &str = "audit";

pub const PROFILES: &[&str] = &[
    "F01-primary-dirs",
    "F02-named-dirs",
    "F03-named-root-tree",
    "F04-named-repeated-root",
    "F05-recursive-positions",
    "F06-nested-batches",
    "F07-legacy-fallback",
    "F08-composed-companions",
    "F09-not-a-bool",
];

/// The predicate of the Primary target's filter.
pub const PREDICATE: u32 = 20;
/// The predicate of the audit target's Lookup filter.
pub const AUDIT_PREDICATE: u32 = 92;

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
fn integer(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}
/// One recursive directory: its name, items with their expectations, children.
pub fn directory_schema() -> SchemaNode {
    SchemaNode::group(
        "Dir",
        vec![
            string("name"),
            SchemaNode::group(
                "file",
                vec![
                    string("name"),
                    integer("pos"),
                    integer("dirpos"),
                    string("dir"),
                    string("batch"),
                ],
            )
            .repeating(),
            SchemaNode::recursive_group("Dir", "Dir").repeating(),
        ],
    )
}
pub fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("Suffix"),
            directory_schema().repeating(),
            // Batch directories are their own `Dir` groups, so the owned
            // collection path ends at a concrete group.
            SchemaNode::group(
                "Batches",
                vec![string("Id"), directory_schema().repeating()],
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
fn position(name: &str, collection: Option<&[&str]>) -> Node {
    Node::OwnedPosition(mapping::OwnedPosition {
        owner: owner(name),
        collection: collection.map(path),
    })
}
fn call(function: &str, args: &[u32]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}
/// A target field `Dir` filtering `collection` of `name` with `predicate`.
pub fn filter(name: &str, collection: &[&str], predicate: u32) -> Scope {
    Scope {
        target_field: "Dir".into(),
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: owner(name),
                path: path(collection),
            },
            output_path: None,
        },
        construction: ScopeConstruction::RecursiveFilter {
            plan: RecursiveFilterPlan::new("Dir".into(), "file".into(), predicate)
                .expect("authored recursive filter plan"),
        },
        ..Scope::default()
    }
}
fn dirs_target(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![directory_schema().repeating()])
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

/// The audit target filters Lookup's directories by Lookup's own suffix in
/// every Project; the Primary target is supplied per profile.
pub fn base() -> Project {
    let mut graph = Graph::default();
    graph.nodes.extend([
        (90, owned_field(LOOKUP, &["Dir", "file", "name"])),
        (91, owned_field(LOOKUP, &["Suffix"])),
        (AUDIT_PREDICATE, call("ends_with", &[90, 91])),
    ]);
    Project {
        source: endpoint_schema("Input"),
        target: dirs_target("Out"),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            named_source(LOOKUP, endpoint_schema(LOOKUP)),
            // Each named root is itself the recursive `Dir` anchor.
            named_source(TREE, directory_schema()),
            named_source(FOREST, directory_schema().repeating()),
        ],
        extra_targets: vec![NamedTarget {
            name: AUDIT.into(),
            path: None,
            schema: dirs_target("Audit"),
            options: Default::default(),
            root: Scope {
                children: vec![filter(LOOKUP, &["Dir"], AUDIT_PREDICATE)],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope::default(),
    }
}

/// `name`'s item names ending with `suffix_owner`'s root suffix.
fn suffix_predicate(project: &mut Project, name: &str, items: &[&str], suffix_owner: &str) {
    let mut name_path = items.to_vec();
    name_path.push("name");
    project.graph.nodes.extend([
        (10, owned_field(name, &name_path)),
        (11, owned_field(suffix_owner, &["Suffix"])),
        (PREDICATE, call("ends_with", &[10, 11])),
    ]);
}
fn single(project: &mut Project, scope: Scope) {
    project.root = Scope {
        children: vec![scope],
        ..Scope::default()
    };
}

pub fn project(profile: &str) -> Project {
    let mut project = base();
    match profile {
        "F01-primary-dirs" => {
            suffix_predicate(&mut project, "Primary", &["Dir", "file"], "Primary");
            single(&mut project, filter("Primary", &["Dir"], PREDICATE));
        }
        // Lookup items, kept by the Primary endpoint's suffix.
        "F02-named-dirs" => {
            suffix_predicate(&mut project, LOOKUP, &["Dir", "file"], "Primary");
            single(&mut project, filter(LOOKUP, &["Dir"], PREDICATE));
        }
        // An endpoint-root group is one identity selection.
        "F03-named-root-tree" => {
            suffix_predicate(&mut project, TREE, &["file"], "Primary");
            single(&mut project, filter(TREE, &[], PREDICATE));
        }
        "F04-named-repeated-root" => {
            suffix_predicate(&mut project, FOREST, &["file"], "Primary");
            single(&mut project, filter(FOREST, &[], PREDICATE));
        }
        // Items keep their own, their group's and the latest Lookup position,
        // and read their innermost group's name.
        "F05-recursive-positions" => {
            project.graph.nodes.extend([
                (10, position(LOOKUP, Some(&["Dir", "file"]))),
                (11, owned_field(LOOKUP, &["Dir", "file", "pos"])),
                (12, call("equal", &[10, 11])),
                (13, position(LOOKUP, Some(&["Dir"]))),
                (14, owned_field(LOOKUP, &["Dir", "file", "dirpos"])),
                (15, call("equal", &[13, 14])),
                (16, owned_field(LOOKUP, &["Dir", "name"])),
                (17, owned_field(LOOKUP, &["Dir", "file", "dir"])),
                (18, call("equal", &[16, 17])),
                (19, position(LOOKUP, None)),
                (21, call("equal", &[19, 11])),
                (PREDICATE, call("and", &[12, 15, 18, 21])),
            ]);
            single(&mut project, filter(LOOKUP, &["Dir"], PREDICATE));
        }
        // The active Primary batch frame supplies its Id to every level.
        "F06-nested-batches" => {
            project.graph.nodes.extend([
                (10, owned_field("Primary", &["Batches", "Id"])),
                (
                    11,
                    owned_field("Primary", &["Batches", "Dir", "file", "batch"]),
                ),
                (PREDICATE, call("equal", &[10, 11])),
            ]);
            project.target = SchemaNode::group(
                "Out",
                vec![SchemaNode::group("Batch", vec![directory_schema().repeating()]).repeating()],
            );
            single(
                &mut project,
                Scope {
                    target_field: "Batch".into(),
                    iteration: ScopeIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: owner("Primary"),
                            path: path(&["Batches"]),
                        },
                        output_path: None,
                    },
                    children: vec![filter("Primary", &["Batches", "Dir"], PREDICATE)],
                    ..Scope::default()
                },
            );
        }
        // A legacy read keeps its innermost-first fallback inside the owned filter.
        "F07-legacy-fallback" => {
            project.graph.nodes.extend([
                (
                    10,
                    Node::SourceField {
                        path: path(&["name"]),
                        frame: None,
                    },
                ),
                (11, owned_field(LOOKUP, &["Suffix"])),
                (PREDICATE, call("ends_with", &[10, 11])),
            ]);
            single(&mut project, filter(LOOKUP, &["Dir"], PREDICATE));
        }
        // The owned filter beside an owned recursive-walk ItemAt and an owned
        // failure walk, the companions it composes.
        "F08-composed-companions" => {
            suffix_predicate(&mut project, "Primary", &["Dir", "file"], "Primary");
            let constant = |value: &str| Node::Const {
                value: Value::String(value.into()),
            };
            project.graph.nodes.extend([
                (50, constant("")),
                (51, constant("/")),
                (
                    52,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    53,
                    Node::SequenceItemAt {
                        sequence: SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
                            collection: OwnedSourceRef {
                                owner: owner(LOOKUP),
                                path: path(&["Dir"]),
                            },
                            children: path(&["Dir"]),
                            descent_value: path(&["name"]),
                            values: path(&["file"]),
                            value: path(&["name"]),
                            prefix: 50,
                            separator: 51,
                            item: 54,
                        }),
                        index: 52,
                    },
                ),
                (
                    54,
                    Node::SourceField {
                        path: vec![],
                        frame: None,
                    },
                ),
                (55, owned_field(LOOKUP, &["Dir", "name"])),
                (56, constant("boom")),
                (57, call("equal", &[55, 56])),
            ]);
            project.target =
                SchemaNode::group("Out", vec![directory_schema().repeating(), string("First")]);
            project.root = Scope {
                bindings: vec![Binding {
                    target_field: "First".into(),
                    node: 53,
                }],
                children: vec![filter("Primary", &["Dir"], PREDICATE)],
                ..Scope::default()
            };
            project.failure_rules = vec![FailureRule {
                iteration: FailureIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: owner(LOOKUP),
                        path: path(&["Dir"]),
                    },
                },
                selection: FailureSelection::WhenTrue { predicate: 57 },
                message: Some(55),
            }];
        }
        // The predicate yields the item name itself, which is not a Boolean.
        "F09-not-a-bool" => {
            project
                .graph
                .nodes
                .insert(PREDICATE, owned_field("Primary", &["Dir", "file", "name"]));
            single(&mut project, filter("Primary", &["Dir"], PREDICATE));
        }
        _ => panic!("authored profile {profile}"),
    }
    project
}

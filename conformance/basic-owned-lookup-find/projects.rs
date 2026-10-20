//! Genuine public Projects for owned Lookup and CollectionFind (#446).
//!
//! Primary and the named `Lookup` endpoint share one schema, so every owned
//! collection also exists under the other owner; their row codes are disjoint.
//! `Batch` is a repeated named root. Expected results live in `cases.rs`.
use super::*;
use mapping::{
    FailureIteration, FailureRule, FailureSelection, NamedSource, NamedTarget, OwnedCollectionFind,
    OwnedLookup, OwnedPathHierarchyPlan, OwnedPosition, OwnedRecursiveCollect, OwnedSourceField,
    OwnedSourceRef, RecursiveFilterPlan, ScopeConstruction, SequenceExpr, SourceOwner,
};

pub const LOOKUP: &str = "Lookup";
pub const BATCH: &str = "Batch";
pub const AUDIT: &str = "audit";

pub const PROFILES: &[&str] = &[
    "L01-lookup-by-parent-needle",
    "L02-find-with-owned-ordinal",
    "L03-find-with-legacy-item-reads",
    "L04-nested-parent-lines",
    "L05-find-reads-parent-and-root-pins",
    "L06-predicate-policy",
    "L07-repeated-named-root",
    "L08-find-within-owned-groups",
    "L09-failure-walk-beside-search",
    "L10-root-identity-lookup",
    "L11-search-beside-owned-hierarchy",
    "L12-find-within-owned-blocks",
    "L13-search-beside-owned-recursive-walk",
    "L14-find-beside-owned-recursive-filter",
];

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
pub fn source(name: &str, parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: owner(name),
        path: path(parts),
    }
}
pub fn field(name: &str, parts: &[&str], pin: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: source(name, parts),
        frame: pin.map(path),
    })
}
pub fn position(name: &str, parts: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner: owner(name),
        collection: Some(path(parts)),
    })
}
pub fn legacy(parts: &[&str]) -> Node {
    Node::SourceField {
        path: path(parts),
        frame: None,
    }
}
pub fn lookup(name: &str, collection: &[&str], key: &[&str], matches: u32, value: &[&str]) -> Node {
    Node::OwnedLookup(OwnedLookup {
        collection: source(name, collection),
        key: path(key),
        matches,
        value: path(value),
    })
}
pub fn find(name: &str, collection: &[&str], predicate: u32, value: u32) -> Node {
    Node::OwnedCollectionFind(OwnedCollectionFind {
        collection: source(name, collection),
        predicate,
        value,
    })
}
fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}
pub fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("Note"),
            SchemaNode::group(
                "Rows",
                vec![
                    string("Code"),
                    string("Name"),
                    int("Rank"),
                    SchemaNode::group("Lines", vec![string("Sku"), int("Qty")]).repeating(),
                ],
            )
            .repeating(),
            string("Paths").repeating(),
            SchemaNode::group(
                "Dir",
                vec![
                    string("Note"),
                    SchemaNode::group("file", vec![string("Note")]).repeating(),
                    SchemaNode::recursive_group("Dir", "Dir").repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
pub fn batch_schema() -> SchemaNode {
    SchemaNode::group(BATCH, vec![string("Code"), string("Name")]).repeating()
}
fn insert(project: &mut Project, id: u32, node: Node) {
    project.graph.nodes.insert(id, node);
}
fn call(project: &mut Project, id: u32, function: &str, args: &[u32]) {
    insert(
        project,
        id,
        Node::Call {
            function: function.into(),
            args: args.to_vec(),
        },
    );
}
fn bind(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}
/// A Primary `Items` child scope iterating owned Primary rows.
fn items(bindings: Vec<Binding>) -> Scope {
    Scope {
        target_field: "Items".into(),
        iteration: ScopeIteration::OwnedSource {
            source: source("Primary", &["Rows"]),
            output_path: None,
        },
        bindings,
        ..Scope::default()
    }
}

/// Shared nodes: Primary row reads 1-4, Lookup row reads 11-14, root notes
/// 21-24, legacy reads 31-32 and constants 40-49. The Primary target binds
/// `Value` to node 90 and the audit target binds `Value` to node 91; profiles
/// add their nodes from 50.
pub fn base() -> Project {
    let target = SchemaNode::group(
        "Output",
        vec![
            string("Value"),
            SchemaNode::group("Items", vec![string("Code"), string("Value")]).repeating(),
        ],
    );
    let audit = SchemaNode::group("Audit", vec![string("Value")]);
    let mut project = Project {
        source: endpoint_schema("Input"),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: LOOKUP.into(),
                path: "lookup.json".into(),
                schema: endpoint_schema(LOOKUP),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: BATCH.into(),
                path: "batch.json".into(),
                schema: batch_schema(),
                options: Default::default(),
                dynamic_path: None,
            },
        ],
        extra_targets: vec![NamedTarget {
            name: AUDIT.into(),
            path: None,
            schema: audit,
            options: Default::default(),
            root: Scope {
                bindings: vec![bind("Value", 91)],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            bindings: vec![bind("Value", 90)],
            ..Scope::default()
        },
    };
    for (offset, name) in [(0, "Primary"), (10, LOOKUP)] {
        insert(&mut project, offset + 1, field(name, &["Rows", "Code"], None));
        insert(&mut project, offset + 2, field(name, &["Rows", "Name"], None));
        insert(&mut project, offset + 3, field(name, &["Rows", "Rank"], Some(&["Rows"])));
        insert(&mut project, offset + 4, position(name, &["Rows"]));
    }
    insert(&mut project, 21, field("Primary", &["Note"], None));
    insert(&mut project, 22, field(LOOKUP, &["Note"], None));
    insert(&mut project, 23, field("Primary", &["Note"], Some(&[])));
    insert(&mut project, 24, field(LOOKUP, &["Note"], Some(&[])));
    insert(&mut project, 31, legacy(&["Code"]));
    insert(&mut project, 32, legacy(&["Name"]));
    for (id, value) in [
        (40, Value::String(":".into())),
        (41, Value::String("A".into())),
        (42, Value::String("B".into())),
        (43, Value::Bool(true)),
        (44, Value::String("not boolean".into())),
        (45, Value::String("a1".into())),
        (46, Value::String("/".into())),
        (47, Value::Null),
        (48, Value::Int(2)),
        (49, Value::String("X".into())),
    ] {
        insert(&mut project, id, Node::Const { value });
    }
    project
}

pub fn project(profile: &str) -> Project {
    let mut p = base();
    match profile {
        // The Primary note is the needle for both endpoints' row codes.
        "L01-lookup-by-parent-needle" => {
            insert(&mut p, 90, lookup("Primary", &["Rows"], &["Code"], 21, &["Name"]));
            insert(&mut p, 91, lookup(LOOKUP, &["Rows"], &["Code"], 21, &["Name"]));
        }
        "L02-find-with-owned-ordinal" => {
            for (offset, result) in [(0, 90), (10, 91)] {
                call(&mut p, 50 + offset, "equal", &[1 + offset, 21]);
                call(&mut p, 51 + offset, "concat", &[2 + offset, 40, 4 + offset]);
                insert(
                    &mut p,
                    result,
                    find(if offset == 0 { "Primary" } else { LOOKUP }, &["Rows"], 50 + offset, 51 + offset),
                );
            }
        }
        // Legacy item reads resolve against the selected owner item first.
        "L03-find-with-legacy-item-reads" => {
            call(&mut p, 50, "equal", &[31, 21]);
            insert(&mut p, 90, find("Primary", &["Rows"], 50, 32));
            insert(&mut p, 91, find(LOOKUP, &["Rows"], 50, 32));
        }
        // A Primary row parent anchors Primary line walks only.
        "L04-nested-parent-lines" => {
            insert(&mut p, 50, lookup("Primary", &["Rows", "Lines"], &["Sku"], 45, &["Qty"]));
            insert(&mut p, 51, lookup(LOOKUP, &["Rows", "Lines"], &["Sku"], 45, &["Qty"]));
            call(&mut p, 52, "concat", &[50, 46, 51]);
            insert(&mut p, 90, Node::Const { value: Value::String("lines".into()) });
            insert(&mut p, 91, lookup(LOOKUP, &["Rows", "Lines"], &["Sku"], 45, &["Qty"]));
            p.root.children.push(items(vec![bind("Code", 1), bind("Value", 52)]));
        }
        "L05-find-reads-parent-and-root-pins" => {
            call(&mut p, 50, "equal", &[13, 3]);
            call(&mut p, 51, "concat", &[12, 46, 2, 46, 24]);
            insert(&mut p, 52, find(LOOKUP, &["Rows"], 50, 51));
            insert(&mut p, 90, Node::Const { value: Value::String("ranks".into()) });
            // At the root no Primary row is active, so audit compares a constant.
            call(&mut p, 53, "equal", &[13, 48]);
            call(&mut p, 54, "concat", &[12, 46, 24]);
            insert(&mut p, 91, find(LOOKUP, &["Rows"], 53, 54));
            p.root.children.push(items(vec![bind("Code", 1), bind("Value", 52)]));
        }
        // A match returns before later predicates; Null skips; other values fail.
        "L06-predicate-policy" => {
            call(&mut p, 50, "equal", &[1, 42]);
            insert(&mut p, 51, Node::If { condition: 50, then: 43, else_: 47 });
            insert(&mut p, 90, find("Primary", &["Rows"], 51, 2));
            call(&mut p, 52, "equal", &[11, 21]);
            insert(&mut p, 53, Node::If { condition: 52, then: 43, else_: 44 });
            insert(&mut p, 91, find(LOOKUP, &["Rows"], 53, 12));
        }
        "L07-repeated-named-root" => {
            insert(&mut p, 50, field(BATCH, &["Code"], None));
            insert(&mut p, 51, field(BATCH, &["Name"], None));
            insert(&mut p, 52, position(BATCH, &[]));
            call(&mut p, 53, "equal", &[50, 21]);
            call(&mut p, 54, "concat", &[51, 40, 52]);
            insert(&mut p, 90, lookup(BATCH, &[], &["Code"], 21, &["Name"]));
            insert(&mut p, 91, find(BATCH, &[], 53, 54));
        }
        // Inside an owned group, a same-collection walk enumerates its members.
        "L08-find-within-owned-groups" => {
            call(&mut p, 50, "greater_than", &[3, 48]);
            call(&mut p, 51, "concat", &[2, 40, 4]);
            insert(&mut p, 52, find("Primary", &["Rows"], 50, 51));
            insert(&mut p, 90, lookup(LOOKUP, &["Rows"], &["Code"], 21, &["Name"]));
            insert(&mut p, 91, find("Primary", &["Rows"], 50, 51));
            let mut grouped = items(vec![bind("Code", 1), bind("Value", 52)]);
            grouped.group_by = Some(1);
            p.root.children.push(grouped);
        }
        // An owned failure walk precedes every owned search in the targets.
        "L09-failure-walk-beside-search" => {
            call(&mut p, 50, "equal", &[11, 49]);
            insert(&mut p, 90, lookup("Primary", &["Rows"], &["Code"], 21, &["Name"]));
            insert(&mut p, 91, lookup(LOOKUP, &["Rows"], &["Code"], 21, &["Name"]));
            p.failure_rules = vec![FailureRule {
                iteration: FailureIteration::OwnedSource {
                    source: source(LOOKUP, &["Rows"]),
                },
                selection: FailureSelection::WhenTrue { predicate: 50 },
                message: Some(12),
            }];
        }
        // A Group root is one identity item for an explicit empty collection.
        "L10-root-identity-lookup" => {
            insert(&mut p, 90, lookup("Primary", &[], &["Note"], 21, &["Note"]));
            insert(&mut p, 91, lookup(LOOKUP, &[], &["Note"], 21, &["Note"]));
        }
        // The audit target is an owned directory tree of Lookup paths.
        "L11-search-beside-owned-hierarchy" => {
            insert(&mut p, 90, lookup(LOOKUP, &["Rows"], &["Code"], 21, &["Name"]));
            let plan = OwnedPathHierarchyPlan::new(
                source(LOOKUP, &["Paths"]),
                "/".into(),
                "directory".into(),
                "file".into(),
                "name".into(),
            )
            .expect("authored owned plan");
            p.extra_targets[0].schema = SchemaNode::group(
                "Audit",
                vec![
                    SchemaNode::group("file", vec![string("name")]).repeating(),
                    SchemaNode::recursive_group("directory", "Audit").repeating(),
                    string("name"),
                ],
            );
            p.extra_targets[0].root = Scope {
                construction: ScopeConstruction::OwnedPathHierarchy { plan },
                ..Scope::default()
            };
        }
        // Inside an owned block, a same-collection walk enumerates its members.
        "L12-find-within-owned-blocks" => {
            call(&mut p, 50, "greater_or_equal", &[3, 48]);
            call(&mut p, 51, "concat", &[2, 40, 4]);
            insert(&mut p, 52, find("Primary", &["Rows"], 50, 51));
            insert(&mut p, 90, Node::Const { value: Value::String("blocks".into()) });
            insert(&mut p, 91, find("Primary", &["Rows"], 50, 51));
            let mut blocks = items(vec![bind("Code", 1), bind("Value", 52)]);
            blocks.group_into_blocks = Some(48);
            p.root.children.push(blocks);
        }
        // Primary selects the second path of an owned recursive Lookup walk.
        "L13-search-beside-owned-recursive-walk" => {
            insert(&mut p, 60, Node::Const { value: Value::String(String::new()) });
            insert(&mut p, 61, legacy(&[]));
            insert(
                &mut p,
                90,
                Node::SequenceItemAt {
                    sequence: SequenceExpr::OwnedRecursiveCollect(OwnedRecursiveCollect {
                        collection: source(LOOKUP, &["Dir"]),
                        children: path(&["Dir"]),
                        descent_value: path(&["Note"]),
                        values: path(&["file"]),
                        value: path(&["Note"]),
                        prefix: 60,
                        separator: 46,
                        item: 61,
                    }),
                    index: 48,
                },
            );
            insert(&mut p, 91, lookup(LOOKUP, &["Rows"], &["Code"], 21, &["Name"]));
        }
        // The audit target keeps only Lookup files named `k` beside an owned find.
        "L14-find-beside-owned-recursive-filter" => {
            call(&mut p, 50, "equal", &[11, 21]);
            insert(&mut p, 90, find(LOOKUP, &["Rows"], 50, 12));
            insert(&mut p, 51, legacy(&["Note"]));
            insert(&mut p, 52, Node::Const { value: Value::String("k".into()) });
            call(&mut p, 53, "equal", &[51, 52]);
            let directory = endpoint_schema(LOOKUP)
                .child("Dir")
                .cloned()
                .expect("authored directory schema");
            p.extra_targets[0].schema = SchemaNode::group("Audit", vec![directory]);
            p.extra_targets[0].root = Scope {
                children: vec![Scope {
                    target_field: "Dir".into(),
                    iteration: ScopeIteration::OwnedSource {
                        source: source(LOOKUP, &["Dir"]),
                        output_path: None,
                    },
                    construction: ScopeConstruction::RecursiveFilter {
                        plan: RecursiveFilterPlan::new("Dir".into(), "file".into(), 53)
                            .expect("authored recursive filter plan"),
                    },
                    ..Scope::default()
                }],
                ..Scope::default()
            };
        }
        _ => panic!("authored profile {profile}"),
    }
    p
}

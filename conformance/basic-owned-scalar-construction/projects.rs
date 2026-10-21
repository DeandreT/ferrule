//! Genuine public Projects for owned reads in scalar target construction (#550).
//!
//! Primary and every named endpoint share one schema, so each owned path also
//! exists under the other owners and under a colliding legacy named prefix.
//! Every Project except the scalar-primary control has an `audit` named scalar
//! target; the Primary target is supplied per profile. Expected results live
//! separately in `cases.rs`.
use super::*;
use mapping::{
    NamedSource, NamedTarget, OwnedSourceField, OwnedSourceRef, ScopeConstruction, SourceOwner,
};

pub const REF: &str = "Ref";
pub const SNOW: &str = " Ref 雪 ";
pub const LOWER: &str = "ref";
pub const LITERAL: &str = "primary";
pub const UNUSED: &str = "Unused";
pub const AUDIT: &str = "audit";

pub const PROFILES: &[&str] = &[
    "S01-primary-root",
    "S02-named-collision",
    "S03-literal-names",
    "S04-absent-owner",
    "S05-lazy-branch",
    "S06-argument-order",
    "S07-numeric-adaptation",
    "S08-nested-repetitions",
    "S09-scalar-primary",
];

/// The Primary target's scalar expression.
pub const VALUE: u32 = 20;
/// The audit target's scalar expression.
pub const AUDIT_VALUE: u32 = 90;

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
fn scalar(name: &str, ty: ScalarType) -> SchemaNode {
    SchemaNode::scalar(name, ty)
}
fn string(name: &str) -> SchemaNode {
    scalar(name, ScalarType::String)
}
pub fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("Value"),
            scalar("Count", ScalarType::Int),
            scalar("Ratio", ScalarType::Float),
            scalar("Flag", ScalarType::Bool),
            SchemaNode::group("Ref", vec![string("Value")]),
            SchemaNode::group(
                "Rows",
                vec![
                    string("Value"),
                    SchemaNode::group("Items", vec![string("Value")]).repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
pub fn owned(name: &str, parts: &[&str]) -> Node {
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
fn legacy(parts: &[&str]) -> Node {
    Node::SourceField {
        path: path(parts),
        frame: None,
    }
}
fn text(value: &str) -> Node {
    Node::Const {
        value: Value::String(value.into()),
    }
}
fn concat(args: &[u32]) -> Node {
    Node::Call {
        function: "concat".into(),
        args: args.to_vec(),
    }
}
/// A scalar scope populating `field` with `value`.
pub fn scalar_scope(field: &str, value: u32) -> Scope {
    Scope {
        target_field: field.into(),
        construction: ScopeConstruction::Scalar { value },
        ..Scope::default()
    }
}
/// A scalar scope repeated once per item of `name`'s owned `collection`.
fn owned_scalars(field: &str, name: &str, collection: &[&str], value: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: owner(name),
                path: path(collection),
            },
            output_path: None,
        },
        ..scalar_scope(field, value)
    }
}
fn named_source(name: &str) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{}.json", name.trim()),
        schema: endpoint_schema("Static"),
        options: Default::default(),
        dynamic_path: None,
    }
}

/// The audit target reads the named `Ref` endpoint's root value in every
/// Project that declares named sources.
pub fn base() -> Project {
    let mut graph = Graph::default();
    graph.nodes.insert(AUDIT_VALUE, owned(REF, &["Value"]));
    Project {
        source: endpoint_schema("Input"),
        target: string("Out"),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: [REF, SNOW, LOWER, LITERAL, UNUSED]
            .into_iter()
            .map(named_source)
            .collect(),
        extra_targets: vec![NamedTarget {
            name: AUDIT.into(),
            path: None,
            schema: string("Audit"),
            options: Default::default(),
            root: scalar_scope("", AUDIT_VALUE),
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: scalar_scope("", VALUE),
    }
}

pub fn project(profile: &str) -> Project {
    let mut project = base();
    let nodes = &mut project.graph.nodes;
    match profile {
        "S01-primary-root" => {
            nodes.insert(VALUE, owned("Primary", &["Value"]));
        }
        // The named root, the colliding legacy `Ref/Value` prefix, Primary's own
        // `Ref` group and the named endpoint's `Ref` group stay distinct.
        "S02-named-collision" => {
            nodes.extend([
                (10, owned(REF, &["Value"])),
                (11, legacy(&["Ref", "Value"])),
                (12, owned("Primary", &["Ref", "Value"])),
                (13, owned(REF, &["Ref", "Value"])),
                (19, text("|")),
                (VALUE, concat(&[10, 19, 11, 19, 12, 19, 13])),
            ]);
        }
        // Owner names are exact bytes: no trimming, case folding or alias.
        "S03-literal-names" => {
            nodes.extend([
                (10, owned(SNOW, &["Value"])),
                (11, owned(LOWER, &["Value"])),
                (12, owned(LITERAL, &["Value"])),
                (19, text("|")),
                (VALUE, concat(&[10, 19, 11, 19, 12])),
            ]);
        }
        "S04-absent-owner" => {
            nodes.insert(VALUE, owned(UNUSED, &["Value"]));
        }
        // Only the selected branch is evaluated.
        "S05-lazy-branch" => {
            nodes.extend([
                (10, owned("Primary", &["Flag"])),
                (11, owned("Primary", &["Value"])),
                (12, owned(UNUSED, &["Value"])),
                (
                    VALUE,
                    Node::If {
                        condition: 10,
                        then: 11,
                        else_: 12,
                    },
                ),
            ]);
        }
        // Arguments evaluate left to right; the first failure wins.
        "S06-argument-order" => {
            nodes.extend([
                (10, owned(UNUSED, &["Value"])),
                (11, owned("Primary", &["Ref", "Value"])),
                (VALUE, concat(&[10, 11])),
            ]);
        }
        // Scalar target domains adapt exact numeric values only.
        "S07-numeric-adaptation" => {
            nodes.insert(VALUE, owned("Primary", &["Count"]));
            nodes.insert(AUDIT_VALUE, owned(REF, &["Ratio"]));
            project.target = scalar("Out", ScalarType::Float);
            project.extra_targets[0].schema = scalar("Audit", ScalarType::Int);
        }
        // Nested scalar targets inside and beside owned and legacy repetitions.
        "S08-nested-repetitions" => {
            nodes.extend([
                (30, owned("Primary", &["Rows", "Value"])),
                (31, owned("Primary", &["Rows", "Items", "Value"])),
                (32, position("Primary", Some(&["Rows"]))),
                (33, position("Primary", None)),
                (34, owned(REF, &["Rows", "Value"])),
                (35, position(REF, None)),
                (36, owned("Primary", &["Value"])),
                (37, legacy(&["Value"])),
                (40, text("#")),
                (41, text(":")),
                (42, text(".")),
                (43, text("@")),
                (44, text("/")),
                (45, text("~")),
                (50, concat(&[30, 40, 32])),
                (51, concat(&[30, 41, 31, 41, 32, 42, 33])),
                (52, concat(&[34, 43, 35, 44, 36])),
                (53, concat(&[37, 45, 36])),
            ]);
            project.target = SchemaNode::group(
                "Out",
                vec![
                    SchemaNode::group(
                        "Item",
                        vec![
                            string("Label"),
                            string("Entry").repeating(),
                            string("Plain"),
                        ],
                    )
                    .repeating(),
                    string("Named").repeating(),
                    string("Legacy").repeating(),
                ],
            );
            project.root = Scope {
                children: vec![
                    Scope {
                        target_field: "Item".into(),
                        iteration: ScopeIteration::OwnedSource {
                            source: OwnedSourceRef {
                                owner: owner("Primary"),
                                path: path(&["Rows"]),
                            },
                            output_path: None,
                        },
                        children: vec![
                            scalar_scope("Label", 50),
                            owned_scalars("Entry", "Primary", &["Rows", "Items"], 51),
                            // An ordinary read inherits the owned row's frame.
                            scalar_scope("Plain", 37),
                        ],
                        ..Scope::default()
                    },
                    owned_scalars("Named", REF, &["Rows"], 52),
                    Scope {
                        iteration: ScopeIteration::Source(path(&["Rows"])),
                        ..scalar_scope("Legacy", 53)
                    },
                ],
                ..Scope::default()
            };
        }
        // A scalar Primary endpoint read at `[]` without named declarations.
        "S09-scalar-primary" => {
            nodes.clear();
            nodes.insert(VALUE, owned("Primary", &[]));
            project.source = string("Input");
            project.extra_sources.clear();
            project.extra_targets.clear();
        }
        _ => panic!("authored profile {profile}"),
    }
    project
}

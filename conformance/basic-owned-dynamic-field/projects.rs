//! Genuine public Projects for owned dynamic source-field reads (#467).
//!
//! Primary and `Lookup` share one endpoint schema, so every owned open object
//! and property name also exists under the other owner. `Settings` is a named
//! endpoint whose root is itself an open object, and `Tree` holds recursive
//! directories for the composed recursive filter. Expected results live
//! separately in `cases.rs`.
use super::*;
use mapping::{
    NamedSource, NamedTarget, OwnedDynamicSourceField, OwnedLookup, OwnedSourceField,
    OwnedSourceRef, RecursiveFilterPlan, ScopeConstruction, SourceOwner,
};

pub const LOOKUP: &str = "Lookup";
pub const SETTINGS: &str = "Settings";
pub const TREE: &str = "Tree";
pub const AUDIT: &str = "audit";

pub const PROFILES: &[&str] = &[
    "D01-primary-object",
    "D02-named-object",
    "D03-open-roots-and-source-keys",
    "D04-primary-rows",
    "D05-named-rows",
    "D06-foreign-row-frames",
    "D07-lazy-keys",
    "D08-grouped-members",
    "D09-recursive-filter-predicate",
    "D10-lookup-needle",
];

/// The owned read bound to the audit target's `First` field: Lookup `size`.
pub const AUDIT_NODE: u32 = 90;
/// The owned read bound to the first Primary output field.
pub const FIRST_NODE: u32 = 101;
/// Key constants: `color`, `size`, the integer 7, `blank`, `nested`, `Id` and
/// `mode`; 8 raises; 9 and 10 are the `skip` and `taken` fallbacks.
pub const COLOR: u32 = 1;
pub const SIZE: u32 = 2;
pub const INT: u32 = 3;
pub const BLANK: u32 = 4;
pub const NESTED: u32 = 5;
pub const ID: u32 = 6;
pub const MODE: u32 = 7;
pub const RAISE: u32 = 8;
/// Owned source keys and conditions: Primary `Key`, Lookup `Key`, Primary
/// `Flag` and the current Primary row's `Pick`.
pub const PRIMARY_KEY: u32 = 20;
pub const LOOKUP_KEY: u32 = 21;
pub const PRIMARY_FLAG: u32 = 22;
pub const ROW_PICK: u32 = 23;
/// The owned Settings `mode` read inside the composed recursive filter.
pub const FILTER_READ: u32 = 131;
/// The owned Settings `pick` read used as both owned lookups' needle.
pub const NEEDLE_READ: u32 = 140;

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
/// An open object with one declared `Id` and string runtime-named properties.
pub fn open(name: &str, declared: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, declared)
        .with_dynamic_fields(string("*"))
        .expect("open object schema")
}
pub fn endpoint_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            string("Key"),
            SchemaNode::scalar("Flag", ScalarType::Bool),
            open("Props", vec![string("Id")]),
            SchemaNode::group(
                "Rows",
                vec![string("Id"), string("Pick"), open("Props", vec![])],
            )
            .repeating(),
        ],
    )
}
/// One recursive directory: its name, leaf files and child directories.
pub fn directory_schema() -> SchemaNode {
    SchemaNode::group(
        "Dir",
        vec![
            string("name"),
            SchemaNode::group("file", vec![string("name")]).repeating(),
            SchemaNode::recursive_group("Dir", "Dir").repeating(),
        ],
    )
}
pub fn dynamic(name: &str, object: &[&str], frame: Option<&[&str]>, key: u32) -> Node {
    Node::OwnedDynamicSourceField(OwnedDynamicSourceField {
        object: OwnedSourceRef {
            owner: owner(name),
            path: path(object),
        },
        frame: frame.map(path),
        key,
    })
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
fn conditional(condition: u32, then: u32, else_: u32) -> Node {
    Node::If {
        condition,
        then,
        else_,
    }
}
fn bindings(count: usize) -> Vec<Binding> {
    (0..count)
        .map(|index| Binding {
            target_field: format!("V{}", index + 1),
            node: FIRST_NODE + index as u32,
        })
        .collect()
}
fn fields(count: usize) -> Vec<SchemaNode> {
    (1..=count)
        .map(|index| string(&format!("V{index}")))
        .collect()
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

/// The audit target reads Lookup's `size` property in every Project.
pub fn base() -> Project {
    let mut graph = Graph::default();
    for (id, value) in [
        (COLOR, Value::String("color".into())),
        (SIZE, Value::String("size".into())),
        (INT, Value::Int(7)),
        (BLANK, Value::String("blank".into())),
        (NESTED, Value::String("nested".into())),
        (ID, Value::String("Id".into())),
        (MODE, Value::String("mode".into())),
        (9, Value::String("skip".into())),
        (10, Value::String("taken".into())),
    ] {
        graph.nodes.insert(id, Node::Const { value });
    }
    graph.nodes.insert(RAISE, Node::Raise { message: None });
    graph
        .nodes
        .insert(PRIMARY_KEY, owned_field("Primary", &["Key"]));
    graph
        .nodes
        .insert(LOOKUP_KEY, owned_field(LOOKUP, &["Key"]));
    graph
        .nodes
        .insert(PRIMARY_FLAG, owned_field("Primary", &["Flag"]));
    graph
        .nodes
        .insert(ROW_PICK, owned_field("Primary", &["Rows", "Pick"]));
    graph
        .nodes
        .insert(AUDIT_NODE, dynamic(LOOKUP, &["Props"], None, SIZE));
    Project {
        source: endpoint_schema("Input"),
        target: SchemaNode::group("Out", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            named_source(LOOKUP, endpoint_schema(LOOKUP)),
            named_source(SETTINGS, open(SETTINGS, vec![])),
            named_source(
                TREE,
                SchemaNode::group(TREE, vec![directory_schema().repeating()]),
            ),
        ],
        extra_targets: vec![NamedTarget {
            name: AUDIT.into(),
            path: None,
            schema: SchemaNode::group("Audit", vec![string("First")]),
            options: Default::default(),
            root: Scope {
                bindings: vec![Binding {
                    target_field: "First".into(),
                    node: AUDIT_NODE,
                }],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope::default(),
    }
}

/// Bind `values` to the root fields `V1..`.
fn flat(mut project: Project, values: Vec<Node>) -> Project {
    let count = values.len();
    for (index, value) in values.into_iter().enumerate() {
        project.graph.nodes.insert(FIRST_NODE + index as u32, value);
    }
    project.target = SchemaNode::group("Out", fields(count));
    project.root.bindings = bindings(count);
    project
}
/// Bind `values` to `V1..` of one item per row of `name`'s `Rows`.
fn rows(mut project: Project, field: &str, name: &str, values: Vec<Node>) -> Project {
    project = flat(project, values);
    let fields = match &project.target.kind {
        ir::SchemaKind::Group { children, .. } => children.clone(),
        _ => unreachable!("authored flat target"),
    };
    project.target = SchemaNode::group("Out", vec![SchemaNode::group(field, fields).repeating()]);
    project.root = Scope {
        children: vec![Scope {
            target_field: field.into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: owner(name),
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            bindings: std::mem::take(&mut project.root.bindings),
            ..Scope::default()
        }],
        ..Scope::default()
    };
    project
}

pub fn project(profile: &str) -> Project {
    let project = base();
    match profile {
        // Every result kind from one Primary object.
        "D01-primary-object" => flat(
            project,
            [COLOR, SIZE, BLANK, NESTED, INT, ID]
                .into_iter()
                .map(|key| dynamic("Primary", &["Props"], None, key))
                .collect(),
        ),
        // The same object and key names under Lookup, including a root pin.
        "D02-named-object" => flat(
            project,
            vec![
                dynamic(LOOKUP, &["Props"], None, COLOR),
                dynamic(LOOKUP, &["Props"], None, SIZE),
                dynamic(LOOKUP, &["Props"], Some(&[]), ID),
                dynamic("Primary", &["Props"], None, SIZE),
            ],
        ),
        // An open endpoint root, root pins and keys read from either owner.
        "D03-open-roots-and-source-keys" => flat(
            project,
            vec![
                dynamic(SETTINGS, &[], None, MODE),
                dynamic(SETTINGS, &[], Some(&[]), PRIMARY_KEY),
                dynamic(LOOKUP, &["Props"], None, LOOKUP_KEY),
                dynamic("Primary", &["Props"], None, LOOKUP_KEY),
                dynamic(LOOKUP, &["Props"], None, PRIMARY_KEY),
            ],
        ),
        // Primary row objects, the Primary root object and Lookup's root
        // object while Primary rows are active.
        "D04-primary-rows" => rows(
            project,
            "Items",
            "Primary",
            vec![
                dynamic("Primary", &["Rows", "Props"], None, COLOR),
                dynamic("Primary", &["Rows", "Props"], Some(&["Rows"]), SIZE),
                dynamic("Primary", &["Props"], None, COLOR),
                dynamic(LOOKUP, &["Props"], None, SIZE),
                dynamic("Primary", &["Rows", "Props"], Some(&["Rows"]), ROW_PICK),
            ],
        ),
        // Lookup row objects while Primary keeps its root.
        "D05-named-rows" => rows(
            project,
            "Items",
            LOOKUP,
            vec![
                dynamic(LOOKUP, &["Rows", "Props"], Some(&["Rows"]), COLOR),
                dynamic("Primary", &["Props"], None, COLOR),
                dynamic(LOOKUP, &["Props"], Some(&[]), SIZE),
            ],
        ),
        // A Primary row never supplies Lookup's row object: an unframed read
        // crosses Lookup's repetition and a pinned read has no active frame.
        "D06-foreign-row-frames" => {
            let mut project = rows(
                project,
                "Items",
                "Primary",
                vec![conditional(PRIMARY_FLAG, 110, 111)],
            );
            let nodes = &mut project.graph.nodes;
            nodes.insert(110, dynamic(LOOKUP, &["Rows", "Props"], None, COLOR));
            nodes.insert(
                111,
                dynamic(LOOKUP, &["Rows", "Props"], Some(&["Rows"]), COLOR),
            );
            project
        }
        // Untaken branches evaluate no key and inspect no owner; a raising key
        // precedes the owner check, and a non-string key reads nothing.
        "D07-lazy-keys" => {
            let mut project = flat(
                project,
                vec![
                    conditional(PRIMARY_FLAG, 120, 9),
                    conditional(PRIMARY_FLAG, 121, 10),
                    dynamic(LOOKUP, &["Props"], None, INT),
                ],
            );
            let nodes = &mut project.graph.nodes;
            nodes.insert(120, dynamic(LOOKUP, &["Props"], None, RAISE));
            nodes.insert(121, dynamic(LOOKUP, &["Props"], None, SIZE));
            project
        }
        // A grouped member container exposes its first member's row object.
        "D08-grouped-members" => {
            let mut project = rows(
                project,
                "Groups",
                "Primary",
                vec![
                    dynamic("Primary", &["Rows", "Props"], None, COLOR),
                    dynamic("Primary", &["Rows", "Props"], Some(&["Rows"]), SIZE),
                    owned_field("Primary", &["Rows", "Pick"]),
                ],
            );
            project.root.children[0].group_by = Some(ROW_PICK);
            project
        }
        // The recursive-filter companion composes: an owned filter over Tree
        // keeps each file only while Settings has a `mode` property.
        "D09-recursive-filter-predicate" => {
            let mut project = flat(project, vec![dynamic("Primary", &["Props"], None, COLOR)]);
            let fields = match &project.target.kind {
                ir::SchemaKind::Group { children, .. } => children.clone(),
                _ => unreachable!("authored flat target"),
            };
            project.target = SchemaNode::group(
                "Out",
                fields
                    .into_iter()
                    .chain([directory_schema().repeating()])
                    .collect(),
            );
            let nodes = &mut project.graph.nodes;
            nodes.insert(
                130,
                Node::Call {
                    function: "exists".into(),
                    args: vec![FILTER_READ],
                },
            );
            nodes.insert(FILTER_READ, dynamic(SETTINGS, &[], None, MODE));
            project.root.children.push(Scope {
                target_field: "Dir".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: owner(TREE),
                        path: path(&["Dir"]),
                    },
                    output_path: None,
                },
                construction: ScopeConstruction::RecursiveFilter {
                    plan: RecursiveFilterPlan::new("Dir".into(), "file".into(), 130)
                        .expect("authored recursive filter plan"),
                },
                ..Scope::default()
            });
            project
        }
        // The lookup/find companion composes: owned lookups over each owner's
        // rows use an owned Settings property as their parent-context needle.
        "D10-lookup-needle" => {
            let lookup = |name: &str| {
                Node::OwnedLookup(OwnedLookup {
                    collection: OwnedSourceRef {
                        owner: owner(name),
                        path: path(&["Rows"]),
                    },
                    key: path(&["Pick"]),
                    matches: NEEDLE_READ,
                    value: path(&["Id"]),
                })
            };
            let mut project = flat(project, vec![lookup(LOOKUP), lookup("Primary")]);
            let nodes = &mut project.graph.nodes;
            nodes.insert(NEEDLE_READ, dynamic(SETTINGS, &[], None, 141));
            nodes.insert(
                141,
                Node::Const {
                    value: Value::String("pick".into()),
                },
            );
            project
        }
        _ => panic!("authored profile {profile}"),
    }
}

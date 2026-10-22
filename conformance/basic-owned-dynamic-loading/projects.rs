//! Genuine public Projects for owned dynamic driver loading (#475).
//!
//! Primary and the static `Catalog` endpoint share one driver shape,
//! `Files/{Path, Label}`. `Docs` loads one document per row of its exact
//! driver collection, and `Parts` loads one document per `Parts` row of the
//! current `Docs` document. Expected results live separately in `cases.rs`.
use super::*;
use mapping::{
    DynamicSourcePath, NamedSource, NamedTarget, OwnedPosition, OwnedSourceField, OwnedSourceRef,
    SequenceWindow, SourceOwner,
};

pub const CATALOG: &str = "Catalog";
pub const DOCS: &str = "Docs";
pub const PARTS: &str = "Parts";
pub const AUDIT: &str = "audit";

pub const PROFILES: &[&str] = &[
    "L01-primary-driver",
    "L02-named-driver",
    "L03-filter-sort",
    "L04-window",
    "L05-audit-loads",
    "L06-outside-read",
];

/// The Docs path read in each driver row; the Parts path read in each Docs
/// document.
pub const DOC_PATH: u32 = 1;
pub const PART_PATH: u32 = 2;
pub const TITLE: u32 = 10;
pub const LABEL: u32 = 11;
pub const DRIVER_POSITION: u32 = 12;
pub const CODE: u32 = 13;
pub const LINE_POSITION: u32 = 14;
pub const PART_NAME: u32 = 15;
pub const PINNED_TITLE: u32 = 16;
pub const AUDIT_LABEL: u32 = 20;
const CONST_A: u32 = 30;
const NOT_A: u32 = 31;
const ONE: u32 = 32;

pub fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
pub fn owner(name: &str) -> SourceOwner {
    match name {
        "Primary" => SourceOwner::Primary,
        name => SourceOwner::Named { name: name.into() },
    }
}
fn source(owner_name: &str, parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: owner(owner_name),
        path: path(parts),
    }
}
fn field(owner_name: &str, parts: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: source(owner_name, parts),
        frame: frame.map(path),
    })
}
fn position(owner_name: &str, collection: &[&str]) -> Node {
    Node::OwnedPosition(OwnedPosition {
        owner: owner(owner_name),
        collection: Some(path(collection)),
    })
}
fn binding(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}
fn owned_scope(target_field: &str, owner_name: &str, parts: &[&str]) -> Scope {
    Scope {
        target_field: target_field.into(),
        iteration: ScopeIteration::OwnedSource {
            source: source(owner_name, parts),
            output_path: None,
        },
        ..Scope::default()
    }
}

pub fn batch_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::group(
                "Files",
                vec![
                    SchemaNode::scalar("Path", ScalarType::String),
                    SchemaNode::scalar("Label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}
fn doc_schema() -> SchemaNode {
    SchemaNode::group(
        "Doc",
        vec![
            SchemaNode::scalar("Title", ScalarType::String),
            SchemaNode::group(
                "Lines",
                vec![SchemaNode::scalar("Code", ScalarType::String)],
            )
            .repeating(),
            SchemaNode::group(
                "Parts",
                vec![SchemaNode::scalar("Path", ScalarType::String)],
            )
            .repeating(),
        ],
    )
}
/// `title` adds a root `Title` read outside the loading scope.
fn out_schema(title: bool) -> SchemaNode {
    let title = title.then(|| SchemaNode::scalar("Title", ScalarType::String));
    SchemaNode::group(
        "Out",
        title
            .into_iter()
            .chain([SchemaNode::group(
                "Docs",
                vec![
                    SchemaNode::scalar("Title", ScalarType::String),
                    SchemaNode::scalar("Label", ScalarType::String),
                    SchemaNode::scalar("Driver", ScalarType::Int),
                    SchemaNode::group(
                        "Lines",
                        vec![
                            SchemaNode::scalar("Code", ScalarType::String),
                            SchemaNode::scalar("Position", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                    SchemaNode::group(
                        "Parts",
                        vec![
                            SchemaNode::scalar("Name", ScalarType::String),
                            SchemaNode::scalar("Doc", ScalarType::String),
                            SchemaNode::scalar("Label", ScalarType::String),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating()])
            .collect(),
    )
}
fn audit_schema() -> SchemaNode {
    SchemaNode::group(
        "Audit",
        vec![
            SchemaNode::group(
                "Labels",
                vec![SchemaNode::scalar("Label", ScalarType::String)],
            )
            .repeating(),
        ],
    )
}

/// The loading scope: each loaded document under its own driver row, with
/// its lines and its own nested Parts loading.
fn docs_scope() -> Scope {
    Scope {
        bindings: vec![
            binding("Title", TITLE),
            binding("Label", LABEL),
            binding("Driver", DRIVER_POSITION),
        ],
        children: vec![
            Scope {
                bindings: vec![binding("Code", CODE), binding("Position", LINE_POSITION)],
                ..owned_scope("Lines", DOCS, &["Lines"])
            },
            Scope {
                bindings: vec![
                    binding("Name", PART_NAME),
                    binding("Doc", PINNED_TITLE),
                    binding("Label", LABEL),
                ],
                ..owned_scope("Parts", PARTS, &[])
            },
        ],
        ..owned_scope("Docs", DOCS, &[])
    }
}
/// The audit scope never loads: it reads only Primary driver labels.
fn labels_scope() -> Scope {
    Scope {
        bindings: vec![binding("Label", AUDIT_LABEL)],
        ..owned_scope("Labels", "Primary", &["Files"])
    }
}

/// The common Project: Docs is driven by `driver`'s Files rows.
pub fn base(driver: &str) -> Project {
    let nodes = BTreeMap::from([
        (
            DOC_PATH,
            field(driver, &["Files", "Path"], Some(&["Files"])),
        ),
        (PART_PATH, field(DOCS, &["Parts", "Path"], Some(&["Parts"]))),
        (TITLE, field(DOCS, &["Title"], None)),
        (LABEL, field(driver, &["Files", "Label"], Some(&["Files"]))),
        (DRIVER_POSITION, position(driver, &["Files"])),
        (CODE, field(DOCS, &["Lines", "Code"], Some(&["Lines"]))),
        (LINE_POSITION, position(DOCS, &["Lines"])),
        (PART_NAME, field(PARTS, &["Name"], None)),
        (PINNED_TITLE, field(DOCS, &["Title"], Some(&[]))),
        (
            AUDIT_LABEL,
            field("Primary", &["Files", "Label"], Some(&["Files"])),
        ),
    ]);
    Project {
        source: batch_schema("Batch"),
        target: out_schema(false),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: CATALOG.into(),
                path: "catalog.json".into(),
                schema: batch_schema(CATALOG),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: DOCS.into(),
                path: String::new(),
                schema: doc_schema(),
                options: Default::default(),
                dynamic_path: Some(DynamicSourcePath::owned(
                    DOC_PATH,
                    source(driver, &["Files"]),
                )),
            },
            NamedSource {
                name: PARTS.into(),
                path: String::new(),
                schema: SchemaNode::group(
                    "Part",
                    vec![SchemaNode::scalar("Name", ScalarType::String)],
                ),
                options: Default::default(),
                dynamic_path: Some(DynamicSourcePath::owned(
                    PART_PATH,
                    source(DOCS, &["Parts"]),
                )),
            },
        ],
        extra_targets: vec![NamedTarget {
            name: AUDIT.into(),
            path: None,
            schema: audit_schema(),
            options: Default::default(),
            root: Scope {
                children: vec![labels_scope()],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![docs_scope()],
            ..Scope::default()
        },
    }
}

pub fn project(profile: &str) -> Project {
    match profile {
        "L01-primary-driver" => base("Primary"),
        "L02-named-driver" => base(CATALOG),
        "L03-filter-sort" => {
            // The filter and sort run over loaded documents; the driver
            // position then reports the compacted output position.
            let mut project = base("Primary");
            let nodes = &mut project.graph.nodes;
            nodes.insert(
                CONST_A,
                Node::Const {
                    value: Value::String("A".into()),
                },
            );
            nodes.insert(
                NOT_A,
                Node::Call {
                    function: "not_equal".into(),
                    args: vec![TITLE, CONST_A],
                },
            );
            let docs = &mut project.root.children[0];
            docs.filter = Some(NOT_A);
            docs.sort_by = Some(LABEL);
            docs.sort_descending = true;
            project
        }
        "L04-window" => {
            let mut project = base("Primary");
            project.graph.nodes.insert(
                ONE,
                Node::Const {
                    value: Value::Int(1),
                },
            );
            project.root.children[0].windows = vec![SequenceWindow::SkipFirst { count: ONE }];
            project
        }
        "L05-audit-loads" => {
            // Only the named target loads; the Primary target never requests.
            let mut project = base("Primary");
            std::mem::swap(&mut project.target, &mut project.extra_targets[0].schema);
            std::mem::swap(&mut project.root, &mut project.extra_targets[0].root);
            project
        }
        "L06-outside-read" => {
            // A loaded owner outside its loading scope has no document.
            let mut project = base("Primary");
            project.target = out_schema(true);
            project.root.bindings.push(binding("Title", TITLE));
            project
        }
        _ => panic!("authored profile"),
    }
}

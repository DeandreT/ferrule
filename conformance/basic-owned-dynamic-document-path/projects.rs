//! Genuine public Projects for owned document paths of dynamically loaded
//! documents (#994). Authored source only: no mapping, load or path is
//! evaluated here.
//!
//! `Docs` loads one document per `Files` row of its driver, and `Parts` loads
//! one document per `Parts` row of the current `Docs` document. Each loaded
//! document reports the logical path that loaded it; a document-set member of
//! the primary endpoint reports its location.
use super::*;
use mapping::{
    DynamicSourcePath, NamedSource, OwnedSourceDocumentPath, OwnedSourceField, OwnedSourceRef,
    SourceOwner,
};

pub const CATALOG: &str = "Catalog";
pub const DOCS: &str = "Docs";
pub const PARTS: &str = "Parts";

/// Model order is the `model` index the generated hosts receive.
pub const MODELS: [&str; 7] = [
    "nested",
    "named_driver",
    "sorted",
    "outside",
    "unloaded_parts",
    "primary_path",
    "members",
];

const DOC_PATH: u32 = 1;
const PART_PATH: u32 = 2;
const DOCS_LOCATION: u32 = 3;
const PARTS_LOCATION: u32 = 4;
const PRIMARY_LOCATION: u32 = 5;
const TITLE: u32 = 6;
const LABEL: u32 = 7;
const PART_NAME: u32 = 8;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn owner(name: &str) -> SourceOwner {
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
fn location(owner_name: &str) -> Node {
    Node::OwnedSourceDocumentPath(OwnedSourceDocumentPath {
        owner: owner(owner_name),
    })
}
fn bind(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}
fn scope(target_field: &str, owner_name: &str, parts: &[&str]) -> Scope {
    Scope {
        target_field: target_field.into(),
        iteration: ScopeIteration::OwnedSource {
            source: source(owner_name, parts),
            output_path: None,
        },
        ..Scope::default()
    }
}
fn repeating(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, children).repeating()
}

fn batch_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![repeating("Files", vec![string("Path"), string("Label")])],
    )
}
fn doc_schema() -> SchemaNode {
    SchemaNode::group(
        "Doc",
        vec![string("Title"), repeating("Parts", vec![string("Path")])],
    )
}

/// Docs with each document's path, title and driver label, and inside it the
/// Parts it loads with their own path, the enclosing document's path and name.
fn nested_target() -> (SchemaNode, Scope) {
    (
        SchemaNode::group(
            "Out",
            vec![repeating(
                "Docs",
                vec![
                    string("Path"),
                    string("Title"),
                    string("Label"),
                    repeating(
                        "Parts",
                        vec![string("Path"), string("DocPath"), string("Name")],
                    ),
                ],
            )],
        ),
        Scope {
            children: vec![Scope {
                bindings: vec![
                    bind("Path", DOCS_LOCATION),
                    bind("Title", TITLE),
                    bind("Label", LABEL),
                ],
                children: vec![Scope {
                    bindings: vec![
                        bind("Path", PARTS_LOCATION),
                        bind("DocPath", DOCS_LOCATION),
                        bind("Name", PART_NAME),
                    ],
                    ..scope("Parts", PARTS, &[])
                }],
                ..scope("Docs", DOCS, &[])
            }],
            ..Scope::default()
        },
    )
}

/// Docs whose bindings read the document path and one more `other` field.
fn read_target(other: &str, node: u32) -> (SchemaNode, Scope) {
    (
        SchemaNode::group(
            "Out",
            vec![repeating("Docs", vec![string("Path"), string(other)])],
        ),
        Scope {
            children: vec![Scope {
                bindings: vec![bind("Path", DOCS_LOCATION), bind(other, node)],
                ..scope("Docs", DOCS, &[])
            }],
            ..Scope::default()
        },
    )
}

/// Each primary document-set member, and inside it the Docs its own rows load.
fn members_target() -> (SchemaNode, Scope) {
    (
        SchemaNode::group(
            "Out",
            vec![repeating(
                "Members",
                vec![
                    string("Path"),
                    repeating(
                        "Docs",
                        vec![string("Path"), string("MemberPath"), string("Title")],
                    ),
                ],
            )],
        ),
        Scope {
            children: vec![Scope {
                bindings: vec![bind("Path", PRIMARY_LOCATION)],
                children: vec![Scope {
                    bindings: vec![
                        bind("Path", DOCS_LOCATION),
                        bind("MemberPath", PRIMARY_LOCATION),
                        bind("Title", TITLE),
                    ],
                    ..scope("Docs", DOCS, &[])
                }],
                ..scope("Members", "Primary", &[])
            }],
            ..Scope::default()
        },
    )
}

pub fn project(model: &str) -> Project {
    let driver = if model == "named_driver" {
        CATALOG
    } else {
        "Primary"
    };
    let (target, root) = match model {
        "nested" | "named_driver" | "sorted" => {
            let (target, mut root) = nested_target();
            if model == "sorted" {
                root.children[0].sort_by = Some(LABEL);
                root.children[0].sort_descending = true;
            }
            (target, root)
        }
        "outside" => (
            SchemaNode::group("Out", vec![string("Path")]),
            Scope {
                bindings: vec![bind("Path", DOCS_LOCATION)],
                ..Scope::default()
            },
        ),
        "unloaded_parts" => read_target("PartsPath", PARTS_LOCATION),
        "primary_path" => read_target("PrimaryPath", PRIMARY_LOCATION),
        "members" => members_target(),
        _ => panic!("authored model"),
    };
    let nodes = BTreeMap::from([
        (
            DOC_PATH,
            field(driver, &["Files", "Path"], Some(&["Files"])),
        ),
        (PART_PATH, field(DOCS, &["Parts", "Path"], Some(&["Parts"]))),
        (DOCS_LOCATION, location(DOCS)),
        (PARTS_LOCATION, location(PARTS)),
        (PRIMARY_LOCATION, location("Primary")),
        (TITLE, field(DOCS, &["Title"], None)),
        (LABEL, field(driver, &["Files", "Label"], Some(&["Files"]))),
        (PART_NAME, field(PARTS, &["Name"], None)),
    ]);
    Project {
        source: batch_schema("Batch"),
        target,
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
                schema: SchemaNode::group("Part", vec![string("Name")]),
                options: Default::default(),
                dynamic_path: Some(DynamicSourcePath::owned(
                    PART_PATH,
                    source(DOCS, &["Parts"]),
                )),
            },
        ],
        extra_targets: vec![],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root,
    }
}

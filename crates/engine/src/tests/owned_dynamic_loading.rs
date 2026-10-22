//! #475 owned dynamic driver loading: an exact driver endpoint and loaded
//! documents that keep their declared owner and identity.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, Graph, NamedSource, Node, NodeId, OwnedPosition, OwnedSourceField,
    OwnedSourceRef, Project, Scope, ScopeIteration, SourceOwner,
};

use crate::{DynamicSourceLoader, ExecutionContext};

pub(crate) fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
pub(crate) fn owner(name: &str) -> SourceOwner {
    match name {
        "Primary" => SourceOwner::Primary,
        name => SourceOwner::Named { name: name.into() },
    }
}
pub(crate) fn source(owner_name: &str, parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: owner(owner_name),
        path: path(parts),
    }
}
pub(crate) fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
pub(crate) fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
pub(crate) fn repeated(items: Vec<Instance>) -> Instance {
    Instance::Repeated(items)
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

/// Primary and Catalog share one driver shape: `Files/{Path, Label}`.
pub(crate) fn batch_schema(name: &str) -> SchemaNode {
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
pub(crate) fn doc_schema() -> SchemaNode {
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
pub(crate) fn part_schema() -> SchemaNode {
    SchemaNode::group("Part", vec![SchemaNode::scalar("Name", ScalarType::String)])
}

pub(crate) const DOC_PATH: NodeId = 1;
pub(crate) const PART_PATH: NodeId = 2;
pub(crate) const TITLE: NodeId = 10;
pub(crate) const LABEL: NodeId = 11;
pub(crate) const DRIVER_POSITION: NodeId = 12;
pub(crate) const CODE: NodeId = 13;
pub(crate) const LINE_POSITION: NodeId = 14;
pub(crate) const PART_NAME: NodeId = 15;
pub(crate) const PINNED_TITLE: NodeId = 16;

fn binding(target_field: &str, node: NodeId) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}

/// `Docs` loads one document per `Files` row of `driver`; `Parts` loads one
/// document per `Parts` row of the current `Docs` document.
pub(crate) fn project(driver: &str) -> Project {
    let nodes = BTreeMap::from([
        (
            DOC_PATH,
            field(driver, &["Files", "Path"], Some(&["Files"])),
        ),
        (
            PART_PATH,
            field("Docs", &["Parts", "Path"], Some(&["Parts"])),
        ),
        (TITLE, field("Docs", &["Title"], None)),
        (LABEL, field(driver, &["Files", "Label"], Some(&["Files"]))),
        (DRIVER_POSITION, position(driver, &["Files"])),
        (CODE, field("Docs", &["Lines", "Code"], Some(&["Lines"]))),
        (LINE_POSITION, position("Docs", &["Lines"])),
        (PART_NAME, field("Parts", &["Name"], None)),
        (PINNED_TITLE, field("Docs", &["Title"], Some(&[]))),
    ]);
    let doc_row = SchemaNode::group(
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
    .repeating();
    Project {
        source: batch_schema("Batch"),
        target: SchemaNode::group("Out", vec![doc_row]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: "Catalog".into(),
                path: "catalog.json".into(),
                schema: batch_schema("Catalog"),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: "Docs".into(),
                path: String::new(),
                schema: doc_schema(),
                options: Default::default(),
                dynamic_path: Some(DynamicSourcePath::owned(
                    DOC_PATH,
                    source(driver, &["Files"]),
                )),
            },
            NamedSource {
                name: "Parts".into(),
                path: String::new(),
                schema: part_schema(),
                options: Default::default(),
                dynamic_path: Some(DynamicSourcePath::owned(
                    PART_PATH,
                    source("Docs", &["Parts"]),
                )),
            },
        ],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Docs".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: source("Docs", &[]),
                    output_path: None,
                },
                bindings: vec![
                    binding("Title", TITLE),
                    binding("Label", LABEL),
                    binding("Driver", DRIVER_POSITION),
                ],
                children: vec![
                    Scope {
                        target_field: "Lines".into(),
                        iteration: ScopeIteration::OwnedSource {
                            source: source("Docs", &["Lines"]),
                            output_path: None,
                        },
                        bindings: vec![binding("Code", CODE), binding("Position", LINE_POSITION)],
                        ..Scope::default()
                    },
                    Scope {
                        target_field: "Parts".into(),
                        iteration: ScopeIteration::OwnedSource {
                            source: source("Parts", &[]),
                            output_path: None,
                        },
                        bindings: vec![
                            binding("Name", PART_NAME),
                            binding("Doc", PINNED_TITLE),
                            binding("Label", LABEL),
                        ],
                        ..Scope::default()
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

pub(crate) fn batch(files: &[(&str, &str)]) -> Instance {
    group(vec![(
        "Files",
        repeated(
            files
                .iter()
                .map(|(path, label)| group(vec![("Path", text(path)), ("Label", text(label))]))
                .collect(),
        ),
    )])
}
pub(crate) fn doc(title: &str, lines: &[&str], parts: &[&str]) -> Instance {
    group(vec![
        ("Title", text(title)),
        (
            "Lines",
            repeated(
                lines
                    .iter()
                    .map(|code| group(vec![("Code", text(code))]))
                    .collect(),
            ),
        ),
        (
            "Parts",
            repeated(
                parts
                    .iter()
                    .map(|path| group(vec![("Path", text(path))]))
                    .collect(),
            ),
        ),
    ])
}
pub(crate) fn part(name: &str) -> Instance {
    group(vec![("Name", text(name))])
}

/// Records every attempted callback separately from physical reads. Like the
/// CLI host, it reuses successful documents by source and path and never
/// caches a failure.
#[derive(Default)]
pub(crate) struct RecordingLoader {
    pub documents: BTreeMap<(String, String), Instance>,
    pub calls: RefCell<Vec<(String, String)>>,
    pub physical: RefCell<Vec<(String, String)>>,
    cache: RefCell<BTreeMap<(String, String), Arc<Instance>>>,
}

impl RecordingLoader {
    pub(crate) fn new(documents: Vec<(&str, &str, Instance)>) -> Self {
        Self {
            documents: documents
                .into_iter()
                .map(|(source, path, document)| ((source.into(), path.into()), document))
                .collect(),
            ..Self::default()
        }
    }
    pub(crate) fn calls(&self) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .map(|(source, path)| format!("{source}:{path}"))
            .collect()
    }
    pub(crate) fn physical(&self) -> Vec<String> {
        self.physical
            .borrow()
            .iter()
            .map(|(source, path)| format!("{source}:{path}"))
            .collect()
    }
}

impl DynamicSourceLoader for RecordingLoader {
    fn load(&self, source: &str, path: &str) -> Result<Arc<Instance>, String> {
        let key = (source.to_owned(), path.to_owned());
        self.calls.borrow_mut().push(key.clone());
        if let Some(document) = self.cache.borrow().get(&key) {
            return Ok(Arc::clone(document));
        }
        self.physical.borrow_mut().push(key.clone());
        let document = Arc::new(
            self.documents
                .get(&key)
                .cloned()
                .ok_or_else(|| format!("no document {source}:{path}"))?,
        );
        self.cache.borrow_mut().insert(key, Arc::clone(&document));
        Ok(document)
    }
}

pub(crate) fn execution(loader: &RecordingLoader) -> ExecutionContext<'_> {
    ExecutionContext::new(Path::new("dynload475.mapping")).with_dynamic_source_loader(loader)
}

fn issues(project: &Project) -> BTreeSet<String> {
    crate::validate(project)
        .into_iter()
        .map(|issue| issue.to_string())
        .collect()
}

#[test]
fn owned_drivers_validate_for_primary_and_named_endpoints() {
    for driver in ["Primary", "Catalog"] {
        assert!(issues(&project(driver)).is_empty(), "{driver}");
    }
}

#[test]
fn owned_drivers_validate_their_exact_endpoint_and_collection() {
    let cases: Vec<(&str, OwnedSourceRef, &str)> = vec![
        (
            "unknown",
            source("Missing", &["Files"]),
            "unknown source owner `Missing`",
        ),
        (
            "missing path",
            source("Catalog", &["Rows"]),
            "owned collection `Rows` matches no path in its owner",
        ),
        (
            "non-repeating",
            source("Catalog", &["Files", "Path"]),
            "owned collection `Files/Path` is not a repeating path in its owner",
        ),
        (
            "self",
            source("Docs", &["Parts"]),
            "an owned dynamic driver cannot select its own endpoint",
        ),
    ];
    for (id, driver, expected) in cases {
        let mut project = project("Primary");
        project.extra_sources[1].dynamic_path = Some(DynamicSourcePath::owned(DOC_PATH, driver));
        let issues = issues(&project);
        assert!(
            issues
                .iter()
                .any(|issue| issue == &format!("extra source `Docs`: {expected}")),
            "{id}: {issues:?}"
        );
    }
    // An owned driver never selects a legacy dynamic source.
    let mut legacy = project("Primary");
    legacy.extra_sources[1].dynamic_path =
        Some(DynamicSourcePath::legacy(DOC_PATH, path(&["Files"])));
    assert!(
        issues(&legacy).contains(
            "extra source `Parts`: an owned dynamic driver cannot select legacy dynamic source `Docs`"
        ),
        "{:?}",
        issues(&legacy)
    );
}

#[test]
fn legacy_aliases_never_reach_an_owned_dynamic_source() {
    let alias = "legacy path `Docs/Lines` names owned dynamic source `Docs`; use an owned selector";
    let mut scope = project("Primary");
    scope.root.children[0].children[0].iteration = ScopeIteration::Source(path(&["Docs", "Lines"]));
    assert!(
        issues(&scope).contains(&format!("scope `Docs/Lines`: {alias}")),
        "{:?}",
        issues(&scope)
    );
    let mut read = project("Primary");
    read.graph.nodes.insert(
        30,
        Node::SourceField {
            path: path(&["Docs", "Lines"]),
            frame: None,
        },
    );
    assert!(
        issues(&read).contains(&format!("node 30: {alias}")),
        "{:?}",
        issues(&read)
    );
    let mut driver = project("Primary");
    driver.extra_sources[2].dynamic_path = Some(DynamicSourcePath::legacy(
        PART_PATH,
        path(&["Docs", "Lines"]),
    ));
    assert!(
        issues(&driver).contains(&format!("extra source `Parts`: {alias}")),
        "{:?}",
        issues(&driver)
    );
}

#[test]
fn owned_dynamic_owners_stay_closed_before_any_callback() {
    let loader = RecordingLoader::new(vec![
        ("Docs", "a.json", doc("A", &["a1"], &["p.json"])),
        ("Parts", "p.json", part("P")),
    ]);
    let result = crate::run_with_sources_and_context(
        &project("Primary"),
        &batch(&[("a.json", "first")]),
        Vec::new(),
        &execution(&loader),
    );
    assert!(
        matches!(
            result,
            Err(crate::EngineError::UnsupportedOwnedContext {
                context: crate::OwnedCapabilityContext::DynamicOwner,
                ..
            })
        ),
        "{result:?}"
    );
    assert_eq!(loader.calls(), Vec::<String>::new());
    assert_eq!(loader.physical(), Vec::<String>::new());
}

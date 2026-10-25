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
fn loading_scopes_never_group_loaded_documents() {
    let mut grouped = project("Primary");
    grouped.root.children[0].group_by = Some(LABEL);
    assert!(
        issues(&grouped)
            .contains("scope `Docs`: an owned dynamic loading scope cannot group `Docs` documents"),
        "{:?}",
        issues(&grouped)
    );
    // A nested iteration of the current document is not a loading scope.
    let mut nested = project("Primary");
    nested.root.children[0].children[0].group_by = Some(CODE);
    assert!(issues(&nested).is_empty(), "{:?}", issues(&nested));
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

fn int(value: i64) -> Instance {
    Instance::Scalar(Value::Int(value))
}
fn line(code: &str, position: i64) -> Instance {
    group(vec![("Code", text(code)), ("Position", int(position))])
}
fn loaded_part(name: &str, doc: &str, label: &str) -> Instance {
    group(vec![
        ("Name", text(name)),
        ("Doc", text(doc)),
        ("Label", text(label)),
    ])
}
fn out_doc(
    title: &str,
    label: &str,
    driver: i64,
    lines: Vec<Instance>,
    parts: Vec<Instance>,
) -> Instance {
    group(vec![
        ("Title", text(title)),
        ("Label", text(label)),
        ("Driver", int(driver)),
        ("Lines", repeated(lines)),
        ("Parts", repeated(parts)),
    ])
}
fn out(docs: Vec<Instance>) -> Instance {
    group(vec![("Docs", repeated(docs))])
}
fn catalog(files: &[(&str, &str)]) -> Vec<(String, Instance)> {
    vec![("Catalog".into(), batch(files))]
}
fn documents() -> RecordingLoader {
    RecordingLoader::new(vec![
        ("Docs", "a.json", doc("A", &["a1", "a2"], &["p1.json"])),
        ("Docs", "b.json", doc("B", &["b1"], &["p1.json", "p2.json"])),
        ("Docs", "c.json", doc("C", &[], &[])),
        ("Parts", "p1.json", part("P1")),
        ("Parts", "p2.json", part("P2")),
    ])
}
fn run(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    loader: &RecordingLoader,
) -> Result<Instance, crate::EngineError> {
    crate::run_with_sources_and_context(project, source, extras, &execution(loader))
}
fn unavailable(name: &str) -> crate::OwnedSourceError {
    crate::OwnedSourceError::SourceUnavailable {
        owner: SourceOwner::Named { name: name.into() },
    }
}

#[test]
fn loaded_documents_keep_their_driver_owner_and_identity_through_nested_loading() {
    let loader = documents();
    let output = run(
        &project("Primary"),
        &batch(&[("a.json", "first"), ("b.json", "second")]),
        catalog(&[("c.json", "catalog")]),
        &loader,
    )
    .unwrap();
    assert_eq!(
        output,
        out(vec![
            out_doc(
                "A",
                "first",
                1,
                vec![line("a1", 1), line("a2", 2)],
                vec![loaded_part("P1", "A", "first")],
            ),
            out_doc(
                "B",
                "second",
                2,
                vec![line("b1", 1)],
                vec![
                    loaded_part("P1", "B", "second"),
                    loaded_part("P2", "B", "second"),
                ],
            ),
        ])
    );
    // Every attempted callback is recorded; a repeated successful path is
    // physically read once by the success-only host cache.
    assert_eq!(
        loader.calls(),
        [
            "Docs:a.json",
            "Docs:b.json",
            "Parts:p1.json",
            "Parts:p1.json",
            "Parts:p2.json"
        ]
    );
    assert_eq!(
        loader.physical(),
        [
            "Docs:a.json",
            "Docs:b.json",
            "Parts:p1.json",
            "Parts:p2.json"
        ]
    );
}

#[test]
fn same_shaped_driver_collections_select_only_the_declared_endpoint() {
    let loader = documents();
    let output = run(
        &project("Catalog"),
        &batch(&[("a.json", "first"), ("b.json", "second")]),
        catalog(&[("c.json", "catalog")]),
        &loader,
    )
    .unwrap();
    assert_eq!(
        output,
        out(vec![out_doc("C", "catalog", 1, Vec::new(), Vec::new())])
    );
    assert_eq!(loader.calls(), ["Docs:c.json"]);

    // An empty selected collection loads nothing; the primary rows of the same
    // shape never repair it.
    let empty = documents();
    let output = run(
        &project("Catalog"),
        &batch(&[("a.json", "first")]),
        vec![("Catalog".into(), group(Vec::new()))],
        &empty,
    )
    .unwrap();
    assert_eq!(output, out(Vec::new()));
    assert_eq!(empty.calls(), Vec::<String>::new());

    // An unsupplied driver endpoint is a typed failure at the loading scope.
    let missing = documents();
    let error = run(
        &project("Catalog"),
        &batch(&[("a.json", "first")]),
        Vec::new(),
        &missing,
    )
    .unwrap_err();
    assert!(
        matches!(&error, crate::EngineError::OwnedSourceAccess { node: None, scope: Some(_), source }
            if *source == unavailable("Catalog")),
        "{error:?}"
    );
    assert_eq!(missing.calls(), Vec::<String>::new());
}

#[test]
fn absent_and_null_paths_skip_their_driver_and_other_values_are_typed_failures() {
    let mut source = batch(&[("a.json", "first")]);
    let Instance::Group(fields) = &mut source else {
        unreachable!()
    };
    let rows = |paths: Vec<Option<Value>>| {
        repeated(
            paths
                .into_iter()
                .enumerate()
                .map(|(index, path)| {
                    let label = text(&format!("row{}", index + 1));
                    match path {
                        Some(path) => {
                            group(vec![("Path", Instance::Scalar(path)), ("Label", label)])
                        }
                        None => group(vec![("Label", label)]),
                    }
                })
                .collect(),
        )
    };
    let mut fields: Vec<(String, Instance)> = fields.iter().cloned().collect();
    fields[0].1 = rows(vec![
        None,
        Some(Value::Null),
        Some(Value::json_null()),
        Some(Value::String("b.json".into())),
    ]);
    let skipping = Instance::Group(fields.clone().into());
    let loader = documents();
    let output = run(&project("Primary"), &skipping, Vec::new(), &loader).unwrap();
    // The driver position stays the raw driver row's position.
    assert_eq!(
        output,
        out(vec![out_doc(
            "B",
            "row4",
            4,
            vec![line("b1", 1)],
            vec![
                loaded_part("P1", "B", "row4"),
                loaded_part("P2", "B", "row4")
            ],
        )])
    );
    assert_eq!(loader.calls()[0], "Docs:b.json");

    fields[0].1 = rows(vec![
        Some(Value::String("a.json".into())),
        Some(Value::Int(7)),
        Some(Value::String("b.json".into())),
    ]);
    let invalid = Instance::Group(fields.into());
    let loader = documents();
    let error = run(&project("Primary"), &invalid, Vec::new(), &loader).unwrap_err();
    assert_eq!(
        error,
        crate::EngineError::DynamicSourcePath {
            source_name: "Docs".into(),
            found: "int",
        }
    );
    // Requests already issued in driver order remain; later drivers never run.
    assert_eq!(loader.calls(), ["Docs:a.json"]);
}

#[test]
fn reached_loading_scopes_require_a_loader_and_host_failures_keep_their_path() {
    let error = crate::run_with_sources_and_context(
        &project("Primary"),
        &batch(&[]),
        Vec::new(),
        &ExecutionContext::new(Path::new("dynload475.mapping")),
    )
    .unwrap_err();
    assert_eq!(
        error,
        crate::EngineError::MissingDynamicSourceLoader {
            source_name: "Docs".into()
        }
    );
    let loader = documents();
    let error = run(
        &project("Primary"),
        &batch(&[("a.json", "first"), ("missing.json", "second")]),
        Vec::new(),
        &loader,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        crate::EngineError::DynamicSourceLoad {
            source_name: "Docs".into(),
            path: "missing.json".into(),
            message: "no document Docs:missing.json".into(),
        }
        .to_string()
    );
    // The failure is attempted after the earlier success and is not cached.
    assert_eq!(loader.calls(), ["Docs:a.json", "Docs:missing.json"]);
    assert_eq!(loader.physical(), ["Docs:a.json", "Docs:missing.json"]);
    let retry = run(
        &project("Primary"),
        &batch(&[("a.json", "first"), ("missing.json", "second")]),
        Vec::new(),
        &loader,
    );
    assert!(retry.is_err());
    assert_eq!(
        loader.physical(),
        ["Docs:a.json", "Docs:missing.json", "Docs:missing.json"]
    );
}

#[test]
fn unused_owned_dynamic_sources_are_never_requested() {
    let mut unused = project("Primary");
    unused.root.children.clear();
    let output = crate::run_with_sources_and_context(
        &unused,
        &batch(&[("a.json", "first")]),
        Vec::new(),
        &ExecutionContext::new(Path::new("dynload475.mapping")),
    )
    .unwrap();
    assert_eq!(output, group(Vec::new()));

    // A named target that loads documents runs only when it is selected.
    let mut targets = project("Primary");
    targets.extra_targets.push(mapping::NamedTarget {
        name: "audit".into(),
        path: None,
        schema: targets.target.clone(),
        options: Default::default(),
        root: std::mem::take(&mut targets.root),
    });
    let loader = documents();
    let selected = crate::run_selected_target_with_sources_and_context(
        &targets,
        &batch(&[("a.json", "first")]),
        Vec::new(),
        &execution(&loader),
        crate::TargetSelection::Primary,
    )
    .unwrap();
    assert!(matches!(selected, crate::SelectedTargetOutput::Primary(_)));
    assert_eq!(loader.calls(), Vec::<String>::new());
    let all = crate::run_outputs_with_sources_and_context(
        &targets,
        &batch(&[("a.json", "first")]),
        Vec::new(),
        &execution(&loader),
    )
    .unwrap();
    assert_eq!(all.extras.len(), 1);
    assert_eq!(loader.calls(), ["Docs:a.json", "Parts:p1.json"]);
}

#[test]
fn owned_dynamic_owners_outside_their_loading_scope_are_unavailable() {
    // A read of a loaded endpoint outside its loading scope never borrows a
    // document, driver row or another endpoint.
    let mut read = project("Primary");
    read.target = SchemaNode::group(
        "Out",
        vec![
            SchemaNode::scalar("Title", ScalarType::String),
            read.target.child("Docs").cloned().unwrap(),
        ],
    );
    read.root.bindings.push(binding("Title", TITLE));
    let loader = documents();
    let error = run(&read, &batch(&[("a.json", "first")]), Vec::new(), &loader).unwrap_err();
    assert!(
        matches!(&error, crate::EngineError::OwnedSourceAccess { node: Some(TITLE), source, .. }
            if *source == unavailable("Docs")),
        "{error:?}"
    );
    // A nested driver needs its own driver's current document.
    let mut nested = project("Primary");
    let parts = nested.root.children[0].children.remove(1);
    nested.root.children[0] = Scope {
        target_field: "Docs".into(),
        ..Scope::default()
    };
    nested.root.children[0].children.push(parts);
    nested.root.children[0].children[0].bindings.truncate(1);
    let loader = documents();
    let error = run(&nested, &batch(&[("a.json", "first")]), Vec::new(), &loader).unwrap_err();
    assert!(
        matches!(&error, crate::EngineError::OwnedSourceAccess { node: None, scope: Some(_), source }
            if *source == unavailable("Docs")),
        "{error:?}"
    );
    assert_eq!(loader.calls(), Vec::<String>::new());
}

#[test]
fn legacy_drivers_keep_their_behavior_beside_owned_drivers() {
    // The legacy fixture keeps its wire, driver walk and requests.
    let project = super::dynamic_source_tests::dynamic_project();
    let wire = serde_json::to_value(&project.extra_sources[0].dynamic_path).unwrap();
    assert_eq!(wire, serde_json::json!({"node":0,"iteration":["File"]}));
}

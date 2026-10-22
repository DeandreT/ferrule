//! #475 owned dynamic driver loading through the filesystem host: exact
//! driver endpoints, nested loading and every attempted request in order.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, DynamicSourcePath, Graph, NamedSource, Node, NodeId, OwnedPosition, OwnedSourceField,
    OwnedSourceRef, Project, Scope, ScopeIteration, SourceOwner,
};
use serde_json::{Value as Json, json};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_owned_dynamic_loading_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("docs")).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, value: Json) {
        std::fs::write(self.0.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Requests(RefCell<Vec<String>>);

impl cli::TraceSink for Requests {
    fn record(&self, _: cli::TraceEvent) {}
    fn dynamic_source_load_requested(&self, source: &str, path: &str) {
        self.0.borrow_mut().push(format!("{source}:{path}"));
    }
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn owner(name: &str) -> SourceOwner {
    match name {
        "Primary" => SourceOwner::Primary,
        name => SourceOwner::Named { name: name.into() },
    }
}
fn owned(name: &str, parts: &[&str]) -> OwnedSourceRef {
    OwnedSourceRef {
        owner: owner(name),
        path: path(parts),
    }
}
fn field(name: &str, parts: &[&str], frame: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: owned(name, parts),
        frame: frame.map(path),
    })
}
fn binding(target_field: &str, node: NodeId) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}
fn batch(name: &str) -> SchemaNode {
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
fn loaded(name: &str, node: NodeId, driver: OwnedSourceRef, schema: SchemaNode) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: String::new(),
        schema,
        options: Default::default(),
        dynamic_path: Some(DynamicSourcePath::owned(node, driver)),
    }
}

/// `Docs` loads per `Files` row of `driver`; `Parts` loads per `Parts` row of
/// the current `Docs` document.
fn project(driver: &str) -> Project {
    let nodes = BTreeMap::from([
        (1, field(driver, &["Files", "Path"], Some(&["Files"]))),
        (2, field("Docs", &["Parts", "Path"], Some(&["Parts"]))),
        (10, field("Docs", &["Title"], None)),
        (11, field(driver, &["Files", "Label"], Some(&["Files"]))),
        (
            12,
            Node::OwnedPosition(OwnedPosition {
                owner: owner(driver),
                collection: Some(path(&["Files"])),
            }),
        ),
        (13, field("Docs", &["Lines", "Code"], Some(&["Lines"]))),
        (15, field("Parts", &["Name"], None)),
        (16, field("Docs", &["Title"], Some(&[]))),
    ]);
    let docs = SchemaNode::group(
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
    );
    let parts = SchemaNode::group("Part", vec![SchemaNode::scalar("Name", ScalarType::String)]);
    let target = SchemaNode::group(
        "Out",
        vec![
            SchemaNode::group(
                "Docs",
                vec![
                    SchemaNode::scalar("Title", ScalarType::String),
                    SchemaNode::scalar("Label", ScalarType::String),
                    SchemaNode::scalar("Driver", ScalarType::Int),
                    SchemaNode::group(
                        "Lines",
                        vec![SchemaNode::scalar("Code", ScalarType::String)],
                    )
                    .repeating(),
                    SchemaNode::group(
                        "Parts",
                        vec![
                            SchemaNode::scalar("Name", ScalarType::String),
                            SchemaNode::scalar("Doc", ScalarType::String),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    Project {
        source: batch("Batch"),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: "Catalog".into(),
                path: "catalog.json".into(),
                schema: batch("Catalog"),
                options: Default::default(),
                dynamic_path: None,
            },
            loaded("Docs", 1, owned(driver, &["Files"]), docs),
            loaded("Parts", 2, owned("Docs", &["Parts"]), parts),
        ],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Docs".into(),
                iteration: ScopeIteration::OwnedSource {
                    source: owned("Docs", &[]),
                    output_path: None,
                },
                bindings: vec![
                    binding("Title", 10),
                    binding("Label", 11),
                    binding("Driver", 12),
                ],
                children: vec![
                    Scope {
                        target_field: "Lines".into(),
                        iteration: ScopeIteration::OwnedSource {
                            source: owned("Docs", &["Lines"]),
                            output_path: None,
                        },
                        bindings: vec![binding("Code", 13)],
                        ..Scope::default()
                    },
                    Scope {
                        target_field: "Parts".into(),
                        iteration: ScopeIteration::OwnedSource {
                            source: owned("Parts", &[]),
                            output_path: None,
                        },
                        bindings: vec![binding("Name", 15), binding("Doc", 16)],
                        ..Scope::default()
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn files(rows: &[(&str, &str)]) -> Json {
    json!({"Files": rows.iter().map(|(path, label)| json!({"Path": path, "Label": label})).collect::<Vec<_>>()})
}

fn fixture() -> TestDir {
    let dir = TestDir::new();
    dir.write(
        "batch.json",
        files(&[("docs/a.json", "first"), ("docs/b.json", "second")]),
    );
    dir.write("catalog.json", files(&[("docs/c.json", "catalog")]));
    dir.write(
        "docs/a.json",
        json!({"Title":"A","Lines":[{"Code":"a1"},{"Code":"a2"}],"Parts":[{"Path":"docs/p1.json"}]}),
    );
    dir.write(
        "docs/b.json",
        json!({"Title":"B","Lines":[{"Code":"b1"}],"Parts":[{"Path":"docs/p1.json"},{"Path":"docs/p2.json"}]}),
    );
    dir.write("docs/c.json", json!({"Title":"C","Lines":[],"Parts":[]}));
    dir.write("docs/p1.json", json!({"Name":"P1"}));
    dir.write("docs/p2.json", json!({"Name":"P2"}));
    dir
}

fn run(project: &Project, dir: &Path, sink: &dyn cli::TraceSink) -> anyhow::Result<Json> {
    let output = dir.join("out.json");
    cli::run_project_value_with_options(
        project,
        &dir.join("project.json"),
        &cli::RunOptions {
            input_path: Some(&dir.join("batch.json")),
            output_path: Some(&output),
            trace_sink: Some(sink),
            ..cli::RunOptions::default()
        },
    )?;
    Ok(serde_json::from_slice(&std::fs::read(output)?)?)
}

#[test]
fn filesystem_runs_load_exactly_the_declared_driver_documents_in_order() {
    let dir = fixture();
    let requests = Requests::default();
    let output = run(&project("Primary"), &dir.0, &requests).unwrap();
    assert_eq!(
        output,
        json!({"Docs":[
            {"Title":"A","Label":"first","Driver":1,"Lines":[{"Code":"a1"},{"Code":"a2"}],
                "Parts":[{"Name":"P1","Doc":"A"}]},
            {"Title":"B","Label":"second","Driver":2,"Lines":[{"Code":"b1"}],
                "Parts":[{"Name":"P1","Doc":"B"},{"Name":"P2","Doc":"B"}]}
        ]})
    );
    // Repeated successful paths are requested again; the host reuses them.
    assert_eq!(
        *requests.0.borrow(),
        [
            "Docs:docs/a.json",
            "Docs:docs/b.json",
            "Parts:docs/p1.json",
            "Parts:docs/p1.json",
            "Parts:docs/p2.json"
        ]
    );

    // The same-shaped named driver supplies only its own rows.
    let requests = Requests::default();
    let output = run(&project("Catalog"), &dir.0, &requests).unwrap();
    assert_eq!(
        output,
        json!({"Docs":[{"Title":"C","Label":"catalog","Driver":1,"Lines":[],"Parts":[]}]})
    );
    assert_eq!(*requests.0.borrow(), ["Docs:docs/c.json"]);
}

#[test]
fn filesystem_failures_keep_the_loaded_endpoint_and_path() {
    let dir = fixture();
    std::fs::remove_file(dir.0.join("docs/p2.json")).unwrap();
    let requests = Requests::default();
    let error = run(&project("Primary"), &dir.0, &requests).unwrap_err();
    let text = format!("{error:#}");
    assert!(
        text.contains("Parts") && text.contains("docs/p2.json"),
        "{text}"
    );
    assert_eq!(
        requests.0.borrow().last().map(String::as_str),
        Some("Parts:docs/p2.json")
    );
    assert!(!dir.0.join("out.json").exists());
}

#[test]
fn json_traces_record_owned_loading_requests() {
    let dir = fixture();
    let trace_path = dir.0.join("trace.jsonl");
    let trace = cli::JsonTraceFile::create(&trace_path).unwrap();
    let output = dir.0.join("out.json");
    let outcome = cli::run_project_value_with_options(
        &project("Primary"),
        &dir.0.join("project.json"),
        &cli::RunOptions {
            input_path: Some(&dir.0.join("batch.json")),
            output_path: Some(&output),
            trace_sink: Some(&trace),
            ..cli::RunOptions::default()
        },
    )
    .unwrap();
    trace.finish(&outcome.artifacts).unwrap();
    let records = std::fs::read_to_string(&trace_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Json>(line).unwrap())
        .filter(|record| record["event"]["kind"] == "dynamic_source_load_requested")
        .map(|record| {
            format!(
                "{}:{}",
                record["event"]["source"].as_str().unwrap(),
                record["event"]["path"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        records,
        [
            "Docs:docs/a.json",
            "Docs:docs/b.json",
            "Parts:docs/p1.json",
            "Parts:docs/p1.json",
            "Parts:docs/p2.json"
        ]
    );
}
